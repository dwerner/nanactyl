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

use spirv_std::glam::{Vec2, Vec3, Vec4};

pub struct UBO {
    pub color: Vec3,
}

#[spirv(fragment)]
pub fn shader_main_long_name(
    #[spirv(uniform_constant, descriptor_set = 0, binding = 1)] sampled_image: &hide::Img2d,
    o_uv: Vec2,
    frag_color: &mut Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] ubo: &mut UBO,
) {
    let _color = ubo.color;
    let color = unsafe { sampled_image.sample(o_uv) };
    *frag_color = color;
}
