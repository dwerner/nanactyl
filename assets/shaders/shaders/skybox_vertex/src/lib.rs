#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

use shader_objects::{PushConstants, UniformBuffer};
use spirv_std::glam::{Vec2, Vec4};
use spirv_std::spirv;

#[spirv(vertex)]
pub fn vertex_main(
    #[spirv(uniform, descriptor_set = 0, binding = 0)] ubo: &UniformBuffer,
    #[spirv(push_constant)] push_constants: &PushConstants,
    pos: Vec4,
    uv: Vec2,
    _normal: Vec4,
    o_uv: &mut Vec2,
    #[spirv(position)] o_pos: &mut Vec4,
) {
    let mat = push_constants.model_mat;
    *o_uv = uv;
    // skybox shouldn't move with the model matrix
    *o_pos = ubo.proj * mat * Vec4::new(pos.x, pos.y, pos.z, pos.w);
}
