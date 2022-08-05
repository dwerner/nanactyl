use std::{
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use const_gen::{const_declaration, CompileConst};

fn main() {
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");

    // Use the OUT_DIR environment variable to get an
    // appropriate path.
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("const_gen.rs");

    // let target_dir = format!("{}", find_target_dir().as_path().display());
    // let _target_dir = &target_dir;

    let relative_target_dir = format!("../../target/{}", env::var("PROFILE").unwrap());

    fs::write(
        &dest_path,
        vec![
            // const_declaration!(TARGET_DIR = target_dir),
            const_declaration!(RELATIVE_TARGET_DIR = relative_target_dir),
        ]
        .join("\n"),
    )
    .unwrap();
}

fn find_target_dir() -> PathBuf {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let mut buf = PathBuf::from_str(&out_dir).unwrap();
    while buf.iter().last().unwrap() != std::env::var("PROFILE").unwrap().as_str() {
        buf.pop();
    }
    buf
}
