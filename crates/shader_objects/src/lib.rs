#![no_std]

#[cfg(feature = "std")]
use bytemuck::{Pod, Zeroable};
#[cfg(feature = "std")]
use glam::{Mat4, Vec4};
// spirv-std has made glam a mandatory re-export, so we build two feature sets of this crate to
// maintain compatibility with both spirv-std and std.
#[cfg(feature = "spirv-std")]
use spirv_std::glam::{Mat4, Vec4};

pub const MAX_LIGHTS: usize = 2;

#[cfg_attr(feature = "std", derive(Pod, Zeroable))]
#[derive(Copy, Clone)]
#[repr(C)]
pub struct Light {
    pub color: Vec4,
    pub pos: Vec4,
}

#[cfg_attr(feature = "std", derive(Pod, Zeroable))]
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PushConstants {
    pub model_transform: Mat4,
}

impl PushConstants {
    pub fn new(model_transform: Mat4) -> Self {
        Self { model_transform }
    }

    pub fn to_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

#[cfg_attr(feature = "std", derive(Pod, Zeroable))]
#[derive(Copy, Clone)]
#[repr(C)]
pub struct UniformBuffer {
    pub proj: Mat4,
    pub lights: [Light; MAX_LIGHTS],
    pub fog_color: Vec4,
    pub fog_start: f32,
    pub fog_end: f32,
    pub _pad1: f32,
    pub _pad2: f32,
}

impl UniformBuffer {
    pub fn new() -> Self {
        Self::with_proj(Mat4::IDENTITY)
    }
    pub fn with_proj(proj: Mat4) -> Self {
        Self {
            proj,
            lights: [
                Light {
                    color: Vec4::new(1.0, 1.0, 1.0, 1.0),
                    pos: Vec4::new(10.0, 10.0, 10.0, 1.0),
                },
                Light {
                    color: Vec4::new(1.0, 1.0, 1.0, 1.0),
                    pos: Vec4::new(-10.0, 10.0, -10.0, 1.0),
                },
            ],
            fog_color: Vec4::ONE,
            fog_start: 1.0,
            fog_end: 5.0,
            _pad1: 0.0,
            _pad2: 0.0,
        }
    }
}
