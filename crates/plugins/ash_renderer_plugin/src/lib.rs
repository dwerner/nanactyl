//! Plugin: `tui_renderer_plugin`
//! engine.
//!
//! As parts of this are solidified, they can be moved into the crates/render
//! crate, and only expose the plugin for truly dynamic things that are
//! desireable to change at runtime.

mod debug_callback;
mod device;
mod types;

use std::collections::{hash_map, HashMap};
use std::ffi::CString;
use std::mem;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ash::extensions::khr::{Surface, Swapchain};
use ash::{vk, Device, Entry};
use device::GraphicsHandle;
use gfx::{DiffuseColor, GpuNeeds, Graphic, Primitive, Vertex};
use glam::{Mat4, Vec3};
use logger::{debug, error, info, Logger};
use platform::WinPtr;
use plugin_self::{impl_plugin_static, PluginState};
use render::{Presenter, RenderPluginState, RenderState, RenderStateError};
use shader_objects::{PushConstants, UniformBuffer};
use stable_typeid::StableTypeId;
use types::{
    Attachments, AttachmentsModifier, BufferAndMemory, Pipeline, RenderError, Shader, ShaderStage,
    ShaderStages, VertexInputAssembly,
};
use world::components::{Camera, Drawable, Spatial};
use world::graphics::EULER_ROT_ORDER;
use world::{Entity, World};

use crate::device::DeviceWrapper;
use crate::types::DescriptorSetLayoutBinding;

// Prevent the renderer from rebuilding more than once every N ms.
const PIPELINE_REBUILD_DELAY_MILLIS: u64 = 250;

/// Renderer struct owning the descriptor pool, pipelines and descriptions.
struct Renderer {
    descriptor_pool: vk::DescriptorPool,
    pipelines: HashMap<Entity, Pipeline>,
    logger: Logger,
    last_pipeline_rebuild: Instant,
}

#[repr(C)]
struct VulkanDebug {
    placeholder: u32,
    logger: Logger,
}

impl VulkanDebug {
    fn new(logger: Logger) -> Arc<Self> {
        let s = VulkanDebug {
            placeholder: 42,
            logger,
        };
        Arc::new(s)
    }
}

