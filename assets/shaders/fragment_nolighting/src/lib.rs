#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

use shader_objects::UniformBuffer;
mod sampler;

use spirv_std::glam::{Vec2, Vec4};
use spirv_std::spirv;

#[spirv(fragment)]
pub fn fragment_main(
    #[spirv(frag_coord)] _in_frag_coord: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] _ubo: &UniformBuffer,
    #[spirv(descriptor_set = 0, binding = 2)] diffuse_sampler: &sampler::Sampler2d,
    #[spirv(descriptor_set = 0, binding = 3)] _specular_sampler: &sampler::Sampler2d,
    #[spirv(descriptor_set = 0, binding = 4)] _bump_sampler: &sampler::Sampler2d,
    uv: Vec2,
    _normal: Vec4,
    _pos: Vec4,
    out_frag_color: &mut Vec4,
) {
    let texture: Vec4 = unsafe { diffuse_sampler.sample(uv) };
    *out_frag_color = texture;
}
