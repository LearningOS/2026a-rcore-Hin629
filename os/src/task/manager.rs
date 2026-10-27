//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::BinaryHeap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::*;

const BIG_STRIDE : u128 = 1u128 << 64;
struct StrideTCB {
    stride : u128,
    task : Arc<TaskControlBlock>
}
impl PartialEq for StrideTCB {
    fn eq(&self, other: &Self) -> bool {
        self.stride.eq(&other.stride)
    }
}
impl Eq for StrideTCB {}
impl Ord for StrideTCB {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        other.stride.cmp(&self.stride)
    }
}
impl PartialOrd for StrideTCB {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: BinaryHeap<StrideTCB>,
}

/// stride scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: BinaryHeap::new(),
        }
    }
    /// 获取当前的基准 stride 
    pub fn get_current_stride(&self) -> u128 {
        match self.ready_queue.peek() {
            Some(st) => {
                st.stride
            }
            None => {
                0u128
            }
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        let stride = *task.stride.shared_access();
        self.ready_queue.push(StrideTCB {
            stride,
            task
        });
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let st = self.ready_queue.pop()?;
        let task = st.task;
        let prio = *task.priority.shared_access() as u128; 
        assert!(prio > 1);
        let pass = BIG_STRIDE / prio;
        assert!(pass > 0);

        let min_stride = st.stride;
        let next_stride = match min_stride.checked_add(pass) {
            Some(next) => next,
            None => {
                let mut tasks : Vec<_> = self.ready_queue.drain().collect();

                for item in &mut tasks {
                    let ns = item.stride - min_stride;
                    item.stride = ns;
                    item.task.set_stride(ns);
                }
                
                self.ready_queue = BinaryHeap::from(tasks);
                pass
            }
        };
        
        task.set_stride(next_stride);
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

/// 获取当前的基准 stride 
pub fn get_current_stride() -> u128 {
    TASK_MANAGER.exclusive_access().get_current_stride()
}

