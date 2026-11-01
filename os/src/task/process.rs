//! Implementation of  [`ProcessControlBlock`]

use super::id::RecycleAllocator;
use super::manager::insert_into_pid2process;
use super::TaskControlBlock;
use super::{add_task, SignalFlags};
use super::{pid_alloc, PidHandle};
use crate::fs::{File, Stdin, Stdout};
use crate::mm::{translated_refmut, MemorySet, KERNEL_SPACE};
use crate::sync::{Condvar, Mutex, Semaphore, UPSafeCell};
use crate::trap::{trap_handler, TrapContext};
use alloc::collections::{BTreeMap, BTreeSet, BinaryHeap};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefMut;
use core::cmp::Reverse;

/// Process Control Block
pub struct ProcessControlBlock {
    /// immutable
    pub pid: PidHandle,
    /// mutable
    inner: UPSafeCell<ProcessControlBlockInner>,
}

/// Inner of Process Control Block
pub struct ProcessControlBlockInner {
    /// is zombie?
    pub is_zombie: bool,
    /// memory set(address space)
    pub memory_set: MemorySet,
    /// parent process
    pub parent: Option<Weak<ProcessControlBlock>>,
    /// children process
    pub children: Vec<Arc<ProcessControlBlock>>,
    /// exit code
    pub exit_code: i32,
    /// file descriptor table
    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
    /// signal flags
    pub signals: SignalFlags,
    /// tasks(also known as threads)
    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    /// task resource allocator
    pub task_res_allocator: RecycleAllocator,
    /// mutex list
    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
    /// semaphore list
    pub semaphore_list: Vec<Option<Arc<Semaphore>>>,
    /// condvar list
    pub condvar_list: Vec<Option<Arc<Condvar>>>,
    /// 并发资源可用量
    pub lock_available : Vec<usize>,
    /// 是否启用死锁检测功能
    pub open_dead_lock_check : bool,
    /// 线程分配矩阵
    pub lock_allocation : Vec<BTreeMap<usize,usize>>,
    /// 线程需求矩阵
    pub lock_need : Vec<BTreeMap<usize,usize>>,
    // DEADLOCK FIX: 退出线程不能参与安全序列，tid 复用时重新初始化。
    pub lock_thread_alive: Vec<bool>,
}

