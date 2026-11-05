# lab5 实验报告

## 一、功能总结（200 字以内，不贴代码）
本次实验在 ch8 框架上实现死锁检测：为每个进程维护开关与线程资源记录（各线程持有哪些 mutex/信号量、在等哪个资源），在 mutex_lock 与 semaphore_down 阻塞前按银行家算法用 Available/Allocation/Need 检查安全性，无安全序列则拒绝并返回 -0xDEAD；新增 sys_enable_deadlock_detect（469）按进程开关，并补上测例依赖的 sys_get_time。

## 二、简答题

### 简答1

**（1）主线程退出时需要回收的资源**

主线程（tid==0）退出即整个进程退出，由 `exit_current_and_run_next` 收尾。此时其他线程可能处于 Ready（挂在就绪队列上）或 Blocked（睡在定时器、信号量、互斥锁、条件变量的等待队列上），但都不可能正在运行（单核，当前 CPU 上只有退出者自己）。需要回收的有：

1. **每个线程的用户资源（TaskUserRes）**：tid，以及为该线程分配的用户栈和 TrapContext 两个物理页。主线程自己的 `res` 直接置 `None` 释放；其他线程的 `res` 先 `take()` 到 `recycle_res`，等释放 PCB 的可变借用之后再统一 drop。`Drop for TaskUserRes` 会调用 `dealloc_tid()`，并把用户栈、TrapContext 两个区域从 MemorySet 中移除，区域里的 `data_frames` 随之 drop，`FrameTracker::drop` 把物理页还给帧分配器。这里之所以先收集、后回收，是因为 `dealloc_tid`/`dealloc_user_res` 还要再次访问 PCB inner，不能在持有 `process_inner` 借用时做。

2. **内核栈**：`TaskControlBlock` 的 `KernelStack` drop 时会从 `KERNEL_SPACE` 中删除对应区间并归还 kstack id。但退出者正在自己的内核栈上执行，不能立刻释放，所以主线程的 TCB 被放进 `TASK_MANAGER.stop_task`；等下一次有任务停止、`add_stopping_task` 覆盖它时才真正释放（单核下此时它已不再使用这块内核栈）。其他线程如果在就绪队列上，TCB 的 Arc 归零后内核栈随之释放。

3. **用户地址空间**：`memory_set.recycle_data_pages()` 清空所有 MapArea，代码段、数据段、堆等数据页被归还；页表本身占用的帧记录在 `PageTable.frames` 里，随 PCB 真正析构时释放。

4. **打开的文件**：`fd_table.clear()` 逐个 drop `Arc<dyn File>`，stdin/stdout、管道端点、OSInode 等在这里关闭，相关对象在引用计数归零后释放。

5. **子进程关系**：`process_inner.children` 里的子进程全部改挂到 initproc 名下，然后清空自己的 children 列表，避免子进程的 parent 指向一个已退出的进程。

6. **pid**：`remove_from_pid2process` 把 PCB 从 `PID2PCB` 中删除；`PidHandle` 的 Drop 在 PCB 析构时把 pid 还给分配器。

7. **定时器**：`remove_inactive_task` 对每个线程调用 `remove_task` 与 `remove_timer`，把还在 `TIMERS` 里的唤醒项删掉。

8. **同步资源**：`mutex_list`、`semaphore_list`、`condvar_list` 及其等待队列，随 PCB 一起 drop 即可（本框架中这些资源是进程私有的）。

9. **进程控制块本身**：PCB 被标记为 zombie 并保存 exit_code，等父进程 `waitpid` 取走退出码、把它从 `children` 中移除后 Arc 归零，PCB（含页表、同步资源列表等）才真正释放；在此之前只回收用户资源，进程处于"僵尸"状态。

**（2）其他线程的 TaskControlBlock 可能在哪些位置被引用，分别是否需要回收，为什么**

