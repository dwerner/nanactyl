use std::borrow::Cow;
use std::ffi::CStr;
use std::os::raw::c_void;
use std::sync::{Arc, Weak};
use std::{mem, ptr};

use ash::extensions::ext;
use ash::{vk, Entry};
use logger::warn;

use crate::VulkanDebug;

/// Vulkan's debug callback.
unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
    let callback_data = *p_callback_data;
    let message_id_number = callback_data.message_id_number;

    let message_id_name = if callback_data.p_message_id_name.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message_id_name).to_string_lossy()
    };

    let message = if callback_data.p_message.is_null() {
        Cow::from("")
    } else {
        CStr::from_ptr(callback_data.p_message).to_string_lossy()
    };

    if ptr::null() != user_data {
        let debug_struct = Weak::from_raw(user_data as *const VulkanDebug);
        if let Some(debug_struct) = debug_struct.upgrade() {
            warn!(
                debug_struct.logger,
                "VK({}) {:?}: {:?} [{} ({})] : {}",
                debug_struct.placeholder,
                message_severity,
                message_type,
                message_id_name,
                &message_id_number.to_string(),
                message.trim(),
            );
        }
        // We're done with this call, but Weak::from_raw takes ownership, and we don't
        // want to drop this pointer. We will be called many times with this pointer.
        mem::forget(debug_struct);
        return vk::FALSE;
    }

    println!(
        "VK {:?}: {:?} [{} ({})] : {}",
        message_severity,
        message_type,
        message_id_name,
        &message_id_number.to_string(),
        message.trim(),
    );

    vk::FALSE
}

pub(crate) fn create_debug_callback(
    entry: &Entry,
    instance: &ash::Instance,
    debug: &Arc<VulkanDebug>,
) -> (ext::DebugUtils, vk::DebugUtilsMessengerEXT) {
    let debug_utils_loader = ext::DebugUtils::new(entry, instance);

    // Just attach a placeholder userdata object for context to be passed into
    // callbacks. VulkanBase owns the Arc.
    let weak = Arc::downgrade(debug);

    // Yee haw.
    let user_data = Weak::into_raw(weak) as *mut c_void;

    let debug_call_back = {
        let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                    | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .user_data(user_data)
            .pfn_user_callback(Some(vulkan_debug_callback));

        unsafe {
            debug_utils_loader
                .create_debug_utils_messenger(&debug_info, None)
                .unwrap()
        }
    };
    (debug_utils_loader, debug_call_back)
}
