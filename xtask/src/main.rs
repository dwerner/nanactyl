use std::fs::File;
use std::io::BufRead;
use std::path::PathBuf;
use std::time::Duration;

use colorful::Colorful;
use duct::cmd;
use structopt::StructOpt;

// This must be kept in sync with rust-gpu's requirements, and see also the
// rust-toolchain files in the respective assets/shaders subdirs.
const RUST_GPU_TOOLCHAIN: &str = "nightly-2022-12-18";

#[derive(StructOpt, Debug)]
enum Command {
    RunServerAndClient,
    FmtLint,
    BuildShaders,
    BuildPlugin {
        #[structopt(short)]
        plugin_name: String,
    },
    BuildAllPlugins,
    BuildAll,
}

#[derive(StructOpt)]
struct Opts {
    #[structopt(subcommand)]
    cmd: Option<Command>,
}

macro_rules! client {
    ($($arg:tt)*) => {
        println!("{}", format!($($arg)*).color(colorful::Color::DarkGray))
     };
}

macro_rules! server {
    ($($arg:tt)*) => {
        println!("{}", format!($($arg)*).color(colorful::Color::NavyBlue))
     };
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
    println!("xtask : {:?}", cmd);
    match cmd {
        Command::RunServerAndClient => {
            run_server_and_client()?;
            Ok(())
        }
        Command::FmtLint => {
            fmt_and_lint()?;
            Ok(())
        }
        Command::BuildAllPlugins => {
            build_plugins()?;
            Ok(())
        }
        Command::BuildPlugin { plugin_name } => {
            build_one_plugin(&plugin_name)?;
            Ok(())
        }
        Command::BuildShaders => {
            build_shaders()?;
            Ok(())
        }
        Command::BuildAll => {
            fmt_and_lint()?;
            build_shaders()?;
            build_plugins()?;
            Ok(())
        }
    }
}

fn fmt_and_lint() -> Result<(), std::io::Error> {
    let output = cmd!("cargo", "+nightly", "fmt").run()?;
    println!("xtask fmt {}", String::from_utf8_lossy(&output.stdout));
    let output = cmd!("cargo", "clippy").run()?;
    println!("xtask clippy {}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn build_plugins() -> Result<(), std::io::Error> {
    let output = cmd!("cargo", "build", "--release").run()?;
    println!(
        "xtask build everything {}",
        String::from_utf8_lossy(&output.stdout)
    );
    Ok(())
}

fn build_one_plugin(plugin_name: &str) -> Result<(), std::io::Error> {
    let project_root_dir = std::env::current_dir()?;
    let mut plugin_dir = project_root_dir.clone();
    plugin_dir.push(format!("crates/plugins/{plugin_name}"));
    client!("{plugin_dir:?}");
    assert!(
        File::open(&plugin_dir)?.metadata()?.is_dir(),
        "{plugin_dir:?} doesn't correspond to a plugin."
    );
    std::env::set_current_dir(plugin_dir)?;
    let output = cmd!("cargo", "build", "--release").run()?;
    println!(
        "Plugin {} compiled: {}",
        plugin_name,
        String::from_utf8_lossy(&output.stdout)
    );
    server!("Built plugin: crates/plugins/{plugin_name}.");
    std::env::set_current_dir(project_root_dir)?;
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
        "Shaders compiled to spirv. {}",
        String::from_utf8_lossy(&output.stdout)
    );
    std::env::set_current_dir(project_root_dir)?;
    Ok(())
}

/// Currently bound to the concept of a single server and client. Starts a
/// server, waits a second, then starts the client. Prints output to stdout with
/// fancy ascii coloration.
fn run_server_and_client() -> Result<(), std::io::Error> {
    let server_proc = cmd!(
        "cargo",
        "run",
        "--release",
        "--bin",
        "nshell",
        "--",
        //"--enable-validation-layer",
    )
    .reader()?;
    // HACK: wait 1 second for the server to start
    std::thread::sleep(Duration::from_secs(1));
    let client_proc = cmd!(
        "cargo",
        "run",
        "--release",
        "--bin",
        "nshell",
        "--",
        "--connect-to-server",
        "127.0.0.1:12002",
     //   "--enable-validation-layer",
    )
    .reader()?;
    let jh = std::thread::spawn(move || {
        for line in std::io::BufReader::new(&server_proc).lines() {
            let server_pids = server_proc.pids();
            server!("server{:?}: {}", server_pids, line.unwrap());
        }
    });
    let ch = std::thread::spawn(move || {
        for line in std::io::BufReader::new(&client_proc).lines() {
            let client_pids = client_proc.pids();
            client!("client{:?}: {}", client_pids, line.unwrap());
        }
    });

    jh.join().unwrap();
    ch.join().unwrap();
    Ok(())
}
