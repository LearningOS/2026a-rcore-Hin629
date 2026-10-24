//! Process management syscalls
use core::mem::size_of;

use crate::{mm::{translated_byte_buffer, MapPermission, PTEFlags, VirtAddr}, task::{change_program_brk, current_user_token, exit_current_and_run_next, get_current_syscall_num, mmap, munmap, suspend_current_and_run_next, va_in_current_and_satisfied}, timer::get_time_us};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// 获取当前的时间
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let token = current_user_token();
    let buffers = translated_byte_buffer(token, ts as *const u8, size_of::<TimeVal>());
    let us = get_time_us();
    let sec = us / 1_000_000;
    let usec = us % 1_000_000;
    let data = sec.to_ne_bytes().into_iter().chain(usec.to_ne_bytes());
    for (dst, src) in buffers.into_iter().flatten().zip(data) {
        *dst = src;
    }
    0
}

/// request = 0 表示读 id 地址的一个字节
/// 
/// request = 0 表示往 id 地址写入 data 这个数据内的一个字节
/// 
/// request = 2 表示获取当前运行的任务的第 id 个系统调用的调用次数
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => {
            let flag : PTEFlags = PTEFlags::V | PTEFlags::R | PTEFlags::U;
            if let Some(pa) = va_in_current_and_satisfied(id.into(), flag) {
                unsafe {
                    *(usize::from(pa) as *const u8) as isize
                }
            }else {
                -1
            }
        }
        1 => {
            let flag : PTEFlags = PTEFlags::V | PTEFlags::W | PTEFlags::U;
            if let Some(pa) = va_in_current_and_satisfied(id.into(), flag) {
                unsafe {
                    *(usize::from(pa) as *mut u8) = data as u8;
                    0
                }
            }else {
                -1
            }
        }
        _ => {
            get_current_syscall_num(id) as isize
        }
    }
}

/// 映射一块地址，需要保证 start 是页对齐的
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    let st_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    if !st_va.aligned() || (port & !0x7) != 0 || (port & 0x7) == 0 {
        return -1;
    }
    match mmap(st_va, end_va, MapPermission::from_bits_truncate((port as u8) << 1) | MapPermission::U ){
        Ok(_) => {
            0
        }
        Err(_) => {
            -1
        }
    }
}

/// 取消映射一块地址，需要保证 start 是页对齐的
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    let st_va = VirtAddr::from(start);
    let end_va = VirtAddr::from(start + len);
    if !st_va.aligned() {
        return -1;
    }
    match munmap(st_va, end_va){
        Ok(_) => {
            0
        }
        Err(_) => {
            -1
        }
    }
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
