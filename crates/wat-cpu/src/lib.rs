#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "freebsd"
))]
mod unixlike;

/// Get the current CPU id.
pub fn get_current_cpu() -> usize {
    #[cfg(target_os = "windows")]
    return windows::current_processor_id();
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd"
    ))]
    return unixlike::current_processor_id();
}