impl ProcessControlBlockInner {
    #[allow(unused)]
    /// get the address of app's page table
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// allocate a new file descriptor
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
    /// allocate a new task id
    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }
    /// deallocate a task id
    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }
    /// the count of tasks(threads) in this process
    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }
    /// get a task with tid in this process
    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }

    // DEADLOCK FIX: 新线程和复用的 tid 都从空记账开始。
    pub fn init_lock_thread(&mut self, tid: usize) {
        self.lock_need
            .resize_with(self.lock_need.len().max(tid + 1), BTreeMap::new);
        self.lock_allocation
            .resize_with(self.lock_allocation.len().max(tid + 1), BTreeMap::new);
        self.lock_thread_alive
            .resize(self.lock_thread_alive.len().max(tid + 1), false);
        self.lock_need[tid].clear();
        self.lock_allocation[tid].clear();
        self.lock_thread_alive[tid] = true;
    }

    // DEADLOCK FIX: 不增加 available；实际同步原语并未因线程退出而释放。
    pub fn exit_lock_thread(&mut self, tid: usize) {
        self.lock_thread_alive[tid] = false;
        self.lock_need[tid].clear();
        self.lock_allocation[tid].clear();
    }

    // DEADLOCK FIX: semaphore up 允许无持有记录的线程产生一个许可。
    // 不猜测它对应其他线程的哪次 down；有自己的持有记录时才扣除。
    pub fn semaphore_release(&mut self, tid: usize, lock_id: usize) {
        let allocation = &mut self.lock_allocation[tid];
        if let Some(count) = allocation.get_mut(&lock_id) {
            assert!(*count > 0);
            *count -= 1;
            if *count == 0 {
                allocation.remove(&lock_id);
            }
        }
        self.lock_available[lock_id] += 1;
    }

    /// 为新创建的一个 lock 分配 id，并添加 available 资源
    pub fn get_new_lock_id(&mut self, cnt : usize) -> usize{
        let new_id = self.lock_available.len();
        self.lock_available.push(cnt);
        new_id
    }

    /// 尝试 lock，把对应的 need + cnt
    pub fn try_lock(&mut self,thread_id : usize, lock_id : usize, cnt : usize){
        let ne =  self.lock_need[thread_id].entry(lock_id).or_insert(0);
        *ne += cnt;
    }

    /// 取消 try_lock，把对应的 need - cnt
    ///
    /// 检查加锁合法性失败的操作
    pub fn un_try_lock(&mut self,thread_id : usize, lock_id : usize, cnt : usize){
        let ne =  self.lock_need[thread_id].entry(lock_id).or_insert(0);
        *ne -= cnt;
        if *ne == 0 {
            self.lock_need[thread_id].remove(&lock_id);
        }
    }

    /// 尝试 lock，把对应的 need - cnt，并把 alloc + cnt
    pub fn lock(&mut self,thread_id : usize, lock_id : usize, cnt : usize) {
        let ne =  self.lock_need[thread_id].entry(lock_id).or_insert(0);
        *ne -= cnt;
        if *ne == 0 {
            self.lock_need[thread_id].remove(&lock_id);
        }
        self.lock_available[lock_id] -= cnt;
        let al =  self.lock_allocation[thread_id].entry(lock_id).or_insert(0);
        *al += cnt;
    }

    /// 取消 lock，把对应的 alloc - cnt
    pub fn unlock(&mut self,thread_id : usize, lock_id : usize, cnt : usize) {
        let al =  self.lock_allocation[thread_id].entry(lock_id).or_insert(0);
        *al -= cnt;
        self.lock_available[lock_id] += cnt;
        if *al == 0 {
            self.lock_allocation[thread_id].remove(&lock_id);
        }
    }

    /// 死锁检查
    pub fn check_dead_lock(&self) -> bool {
        if !self.open_dead_lock_check {
            return false;
        }
        let mut work = self.lock_available.clone();
        let resources_num = self.lock_available.len();
        let thread_num = self.lock_need.len();
        // DEADLOCK FIX: 仅存活线程需要完成，退出者的资源不能模拟释放。
        let alive_num = self.lock_thread_alive.iter().filter(|&&alive| alive).count();
        let mut resoures_need_small_heap: Vec<BinaryHeap<Reverse<(usize, usize)>>> = vec![BinaryHeap::new();resources_num];
        let mut true_nums = 0;
        let mut unsatisfaced = vec![0;thread_num];
        let mut new_upadte: BTreeSet<usize> = BTreeSet::new();
        for i in 0..thread_num {
            if !self.lock_thread_alive[i] {
                continue;
            }
            unsatisfaced[i] = self.lock_need[i].len();
            for (&j,&cnt) in self.lock_need[i].iter() {
                if cnt <= work[j] {
                    unsatisfaced[i] -= 1;
                }else {
                    resoures_need_small_heap[j].push(Reverse((cnt,i)));
                }
            }
            if unsatisfaced[i] == 0 {
                true_nums += 1;
                for (&j, &cnt) in self.lock_allocation[i].iter() {
                    work[j] += cnt;
                    new_upadte.insert(j);
                }
            }
        }
        while let Some(j) = new_upadte.pop_first() {
            while let Some(&Reverse((cnt,i))) = resoures_need_small_heap[j].peek() {
                if cnt <= work[j] {
                    unsatisfaced[i] -= 1;
                    if unsatisfaced[i] == 0 {
                        true_nums += 1;
                        for (&j1, &cnt1) in self.lock_allocation[i].iter() {
                            work[j1] += cnt1;
                            new_upadte.insert(j1);
                        }
                    }
                    resoures_need_small_heap[j].pop();
                }else {
                    break;
                }
            }
        }
        true_nums != alive_num
    }
}

