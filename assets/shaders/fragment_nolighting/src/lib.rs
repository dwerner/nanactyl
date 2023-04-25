#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

mod sampler;

use spirv_std::glam::{Vec2, Vec4};
use spirv_std::spirv;

#[spirv(fragment)]
pub fn fragment_main(
    #[spirv(descriptor_set = 0, binding = 2)] sampler: &sampler::Sampler2d,
    _normal: Vec4,
    uv: Vec2,
    frag_color: &mut Vec4,
) {
    let texture: Vec4 = unsafe { sampler.sample(uv) };
    *frag_color = texture;
}
