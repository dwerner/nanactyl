#![no_std]

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec4};

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
pub struct UniformBuffer {
    pub proj: Mat4,
    pub light: Light,
}

impl UniformBuffer {
    pub fn new() -> Self {
        Self::with_proj(Mat4::IDENTITY)
    }
    pub fn with_proj(proj: Mat4) -> Self {
        Self {
            proj,
            light: Light {
                color: Vec4::new(1.0, 1.0, 1.0, 1.0),
                pos: Vec4::new(0.0, 10.0, 0.0, 1.0),
            },
        }
    }
}

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
pub struct Light {
    pub color: Vec4,
    pub pos: Vec4,
}

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
pub struct ShaderConstants {
    pub model_mat: Mat4,
}
