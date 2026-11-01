//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// stride 调度器。
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// 从就绪队列中取出 stride 最小的进程，这是 stride 调度算法的核心。
    ///
    /// 被选中的进程返回之前，它的 `stride` 会加上自己的
    /// `pass = BIG_STRIDE / priority`，因此它得到的 CPU 时间与其优先级成正比。
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let mut chosen: Option<usize> = None;
        let mut min_stride = usize::MAX;
        for (idx, task) in self.ready_queue.iter().enumerate() {
            let stride = task.inner_exclusive_access().stride;
            if chosen.is_none() || stride < min_stride {
                min_stride = stride;
                chosen = Some(idx);
            }
        }
        let task = self.ready_queue.remove(chosen?)?;
        let pass = task.inner_exclusive_access().pass();
        task.inner_exclusive_access().stride += pass;
        Some(task)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
