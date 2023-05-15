#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

use spirv_std::glam::{Vec2, Vec4};
use spirv_std::spirv;

#[spirv(fragment)]
pub fn fragment_main(
    #[spirv(frag_coord)] _in_frag_coord: Vec4,
    _normal: Vec4,
    _uv: Vec2,
    out_frag_color: &mut Vec4,
) {
    *out_frag_color = Vec4::new(1.0, 1.0, 1.0, 1.0);
}
