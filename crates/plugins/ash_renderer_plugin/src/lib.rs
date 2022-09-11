use std::{fs::File, io::BufReader, mem::align_of, time::Duration};

use ash::{util::Align, vk};
use models::{Image, Vertex};
use render::{
    types::{
        Attachments, AttachmentsModifier, BufferAndMemory, GpuModelRef, PipelineDesc,
        ShaderBindingDesc, ShaderDesc, ShaderStage, ShaderStages, Texture, VertexInputAssembly,
        VulkanError,
    },
    Presenter, RenderState, VulkanBase,
};
use world::Matrix4;

impl Renderer {
    fn present_with_base(&self, base: &mut VulkanBase) -> Result<(), VulkanError> {
        let (present_index, _) = unsafe {
            base.swapchain_loader.acquire_next_image(
                base.swapchain,
                std::u64::MAX,
                base.present_complete_semaphore,
                vk::Fence::null(),
            )
        }
        .map_err(VulkanError::SwapchainAquireNextImage)?;

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

        let w = DeviceWrap(&base.device);

        w.wait_for_fence(base.draw_commands_reuse_fence)?;
        w.reset_fence(base.draw_commands_reuse_fence)?;

        w.begin_command_buffer(base.draw_cmd_buf)?;

        let render_pass_begin_info = vk::RenderPassBeginInfo::builder()
            .render_pass(self.render_pass)
            .framebuffer(self.framebuffers[present_index as usize])
            .render_area(base.surface_resolution.into())
            .clear_values(&clear_values);

        w.cmd_begin_render_pass(
            base.draw_cmd_buf,
            &render_pass_begin_info,
            vk::SubpassContents::INLINE,
        );

        // TODO: iterate over scene's Things, not uploaded_models.
        // From there, we can get a model matrix from the physical facet.
        // FOR NOW: hard-code a matrix.
        let model_mat = Matrix4::<f32>::identity();
        let model_mat = model_mat.as_slice();
        let mut mat = [0f32; 16];
        mat.copy_from_slice(&model_mat);
        let push_constant_bytes = bytemuck::bytes_of(&mat);

        for (index, (_model_index, model)) in base.uploaded_models.iter().enumerate() {
            let desc = &self.pipeline_descriptions[index];
            let pipeline = self.graphics_pipelines[index];
            w.cmd_bind_descriptor_sets(
                base.draw_cmd_buf,
                vk::PipelineBindPoint::GRAPHICS,
                desc.layout,
                0,
                &[desc.descriptor_set],
                &[],
            );
            w.cmd_bind_pipeline(base.draw_cmd_buf, vk::PipelineBindPoint::GRAPHICS, pipeline);
            w.cmd_set_viewport(base.draw_cmd_buf, 0, &desc.viewports);
            w.cmd_set_scissor(base.draw_cmd_buf, 0, &desc.scissors);
            w.cmd_bind_vertex_buffers(base.draw_cmd_buf, 0, &[model.vertex_buffer.buffer], &[0]);
            w.cmd_bind_index_buffer(
                base.draw_cmd_buf,
                model.index_buffer.buffer,
                0,
                vk::IndexType::UINT32,
            );
            w.cmd_push_constants(
                base.draw_cmd_buf,
                desc.layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                push_constant_bytes,
            );
            w.cmd_draw_indexed(base.draw_cmd_buf, model.index_buffer.len as u32, 1, 0, 0, 1);
        }
        w.cmd_end_render_pass(base.draw_cmd_buf);

        let command_buffers = vec![base.draw_cmd_buf];
        let submit_info = vk::SubmitInfo::builder()
            .wait_semaphores(&[base.present_complete_semaphore])
            .wait_dst_stage_mask(&[vk::PipelineStageFlags::BOTTOM_OF_PIPE])
            .command_buffers(&command_buffers)
            .signal_semaphores(&[base.rendering_complete_semaphore])
            .build();

        w.end_command_buffer(base.draw_cmd_buf)?;

        w.queue_submit(
            base.draw_commands_reuse_fence,
            base.present_queue,
            &[submit_info],
        )?;

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
        .map_err(VulkanError::VkResultToDo)?;

        Ok(())
    }

