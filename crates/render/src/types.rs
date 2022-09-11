use std::{
    collections::HashMap,
    ffi::{CString, NulError},
    io,
    path::PathBuf,
};

use ash::vk;

use crate::VulkanBase;

#[derive(thiserror::Error, Debug)]
pub enum VulkanError {
    #[error("error reading shader ({0:?})")]
    ShaderRead(io::Error),

    #[error("Unable to find suitable memorytype for the buffer")]
    UnableToFindMemoryTypeForBuffer,

    #[error("vk result ({0:?}) todo: assign a real error variant")]
    VkResultToDo(vk::Result),

    #[error("invalid CString from &'static str")]
    InvalidCString(NulError),

    #[error("failed to create pipeline {1:?}")]
    FailedToCreatePipeline(Vec<vk::Pipeline>, vk::Result),

    #[error("image error {0:?}")]
    Image(image::ImageError),

    #[error("swapchain acquire next image error {0:?}")]
    SwapchainAquireNextImage(vk::Result),

    #[error("no scene to present")]
    NoSceneToPresent,

    // Fences
    #[error("fence error {0:?}")]
    Fence(vk::Result),

    #[error("reset fence error {0:?}")]
    FenceReset(vk::Result),

    // Command buffers
    #[error("begin command buffer error {0:?}")]
    BeginCommandBuffer(vk::Result),

    #[error("end command buffer error {0:?}")]
    EndCommandBuffer(vk::Result),

    #[error("submit command buffer error {0:?}")]
    SubmitCommandBuffers(vk::Result),
}

/// A handle to a Vulkan GPU buffer and it's backing memory.
pub struct BufferAndMemory {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,

    /// len of the original slice copied from.
    pub len: usize,
}

impl BufferAndMemory {
    pub fn new(buffer: vk::Buffer, memory: vk::DeviceMemory, len: usize) -> Self {
        Self {
            buffer,
            memory,
            len,
        }
    }

    pub fn deallocate(&self, device: &ash::Device) {
        unsafe {
            device.free_memory(self.memory, None);
            device.destroy_buffer(self.buffer, None);
        }
    }
}

/// Describes a shader binding.
#[derive(Clone)]
pub struct ShaderBindingDesc {
    pub binding: u32,
    pub descriptor_type: vk::DescriptorType,
    pub descriptor_count: u32,
    pub stage_flags: vk::ShaderStageFlags,
}
impl ShaderBindingDesc {
    pub fn into_layout_binding(self) -> vk::DescriptorSetLayoutBinding {
        let Self {
            binding,
            descriptor_type,
            descriptor_count,
            stage_flags,
        } = self;
        vk::DescriptorSetLayoutBinding {
            binding,
            descriptor_type,
            descriptor_count,
            stage_flags,
            ..Default::default()
        }
    }
}

#[derive(Clone)]
pub struct ShaderDesc {
    pub desc_set_layout_bindings: Vec<ShaderBindingDesc>,
    pub vertex_shader: PathBuf,
    pub fragment_shader: PathBuf,
}

/// Handle to resources on the GPU comprising a texture.
pub struct GpuModelRef {
    pub vertex_buffer: BufferAndMemory,
    pub index_buffer: BufferAndMemory,
    pub texture: Texture,
    pub shaders: ShaderDesc,
}

impl GpuModelRef {
    pub fn new(
        texture: Texture,
        vertex_buffer: BufferAndMemory,
        index_buffer: BufferAndMemory,
        shaders: ShaderDesc,
    ) -> Self {
        Self {
            texture,
            vertex_buffer,
            index_buffer,
            shaders,
        }
    }
    pub(crate) fn deallocate(&self, base: &mut VulkanBase) {
        self.index_buffer.deallocate(&base.device);
        self.vertex_buffer.deallocate(&base.device);
        self.texture.deallocate(&base.device);
    }
}

/// Holds references to GPU resources for a texture.
pub struct Texture {
    pub format: vk::Format,
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub image_view: vk::ImageView,
}

impl Texture {
    pub fn new(
        format: vk::Format,
        image: vk::Image,
        memory: vk::DeviceMemory,
        device: &ash::Device,
    ) -> Result<Self, VulkanError> {
        let img_view_info = vk::ImageViewCreateInfo {
            view_type: vk::ImageViewType::TYPE_2D,
            format,
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
            image,
            ..Default::default()
        };
        let image_view = unsafe { device.create_image_view(&img_view_info, None) }
            .map_err(VulkanError::VkResultToDo)?;

        Ok(Self {
            image,
            format,
            memory,
            image_view,
        })
    }

    pub fn deallocate(&self, device: &ash::Device) {
        unsafe {
            device.destroy_image_view(self.image_view, None);
            device.free_memory(self.memory, None);
            device.destroy_image(self.image, None);
        }
    }
}

pub struct VertexInputAssembly {
    pub topology: vk::PrimitiveTopology,
    pub binding_descriptions: Vec<vk::VertexInputBindingDescription>,
    pub attribute_descriptions: Vec<vk::VertexInputAttributeDescription>,
}

