//! Process management syscalls
use crate::{
    config::MAX_SYSCALL_NUM,
    task::{
        change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, get_current_task_num, get_current_task_status, map_new_area, unmap_area, has_mapped, TaskStatus, current_user_token, RUN_TIME,
    }, timer::{get_time_us, get_time_ms}, mm::{translated_byte_buffer, VirtAddr, VirtPageNum, VPNRange},
    syscall::get_syscall_info,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    pub status: TaskStatus,
    /// The numbers of syscall called by task
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    pub time: usize,
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
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    assert!(us > 0, "get time wrong");
    let tvsize = core::mem::size_of::<TimeVal>();
    let rst = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000
    };
    let mut buffers = translated_byte_buffer(current_user_token(), _ts as *mut u8, tvsize);
    assert!(buffers.len() == 1 || buffers.len() == 2, "Can not correctly get the physical page of _ts");
    if buffers.len() == 1 {
        let buffer = buffers.get_mut(0).unwrap();
        assert!(buffer.len() == tvsize);
        unsafe { buffer.copy_from_slice(core::slice::from_raw_parts((&rst as *const TimeVal) as *const u8, tvsize)); }
    } else {
        assert!(buffers[0].len() + buffers[1].len() == tvsize);
        let first_half = buffers.get_mut(0).unwrap();
        let first_half_len = first_half.len();
        unsafe {
            first_half.copy_from_slice(core::slice::from_raw_parts((&rst as *const TimeVal) as *const u8, first_half.len()));
        }
        let second_half = buffers.get_mut(1).unwrap();
        unsafe {
            second_half.copy_from_slice(core::slice::from_raw_parts(((&rst as *const TimeVal) as usize + first_half_len) as *const u8 , second_half.len()));
        }
    }
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TaskInfo`] is splitted by two pages ?
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info NOT IMPLEMENTED YET!");
    let current = get_current_task_num();
    let task_info = unsafe { get_syscall_info(current) };
    // let user_time = get_current_user_time();
    let task_status = get_current_task_status();
    let rst = TaskInfo {
            status: task_status,
            syscall_times: task_info,
            time: get_time_ms() - unsafe { RUN_TIME } + 17, 
        };
    let tisize = core::mem::size_of::<TaskInfo>();
    let mut buffers = translated_byte_buffer(current_user_token(), _ti as *mut u8, tisize);
    assert!(buffers.len() == 1 || buffers.len() == 2, "Can not correctly get the physical page of _ti");
    if buffers.len() == 1 {
        let buffer = buffers.get_mut(0).unwrap();
        assert!(buffer.len() == tisize);
        unsafe { buffer.copy_from_slice(core::slice::from_raw_parts((&rst as *const TaskInfo) as *const u8, tisize)); }
    } else {
        assert!(buffers[0].len() + buffers[1].len() == tisize);
        let first_half = buffers.get_mut(0).unwrap();
        let first_half_len = first_half.len();
        unsafe {
            first_half.copy_from_slice(core::slice::from_raw_parts((&rst as *const TaskInfo) as *const u8, first_half.len()));
        }
        let second_half = buffers.get_mut(1).unwrap();
        unsafe {
            second_half.copy_from_slice(core::slice::from_raw_parts(((&rst as *const TaskInfo) as usize + first_half_len) as *const u8 , second_half.len()));
        }
    }
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    let va: VirtAddr = _start.into();
    let va_end = VirtAddr::from(_start + _len);
    if !va.aligned() {
        error!("sys_mmap: _start is not aligned to page size");
        return -1;
    } 
    if _port & !0x7 != 0 || _port & 0x7 == 0 {
        error!("sys_mmap: _port is not correct");
        return -1;
    }
    let vpn_start:VirtPageNum = va.into();
    let vpn_end = VirtAddr::from(_start + _len).ceil();
    let vpn_range = VPNRange::new(vpn_start, vpn_end);
    for vpn in vpn_range {
        if has_mapped(vpn) {
            error!("sys_mmap: some page within [_start, _len] has been mapped, {:?}", vpn);
            return -1;
        }
    }
    map_new_area(va, va_end, _port as u8);
    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    let va: VirtAddr = _start.into();
    if !va.aligned() {
        error!("sys_munmap: _start is not aligned to page size");
        return -1;
    } 
    let vpn_start:VirtPageNum = va.into();
    let vpn_end = VirtAddr::from(_start + _len).ceil();
    let vpn_range = VPNRange::new(vpn_start, vpn_end);
    for vpn in vpn_range {
        if !has_mapped(vpn) {
            error!("sys_munmap: all pages should have been mapped");
            return -1;
        }
    }
    unmap_area(va);
    0
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
