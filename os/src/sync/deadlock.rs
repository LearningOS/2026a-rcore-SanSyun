//! Deadlock detection (Banker's algorithm)
//!
//! A process can enable deadlock detection by calling
//! [`crate::syscall::sync::sys_enable_deadlock_detect`]. While it is enabled, a
//! `mutex_lock` / `semaphore_down` request that would put the process into an
//! unsafe state is rejected with [`DEADLOCK_RET`] instead of blocking.
//!
//! Mutexes and semaphores are checked separately (mixed usage with e.g.
//! `waittid` is not taken into account), which keeps the model simple enough
//! for the classic Banker's safety algorithm:
//!
//! * `Available`: how many units of each resource can still be acquired,
//! * `Allocation`: how many units of each resource every thread holds,
//! * `Need`: the resource a blocked thread is waiting for (one unit).
//!
//! A thread that is not blocked on this kind of resource can always run to
//! completion and release everything it holds, so its allocation is added to
//! `Work` before the iterative part of the algorithm starts. If some blocked
//! thread can never be satisfied, the system is in an unsafe state and the new
//! request must not block.

use crate::task::{current_process, current_task, ProcessControlBlock, TaskStatus};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

/// The two kinds of synchronization resources we can detect deadlocks on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResKind {
    /// Mutex
    Mutex,
    /// Semaphore
    Sem,
}

/// The value returned to the user program if a deadlock is detected.
pub const DEADLOCK_RET: isize = -0xdead;

/// Get the tid of the current thread.
fn current_tid() -> usize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid
}

/// Record that the current thread has acquired one unit of `res`.
pub fn acquire(kind: ResKind, res: usize) {
    let tid = current_tid();
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    if let Some(item) = inner
        .res_hold
        .iter_mut()
        .find(|(t, k, r, _)| *t == tid && *k == kind && *r == res)
    {
        item.3 += 1;
    } else {
        inner.res_hold.push((tid, kind, res, 1));
    }
}

/// Record that the current thread has released one unit of `res`.
pub fn release(kind: ResKind, res: usize) {
    let tid = current_tid();
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    if let Some(idx) = inner
        .res_hold
        .iter()
        .position(|(t, k, r, _)| *t == tid && *k == kind && *r == res)
    {
        if inner.res_hold[idx].3 > 1 {
            inner.res_hold[idx].3 -= 1;
        } else {
            inner.res_hold.remove(idx);
        }
    }
}

/// Mark the current thread as waiting for `res`.
pub fn set_waiting(kind: ResKind, res: usize) {
    let tid = current_tid();
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    inner.res_wait.retain(|(t, _, _)| *t != tid);
    inner.res_wait.push((tid, kind, res));
}

/// Clear the waiting state of the current thread.
pub fn clear_waiting() {
    let tid = current_tid();
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    inner.res_wait.retain(|(t, _, _)| *t != tid);
}

/// Check whether granting `req` to the current thread would deadlock.
///
/// Returns `true` if the system is in an unsafe state, in which case the
/// caller should refuse the request and return [`DEADLOCK_RET`] to the user.
pub fn check_deadlock(process: &Arc<ProcessControlBlock>, kind: ResKind, req: usize) -> bool {
    let tid = current_tid();
    let inner = process.inner_exclusive_access();

    // 1. Collect all resources of this kind and their available amounts.
    let mut ids: Vec<usize> = Vec::new();
    let mut avail: Vec<usize> = Vec::new();
    match kind {
        ResKind::Mutex => {
            for mutex in inner.mutex_list.iter().flatten() {
                ids.push(mutex.id());
                avail.push(if mutex.is_locked() { 0 } else { 1 });
            }
        }
        ResKind::Sem => {
            for sem in inner.semaphore_list.iter().flatten() {
                ids.push(sem.id());
                avail.push(sem.available());
            }
        }
    }
    let m = ids.len();
    let req_idx = match ids.iter().position(|&id| id == req) {
        Some(idx) => idx,
        None => return false,
    };

    // 2. Build the Allocation / Need matrices of all live threads.
    let n = inner.tasks.len();
    let mut finish = vec![true; n];
    let mut alloc = vec![vec![0usize; m]; n];
    let mut need: Vec<Option<usize>> = vec![None; n];
    for (i, task) in inner.tasks.iter().enumerate() {
        let task = match task {
            Some(task) => task,
            None => continue,
        };
        // A thread that is blocked on this kind of resource still needs one
        // more unit of it, others can run to completion and release all their
        // resources.
        let status = task.inner_exclusive_access().task_status;
        for &(t, k, r, cnt) in inner.res_hold.iter() {
            if t == i && k == kind {
                if let Some(j) = ids.iter().position(|&id| id == r) {
                    alloc[i][j] += cnt;
                }
            }
        }
        if status == TaskStatus::Blocked {
            if let Some(&(_, _, r)) = inner
                .res_wait
                .iter()
                .find(|(t, k, _)| *t == i && *k == kind)
            {
                if let Some(j) = ids.iter().position(|&id| id == r) {
                    need[i] = Some(j);
                    finish[i] = false;
                }
            }
        }
    }
    // The current thread is the one about to block on `req`.
    if tid < n && inner.tasks[tid].is_some() {
        finish[tid] = false;
        need[tid] = Some(req_idx);
    }

    // 3. Run the Banker's safety algorithm.
    let mut work = avail;
    for i in 0..n {
        if finish[i] {
            for (j, w) in work.iter_mut().enumerate() {
                *w += alloc[i][j];
            }
        }
    }
    loop {
        let mut progressed = false;
        for i in 0..n {
            if !finish[i] {
                let j = need[i].unwrap();
                if work[j] >= 1 {
                    work[j] -= 1;
                    for (k, w) in work.iter_mut().enumerate() {
                        *w += alloc[i][k];
                    }
                    finish[i] = true;
                    progressed = true;
                }
            }
        }
        if !progressed {
            break;
        }
    }
    finish.iter().any(|&done| !done)
}
