use cstr::cstr;
use once_cell::sync::Lazy;
use proc_maps::Pid;
use std::{collections::HashSet, ffi::c_void};

// TODO document black magic:
// Redefine and inject our own wrapper around the registration of thread-local destructors.

pub type NextFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void);

static SYSTEM_THREAD_ATEXIT: Lazy<Option<NextFn>> = Lazy::new(|| unsafe {
    #[allow(clippy::transmute_ptr_to_ref)]
    let name = cstr!("__cxa_thread_atexit_impl").as_ptr();
    std::mem::transmute(libc::dlsym(
        libc::RTLD_NEXT,
        #[allow(clippy::transmute_ptr_to_ref)]
        name,
    ))
});

/// Turns glibc's TLS destructor register function, `__cxa_thread_atexit_impl`,
///
/// # Safety
/// This needs to be public for symbol visibility reasons, but you should
/// never need to call this yourself
pub unsafe fn thread_atexit(func: *mut c_void, obj: *mut c_void, dso_symbol: *mut c_void) {
    // Default behavior, left here to provide a hook in case we want to disable thread local destructors from being registered.
    if let Some(system_thread_atexit) = *SYSTEM_THREAD_ATEXIT {
        // Just don't register dtors

        log::warn!("thread local dtor registration is disabled.");
        // system_thread_atexit(func, obj, dso_symbol);
    } else {
        // hot reloading is disabled *and* we don't have `__cxa_thread_atexit_impl`,
        // throw hands up in the air and leak memory.
    }
}

/// Check /proc/PID/maps for our process and a plugin name.
/// Returns a HashSet of plugins currently mapped to our Pid (file stem only)
pub fn distinct_plugins_mapped(module: &str) -> Vec<String> {
    let map = proc_maps::get_process_maps(std::process::id() as Pid).unwrap();
    let distinct = map
        .iter()
        .flat_map(|item| item.filename())
        .flat_map(|item| item.file_stem())
        .flat_map(|item| item.to_str().map(str::to_string))
        .filter(|item| item.contains(module))
        .collect::<HashSet<_>>();

    distinct.into_iter().collect::<Vec<_>>()
}
