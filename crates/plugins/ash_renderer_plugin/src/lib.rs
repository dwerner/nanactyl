use std::{ffi::CStr, io::Cursor, mem::align_of, time::Duration};

use ash::{util::Align, vk};

use render::{Presenter, RenderState, VulkanBase};

impl Presenter for Renderer {
    fn present(&self, base: &mut VulkanBase) {
        //println!("presented something... Ha HAA");
        present(self, base).unwrap();
    }
    fn drop_resources(&self, base: &mut VulkanBase) {
        drop_resources(base, self).unwrap();
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

#[derive(Clone, Debug, Copy)]
struct Vertex {
    pos: [f32; 4],
    uv: [f32; 2],
}

#[derive(Clone, Debug, Copy)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub _pad: f32,
}

#[derive(Default)]
pub struct Attachments {
    descriptions: Vec<vk::AttachmentDescription>,
}

impl Attachments {
    pub fn all(&self) -> &[vk::AttachmentDescription] {
        &self.descriptions
    }
}

struct AttachmentsModifier<'a> {
    attachments: &'a mut Attachments,
    attachment_refs: Vec<vk::AttachmentReference>,
}

impl<'a> AttachmentsModifier<'a> {
    pub fn new(attachments: &'a mut Attachments) -> Self {
        Self {
            attachments,
            attachment_refs: Vec::new(),
        }
    }

    pub fn add_single(
        &mut self,
        description: vk::AttachmentDescription,
        ref_layout: vk::ImageLayout,
    ) -> vk::AttachmentReference {
        let index = self.attachments.descriptions.len();
        self.attachments.descriptions.push(description);
        
        vk::AttachmentReference {
            attachment: index as u32,
            layout: ref_layout,
        }
    }

    pub fn add_attachment(
        mut self,
        description: vk::AttachmentDescription,
        ref_layout: vk::ImageLayout,
    ) -> Self {
        let reference = self.add_single(description, ref_layout);
        self.attachment_refs.push(reference);
        self
    }

    pub fn into_refs(self) -> Vec<vk::AttachmentReference> {
        self.attachment_refs
    }
}

