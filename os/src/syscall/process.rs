//! Process management syscalls

use crate::{config::PAGE_SIZE, mm::{MapPermission, translated_byte_buffer}, task::{change_program_brk, current_user_token, exit_current_and_run_next, suspend_current_and_run_next}, timer::get_time_us};
use crate::mm::{PTEFlags, PageTable, VirtAddr};
// use core::{mem::size_of, ptr::{read_volatile, write_volatile}, simd::isizex1};
use core::{mem::size_of, ptr::{read_volatile, write_volatile}};

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

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");

    let len = size_of::<TimeVal>();
    let start = ts as usize;
    let Some(end) = start.checked_add(len) else {
        return -1;
    };

    let token = current_user_token();
    let page_table = PageTable::from_token(token);
    for vpn in [VirtAddr::from(start).floor(), VirtAddr::from(end - 1).floor()] {
        match page_table.translate(vpn) {
            Some(pte) if pte.is_valid() && pte.writable() && pte.flags().contains(PTEFlags::U) => {}
            _ => return -1,
        }
    }

    let us = get_time_us();
    let time = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(&time as *const TimeVal as *const u8, len)
    };
    let mut copied = 0;
    for page in translated_byte_buffer(token, ts as *const u8, len) {
        page.copy_from_slice(&bytes[copied..copied + page.len()]);
        copied += page.len();
    }
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 | 1=> {
            let token = current_user_token();
            let page_table = PageTable::from_token(token);
            
            //先检查pte是否合法
            let vpn = VirtAddr::from(id).floor();
            let Some(pte) = page_table.translate(vpn) else {
                return -1;
            };
            if !pte.flags().contains(PTEFlags::U) {
                return -1;
            }
            let page = translated_byte_buffer(token, id as *const u8, 1);
            if trace_request == 0{ //读
                if !(pte.readable() || pte.executable()) {
                    return -1;
                }
                unsafe {
                    let value = read_volatile(page[0].as_ptr() as *const u8)as isize;
                    return value;
                }
            }
            else { //写
                if !pte.writable() {
                    return -1;
                }
                unsafe {
                    write_volatile(page[0].as_ptr() as *mut u8, data as u8);
                    return 0;
                }
            }
        }
        2 => {
            crate::task::get_syscall_times(id)
        }     
        _ => {
            -1
        }   
    }
}

pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap");
    // 页对齐、长度非 0、port 只能取 R/W/X 且至少一个
    if start % PAGE_SIZE != 0 || len == 0 || port == 0 || port & !0b111 != 0 {
        return -1;
    }
    let Some(end) = start.checked_add(len) else {
        return -1;
    };

    let mut perm = MapPermission::U;
    if port & 0b001 != 0 { perm |= MapPermission::R; }
    if port & 0b010 != 0 { perm |= MapPermission::W; }
    if port & 0b100 != 0 { perm |= MapPermission::X; }

    crate::task::mmap_current(VirtAddr(start), VirtAddr(end), perm)
}

pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if start % PAGE_SIZE != 0 || len == 0 {
        return -1;
    }
    let Some(end) = start.checked_add(len) else {
        return -1;
    };
    crate::task::munmap_current(VirtAddr(start), VirtAddr(end))
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