impl Renderer {
    fn present(&mut self, base: &mut VulkanBase, world: &World) -> Result<(), RenderError> {
        if base.flag_recreate_swapchain {
            base.recreate_swapchain()?;
        }

        let present_index = match unsafe {
            base.swapchain_loader.acquire_next_image(
                base.swapchain,
                300 * 1000,
                base.present_complete_semaphore,
                vk::Fence::null(),
            )
        } {
            Ok((index, _suboptimal @ false)) => index,
            Ok((_index, _suboptimal @ true)) => {
                debug!(
                    self.logger.sub("present_with_base"),
                    "will recreate swapchain"
                );
                base.flag_recreate_swapchain = true;
                return Ok(());
            }
            Err(vk::Result::TIMEOUT) => {
                debug!(self.logger, "timeout during acquire_next_image");
                return Ok(());
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                base.flag_recreate_swapchain = true;
                return Ok(());
            }
            Err(err) => {
                return Err(RenderError::SwapchainAcquireNextImage(err));
            }
        };

        let (camera, cam_spatial) = {
            let camera_entity = world.camera().expect("camera should exist");
            let entity = world.heks_world.entity(camera_entity).unwrap();
            if !entity.has::<Camera>() {
                return Err(RenderError::ComponentMissingFromCameraEntity(
                    camera_entity,
                    std::any::type_name::<Camera>(),
                    StableTypeId::of::<Camera>(),
                ));
            }
            if !entity.has::<Spatial>() {
                return Err(RenderError::ComponentMissingFromCameraEntity(
                    camera_entity,
                    std::any::type_name::<Spatial>(),
                    StableTypeId::of::<Spatial>(),
                ));
            }

            (
                world
                    .heks_world
                    .get::<&Camera>(camera_entity)
                    .expect("camera entity"),
                world
                    .heks_world
                    .get::<&Spatial>(camera_entity)
                    .expect("spatial entity"),
            )
        };

        let clear_values = [
            vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.0, 0.0, 0.0, 0.0],
                },
            },
            vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            },
        ];

        let scissors = base.scissors();
        let viewports = base.viewports();

        let w = DeviceWrapper::wrap(&base.device, &self.logger);

        w.wait_for_fence(base.draw_commands_reuse_fence)?;
        w.reset_fence(base.draw_commands_reuse_fence)?;
        w.begin_command_buffer(base.draw_cmd_buf)?;

        let render_pass_begin_info = vk::RenderPassBeginInfo::builder()
            .render_pass(base.render_pass)
            .framebuffer(base.framebuffers[present_index as usize])
            .render_area(base.surface_resolution.into())
            .clear_values(&clear_values);

        w.cmd_begin_render_pass(
            base.draw_cmd_buf,
            &render_pass_begin_info,
            vk::SubpassContents::INLINE,
        );

        let proj_mat = camera.view_projection();

        // TODO more than just models, meshes in general.

        // let logger = self.logger.sub("model render");
        for (gfx_index, (model, _uploaded_instant)) in base.tracked_graphics.iter() {
            // TODO: unified struct for models & pipelines
            let desc = match self.pipelines.get_mut(gfx_index) {
                Some(desc) => desc,
                None => continue,
            };
            let pipeline = match desc.vk.as_ref() {
                Some(pipeline) => pipeline,
                None => continue,
            };

            let ubo = UniformBuffer::with_proj(proj_mat);
            let ubo_bytes = bytemuck::bytes_of(&ubo);
            w.update_buffer(&mut desc.uniform_buffer, ubo_bytes)?;

            w.cmd_bind_descriptor_sets(
                base.draw_cmd_buf,
                vk::PipelineBindPoint::GRAPHICS,
                desc.layout,
                0,
                &[desc.descriptor_set],
                &[],
            );
            w.cmd_bind_pipeline(
                base.draw_cmd_buf,
                vk::PipelineBindPoint::GRAPHICS,
                *pipeline,
            );
            w.cmd_set_viewport(base.draw_cmd_buf, 0, &viewports);
            w.cmd_set_scissor(base.draw_cmd_buf, 0, &scissors);
            w.cmd_bind_vertex_buffers(base.draw_cmd_buf, 0, &[model.vertex_buffer.buffer], &[0]);
            w.cmd_bind_index_buffer(
                base.draw_cmd_buf,
                model.index_buffer.buffer,
                0,
                vk::IndexType::UINT32,
            );
            for (drawable, drawable_spatial) in world
                .heks_world
                .query::<(&Drawable, &Spatial)>()
                .iter()
                .filter_map(|(_entity, (drawable, spatial))| {
                    if drawable.gfx == *gfx_index {
                        Some((drawable, spatial))
                    } else {
                        None
                    }
                })
            {
                let push_constants =
                    PushConstants::new(drawable_transform(drawable, drawable_spatial));
                let push_constant_bytes = push_constants.to_bytes();

                let (model, _) = base.tracked_graphics.get(&drawable.gfx).unwrap();
                w.cmd_push_constants(
                    base.draw_cmd_buf,
                    desc.layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    push_constant_bytes,
                );
                w.cmd_draw_indexed(
                    base.draw_cmd_buf,
                    model.index_buffer.original_len as u32,
                    1,
                    0,
                    0,
                    1,
                );
            }
        }

        w.cmd_end_render_pass(base.draw_cmd_buf);

        let command_buffers = vec![base.draw_cmd_buf];

        // NOT calling build on the builder here prevents a segfault in
        // the release profile.
        let signal = [base.rendering_complete_semaphore];
        let wait = [base.present_complete_semaphore];
        let submit_info = vk::SubmitInfo::builder()
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&[vk::PipelineStageFlags::BOTTOM_OF_PIPE])
            .command_buffers(&command_buffers)
            .signal_semaphores(&signal);

        w.end_command_buffer(base.draw_cmd_buf)?;

        w.queue_submit(
            base.draw_commands_reuse_fence,
            base.present_queue,
            &[*submit_info],
        )?;

        let present_info = vk::PresentInfoKHR {
            wait_semaphore_count: 1,
            p_wait_semaphores: &base.rendering_complete_semaphore,
            swapchain_count: 1,
            p_swapchains: &base.swapchain,
            p_image_indices: &present_index,
            ..Default::default()
        };

        match unsafe {
            base.swapchain_loader
                .queue_present(base.present_queue, &present_info)
        } {
            Ok(_suboptimal @ false) => {}
            Ok(_suboptimal @ true) => {
                base.flag_recreate_swapchain = true;
                return Ok(());
            }
            Err(vk::Result::TIMEOUT) => return Ok(()),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                base.flag_recreate_swapchain = true;
                return Ok(());
            }
            Err(vk_err) => return Err(RenderError::Present(vk_err)),
        };

        Ok(())
    }

    /// Rebuilds pipelines and reloads shaders *from disk*.
    // TODO: build pipeline and bindings from more rich introspection of assets.
    fn rebuild_pipelines(&mut self, base: &mut VulkanBase) -> Result<(), RenderError> {
        let logger = self.logger.sub("rebuild_pipelines");
        if Instant::now().duration_since(self.last_pipeline_rebuild)
            < Duration::from_secs(PIPELINE_REBUILD_DELAY_MILLIS)
        {
            info!(logger, "pipeline rebuild too soon, skipping");
        }
        if !self.pipelines.is_empty() {
            info!(logger, "destroying {} pipelines", self.pipelines.len());
            unsafe {
                base.device
                    .device_wait_idle()
                    .map_err(RenderError::VkResultToDo)?;

                for (_, mut pipeline) in self.pipelines.drain() {
                    if let Some(vk_pipeline) = pipeline.vk.take() {
                        base.device.destroy_pipeline(vk_pipeline, None);
                    }
                    pipeline.deallocate(&base.device);
                }
            }
        }

        // For now we are creating a pipeline per model.
        // TODO: programmatically compose descriptor set and shader bindings from the
        // shaders themselves:
        // - do they have a uniform buffer?
        // - do they have a texture/sampler etc?
        // - compose the shaders into a pipeline and determine stages from reflected
        //   entry points
        for (graphics_index, (handle, _uploaded_instant)) in base.tracked_graphics.iter() {
            info!(
                logger,
                "upload graphics {graphics_index:?}, vert: {} frag: {}",
                handle.vertex_shader.path().display(),
                handle.fragment_shader.path().display()
            );
            let uniform_buffer = {
                let uniform_buffer = UniformBuffer::new();
                let uniform_bytes = bytemuck::bytes_of(&uniform_buffer);
                let device = DeviceWrapper::wrap(&base.device, &logger);
                device.allocate_and_init_buffer(
                    vk::BufferUsageFlags::UNIFORM_BUFFER,
                    base.device_memory_properties,
                    uniform_bytes,
                )?
            };

            // todo: take a list of shaders instead, and compose a descriptor set from them
            let desc_set_layout =
                base.create_descriptor_set_layout(&handle.vertex_shader, &handle.fragment_shader)?;

            let descriptor_sets =
                base.allocate_descriptor_sets(self.descriptor_pool, &[desc_set_layout])?;

            // TODO compose a struct for containing samplers and related images
            //let specular_sampler = bw.create_sampler()?;
            //let bump_sampler = bw.create_sampler()?;

            let mut maybe_diffuse_sampler = None;
            let maybe_diffuse_image_view = handle.diffuse_map.as_ref().map(|map| map.image_view);
            let descriptor_set = descriptor_sets[0];

            if handle.diffuse_map.is_some() {
                let diffuse_sampler = base.create_sampler()?;
                maybe_diffuse_sampler = Some(diffuse_sampler);
            }

            VulkanBase::update_descriptor_set(
                &base.device,
                descriptor_set,
                &uniform_buffer,
                maybe_diffuse_image_view,
                // None, // model.specular_map.as_ref().map(|x| x.image_view),
                // None, // model.bump_map.as_ref().map(|x| x.image_view),
                maybe_diffuse_sampler,
                // specular_sampler,
                // bump_sampler,
            );

            let mut shader_stages = ShaderStages::new();
            shader_stages.add_shader(
                &base.device,
                Arc::clone(&handle.vertex_shader),
                vk::ShaderStageFlags::VERTEX,
            )?;
            shader_stages.add_shader(
                &base.device,
                Arc::clone(&handle.fragment_shader),
                vk::ShaderStageFlags::FRAGMENT,
            )?;

            let topology = handle.primitive_topology();
            let mut vertex_input_assembly = VertexInputAssembly::new(topology);

            vertex_input_assembly.add_binding_description::<Vertex>(0, vk::VertexInputRate::VERTEX);
            vertex_input_assembly.add_attribute_description(
                0,
                0,
                vk::Format::R32G32B32A32_SFLOAT,
                offset_of!(Vertex, pos) as u32,
            );
            vertex_input_assembly.add_attribute_description(
                0,
                1,
                vk::Format::R32G32_SFLOAT,
                offset_of!(Vertex, uv) as u32,
            );
            vertex_input_assembly.add_attribute_description(
                0,
                2,
                vk::Format::R32G32B32_SFLOAT,
                offset_of!(Vertex, normal) as u32,
            );

            let w = DeviceWrapper::wrap(&base.device, &logger);

            let pipeline_layout = w.pipeline_layout(
                std::mem::size_of::<PushConstants>() as u32,
                &[desc_set_layout],
            )?;

            let vk = base.create_pipeline(
                &shader_stages,
                &base.scissors(),
                &base.viewports(),
                pipeline_layout,
                primitive_to_vk_polygon_mode(handle.primitive),
                &vertex_input_assembly,
                base.render_pass,
            )?;
            let mut pipeline = Pipeline::create(
                desc_set_layout,
                uniform_buffer,
                descriptor_set,
                maybe_diffuse_sampler,
                // specular_sampler,
                // bump_sampler,
                pipeline_layout,
                base.viewports(),
                base.scissors(),
                shader_stages,
                vertex_input_assembly,
                primitive_to_vk_polygon_mode(handle.primitive),
            );
            pipeline.set_vk(vk);
            self.pipelines.insert(*graphics_index, pipeline);
        }

        info!(logger, "rebuilt {} pipelines", self.pipelines.len());

        Ok(())
    }

    fn deallocate(&mut self, base: &mut VulkanBase) -> Result<(), RenderError> {
        unsafe {
            base.device
                .device_wait_idle()
                .map_err(RenderError::VkResultToDo)?;
        }
        for (_, desc) in self.pipelines.iter() {
            unsafe {
                if let Some(pipeline) = desc.vk {
                    base.device.destroy_pipeline(pipeline, None);
                }
            };
            desc.deallocate(&base.device);
        }
        unsafe {
            base.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
        }
        Ok(())
    }
}

