#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

use spirv_std::glam::{Vec2, Vec4};
use spirv_std::image::SampledImage;
use spirv_std::{spirv, Image};

#[spirv(fragment)]
pub fn fragment_main(
    #[spirv(descriptor_set = 0, binding = 1)] diffuse_sampler: &SampledImage<
        Image!(2D, type=f32, sampled, depth=false),
    >,
    uv: Vec2,
    out_frag_color: &mut Vec4,
) {
    let texture: Vec4 = diffuse_sampler.sample(uv);
    *out_frag_color = texture;
}