impl VertexInputAssembly {
    pub fn new(topology: vk::PrimitiveTopology) -> Self {
        Self {
            topology,
            binding_descriptions: Vec::new(),
            attribute_descriptions: Vec::new(),
        }
    }
    pub fn assembly_state_info(&self) -> vk::PipelineInputAssemblyStateCreateInfo {
        vk::PipelineInputAssemblyStateCreateInfo {
            topology: self.topology,
            ..Default::default()
        }
    }

    pub fn add_binding_description<T>(&mut self, binding: u32, input_rate: vk::VertexInputRate)
    where
        T: Copy,
    {
        self.binding_descriptions
            .push(vk::VertexInputBindingDescription {
                binding,
                stride: std::mem::size_of::<T>() as u32,
                input_rate,
            });
    }
    pub fn add_attribute_description(
        &mut self,
        location: u32,
        binding: u32,
        format: vk::Format,
        offset: u32,
    ) {
        self.attribute_descriptions
            .push(vk::VertexInputAttributeDescription {
                location,
                binding,
                format,
                offset,
            });
    }

    pub fn input_state_info(&self) -> vk::PipelineVertexInputStateCreateInfo {
        vk::PipelineVertexInputStateCreateInfo::builder()
            .vertex_attribute_descriptions(&self.attribute_descriptions)
            .vertex_binding_descriptions(&self.binding_descriptions)
            .build()
    }
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

pub struct AttachmentsModifier<'a> {
    pub attachments: &'a mut Attachments,
    pub attachment_refs: Vec<vk::AttachmentReference>,
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

/// Describes a pipeline.
pub struct PipelineDesc {
    pub uniform_buffer: BufferAndMemory,
    pub descriptor_set: vk::DescriptorSet,
    pub sampler: vk::Sampler,
    pub layout: vk::PipelineLayout,
    pub viewports: Vec<vk::Viewport>,
    pub scissors: Vec<vk::Rect2D>,
    pub shader_stages: ShaderStages,
    pub vertex_input_assembly: VertexInputAssembly,
}

impl PipelineDesc {
    pub fn new(
        uniform_buffer: BufferAndMemory,
        descriptor_set: vk::DescriptorSet,
        sampler: vk::Sampler,
        layout: vk::PipelineLayout,
        viewports: Vec<vk::Viewport>,
        scissors: Vec<vk::Rect2D>,
        shader_stages: ShaderStages,
        vertex_input_assembly: VertexInputAssembly,
    ) -> Self {
        Self {
            uniform_buffer,
            descriptor_set,
            sampler,
            layout,
            viewports,
            scissors,
            shader_stages,
            vertex_input_assembly,
        }
    }
    pub fn deallocate(&self, device: &ash::Device) {
        unsafe {
            self.uniform_buffer.deallocate(device);
            device.destroy_pipeline_layout(self.layout, None);
            for shader_module in self.shader_stages.modules.iter() {
                device.destroy_shader_module(*shader_module, None);
            }
            device.destroy_sampler(self.sampler, None);
        }
    }
}

/// TODO: Move to DeviceWrap
pub fn read_shader_module<R>(
    device: &ash::Device,
    reader: &mut R,
) -> Result<vk::ShaderModule, VulkanError>
where
    R: io::Read + io::Seek,
{
    let shader_code = ash::util::read_spv(reader).map_err(VulkanError::ShaderRead)?;
    let shader_create_info = vk::ShaderModuleCreateInfo::builder().code(&shader_code);
    unsafe { device.create_shader_module(&shader_create_info, None) }
        .map_err(VulkanError::VkResultToDo)
}

/// TODO: further implementaion of shader caching.
pub struct ShaderModuleCache {
    _shaders: HashMap<String, ShaderStages>,
}

/// Tracks modules and definitions used to initialize shaders.
pub struct ShaderStages {
    pub modules: Vec<vk::ShaderModule>,
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
        device: &ash::Device,
        reader: &mut R,
        entry_point_name: &'static str,
        stage: vk::ShaderStageFlags,
        flags: vk::PipelineShaderStageCreateFlags,
    ) -> Result<(), VulkanError>
    where
        R: io::Read + io::Seek,
    {
        let module = read_shader_module(device, reader)?;
        let idx = self.modules.len();
        self.modules.push(module);
        let shader_stage = ShaderStage::new(self.modules[idx], entry_point_name, stage, flags)?;
        self.shader_stage_defs.push(shader_stage);
        Ok(())
    }
}

/// TODO: Maybe chain together instead of leaving disparate.

pub struct ShaderStage {
    module: vk::ShaderModule,
    entry_point_name: CString,
    stage: vk::ShaderStageFlags,
    flags: vk::PipelineShaderStageCreateFlags,
}

impl ShaderStage {
    pub fn new(
        module: vk::ShaderModule,
        entry_point_name: &'static str,
        stage: vk::ShaderStageFlags,
        flags: vk::PipelineShaderStageCreateFlags,
    ) -> Result<Self, VulkanError> {
        Ok(Self {
            module,
            entry_point_name: CString::new(entry_point_name)
                .map_err(VulkanError::InvalidCString)?,
            stage,
            flags,
        })
    }

    pub fn create_info(&self) -> vk::PipelineShaderStageCreateInfo {
        vk::PipelineShaderStageCreateInfo::builder()
            .module(self.module)
            .name(self.entry_point_name.as_c_str())
            .stage(self.stage)
            .flags(self.flags)
            .build()
    }
}
