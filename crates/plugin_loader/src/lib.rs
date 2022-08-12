mod plugin;

pub use plugin::{Plugin, PluginCheck, PluginError};

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
