use std::ffi::NulError;

use ash::vk;

use crate::VulkanBase;

#[derive(thiserror::Error, Debug)]
pub enum VulkanError {
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

    pub fn deallocate(&self, base: &mut VulkanBase) {
        unsafe {
            base.device.free_memory(self.memory, None);
            base.device.destroy_buffer(self.buffer, None);
        }
    }
}

// TODO rename
pub struct UploadedModelRef {
    pub src_image: BufferAndMemory,
    pub vertex_buffer: BufferAndMemory,
    pub index_buffer: BufferAndMemory,
    pub texture: Texture,
}

impl UploadedModelRef {
    pub fn new(
        texture: Texture,
        vertex_buffer: BufferAndMemory,
        index_buffer: BufferAndMemory,
        src_image: BufferAndMemory,
    ) -> Self {
        Self {
            texture,
            vertex_buffer,
            index_buffer,
            src_image,
        }
    }
    pub(crate) fn deallocate(&self, base: &mut VulkanBase) {
        self.src_image.deallocate(base);
        self.index_buffer.deallocate(base);
        self.vertex_buffer.deallocate(base);
        self.texture.deallocate(base);
    }
}

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

    pub fn deallocate(&self, base: &mut VulkanBase) {
        unsafe {
            base.device.destroy_image_view(self.image_view, None);
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
