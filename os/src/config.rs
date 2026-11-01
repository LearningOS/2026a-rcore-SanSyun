//! Constants in the kernel

#[allow(unused)]

/// user app's stack size
pub const USER_STACK_SIZE: usize = 4096 * 2;
/// kernel stack size
pub const KERNEL_STACK_SIZE: usize = 4096 * 2;
/// kernel heap size
pub const KERNEL_HEAP_SIZE: usize = 0x200_0000;

/// page size : 4KB
pub const PAGE_SIZE: usize = 0x1000;
/// page size bits: 12
pub const PAGE_SIZE_BITS: usize = 0xc;
/// the virtual addr of trapoline
pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;
/// the virtual addr of trap context
pub const TRAP_CONTEXT_BASE: usize = TRAMPOLINE - PAGE_SIZE;
/// clock frequency
pub const CLOCK_FREQ: usize = 12500000;
/// the physical memory end
pub const MEMORY_END: usize = 0x88000000;
/// stride 调度算法中的大步长
///
/// `pass = BIG_STRIDE / priority`，所以 `BIG_STRIDE` 越大调度的粒度越细，
/// 但取值过大时 `stride` 会增长过快而溢出。
pub const BIG_STRIDE: usize = 1_000_000;
/// 新建进程的初始优先级
pub const DEFAULT_PRIORITY: usize = 16;
/// 允许的最小优先级：取 1 会让 `pass == BIG_STRIDE`，取 0 则 `pass` 无法定义
pub const MIN_PRIORITY: isize = 2;
