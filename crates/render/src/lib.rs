use std::borrow::Cow;
use std::ffi::{CStr, CString};
use std::ops::{Deref, DerefMut};
use std::{sync::Arc, time::Duration};

use ash::extensions::ext;
use ash::vk;
use ash::{
    extensions::khr::{Surface, Swapchain},
    Device, Entry,
};

use async_lock::{Mutex, MutexGuardArc};
use platform::WinPtr;
use world::World;

pub mod types;

#[derive(Debug)]
pub struct Drawable {
    pub id: world::Identity,
    pub rendered: Duration,
}

#[derive(thiserror::Error, Debug)]
pub enum RenderStateError {
    #[error("plugin error {0:?}")]
    PluginError(Box<dyn std::error::Error + Send + Sync>),
}

pub struct RenderState {
    // pub entities: Vec<Drawable>,
    pub updates: u64,
    pub win_ptr: WinPtr,
    vulkan: VulkanRendererState,
}

impl RenderState {
    pub fn set_base(&mut self, base: VulkanBase) {
        self.vulkan.base = Some(base);
    }

    // Eventually, VulkanBase and VulkanBaseWrapper join together, and this base & presenter pair go away
    pub fn cleanup_base_and_presenter(&mut self) {
        if let (Some(mut presenter), Some(mut base)) =
            (self.vulkan.presenter.take(), self.vulkan.base.take())
        {
            presenter.drop_resources(&mut base);
        }
    }

    pub fn take_presenter(&mut self) -> Option<Box<dyn Presenter + Send + Sync>> {
        self.vulkan.presenter.take()
    }

    pub fn take_base(&mut self) -> Option<VulkanBase> {
        self.vulkan.base.take()
    }

    pub fn present(&mut self) {
        if let (Some(present), Some(base)) = (&self.vulkan.presenter, &mut self.vulkan.base) {
            present.present(base);
            return;
        }
        println!("present called with no renderer assigned");
    }
    // pub fn upload_and_track_model(&mut self, model: &model::Model) -> Result<(), RenderStateError> {
    // }
    pub fn set_presenter(&mut self, presenter: Box<dyn Presenter + Send + Sync>) {
        self.vulkan.presenter = Some(presenter);
    }

    pub fn create_base(&mut self) -> Result<VulkanBase, RenderStateError> {
        Ok(VulkanBase::new(self.win_ptr))
    }

    pub fn new(win_ptr: WinPtr) -> Self {
        Self {
            updates: 0,
            win_ptr,
            vulkan: VulkanRendererState::default(),
        }
    }

    pub fn into_shared(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }
}

impl Drop for RenderState {
    fn drop(&mut self) {
        self.cleanup_base_and_presenter();
    }
}

#[derive(Default)]
pub struct VulkanRendererState {
    pub base: Option<VulkanBase>,
    pub presenter: Option<Box<dyn Presenter + Send + Sync>>,
}

pub trait Presenter {
    fn present(&self, base: &mut VulkanBase);
    fn drop_resources(&mut self, base: &mut VulkanBase);
}

pub struct VulkanBase {
    pub win_ptr: platform::WinPtr,
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub device: Device,
    pub surface_loader: Surface,
    pub swapchain_loader: Swapchain,

    pub physical_device: vk::PhysicalDevice,
    pub device_memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub queue_family_index: u32,
    pub present_queue: vk::Queue,

    pub surface: vk::SurfaceKHR,
    pub surface_format: vk::SurfaceFormatKHR,
    pub surface_resolution: vk::Extent2D,

    pub swapchain: vk::SwapchainKHR,
    pub present_images: Vec<vk::Image>,
    pub present_image_views: Vec<vk::ImageView>,

    pub pool: vk::CommandPool,
    pub draw_command_buffer: vk::CommandBuffer,
    pub setup_command_buffer: vk::CommandBuffer,

    pub depth_image: vk::Image,
    pub depth_image_view: vk::ImageView,
    pub depth_image_memory: vk::DeviceMemory,

    pub present_complete_semaphore: vk::Semaphore,
    pub rendering_complete_semaphore: vk::Semaphore,

    pub draw_commands_reuse_fence: vk::Fence,
    pub setup_commands_reuse_fence: vk::Fence,

