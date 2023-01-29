//! In support of ash_rendering_plugin, implements various wrappers over
//! vulkan/ash that are used in the plugin.
//!
//! This module is a landing-pad (In particular VulkanBase) for functionality
//! from

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, CString};
use std::mem;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ash::extensions::ext;
use ash::extensions::khr::{Surface, Swapchain};
use ash::{vk, Device, Entry};
use async_lock::{Mutex, MutexGuardArc};
use nalgebra::Vector3;
use platform::WinPtr;
use types::{Attachments, AttachmentsModifier, GpuModelRef, VulkanError};
use world::thing::{CameraFacet, CameraIndex, ModelIndex, PhysicalFacet, PhysicalIndex};
use world::{Identity, World};

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
    #[error("model upload error")]
    ModelUploadTODO,
}

#[derive(thiserror::Error, Debug)]
pub enum SceneError {
    #[error("query error {0}")]
    Query(#[from] SceneQueryError),
    #[error("world error {0:?}")]
    World(world::WorldError),
}

#[derive(thiserror::Error, Debug)]
pub enum SceneQueryError {
    #[error("thing with id {0:?} not found in scene")]
    ThingNotFound(Identity),
    #[error("no phys facet at index {0:?}")]
    NoSuchPhys(PhysicalIndex),
    #[error("no camera facet at index {0:?}")]
    NoSuchCamera(CameraIndex),
}

/// "Declarative" style api attempt - don't expose any renderer details/buffers,
/// instead have RenderState track them
pub struct RenderState {
    pub updates: u64,
    pub win_ptr: WinPtr,
    vulkan: VulkanRendererState,
    pub enable_validation_layer: bool,
    model_upload_queue: VecDeque<(ModelIndex, models::Model)>,
    pub scene: RenderScene,
}

impl RenderState {
    pub fn new(win_ptr: WinPtr, enable_validation_layer: bool, is_server: bool) -> Self {
        Self {
            updates: 0,
            win_ptr,
            vulkan: VulkanRendererState::default(),
            enable_validation_layer,
            scene: RenderScene {
                active_camera: if is_server { 0 } else { 1 },
                cameras: vec![],
                drawables: vec![],
            },
            model_upload_queue: Default::default(),
        }
    }

