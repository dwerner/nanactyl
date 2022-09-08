use std::{
    ffi::CString,
    io::{self, Cursor},
    mem::align_of,
    time::Duration,
};

use ash::{util::Align, vk};
use models::{Image, Vertex};
use render::{
    types::{
        Attachments, AttachmentsModifier, BufferAndMemory, BufferWithData, Texture,
        UploadedModelRef, VertexInputAssembly, VulkanError,
    },
    Presenter, RenderState, VulkanBase,
};

pub struct ShaderStages {
    modules: Vec<vk::ShaderModule>,
    pub shader_stage_defs: Vec<ShaderStage>,
}

impl ShaderStages {
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
            shader_stage_defs: Vec::new(),
        }
    }

    pub fn add_shader<R>(
        &mut self,
        v: &mut VulkanBaseWrapper,
        reader: &mut R,
        entry_point_name: &'static str,
        stage: vk::ShaderStageFlags,
    ) -> Result<(), VulkanError>
    where
        R: io::Read + io::Seek,
    {
        let module = v.read_shader_module(reader)?;
        let idx = self.modules.len();
        self.modules.push(module);
        let shader_stage = ShaderStage::new(self.modules[idx], entry_point_name, stage)?;
        self.shader_stage_defs.push(shader_stage);
        Ok(())
    }
}

pub struct ShaderStage {
    module: vk::ShaderModule,
    entry_point_name: CString,
    stage: vk::ShaderStageFlags,
}

impl ShaderStage {
    pub fn new(
        module: vk::ShaderModule,
        entry_point_name: &'static str,
        stage: vk::ShaderStageFlags,
    ) -> Result<Self, VulkanError> {
        Ok(Self {
            module,
            entry_point_name: CString::new(entry_point_name)
                .map_err(VulkanError::InvalidCString)?,
            stage,
        })
    }

