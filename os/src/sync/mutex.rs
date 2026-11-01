//! Mutex (spin-like and blocking(sleep))

use super::UPSafeCell;
use crate::sync::{lock, unlock};
use crate::task::{current_process, TaskControlBlock};
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self, tid : usize);
    /// Unlock the mutex
    fn unlock(&self, tid : usize);
    /// 获取 lock id
    fn get_lock_id(&self) -> usize;
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    lock_id : usize,
    locked: UPSafeCell<bool>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new(lock_id : usize) -> Self {
        Self {
            lock_id : lock_id,
            locked: unsafe { UPSafeCell::new(false) },
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self, tid : usize){
        trace!("kernel: MutexSpin::lock");
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                lock(tid, self.lock_id, 1);
                return;
            }
        }
    }

    fn unlock(&self, tid : usize) {
        trace!("kernel: MutexSpin::unlock");
        let mut locked = self.locked.exclusive_access();
        assert!(*locked);
        *locked = false;
        unlock(tid, self.lock_id, 1);
    }

    fn get_lock_id(&self) -> usize {
        self.lock_id
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    lock_id : usize,
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new(lock_id : usize) -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            lock_id : lock_id,
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
    fn lock(&self, tid : usize) {
        trace!("kernel: MutexBlocking::lock");
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
            // DEADLOCK FIX: unlock 已把资源转交给本线程。
            return;
        } else {
            mutex_inner.locked = true;
        }
        lock(tid, self.lock_id, 1);
    }

    /// unlock the blocking mutex
    fn unlock(&self, tid : usize) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        // DEADLOCK FIX: 原持有者释放和等待者获取在同一次 PCB 借用中完成。
        let waking_task = mutex_inner.wait_queue.pop_front();
        let waiting_tid = waking_task.as_ref().map(|task| {
            task.inner_exclusive_access().res.as_ref().unwrap().tid
        });
        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        process_inner.unlock(tid, self.lock_id, 1);
        if let Some(waiting_tid) = waiting_tid {
            process_inner.lock(waiting_tid, self.lock_id, 1);
        } else {
            mutex_inner.locked = false;
        }
        drop(process_inner);
        drop(mutex_inner);
        if let Some(task) = waking_task {
            wakeup_task(task);
        }
    }

    fn get_lock_id(&self) -> usize {
        self.lock_id
    }
}
