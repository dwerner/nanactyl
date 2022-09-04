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

use spirv_std::glam::{Vec2, Vec4};

#[spirv(vertex)]
pub fn shader_main_long_name(
    pos: Vec4,
    uv: Vec2,
    o_uv: &mut Vec2,
    #[spirv(position)] o_pos: &mut Vec4,
) {
    *o_pos = pos;
    *o_uv = uv;
}