    pub fn create_info(&self) -> vk::PipelineShaderStageCreateInfo {
        vk::PipelineShaderStageCreateInfo::builder()
            .module(self.module)
            .name(self.entry_point_name.as_c_str())
            .stage(self.stage)
            .build()
    }
}

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
            .render_pass(self.render_pass)
            .framebuffer(self.framebuffers[present_index as usize])
            .render_area(base.surface_resolution.into())
            .clear_values(&clear_values);

        VulkanBase::record_and_submit_commandbuffer(
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
                        self.pipeline_layout,
                        0,
                        &self.descriptor_sets[..],
                        &[],
                    );
                    device.cmd_bind_pipeline(
                        draw_command_buffer,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.graphics_pipelines[0],
                    );
                    device.cmd_set_viewport(draw_command_buffer, 0, &self.viewports);
                    device.cmd_set_scissor(draw_command_buffer, 0, &self.scissors);
                    device.cmd_bind_vertex_buffers(
                        draw_command_buffer,
                        0,
                        &[self.vertex_input.buffer],
                        &[0],
                    );
                    device.cmd_bind_index_buffer(
                        draw_command_buffer,
                        self.index_with_data.buffer.buffer,
                        0,
                        vk::IndexType::UINT32,
                    );
                    device.cmd_draw_indexed(
                        draw_command_buffer,
                        self.index_with_data.data.len() as u32,
                        1,
                        0,
                        0,
                        1,
                    );
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

    fn drop_resources_with_base(&mut self, base: &mut VulkanBase) -> Result<(), VulkanError> {
        unsafe {
            base.device
                .device_wait_idle()
                .map_err(VulkanError::VkResult)?;

            for pipeline in self.graphics_pipelines.iter() {
                base.device.destroy_pipeline(*pipeline, None);
            }
            base.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            for shader_module in self.shader_stages.modules.drain(..) {
                base.device.destroy_shader_module(shader_module, None);
            }
            self.texture.deallocate(base);
            base.device.destroy_image_view(self.tex_image_view, None);
            self.index_with_data.buffer.deallocate(base);
            self.vertex_input.deallocate(base);
            for &descriptor_set_layout in self.desc_set_layouts.iter() {
                base.device
                    .destroy_descriptor_set_layout(descriptor_set_layout, None);
            }
            base.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            base.device.destroy_sampler(self.sampler, None);
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

// #[derive(Clone, Debug, Copy)]
// pub struct Vector3 {
//     pub x: f32,
//     pub y: f32,
//     pub z: f32,
//     pub _pad: f32,
// }

pub struct VulkanBaseWrapper<'a>(&'a mut VulkanBase);

impl<'a> VulkanBaseWrapper<'a> {
    pub fn new(base: &'a mut VulkanBase) -> Self {
        Self(base)
    }

    pub fn create_renderer(&mut self) -> Result<Renderer, VulkanError> {
        let device = self.0.device.clone();
        let w = DeviceWrap(&device);
        let index_with_data = {
            let index_buffer_data = vec![0u32, 1, 2, 2, 3, 0];
            let index = w.allocate_and_init_buffer(
                vk::BufferUsageFlags::INDEX_BUFFER,
                self.0.device_memory_properties,
                &index_buffer_data,
            )?;
            BufferWithData::new(index, index_buffer_data)
        };

        let vertex_input = {
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
            w.allocate_and_init_buffer(
                vk::BufferUsageFlags::VERTEX_BUFFER,
                self.0.device_memory_properties,
                &vertex_data,
            )?
        };

        // let uniform_color_with_data = {
        //     let uniform_color_buffer_data = vec![Vector3 {
        //         x: 1.0,
        //         y: 1.0,
        //         z: 1.0,
        //         _pad: 0.0,
        //     }];
        //     let uniform_color = self.init_buffer(
        //         vk::BufferUsageFlags::UNIFORM_BUFFER,
        //         &uniform_color_buffer_data,
        //     )?;
        //     BufferWithData::new(uniform_color, uniform_color_buffer_data)
        // };

        let (attachments, color, depth) = self.attachments();
        let render_pass = self.render_pass(attachments.all(), &color, &depth);
        let framebuffers: Vec<vk::Framebuffer> = self.framebuffers(render_pass)?;

        let texture = {
            // This is very inefficient - we should be lining up all texture uploads and recording those command buffers.
            // Then join threads/jobs and submit the buffers, but wait on device idle for non-compliant vulkan implementations
            // missing vkWaitSemaphore ~ device.wait_semaphore().
            let image = image::load_from_memory(include_bytes!("../../../../assets/ping.png"))
                .map_err(VulkanError::Image)?
                .to_rgba8();
            let (width, height) = image.dimensions();
            let image_extent = vk::Extent2D { width, height };
            let image_data = image.into_raw();
            let src_image = w.allocate_and_init_buffer(
                vk::BufferUsageFlags::TRANSFER_SRC,
                self.0.device_memory_properties,
                &image_data,
            )?;
            let texture =
                w.allocate_texture_dest_buffer(self.0.device_memory_properties, image_extent)?;
            self.submit_upload_texture(image_extent, &src_image, &texture);
            src_image.deallocate(self.0);
            texture
        };

        let sampler = self.sampler()?;
        let tex_image_view = self.image_view(&texture)?;
        let descriptor_pool = self.descriptor_pool()?;
        let desc_set_layouts = self.descriptor_set_layouts()?;
        let descriptor_sets = self.descriptor_sets(descriptor_pool, &desc_set_layouts)?;
        self.update_descriptor_set(
            descriptor_sets[0],
            //&uniform_color_with_data,
            tex_image_view,
            sampler,
        );
        let pipeline_layout = self.pipeline_layout(&desc_set_layouts)?;

        let viewports = self.viewports();
        let scissors = self.scissors();

        let mut shader_stages = ShaderStages::new();

        //? shader compiler could live as a Plugin
        let mut vertex_spv_file =
            Cursor::new(&include_bytes!("../../../../assets/shaders/vertex_rustgpu.spv")[..]);
        let mut frag_spv_file =
            Cursor::new(&include_bytes!("../../../../assets/shaders/fragment_rustgpu.spv")[..]);

        shader_stages.add_shader(
            self,
            &mut vertex_spv_file,
            "shader_main_long_name",
            vk::ShaderStageFlags::VERTEX,
        )?;
        shader_stages.add_shader(
            self,
            &mut frag_spv_file,
            "shader_main_long_name",
            vk::ShaderStageFlags::FRAGMENT,
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
            vk::Format::R32G32_SFLOAT,
            offset_of!(Vertex, uv) as u32,
        );

        let graphics_pipelines = self.graphics_pipelines(
            &viewports,
            &scissors,
            pipeline_layout,
            render_pass,
            &shader_stages.shader_stage_defs,
            &vertex_input_assembly,
        )?;

        Ok(Renderer {
            desc_set_layouts,
            descriptor_pool,
            descriptor_sets,
            framebuffers,
            graphics_pipelines,
            index_with_data,
            pipeline_layout,
            render_pass,
            texture,
            tex_image_view,
            sampler,
            scissors,
            shader_stages,
            vertex_input,
            viewports,
            // uniform_color_with_data,
        })
    }

    pub fn read_shader_module<R>(&self, reader: &mut R) -> Result<vk::ShaderModule, VulkanError>
    where
        R: io::Read + io::Seek,
    {
        // TODO: convert to VulkanError
        let shader_code = ash::util::read_spv(reader).expect("Failed to read shader spv data");

        let shader_create_info = vk::ShaderModuleCreateInfo::builder().code(&shader_code);
        unsafe {
            self.0
                .device
                .create_shader_module(&shader_create_info, None)
        }
        .map_err(VulkanError::VkResult)
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
        viewports: &[vk::Viewport],
        scissors: &[vk::Rect2D],
        pipeline_layout: vk::PipelineLayout,
        render_pass: vk::RenderPass,
        shader_stage_defs: &[ShaderStage],
        vertex_input_assembly: &VertexInputAssembly,
    ) -> Result<Vec<vk::Pipeline>, VulkanError> {
        let shader_stage_create_infos: Vec<vk::PipelineShaderStageCreateInfo> = shader_stage_defs
            .iter()
            .map(ShaderStage::create_info)
            .collect();

        let viewport_state_info = vk::PipelineViewportStateCreateInfo::builder()
            .scissors(scissors)
            .viewports(viewports);

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

        let vertex_input_state_info = vertex_input_assembly.input_state_info();
        let vertex_input_assembly_state_info = vertex_input_assembly.assembly_state_info();

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
            .render_pass(render_pass);

        unsafe {
            self.0.device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                &[graphic_pipeline_infos.build()],
                None,
            )
        }
        .map_err(|(pipeline, result)| VulkanError::FailedToCreatePipeline(pipeline, result))
    }

    pub fn pipeline_layout(
        &mut self,
        desc_set_layouts: &[vk::DescriptorSetLayout],
    ) -> Result<vk::PipelineLayout, VulkanError> {
        let layout_create_info =
            vk::PipelineLayoutCreateInfo::builder().set_layouts(desc_set_layouts);
        unsafe {
            self.0
                .device
                .create_pipeline_layout(&layout_create_info, None)
        }
        .map_err(VulkanError::VkResult)
    }

    // update descriptor sets with uniform buffer and tex_image_view
    pub fn update_descriptor_set(
        &mut self,
        descriptor_set: vk::DescriptorSet,
        // uniform_color: &BufferWithData<Vector3>,
        tex_image_view: vk::ImageView,
        sampler: vk::Sampler,
    ) {
        // let uniform_color_buffer_descriptor = vk::DescriptorBufferInfo {
        //     buffer: uniform_color.buffer.buffer,
        //     offset: 0,
        //     range: (uniform_color.data.len() * std::mem::size_of::<Vector3>()) as u64,
        // };

        let tex_descriptor = vk::DescriptorImageInfo {
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            image_view: tex_image_view,
            sampler,
        };

        let write_desc_sets = [
            // vk::WriteDescriptorSet::builder()
            //     .dst_set(descriptor_set)
            //     .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            //     .buffer_info(&[uniform_color_buffer_descriptor])
            //     .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(descriptor_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&[tex_descriptor])
                .build(),
        ];
        unsafe { self.0.device.update_descriptor_sets(&write_desc_sets, &[]) };
    }

    pub fn descriptor_sets(
        &mut self,
        pool: vk::DescriptorPool,
        layouts: &[vk::DescriptorSetLayout],
    ) -> Result<Vec<vk::DescriptorSet>, VulkanError> {
        let desc_alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(layouts);
        unsafe { self.0.device.allocate_descriptor_sets(&desc_alloc_info) }
            .map_err(VulkanError::VkResult)
    }

    pub fn descriptor_set_layouts(&mut self) -> Result<Vec<vk::DescriptorSetLayout>, VulkanError> {
        let desc_layout_bindings = [
            // vk::DescriptorSetLayoutBinding {
            //     binding: 0,
            //     descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
            //     descriptor_count: 1,
            //     stage_flags: vk::ShaderStageFlags::FRAGMENT,
            //     ..Default::default()
            // },
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
        let layout = unsafe {
            self.0
                .device
                .create_descriptor_set_layout(&descriptor_info, None)
        }
        .map_err(VulkanError::VkResult)?;
        Ok(vec![layout])
    }

    pub fn descriptor_pool(&mut self) -> Result<vk::DescriptorPool, VulkanError> {
        let descriptor_sizes = [
            // vk::DescriptorPoolSize {
            //     ty: vk::DescriptorType::UNIFORM_BUFFER,
            //     descriptor_count: 1,
            // },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: 1,
            },
        ];
        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&descriptor_sizes)
            .max_sets(1);
        unsafe {
            self.0
                .device
                .create_descriptor_pool(&descriptor_pool_info, None)
        }
        .map_err(VulkanError::VkResult)
    }

    pub fn submit_upload_texture(
        &mut self,
        image_extent: vk::Extent2D,
        src_image: &BufferAndMemory,
        dest_texture: &Texture,
    ) {
        VulkanBase::record_and_submit_commandbuffer(
            &self.0.device,
            self.0.setup_command_buffer,
            self.0.setup_commands_reuse_fence,
            self.0.present_queue,
            &[],
            &[],
            &[],
            |device, texture_command_buffer| {
                let texture_barrier = vk::ImageMemoryBarrier {
                    dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                    new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    image: dest_texture.image,
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
                        src_image.buffer,
                        dest_texture.image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &[buffer_copy_regions.build()],
                    )
                };
                let texture_barrier_end = vk::ImageMemoryBarrier {
                    src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                    dst_access_mask: vk::AccessFlags::SHADER_READ,
                    old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    image: dest_texture.image,
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
        unsafe {
            // Gotta be a better way for this.
            self.0.device.device_wait_idle().unwrap();
        }
    }

    pub fn sampler(&mut self) -> Result<vk::Sampler, VulkanError> {
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
        unsafe { self.0.device.create_sampler(&sampler_info, None) }.map_err(VulkanError::VkResult)
    }

    pub fn image_view(&mut self, texture: &Texture) -> Result<vk::ImageView, VulkanError> {
        let base = &mut *self.0;
        let tex_image_view_info = vk::ImageViewCreateInfo {
            view_type: vk::ImageViewType::TYPE_2D,
            format: texture.format,
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
            image: texture.image,
            ..Default::default()
        };
        unsafe { base.device.create_image_view(&tex_image_view_info, None) }
            .map_err(VulkanError::VkResult)
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
            .map_err(VulkanError::VkResult)?;
            framebuffers.push(framebuffer);
        }
        Ok(framebuffers)
    }
}
pub struct Renderer {
    descriptor_pool: vk::DescriptorPool,
    descriptor_sets: Vec<vk::DescriptorSet>,
    desc_set_layouts: Vec<vk::DescriptorSetLayout>,
    framebuffers: Vec<vk::Framebuffer>,
    index_with_data: BufferWithData<u32>,
    graphics_pipelines: Vec<vk::Pipeline>,
    pipeline_layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
    sampler: vk::Sampler,
    scissors: Vec<vk::Rect2D>,
    texture: Texture,
    tex_image_view: vk::ImageView,
    // uniform_color_with_data: BufferWithData<Vector3>,
    vertex_input: BufferAndMemory,
    viewports: Vec<vk::Viewport>,
    shader_stages: ShaderStages,
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
                .map_err(VulkanError::ResetFence)?;
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
            .map_err(VulkanError::VkResult)?;
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
            .map_err(VulkanError::VkResult)?;