impl ProcessControlBlock {
    /// inner_exclusive_access
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// new process from elf file
    pub fn new(elf_data: &[u8]) -> Arc<Self> {
        trace!("kernel: ProcessControlBlock::new");
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        // allocate a pid
        let pid_handle = pid_alloc();
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> stdin
                        Some(Arc::new(Stdin)),
                        // 1 -> stdout
                        Some(Arc::new(Stdout)),
                        // 2 -> stderr
                        Some(Arc::new(Stdout)),
                    ],
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    lock_available : Vec::new(),
                    open_dead_lock_check : false,
                    lock_allocation : Vec::new(),
                    lock_need : Vec::new(),
                    // DEADLOCK FIX: 初始化线程存活记录。
                    lock_thread_alive: Vec::new(),
                })
            },
        });
        // create a main thread, we should allocate ustack and trap_cx here
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&process),
            ustack_base,
            true,
        ));
        // prepare trap_cx of main thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        let ustack_top = task_inner.res.as_ref().unwrap().ustack_top();
        let kstack_top = task.kstack.get_top();
        drop(task_inner);
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            ustack_top,
            KERNEL_SPACE.exclusive_access().token(),
            kstack_top,
            trap_handler as usize,
        );
        // add main thread to the process
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(Arc::clone(&task)));
        // DEADLOCK FIX: 主线程也通过统一入口初始化。
        process_inner.init_lock_thread(0);
        drop(process_inner);
        insert_into_pid2process(process.getpid(), Arc::clone(&process));
        // add main thread to scheduler
        add_task(task);
        process
    }

    /// Only support processes with a single thread.
    pub fn exec(self: &Arc<Self>, elf_data: &[u8], args: Vec<String>) {
        trace!("kernel: exec");
        assert_eq!(self.inner_exclusive_access().thread_count(), 1);
        // memory_set with elf program headers/trampoline/trap context/user stack
        trace!("kernel: exec .. MemorySet::from_elf");
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        let new_token = memory_set.token();
        // substitute memory_set
        trace!("kernel: exec .. substitute memory_set");
        self.inner_exclusive_access().memory_set = memory_set;
        // then we alloc user resource for main thread again
        // since memory_set has been changed
        trace!("kernel: exec .. alloc user resource for main thread again");
        let task = self.inner_exclusive_access().get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        task_inner.res.as_mut().unwrap().ustack_base = ustack_base;
        task_inner.res.as_mut().unwrap().alloc_user_res();
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();
        // push arguments on user stack
        trace!("kernel: exec .. push arguments on user stack");
        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_refmut(
                    new_token,
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize,
                )
            })
            .collect();
        *argv[args.len()] = 0;
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_refmut(new_token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_refmut(new_token, p as *mut u8) = 0;
        }
        // make the user_sp aligned to 8B for k210 platform
        user_sp -= user_sp % core::mem::size_of::<usize>();
        // initialize trap_cx
        trace!("kernel: exec .. initialize trap_cx");
        let mut trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kstack.get_top(),
            trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
    }

    /// Only support processes with a single thread.
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        trace!("kernel: fork");
        let mut parent = self.inner_exclusive_access();
        assert_eq!(parent.thread_count(), 1);
        // clone parent's memory_set completely including trampoline/ustacks/trap_cxs
        let memory_set = MemorySet::from_existed_user(&parent.memory_set);
        // alloc a pid
        let pid = pid_alloc();
        // copy fd table
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // create child process pcb
        let child = Arc::new(Self {
            pid,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    is_zombie: false,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                    lock_available : Vec::new(),
                    open_dead_lock_check : false,
                    lock_allocation : Vec::new(),
                    lock_need : Vec::new(),
                    // DEADLOCK FIX: 初始化线程存活记录。
                    lock_thread_alive: Vec::new(),
                })
            },
        });
        // add child
        parent.children.push(Arc::clone(&child));
        // create main thread of child process
        let task = Arc::new(TaskControlBlock::new(
            Arc::clone(&child),
            parent
                .get_task(0)
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .ustack_base(),
            // here we do not allocate trap_cx or ustack again
            // but mention that we allocate a new kstack here
            false,
        ));
        // attach task to child process
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(Arc::clone(&task)));
        // DEADLOCK FIX: fork 子进程主线程的检测状态。
        child_inner.init_lock_thread(0);
        drop(child_inner);
        // modify kstack_top in trap_cx of this thread
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kstack.get_top();
        drop(task_inner);
        insert_into_pid2process(child.getpid(), Arc::clone(&child));
        // add this thread to scheduler
        add_task(task);
        child
    }
    /// get pid
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
}