    fn drop_resources_with_base(&mut self, base: &mut VulkanBase) -> Result<(), VulkanError> {
        unsafe {
            base.device
                .device_wait_idle()
                .map_err(VulkanError::VkResultToDo)?;

            for pipeline in self.graphics_pipelines.iter() {
                base.device.destroy_pipeline(*pipeline, None);
            }
            for desc in self.pipeline_descriptions.iter() {
                desc.deallocate(&base.device);
            }
            for &descriptor_set_layout in self.desc_set_layouts.iter() {
                base.device
                    .destroy_descriptor_set_layout(descriptor_set_layout, None);
            }
            base.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            for framebuffer in self.framebuffers.iter() {
                base.device.destroy_framebuffer(*framebuffer, None);
            }
            base.device.destroy_render_pass(self.render_pass, None);
            Ok(())
        }
    }
}

impl Presenter for Renderer {
    fn present(&self, base: &mut VulkanBase) {
        self.present_with_base(base).unwrap();
    }
    fn drop_resources(&mut self, base: &mut VulkanBase) {
        self.drop_resources_with_base(base).unwrap();
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

pub struct VulkanBaseWrapper<'a>(&'a mut VulkanBase);

impl<'a> VulkanBaseWrapper<'a> {
    pub fn new(base: &'a mut VulkanBase) -> Self {
        Self(base)
    }

    pub fn renderer(&mut self) -> Result<Renderer, VulkanError> {
        let (attachments, color, depth) = self.attachments();
        let render_pass = self.render_pass(attachments.all(), &color, &depth);
        let framebuffers = self.framebuffers(render_pass)?;

        // TODO: shaders that apply only to certain models need different descriptor sets.
        //? TODO: any pool can be a thread local, but then any object must be destroyed on that thread.
        let descriptor_pool = self.descriptor_pool(10, 4, 4)?;

        let mut desc_set_layouts = Vec::new();
        let mut mirrored_model_indices = Vec::new();
        let mut pipeline_descriptions = Vec::new();

        // For now we are creating a pipeline per model.
        for (model_index, model) in self.0.uploaded_models.iter() {
            let uniform_buffer = {
                let device = DeviceWrap(&self.0.device);
                device.allocate_and_init_buffer(
                    vk::BufferUsageFlags::UNIFORM_BUFFER,
                    self.0.device_memory_properties,
                    Matrix4::<f32>::identity().as_slice(),
                )?
            };

            desc_set_layouts
                .push(self.descriptor_set_layout(model.shaders.desc_set_layout_bindings.clone())?);
            mirrored_model_indices.push(model_index);
            let descriptor_sets =
                self.allocate_descriptor_sets(descriptor_pool, &desc_set_layouts)?;

            let sampler = self.sampler()?;

            let descriptor_set = descriptor_sets[0];
            Self::update_descriptor_set(
                &self.0.device,
                descriptor_set,
                uniform_buffer.buffer,
                model.texture.image_view,
                sampler,
            );
            let mut vertex_spv_file = BufReader::new(
                File::open(&model.shaders.vertex_shader).map_err(VulkanError::ShaderRead)?,
            );
            let mut frag_spv_file = BufReader::new(
                File::open(&model.shaders.fragment_shader).map_err(VulkanError::ShaderRead)?,
            );

            let mut shader_stages = ShaderStages::new();
            shader_stages.add_shader(
                &self.0.device,
                &mut vertex_spv_file,
                "vertex_main",
                vk::ShaderStageFlags::VERTEX,
                vk::PipelineShaderStageCreateFlags::empty(),
            )?;
            shader_stages.add_shader(
                &self.0.device,
                &mut frag_spv_file,
                "fragment_main",
                vk::ShaderStageFlags::FRAGMENT,
                vk::PipelineShaderStageCreateFlags::empty(),
            )?;

            let mut vertex_input_assembly =
                VertexInputAssembly::new(vk::PrimitiveTopology::TRIANGLE_LIST);
            vertex_input_assembly.add_binding_description::<Vertex>(0, vk::VertexInputRate::VERTEX);
            vertex_input_assembly.add_attribute_description(
                0,
                0,
                vk::Format::R32G32B32A32_SFLOAT,
                offset_of!(Vertex, pos) as u32,
            );
            vertex_input_assembly.add_attribute_description(
                1,
                0,
                vk::Format::R32G32B32_SFLOAT,
                offset_of!(Vertex, normal) as u32,
            );
            vertex_input_assembly.add_attribute_description(
                2,
                0,
                vk::Format::R32G32_SFLOAT,
                offset_of!(Vertex, uv) as u32,
            );

            pipeline_descriptions.push(PipelineDesc::new(
                uniform_buffer,
                descriptor_sets[0],
                sampler,
                Self::pipeline_layout(&self.0.device, &desc_set_layouts)?,
                self.viewports(),
                self.scissors(),
                shader_stages,
                vertex_input_assembly,
            ));
        }

        let graphics_pipelines = self.graphics_pipelines(&pipeline_descriptions, render_pass)?;

        Ok(Renderer {
            desc_set_layouts,
            descriptor_pool,
            framebuffers,
            graphics_pipelines,
            render_pass,
            pipeline_descriptions,
        })
    }

