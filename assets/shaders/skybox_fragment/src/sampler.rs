use spirv_std::image::SampledImage;

pub type Sampler2d = SampledImage<spirv_std::Image!(2D, type=f32, sampled, depth=false)>;