fn drawable_transform(drawable: &Drawable, drawable_spatial: &Spatial) -> Mat4 {
    let scale = Mat4::from_scale(drawable.scale * Vec3::ONE);
    let translation = Mat4::from_translation(drawable_spatial.pos);
    let rot = Mat4::from_euler(
        EULER_ROT_ORDER,
        drawable_spatial.angles.x,
        drawable_spatial.angles.y,
        drawable_spatial.angles.z,
    );
    scale * rot * translation
}

impl Presenter for VulkanRenderPluginState {
    fn present(&mut self, world: &World) {
        if let Some(renderer) = &mut self.renderer {
            if let Err(err) = renderer.present(self.base.as_mut().unwrap(), world) {
                error!(self.logger.sub("entity"), "error in present : {:?}", err);
            }
        }
    }

    fn update_resources(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            renderer
                .rebuild_pipelines(self.base.as_mut().unwrap())
                .unwrap();
        }
    }

    fn deallocate(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            renderer.deallocate(self.base.as_mut().unwrap()).unwrap();
        }
    }

    fn tracked_graphics(&self, entity: Entity) -> Option<Instant> {
        self.base
            .as_ref()?
            .tracked_graphics
            .get(&entity)
            .map(|(_, tracked_instant)| *tracked_instant)
    }

    fn upload_graphics(&mut self, graphics: &[(Entity, &Graphic)]) -> Result<(), RenderStateError> {
        let logger = self.logger.sub("upload_graphic");

        let (base, renderer) = self
            .base
            .as_mut()
            .zip(self.renderer.as_mut())
            .ok_or(RenderStateError::NoVulkanBase)?;

        for (index, handle) in base.upload_graphics(graphics, &logger) {
            info!(logger, "plugin side upload graphics: {:?}", index);
            base.track_uploaded_graphic(index, handle);
        }

        // todo: do this only when we actually upload something
        renderer
            .rebuild_pipelines(self.base.as_mut().unwrap())
            .unwrap();

        Ok(())
    }
}

