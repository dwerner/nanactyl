use std::path::PathBuf;

use duct::cmd;
use structopt::StructOpt;

// This must be kept in sync with rust-gpu's requirements, and see also the
// rust-toolchain files in the respective assets/shaders subdirs.
const RUST_GPU_TOOLCHAIN: &str = "nightly-2022-12-18";

#[derive(StructOpt, Debug)]
enum Command {
    FmtLint,
    BuildShaders,
}

#[derive(StructOpt)]
struct Opts {
    #[structopt(subcommand)]
    cmd: Option<Command>,
}

// Panic if we aren't in the project root.
fn enforce_root_dir_is_cwd() {
    let current_dir = std::env::current_dir().unwrap();
    let mut project_root = env!("CARGO_MANIFEST_DIR").parse::<PathBuf>().unwrap();
    project_root.pop();
    assert_eq!(
        current_dir, project_root,
        "xtask must be called from project root: {project_root:?}"
    );
}

fn main() {
    let opts = Opts::from_args();
    std::env::set_var("RUST_BACKTRACE", "full");
    set_required_rustflags(&["-Wunused-crate-dependencies"]);
    enforce_root_dir_is_cwd();
    dispatch(opts.cmd.unwrap_or_else(|| panic!("no command given")))
        .unwrap_or_else(|err| panic!("error with xtask, {err:?}"));
}

// Append passed flags onto rustflags for compilation.
fn set_required_rustflags(flags: &[&'static str]) {
    let existing_rustflags = std::env::var("RUSTFLAGS").unwrap_or_else(|_| "".to_owned());
    let additional = flags.iter().map(|s| format!("{s} ")).collect::<String>();
    std::env::set_var("RUSTFLAGS", format!("{existing_rustflags} {additional}"));
}

fn dispatch(cmd: Command) -> Result<(), std::io::Error> {
    println!("xtask : {cmd:?}");
    match cmd {
        Command::FmtLint => {
            fmt_and_lint()?;
            Ok(())
        }
        Command::BuildShaders => {
            build_shaders()?;
            Ok(())
        }
    }
}

fn fmt_and_lint() -> Result<(), std::io::Error> {
    let output = cmd!("cargo", "fmt").run()?;
    println!("xtask fmt {}", String::from_utf8_lossy(&output.stdout));
    let output = cmd!("cargo", "clippy").run()?;
    println!("xtask clippy {}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn build_shaders() -> Result<(), std::io::Error> {
    // NOTE: rust-gpu can be invoked from a build.rs step, but for now we separate
    // the projects and run this step manually.
    let project_root_dir = std::env::current_dir()?;
    let mut shader_dir = project_root_dir.clone();
    shader_dir.push("assets/shaders");
    std::env::set_current_dir(&shader_dir)?;
    println!(
        "running 'cargo build' with HARDCODED toolchain {RUST_GPU_TOOLCHAIN} (in {shader_dir:?})"
    );

    std::env::set_var("RUSTUP_TOOLCHAIN", RUST_GPU_TOOLCHAIN);

    let output = cmd!("cargo", "build").run()?;
    println!(
        "Shaders compiled to spirv. {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    std::env::set_current_dir(project_root_dir)?;
    Ok(())
}
