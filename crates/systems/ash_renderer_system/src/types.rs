use std::ffi::{CString, NulError};
use std::fs::File;
use std::io::{self, BufReader, Cursor, Read};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ash::vk;
use stable_typeid::StableTypeId;
use world::Entity;

/// Collection of specific error types that Vulkan can raise, in rust form.
#[derive(thiserror::Error, Debug)]
pub enum RenderError {
    #[error("error reading shader ({0:?})")]
    ShaderRead(io::Error),

    #[error("error reflecting over shader {0}")]
    ShaderReflect(String),

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

    #[error("error no shader entry point found")]
    NoShaderEntryPoint,

    #[error("component missing from camera entity {0:?}, {1:?}, {2:?}")]
    ComponentMissingFromCameraEntity(Entity, &'static str, StableTypeId),
}

impl RenderError {
    pub fn shader_reflect(s: &str) -> Self {
        RenderError::ShaderReflect(s.to_string())
    }
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
#[derive(Debug, Clone)]
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
    ) -> Result<Self, RenderError> {
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
            .map_err(RenderError::VkResultToDo)?;

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
        self.uniform_buffer.deallocate(device);

        unsafe {
            device.destroy_pipeline_layout(self.layout, None);
        }

        self.shader_stages.deallocate(device);

        if let Some(sampler) = self.maybe_diffuse_sampler {
            unsafe {
                device.destroy_sampler(sampler, None);
            }
        }

        // device.destroy_sampler(self.specular_sampler, None);
        // device.destroy_sampler(self.bump_sampler, None);
        unsafe {
            device.destroy_descriptor_set_layout(self.desc_set_layout, None);
        }
    }

    pub(crate) fn set_vk(&mut self, vk: vk::Pipeline) {
        self.vk = Some(vk)
    }
}

/// Describes a shader entry point, enclosing the bindings.
#[derive(Debug)]
pub struct EntryPoint {
    name: String,
    stage_flags: vk::ShaderStageFlags,
    descriptor_set_layout_bindings: Vec<DescriptorSetLayoutBinding>,
    push_constant_ranges: Vec<vk::PushConstantRange>,
}

impl EntryPoint {
    /// Get the name of the entry point
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the stage flags of the entry point, as determined from the entry
    /// point's execution model
    pub fn stage_flags(&self) -> vk::ShaderStageFlags {
        self.stage_flags
    }

    /// Get the descriptor set layout bindings of the entry point
    pub fn desc_set_layout_bindings(&self) -> &[DescriptorSetLayoutBinding] {
        &self.descriptor_set_layout_bindings
    }

    /// Get the push constant ranges of the entry point
    pub fn push_constant_ranges(&self) -> &[vk::PushConstantRange] {
        &self.push_constant_ranges
    }
}

pub struct Shader {
    data: Vec<u8>,
    path: PathBuf,
    entry_points: Vec<EntryPoint>,
}