    pub fn into_shared(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    /// Queue a model for upload to the GPU. This is done by adding it to a
    /// queue on RenderState, and from there it is uploaded at a plugin-defined
    /// point.
    pub fn queue_model_for_upload(
        &mut self,
        index: ModelIndex,
        model: models::Model,
    ) -> Result<(), RenderStateError> {
        self.model_upload_queue.push_front((index, model));
        Ok(())
    }

    // Eventually, VulkanBase and VulkanBaseWrapper join together, and this base &
    // presenter pair go away
    pub fn set_base(&mut self, base: VulkanBase) {
        self.vulkan.base = Some(base);
    }

    pub fn base_mut(&mut self) -> Option<&mut VulkanBase> {
        self.vulkan.base.as_mut()
    }

    /// Calls `Presenter`'s drop_resources hook to clean up.
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

    pub fn drain_upload_queue(&mut self) -> VecDeque<(ModelIndex, models::Model)> {
        std::mem::take(&mut self.model_upload_queue)
    }

    pub fn tracked_model(&mut self, index: ModelIndex) -> Option<Instant> {
        self.vulkan
            .base
            .as_ref()?
            .tracked_models
            .get(&index)
            .map(|(_, tracked_instant)| *tracked_instant)
    }

    pub fn queued_model(&self, index: ModelIndex) -> bool {
        self.model_upload_queue
            .iter()
            .any(|(queued_idx, _)| index == *queued_idx)
    }

    pub fn present(&mut self) {
        if let (Some(present), Some(base)) = (&mut self.vulkan.presenter, &mut self.vulkan.base) {
            present.present(base, &self.scene);
            return;
        }
        println!("present called with no renderer assigned");
    }

    pub fn update_resources(&mut self) {
        if let (Some(present), Some(base)) = (&mut self.vulkan.presenter, &mut self.vulkan.base) {
            present.update_resources(base);
            return;
        }
        println!("update_resources called with no renderer assigned");
    }

    pub fn set_presenter(&mut self, presenter: Box<dyn Presenter + Send + Sync>) {
        self.vulkan.presenter = Some(presenter);
    }

    pub fn update_scene(&mut self, scene: RenderScene) -> Result<(), SceneError> {
        self.scene.drawables = scene.drawables;
        self.scene.cameras = scene.cameras;
        Ok(())
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
    fn present(&mut self, base: &mut VulkanBase, scene: &RenderScene);
    fn update_resources(&mut self, base: &mut VulkanBase);
    fn drop_resources(&mut self, base: &mut VulkanBase);
}

#[derive(Debug, Copy, Clone)]
pub struct TextureId(u32);

impl TextureId {
    pub fn new(value: u32) -> Self {
        Self(value)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum TextureUploaderError {
    #[error("queue send error")]
    QueueSend,
    #[error("queue send error")]
    QueueRecv,
}

/// VulkanBase - ahead-of-runtime base functionality for the Vulkan plugin. The
/// idea here is to keep the facilities as generic as is reasonable and provide
/// them to `ash_renderer_plugin`. In essence, what doesn't change much stays
/// here, while rapidly moving/changing code should live in the plugin until it
/// stabilizes into generic functionality.
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
    pub draw_cmd_buf: vk::CommandBuffer,
    pub setup_command_buffer: vk::CommandBuffer,

    pub depth_image: vk::Image,
    pub depth_image_view: vk::ImageView,
    pub depth_image_memory: vk::DeviceMemory,

    pub present_complete_semaphore: vk::Semaphore,
    pub rendering_complete_semaphore: vk::Semaphore,

    pub draw_commands_reuse_fence: vk::Fence,
    pub setup_commands_reuse_fence: vk::Fence,

    pub maybe_debug_utils_loader: Option<ext::DebugUtils>,
    pub maybe_debug_call_back: Option<vk::DebugUtilsMessengerEXT>,

    pub tracked_models: HashMap<ModelIndex, (GpuModelRef, Instant)>,

    pub framebuffers: Vec<vk::Framebuffer>,
    pub render_pass: vk::RenderPass,

    pub flag_recreate_swapchain: bool,
}

impl VulkanBase {
    /// Create attachments for renderpass construction
    pub fn create_attachments(
        format: vk::Format,
    ) -> (
        Attachments,
        Vec<vk::AttachmentReference>,
        vk::AttachmentReference,
    ) {
        let mut attachments = Attachments::default();
        let color_attachment_refs = AttachmentsModifier::new(&mut attachments)
            .add_attachment(
                vk::AttachmentDescription::builder()
                    .format(format)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                    .build(),
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            )
            .into_refs();
        let depth_attachment_ref = AttachmentsModifier::new(&mut attachments).add_single(
            vk::AttachmentDescription::builder()
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .format(vk::Format::D16_UNORM)
                .initial_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                .build(),
            vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        );
        (attachments, color_attachment_refs, depth_attachment_ref)
    }

    /// Create a renderpass with attachments.
    pub fn create_render_pass(
        device: &ash::Device,
        all_attachments: &[vk::AttachmentDescription],
        color_attachment_refs: &[vk::AttachmentReference],
        depth_attachment_ref: &vk::AttachmentReference,
    ) -> Result<vk::RenderPass, VulkanError> {
        let dependencies = [vk::SubpassDependency::builder()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            )
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .build()];
        let subpass = vk::SubpassDescription::builder()
            .color_attachments(color_attachment_refs)
            .depth_stencil_attachment(depth_attachment_ref)
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .build();
        let subpasses = vec![subpass];
        let renderpass_create_info = vk::RenderPassCreateInfo::builder()
            .attachments(all_attachments)
            .subpasses(&subpasses)
            .dependencies(&dependencies)
            .build();

        unsafe { device.create_render_pass(&renderpass_create_info, None) }
            .map_err(VulkanError::VkResultToDo)
    }

    /// Create framebuffers needed, consumes the renderpass. Returns an error
    /// when unable to create framebuffers.
    pub fn create_framebuffers(
        device: &ash::Device,
        depth_image_view: vk::ImageView,
        present_image_views: &[vk::ImageView],
        render_pass: vk::RenderPass,
        surface_resolution: vk::Extent2D,
    ) -> Result<Vec<vk::Framebuffer>, VulkanError> {
        let mut framebuffers = Vec::new();
        println!("creating new framebuffers with extent {surface_resolution:?}");
        for present_image_view in present_image_views.iter() {
            let framebuffer_attachments = [*present_image_view, depth_image_view];
            let frame_buffer_create_info = vk::FramebufferCreateInfo::builder()
                .render_pass(render_pass)
                .attachments(&framebuffer_attachments)
                .width(surface_resolution.width)
                .height(surface_resolution.height)
                .layers(1);

            let framebuffer = unsafe { device.create_framebuffer(&frame_buffer_create_info, None) }
                .map_err(VulkanError::VkResultToDo)?;
            framebuffers.push(framebuffer);
        }
        Ok(framebuffers)
    }
    /// Track a model reference for cleanup when VulkanBase is dropped.
    pub fn track_uploaded_model(&mut self, index: ModelIndex, model_ref: GpuModelRef) {
        if let Some((existing_model, _instant)) = self
            .tracked_models
            .insert(index, (model_ref, Instant::now()))
        {
            existing_model.deallocate(self);
        }
    }

    /// Get a handle to the GPU-tracked data for a given model. Returns `None`
    /// if not tracked yet, and must be uploaded with `track_uploaded_model`
    /// first.
    pub fn get_tracked_model(&self, index: impl Into<ModelIndex>) -> Option<&GpuModelRef> {
        self.tracked_models.get(&index.into()).map(|(gpu, _)| gpu)
    }

    /// Create a new instance of VulkanBase, takes a platform::WinPtr and some
    /// flags. This allows each window created by a process to be injected
    /// into the renderer intended to bind it. Returns an error when the
    /// instance cannot be created.
    pub fn new(
        win_ptr: platform::WinPtr,
        enable_validation_layer: bool,
    ) -> Result<Self, VulkanError> {
        let entry = unsafe { Entry::load() }.expect("unable to load vulkan");
        let application_info = &vk::ApplicationInfo {
            api_version: vk::make_api_version(0, 1, 0, 0),
            ..Default::default()
        };

        let mut required_extension_names = ash_window::enumerate_required_extensions(&win_ptr)
            .unwrap()
            .to_vec();

        // TODO: make validation optional as this layer won't exist on most systems if
        // the Vulkan SDK isn't installed
        let layer_names = if enable_validation_layer {
            vec![CString::new("VK_LAYER_KHRONOS_validation").unwrap()]
        } else {
            vec![]
        };
        let layers_names_raw: Vec<*const i8> = layer_names
            .iter()
            .map(|raw_name| raw_name.as_ptr())
            .collect();

        required_extension_names.push(ext::DebugUtils::name().as_ptr());

        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(application_info)
            .enabled_layer_names(&layers_names_raw)
            .enabled_extension_names(&required_extension_names)
            .build();

        let instance = unsafe { entry.create_instance(&create_info, None) }.unwrap();

        let (maybe_debug_utils_loader, maybe_debug_call_back) = {
            if enable_validation_layer {
                let (debug_utils_loader, debug_call_back) =
                    create_debug_callback(&entry, &instance);
                (Some(debug_utils_loader), Some(debug_call_back))
            } else {
                (None, None)
            }
        };

        let surface =
            unsafe { ash_window::create_surface(&entry, &instance, &win_ptr, None) }.unwrap();

        let physical_devices = unsafe { instance.enumerate_physical_devices() }.unwrap();
        let surface_loader = Surface::new(&entry, &instance);
        let (physical_device, queue_family_index) =
            surface_loader_physical_device(&physical_devices, &instance, &surface_loader, surface)
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

        let present_queue = unsafe { device.get_device_queue(queue_family_index, 0) };
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

        Self::record_and_submit_commandbuffer(
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

        let (attachments, color, depth) = Self::create_attachments(surface_format.format);
        let render_pass = Self::create_render_pass(&device, attachments.all(), &color, &depth)?;
        let framebuffers = Self::create_framebuffers(
            &device,
            depth_image_view,
            &present_image_views,
            render_pass,
            surface_resolution,
        )
        .unwrap();

        Ok(Self {
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
            draw_cmd_buf: draw_command_buffer,
            setup_command_buffer,
            depth_image,
            depth_image_view,
            present_complete_semaphore,
            rendering_complete_semaphore,
            draw_commands_reuse_fence,
            setup_commands_reuse_fence,
            surface,
            depth_image_memory,
            maybe_debug_utils_loader,
            maybe_debug_call_back,
            tracked_models: HashMap::new(),
            framebuffers,
            render_pass,
            flag_recreate_swapchain: false,
        })
    }

    /// Re-create the swapchain bound. Useful when window properties change, on
    /// resize, fullscreen, focus, etc.
    pub fn recreate_swapchain(&mut self) -> Result<(), VulkanError> {
        let surface_loader = Surface::new(&self.entry, &self.instance);
        let old_surface_loader = mem::replace(&mut self.surface_loader, surface_loader);

        let surface =
            unsafe { ash_window::create_surface(&self.entry, &self.instance, &self.win_ptr, None) }
                .map_err(VulkanError::VkResultToDo)?;
        let old_surface = mem::replace(&mut self.surface, surface);

        let physical_devices = unsafe { self.instance.enumerate_physical_devices() }
            .map_err(VulkanError::EnumeratePhysicalDevices)?;
        let (physical_device, queue_family_index) = surface_loader_physical_device(
            &physical_devices,
            &self.instance,
            &self.surface_loader,
            self.surface,
        )
        .expect("couldn't find suitable device");
        self.queue_family_index = queue_family_index;
        self.physical_device = *physical_device;

        let surface_capabilities = unsafe {
            self.surface_loader
                .get_physical_device_surface_capabilities(self.physical_device, self.surface)
        }
        .map_err(VulkanError::VkResultToDo)?;

        let desired_image_count =
            (surface_capabilities.min_image_count + 1).max(surface_capabilities.max_image_count);
        let surface_resolution = surface_capabilities.current_extent;

        println!("recreate_swapchain with surface resolution {surface_resolution:?}");
        self.surface_resolution = surface_resolution;

        let pre_transform = surface_capabilities.current_transform;
        let present_modes = unsafe {
            self.surface_loader
                .get_physical_device_surface_present_modes(self.physical_device, self.surface)
        }
        .unwrap();

        let present_mode = present_modes
            .iter()
            .cloned()
            .find(|&mode| mode == vk::PresentModeKHR::MAILBOX)
            .unwrap_or(vk::PresentModeKHR::FIFO);
        println!("recreate with present mode {present_mode:?}");
        let swapchain_loader = Swapchain::new(&self.instance, &self.device);
        let old_swapchain_loader = mem::replace(&mut self.swapchain_loader, swapchain_loader);

        let swapchain_create_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(self.surface)
            .min_image_count(desired_image_count)
            .image_color_space(self.surface_format.color_space)
            .image_format(self.surface_format.format)
            .image_extent(self.surface_resolution)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(pre_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .image_array_layers(1)
            .old_swapchain(self.swapchain)
            .build();

        let swapchain = unsafe {
            self.swapchain_loader
                .create_swapchain(&swapchain_create_info, None)
        }
        .unwrap();

        let old_swapchain = mem::replace(&mut self.swapchain, swapchain);

        let present_images =
            unsafe { self.swapchain_loader.get_swapchain_images(swapchain) }.unwrap();
        self.present_images = present_images;

        println!(
            "recreating {} present image views",
            self.present_images.len()
        );
        let present_image_views = self
            .present_images
            .iter()
            .map(|&image| {
                let create_view_info = vk::ImageViewCreateInfo::builder()
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(self.surface_format.format)
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
                unsafe { self.device.create_image_view(&create_view_info, None) }
                    .map_err(VulkanError::VkResultToDo)
                    .unwrap()
            })
            .collect();
        let old_present_image_views =
            mem::replace(&mut self.present_image_views, present_image_views);
        self.device_memory_properties = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical_device)
        };
        let depth_image_create_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::D16_UNORM)
            .extent(self.surface_resolution.into())
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let depth_image =
            unsafe { self.device.create_image(&depth_image_create_info, None) }.unwrap();
        let old_depth_image = mem::replace(&mut self.depth_image, depth_image);

        let depth_image_memory_req =
            unsafe { self.device.get_image_memory_requirements(self.depth_image) };
        let depth_image_memory_index = Self::find_memorytype_index(
            &depth_image_memory_req,
            &self.device_memory_properties,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
        .unwrap();

        let depth_image_allocate_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(depth_image_memory_req.size)
            .memory_type_index(depth_image_memory_index)
            .build();

        let depth_image_memory = unsafe {
            self.device
                .allocate_memory(&depth_image_allocate_info, None)
        }
        .map_err(VulkanError::VkResultToDo)?;
        unsafe {
            self.device
                .bind_image_memory(self.depth_image, depth_image_memory, 0)
        }
        .map_err(VulkanError::VkResultToDo)?;
        let old_depth_image_memory = mem::replace(&mut self.depth_image_memory, depth_image_memory);

        Self::record_and_submit_commandbuffer(
            &self.device,
            self.setup_command_buffer,
            self.setup_commands_reuse_fence,
            self.present_queue,
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
            unsafe { self.device.create_image_view(&depth_image_view_info, None) }
                .map_err(VulkanError::VkResultToDo)?;
        let old_depth_image_view = mem::replace(&mut self.depth_image_view, depth_image_view);

        let (attachments, color, depth) = Self::create_attachments(self.surface_format.format);
        let render_pass =
            Self::create_render_pass(&self.device, attachments.all(), &color, &depth)?;
        let old_render_pass = mem::replace(&mut self.render_pass, render_pass);
        let framebuffers = Self::create_framebuffers(
            &self.device,
            self.depth_image_view,
            &self.present_image_views,
            self.render_pass,
            self.surface_resolution,
        )
        .unwrap();
        let old_framebuffers = mem::replace(&mut self.framebuffers, framebuffers);

        unsafe {
            println!("cleaning up old swapchain");
            self.device.device_wait_idle().unwrap();
            self.device.free_memory(old_depth_image_memory, None);
            self.device.destroy_image_view(old_depth_image_view, None);
            self.device.destroy_image(old_depth_image, None);
            for &old_image_view in old_present_image_views.iter() {
                self.device.destroy_image_view(old_image_view, None);
            }
            for framebuffer in old_framebuffers.iter() {
                self.device.destroy_framebuffer(*framebuffer, None);
            }
            self.device.destroy_render_pass(old_render_pass, None);
            old_swapchain_loader.destroy_swapchain(old_swapchain, None);
            old_surface_loader.destroy_surface(old_surface, None);
        }
        self.flag_recreate_swapchain = false;
        Ok(())
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

    //TODO: TaskWithShutdown that can record, record, record, then submit on close.
    #[allow(clippy::too_many_arguments)]
    pub fn record_and_submit_commandbuffer<F>(
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

fn surface_loader_physical_device<'a>(
    physical_devices: &'a [vk::PhysicalDevice],
    instance: &ash::Instance,
    surface_loader: &Surface,
    surface: vk::SurfaceKHR,
) -> Option<(&'a vk::PhysicalDevice, u32)> {
    physical_devices.iter().find_map(|p| {
        unsafe { instance.get_physical_device_queue_family_properties(*p) }
            .iter()
            .enumerate()
            .find_map(move |(index, info)| {
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
}

fn create_debug_callback(
    entry: &Entry,
    instance: &ash::Instance,
) -> (ext::DebugUtils, vk::DebugUtilsMessengerEXT) {
    let debug_utils_loader = ext::DebugUtils::new(entry, instance);
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
    (debug_utils_loader, debug_call_back)
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

            let tracked_models: Vec<_> = self.tracked_models.drain().collect();
            for (_index, (gpu_model, _instant)) in tracked_models {
                gpu_model.deallocate(self);
            }

            self.device.free_memory(self.depth_image_memory, None);
            self.device.destroy_image_view(self.depth_image_view, None);
            self.device.destroy_image(self.depth_image, None);
            for &image_view in self.present_image_views.iter() {
                self.device.destroy_image_view(image_view, None);
            }

            for framebuffer in self.framebuffers.iter() {
                self.device.destroy_framebuffer(*framebuffer, None);
            }
            self.device.destroy_render_pass(self.render_pass, None);

            self.device.destroy_command_pool(self.pool, None);

            self.swapchain_loader
                .destroy_swapchain(self.swapchain, None);

            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);

            if let Some((debug_utils, call_back)) = Option::zip(
                self.maybe_debug_utils_loader.take(),
                self.maybe_debug_call_back.take(),
            ) {
                debug_utils.destroy_debug_utils_messenger(call_back, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

/// Represents the constructed scene as references into world state.
pub struct RenderScene {
    // TODO: should this just be indices?
    pub active_camera: usize,
    pub cameras: Vec<(PhysicalFacet, CameraFacet)>,
    pub drawables: Vec<SceneModelInstance>,
}

/// Reference to a model and for now positional and orientation data.
/// Intended to represent a model (uploaded to the GPU once) with instance
/// information. Should attach to a game object or similar.
pub struct SceneModelInstance {
    pub model: ModelIndex,
    pub pos: Vector3<f32>,
    pub angles: Vector3<f32>,
}

/// Acts as a combiner for Mutex, locking both mutexes but also releases both
/// mutexes when dropped.
pub struct LockWorldAndRenderState {
    world: MutexGuardArc<World>,
    render_state: MutexGuardArc<RenderState>,
}

impl LockWorldAndRenderState {
    pub fn update_render_scene(&mut self) -> Result<(), SceneError> {
        // TODO Fix hardcoded cameras.
        let c1 = self
            .world()
            .get_camera_facet(0u32.into())
            .map_err(SceneError::World)?;
        let c2 = self
            .world()
            .get_camera_facet(1u32.into())
            .map_err(SceneError::World)?;

        let cameras = vec![c1, c2];
        let mut drawables = vec![];

        for (_id, thing) in self.world().things().iter().enumerate() {
            let model_ref = match &thing.facets {
                world::thing::ThingType::Camera { phys, camera } => {
                    let phys = self
                        .world()
                        .facets
                        .physical(*phys)
                        .ok_or(SceneQueryError::NoSuchPhys(*phys))?;
                    let cam = self
                        .world()
                        .facets
                        .camera(*camera)
                        .ok_or(SceneQueryError::NoSuchCamera(*camera))?;

                    let right = cam.right(phys);
                    let forward = cam.forward(phys);
                    let pos = phys.position
                        + Vector3::new(right.x + forward.x, -2.0, right.z + forward.z);
                    let angles = Vector3::new(0.0, phys.angles.y - 1.57, 0.0);

                    SceneModelInstance {
                        model: cam.associated_model.unwrap(),
                        pos,
                        angles,
                    }
                }
                world::thing::ThingType::ModelObject { phys, model } => {
                    let facet = self
                        .world()
                        .facets
                        .physical(*phys)
                        .ok_or(SceneQueryError::NoSuchPhys(*phys))?;

                    SceneModelInstance {
                        model: *model,
                        pos: facet.position,
                        angles: facet.angles,
                    }
                }
            };
            drawables.push(model_ref);
        }
        let active_camera = if self.world().is_server() { 0 } else { 1 };
        let scene = RenderScene {
            active_camera,
            cameras,
            drawables,
        };
        self.render_state().update_scene(scene)?;
        Ok(())
    }

    /// Search through the world for models that need to be uploaded, and do so.
    /// Does not yet handle updates to models.
    pub fn update_models(&mut self) {
        let models: Vec<_> = {
            let world = self.world();
            world
                .facets
                .model_iter()
                .map(|(index, model)| (index, model.clone()))
                .collect()
        };
        // This needs to move to somewhere that owns the assets...
        for (index, model) in models {
            if let Some(_uploaded) = self.render_state().tracked_model(index) {
                // TODO: handle model updates
                // model already uploaded
            } else if self.render_state().queued_model(index) {
                // model already queued for upload
            } else {
                self.render_state()
                    .queue_model_for_upload(index, model)
                    .expect("should upload");
            }
        }
    }

    /// Locks the world and render state so that the renderstate may be updated
    /// from the world.
    pub async fn lock(world: &Arc<Mutex<World>>, render_state: &Arc<Mutex<RenderState>>) -> Self {
        let world = Arc::clone(world).lock_arc().await;
        let render_state = Arc::clone(render_state).lock_arc().await;
        Self {
            world,
            render_state,
        }
    }

    pub fn world(&self) -> &World {
        self.world.deref()
    }

    pub fn render_state(&mut self) -> &mut RenderState {
        self.render_state.deref_mut()
    }
}

/// Vulkan's debug callback.
unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::os::raw::c_void,
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
