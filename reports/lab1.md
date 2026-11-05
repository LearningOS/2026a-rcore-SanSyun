# lab1 实验报告

## 一、功能总结（200 字以内，不贴代码）
本次实验在ch3框架上实现了sys_trace系统调用（编号 410）。它在TaskControlBlock中新增按任务维护的系统调用计数数组，并在syscall分发入口处先计数、后处理，使本次调用自身也被统计。据此支持三类请求：读取用户地址处的一个字节、向用户地址写入一个字节、查询当前任务对指定系统调用的调用次数；非法请求返回-1。计数随任务切换自然隔离，无需额外全局状态。

## 二、简答题
### 简答1
执行make -C os run TEST=2 BASE=1指令后可看到如下出错行为：
[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003a4, kernel killed it.
bad addr = 0x0是stval（出错地址），bad instruction = 0x804003a4是sepc（出错指令的用户态PC）。地址0不在RustSBI配置的PMP可访问区间内，所以写操作被硬件拦下，产生访存异常。
[kernel] IllegalInstruction in application, kernel killed it.
sret是S态特权指令，U态执行时硬件直接报illegal instruction（scause = 2），根本不会真的去改特权级
[kernel] IllegalInstruction in application, kernel killed it.
csrr sstatus访问的是S态CSR，U态同样无权访问，报的也是illegal instruction。

本次实验使用的固件为 RustSBI 0.4.1

### 简答2
1. sp的值指向内核栈上trapContex的起始地址；场景一：首次进入用户态rust_main到run_first_task到__switch，切到TaskContext::goto_restore(init_app_cx(i))构造的上下文，其ra正是__restore，sp正是TrapContext指针，ret后落在这里；场景二：从trap返回__alltraps保存完现场call trap_handler，函数返回后顺序落到__restore。

2. 它们不是通用寄存器，不在x1–x31的保存布局里，所以SAVE_GP/LOAD_GP覆盖不到；进入时被单独存在32*8（sstatus）、33*8（sepc）、2*8（用户sp，来自sscratch）；三者的意义在于：sstatus的spp位决定sret之后回到哪个特权级，SPIE决定返回后中断是否使能。对于sepc，sret会把他的值放到pc，用户程序返回的地址，让程序接着trap的下一条指令执行。sscratch存放用户栈指针，供下一次trap的csrrw换回内核栈。先恢复 sstatus、sepc，最后才写 sscratch，因为前两条还要靠 sp 相对寻址，sscratch 一旦写回就代表用户栈了。

3. x2是sp。用户sp虽然存在2*8(sp)，但不能直接ld进sp（会毁掉当前基址），所以留到第六十行用csrrw一次性交换；x4是tp（线程指针）。用户程序不使用TLS，__alltraps第21行注释也明说“application does not use it”，进内核时根本没保存，自然不恢复。

4. 第六十行指令代表交换sp和sscracth的值，sp代表从sscratch换过来的用户栈指针，sscratch为内核栈顶，这次对换是为了下一次trap做准备，下次ecall进来csrrw又能把两者换回去。

5. sret。它按sstatus.SPP恢复特权级（__restore此处为U），并把sepc写入pc；执行完这条指令，CPU就已经在U态取指了。

6. sp指向内核栈顶（TrapContext还没分配，紧接着addi sp, sp, -34*8才开栈帧），sscratch指向用户栈指针，其目的是把“用户栈”从sp上摘下来存好，同时让sp安全地指向内核栈，避免用用户栈做内核操作。

7. U态进入S态是在ecall的时候发生的。ecall触发environment call from U-mode异常，硬件跳到stvec指向的__alltraps，不是函数调用。


## 三、荣誉准则
1. 我独立完成了本次实验的所有工作，包括代码与文档。
2. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

