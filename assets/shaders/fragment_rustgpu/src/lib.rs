#![cfg_attr(
    target_arch = "spirv",
    no_std,
    feature(register_attr),
    register_attr(spirv)
)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

#[cfg(not(target_arch = "spirv"))]
use spirv_std::macros::spirv;

mod hide;

use spirv_std::glam::{Vec2, Vec4};

#[spirv(fragment)]
pub fn fragment_main(
    normal: Vec4,
    uv: Vec2,
    frag_color: &mut Vec4,
    #[spirv(descriptor_set = 0, binding = 2)] sampler: &hide::Sampler2d,
) {
    let color: Vec4 = unsafe { sampler.sample(uv) };
    // for now we combine the texture and normal colors, and that prevents a
    // validation error. Could do other things like shading.
    *frag_color = color * normal;
}
