//! Mutex (spin-like and blocking(sleep))

use super::deadlock::{self, ResKind};
use super::UPSafeCell;
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self);
    /// Unlock the mutex
    fn unlock(&self);
    /// Whether the mutex is currently locked
    fn is_locked(&self) -> bool;
    /// A unique identifier of this mutex (used by the deadlock detector)
    fn id(&self) -> usize;
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    locked: UPSafeCell<bool>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new() -> Self {
        Self {
            locked: unsafe { UPSafeCell::new(false) },
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) {
        trace!("kernel: MutexSpin::lock");
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                break;
            }
        }
        deadlock::acquire(ResKind::Mutex, self.id());
    }

    fn unlock(&self) {
        trace!("kernel: MutexSpin::unlock");
        let mut locked = self.locked.exclusive_access();
        *locked = false;
        drop(locked);
        deadlock::release(ResKind::Mutex, self.id());
    }

    fn is_locked(&self) -> bool {
        *self.locked.exclusive_access()
    }

    fn id(&self) -> usize {
        self as *const Self as usize
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new() -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) {
        trace!("kernel: MutexBlocking::lock");
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
            drop(mutex_inner);
        }
        deadlock::acquire(ResKind::Mutex, self.id());
    }

    /// unlock the blocking mutex
    fn unlock(&self) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        let waking_task = mutex_inner.wait_queue.pop_front();
        if waking_task.is_none() {
            mutex_inner.locked = false;
        }
        drop(mutex_inner);
        if let Some(waking_task) = waking_task {
            wakeup_task(waking_task);
        }
        deadlock::release(ResKind::Mutex, self.id());
    }

    fn is_locked(&self) -> bool {
        self.inner.exclusive_access().locked
    }

    fn id(&self) -> usize {
        self as *const Self as usize
    }
}
