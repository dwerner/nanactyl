use spirv_std::image::SampledImage;

pub type Sampler2d = SampledImage<spirv_std::Image!(2D, type=f32, sampled, depth=false)>;

//use spirv_std::image::ImageFormat::
//pub type Img2d = spirv_std::Image!(2D, type=f32, sampled);
