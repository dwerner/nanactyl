use libc::sched_getcpu;

/// Linux implementation of `current_processor_id`.
pub fn current_processor_id() -> usize {
    unsafe { sched_getcpu() as usize }
}
