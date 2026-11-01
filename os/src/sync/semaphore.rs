//! Semaphore

use crate::sync::{lock, UPSafeCell};
use crate::task::current_process;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore structure
pub struct Semaphore {
    /// 资源管理对应的 lock id
    pub lock_id : usize,
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(lock_id : usize, res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            lock_id : lock_id,
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self, tid : usize) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;
        // DEADLOCK FIX: 每次 up 都记账；许可在唤醒前直接分配给等待者。
        let waking_task = if inner.count <= 0 {
            Some(inner.wait_queue.pop_front().expect("semaphore count/queue mismatch"))
        } else {
            None
        };
        let waiting_tid = waking_task.as_ref().map(|task| {
            task.inner_exclusive_access().res.as_ref().unwrap().tid
        });
        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        process_inner.semaphore_release(tid, self.lock_id);
        if let Some(waiting_tid) = waiting_tid {
            process_inner.lock(waiting_tid, self.lock_id, 1);
        }
        drop(process_inner);
        drop(inner);
        if let Some(task) = waking_task {
            wakeup_task(task);
        }
    }

    /// down operation of semaphore
    pub fn down(&self, tid : usize) {
        trace!("kernel: Semaphore::down");
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
            // DEADLOCK FIX: up 已完成分配，恢复后不能再次扣减 need/available。
            return;
        }
        lock(tid, self.lock_id, 1);
    }

    /// 获取 lock_id
    pub fn get_lock_id(&self) -> usize {
        self.lock_id
    }
}