    pub fn scissors(&self) -> Vec<vk::Rect2D> {
        vec![self.0.surface_resolution.into()]
    }

    pub fn viewports(&self) -> Vec<vk::Viewport> {
        vec![vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.0.surface_resolution.width as f32,
            height: self.0.surface_resolution.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }]
    }

    pub fn graphics_pipelines(
        &mut self,
        pipeline_deps: &[PipelineDesc],
        render_pass: vk::RenderPass,
    ) -> Result<Vec<vk::Pipeline>, VulkanError> {
        let mut graphics_pipelines = Vec::new();
        for desc in pipeline_deps {
            let shader_stage_create_infos: Vec<vk::PipelineShaderStageCreateInfo> = desc
                .shader_stages
                .shader_stage_defs
                .iter()
                .map(ShaderStage::create_info)
                .collect();

            let viewport_state_info = vk::PipelineViewportStateCreateInfo::builder()
                .scissors(&desc.scissors)
                .viewports(&desc.viewports);

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

            let vertex_input_state_info = desc.vertex_input_assembly.input_state_info();
            let vertex_input_assembly_state_info = desc.vertex_input_assembly.assembly_state_info();

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
                .layout(desc.layout)
                .render_pass(render_pass);

            let pipelines = unsafe {
                self.0.device.create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    &[graphics_pipeline_info.build()],
                    None,
                )
            }
            .map_err(|(pipeline, result)| VulkanError::FailedToCreatePipeline(pipeline, result))?;
            graphics_pipelines.extend(pipelines);
        }
        Ok(graphics_pipelines)
    }

    pub fn pipeline_layout(
        device: &ash::Device,
        desc_set_layouts: &[vk::DescriptorSetLayout],
    ) -> Result<vk::PipelineLayout, VulkanError> {
        let layout_create_info =
            vk::PipelineLayoutCreateInfo::builder().set_layouts(desc_set_layouts);
        unsafe { device.create_pipeline_layout(&layout_create_info, None) }
            .map_err(VulkanError::VkResultToDo)
    }

    // This could be updated to update many descriptor sets in bulk, however we only have one we care
    // about, per-pipeline when this was written.
    pub fn update_descriptor_set(
        device: &ash::Device,
        descriptor_set: vk::DescriptorSet,
        uniform_buffer: vk::Buffer,
        tex_image_view: vk::ImageView,
        sampler: vk::Sampler,
    ) {
        let uniform_descriptor = vk::DescriptorBufferInfo::builder()
            .buffer(uniform_buffer)
            .build();

        let tex_descriptor = vk::DescriptorImageInfo {
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            image_view: tex_image_view,
            sampler,
        };
        let write_desc_sets = [
            vk::WriteDescriptorSet::builder()
                .dst_set(descriptor_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&[uniform_descriptor])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(descriptor_set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&[tex_descriptor])
                .build(),
        ];
        unsafe { device.update_descriptor_sets(&write_desc_sets, &[]) };
    }

    pub fn allocate_descriptor_sets(
        &self,
        pool: vk::DescriptorPool,
        layouts: &[vk::DescriptorSetLayout],
    ) -> Result<Vec<vk::DescriptorSet>, VulkanError> {
        let desc_alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(layouts);
        unsafe { self.0.device.allocate_descriptor_sets(&desc_alloc_info) }
            .map_err(VulkanError::VkResultToDo)
    }

    pub fn descriptor_set_layout(
        &self,
        bindings: Vec<ShaderBindingDesc>,
    ) -> Result<vk::DescriptorSetLayout, VulkanError> {
        let bindings: Vec<_> = bindings
            .into_iter()
            .map(|desc| desc.into_layout_binding())
            .collect();

        let descriptor_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
        let layout = unsafe {
            self.0
                .device
                .create_descriptor_set_layout(&descriptor_info, None)
        }
        .map_err(VulkanError::VkResultToDo)?;
        Ok(layout)
    }

    pub fn descriptor_pool(
        &mut self,
        max_sets: u32,
        max_samplers: u32,
        max_uniform_buffers: u32,
    ) -> Result<vk::DescriptorPool, VulkanError> {
        let descriptor_sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::UNIFORM_BUFFER,
                descriptor_count: max_uniform_buffers,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: max_samplers,
            },
        ];
        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&descriptor_sizes)
            .max_sets(max_sets);
        unsafe {
            self.0
                .device
                .create_descriptor_pool(&descriptor_pool_info, None)
        }
        .map_err(VulkanError::VkResultToDo)
    }

    pub fn sampler(&self) -> Result<vk::Sampler, VulkanError> {
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

        unsafe { self.0.device.create_sampler(&sampler_info, None) }
            .map_err(VulkanError::VkResultToDo)
    }

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
    fn render_pass(
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
        render_pass: vk::RenderPass,
    ) -> Result<Vec<vk::Framebuffer>, VulkanError> {
        let mut framebuffers = Vec::new();
        for present_image_view in self.0.present_image_views.iter() {
            let framebuffer_attachments = [*present_image_view, self.0.depth_image_view];
            let frame_buffer_create_info = vk::FramebufferCreateInfo::builder()
                .render_pass(render_pass)
                .attachments(&framebuffer_attachments)
                .width(self.0.surface_resolution.width)
                .height(self.0.surface_resolution.height)
                .layers(1);

            let framebuffer = unsafe {
                self.0
                    .device
                    .create_framebuffer(&frame_buffer_create_info, None)
            }
            .map_err(VulkanError::VkResultToDo)?;
            framebuffers.push(framebuffer);
        }
        Ok(framebuffers)
    }
}
pub struct Renderer {
    descriptor_pool: vk::DescriptorPool,
    desc_set_layouts: Vec<vk::DescriptorSetLayout>,
    framebuffers: Vec<vk::Framebuffer>,
    graphics_pipelines: Vec<vk::Pipeline>,
    render_pass: vk::RenderPass,
    pipeline_descriptions: Vec<PipelineDesc>,
}

