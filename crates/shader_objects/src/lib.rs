#![no_std]

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
pub struct UniformBuffer {
    pub proj: Mat4,
}

#[derive(Copy, Clone, Pod, Zeroable)]
#[repr(C)]
pub struct ShaderConstants {
    pub model_mat: Mat4,
}