// Simple offset_of macro akin to C++ offsetof
#[macro_export]
macro_rules! offset_of {
    ($base:path, $field:ident) => {{
        #[allow(unused_unsafe)]
        unsafe {
            let b: $base = ::std::mem::zeroed();
            (&b.$field as *const _ as isize) - (&b as *const _ as isize)
        }
    }};
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

// TODO: write a frame struct here that's to hold all the resources a frame
// needs
struct Frame {
    pipelines: Vec<Pipeline>,
}

/// Carries vulkan state.
struct VulkanBase {
    win_ptr: platform::WinPtr,
    entry: ash::Entry,
    instance: ash::Instance,
    device: Device,
    surface_loader: Surface,
    swapchain_loader: Swapchain,

    physical_device: vk::PhysicalDevice,
    device_memory_properties: vk::PhysicalDeviceMemoryProperties,
    queue_family_index: u32,
    present_queue: vk::Queue,

    surface: vk::SurfaceKHR,
    surface_format: vk::SurfaceFormatKHR,
    surface_resolution: vk::Extent2D,

    swapchain: vk::SwapchainKHR,
    present_images: Vec<vk::Image>,
    present_image_views: Vec<vk::ImageView>,

    pool: vk::CommandPool,
    draw_cmd_buf: vk::CommandBuffer,
    setup_command_buffer: vk::CommandBuffer,

    depth_image: vk::Image,
    depth_image_view: vk::ImageView,
    depth_image_memory: vk::DeviceMemory,

    present_complete_semaphore: vk::Semaphore,
    rendering_complete_semaphore: vk::Semaphore,

    draw_commands_reuse_fence: vk::Fence,
    setup_commands_reuse_fence: vk::Fence,

    maybe_debug_utils_loader: Option<ash::extensions::ext::DebugUtils>,
    maybe_debug_call_back: Option<vk::DebugUtilsMessengerEXT>,

    tracked_graphics: HashMap<Entity, (GraphicsHandle, Instant)>,

    framebuffers: Vec<vk::Framebuffer>,
    render_pass: vk::RenderPass,

    flag_recreate_swapchain: bool,

    logger: Logger,

    _debug_struct: Arc<VulkanDebug>,
}

impl VulkanBase {
    fn upload_graphics(
        &mut self,
        upload_queue: &[(Entity, &Graphic)],
        logger: &Logger,
    ) -> Vec<(Entity, GraphicsHandle)> {
        let logger = logger.sub("upload_graphics");

        if upload_queue.is_empty() {
            return vec![];
        }

        let device = self.device.clone();
        let queue = self.present_queue;
        let queue_family_index = self.queue_family_index;
        let device_memory_properties = self.device_memory_properties;
        let w = DeviceWrapper::wrap(&device, &logger.sub("device"));
        let pool = w.create_command_pool(queue_family_index).unwrap();
        let fence = w.create_fence().unwrap();
        let mut src_images = Vec::new();
        let mut completed_uploads = Vec::new();
        for (index, graphic) in upload_queue {
            debug!(logger, "loading graphics object at {index:?}");

            let command_buffers = w.allocate_command_buffers(pool).unwrap();
            let command_buffer = command_buffers[0];

            w.wait_for_fence(fence).unwrap();
            w.begin_command_buffer(command_buffer).unwrap();

            let diffuse_map = match graphic.diffuse_color() {
                Some(DiffuseColor::Texture(texture)) => Some(w.cmd_upload_image(
                    texture,
                    device_memory_properties,
                    command_buffer,
                    &mut src_images,
                )),
                None | Some(DiffuseColor::Color(_)) => None,
            };

            // let specular_map = maybe_cmd_upload_image(
            //     &w,
            //     model.material.specular_map.as_ref(),
            //     device_memory_properties,
            //     command_buffer,
            //     &mut src_images,
            // );
            // let bump_map = maybe_cmd_upload_image(
            //     &w,
            //     model.material.bump_map.as_ref(),
            //     device_memory_properties,
            //     command_buffer,
            //     &mut src_images,
            // );

            w.end_command_buffer(command_buffer).unwrap();

            let submit_infos = [*vk::SubmitInfo::builder().command_buffers(&command_buffers)];
            w.queue_submit(fence, queue, &submit_infos).unwrap();

            let vertex_buffer = w
                .allocate_and_init_buffer(
                    vk::BufferUsageFlags::VERTEX_BUFFER,
                    device_memory_properties,
                    graphic.vertices(),
                )
                .unwrap();

            let index_buffer = w
                .allocate_and_init_buffer(
                    vk::BufferUsageFlags::INDEX_BUFFER,
                    device_memory_properties,
                    graphic.indices(),
                )
                .unwrap();

            // reflect over shaders and determine descriptor sets
            let vertex_shader =
                Shader::read_spv(graphic.vertex_shader_path().to_path_buf()).unwrap();

            let fragment_shader =
                Shader::read_spv(graphic.fragment_shader_path().to_path_buf()).unwrap();

            let handle = GraphicsHandle::new(
                diffuse_map,
                vertex_buffer,
                index_buffer,
                vertex_shader,
                fragment_shader,
                graphic.primitive(),
            );
            completed_uploads.push((*index, handle));
        }
        unsafe {
            device.device_wait_idle().unwrap();
        }
        for image in src_images {
            image.deallocate(&device);
        }
        unsafe {
            device.destroy_fence(fence, None);
            device.destroy_command_pool(pool, None);
        }
        completed_uploads
    }

    fn renderer(&mut self) -> Result<Renderer, RenderError> {
        // TODO: shaders that apply only to certain models need different descriptor
        // sets.
        //? TODO: any pool can be a thread local, but then any object must be destroyed
        //? on that thread.
        let descriptor_pool = self.create_descriptor_pool(20, 20, 20)?;
        let mut renderer = Renderer {
            descriptor_pool,
            pipelines: HashMap::new(),
            logger: self.logger.sub("renderer"),
            last_pipeline_rebuild: Instant::now() - Duration::from_secs(60),
        };
        renderer.rebuild_pipelines(self)?;
        Ok(renderer)
    }

    pub fn scissors(&self) -> Vec<vk::Rect2D> {
        vec![self.surface_resolution.into()]
    }

    pub fn viewports(&self) -> Vec<vk::Viewport> {
        vec![vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.surface_resolution.width as f32,
            height: self.surface_resolution.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }]
    }

    fn create_pipeline(
        &self,
        shader_stages: &ShaderStages,
        scissors: &[vk::Rect2D],
        viewports: &[vk::Viewport],
        layout: vk::PipelineLayout,
        polygon_mode: vk::PolygonMode,
        vertex_input_assembly: &VertexInputAssembly,
        render_pass: vk::RenderPass,
    ) -> Result<vk::Pipeline, RenderError> {
        let shader_stage_create_infos: Vec<vk::PipelineShaderStageCreateInfo> = shader_stages
            .shader_stage_defs
            .iter()
            .map(ShaderStage::create_info)
            .collect();
        let viewport_state_info = vk::PipelineViewportStateCreateInfo::builder()
            .scissors(&scissors)
            .viewports(&viewports);
        let rasterization_info = vk::PipelineRasterizationStateCreateInfo {
            front_face: vk::FrontFace::COUNTER_CLOCKWISE,
            line_width: 1.0,
            polygon_mode,
            ..Default::default()
        };
        let multisample_state_info = vk::PipelineMultisampleStateCreateInfo::builder()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let noop_stencil_state = vk::StencilOpState {
            fail_op: vk::StencilOp::KEEP,
            pass_op: vk::StencilOp::KEEP,
            depth_fail_op: vk::StencilOp::KEEP,
            compare_op: vk::CompareOp::ALWAYS,
            ..Default::default()
        };
        let depth_state_info = vk::PipelineDepthStencilStateCreateInfo {
            depth_test_enable: 1,
            depth_write_enable: 1,
            depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
            front: noop_stencil_state,
            back: noop_stencil_state,
            max_depth_bounds: 1.0,
            ..Default::default()
        };
        let color_blend_attachment_states = [vk::PipelineColorBlendAttachmentState {
            blend_enable: 0,
            src_color_blend_factor: vk::BlendFactor::SRC_COLOR,
            dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_DST_COLOR,
            color_blend_op: vk::BlendOp::ADD,
            src_alpha_blend_factor: vk::BlendFactor::ZERO,
            dst_alpha_blend_factor: vk::BlendFactor::ZERO,
            alpha_blend_op: vk::BlendOp::ADD,
            color_write_mask: vk::ColorComponentFlags::RGBA,
        }];
        let color_blend_state = vk::PipelineColorBlendStateCreateInfo::builder()
            .logic_op(vk::LogicOp::CLEAR)
            .attachments(&color_blend_attachment_states);
        let dynamic_state = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::builder().dynamic_states(&dynamic_state);
        let vertex_input_state_info = vertex_input_assembly.input_state_info();
        let vertex_input_assembly_state_info = vertex_input_assembly.assembly_state_info();
        let graphics_pipeline_info = vk::GraphicsPipelineCreateInfo::builder()
            .stages(&shader_stage_create_infos)
            .vertex_input_state(&vertex_input_state_info)
            .input_assembly_state(&vertex_input_assembly_state_info)
            .viewport_state(&viewport_state_info)
            .rasterization_state(&rasterization_info)
            .multisample_state(&multisample_state_info)
            .depth_stencil_state(&depth_state_info)
            .color_blend_state(&color_blend_state)
            .dynamic_state(&dynamic_state_info)
            .layout(layout)
            .render_pass(render_pass);
        let pipeline = unsafe {
            self.device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                &[*graphics_pipeline_info],
                None,
            )
        }
        .map_err(|(pipeline, result)| RenderError::FailedToCreatePipeline(pipeline, result))?[0];
        Ok(pipeline)
    }

    // This could be updated to update many descriptor sets in bulk, however we only
    // have one we care about, per-pipeline when this was written.
    pub fn update_descriptor_set(
        device: &ash::Device,
        descriptor_set: vk::DescriptorSet,
        uniform_buffer: &BufferAndMemory,

        // TODO: imageview + sampler struct
        maybe_diffuse_image_view: Option<vk::ImageView>,
        maybe_diffuse_sampler: Option<vk::Sampler>,
        //specular_image_view: Option<vk::ImageView>,
        //bump_image_view: Option<vk::ImageView>,
        //_specular_sampler: vk::Sampler,
        //_bump_sampler: vk::Sampler,
    ) {
        let uniform_descriptors = [*vk::DescriptorBufferInfo::builder()
            .buffer(uniform_buffer.buffer)
            .range(uniform_buffer.original_len as u64)];

        let mut write_desc_sets = vec![*vk::WriteDescriptorSet::builder()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(&uniform_descriptors)];

        if let (Some(diffuse), Some(diffuse_sampler)) =
            (maybe_diffuse_image_view, maybe_diffuse_sampler)
        {
            let descriptor = vk::DescriptorImageInfo::builder()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(diffuse)
                .sampler(diffuse_sampler);
            let tex_descriptors = vec![*descriptor];
            //let binding = 1 + tex_descriptors.len();
            let write = vk::WriteDescriptorSet::builder()
                .dst_set(descriptor_set)
                .dst_binding(1)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&tex_descriptors);
            write_desc_sets.push(*write);
            unsafe { device.update_descriptor_sets(&write_desc_sets, &[]) };
            return;
        } else {
            unsafe { device.update_descriptor_sets(&write_desc_sets, &[]) };
        }

        // if let Some(specular) = specular_image_view {
        //     let descriptor = vk::DescriptorImageInfo::builder()
        //         .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        //         .image_view(specular)
        //         .sampler(specular_sampler);
        //     let tex_descriptors = vec![*descriptor];
        //     let binding = 1 + tex_descriptors.len();
        //     let write = vk::WriteDescriptorSet::builder()
        //         .dst_set(descriptor_set)
        //         .dst_binding(binding as u32)
        //         .dst_array_element(0)
        //         .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        //         .image_info(&tex_descriptors);
        //     let write_desc_sets = vec![*write];
        //     unsafe { device.update_descriptor_sets(&write_desc_sets, &[]) };
        // } else {
        //     unreachable!("spec")
        // }

        // if let Some(bump) = bump_image_view {
        //     let descriptor = vk::DescriptorImageInfo::builder()
        //         .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        //         .image_view(bump)
        //         .sampler(bump_sampler);
        //     let tex_descriptors = vec![*descriptor];
        //     let binding = 1 + tex_descriptors.len();
        //     let write = vk::WriteDescriptorSet::builder()
        //         .dst_set(descriptor_set)
        //         .dst_binding(binding as u32)
        //         .dst_array_element(0)
        //         .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        //         .image_info(&tex_descriptors);
        //     let write_desc_sets = vec![*write];
        //     unsafe { device.update_descriptor_sets(&write_desc_sets, &[]) };
        // } else {
        //     unreachable!("bump")
        // }
    }

    /// Allocates a descriptor set.
    pub fn allocate_descriptor_sets(
        &self,
        pool: vk::DescriptorPool,
        layouts: &[vk::DescriptorSetLayout],
    ) -> Result<Vec<vk::DescriptorSet>, RenderError> {
        let desc_alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(layouts);
        unsafe { self.device.allocate_descriptor_sets(&desc_alloc_info) }
            .map_err(RenderError::VkResultToDo)
    }

    /// Creates a descriptor set layout from the provided `ShaderBindingDesc`
    /// struct.
    pub fn create_descriptor_set_layout(
        &self,
        vertex_shader: &Shader,
        fragment_shader: &Shader,
    ) -> Result<vk::DescriptorSetLayout, RenderError> {
        let vertex_bindings =
            vertex_shader
                .entry_points()
                .iter()
                .fold(Vec::new(), |mut acc, entry_point| {
                    acc.extend(entry_point.desc_set_layout_bindings());
                    acc
                });

        let fragment_bindings =
            fragment_shader
                .entry_points()
                .iter()
                .fold(Vec::new(), |mut acc, entry_point| {
                    acc.extend(entry_point.desc_set_layout_bindings());
                    acc
                });

        let bindings: Vec<_> =
            merge_vertex_and_fragment_bindings(&vertex_bindings, &fragment_bindings).collect();

        let descriptor_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        let layout = unsafe {
            self.device
                .create_descriptor_set_layout(&descriptor_info, None)
        }
        .map_err(RenderError::VkResultToDo)?;
        Ok(layout)
    }

    /// Creates a descriptor pool with the provided parameters.
    pub fn create_descriptor_pool(
        &mut self,
        max_sets: u32,
        max_samplers: u32,
        max_uniform_buffers: u32,
    ) -> Result<vk::DescriptorPool, RenderError> {
        let descriptor_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: max_uniform_buffers,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: max_samplers,
            },
            // vk::DescriptorPoolSize {
            //     ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            //     descriptor_count: max_samplers,
            // },
            // vk::DescriptorPoolSize {
            //     ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            //     descriptor_count: max_samplers,
            // },
        ];
        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&descriptor_sizes)
            .max_sets(max_sets);
        unsafe {
            self.device
                .create_descriptor_pool(&descriptor_pool_info, None)
        }
        .map_err(RenderError::VkResultToDo)
    }

    /// Creates a sampler.
    pub fn create_sampler(&self) -> Result<vk::Sampler, RenderError> {
        // start preparing shader related structures
        let sampler_info = vk::SamplerCreateInfo {
            mag_filter: vk::Filter::LINEAR,
            min_filter: vk::Filter::LINEAR,
            mipmap_mode: vk::SamplerMipmapMode::LINEAR,
            address_mode_u: vk::SamplerAddressMode::MIRRORED_REPEAT,
            address_mode_v: vk::SamplerAddressMode::MIRRORED_REPEAT,
            address_mode_w: vk::SamplerAddressMode::MIRRORED_REPEAT,
            max_anisotropy: 1.0,
            border_color: vk::BorderColor::FLOAT_OPAQUE_WHITE,
            compare_op: vk::CompareOp::NEVER,
            ..Default::default()
        };

        unsafe { self.device.create_sampler(&sampler_info, None) }
            .map_err(RenderError::VkResultToDo)
    }
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
                *vk::AttachmentDescription::builder()
                    .format(format)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::CLEAR)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .final_layout(vk::ImageLayout::PRESENT_SRC_KHR),
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            )
            .into_refs();
        let depth_attachment_ref = AttachmentsModifier::new(&mut attachments).add_single(
            *vk::AttachmentDescription::builder()
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .format(vk::Format::D16_UNORM)
                .initial_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
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
    ) -> Result<vk::RenderPass, RenderError> {
        let dependencies = [*vk::SubpassDependency::builder()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            )
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)];
        let subpass = vk::SubpassDescription::builder()
            .color_attachments(color_attachment_refs)
            .depth_stencil_attachment(depth_attachment_ref)
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS);
        let subpasses = vec![*subpass];
        let renderpass_create_info = *vk::RenderPassCreateInfo::builder()
            .attachments(all_attachments)
            .subpasses(&subpasses)
            .dependencies(&dependencies);

        unsafe { device.create_render_pass(&renderpass_create_info, None) }
            .map_err(RenderError::VkResultToDo)
    }

    /// Create framebuffers needed, consumes the renderpass. Returns an error
    /// when unable to create framebuffers.
    pub fn create_framebuffers(
        device: &ash::Device,
        depth_image_view: vk::ImageView,
        present_image_views: &[vk::ImageView],
        render_pass: vk::RenderPass,
        surface_resolution: vk::Extent2D,
    ) -> Result<Vec<vk::Framebuffer>, RenderError> {
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
                .map_err(RenderError::VkResultToDo)?;
            framebuffers.push(framebuffer);
        }
        Ok(framebuffers)
    }
    /// Track a model reference for cleanup when VulkanBase is dropped.
    fn track_uploaded_graphic(&mut self, entity: Entity, handle: GraphicsHandle) {
        debug!(self.logger, "Tracking model {:?}", entity);
        if let Some((existing_model, _instant)) = self
            .tracked_graphics
            .insert(entity, (handle, Instant::now()))
        {
            debug!(self.logger, "Deallocating existing model {:?}", entity);
            existing_model.deallocate(self);
        }
    }

    /// Create a new instance of VulkanBase, takes a platform::WinPtr and some
    /// flags. This allows each window created by a process to be injected
    /// into the renderer intended to bind it. Returns an error when the
    /// instance cannot be created.
    pub fn new(
        win_ptr: platform::WinPtr,
        enable_validation_layer: bool,
        logger: Logger,
    ) -> Result<Self, RenderError> {
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

        required_extension_names.push(ash::extensions::ext::DebugUtils::name().as_ptr());

        let create_info = *vk::InstanceCreateInfo::builder()
            .application_info(application_info)
            .enabled_layer_names(&layers_names_raw)
            .enabled_extension_names(&required_extension_names);

        let instance = unsafe { entry.create_instance(&create_info, None) }.unwrap();

        let debug = VulkanDebug::new(logger.sub("vk-callback"));
        let (maybe_debug_utils_loader, maybe_debug_call_back) = {
            if enable_validation_layer {
                let (debug_utils_loader, debug_call_back) =
                    debug_callback::create_debug_callback(&entry, &instance, &debug);
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
            fill_mode_non_solid: 1,
            ..Default::default()
        };
        let priorities = [1.0];
        let queue_create_infos = [*vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities)];

        let device_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&queue_create_infos)
            .enabled_extension_names(&device_extension_names_raw)
            .enabled_features(&features);

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

        info!(logger, "present_mode: {present_mode:?}");

        let swapchain_loader = Swapchain::new(&instance, &device);
        let swapchain_create_info = *vk::SwapchainCreateInfoKHR::builder()
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
            .image_array_layers(1);

        let swapchain =
            unsafe { swapchain_loader.create_swapchain(&swapchain_create_info, None) }.unwrap();

        let pool_create_info = *vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family_index);

        let pool = unsafe { device.create_command_pool(&pool_create_info, None) }.unwrap();
        let command_buffer_allocate_info = *vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(2)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY);

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
        let depth_image_allocate_info = *vk::MemoryAllocateInfo::builder()
            .allocation_size(depth_image_memory_req.size)
            .memory_type_index(depth_image_memory_index);

        let depth_image_memory =
            unsafe { device.allocate_memory(&depth_image_allocate_info, None) }.unwrap();

        unsafe { device.bind_image_memory(depth_image, depth_image_memory, 0) }.unwrap();

        let fence_create_info =
            *vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

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
                let layout_transition_barriers = *vk::ImageMemoryBarrier::builder()
                    .image(depth_image)
                    .dst_access_mask(
                        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    )
                    .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .subresource_range(
                        *vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::DEPTH)
                            .layer_count(1)
                            .level_count(1),
                    );

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

        let depth_image_view_info = *vk::ImageViewCreateInfo::builder()
            .subresource_range(
                *vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::DEPTH)
                    .level_count(1)
                    .layer_count(1),
            )
            .image(depth_image)
            .format(depth_image_create_info.format)
            .view_type(vk::ImageViewType::TYPE_2D);

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
            tracked_graphics: HashMap::new(),
            framebuffers,
            render_pass,
            flag_recreate_swapchain: false,
            logger,
            _debug_struct: debug,
        })
    }

    /// Re-create the swapchain bound. Useful when window properties change, on
    /// resize, fullscreen, focus, etc.
    pub fn recreate_swapchain(&mut self) -> Result<(), RenderError> {
        let surface_loader = Surface::new(&self.entry, &self.instance);
        let old_surface_loader = mem::replace(&mut self.surface_loader, surface_loader);

        let surface =
            unsafe { ash_window::create_surface(&self.entry, &self.instance, &self.win_ptr, None) }
                .map_err(RenderError::VkResultToDo)?;
        let old_surface = mem::replace(&mut self.surface, surface);

        let physical_devices = unsafe { self.instance.enumerate_physical_devices() }
            .map_err(RenderError::EnumeratePhysicalDevices)?;
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
        .map_err(RenderError::VkResultToDo)?;

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

        let swapchain_create_info = *vk::SwapchainCreateInfoKHR::builder()
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
            .old_swapchain(self.swapchain);

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
                    .map_err(RenderError::VkResultToDo)
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

        let depth_image_allocate_info = *vk::MemoryAllocateInfo::builder()
            .allocation_size(depth_image_memory_req.size)
            .memory_type_index(depth_image_memory_index);

        let depth_image_memory = unsafe {
            self.device
                .allocate_memory(&depth_image_allocate_info, None)
        }
        .map_err(RenderError::VkResultToDo)?;
        unsafe {
            self.device
                .bind_image_memory(self.depth_image, depth_image_memory, 0)
        }
        .map_err(RenderError::VkResultToDo)?;
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
                let layout_transition_barriers = *vk::ImageMemoryBarrier::builder()
                    .image(depth_image)
                    .dst_access_mask(
                        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    )
                    .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .subresource_range(
                        *vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::DEPTH)
                            .layer_count(1)
                            .level_count(1),
                    );

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

        let depth_image_view_info = *vk::ImageViewCreateInfo::builder()
            .subresource_range(
                *vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::DEPTH)
                    .level_count(1)
                    .layer_count(1),
            )
            .image(depth_image)
            .format(depth_image_create_info.format)
            .view_type(vk::ImageViewType::TYPE_2D);

        let depth_image_view =
            unsafe { self.device.create_image_view(&depth_image_view_info, None) }
                .map_err(RenderError::VkResultToDo)?;
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

    // TODO: TaskWithShutdown that can record, record, record, then submit on close.
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
        let command_buffer_begin_info = *vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe { device.begin_command_buffer(command_buffer, &command_buffer_begin_info) }.unwrap();

        command_buffer_fn(device, command_buffer);

        unsafe { device.end_command_buffer(command_buffer) }.unwrap();

        let command_buffers = vec![command_buffer];
        let submit_info = *vk::SubmitInfo::builder()
            .wait_semaphores(wait_semaphores)
            .wait_dst_stage_mask(wait_mask)
            .command_buffers(&command_buffers)
            .signal_semaphores(signal_semaphores);

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

            let tracked_models: Vec<_> = self.tracked_graphics.drain().collect();
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