pub struct DeviceWrap<'a>(&'a ash::Device);

impl<'a> DeviceWrap<'a> {
    fn wait_for_fence(&self, fence: vk::Fence) -> Result<(), VulkanError> {
        unsafe {
            self.0
                .wait_for_fences(&[fence], true, u64::MAX)
                .map_err(VulkanError::Fence)?;
            self.0
                .reset_fences(&[fence])
                .map_err(VulkanError::FenceReset)?;
        }
        Ok(())
    }

    fn allocate_texture_dest_buffer(
        &self,
        memory_properties: vk::PhysicalDeviceMemoryProperties,
        image_extent: vk::Extent2D,
    ) -> Result<Texture, VulkanError> {
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
        let texture_image = unsafe { self.0.create_image(&texture_create_info, None) }
            .map_err(VulkanError::VkResultToDo)?;
        let texture_memory_req = unsafe { self.0.get_image_memory_requirements(texture_image) };
        let texture_memory_index = VulkanBase::find_memorytype_index(
            &texture_memory_req,
            &memory_properties,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
        .ok_or(VulkanError::UnableToFindMemoryTypeForBuffer)?;

        let texture_allocate_info = vk::MemoryAllocateInfo {
            allocation_size: texture_memory_req.size,
            memory_type_index: texture_memory_index,
            ..Default::default()
        };

        let texture_memory = unsafe { self.0.allocate_memory(&texture_allocate_info, None) }
            .map_err(VulkanError::VkResultToDo)?;

        unsafe { self.0.bind_image_memory(texture_image, texture_memory, 0) }
            .map_err(VulkanError::VkResultToDo)?;

        Ok(Texture::new(
            texture_create_info.format,
            texture_image,
            texture_memory,
            &self.0,
        )?)
    }
    /// Allocate a buffer with usage flags, initialize with data.
    /// TODO: internalize
    pub fn allocate_and_init_buffer<T>(
        &self,
        usage: vk::BufferUsageFlags,
        memory_properties: vk::PhysicalDeviceMemoryProperties,
        data: &[T],
    ) -> Result<BufferAndMemory, VulkanError>
    where
        T: Copy,
    {
        let buffer_info = vk::BufferCreateInfo {
            size: (data.len() * std::mem::size_of::<T>()) as u64,
            usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            ..Default::default()
        };
        let buffer = unsafe { self.0.create_buffer(&buffer_info, None) }
            .map_err(VulkanError::VkResultToDo)?;
        let (allocation_size, memory_type_index) = self.memorytype_index_and_size_for_buffer(
            buffer,
            memory_properties,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size,
            memory_type_index,
            ..Default::default()
        };
        let buffer_memory = unsafe { self.0.allocate_memory(&allocate_info, None) }
            .map_err(VulkanError::VkResultToDo)?;
        let buffer = BufferAndMemory::new(buffer, buffer_memory, data.len());
        let ptr = unsafe {
            self.0.map_memory(
                buffer.memory,
                0,
                allocation_size,
                vk::MemoryMapFlags::empty(),
            )
        }
        .map_err(VulkanError::VkResultToDo)?;
        let mut slice = unsafe { Align::new(ptr, align_of::<T>() as u64, allocation_size) };
        slice.copy_from_slice(data);
        unsafe { self.0.unmap_memory(buffer.memory) };
        unsafe { self.0.bind_buffer_memory(buffer.buffer, buffer.memory, 0) }
            .map_err(VulkanError::VkResultToDo)?;

        Ok(buffer)
    }

    pub fn memorytype_index_and_size_for_buffer(
        &self,
        buffer: vk::Buffer,
        memory_properties: vk::PhysicalDeviceMemoryProperties,
        flags: vk::MemoryPropertyFlags,
    ) -> Result<(u64, u32), VulkanError> {
        let buffer_memory_req = unsafe { self.0.get_buffer_memory_requirements(buffer) };
        Ok((
            buffer_memory_req.size,
            VulkanBase::find_memorytype_index(&buffer_memory_req, &memory_properties, flags)
                .ok_or(VulkanError::UnableToFindMemoryTypeForBuffer)?,
        ))
    }
    pub fn create_fence(&self) -> Result<vk::Fence, VulkanError> {
        let fence_create_info = vk::FenceCreateInfo::builder()
            .flags(vk::FenceCreateFlags::SIGNALED)
            .build();
        unsafe { self.0.create_fence(&fence_create_info, None) }.map_err(VulkanError::VkResultToDo)
    }

    pub fn cmd_copy_buffer_to_image(
        &self,
        src_image: &BufferAndMemory,
        image_extent: vk::Extent2D,
        dest_texture: &Texture,
        command_buffer: vk::CommandBuffer,
    ) {
        let buffer_copy_regions = vk::BufferImageCopy::builder()
            .image_subresource(
                vk::ImageSubresourceLayers::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1)
                    .build(),
            )
            .image_extent(image_extent.into());

        unsafe {
            self.0.cmd_copy_buffer_to_image(
                command_buffer,
                src_image.buffer,
                dest_texture.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[buffer_copy_regions.build()],
            )
        }
    }

