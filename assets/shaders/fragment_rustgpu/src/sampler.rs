use spirv_std::image::SampledImage;

// TODO: move this to a separate shader crate
pub type Sampler2d = SampledImage<spirv_std::Image!(2D, type=f32, sampled, depth=false)>;
