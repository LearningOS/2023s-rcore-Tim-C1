//! File and filesystem-related syscalls
use crate::fs::{open_file, OpenFlags, Stat, link, unlink, link_num};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// YOUR JOB: Implement fstat.
pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    trace!(
        "kernel:pid[{}] sys_fstat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if _fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(inode) = &inner.fd_table[_fd] {
        let ino = inode.get_ino();
        let dev = 0 as u64;
        let mode = inode.file_type();
        let links = link_num(ino as usize);

        drop(inner);
        let rst = Stat {
            dev: dev,
            ino: ino,
            mode: mode,
            nlink: links as u32,
            pad: [0;7],
            };
        let tisize = core::mem::size_of::<Stat>();
        let mut buffers = translated_byte_buffer(current_user_token(), _st as *mut u8, tisize);
        assert!(buffers.len() == 1 || buffers.len() == 2, "Can not correctly get the physical page of _ti");
        if buffers.len() == 1 {
            let buffer = buffers.get_mut(0).unwrap();
            assert!(buffer.len() == tisize);
            unsafe { buffer.copy_from_slice(core::slice::from_raw_parts((&rst as *const Stat) as *const u8, tisize)); }
        } else {
            assert!(buffers[0].len() + buffers[1].len() == tisize);
            let first_half = buffers.get_mut(0).unwrap();
            let first_half_len = first_half.len();
            unsafe {
                first_half.copy_from_slice(core::slice::from_raw_parts((&rst as *const Stat) as *const u8, first_half.len()));
            }
            let second_half = buffers.get_mut(1).unwrap();
            unsafe {
                second_half.copy_from_slice(core::slice::from_raw_parts(((&rst as *const Stat) as usize + first_half_len) as *const u8 , second_half.len()));
            }
        }
        0
    } else {
        -1
    }
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_linkat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let old = translated_str(token, _old_name);
    let new = translated_str(token, _new_name);

    link(old.as_str(), new.as_str());
    0
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_unlinkat NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _name);

    unlink(path.as_str());
    0
}