    pub fn queue_submit(
        &self,
        fence: vk::Fence,
        queue: vk::Queue,
        submits: &[vk::SubmitInfo],
    ) -> Result<(), VulkanError> {
        unsafe { self.0.queue_submit(queue, submits, fence) }
            .map_err(VulkanError::SubmitCommandBuffers)
    }

    pub fn end_command_buffer(&self, command_buffer: vk::CommandBuffer) -> Result<(), VulkanError> {
        unsafe { self.0.end_command_buffer(command_buffer) }.map_err(VulkanError::EndCommandBuffer)
    }

    // just a few flags are different between * and *_end versions, but need to better understand the
    pub fn cmd_pipeline_barrier_end(&self, image: vk::Image, command_buffer: vk::CommandBuffer) {
        let texture_barrier_end = vk::ImageMemoryBarrier {
            src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
            dst_access_mask: vk::AccessFlags::SHADER_READ,
            old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            image,
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                level_count: 1,
                layer_count: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            self.0.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[texture_barrier_end],
            )
        };
    }

    pub fn cmd_pipeline_barrier_start(&self, image: vk::Image, command_buffer: vk::CommandBuffer) {
        let texture_barrier = vk::ImageMemoryBarrier {
            dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
            new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            image,
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                level_count: 1,
                layer_count: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            self.0.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[texture_barrier],
            )
        }
    }

    pub fn copy_image_to_transfer_src_buffer(
        &self,
        image: &Image,
        memory_properties: vk::PhysicalDeviceMemoryProperties,
    ) -> Result<(vk::Extent2D, BufferAndMemory), VulkanError> {
        let image_extent = {
            let (width, height) = image.extent();
            vk::Extent2D { width, height }
        };
        let image_data = image.image.to_rgba8();
        self.allocate_and_init_buffer(
            vk::BufferUsageFlags::TRANSFER_SRC,
            memory_properties,
            &image_data,
        )
        .map(|image| (image_extent, image))
    }

    pub fn begin_command_buffer(
        &self,
        command_buffer: vk::CommandBuffer,
    ) -> Result<(), VulkanError> {
        let command_buffer_begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
            .build();
        unsafe {
            self.0
                .begin_command_buffer(command_buffer, &command_buffer_begin_info)
        }
        .map_err(VulkanError::BeginCommandBuffer)
    }

    pub fn allocate_command_buffers(
        &self,
        pool: vk::CommandPool,
    ) -> Result<Vec<vk::CommandBuffer>, VulkanError> {
        let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(1)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .build();
        unsafe {
            self.0
                .allocate_command_buffers(&command_buffer_allocate_info)
        }
        .map_err(VulkanError::VkResultToDo)
    }

    pub fn create_command_pool(
        &self,
        queue_family_index: u32,
    ) -> Result<vk::CommandPool, VulkanError> {
        let pool_create_info = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family_index)
            .build();
        unsafe { self.0.create_command_pool(&pool_create_info, None) }
            .map_err(VulkanError::VkResultToDo)
    }

    pub fn reset_fence(&self, fence: vk::Fence) -> Result<(), VulkanError> {
        unsafe { self.0.reset_fences(&[fence]) }.map_err(VulkanError::FenceReset)
    }

    pub fn cmd_begin_render_pass(
        &self,
        draw_command_buffer: vk::CommandBuffer,
        render_pass_begin_info: &vk::RenderPassBeginInfoBuilder,
        inline: vk::SubpassContents,
    ) {
        unsafe {
            self.0
                .cmd_begin_render_pass(draw_command_buffer, render_pass_begin_info, inline);
        }
    }

    pub fn cmd_bind_descriptor_sets(
        &self,
        draw_cmd_buf: vk::CommandBuffer,
        graphics: vk::PipelineBindPoint,
        pipeline_layout: vk::PipelineLayout,
        first_set: u32,
        descriptor_sets: &[vk::DescriptorSet],
        dynamic_offsets: &[u32],
    ) {
        unsafe {
            self.0.cmd_bind_descriptor_sets(
                draw_cmd_buf,
                graphics,
                pipeline_layout,
                first_set,
                descriptor_sets,
                dynamic_offsets,
            );
        }
    }

    pub fn cmd_bind_pipeline(
        &self,
        cmd: vk::CommandBuffer,
        graphics: vk::PipelineBindPoint,
        graphics_pipelines: vk::Pipeline,
    ) {
        unsafe {
            self.0.cmd_bind_pipeline(cmd, graphics, graphics_pipelines);
        }
    }

    pub fn cmd_set_viewport(&self, cmd: vk::CommandBuffer, first: u32, viewports: &[vk::Viewport]) {
        unsafe {
            self.0.cmd_set_viewport(cmd, first, viewports);
        }
    }

    pub fn cmd_set_scissor(&self, cmd: vk::CommandBuffer, first: u32, scissors: &[vk::Rect2D]) {
        unsafe { self.0.cmd_set_scissor(cmd, first, scissors) }
    }

    pub fn cmd_bind_vertex_buffers(
        &self,
        command_buffer: vk::CommandBuffer,
        first_binding: u32,
        buffers: &[vk::Buffer],
        offsets: &[vk::DeviceSize],
    ) {
        unsafe {
            self.0
                .cmd_bind_vertex_buffers(command_buffer, first_binding, buffers, offsets);
        }
    }

    pub fn cmd_bind_index_buffer(
        &self,
        command_buffer: vk::CommandBuffer,
        buffer: vk::Buffer,
        offset: vk::DeviceSize,
        index_type: vk::IndexType,
    ) {
        unsafe {
            self.0
                .cmd_bind_index_buffer(command_buffer, buffer, offset, index_type);
        }
    }

    pub fn cmd_push_constants(
        &self,
        command_buffer: vk::CommandBuffer,
        layout: vk::PipelineLayout,
        stage_flags: vk::ShaderStageFlags,
        offset: u32,
        constants: &[u8],
    ) {
        unsafe {
            self.0
                .cmd_push_constants(command_buffer, layout, stage_flags, offset, constants)
        }
    }

    pub fn cmd_draw_indexed(
        &self,
        command_buffer: vk::CommandBuffer,
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) {
        unsafe {
            self.0.cmd_draw_indexed(
                command_buffer,
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            );
        }
    }

    pub fn cmd_end_render_pass(&self, command_buffer: vk::CommandBuffer) {
        unsafe {
            self.0.cmd_end_render_pass(command_buffer);
        }
    }
}

