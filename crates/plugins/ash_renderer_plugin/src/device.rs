use std::mem::align_of;

use ash::util::Align;
use ash::vk;
use gfx::{Image, Primitive};
use logger::Logger;

use crate::types::{BufferAndMemory, ShaderDesc, Texture, VulkanError};
use crate::VulkanBase;

/// Newtype over `ash::Device` allowing our own methods to be implemented.
/// TODO: decide on what parts of this API should be implemented in the plugin
/// vs in the rendering module
pub struct DeviceWrapper<'a> {
    device: &'a ash::Device,
    _logger: Logger,
}

impl<'a> DeviceWrapper<'a> {
    pub fn wrap(device: &'a ash::Device, logger: &Logger) -> Self {
        Self {
            device,
            _logger: logger.sub("device-wrapper"),
        }
    }
    /// Create a pipeline layout. Note `push_constants_len` must be len in bytes
    /// and a multiple of 4
    pub fn pipeline_layout(
        &self,
        push_constants_len: u32,
        desc_set_layouts: &[vk::DescriptorSetLayout],
    ) -> Result<vk::PipelineLayout, VulkanError> {
        let push_constant_ranges = [*vk::PushConstantRange::builder()
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .offset(0)
            .size(push_constants_len)];

        let layout_create_info = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(desc_set_layouts)
            .push_constant_ranges(&push_constant_ranges);

        unsafe {
            self.device
                .create_pipeline_layout(&layout_create_info, None)
        }
        .map_err(VulkanError::VkResultToDo)
    }

    pub(crate) fn wait_for_fence(&self, fence: vk::Fence) -> Result<(), VulkanError> {
        unsafe {
            self.device
                .wait_for_fences(&[fence], true, u64::MAX)
                .map_err(VulkanError::Fence)?;
            self.device
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
        let texture_image = unsafe { self.device.create_image(&texture_create_info, None) }
            .map_err(VulkanError::VkResultToDo)?;
        let texture_memory_req =
            unsafe { self.device.get_image_memory_requirements(texture_image) };
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

        let texture_memory = unsafe { self.device.allocate_memory(&texture_allocate_info, None) }
            .map_err(VulkanError::VkResultToDo)?;

        unsafe {
            self.device
                .bind_image_memory(texture_image, texture_memory, 0)
        }
        .map_err(VulkanError::VkResultToDo)?;

        Texture::create(
            texture_create_info.format,
            texture_image,
            texture_memory,
            self.device,
        )
    }

    /// Updates a buffer binding on the GPU with the given data.
    pub fn update_buffer<T>(
        &self,
        buffer: &mut BufferAndMemory,
        data: &[T],
    ) -> Result<(), VulkanError>
    where
        T: Copy,
    {
        let ptr = unsafe {
            self.device.map_memory(
                buffer.memory,
                0,
                buffer.allocation_size,
                vk::MemoryMapFlags::empty(),
            )
        }
        .map_err(VulkanError::VkResultToDo)?;
        let mut slice = unsafe { Align::new(ptr, align_of::<T>() as u64, buffer.allocation_size) };
        slice.copy_from_slice(data);
        unsafe { self.device.unmap_memory(buffer.memory) };
        Ok(())
    }

    /// Allocate a buffer with usage flags, initialize with data.
    /// TODO:
    ///     - HOST_COHERENT + HOST_VISIBLE
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
        let buffer = unsafe { self.device.create_buffer(&buffer_info, None) }
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
        let buffer_memory = unsafe { self.device.allocate_memory(&allocate_info, None) }
            .map_err(VulkanError::VkResultToDo)?;
        let mut buffer = BufferAndMemory::new(buffer, buffer_memory, data.len(), allocation_size);
        self.update_buffer(&mut buffer, data)?;
        unsafe {
            self.device
                .bind_buffer_memory(buffer.buffer, buffer.memory, 0)
        }
        .map_err(VulkanError::VkResultToDo)?;
        Ok(buffer)
    }

    /// Find the memory type index and get the size for the given buffer.
    pub fn memorytype_index_and_size_for_buffer(
        &self,
        buffer: vk::Buffer,
        memory_properties: vk::PhysicalDeviceMemoryProperties,
        flags: vk::MemoryPropertyFlags,
    ) -> Result<(u64, u32), VulkanError> {
        let buffer_memory_req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        Ok((
            buffer_memory_req.size,
            VulkanBase::find_memorytype_index(&buffer_memory_req, &memory_properties, flags)
                .ok_or(VulkanError::UnableToFindMemoryTypeForBuffer)?,
        ))
    }

