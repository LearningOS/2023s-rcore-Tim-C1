//! Implementation of [`Processor`] and Intersection of control flow
//!
//! Here, the continuous operation of user apps in CPU is maintained,
//! the current running state of CPU is recorded,
//! and the replacement and transfer of control flow of different applications are executed.

use super::__switch;
use super::{fetch_task, TaskStatus};
use super::{ProcessControlBlock, TaskContext, TaskControlBlock};
use crate::mm::{VirtAddr, MapPermission, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::sync::Arc;
use lazy_static::*;

/// total run time
pub static mut RUN_TIME: usize = 0;
/// Processor management structure
pub struct Processor {
    current: Option<Arc<TaskControlBlock>>,

    ///The basic control flow of each core, helping to select and switch process
    idle_task_cx: TaskContext,
}

impl Processor {
    pub fn new() -> Self {
        Self {
            current: None,
            idle_task_cx: TaskContext::zero_init(),
        }
    }

    ///Get mutable reference to `idle_task_cx`
    fn get_idle_task_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_task_cx as *mut _
    }

    ///Get current task in moving semanteme
    pub fn take_current(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.current.take()
    }

    ///Get current task in cloning semanteme
    pub fn current(&self) -> Option<Arc<TaskControlBlock>> {
        self.current.as_ref().map(Arc::clone)
    }
    /// map a new area
    pub fn map_new_area(&self, va_start: VirtAddr, va_end: VirtAddr, flags: u8) {
        let binding = current_task().unwrap().process.upgrade().unwrap();
        let memset = &mut binding.inner_exclusive_access().memory_set;
        let mut perm = MapPermission::U;
        if (flags & 0x1) > 0 {
            perm |= MapPermission::R;
        }

        if (flags & 0x2) > 0 {
            perm |= MapPermission::W;
        }

        if (flags & 0x4) > 0 {
            perm |= MapPermission::X;
        }
        memset.insert_framed_area(va_start, va_end, perm); 
    }

    /// unmap an area
    pub fn unmap_area(&self, va_start: VirtAddr) {
        let binding = current_task().unwrap().process.upgrade().unwrap();
        let memset = &mut binding.inner_exclusive_access().memory_set;
        memset.unmap_area(va_start);
    }

    /// check has mapped
    pub fn has_mapped(&self, vpn: VirtPageNum) -> bool {
        let binding = current_task().unwrap().process.upgrade().unwrap();
        let memset = &mut binding.inner_exclusive_access().memory_set;
        memset.has_mapped(vpn)
    }
}

lazy_static! {
    pub static ref PROCESSOR: UPSafeCell<Processor> = unsafe { UPSafeCell::new(Processor::new()) };
}

///The main part of process execution and scheduling
///Loop `fetch_task` to get the process that needs to run, and switch the process through `__switch`
pub fn run_tasks() {
    loop {
        let mut processor = PROCESSOR.exclusive_access();
        if let Some(task) = fetch_task() {
            let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
            // access coming task TCB exclusively
            let mut task_inner = task.inner_exclusive_access();
            let next_task_cx_ptr = &task_inner.task_cx as *const TaskContext;
            task_inner.task_status = TaskStatus::Running;
            // release coming task_inner manually
            drop(task_inner);
            // release coming task TCB manually
            processor.current = Some(task);
            // release processor manually
            drop(processor);
            unsafe {
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
        } else {
            warn!("no tasks available in run_tasks");
        }
    }
}

/// Get current task through take, leaving a None in its place
pub fn take_current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().take_current()
}

/// Get a copy of the current task
pub fn current_task() -> Option<Arc<TaskControlBlock>> {
    PROCESSOR.exclusive_access().current()
}

/// get current process
pub fn current_process() -> Arc<ProcessControlBlock> {
    current_task().unwrap().process.upgrade().unwrap()
}

/// Get the current user token(addr of page table)
pub fn current_user_token() -> usize {
    let task = current_task().unwrap();
    task.get_user_token()
}

/// Get the mutable reference to trap context of current task
pub fn current_trap_cx() -> &'static mut TrapContext {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .get_trap_cx()
}

/// get the user virtual address of trap context
pub fn current_trap_cx_user_va() -> usize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .trap_cx_user_va()
}

/// get the top addr of kernel stack
pub fn current_kstack_top() -> usize {
    current_task().unwrap().kstack.get_top()
}

/// Return to idle control flow for new scheduling
pub fn schedule(switched_task_cx_ptr: *mut TaskContext) {
    let mut processor = PROCESSOR.exclusive_access();
    let idle_task_cx_ptr = processor.get_idle_task_cx_ptr();
    drop(processor);
    unsafe {
        __switch(switched_task_cx_ptr, idle_task_cx_ptr);
    }
}
/// for mmap 
pub fn map_new_area(va_start: VirtAddr, va_end: VirtAddr, flags: u8) {
    PROCESSOR.exclusive_access().map_new_area(va_start, va_end, flags);
}


/// for munmap
pub fn unmap_area(va_start: VirtAddr) {
    PROCESSOR.exclusive_access().unmap_area(va_start);
}


/// has mapped
pub fn has_mapped(vpn: VirtPageNum) -> bool {
    PROCESSOR.exclusive_access().has_mapped(vpn)
}