use spirv_builder::{MetadataPrintout, SpirvBuilder};

fn main() {
    // SPIR-V Targets
    //     spirv-unknown-spv1.0
    //     spirv-unknown-spv1.1
    //     spirv-unknown-spv1.2
    //     spirv-unknown-spv1.3
    //     spirv-unknown-spv1.4
    //     spirv-unknown-spv1.5
    // Vulkan Targets
    //     spirv-unknown-vulkan1.0
    //     spirv-unknown-vulkan1.1
    //     spirv-unknown-vulkan1.1spv1.4
    //     spirv-unknown-vulkan1.2
    println!("cargo:rerun-if-changed=./rust_shader_builder");
    for shader in ["vertex_rustgpu", "fragment_rustgpu"].iter() {
        println!("cargo:rerun-if-changed=./{}", shader);
        let module_path = SpirvBuilder::new(
            format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), shader),
            "spirv-unknown-spv1.0",
        )
        .print_metadata(MetadataPrintout::Full)
        .build()
        .unwrap()
        .module
        .unwrap_single()
        .to_path_buf();

        println!("cargo-warning={:?}", module_path);
        std::fs::copy(
            &module_path,
            format!("{}/../{}.spv", env!("CARGO_MANIFEST_DIR"), shader),
        )
        .unwrap();
    }
}

// pub fn compile_shaders() -> Vec<SpvFile> {
//     SpirvBuilder::new(".", "spirv-unknown-vulkan1.1")
//         .print_metadata(MetadataPrintout::None)
//         .build()
//         .unwrap();
//         .module
//         .unwrap_single()
//         .to_path_buf();
// let sky_shader = SpvFile {
//     name: "sky_shader".to_string(),
//     data: read_spv(&mut File::open(sky_shader_path).unwrap()).unwrap(),
// };
// vec![sky_shader]
//}
