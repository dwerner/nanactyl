#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

mod hide;

use shader_objects::UniformBuffer;
use spirv_std::glam::{Vec2, Vec4};
use spirv_std::spirv;

#[spirv(fragment)]
pub fn fragment_main(
    #[spirv(frag_coord)] in_frag_coord: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 1)] _ubo: &UniformBuffer,
    #[spirv(descriptor_set = 0, binding = 2)] sampler: &hide::Sampler2d,
    normal: Vec4,
    uv: Vec2,
    frag_color: &mut Vec4,
) {
    //let light = &ubo.light;
    let light_direction = (Vec4::ZERO - in_frag_coord).normalize();
    let normal = normal.normalize();

    let diffuse_intensity = light_direction.dot(normal).max(0.0);
    let diffuse_color = diffuse_intensity * Vec4::ONE; // light.color;

    let texture: Vec4 = unsafe { sampler.sample(uv) };
    *frag_color = texture * diffuse_color;
}
