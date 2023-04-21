//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the operating system.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.

mod context;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::loader::{get_app_data, get_num_app};
use crate::mm::{VirtAddr, MapPermission, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::trap::TrapContext;
use alloc::vec::Vec;
use lazy_static::*;
use crate::timer::get_time_ms;
use crate::timer::get_time_us;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;

/// ..
pub static mut RUN_TIME: usize = 0;
/// The task manager, where all the tasks are managed.
///
/// Functions implemented on `TaskManager` deals with all task state transitions
/// and task context switching. For convenience, you can find wrappers around it
/// in the module level.
///
/// Most of `TaskManager` are hidden behind the field `inner`, to defer
/// borrowing checks to runtime. You can see examples on how to use `inner` in
/// existing functions on `TaskManager`.
pub struct TaskManager {
    /// total number of tasks
    pub num_app: usize,
    /// use inner value to get mutable access
    pub inner: UPSafeCell<TaskManagerInner>,
}

/// The task manager inner in 'UPSafeCell'
pub struct TaskManagerInner {
    /// task list
    tasks: Vec<TaskControlBlock>,
    /// id of current `Running` task
    pub current_task: usize,
    /// stop watch
    pub stop_watch: usize,
}

impl TaskManagerInner {
    fn refresh_stop_watch(&mut self) -> usize {
        let start_time = self.stop_watch;
        self.stop_watch = get_time_ms();
        self.stop_watch - start_time
    }
}

lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                    stop_watch: 0,
                })
            },
        }
    };
}

impl TaskManager {
    /// Run the first task in task list.
    ///
    /// Generally, the first task in task list is an idle task (we call it zero process later).
    /// But in ch4, we load apps statically, so the first task is a real app.
    fn run_first_task(&self) -> ! {
        let mut inner = self.inner.exclusive_access();
        let next_task = &mut inner.tasks[0];
        next_task.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &next_task.task_cx as *const TaskContext;
        unsafe {RUN_TIME += inner.refresh_stop_watch();}
        drop(inner);
        let mut _unused = TaskContext::zero_init();
        // before this, we should drop local variables that must be dropped manually
        unsafe {
            __switch(&mut _unused as *mut _, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// Change the status of current `Running` task into `Ready`.
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].kernel_time += inner.refresh_stop_watch();
        inner.tasks[cur].task_status = TaskStatus::Ready;
    }

    /// Change the status of current `Running` task into `Exited`.
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].kernel_time += inner.refresh_stop_watch();
        println!("[task {} exited. user_time: {} ms, kernel_time: {} ms.]", cur, inner.tasks[cur].user_time, inner.tasks[cur].kernel_time);
        inner.tasks[cur].task_status = TaskStatus::Exited;
    }

    /// Find next task to run and return task id.
    ///
    /// In this case, we only return the first `Ready` task in task list.
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Get the current 'Running' task's token.
    fn get_current_token(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_user_token()
    }

    /// Get the current 'Running' task's trap contexts.
    fn get_current_trap_cx(&self) -> &'static mut TrapContext {
        let inner = self.inner.exclusive_access();
        inner.tasks[inner.current_task].get_trap_cx()
    }

    /// Change the current 'Running' task's program break
    pub fn change_current_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner.exclusive_access();
        let cur = inner.current_task;
        inner.tasks[cur].change_program_brk(size)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running;
            inner.current_task = next;
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr);
            }
            // go back to user mode
        } else {
            panic!("All applications completed!, total switch time {} us", get_switch_time_count());

        }
    }

    /// update user time
    fn user_time_start(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].kernel_time += inner.refresh_stop_watch();
    }

    /// update kernel time
    fn user_time_end(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].user_time += inner.refresh_stop_watch();
    }

    /// get current task user time
    fn get_current_user_time(&self) -> usize {
        let inner = self.inner.exclusive_access();
        // let current = inner.current_task;
        let mut total = 0;
        for t in &inner.tasks {
            total += t.kernel_time + t.user_time
        }
        total
    }

    /// get current task number
    fn get_current_task_num(&self) -> usize {
        let inner = self.inner.exclusive_access();
        inner.current_task
    }

    /// get current task number
    fn get_current_task_status(&self) -> TaskStatus {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status
    }
    /// map a new area
    pub fn map_new_area(&self, va_start: VirtAddr, va_end: VirtAddr, flags: u8) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let memset = &mut inner.tasks[current].memory_set;
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
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let memset = &mut inner.tasks[current].memory_set;
        memset.unmap_area(va_start);
    }

    /// check has mapped
    pub fn has_mapped(&self, vpn: VirtPageNum) -> bool {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let memset = &inner.tasks[current].memory_set;
        memset.has_mapped(vpn)
    }
}

