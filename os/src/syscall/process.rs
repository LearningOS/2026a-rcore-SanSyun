//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    config::{MIN_PRIORITY, PAGE_SIZE},
    loader::get_app_data_by_name,
    mm::{
        translated_byte_buffer, translated_refmut, translated_str, MapPermission, PageTable,
        VirtAddr, PTEFlags,
    },
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    },
    timer::get_time_us,
};
use core::mem::size_of;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!("kernel::pid[{}] sys_waitpid [{}]", current_task().unwrap().pid.0, pid);
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// 获取以秒和微秒表示的时间
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel:pid[{}] sys_get_time", current_task().unwrap().pid.0);
    let len = size_of::<TimeVal>();
    let start = ts as usize;
    let Some(end) = start.checked_add(len) else {
        return -1;
    };
    let token = current_user_token();
    // [`TimeVal`] 可能被两个页面切开，写之前先确认这两页都已映射、
    // 用户态可访问且可写
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
    let bytes = unsafe { core::slice::from_raw_parts(&time as *const TimeVal as *const u8, len) };
    // 逐页拷贝：用户缓冲区在物理内存中可能并不连续
    let mut copied = 0;
    for page in translated_byte_buffer(token, ts as *const u8, len) {
        let page_len = page.len();
        page.copy_from_slice(&bytes[copied..copied + page_len]);
        copied += page_len;
    }
    0
}

/// 为当前进程映射 [start, start + len)，`port` 是 R/W/X 的位组合
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel:pid[{}] sys_mmap", current_task().unwrap().pid.0);
    // start 必须页对齐，len 不能为 0，`port` 只能由 R/W/X 组成且至少有一位
    if start % PAGE_SIZE != 0 || len == 0 || port == 0 || port & !0b111 != 0 {
        return -1;
    }
    let Some(end) = start.checked_add(len) else {
        return -1;
    };

    let mut perm = MapPermission::U;
    if port & 0b001 != 0 {
        perm |= MapPermission::R;
    }
    if port & 0b010 != 0 {
        perm |= MapPermission::W;
    }
    if port & 0b100 != 0 {
        perm |= MapPermission::X;
    }

    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if inner
        .memory_set
        .is_overlap(VirtAddr::from(start), VirtAddr::from(end))
    {
        // 与已有映射冲突，什么都不做
        return -1;
    }
    inner
        .memory_set
        .insert_framed_area(VirtAddr::from(start), VirtAddr::from(end), perm);
    0
}

/// 解除当前进程 [start, start + len) 的映射
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_munmap", current_task().unwrap().pid.0);
    if start % PAGE_SIZE != 0 || len == 0 {
        return -1;
    }
    let Some(end) = start.checked_add(len) else {
        return -1;
    };
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if inner
        .memory_set
        .unmap(VirtAddr::from(start), VirtAddr::from(end))
    {
        0
    } else {
        -1
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// 创建运行名为 `path` 的应用的新进程并返回其 pid，
/// 找不到该应用时返回 -1。
///
/// 与 `fork` 不同，这里完全不复制父进程的地址空间。
pub fn sys_spawn(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_spawn", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        let new_task = task.spawn(data);
        let new_pid = new_task.pid.0;
        // 把新任务加入调度器
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}

/// 设置当前进程的优先级并返回该优先级，出错返回 -1。
///
/// 优先级越大，`pass` 越短，得到的 CPU 时间越多；只接受大于等于 2 的优先级。
pub fn sys_set_priority(prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority [{}]",
        current_task().unwrap().pid.0,
        prio
    );
    if prio < MIN_PRIORITY {
        return -1;
    }
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    inner.priority = prio as usize;
    prio
}
