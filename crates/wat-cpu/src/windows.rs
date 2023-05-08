use winapi::um::winbase::GetCurrentProcessorNumber;

/// Windows implementation of `current_processor_id`.
pub fn current_processor_id() -> usize {
    unsafe { GetCurrentProcessorNumber() as usize }
}