/// Run the first task in task list.
pub fn run_first_task() {
    TASK_MANAGER.run_first_task();
}

/// Switch current `Running` task to the task we have found,
/// or there is no `Ready` task and we can exit with all applications completed
fn run_next_task() {
    TASK_MANAGER.run_next_task();
}

/// Change the status of current `Running` task into `Ready`.
fn mark_current_suspended() {
    TASK_MANAGER.mark_current_suspended();
}

/// Change the status of current `Running` task into `Exited`.
fn mark_current_exited() {
    TASK_MANAGER.mark_current_exited();
}

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}

/// Get the current 'Running' task's token.
pub fn current_user_token() -> usize {
    TASK_MANAGER.get_current_token()
}

/// Get the current 'Running' task's trap contexts.
pub fn current_trap_cx() -> &'static mut TrapContext {
    TASK_MANAGER.get_current_trap_cx()
}

/// Change the current 'Running' task's program break
pub fn change_program_brk(size: i32) -> Option<usize> {
    TASK_MANAGER.change_current_program_brk(size)
}

/// for mmap 
pub fn map_new_area(va_start: VirtAddr, va_end: VirtAddr, flags: u8) {
    TASK_MANAGER.map_new_area(va_start, va_end, flags);
}


/// for munmap
pub fn unmap_area(va_start: VirtAddr) {
    TASK_MANAGER.unmap_area(va_start);
}


/// has mapped
pub fn has_mapped(vpn: VirtPageNum) -> bool {
    TASK_MANAGER.has_mapped(vpn)
}

/// sum kernel time and start use time
pub fn user_time_start() {
    TASK_MANAGER.user_time_start()
}

/// sum user time and start kernel time
pub fn user_time_end() {
    TASK_MANAGER.user_time_end()
}

/// begin time of switch
static mut SWITCH_TIME_START: usize = 0;
/// end time of switch
static mut SWITCH_TIME_COUNT: usize = 0;
/// wrapper of __switch function
unsafe fn __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext) {
    SWITCH_TIME_START = get_time_us();
    switch::__switch(current_task_cx_ptr, next_task_cx_ptr);
    // after __switch next task will start from this line
    SWITCH_TIME_COUNT += get_time_us() - SWITCH_TIME_START;
}

/// getter of SWITCH_TIME_COUNT
fn get_switch_time_count() -> usize {
    unsafe { SWITCH_TIME_COUNT }
}

// pub fn task_syscall_info() -> (TaskStatus, [u32; MAX_SYSCALL_NUM]) {
//     // TASK_MANAGER.get_task_info()
//     (TaskStatus::Ready, [10; MAX_SYSCALL_NUM])
// }

/// wrapper
pub fn get_current_user_time() -> usize {
    TASK_MANAGER.get_current_user_time()
}

/// wrapper
pub fn get_current_task_num() -> usize {
    TASK_MANAGER.get_current_task_num()
}

/// wrapper
pub fn get_current_task_status() -> TaskStatus {
    TASK_MANAGER.get_current_task_status()
}