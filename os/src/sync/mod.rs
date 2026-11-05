//! Synchronization and interior mutability primitives

mod condvar;
pub mod deadlock;
mod mutex;
mod semaphore;
mod up;

pub use condvar::Condvar;
pub use deadlock::{ResKind, DEADLOCK_RET};
pub use mutex::{Mutex, MutexBlocking, MutexSpin};
pub use semaphore::Semaphore;
pub use up::UPSafeCell;