        unsafe { self.0.bind_image_memory(texture_image, texture_memory, 0) }
            .map_err(VulkanError::VkResult)?;

        Ok(Texture::new(
            texture_create_info.format,
            texture_image,
            texture_memory,
        ))
    }
    /// Allocate a buffer with usage flags, initialize with data.
    /// TODO: internalize
    fn allocate_and_init_buffer<T>(
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
        let buffer =
            unsafe { self.0.create_buffer(&buffer_info, None) }.map_err(VulkanError::VkResult)?;
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
            .map_err(VulkanError::VkResult)?;
        let buffer = BufferAndMemory::new(buffer, buffer_memory);
        let ptr = unsafe {
            self.0.map_memory(
                buffer.memory,
                0,
                allocation_size,
                vk::MemoryMapFlags::empty(),
            )
        }
        .map_err(VulkanError::VkResult)?;
        let mut slice = unsafe { Align::new(ptr, align_of::<T>() as u64, allocation_size) };
        slice.copy_from_slice(data);
        unsafe { self.0.unmap_memory(buffer.memory) };
        unsafe { self.0.bind_buffer_memory(buffer.buffer, buffer.memory, 0) }
            .map_err(VulkanError::VkResult)?;

        Ok(buffer)
    }

    fn memorytype_index_and_size_for_buffer(
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
        unsafe { self.0.create_fence(&fence_create_info, None) }.map_err(VulkanError::VkResult)
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

    pub fn submit_command_buffers(
        &self,
        fence: vk::Fence,
        queue: vk::Queue,
        command_buffers: Vec<vk::CommandBuffer>,
    ) -> Result<(), VulkanError> {
        let submit_info = vk::SubmitInfo::builder()
            .command_buffers(&command_buffers)
            .build();

        unsafe { self.0.queue_submit(queue, &[submit_info], fence) }
            .map_err(VulkanError::SubmitCommandBuffers)
    }

    pub fn cmd_buffer_end(&self, command_buffer: vk::CommandBuffer) -> Result<(), VulkanError> {
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
        .map_err(VulkanError::VkResult)
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
            .map_err(VulkanError::VkResult)
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
    for (index, model) in state.models.iter() {
        println!("loading model at {:?}...", index);
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
        w.cmd_buffer_end(command_buffer).unwrap();
        w.submit_command_buffers(fence, queue, command_buffers)
            .unwrap();

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

        let uploaded_model =
            UploadedModelRef::new(dest_texture, vertex_buffer, index_buffer, src_image);
        base.track_uploaded_model(*index, uploaded_model);
    }
    unsafe {
        device.device_wait_idle().unwrap();
        device.destroy_fence(fence, None);
        device.destroy_command_pool(pool, None);
    }
    state.set_presenter(Box::new(
        VulkanBaseWrapper::new(&mut base)
            .create_renderer()
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
        println!("updates: {} dt: {:?}", state.updates, dt);
    }
    state.present();
}

#[no_mangle]
pub extern "C" fn unload(state: &mut RenderState) {
    state.cleanup_base_and_presenter();
    state.cleanup_spawners();
    println!("unloaded ash_renderer_plugin");
}
