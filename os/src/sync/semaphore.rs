//! Semaphore

use crate::sync::deadlock::{self, ResKind};
use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// A unique identifier of this semaphore (used by the deadlock detector)
    pub fn id(&self) -> usize {
        self as *const Self as usize
    }

    /// How many units of this semaphore can still be acquired
    pub fn available(&self) -> usize {
        let count = self.inner.exclusive_access().count;
        if count > 0 {
            count as usize
        } else {
            0
        }
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;
        let waking_task = if inner.count <= 0 {
            inner.wait_queue.pop_front()
        } else {
            None
        };
        drop(inner);
        if let Some(task) = waking_task {
            wakeup_task(task);
        }
        deadlock::release(ResKind::Sem, self.id());
    }

    /// down operation of semaphore
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        let blocked = inner.count < 0;
        if blocked {
            inner.wait_queue.push_back(current_task().unwrap());
        }
        drop(inner);
        if blocked {
            block_current_and_run_next();
        }
        deadlock::acquire(ResKind::Sem, self.id());
    }
}
