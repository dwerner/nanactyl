#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

use shader_objects::{ShaderConstants, UniformBuffer};
use spirv_std::glam::{vec4, Mat3, Mat4, Vec2, Vec4};
use spirv_std::spirv;

#[spirv(vertex)]
pub fn vertex_main(
    #[spirv(uniform, descriptor_set = 0, binding = 1)] ubo: &UniformBuffer,
    #[spirv(push_constant)] push_constants: &ShaderConstants,
    pos: Vec4,
    uv: Vec2,
    normal: Vec4,
    o_normal: &mut Vec4,
    o_uv: &mut Vec2,
    #[spirv(position)] o_pos: &mut Vec4,
) {
    let mat = push_constants.model_mat;
    *o_normal = Mat4::from_mat3(Mat3::from_mat4(mat)).inverse().transpose() * normal;
    *o_pos = ubo.proj * mat * vec4(pos.x, pos.y, pos.z, 1.0);
    *o_uv = uv;
}
