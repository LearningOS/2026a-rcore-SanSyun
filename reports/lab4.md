# lab4 实验报告

## 一、功能总结（200 字以内，不贴代码）
本章在已有 easy-fs 框架上实现文件系统与 I/O：超级块记录各区域大小，inode 位图和数据位图负责分配回收，DiskInode 用 28 个直接块及两级间接块索引数据，目录项保存文件名到 inode 号的映射；OS 侧以 OSInode 封装 inode 和读写偏移，借助 fd 表实现 open/close/read/write 与标准输入输出。编程作业为文件增加 nlink，实现 linkat、unlinkat、fstat：linkat 追加目录项并增加链接数，unlinkat 删除目录项、链接数归零时回收数据块和 inode，fstat 返回 dev、ino、mode、nlink。

## 二、简答题

### 简答1

root inode 是 easy-fs 的根目录 inode，固定为 inode 号 0，代表路径 `/`，是整个文件系统目录树的入口。格式化时 `EasyFileSystem::create` 先调用 `alloc_inode()` 并断言返回 0，再把这个 inode 初始化为 `DiskInodeType::Directory`；`EasyFileSystem::root_inode` 通过 `get_disk_inode_pos(0)` 找到它在 inode 区的位置并构造 `Inode`；OS 启动时 `ROOT_INODE` 这个 lazy_static 打开块设备并长期持有它。之后所有按名字的访问都从根开始：`open_file` 用 `ROOT_INODE.find`/`create` 查找或创建文件，`list_apps` 用 `ROOT_INODE.ls` 列出应用，linkat/unlinkat 也是在根目录的目录项上增删。root inode 的数据块存放 `DirEntry` 数组，每个目录项 32 字节，记录文件名和 inode 号；本实验没有子目录，所有文件都是它的直接目录项，因此它既是根目录，也是唯一的目录。

root inode 没有父目录项指向它，也没有在超级块里单独保存位置，而是由“inode 号 0”这一约定隐式确定；这使它在文件系统中具有唯一性和不可替代性。

如果 root inode 的关键内容损坏，文件系统可能失去入口，按路径的访问轻则失败、重则 panic，具体表现取决于损坏的是哪个字段（`nlink` 在本实验中没有参与根的路径解析，单独损坏影响不大）：

- `type_` 被破坏成非 Directory：`find_inode_id` 开头的 `assert!(disk_inode.is_dir())` 会直接 panic；即使绕过断言，`find`/`create`/`ls` 也会把根目录当普通文件处理。
- `size` 被改错：目录项个数 `size / 32` 随之错误。偏小会让已有文件“消失”；偏大会扫描到尚未写入的目录项区域，`DirEntry::name()` 可能因为找不到 NUL 或非法 UTF-8 而 panic；`create` 追加目录项、`unlink` 搬移目录项也会写到错误位置，破坏目录结构。
- `direct`/`indirect1`/`indirect2` 的块号被改错：一旦目录项需要访问这些块，`get_block_id` 就会指向错误甚至越界的数据块。轻则读出乱码目录项、把名字解析到错误的 inode，重则块设备访问失败或内核 panic；如果错误块号落回 inode 区、位图区，还会进一步破坏 inode 位图或其它 inode。
- inode 0 在 inode 位图中被误释放：后续 `alloc_inode` 可能再次分配 0，新建文件会覆盖 root inode 本身，目录彻底损坏。

easy-fs 没有为 root inode 保存副本，也没有校验和；`EasyFileSystem::open` 只检查超级块的 magic，不会校验根 inode。因此 root inode 损坏通常无法在运行中恢复：系统可能直接 panic，或者表现为 open/create 全部失败、目录内容错乱、文件指向错误数据。如果损坏来自内核写操作并已被 `block_cache_sync_all` 写回磁盘，重启后仍然存在，通常只能重新生成 fs.img（重新格式化）才能恢复；只有尚未同步的内存缓存错误才可能靠重启碰运气恢复。

## 三、荣誉准则
1. 我独立完成了本次实验的所有工作，包括代码与文档。
2. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
