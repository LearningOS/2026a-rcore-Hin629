//! Synchronization and interior mutability primitives

mod condvar;
mod mutex;
mod semaphore;
mod up;

pub use condvar::Condvar;
pub use mutex::{Mutex, MutexBlocking, MutexSpin};
pub use semaphore::Semaphore;
pub use up::UPSafeCell;

use crate::task::current_process;

/// 尝试拿锁
pub fn try_lock(thread_id : usize,lock_id : usize, cnt : usize) {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    inner.try_lock(thread_id, lock_id, cnt);
}

/// 取消尝试
pub fn un_try_lock(thread_id : usize,lock_id : usize, cnt : usize) {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    inner.un_try_lock(thread_id, lock_id, cnt);
}

/// 拿锁
pub fn lock(thread_id : usize,lock_id : usize, cnt : usize) {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    inner.lock(thread_id, lock_id, cnt);
}

/// 放锁
pub fn unlock(thread_id : usize,lock_id : usize, cnt : usize) {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    inner.unlock(thread_id, lock_id, cnt);
}
