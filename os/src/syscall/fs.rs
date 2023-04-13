//! File and filesystem-related syscalls

// use crate::batch::get_app_bounds;
const FD_STDOUT: usize = 1;

/// write buf of length `len`  to a file with `fd`
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel: sys_write");
    // let (l, h) = get_app_bounds();
    // if !(buf as usize >= l && buf as usize <= h) {
    //     panic!("Invalid write: buf is out of current app's memory bounds");
    // }
    match fd {
        FD_STDOUT => {
            let slice = unsafe { core::slice::from_raw_parts(buf, len) };
            let str = core::str::from_utf8(slice).unwrap();
            print!("{}", str);
            len as isize
        }
        _ => {
            panic!("Unsupported fd in sys_write!");
        }
    }
}