#[derive(Default)]
pub struct VulkanRenderPluginState {
    _win_ptr: Option<WinPtr>,
    base: Option<VulkanBase>,
    renderer: Option<Renderer>,
    logger: Logger,
}

impl PluginState for VulkanRenderPluginState {
    type GameState = RenderState;

    fn new() -> Box<Self> {
        Box::new(VulkanRenderPluginState::default())
    }

    fn load(&mut self, state: &mut Self::GameState) {
        // let (state, world) = state;
        let logger = state.logger.sub("ash-renderer-load");
        self.logger.maybe_set_filter(logger.get_filter());

        info!(logger, "loaded ash_renderer_plugin...");

        let mut base = VulkanBase::new(
            state.win_ptr,
            state.enable_validation_layer,
            logger.sub("vulkan-base"),
        )
        .expect("unable to create VulkanBase");

        info!(logger, "initialized vulkan base");

        self.renderer = Some(base.renderer().expect("unable to setup renderer"));
        info!(logger, "set presenter");

        self.base = Some(base);

        info!(logger, "set base");
    }

    fn update(&mut self, state: &mut Self::GameState, _dt: &Duration) {
        // let (state, world) = state;
        // Call render, buffers are updated etc
        // if let Some(renderer) = self.renderer.as_mut() {
        //     state.updates += 1;
        //     renderer
        //         .present(self.base.as_mut().unwrap(), &world)
        //         .unwrap();
        // }
    }

