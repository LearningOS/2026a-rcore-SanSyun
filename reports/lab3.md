# lab3 实验报告

## 一、功能总结（200 字以内，不贴代码）
本次实验在 ch4 基础上实现进程管理：任务有独立 TCB、内核栈与地址空间，支持 fork/exec/waitpid 与父子关系。新增 spawn：按名字加载 ELF 建立全新地址空间，不复制父进程内存；新增 set_priority 与 stride 调度，stride 记录累计 CPU 时间、pass = BIG_STRIDE / priority 为每次调度的增量，每次取 stride 最小者并累加其 pass，使 CPU 时间与优先级成正比。同时把 get_time、mmap、munmap 迁移到按进程保存的 MemorySet 上。

## 二、简答题

### 简答1

不会轮到 p1，而是 p2 继续执行。

p2 执行一个时间片后 `p2.stride = 250 + 10 = 260`，8 位无符号整型放不下 260，回绕成 `260 − 256 = 4`。此时 `p2.stride(4) < p1.stride(255)`，按"选 stride 最小者"的规则，调度器仍然选中 p2。

而且 p2 会一直霸占 CPU：它的 stride 按 4、14、24 …… 254 递增，再 +10 又回绕到 8，取值始终小于 255，p1 永远等不到自己成为最小值。根本原因是 **stride 是单调递增的虚拟时间，回绕破坏了"越小越早被调度"的语义**：真实虚拟时间 p2(260) 已经大于 p1(255)，本该执行 p1，但溢出后的数值看起来 p2 更小。64 位实现只是把回绕推迟，问题依然存在，正确的做法是比较时按 BigStride 取模（见简答3）。

### 简答2

记 `MaxPass = max{pass_i}`。所有优先级 ≥ 2 时，`pass_i = BigStride / priority_i ≤ BigStride / 2`，因此 `MaxPass ≤ BigStride / 2`（这正是"优先级 ≥ 2"这个限制的由来）。

用归纳法证明：**无论怎么调度，就绪队列中所有进程的 stride 都落在一个长度不超过 MaxPass 的区间内。**

设某次调度前最小值为 `S_min`、最大值为 `S_max`，且 `S_max − S_min ≤ MaxPass`（归纳假设）。一次调度只会做两件事：

1. 取出 stride 恰为 `S_min` 的进程，把它加上自己的 `pass` 后放回；
2. 其余进程的 stride 保持不变。

于是调度之后：

- 被选中的进程只增不减，其它进程不动，所以新的最小值不会小于 `S_min`；
- 它回填的新 stride 为 `S_min + pass ≤ S_min + MaxPass`；
- 其余进程的 stride 仍落在 `[S_min, S_max] ⊆ [S_min, S_min + MaxPass]`。

即调度后所有 stride 仍落在 `[S_min, S_min + MaxPass]` 内，区间长度不超过 `MaxPass`，归纳成立。初始时所有 stride 都是 0，属于退化情形，结论自然成立。若不发生溢出，就有 `STRIDE_MAX − STRIDE_MIN ≤ MaxPass ≤ BigStride / 2`。

这个结论的意义在于：**真实的最小值一定在"半圈"窗口内**，所以即使数值会回绕，只要在比较时只承认半圈以内的差值，就能判断出真正的先后；反过来，如果允许 priority = 1，`pass` 会等于 BigStride，一次调度就能把区间拉开到超过半个 BigStride，模比较会把大小关系判反。

### 简答3

两个 stride 的差值不会超过 `BigStride / 2`（简答2），所以可以把它们看成模 `BigStride` 的圆上的两个点：把 `self − other` 按 BigStride 取模，结果落在 `(0, BigStride / 2]` 说明 self 没有绕圈、真实值更大；落在 `(BigStride / 2, BigStride)` 说明 other 已经绕回一圈，self 其实更小。8 bit 时也可以直接看 `self.0.wrapping_sub(other.0) as i8` 的符号，回绕后 125 还是正的、129 会变成 −127。

另外要注意 `BinaryHeap` 是**大顶堆**，`pop` 取的是"最大"元素，而这里要拿到的是 stride 最小的进程，所以比较结果要反过来写：真实 stride 更小的元素，在堆序里要判成"更大"。

```rust
use core::cmp::Ordering;

/// 一圈的长度，题目里 8 bit 的示例取 255
const BIG_STRIDE: u64 = 255;

struct Stride(u64);

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // 假设 stride 已对 BIG_STRIDE 取模，即 0 <= stride <= BIG_STRIDE
        // diff 表示从 other 走到 self 需要往前挪多少（模 BIG_STRIDE）
        let diff = (self.0 + BIG_STRIDE - other.0) % BIG_STRIDE;
        if diff == 0 {
            // 题目假设两个 Stride 永远不会相等
            Some(Ordering::Equal)
        } else if diff <= BIG_STRIDE / 2 {
            // 没有绕圈：self 的真实 stride 更大。
            // 但大顶堆要 pop 出最小 stride，所以这里反过来判为 Less
            Some(Ordering::Less)
        } else {
            // other 已经绕回一圈：self 其实更小，应被判为堆中的"最大"者
            Some(Ordering::Greater)
        }
    }
}

impl PartialEq for Stride {
    fn eq(&self, other: &Self) -> bool {
        false
    }
}
```

如果不想把方向反过来，也可以直接写自然序（`diff <= BIG_STRIDE / 2` 时返回 `Greater`），再把元素用 `core::cmp::Reverse<Stride>` 包起来放进堆里，效果相同。

用题目里的例子验证：p2 执行后 `p1.stride = 255`、`p2.stride = 4`（实际的 260 回绕而成）。`diff = (255 + 255 − 4) % 255 = 251 > 127`，于是 `p1 > p2`，pop 先取出 p1——正是真实 stride 更小的那个，回绕被正确识别。

## 三、荣誉准则
1. 我独立完成了本次实验的所有工作，包括代码与文档。
2. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。 我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。 我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。 我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。