#[no_mangle]
pub extern "C" fn load(state: &mut RenderState) {
    println!("loaded ash_renderer_plugin...");
    let mut base = VulkanBase::new(state.win_ptr, state.enable_validation_layer);

    // Command buffer requirements, thread safe? But cloning pointers
    // #[derive(Clone)]
    // struct CommandBufferReqs {
    //     device: ash::Device,
    //     queue_family_index: u32,
    //     queue: vk::Queue,
    //     device_memory_properties: vk::PhysicalDeviceMemoryProperties,
    // }
    // Cloning pointers...
    let device = base.device.clone();
    let queue = base.present_queue.clone();
    let queue_family_index = base.queue_family_index;
    let device_memory_properties = base.device_memory_properties;

    let w = DeviceWrap(&device);
    let pool = w.create_command_pool(queue_family_index).unwrap();
    let fence = w.create_fence().unwrap();

    // Store src image buffers for cleanup once complete.
    let mut src_images = Vec::new();

    for (index, model) in state.models.iter() {
        println!("loading model at {:?}...", index);
        println!("material {:?}", model.material.path);

        let command_buffers = w.allocate_command_buffers(pool).unwrap();
        let command_buffer = command_buffers[0];

        w.wait_for_fence(fence).unwrap();
        w.begin_command_buffer(command_buffer).unwrap();
        let image = &model.material.diffuse_map;
        let (image_extent, src_image) = w
            .copy_image_to_transfer_src_buffer(image, device_memory_properties)
            .unwrap();
        let dest_texture = w
            .allocate_texture_dest_buffer(device_memory_properties, image_extent)
            .unwrap();
        w.cmd_pipeline_barrier_start(dest_texture.image, command_buffer);
        w.cmd_copy_buffer_to_image(&src_image, image_extent, &dest_texture, command_buffer);
        w.cmd_pipeline_barrier_end(dest_texture.image, command_buffer);
        w.end_command_buffer(command_buffer).unwrap();

        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(&command_buffers)
            .build();
        w.queue_submit(fence, queue, &[submit_info]).unwrap();

        let vertex_buffer = w
            .allocate_and_init_buffer(
                vk::BufferUsageFlags::VERTEX_BUFFER,
                device_memory_properties,
                &model.mesh.vertices,
            )
            .unwrap();

        let index_buffer = w
            .allocate_and_init_buffer(
                vk::BufferUsageFlags::INDEX_BUFFER,
                device_memory_properties,
                &model.mesh.indices,
            )
            .unwrap();

        let uploaded_model = GpuModelRef::new(
            dest_texture,
            vertex_buffer,
            index_buffer,
            // TODO: generate this from model metadata! hardcoing this for now to move forward with model rendering
            ShaderDesc {
                desc_set_layout_bindings: vec![
                    ShaderBindingDesc {
                        binding: 1,
                        descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::VERTEX,
                    },
                    ShaderBindingDesc {
                        binding: 2,
                        descriptor_type: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                        descriptor_count: 1,
                        stage_flags: vk::ShaderStageFlags::FRAGMENT,
                    },
                ],
                vertex_shader: model.vertex_shader.clone(),
                fragment_shader: model.fragment_shader.clone(),
            },
        );
        src_images.push(src_image);
        base.track_uploaded_model(*index, uploaded_model);
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
    state.set_presenter(Box::new(
        VulkanBaseWrapper::new(&mut base)
            .renderer()
            .expect("unable to setup renderer"),
    ));
    state.set_base(base);
    state.create_spawners();
}

#[no_mangle]
pub extern "C" fn update(state: &mut RenderState, dt: &Duration) {
    // Call render, buffers are updated etc
    state.updates += 1;
    if state.updates % 600 == 0 {
        println!("updates: {} dt: {:?}...", state.updates, dt);
    }
    state.present();
}

#[no_mangle]
pub extern "C" fn unload(state: &mut RenderState) {
    state.cleanup_base_and_presenter();
    state.cleanup_spawners();
    println!("unloaded ash_renderer_plugin");
}