// TODO: lift into VulkanBaseExt/ VulkanBaseWrap
fn setup_renderer_from_base(base: &mut VulkanBase) -> Renderer {

    // input data
    let index_buffer_data = [0u32, 1, 2, 2, 3, 0];
    let image = image::load_from_memory(include_bytes!("../../../../assets/ping.png"))
        .unwrap()
        .to_rgba8();

    let (width, height) = image.dimensions();
    let image_extent = vk::Extent2D { width, height };
    let image_data = image.into_raw();
    let uniform_color_buffer_data = [Vector3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
        _pad: 0.0,
    }];
    let vertex_data = [
        Vertex {
            pos: [-1.0, -1.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
        Vertex {
            pos: [-1.0, 1.0, 0.0, 1.0],
            uv: [0.0, 1.0],
        },
        Vertex {
            pos: [1.0, 1.0, 0.0, 1.0],
            uv: [1.0, 1.0],
        },
        Vertex {
            pos: [1.0, -1.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        },
    ];
    // end of input data

    let (attachments, color, depth) = { VulkanBaseWrap(base).attachments() };
    let renderpass = { VulkanBaseWrap(base).renderpass(attachments.all(), &color, &depth) };
    let framebuffers: Vec<vk::Framebuffer> =
        { VulkanBaseWrap(base).framebuffers(renderpass) }.unwrap();

    let (index_buffer, index_buffer_memory) =
        init_buffer_with(base, vk::BufferUsageFlags::INDEX_BUFFER, &index_buffer_data).unwrap();
    let (vertex_input_buffer, vertex_input_buffer_memory) =
        init_buffer_with(base, vk::BufferUsageFlags::VERTEX_BUFFER, &vertex_data).unwrap();
    let (uniform_color_buffer, uniform_color_buffer_memory) = init_buffer_with(
        base,
        vk::BufferUsageFlags::UNIFORM_BUFFER,
        &uniform_color_buffer_data,
    )
    .unwrap();
    let (image_buffer, image_buffer_memory) =
        init_buffer_with(base, vk::BufferUsageFlags::TRANSFER_SRC, &image_data).unwrap();
    let (texture_create_info, texture_image, texture_memory) =
        init_texture_destination_buffer(base, image_extent).unwrap();

    // copy texture from cpu buffer to device by submitting a command buffer
    VulkanBase::record_submit_commandbuffer(
        &base.device,
        base.setup_command_buffer,
        base.setup_commands_reuse_fence,
        base.present_queue,
        &[],
        &[],
        &[],
        |device, texture_command_buffer| {
            let texture_barrier = vk::ImageMemoryBarrier {
                dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                image: texture_image,
                subresource_range: vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    level_count: 1,
                    layer_count: 1,
                    ..Default::default()
                },
                ..Default::default()
            };
            unsafe {
                device.cmd_pipeline_barrier(
                    texture_command_buffer,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[texture_barrier],
                )
            };
            let buffer_copy_regions = vk::BufferImageCopy::builder()
                .image_subresource(
                    vk::ImageSubresourceLayers::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1)
                        .build(),
                )
                .image_extent(image_extent.into());

            unsafe {
                device.cmd_copy_buffer_to_image(
                    texture_command_buffer,
                    image_buffer,
                    texture_image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[buffer_copy_regions.build()],
                )
            };
            let texture_barrier_end = vk::ImageMemoryBarrier {
                src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                dst_access_mask: vk::AccessFlags::SHADER_READ,
                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                image: texture_image,
                subresource_range: vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    level_count: 1,
                    layer_count: 1,
                    ..Default::default()
                },
                ..Default::default()
            };
            unsafe {
                device.cmd_pipeline_barrier(
                    texture_command_buffer,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[texture_barrier_end],
                )
            };
        },
    );


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

    let sampler = unsafe { base.device.create_sampler(&sampler_info, None) }
        .map_err(VulkanError::VkResult)
        .unwrap();

    let tex_image_view_info = vk::ImageViewCreateInfo {
        view_type: vk::ImageViewType::TYPE_2D,
        format: texture_create_info.format,
        components: vk::ComponentMapping {
            r: vk::ComponentSwizzle::R,
            g: vk::ComponentSwizzle::G,
            b: vk::ComponentSwizzle::B,
            a: vk::ComponentSwizzle::A,
        },
        subresource_range: vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            level_count: 1,
            layer_count: 1,
            ..Default::default()
        },
        image: texture_image,
        ..Default::default()
    };
    let tex_image_view = unsafe { base.device.create_image_view(&tex_image_view_info, None) }
        .map_err(VulkanError::VkResult)
        .unwrap();
    let descriptor_sizes = [
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::UNIFORM_BUFFER,
            descriptor_count: 1,
        },
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 1,
        },
    ];
    let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder()
        .pool_sizes(&descriptor_sizes)
        .max_sets(1);

    let descriptor_pool = unsafe {
        base.device
            .create_descriptor_pool(&descriptor_pool_info, None)
    }
    .map_err(VulkanError::VkResult)
    .unwrap();

    let desc_layout_bindings = [
        vk::DescriptorSetLayoutBinding {
            descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::FRAGMENT,
            ..Default::default()
        },
        vk::DescriptorSetLayoutBinding {
            binding: 1,
            descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 1,
            stage_flags: vk::ShaderStageFlags::FRAGMENT,
            ..Default::default()
        },
    ];
    let descriptor_info =
        vk::DescriptorSetLayoutCreateInfo::builder().bindings(&desc_layout_bindings);

    let desc_set_layouts = [unsafe {
        base.device
            .create_descriptor_set_layout(&descriptor_info, None)
    }
    .map_err(VulkanError::VkResult)
    .unwrap()];

    let desc_alloc_info = vk::DescriptorSetAllocateInfo::builder()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&desc_set_layouts);

    let descriptor_sets = unsafe { base.device.allocate_descriptor_sets(&desc_alloc_info) }
        .map_err(VulkanError::VkResult)
        .unwrap();

    let uniform_color_buffer_descriptor = vk::DescriptorBufferInfo {
        buffer: uniform_color_buffer,
        offset: 0,
        range: (uniform_color_buffer_data.len() * std::mem::size_of::<Vector3>()) as u64,
    };

    let tex_descriptor = vk::DescriptorImageInfo {
        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        image_view: tex_image_view,
        sampler,
    };

    let write_desc_sets = [
        vk::WriteDescriptorSet {
            dst_set: descriptor_sets[0],
            descriptor_count: 1,
            descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
            p_buffer_info: &uniform_color_buffer_descriptor,
            ..Default::default()
        },
        vk::WriteDescriptorSet {
            dst_set: descriptor_sets[0],
            dst_binding: 1,
            descriptor_count: 1,
            descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            p_image_info: &tex_descriptor,
            ..Default::default()
        },
    ];
    unsafe { base.device.update_descriptor_sets(&write_desc_sets, &[]) };

    let mut vertex_spv_file =
        Cursor::new(&include_bytes!("../../../../assets/shaders/vert.spv")[..]);
    let mut frag_spv_file = Cursor::new(&include_bytes!("../../../../assets/shaders/frag.spv")[..]);

    let vertex_code =
        ash::util::read_spv(&mut vertex_spv_file).expect("Failed to read vertex shader spv file");
    let vertex_shader_info = vk::ShaderModuleCreateInfo::builder().code(&vertex_code);

    let frag_code =
        ash::util::read_spv(&mut frag_spv_file).expect("Failed to read fragment shader spv file");
    let frag_shader_info = vk::ShaderModuleCreateInfo::builder().code(&frag_code);

    let vertex_shader_module =
        unsafe { base.device.create_shader_module(&vertex_shader_info, None) }
            .expect("Vertex shader module error");

    let fragment_shader_module =
        unsafe { base.device.create_shader_module(&frag_shader_info, None) }
            .expect("Fragment shader module error");

    let layout_create_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(&desc_set_layouts);

    let pipeline_layout = unsafe {
        base.device
            .create_pipeline_layout(&layout_create_info, None)
    }
    .map_err(VulkanError::VkResult)
    .unwrap();

    let shader_entry_name = unsafe { CStr::from_bytes_with_nul_unchecked(b"main\0") };
    let shader_stage_create_infos = [
        vk::PipelineShaderStageCreateInfo {
            module: vertex_shader_module,
            p_name: shader_entry_name.as_ptr(),
            stage: vk::ShaderStageFlags::VERTEX,
            ..Default::default()
        },
        vk::PipelineShaderStageCreateInfo {
            module: fragment_shader_module,
            p_name: shader_entry_name.as_ptr(),
            stage: vk::ShaderStageFlags::FRAGMENT,
            ..Default::default()
        },
    ];
    let vertex_input_binding_descriptions = [vk::VertexInputBindingDescription {
        binding: 0,
        stride: std::mem::size_of::<Vertex>() as u32,
        input_rate: vk::VertexInputRate::VERTEX,
    }];
    let vertex_input_attribute_descriptions = [
        vk::VertexInputAttributeDescription {
            location: 0,
            binding: 0,
            format: vk::Format::R32G32B32A32_SFLOAT,
            offset: offset_of!(Vertex, pos) as u32,
        },
        vk::VertexInputAttributeDescription {
            location: 1,
            binding: 0,
            format: vk::Format::R32G32_SFLOAT,
            offset: offset_of!(Vertex, uv) as u32,
        },
    ];
    let vertex_input_state_info = vk::PipelineVertexInputStateCreateInfo::builder()
        .vertex_attribute_descriptions(&vertex_input_attribute_descriptions)
        .vertex_binding_descriptions(&vertex_input_binding_descriptions);

    let vertex_input_assembly_state_info = vk::PipelineInputAssemblyStateCreateInfo {
        topology: vk::PrimitiveTopology::TRIANGLE_LIST,
        ..Default::default()
    };
    let viewports = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: base.surface_resolution.width as f32,
        height: base.surface_resolution.height as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    }];
    let scissors = [base.surface_resolution.into()];
    let viewport_state_info = vk::PipelineViewportStateCreateInfo::builder()
        .scissors(&scissors)
        .viewports(&viewports);

    let rasterization_info = vk::PipelineRasterizationStateCreateInfo {
        front_face: vk::FrontFace::COUNTER_CLOCKWISE,
        line_width: 1.0,
        polygon_mode: vk::PolygonMode::FILL,
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

    let graphic_pipeline_infos = vk::GraphicsPipelineCreateInfo::builder()
        .stages(&shader_stage_create_infos)
        .vertex_input_state(&vertex_input_state_info)
        .input_assembly_state(&vertex_input_assembly_state_info)
        .viewport_state(&viewport_state_info)
        .rasterization_state(&rasterization_info)
        .multisample_state(&multisample_state_info)
        .depth_stencil_state(&depth_state_info)
        .color_blend_state(&color_blend_state)
        .dynamic_state(&dynamic_state_info)
        .layout(pipeline_layout)
        .render_pass(renderpass);

    let graphics_pipelines = unsafe {
        base.device.create_graphics_pipelines(
            vk::PipelineCache::null(),
            &[graphic_pipeline_infos.build()],
            None,
        )
    }
    .map_err(|(pipeline, result)| VulkanError::FailedToCreatePipeline(pipeline, result))
    .unwrap();

    Renderer {
        graphics_pipelines,
        renderpass,
        pipeline_layout,
        framebuffers,
        descriptor_sets,
        vertex_shader_module,
        fragment_shader_module,
        index_buffer,
        index_buffer_data,
        viewports: viewports.to_vec(),
        scissors: scissors.to_vec(),
        image_buffer_memory,
        image_buffer,
        texture_memory,
        tex_image_view,
        texture_image,
        index_buffer_memory,
        uniform_color_buffer_memory,
        uniform_color_buffer,
        vertex_input_buffer_memory,
        vertex_input_buffer,
        desc_set_layouts: desc_set_layouts.to_vec(),
        descriptor_pool,
        sampler,
    }
}