    pub debug_utils_loader: Option<ext::DebugUtils>,
    pub debug_call_back: Option<vk::DebugUtilsMessengerEXT>,
}

impl VulkanBase {
    pub fn new(win_ptr: platform::WinPtr) -> Self {
        let entry = unsafe { Entry::load() }.expect("unable to load vulkan");
        let application_info = &vk::ApplicationInfo {
            api_version: vk::make_api_version(0, 1, 0, 0),
            ..Default::default()
        };

        let mut required_extension_names = ash_window::enumerate_required_extensions(&win_ptr)
            .unwrap()
            .to_vec();

        let layer_names = vec![CString::new("VK_LAYER_KHRONOS_validation").unwrap()];
        let layers_names_raw: Vec<*const i8> = layer_names
            .iter()
            .map(|raw_name| raw_name.as_ptr())
            .collect();

        required_extension_names.push(ext::DebugUtils::name().as_ptr());

        //TODO: setup VK_LAYER_KHRONOS_validation
        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(application_info)
            .enabled_layer_names(&layers_names_raw)
            .enabled_extension_names(&required_extension_names)
            .build();

        let instance = unsafe { entry.create_instance(&create_info, None) }.unwrap();

        let debug_utils_loader = ext::DebugUtils::new(&entry, &instance);
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
                .pfn_user_callback(Some(vulkan_debug_callback));

            unsafe {
                debug_utils_loader
                    .create_debug_utils_messenger(&debug_info, None)
                    .unwrap()
            }
        };

