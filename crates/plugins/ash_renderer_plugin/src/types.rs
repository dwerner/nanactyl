use std::ffi::{CString, NulError};
use std::fs::File;
use std::io::{self, BufReader, Cursor, Read};
use std::path::PathBuf;
use std::sync::Arc;

use ash::vk;
use rspirv_reflect::{DescriptorType, EntryPoint, Reflection};

/// Collection of specific error types that Vulkan can raise, in rust form.
#[derive(thiserror::Error, Debug)]
pub enum VulkanError {
    #[error("error reading shader ({0:?})")]
    ShaderRead(io::Error),

    #[error("error reflecting over shader ({0:?})")]
    ShaderReflect(rspirv_reflect::ReflectError),

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
pub struct DescriptorSetLayoutBinding {
    pub set: u32,
    pub binding: u32,
    pub descriptor_type: vk::DescriptorType,
    pub descriptor_count: u32,
}

impl DescriptorSetLayoutBinding {
    pub fn as_layout_binding(
        &self,
        stage_flags: vk::ShaderStageFlags,
    ) -> vk::DescriptorSetLayoutBinding {
        let Self {
            binding,
            descriptor_type,
            descriptor_count,
            ..
        } = self;
        vk::DescriptorSetLayoutBinding {
            binding: *binding,
            descriptor_type: *descriptor_type,
            descriptor_count: *descriptor_count,
            stage_flags,
            ..Default::default()
        }
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
pub struct Pipeline {
    pub desc_set_layout: vk::DescriptorSetLayout,
    pub uniform_buffer: BufferAndMemory,
    pub descriptor_set: vk::DescriptorSet,
    pub maybe_diffuse_sampler: Option<vk::Sampler>,
    // pub specular_sampler: vk::Sampler,
    // pub bump_sampler: vk::Sampler,
    pub layout: vk::PipelineLayout,
    pub viewports: Vec<vk::Viewport>,
    pub scissors: Vec<vk::Rect2D>,
    pub shader_stages: ShaderStages,
    pub vertex_input_assembly: VertexInputAssembly,
    pub polygon_mode: vk::PolygonMode,
    pub vk: Option<vk::Pipeline>,
}

impl Pipeline {
    /// Create a new PipelineDesc.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        desc_set_layout: vk::DescriptorSetLayout,
        uniform_buffer: BufferAndMemory,
        descriptor_set: vk::DescriptorSet,
        maybe_diffuse_sampler: Option<vk::Sampler>,
        // specular_sampler: vk::Sampler,
        // bump_sampler: vk::Sampler,
        layout: vk::PipelineLayout,
        viewports: Vec<vk::Viewport>,
        scissors: Vec<vk::Rect2D>,
        shader_stages: ShaderStages,
        vertex_input_assembly: VertexInputAssembly,
        polygon_mode: vk::PolygonMode,
    ) -> Self {
        Self {
            desc_set_layout,
            uniform_buffer,
            descriptor_set,
            maybe_diffuse_sampler,
            // specular_sampler,
            // bump_sampler,
            layout,
            viewports,
            scissors,
            shader_stages,
            vertex_input_assembly,
            polygon_mode,
            vk: None,
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
            if let Some(sampler) = self.maybe_diffuse_sampler {
                device.destroy_sampler(sampler, None);
            }
            // device.destroy_sampler(self.specular_sampler, None);
            // device.destroy_sampler(self.bump_sampler, None);
            device.destroy_descriptor_set_layout(self.desc_set_layout, None);
        }
    }

    pub(crate) fn set_vk(&mut self, vk: vk::Pipeline) {
        self.vk = Some(vk)
    }
}

#[derive(Clone)]
pub struct Shader {
    data: Vec<u8>,
    path: PathBuf,
    entry_points: Vec<EntryPoint>,
    descriptor_set_bindings: Vec<DescriptorSetLayoutBinding>,
    push_constant_ranges: Vec<vk::PushConstantRange>,
}

impl Shader {
    /// Read and reflect over a SPIR-V shader .spv file, storing data and
    /// metadata for later use. Can be used to create an
    /// ash::vk::ShaderModule with `as_shader_module`.
    ///
    /// A shader doesn't know what stages are available, so you must specify
    /// when creating a pipeline.
    pub fn read_spv(path: PathBuf) -> Result<Self, VulkanError> {
        let mut reader = BufReader::new(File::open(&path).map_err(VulkanError::ShaderRead)?);
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .map_err(VulkanError::ShaderRead)?;

        let reflection = Reflection::new_from_spirv(&data).unwrap();

        let mut descriptor_set_bindings = Vec::new();
        for (set, binding_map) in reflection
            .get_descriptor_sets()
            .map_err(VulkanError::ShaderReflect)?
        {
            for (binding, desc) in binding_map {
                let descriptor_type = match desc.ty {
                    DescriptorType::SAMPLER => vk::DescriptorType::SAMPLER,
                    DescriptorType::COMBINED_IMAGE_SAMPLER => {
                        vk::DescriptorType::COMBINED_IMAGE_SAMPLER
                    }
                    DescriptorType::SAMPLED_IMAGE => vk::DescriptorType::SAMPLED_IMAGE,
                    DescriptorType::STORAGE_IMAGE => vk::DescriptorType::STORAGE_IMAGE,
                    DescriptorType::UNIFORM_TEXEL_BUFFER => {
                        vk::DescriptorType::UNIFORM_TEXEL_BUFFER
                    }
                    DescriptorType::STORAGE_TEXEL_BUFFER => {
                        vk::DescriptorType::STORAGE_TEXEL_BUFFER
                    }
                    DescriptorType::UNIFORM_BUFFER => vk::DescriptorType::UNIFORM_BUFFER,
                    DescriptorType::STORAGE_BUFFER => vk::DescriptorType::STORAGE_BUFFER,
                    DescriptorType::UNIFORM_BUFFER_DYNAMIC => {
                        vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC
                    }
                    DescriptorType::STORAGE_BUFFER_DYNAMIC => {
                        vk::DescriptorType::STORAGE_BUFFER_DYNAMIC
                    }
                    DescriptorType::INPUT_ATTACHMENT => vk::DescriptorType::INPUT_ATTACHMENT,
                    DescriptorType::INLINE_UNIFORM_BLOCK_EXT => {
                        vk::DescriptorType::INLINE_UNIFORM_BLOCK_EXT
                    }
                    DescriptorType::ACCELERATION_STRUCTURE_KHR => {
                        vk::DescriptorType::ACCELERATION_STRUCTURE_KHR
                    }
                    DescriptorType::ACCELERATION_STRUCTURE_NV => {
                        vk::DescriptorType::ACCELERATION_STRUCTURE_NV
                    }
                    // todo: err
                    _ => unreachable!("Unsupported descriptor type"),
                };
                let descriptor_count = match desc.binding_count {
                    rspirv_reflect::BindingCount::One => 1,
                    rspirv_reflect::BindingCount::StaticSized(s) => s as u32,
                    // bindless
                    rspirv_reflect::BindingCount::Unbounded => todo!(),
                };
                descriptor_set_bindings.push(DescriptorSetLayoutBinding {
                    set,
                    binding,
                    descriptor_type,
                    descriptor_count,
                });
            }
        }

        let entry_points = reflection
            .get_entry_points()
            .map_err(VulkanError::ShaderReflect)?;

        // TODO: complete this
        let mut push_constant_ranges = Vec::new();

        Ok(Self {
            data,
            path,
            descriptor_set_bindings,
            entry_points,
            push_constant_ranges,
        })
    }

