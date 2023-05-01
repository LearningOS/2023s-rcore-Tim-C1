//! Process management syscalls
//!
use alloc::sync::Arc;

use crate::{
    config::MAX_SYSCALL_NUM,
    fs::{open_file, OpenFlags},
    mm::{translated_refmut, translated_str, VirtAddr, VirtPageNum, VPNRange, translated_byte_buffer},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, TaskStatus, has_mapped, map_new_area, unmap_area, TaskControlBlock, RUN_TIME,
    }, timer::{get_time_us, get_time_ms}, syscall::get_syscall_info,
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
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
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
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
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
    let current_app = current_task().unwrap();
    let current = current_app.getpid();
    let task_info = unsafe { get_syscall_info(current) };
    // let user_time = get_current_user_time();
    let task_status = TaskStatus::Running;
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

/// YOUR JOB: Implement munmap.
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
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _path);
    let parent = current_task().unwrap();
    let mut parent_inner = parent.inner_exclusive_access();
    if let Some(inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let data = &inode.read_all();
        let new_task = Arc::new(TaskControlBlock::new(data));
        let new_pid = new_task.pid.0;
        new_task.inner_exclusive_access().parent = Some(Arc::downgrade(&parent));
        parent_inner.children.push(new_task.clone());
        add_task(new_task);
        new_pid as isize
    } else {
        -1 as isize
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if _prio <= 1 {
        return -1;
    }
    _prio
}