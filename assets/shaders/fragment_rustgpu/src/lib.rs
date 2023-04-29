#![cfg_attr(target_arch = "spirv", no_std)]
// HACK(eddyb) can't easily see warnings otherwise from `spirv-builder` builds.
#![deny(warnings)]

pub mod sampler;

use shader_objects::{UniformBuffer, MAX_LIGHTS};
use spirv_std::glam::{
    Vec2,
    Vec4,
    //Vec4Swizzles
};
// use spirv_std::num_traits::Pow;
use spirv_std::spirv;

#[spirv(fragment)]
pub fn fragment_main(
    #[spirv(frag_coord)] in_frag_coord: Vec4,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] ubo: &UniformBuffer,
    #[spirv(descriptor_set = 0, binding = 1)] diffuse_sampler: &sampler::Sampler2d,
    // #[spirv(descriptor_set = 0, binding = 3)] _specular_sampler: &sampler::Sampler2d,
    // #[spirv(descriptor_set = 0, binding = 4)] _bump_sampler: &sampler::Sampler2d,
    normal: Vec4,
    uv: Vec2,
    out_frag_color: &mut Vec4,
) {
    let mut fog_factor = 0.0;
    let mut diffuse_color = Vec4::ZERO;

    let texture: Vec4 = unsafe { diffuse_sampler.sample(uv) };
    // let bump_map: Vec4 = unsafe { bump_sampler.sample(uv) };
    // let specular_map: Vec4 = unsafe { specular_sampler.sample(uv) };

    for i in 0..MAX_LIGHTS {
        let light = ubo.lights[i];
        let light_direction = (light.pos - in_frag_coord).normalize();

        //     // Transform the normal from the normal map to the [-1, 1] range and normalize it
        //     let bumped_normal = (bump_map * 2.0 - 1.0).normalize();

        // Calculate the view direction
        // let view_direction = (-in_frag_coord.xyz()).normalize();

        //     // Use bumped_normal instead of normal
        //     let diffuse_intensity = light_direction.dot(bumped_normal).max(0.0);
        let diffuse_intensity = light_direction.dot(normal).max(0.0);
        //     let reflection_direction = -light_direction - 2.0 * bumped_normal.dot(-light_direction) * bumped_normal;
        //     let shininess = 1.0;
        //     let specular_intensity = view_direction
        //         .dot(reflection_direction.xyz())
        //         .max(0.0)
        //         .pow(shininess);

        //     let specular_color = light.color * specular_map.x * specular_intensity;
        diffuse_color += diffuse_intensity * light.color; //  + specular_color;

        // Fog calculations
        let fog_distance = in_frag_coord.w;
        fog_factor +=
            ((ubo.fog_end - fog_distance) / (ubo.fog_end - ubo.fog_start)).clamp(0.0, 1.0);
    }

    // Apply fog, combine texture, diffuse, and specular colors
    *out_frag_color = ubo.fog_color.lerp(texture * diffuse_color, fog_factor);
}