    /// Create an ash::vk::ShaderModule from the SPIR-V shader data.
    pub fn as_shader_module(&self, device: &ash::Device) -> Result<vk::ShaderModule, VulkanError> {
        let mut cursor = Cursor::new(&self.data);
        let code = ash::util::read_spv(&mut cursor).map_err(VulkanError::ShaderRead)?;
        let create_info = vk::ShaderModuleCreateInfo::builder().code(&code);
        unsafe { device.create_shader_module(&create_info, None) }
            .map_err(VulkanError::VkResultToDo)
    }

    pub fn desc_set_layout_bindings(&self) -> &[DescriptorSetLayoutBinding] {
        &self.descriptor_set_bindings
    }
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

    pub fn add_shader(
        &mut self,
        device: &ash::Device,
        shader: Arc<Shader>,
        stage_flags: vk::ShaderStageFlags,
    ) -> Result<(), VulkanError> {
        let pipeline_flags = vk::PipelineShaderStageCreateFlags::empty();

        // TODO do more thank just grab the first entry point
        let entry_point_name = shader.entry_points[0].name.clone();
        let module = shader.as_shader_module(device)?;

        let idx = self.modules.len();
        self.modules.push(module);

        let shader_stage = ShaderStage::new(
            self.modules[idx],
            entry_point_name,
            stage_flags,
            pipeline_flags,
        )?;
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
        entry_point_name: String,
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