        let surface =
            unsafe { ash_window::create_surface(&entry, &instance, &win_ptr, None) }.unwrap();
        let physical_devices = unsafe { instance.enumerate_physical_devices() }.unwrap();
        let surface_loader = Surface::new(&entry, &instance);
        let (physical_device, queue_family_index) = physical_devices
            .iter()
            .find_map(|p| {
                unsafe { instance.get_physical_device_queue_family_properties(*p) }
                    .iter()
                    .enumerate()
                    .find_map(|(index, info)| {
                        if info.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                            && unsafe {
                                surface_loader.get_physical_device_surface_support(
                                    *p,
                                    index as u32,
                                    surface,
                                )
                            }
                            .unwrap()
                        {
                            Some((p, index as u32))
                        } else {
                            None
                        }
                    })
            })
            .expect("couldn't find suitable device");
        let device_extension_names_raw = [Swapchain::name().as_ptr()];
        let features = vk::PhysicalDeviceFeatures {
            shader_clip_distance: 1,
            ..Default::default()
        };
        let priorities = [1.0];
        let queue_create_infos = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities)
            .build();
        let device_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&[queue_create_infos])
            .enabled_extension_names(&device_extension_names_raw)
            .enabled_features(&features)
            .build();
        let device =
            unsafe { instance.create_device(*physical_device, &device_create_info, None) }.unwrap();

        let present_queue = unsafe { device.get_device_queue(queue_family_index as u32, 0) };
        let surface_format = unsafe {
            surface_loader.get_physical_device_surface_formats(*physical_device, surface)
        }
        .unwrap()[0];
        let surface_capabilities = unsafe {
            surface_loader.get_physical_device_surface_capabilities(*physical_device, surface)
        }
        .unwrap();

        let desired_image_count =
            (surface_capabilities.min_image_count + 1).max(surface_capabilities.max_image_count);

        let surface_resolution = surface_capabilities.current_extent;
        let pre_transform = surface_capabilities.current_transform;
        let present_modes = unsafe {
            surface_loader.get_physical_device_surface_present_modes(*physical_device, surface)
        }
        .unwrap();
        let present_mode = present_modes
            .iter()
            .cloned()
            .find(|&mode| mode == vk::PresentModeKHR::MAILBOX)
            .unwrap_or(vk::PresentModeKHR::FIFO);

        println!("present_mode: {present_mode:?}");

        let swapchain_loader = Swapchain::new(&instance, &device);
        let swapchain_create_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(surface)
            .min_image_count(desired_image_count)
            .image_color_space(surface_format.color_space)
            .image_format(surface_format.format)
            .image_extent(surface_resolution)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(pre_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .image_array_layers(1)
            .build();

        let swapchain =
            unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None) }.unwrap();

        let pool_create_info = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family_index)
            .build();

        let pool = unsafe { device.create_command_pool(&pool_create_info, None) }.unwrap();
        let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(2)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .build();

        let command_buffers =
            unsafe { device.allocate_command_buffers(&command_buffer_allocate_info) }.unwrap();
        let setup_command_buffer = command_buffers[0];
        let draw_command_buffer = command_buffers[1];

        let present_images = unsafe { swapchain_loader.get_swapchain_images(swapchain) }.unwrap();
        let present_image_views: Vec<vk::ImageView> = present_images
            .iter()
            .map(|&image| {
                let create_view_info = vk::ImageViewCreateInfo::builder()
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(surface_format.format)
                    .components(vk::ComponentMapping {
                        r: vk::ComponentSwizzle::R,
                        g: vk::ComponentSwizzle::G,
                        b: vk::ComponentSwizzle::B,
                        a: vk::ComponentSwizzle::A,
                    })
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .image(image);
                unsafe { device.create_image_view(&create_view_info, None) }.unwrap()
            })
            .collect();
        let device_memory_properties =
            unsafe { instance.get_physical_device_memory_properties(*physical_device) };
        let depth_image_create_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::D16_UNORM)
            .extent(surface_resolution.into())
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);

        let depth_image = unsafe { device.create_image(&depth_image_create_info, None) }.unwrap();
        let depth_image_memory_req = unsafe { device.get_image_memory_requirements(depth_image) };
        let depth_image_memory_index = Self::find_memorytype_index(
            &depth_image_memory_req,
            &device_memory_properties,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
        .unwrap();
        let depth_image_allocate_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(depth_image_memory_req.size)
            .memory_type_index(depth_image_memory_index)
            .build();

        let depth_image_memory =
            unsafe { device.allocate_memory(&depth_image_allocate_info, None) }.unwrap();

        unsafe { device.bind_image_memory(depth_image, depth_image_memory, 0) }.unwrap();

        let fence_create_info = vk::FenceCreateInfo::builder()
            .flags(vk::FenceCreateFlags::SIGNALED)
            .build();

        let draw_commands_reuse_fence =
            unsafe { device.create_fence(&fence_create_info, None) }.unwrap();
        let setup_commands_reuse_fence =
            unsafe { device.create_fence(&fence_create_info, None) }.unwrap();

        Self::record_submit_commandbuffer(
            &device,
            setup_command_buffer,
            setup_commands_reuse_fence,
            present_queue,
            &[],
            &[],
            &[],
            |device, setup_command_buffer| {
                let layout_transition_barriers = vk::ImageMemoryBarrier::builder()
                    .image(depth_image)
                    .dst_access_mask(
                        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    )
                    .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .subresource_range(
                        vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::DEPTH)
                            .layer_count(1)
                            .level_count(1)
                            .build(),
                    )
                    .build();

                unsafe {
                    device.cmd_pipeline_barrier(
                        setup_command_buffer,
                        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                        vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[layout_transition_barriers],
                    );
                }
            },
        );

        let depth_image_view_info = vk::ImageViewCreateInfo::builder()
            .subresource_range(
                vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::DEPTH)
                    .level_count(1)
                    .layer_count(1)
                    .build(),
            )
            .image(depth_image)
            .format(depth_image_create_info.format)
            .view_type(vk::ImageViewType::TYPE_2D)
            .build();

        let depth_image_view =
            unsafe { device.create_image_view(&depth_image_view_info, None) }.unwrap();

        let semaphore_create_info = vk::SemaphoreCreateInfo::default();

        let present_complete_semaphore =
            unsafe { device.create_semaphore(&semaphore_create_info, None) }.unwrap();
        let rendering_complete_semaphore =
            unsafe { device.create_semaphore(&semaphore_create_info, None) }.unwrap();

        Self {
            win_ptr,
            entry,
            instance,
            device,
            queue_family_index,
            physical_device: *physical_device,
            device_memory_properties,
            surface_loader,
            surface_format,
            present_queue,
            surface_resolution,
            swapchain_loader,
            swapchain,
            present_images,
            present_image_views,
            pool,
            draw_command_buffer,
            setup_command_buffer,
            depth_image,
            depth_image_view,
            present_complete_semaphore,
            rendering_complete_semaphore,
            draw_commands_reuse_fence,
            setup_commands_reuse_fence,
            surface,
            depth_image_memory,
            debug_utils_loader: Some(debug_utils_loader),
            debug_call_back: Some(debug_call_back),
        }
    }

    pub fn find_memorytype_index(
        memory_reqs: &vk::MemoryRequirements,
        memory_properties: &vk::PhysicalDeviceMemoryProperties,
        flags: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        memory_properties.memory_types[..memory_properties.memory_type_count as usize]
            .iter()
            .enumerate()
            .find_map(|(index, memory_type)| {
                if (1 << index) & memory_reqs.memory_type_bits != 0
                    && memory_type.property_flags & flags == flags
                {
                    Some(index as u32)
                } else {
                    None
                }
            })
    }

    pub fn record_submit_commandbuffer<F>(
        device: &Device,
        command_buffer: vk::CommandBuffer,
        command_buffer_reuse_fence: vk::Fence,
        submit_queue: vk::Queue,
        wait_mask: &[vk::PipelineStageFlags],
        wait_semaphores: &[vk::Semaphore],
        signal_semaphores: &[vk::Semaphore],
        command_buffer_fn: F,
    ) where
        F: FnOnce(&Device, vk::CommandBuffer),
    {
        unsafe {
            device
                .wait_for_fences(&[command_buffer_reuse_fence], true, u64::MAX)
                .unwrap();
            device.reset_fences(&[command_buffer_reuse_fence]).unwrap();
        }
        let command_buffer_begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
            .build();
        unsafe { device.begin_command_buffer(command_buffer, &command_buffer_begin_info) }.unwrap();

        command_buffer_fn(device, command_buffer);

        unsafe { device.end_command_buffer(command_buffer) }.unwrap();

        let command_buffers = vec![command_buffer];
        let submit_info = vk::SubmitInfo::builder()
            .wait_semaphores(wait_semaphores)
            .wait_dst_stage_mask(wait_mask)
            .command_buffers(&command_buffers)
            .signal_semaphores(signal_semaphores)
            .build();

        unsafe { device.queue_submit(submit_queue, &[submit_info], command_buffer_reuse_fence) }
            .unwrap();
    }
}

