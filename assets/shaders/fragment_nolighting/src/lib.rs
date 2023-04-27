#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

mod sampler;

use spirv_std::glam::{Vec2, Vec4};
use spirv_std::spirv;

#[spirv(fragment)]
pub fn fragment_main(
    #[spirv(frag_coord)] _in_frag_coord: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] _ubo: &UniformBuffer,
    #[spirv(descriptor_set = 0, binding = 2)] _sampler: &sampler::Sampler2d,
    normal: Vec4,
    uv: Vec2,
    frag_color: &mut Vec4,
) {
    let texture: Vec4 = unsafe { sampler.sample(uv) };
    *frag_color = texture;
}