/// Allocate a destination buffer for a texture based on an Extent2D
/// TODO: move to VulkanBase
fn init_texture_destination_buffer(
    base: &mut VulkanBase,
    image_extent: vk::Extent2D,
) -> Result<(vk::ImageCreateInfo, vk::Image, vk::DeviceMemory), VulkanError> {
    let texture_create_info = vk::ImageCreateInfo {
        image_type: vk::ImageType::TYPE_2D,
        format: vk::Format::R8G8B8A8_UNORM,
        extent: image_extent.into(),
        mip_levels: 1,
        array_layers: 1,
        samples: vk::SampleCountFlags::TYPE_1,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        sharing_mode: vk::SharingMode::EXCLUSIVE,
        ..Default::default()
    };
    let texture_image = unsafe { base.device.create_image(&texture_create_info, None) }
        .map_err(VulkanError::VkResult)?;
    let texture_memory_req = unsafe { base.device.get_image_memory_requirements(texture_image) };
    let texture_memory_index = VulkanBase::find_memorytype_index(
        &texture_memory_req,
        &base.device_memory_properties,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )
    .ok_or(VulkanError::UnableToFindMemoryTypeForBuffer)?;

    let texture_allocate_info = vk::MemoryAllocateInfo {
        allocation_size: texture_memory_req.size,
        memory_type_index: texture_memory_index,
        ..Default::default()
    };

    let texture_memory = unsafe { base.device.allocate_memory(&texture_allocate_info, None) }
        .map_err(VulkanError::VkResult)?;

    unsafe {
        base.device
            .bind_image_memory(texture_image, texture_memory, 0)
    }
    .map_err(VulkanError::VkResult)?;

    Ok((texture_create_info, texture_image, texture_memory))
}