| 引用位置 | 是否需要回收 | 原因 |
|---|---|---|
| `ProcessControlBlockInner.tasks` | 需要，`tasks.clear()` | 否则 PCB 一直持有已退出线程的 TCB 和内核栈 |
| `TASK_MANAGER.ready_queue` | 需要，`remove_task` | Ready/被抢占的线程挂在就绪队列上；不摘除的话调度器会取出一个地址空间已销毁、内核栈可能已释放的任务去执行，直接崩溃 |
| `TASK_MANAGER.stop_task` | 不能立即回收 | 退出者正在用这个 TCB 的内核栈，立即释放等于踩自己的栈；由下一次 `add_stopping_task` 覆盖时释放 |
| `TIMERS`（`TimerCondVar.task`） | 需要，`remove_timer` | 睡眠中的线程到期会被 `wakeup_task` 唤醒，TCB 已释放就是 use-after-free |
| mutex/semaphore/condvar 的 wait_queue | 不需要单独处理 | 这些同步资源属于本进程，PCB 释放时等待队列随之 drop，Arc 自然减少；也正因它们持有 Arc，被阻塞线程的 TCB 不会在内核栈回收前被提前释放 |
| `PROCESSOR.current` | 已由 `take_current_task` 取走 | 当前正在退出的线程就是它；其他线程不会出现在 current 上（单核） |
| `PID2PCB` | 不直接持有 TCB | 只持有 PCB 的 Arc，等 waitpid 后随 PCB 释放 |
| 任务自己的内核栈 | 无法从外部回收 | `block_current_and_run_next` 里 `let task = take_current_task()` 得到的 Arc 一直活到 `schedule` 返回，相当于线程自引用；只有任务再次被调度时才会释放。进程退出后这些线程不会再运行，因此这部分 TCB/内核栈实际上不会被回收，这是以 PCB 为单位回收资源时的取舍 |

总的处理原则是：**外部可达的引用（就绪队列、定时器）必须显式摘除，否则会出现 use-after-free；进程私有的引用（tasks、同步资源等待队列）随 PCB 统一释放；正在使用的内核栈只能延迟释放。** 回收顺序上还要注意先摘引用、再释放资源，并且避免在持有 PCB 借用时去 drop 那些会反过来访问 PCB 的对象。

### 简答2

两份实现的功能相同，但"谁在什么时候真正获得锁"不同：

| | Mutex1 | Mutex2 |
|---|---|---|
| lock | `loop` 循环，抢不到就睡眠，醒来后**重新检查** | 无循环，睡眠一次后直接返回，认为已持锁 |
| unlock | 先 `locked = false`，再唤醒队首 | 有等待者时保持 `locked = true`，把锁**交接**给队首；没有等待者才置 false |
| 语义 | 竞争式（通知后重新竞争） | 交接式（handoff，所有权随唤醒转移） |
| 公平性 | 可能被新请求插队 | 严格按等待队列 FIFO |

1. **循环是否必需。** Mutex1 的 `loop` 不是可选项而是前提：unlock 先把锁置空，被唤醒的线程在真正运行前，锁可能已经被别的线程抢走，所以醒来必须重新检查 `locked`；如果去掉 loop，它会在没有锁的情况下返回，互斥性被破坏。Mutex2 因为把锁直接交给队首且保持 `locked = true`，醒来即持有锁，可以省掉循环。

2. **Mutex1 可能的问题：插队与饥饿。** unlock 之后锁有一段空闲窗口，任何新来的线程都能 `lock` 成功；被唤醒的线程重新竞争失败时会再次入队（排到队尾）并睡眠，白白多一次唤醒和上下文切换。如果新请求源源不断，等待者可能长期拿不到锁（饥饿）；在 stride 调度下还会造成优先级反转、延迟不可控。多核下"置 false → 唤醒"不是原子的，插队会更明显。

3. **Mutex2 可能的问题：锁被"挂"在还没运行的线程上。** 交接之后 `locked` 一直是 true，所有权已经属于被唤醒的线程，但它可能还没被调度。如果这个线程永远无法运行（例如线程在运行前被销毁/取消，或它所属的进程异常退出），锁就永远不会被 unlock，其他等待者会永久阻塞——从"锁"的角度看是死锁或资源泄漏。它还把"只有 unlock 会唤醒等待者"当成前提：如果某次 `block_current_and_run_next` 是被信号、定时器等非 unlock 事件唤醒的，Mutex2 会误以为自己拿到了锁；Mutex1 的 loop 对这类虚假唤醒是免疫的。

4. **共同的问题。** 两段代码都用 `add_task` 而不是 `wakeup_task` 来唤醒等待者，`task_status` 仍是 `Blocked`，会出现"状态是阻塞、人却在就绪队列里"的不一致；本框架的 `run_tasks` 取到任务时会把它改成 `Running`，所以暂时不致命，但任何依赖 `TaskStatus` 做判断的代码（调度统计、信号处理、死锁检测等）都可能误判。此外两条 `unlock` 都只做了 `assert!(locked)`，重复 unlock 会 panic，而不是返回错误。

5. **本框架的选择。** 仓库里的 `MutexBlocking` 采用 Mutex2 的交接语义（但用 `wakeup_task`，会把状态改回 Ready）。在单核、内核态不可抢占的前提下，交接不会引入竞态，而且唤醒一次必然成功、天然 FIFO；Mutex1 的竞争式实现在这种环境下只会退化成"被唤醒者重新排队"，并发压力大时更容易饥饿。

## 三、荣誉准则
1. 我独立完成了本次实验的所有工作，包括代码与文档。
2. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
