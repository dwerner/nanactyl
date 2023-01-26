//! Implements the notion of a plugin, allowing code to be hot-swapped at
//! runtime. There are safety concerns to be aware of when using this, as a
//! plugin is merely a .so or .dll that is loaded at runtime and as-such must be
//! properly cleaned up before reloading. This includes any currently-running
//! async tasks as well as any thread-local storage. See the pthread_atexit
//! patch for more details and caveats.

mod plugin;

pub use plugin::{Plugin, PluginCheck, PluginError, RELATIVE_TARGET_DIR};

#[cfg(target_os = "linux")]
pub mod linux;
#[macro_export]
macro_rules! register_tls_dtor_hook {
    () => {
        #[cfg(target_os = "linux")]
        #[no_mangle]
        pub unsafe extern "C" fn __cxa_thread_atexit_impl(
            func: *mut ::std::ffi::c_void,
            obj: *mut ::std::ffi::c_void,
            dso_symbol: *mut ::std::ffi::c_void,
        ) {
            plugin_loader::linux::thread_atexit(func, obj, dso_symbol);
        }
    };
}