#[derive(thiserror::Error, Debug)]
pub enum VulkanError {
    #[error("Unable to find suitable memorytype for the buffer")]
    UnableToFindMemoryTypeForBuffer,

    #[error("vk result")]
    VkResult(vk::Result),

    #[error("failed to create pipeline")]
    FailedToCreatePipeline(Vec<vk::Pipeline>, vk::Result),
}

/// Allocate a buffer with usage flags
/// TODO: move to vulkanbase
pub fn init_buffer_with<T>(
    base: &mut VulkanBase,
    usage: vk::BufferUsageFlags,
    data: &[T],
) -> Result<(vk::Buffer, vk::DeviceMemory), VulkanError>
where
    T: Copy,
{
    let buffer_info = vk::BufferCreateInfo {
        size: (data.len() * std::mem::size_of::<T>()) as u64,
        usage,
        sharing_mode: vk::SharingMode::EXCLUSIVE,
        ..Default::default()
    };
    let buffer =
        unsafe { base.device.create_buffer(&buffer_info, None) }.map_err(VulkanError::VkResult)?;
    let buffer_memory_req = unsafe { base.device.get_buffer_memory_requirements(buffer) };
    let buffer_memory_index = VulkanBase::find_memorytype_index(
        &buffer_memory_req,
        &base.device_memory_properties,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )
    .ok_or(VulkanError::UnableToFindMemoryTypeForBuffer)?;

    let allocate_info = vk::MemoryAllocateInfo {
        allocation_size: buffer_memory_req.size,
        memory_type_index: buffer_memory_index,
        ..Default::default()
    };
    let buffer_memory = unsafe { base.device.allocate_memory(&allocate_info, None) }
        .map_err(VulkanError::VkResult)?;

    let ptr = unsafe {
        base.device.map_memory(
            buffer_memory,
            0,
            buffer_memory_req.size,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(VulkanError::VkResult)?;

    let mut slice = unsafe { Align::new(ptr, align_of::<T>() as u64, buffer_memory_req.size) };
    slice.copy_from_slice(data);
    unsafe { base.device.unmap_memory(buffer_memory) };
    unsafe { base.device.bind_buffer_memory(buffer, buffer_memory, 0) }
        .map_err(VulkanError::VkResult)?;
    Ok((buffer, buffer_memory))
}
struct VulkanBaseWrap<'a>(&'a mut VulkanBase);

impl<'a> VulkanBaseWrap<'a> {
    /// Create attachments for renderpass construction
    fn attachments(
        &mut self,
    ) -> (
        Attachments,
        Vec<vk::AttachmentReference>,
        vk::AttachmentReference,
    ) {
        let mut attachments = Attachments::default();
        let color_attachment_refs = AttachmentsModifier::new(&mut attachments)
            .add_attachment(
                vk::AttachmentDescription::builder()
                    .format(self.0.surface_format.format)
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

    /// Create a renderpass with attachments
    fn renderpass(
        &mut self,
        all_attachments: &[vk::AttachmentDescription],
        color_attachment_refs: &[vk::AttachmentReference],
        depth_attachment_ref: &vk::AttachmentReference,
    ) -> vk::RenderPass {
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
        
        unsafe {
            self.0
                .device
                .create_render_pass(&renderpass_create_info, None)
        }
        .unwrap()
    }

    /// Consume the renderpass and hand back framebuffers
    fn framebuffers(
        &mut self,
        renderpass: vk::RenderPass,
    ) -> Result<Vec<vk::Framebuffer>, VulkanError> {
        let mut framebuffers = Vec::new();
        for present_image_view in self.0.present_image_views.iter() {
            let framebuffer_attachments = [*present_image_view, self.0.depth_image_view];
            let frame_buffer_create_info = vk::FramebufferCreateInfo::builder()
                .render_pass(renderpass)
                .attachments(&framebuffer_attachments)
                .width(self.0.surface_resolution.width)
                .height(self.0.surface_resolution.height)
                .layers(1);

            let framebuffer = unsafe {
                self.0
                    .device
                    .create_framebuffer(&frame_buffer_create_info, None)
            }
            .map_err(VulkanError::VkResult)?;
            framebuffers.push(framebuffer);
        }
        Ok(framebuffers)
    }
}

struct Renderer {
    graphics_pipelines: Vec<vk::Pipeline>,
    pipeline_layout: vk::PipelineLayout,
    renderpass: vk::RenderPass,
    framebuffers: Vec<vk::Framebuffer>,
    descriptor_sets: Vec<vk::DescriptorSet>,
    vertex_shader_module: vk::ShaderModule,
    fragment_shader_module: vk::ShaderModule,
    index_buffer: vk::Buffer,
    index_buffer_data: [u32; 6],
    viewports: Vec<vk::Viewport>,
    scissors: Vec<vk::Rect2D>,
    image_buffer_memory: vk::DeviceMemory,
    image_buffer: vk::Buffer,
    texture_memory: vk::DeviceMemory,
    tex_image_view: vk::ImageView,
    texture_image: vk::Image,
    index_buffer_memory: vk::DeviceMemory,
    uniform_color_buffer_memory: vk::DeviceMemory,
    uniform_color_buffer: vk::Buffer,
    vertex_input_buffer_memory: vk::DeviceMemory,
    vertex_input_buffer: vk::Buffer,
    desc_set_layouts: Vec<vk::DescriptorSetLayout>,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
}

fn present(renderer: &Renderer, base: &mut VulkanBase) -> Result<(), VulkanError> {
    let (present_index, _) = unsafe {
        base.swapchain_loader.acquire_next_image(
            base.swapchain,
            std::u64::MAX,
            base.present_complete_semaphore,
            vk::Fence::null(),
        )
    }
    .map_err(VulkanError::VkResult)?;

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

    let render_pass_begin_info = vk::RenderPassBeginInfo::builder()
        .render_pass(renderer.renderpass)
        .framebuffer(renderer.framebuffers[present_index as usize])
        .render_area(base.surface_resolution.into())
        .clear_values(&clear_values);

    VulkanBase::record_submit_commandbuffer(
        &base.device,
        base.draw_command_buffer,
        base.draw_commands_reuse_fence,
        base.present_queue,
        &[vk::PipelineStageFlags::BOTTOM_OF_PIPE],
        &[base.present_complete_semaphore],
        &[base.rendering_complete_semaphore],
        |device, draw_command_buffer| {
            unsafe {
                device.cmd_begin_render_pass(
                    draw_command_buffer,
                    &render_pass_begin_info,
                    vk::SubpassContents::INLINE,
                );
                device.cmd_bind_descriptor_sets(
                    draw_command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    renderer.pipeline_layout,
                    0,
                    &renderer.descriptor_sets[..],
                    &[],
                );
                device.cmd_bind_pipeline(
                    draw_command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    renderer.graphics_pipelines[0],
                );
                device.cmd_set_viewport(draw_command_buffer, 0, &renderer.viewports);
                device.cmd_set_scissor(draw_command_buffer, 0, &renderer.scissors);
                device.cmd_bind_vertex_buffers(
                    draw_command_buffer,
                    0,
                    &[renderer.vertex_input_buffer],
                    &[0],
                );
                device.cmd_bind_index_buffer(
                    draw_command_buffer,
                    renderer.index_buffer,
                    0,
                    vk::IndexType::UINT32,
                );
                device.cmd_draw_indexed(
                    draw_command_buffer,
                    renderer.index_buffer_data.len() as u32,
                    1,
                    0,
                    0,
                    1,
                );
                // Or draw without the index buffer
                // device.cmd_draw(draw_command_buffer, 3, 1, 0, 0);
                device.cmd_end_render_pass(draw_command_buffer)
            };
        },
    );

    let present_info = vk::PresentInfoKHR {
        wait_semaphore_count: 1,
        p_wait_semaphores: &base.rendering_complete_semaphore,
        swapchain_count: 1,
        p_swapchains: &base.swapchain,
        p_image_indices: &present_index,
        ..Default::default()
    };

    unsafe {
        base.swapchain_loader
            .queue_present(base.present_queue, &present_info)
    }
    .map_err(VulkanError::VkResult)?;

    Ok(())
}

fn drop_resources(base: &mut VulkanBase, renderer: &Renderer) -> Result<(), VulkanError> {
    unsafe {
        base.device
            .device_wait_idle()
            .map_err(VulkanError::VkResult)?;
        for pipeline in renderer.graphics_pipelines.iter() {
            base.device.destroy_pipeline(*pipeline, None);
        }
        base.device
            .destroy_pipeline_layout(renderer.pipeline_layout, None);
        base.device
            .destroy_shader_module(renderer.vertex_shader_module, None);
        base.device
            .destroy_shader_module(renderer.fragment_shader_module, None);
        base.device.free_memory(renderer.image_buffer_memory, None);
        base.device.destroy_buffer(renderer.image_buffer, None);
        base.device.free_memory(renderer.texture_memory, None);
        base.device
            .destroy_image_view(renderer.tex_image_view, None);
        base.device.destroy_image(renderer.texture_image, None);
        base.device.free_memory(renderer.index_buffer_memory, None);
        base.device.destroy_buffer(renderer.index_buffer, None);
        base.device
            .free_memory(renderer.uniform_color_buffer_memory, None);
        base.device
            .destroy_buffer(renderer.uniform_color_buffer, None);
        base.device
            .free_memory(renderer.vertex_input_buffer_memory, None);
        base.device
            .destroy_buffer(renderer.vertex_input_buffer, None);
        for &descriptor_set_layout in renderer.desc_set_layouts.iter() {
            base.device
                .destroy_descriptor_set_layout(descriptor_set_layout, None);
        }
        base.device
            .destroy_descriptor_pool(renderer.descriptor_pool, None);
        base.device.destroy_sampler(renderer.sampler, None);
        for framebuffer in renderer.framebuffers.iter() {
            base.device.destroy_framebuffer(*framebuffer, None);
        }
        base.device.destroy_render_pass(renderer.renderpass, None);
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn load(state: &mut RenderState) {
    println!("loaded ash_renderer_plugin");

    let mut base = VulkanBase::new(state.win_ptr);
    state.vulkan.presenter = Some(Box::pin(setup_renderer_from_base(&mut base)));
    state.vulkan.base = Some(base);
}

#[no_mangle]
pub extern "C" fn update(state: &mut RenderState, dt: &Duration) {
    // Call render, buffers are updated etc
    if state.updates % 600 == 0 {
        println!("state: {} dt: {:?}", state.updates, dt);
    }
    if let (Some(present), Some(base)) = (&state.vulkan.presenter, &mut state.vulkan.base) {
        present.present(base);
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut RenderState) {
    println!("unloaded ash_renderer_plugin");
    state.vulkan.presenter.take();
    state.vulkan.base.take();
}
