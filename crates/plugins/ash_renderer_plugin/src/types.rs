use core::fmt;
use std::ffi::{CString, NulError};
use std::fmt::Formatter;
use std::io;
use std::path::PathBuf;

use ash::vk;

/// Collection of specific error types that Vulkan can raise, in rust form.
#[derive(thiserror::Error, Debug)]
pub enum VulkanError {
    #[error("error reading shader ({0:?})")]
    ShaderRead(io::Error),

    #[error("Unable to find suitable memorytype for the buffer")]
    UnableToFindMemoryTypeForBuffer,

    // TODO: find call sites and generate new error variants for this
    #[error("vk result ({0:?}) todo: assign a real error variant")]
    VkResultToDo(vk::Result),

    #[error("vk result ({0:?}) during present")]
    Present(vk::Result),

    #[error("invalid CString from &'static str")]
    InvalidCString(NulError),

    #[error("failed to create pipeline {1:?}")]
    FailedToCreatePipeline(Vec<vk::Pipeline>, vk::Result),

    #[error("swapchain acquire next image error {0:?}")]
    SwapchainAcquireNextImage(vk::Result),

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

    #[error("error enumerating physical devices {0:?})")]
    EnumeratePhysicalDevices(vk::Result),
}

/// A handle to a Vulkan GPU buffer and it's backing memory.
/// TODO: typed buffer and memory
pub struct BufferAndMemory {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,

    /// len of the original slice copied from.
    pub original_len: usize,

    /// len in bytes of the original allocation.
    pub allocation_size: u64,
}

/// Combination of vulkan's buffer and memory types, encapsulating the two for
/// RAII purposes.
impl BufferAndMemory {
    pub fn new(
        buffer: vk::Buffer,
        memory: vk::DeviceMemory,
        original_len: usize,
        allocation_size: u64,
    ) -> Self {
        Self {
            buffer,
            memory,
            original_len,
            allocation_size,
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

impl fmt::Debug for ShaderBindingDesc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let vertex = self.stage_flags.contains(vk::ShaderStageFlags::VERTEX);
        let fragment = self.stage_flags.contains(vk::ShaderStageFlags::FRAGMENT);
        // todo: more shader stages

        let mut flags = Vec::new();
        if vertex {
            flags.push("VERTEX");
        }
        if fragment {
            flags.push("FRAGMENT");
        }
        f.debug_struct("ShaderBindingDesc")
            .field("binding", &self.binding)
            .field("descriptor_type", &self.descriptor_type)
            .field("descriptor_count", &self.descriptor_count)
            .field("stage_flags", &[flags.join(",")])
            .finish()
    }
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
    pub vertex_shader_path: PathBuf,
    pub fragment_shader_path: PathBuf,
}

/// Holds references to GPU resources for a texture.
pub struct Texture {
    pub format: vk::Format,
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub image_view: vk::ImageView,
}

impl Texture {
    pub fn create(
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
        binding: u32,
        location: u32,
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
        *vk::PipelineVertexInputStateCreateInfo::builder()
            .vertex_attribute_descriptions(&self.attribute_descriptions)
            .vertex_binding_descriptions(&self.binding_descriptions)
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
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub uniform_buffer: BufferAndMemory,
    pub descriptor_set: vk::DescriptorSet,
    pub diffuse_sampler: vk::Sampler,
    // pub specular_sampler: vk::Sampler,
    // pub bump_sampler: vk::Sampler,
    pub layout: vk::PipelineLayout,
    pub viewports: Vec<vk::Viewport>,
    pub scissors: Vec<vk::Rect2D>,
    pub shader_stages: ShaderStages,
    pub vertex_input_assembly: VertexInputAssembly,
}

impl PipelineDesc {
    /// Create a new PipelineDesc.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        desc_set_layout: vk::DescriptorSetLayout,
        uniform_buffer: BufferAndMemory,
        descriptor_set: vk::DescriptorSet,
        diffuse_sampler: vk::Sampler,
        // specular_sampler: vk::Sampler,
        // bump_sampler: vk::Sampler,
        layout: vk::PipelineLayout,
        viewports: Vec<vk::Viewport>,
        scissors: Vec<vk::Rect2D>,
        shader_stages: ShaderStages,
        vertex_input_assembly: VertexInputAssembly,
    ) -> Self {
        Self {
            desc_set_layout,
            uniform_buffer,
            descriptor_set,
            diffuse_sampler,
            // specular_sampler,
            // bump_sampler,
            layout,
            viewports,
            scissors,
            shader_stages,
            vertex_input_assembly,
        }
    }
    /// Deallocate PipelineDesc's resources on the GPU.
    pub fn deallocate(&self, device: &ash::Device) {
        unsafe {
            self.uniform_buffer.deallocate(device);
            device.destroy_pipeline_layout(self.layout, None);
            for shader_module in self.shader_stages.modules.iter() {
                device.destroy_shader_module(*shader_module, None);
            }
            device.destroy_sampler(self.diffuse_sampler, None);
            // device.destroy_sampler(self.specular_sampler, None);
            // device.destroy_sampler(self.bump_sampler, None);
            device.destroy_descriptor_set_layout(self.desc_set_layout, None);
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

/// Tracks modules and definitions used to initialize shaders.
#[derive(Default)]
pub struct ShaderStages {
    pub modules: Vec<vk::ShaderModule>,
    pub shader_stage_defs: Vec<ShaderStage>,
}

impl ShaderStages {
    pub fn new() -> Self {
        Self::default()
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
        *vk::PipelineShaderStageCreateInfo::builder()
            .module(self.module)
            .name(self.entry_point_name.as_c_str())
            .stage(self.stage)
            .flags(self.flags)
    }
}