    /// Create a fence on the GPU.
    pub fn create_fence(&self) -> Result<vk::Fence, VulkanError> {
        let fence_create_info =
            vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
        unsafe { self.device.create_fence(&fence_create_info, None) }
            .map_err(VulkanError::VkResultToDo)
    }

    /// Copy buffer to an image.
    pub fn cmd_copy_buffer_to_image(
        &self,
        src_image: &BufferAndMemory,
        image_extent: vk::Extent2D,
        dest_texture: &Texture,
        command_buffer: vk::CommandBuffer,
    ) {
        let buffer_copy_regions = [*vk::BufferImageCopy::builder()
            .image_subresource(
                *vk::ImageSubresourceLayers::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(image_extent.into())];

        unsafe {
            self.device.cmd_copy_buffer_to_image(
                command_buffer,
                src_image.buffer,
                dest_texture.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &buffer_copy_regions,
            )
        }
    }

    /// Submit queue.
    pub fn queue_submit(
        &self,
        fence: vk::Fence,
        queue: vk::Queue,
        submits: &[vk::SubmitInfo],
    ) -> Result<(), VulkanError> {
        unsafe { self.device.queue_submit(queue, submits, fence) }
            .map_err(VulkanError::SubmitCommandBuffers)
    }

    /// End command buffer.
    pub fn end_command_buffer(&self, command_buffer: vk::CommandBuffer) -> Result<(), VulkanError> {
        unsafe { self.device.end_command_buffer(command_buffer) }
            .map_err(VulkanError::EndCommandBuffer)
    }

    /// Insert a barrier end.
    // just a few flags are different between * and *_end versions, but need to
    // better understand the ... <half-written note>
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
            self.device.cmd_pipeline_barrier(
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

    /// Insert a barrier start.
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
            self.device.cmd_pipeline_barrier(
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

    /// Bind image data to a `BufferAndMemory`.
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

    /// Begin a command buffer.
    pub fn begin_command_buffer(
        &self,
        command_buffer: vk::CommandBuffer,
    ) -> Result<(), VulkanError> {
        let command_buffer_begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            self.device
                .begin_command_buffer(command_buffer, &command_buffer_begin_info)
        }
        .map_err(VulkanError::BeginCommandBuffer)
    }

    /// Allocate a command buffer. TODO: could be more than a single buffer.
    pub fn allocate_command_buffers(
        &self,
        pool: vk::CommandPool,
    ) -> Result<Vec<vk::CommandBuffer>, VulkanError> {
        let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(1)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY);
        unsafe {
            self.device
                .allocate_command_buffers(&command_buffer_allocate_info)
        }
        .map_err(VulkanError::VkResultToDo)
    }

    /// Creates a command pool.
    pub fn create_command_pool(
        &self,
        queue_family_index: u32,
    ) -> Result<vk::CommandPool, VulkanError> {
        let pool_create_info = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family_index);
        unsafe { self.device.create_command_pool(&pool_create_info, None) }
            .map_err(VulkanError::VkResultToDo)
    }

    /// Resets a fence.
    pub fn reset_fence(&self, fence: vk::Fence) -> Result<(), VulkanError> {
        unsafe { self.device.reset_fences(&[fence]) }.map_err(VulkanError::FenceReset)
    }

    /// Record beginning of render pass.
    pub fn cmd_begin_render_pass(
        &self,
        draw_command_buffer: vk::CommandBuffer,
        render_pass_begin_info: &vk::RenderPassBeginInfoBuilder,
        inline: vk::SubpassContents,
    ) {
        unsafe {
            self.device
                .cmd_begin_render_pass(draw_command_buffer, render_pass_begin_info, inline);
        }
    }