impl Drop for VulkanBase {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().unwrap();
            self.device
                .destroy_semaphore(self.present_complete_semaphore, None);
            self.device
                .destroy_semaphore(self.rendering_complete_semaphore, None);
            self.device
                .destroy_fence(self.draw_commands_reuse_fence, None);
            self.device
                .destroy_fence(self.setup_commands_reuse_fence, None);
            self.device.free_memory(self.depth_image_memory, None);
            self.device.destroy_image_view(self.depth_image_view, None);
            self.device.destroy_image(self.depth_image, None);
            for &image_view in self.present_image_views.iter() {
                self.device.destroy_image_view(image_view, None);
            }
            self.device.destroy_command_pool(self.pool, None);
            self.swapchain_loader
                .destroy_swapchain(self.swapchain, None);
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);

            if let Some((debug_utils, call_back)) =
                Option::zip(self.debug_utils_loader.take(), self.debug_call_back.take())
            {
                debug_utils.destroy_debug_utils_messenger(call_back, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

// TODO: consider a generic version of this?
/// Acts as a combiner for Mutex, locking both mutexes but also releases both mutexes when dropped.
pub struct WorldRenderState {
    render_state: MutexGuardArc<RenderState>,
    world: MutexGuardArc<World>,
}

impl WorldRenderState {
    pub async fn new(world: &Arc<Mutex<World>>, render_state: &Arc<Mutex<RenderState>>) -> Self {
        let world = Arc::clone(world).lock_arc().await;
        let render_state = Arc::clone(render_state).lock_arc().await;
        Self {
            world,
            render_state,
        }
    }

    pub fn world(&mut self) -> &World {
        self.world.deref()
    }

    pub fn render_state(&mut self) -> &mut RenderState {
        self.render_state.deref_mut()
    }
}

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::os::raw::c_void,
) -> vk::Bool32 {
    let callback_data = *p_callback_data;
    let message_id_number: i32 = callback_data.message_id_number as i32;

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
