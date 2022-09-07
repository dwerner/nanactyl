use ash::vk;
use models::Vertex;

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
}

pub struct UploadedModel {
    pub vertices: BufferWithData<Vertex>,
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