    /// Records a binding to descriptor sets.
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
            self.device.cmd_bind_descriptor_sets(
                draw_cmd_buf,
                graphics,
                pipeline_layout,
                first_set,
                descriptor_sets,
                dynamic_offsets,
            );
        }
    }

    /// Records a binding to a pipeline.
    pub fn cmd_bind_pipeline(
        &self,
        cmd: vk::CommandBuffer,
        graphics: vk::PipelineBindPoint,
        graphics_pipelines: vk::Pipeline,
    ) {
        unsafe {
            self.device
                .cmd_bind_pipeline(cmd, graphics, graphics_pipelines);
        }
    }

    /// Records setting a viewport.
    pub fn cmd_set_viewport(&self, cmd: vk::CommandBuffer, first: u32, viewports: &[vk::Viewport]) {
        unsafe {
            self.device.cmd_set_viewport(cmd, first, viewports);
        }
    }

    /// Records setting scissor.
    pub fn cmd_set_scissor(&self, cmd: vk::CommandBuffer, first: u32, scissors: &[vk::Rect2D]) {
        unsafe { self.device.cmd_set_scissor(cmd, first, scissors) }
    }

    /// Records binding a vertex buffer.
    pub fn cmd_bind_vertex_buffers(
        &self,
        command_buffer: vk::CommandBuffer,
        first_binding: u32,
        buffers: &[vk::Buffer],
        offsets: &[vk::DeviceSize],
    ) {
        unsafe {
            self.device
                .cmd_bind_vertex_buffers(command_buffer, first_binding, buffers, offsets);
        }
    }

    /// Records binding an image buffer.
    pub fn cmd_bind_index_buffer(
        &self,
        command_buffer: vk::CommandBuffer,
        buffer: vk::Buffer,
        offset: vk::DeviceSize,
        index_type: vk::IndexType,
    ) {
        unsafe {
            self.device
                .cmd_bind_index_buffer(command_buffer, buffer, offset, index_type);
        }
    }

    /// Records push contants.
    pub fn cmd_push_constants(
        &self,
        command_buffer: vk::CommandBuffer,
        layout: vk::PipelineLayout,
        stage_flags: vk::ShaderStageFlags,
        offset: u32,
        constants: &[u8],
    ) {
        unsafe {
            self.device
                .cmd_push_constants(command_buffer, layout, stage_flags, offset, constants)
        }
    }

    /// Records an indexed draw call.
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
            self.device.cmd_draw_indexed(
                command_buffer,
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            );
        }
    }

    /// Records the end of a render pass.
    pub(crate) fn cmd_end_render_pass(&self, command_buffer: vk::CommandBuffer) {
        unsafe {
            self.device.cmd_end_render_pass(command_buffer);
        }
    }

    pub(crate) fn cmd_upload_image(
        &self,
        image: &Image,
        device_memory_properties: vk::PhysicalDeviceMemoryProperties,
        command_buffer: vk::CommandBuffer,
        src_images: &mut Vec<BufferAndMemory>,
    ) -> Texture {
        let (diffuse_map_buffer, dest_texture) =
            self.record_upload_image(image, device_memory_properties, command_buffer);
        src_images.push(diffuse_map_buffer);
        dest_texture
    }

    fn record_upload_image(
        &self,
        image: &Image,
        device_memory_properties: vk::PhysicalDeviceMemoryProperties,
        command_buffer: vk::CommandBuffer,
    ) -> (BufferAndMemory, Texture) {
        let (image_extent, src_image) = self
            .copy_image_to_transfer_src_buffer(image, device_memory_properties)
            .unwrap();
        let dest_texture = self
            .allocate_texture_dest_buffer(device_memory_properties, image_extent)
            .unwrap();
        self.cmd_pipeline_barrier_start(dest_texture.image, command_buffer);
        self.cmd_copy_buffer_to_image(&src_image, image_extent, &dest_texture, command_buffer);
        self.cmd_pipeline_barrier_end(dest_texture.image, command_buffer);
        (src_image, dest_texture)
    }
}

/// Handle to resources on the GPU comprising a mesh, texture and shader.
pub struct GraphicsHandle {
    pub vertex_buffer: BufferAndMemory,
    pub index_buffer: BufferAndMemory,
    pub diffuse_map: Option<Texture>,
    // pub specular_map: Option<Texture>,
    // pub bump_map: Option<Texture>,
    pub shaders: ShaderDesc,
    pub primitive: Primitive,
}

impl GraphicsHandle {
    pub(crate) fn new(
        diffuse_map: Option<Texture>,
        // specular_map: Option<Texture>,
        // bump_map: Option<Texture>,
        vertex_buffer: BufferAndMemory,
        index_buffer: BufferAndMemory,
        shaders: ShaderDesc,
        primitive: Primitive,
    ) -> Self {
        Self {
            diffuse_map,
            // specular_map,
            // bump_map,
            vertex_buffer,
            index_buffer,
            shaders,
            primitive,
        }
    }
    pub(crate) fn deallocate(&self, base: &mut VulkanBase) {
        self.index_buffer.deallocate(&base.device);
        self.vertex_buffer.deallocate(&base.device);
        self.diffuse_map
            .as_ref()
            .map(|map| map.deallocate(&base.device));
        // self.specular_map
        //     .as_ref()
        //     .map(|map| map.deallocate(&base.device));
        // self.bump_map
        //     .as_ref()
        //     .map(|map| map.deallocate(&base.device));
    }
}
// TODO cleanup pass