/// TODO: combine with Shader? we want to define a set of stages for a
/// given pipeline, which may even have compute or whatever stages.
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
    ) -> Result<Self, RenderError> {
        Ok(Self {
            module,
            entry_point_name: CString::new(entry_point_name)
                .map_err(RenderError::InvalidCString)?,
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

impl Shader {
    /// Create an ash::vk::ShaderModule from the SPIR-V shader data.
    pub fn as_shader_module(&self, device: &ash::Device) -> Result<vk::ShaderModule, RenderError> {
        let mut cursor = Cursor::new(&self.data);
        let code = ash::util::read_spv(&mut cursor).map_err(RenderError::ShaderRead)?;
        let create_info = vk::ShaderModuleCreateInfo::builder().code(&code);
        unsafe { device.create_shader_module(&create_info, None) }
            .map_err(RenderError::VkResultToDo)
    }

    pub fn entry_points(&self) -> &[EntryPoint] {
        &self.entry_points
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reload this shader from disk.
    pub fn reload(&mut self) -> Result<(), RenderError> {
        let new = Self::read_spv(self.path.clone())?;
        let _old = mem::replace(&mut *self, new);
        Ok(())
    }

    /// Read and reflect over a SPIR-V shader .spv file, storing data and
    /// metadata for later use. Returns a `Shader` which can be used to create
    /// an ash::vk::ShaderModule with `as_shader_module`.
    ///
    /// A shader doesn't know what stages are available, so you must specify
    /// when creating a pipeline.
    pub fn read_spv(path: PathBuf) -> Result<Self, RenderError> {
        let mut reader = BufReader::new(File::open(&path).map_err(RenderError::ShaderRead)?);
        let mut data = Vec::new();
        reader
            .read_to_end(&mut data)
            .map_err(RenderError::ShaderRead)?;

        let shader_module = spirv_reflect::ShaderModule::load_u8_data(&data)
            .map_err(RenderError::shader_reflect)?;

        let mut entry_points = Vec::new();
        for entry_point in shader_module
            .enumerate_entry_points()
            .map_err(RenderError::shader_reflect)?
        {
            let stage_flags = spirv_reflect_shader_stage_to_vk(entry_point.shader_stage);
            let mut descriptor_set_bindings = Vec::new();
            for descriptor_set in shader_module
                .enumerate_descriptor_sets(Some(&entry_point.name))
                .map_err(RenderError::shader_reflect)?
            {
                for binding in descriptor_set.bindings {
                    let set = binding.set;
                    let descriptor_type =
                        spirv_reflect_descriptor_type_to_vk(&binding.descriptor_type);
                    let descriptor_count = binding.count;
                    let binding = binding.binding;
                    descriptor_set_bindings.push(DescriptorSetLayoutBinding {
                        set,
                        binding,
                        descriptor_type,
                        descriptor_count,
                    });
                }
            }
            let mut push_constant_ranges = Vec::new();
            for push_constant in shader_module
                .enumerate_push_constant_blocks(Some(&entry_point.name))
                .map_err(RenderError::shader_reflect)?
            {
                let offset = push_constant.offset;
                let size = push_constant.size;
                push_constant_ranges.push(vk::PushConstantRange {
                    stage_flags,
                    offset,
                    size,
                });
            }
            entry_points.push(EntryPoint {
                name: entry_point.name.clone(),
                stage_flags,
                descriptor_set_layout_bindings: descriptor_set_bindings.clone(),
                push_constant_ranges: Vec::new(),
            })
        }

        Ok(Self {
            data,
            path,
            entry_points,
        })
    }
}

fn spirv_reflect_shader_stage_to_vk(
    stage: spirv_reflect::types::ReflectShaderStageFlags,
) -> vk::ShaderStageFlags {
    vk::ShaderStageFlags::from_raw(stage.bits())
}

fn spirv_reflect_descriptor_type_to_vk(
    desc: &spirv_reflect::types::ReflectDescriptorType,
) -> vk::DescriptorType {
    use spirv_reflect::types::ReflectDescriptorType::*;
    match desc {
        Undefined => unimplemented!("Undefined descriptor type"),
        Sampler => vk::DescriptorType::SAMPLER,
        CombinedImageSampler => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
        SampledImage => vk::DescriptorType::SAMPLED_IMAGE,
        StorageImage => vk::DescriptorType::STORAGE_IMAGE,
        UniformTexelBuffer => vk::DescriptorType::UNIFORM_TEXEL_BUFFER,
        StorageTexelBuffer => vk::DescriptorType::STORAGE_TEXEL_BUFFER,
        UniformBuffer => vk::DescriptorType::UNIFORM_BUFFER,
        StorageBuffer => vk::DescriptorType::STORAGE_BUFFER,
        UniformBufferDynamic => vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
        StorageBufferDynamic => vk::DescriptorType::STORAGE_BUFFER_DYNAMIC,
        InputAttachment => vk::DescriptorType::INPUT_ATTACHMENT,
        AccelerationStructureNV => vk::DescriptorType::ACCELERATION_STRUCTURE_NV,
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
    ) -> Result<(), RenderError> {
        let pipeline_flags = vk::PipelineShaderStageCreateFlags::empty();

        // TODO do more thank just grab the first entry point
        let entry_point_name = shader
            .entry_points
            .get(0)
            .ok_or(RenderError::NoShaderEntryPoint)?
            .name
            .clone();

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

    pub fn deallocate(&self, device: &ash::Device) {
        for shader_module in self.modules.iter() {
            unsafe { device.destroy_shader_module(*shader_module, None) };
        }
    }
}
