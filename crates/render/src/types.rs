use std::ffi::NulError;

use ash::vk;
use models::Vertex;

use crate::VulkanBase;

#[derive(thiserror::Error, Debug)]
pub enum VulkanError {
    #[error("Unable to find suitable memorytype for the buffer")]
    UnableToFindMemoryTypeForBuffer,

    #[error("vk result")]
    VkResult(vk::Result),

    #[error("failed to create pipeline")]
    FailedToCreatePipeline(Vec<vk::Pipeline>, vk::Result),

    #[error("invalid CString from &'static str")]
    InvalidCString(NulError),

    #[error("image error {0:?}")]
    Image(image::ImageError),

    #[error("fence error {0:?}")]
    Fence(vk::Result),

    #[error("reset fence error {0:?}")]
    ResetFence(vk::Result),

    #[error("begin command buffer error {0:?}")]
    BeginCommandBuffer(vk::Result),

    #[error("end command buffer error {0:?}")]
    EndCommandBuffer(vk::Result),

    #[error("submit command buffer error {0:?}")]
    SubmitCommandBuffers(vk::Result),
}

// Ultimately, do we want this to exist at all? Why keep the source data around at all?
// If we were to try to use the original data against the new, we'd want a way to diff
// the two and arrive at a set of operations to write, in order to update the buffer.
// Is that worth the cost of mirroring GPU local data in general RAM?
pub struct BufferWithData<T> {
    pub data: Vec<T>,
    pub buffer: BufferAndMemory,
}

impl<T> BufferWithData<T> {
    pub fn new(buffer: BufferAndMemory, data: Vec<T>) -> Self {
        Self { buffer, data }
    }
}

pub struct BufferAndMemory {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
}

impl BufferAndMemory {
    pub fn new(buffer: vk::Buffer, memory: vk::DeviceMemory) -> Self {
        Self { buffer, memory }
    }

    pub fn deallocate(&self, base: &mut VulkanBase) {
        unsafe {
            base.device.free_memory(self.memory, None);
            base.device.destroy_buffer(self.buffer, None);
        }
    }
}

// TODO rename
pub struct UploadedModelRef {
    pub vertex_buffer: BufferAndMemory,
    pub index_buffer: BufferAndMemory,
    pub texture: Texture,
}

impl UploadedModelRef {
    pub(crate) fn deallocate(&self, base: &mut VulkanBase) {
        self.index_buffer.deallocate(base);
        self.vertex_buffer.deallocate(base);
        self.texture.deallocate(base);
    }
}

pub struct Texture {
    pub format: vk::Format,
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
}

impl Texture {
    pub fn new(format: vk::Format, image: vk::Image, memory: vk::DeviceMemory) -> Self {
        Self {
            image,
            format,
            memory,
        }
    }

    pub fn deallocate(&self, base: &mut VulkanBase) {
        unsafe {
            base.device.free_memory(self.memory, None);
            base.device.destroy_image(self.image, None);
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