    fn unload(&mut self, state: &mut Self::GameState) {
        // let (state, world) = state;
        let logger = state.logger.sub("ash-renderer-unload");
        info!(logger, "unloading ash_renderer_plugin...");
        if let Some(presenter) = &mut self.renderer {
            presenter.deallocate(self.base.as_mut().unwrap()).unwrap();
        }
    }
}

impl Drop for VulkanRenderPluginState {
    fn drop(&mut self) {
        info!(self.logger, "dropping vulkan render plugin state...");
    }
}

/// Merge and determine DescriptorSetLayoutBindings and associated
/// ShaderStageFlags based on if they are coming from a vertex or fragment
/// shader.
fn merge_vertex_and_fragment_bindings<'a>(
    vertex_bindings: &[&'a DescriptorSetLayoutBinding],
    fragment_bindings: &[&'a DescriptorSetLayoutBinding],
) -> impl Iterator<Item = vk::DescriptorSetLayoutBinding> {
    let mut merged = HashMap::new();
    for binding in vertex_bindings {
        merged.insert(
            binding.binding,
            binding.as_layout_binding(vk::ShaderStageFlags::VERTEX),
        );
    }
    for binding in fragment_bindings {
        match merged.entry(binding.binding) {
            hash_map::Entry::Occupied(mut entry) => {
                let mut merged = entry.get_mut();
                merged.stage_flags |= vk::ShaderStageFlags::FRAGMENT;
                merged.descriptor_type = binding.descriptor_type;
            }
            hash_map::Entry::Vacant(entry) => {
                entry.insert(binding.as_layout_binding(vk::ShaderStageFlags::FRAGMENT));
            }
        }
    }

    merged.into_iter().map(|(_, binding)| binding)
}

fn primitive_to_vk_polygon_mode(primitive: Primitive) -> vk::PolygonMode {
    match primitive {
        Primitive::PointList => vk::PolygonMode::POINT,
        Primitive::LineList => vk::PolygonMode::LINE,
        Primitive::LineStrip => vk::PolygonMode::LINE,
        Primitive::TriangleList => vk::PolygonMode::FILL,
    }
}

impl RenderPluginState for VulkanRenderPluginState {}

impl_plugin_static!(
    VulkanRenderPluginState,
    RenderState,
    RenderPluginState<GameState = RenderState>
);
