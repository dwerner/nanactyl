use std::fs::File;
use std::io::BufRead;
use std::time::Duration;

use duct::cmd;
use structopt::StructOpt;
use colorful::Colorful;

#[derive(StructOpt, Debug)]
enum Command {
    RunServerAndClient,
    FmtLint,
    BuildShaders,
    BuildPlugin {
        #[structopt(short)]
        plugin_name: String
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

fn main() {
    std::env::set_var("RUST_BACKTRACE", "full");
    let opts = Opts::from_args();
    println!("printlnd: {:?}", opts.cmd);
    (|| -> Result<(), std::io::Error> {
        match opts.cmd {
            Some(Command::RunServerAndClient) => {
                run_server_and_client()?;
                Ok(())
            }
            Some(Command::FmtLint) => {
                fmt_and_lint()?;
                Ok(())
            }
            Some(Command::BuildAllPlugins) => {
                build_plugins()?;
                Ok(())
            }
            Some(Command::BuildPlugin{plugin_name}) => {
                build_one_plugin(&plugin_name)?;
                Ok(())
            }
            Some(Command::BuildShaders) => {
                build_shaders()?;
                Ok(())
            }
            Some(Command::BuildAll) => {
                fmt_and_lint()?;
                build_shaders()?;
                build_plugins()?;
                Ok(())
            }
            None => {
                println!("No command given.");
                Ok(())
            }
        }
    })()
    .unwrap();
}

fn fmt_and_lint() -> Result<(), std::io::Error> {
    cmd!("cargo", "+nightly", "fmt").run()?;
    cmd!("cargo", "clippy").run()?;
    Ok(())
}

fn build_plugins() -> Result<(), std::io::Error> {
    cmd!("cargo", "build", "--release").run()?;
    Ok(())
}

fn build_one_plugin(plugin_name: &str) -> Result<(), std::io::Error> {
    let project_root_dir = std::env::current_dir()?;
    let mut plugin_dir = project_root_dir.clone();
    plugin_dir.push(format!("crates/plugins/{plugin_name}"));
    client!("{plugin_dir:?}");
    assert!(File::open(&plugin_dir)?.metadata()?.is_dir(), "{plugin_dir:?} doesn't correspond to a plugin.");
    std::env::set_current_dir(plugin_dir)?;
    cmd!("cargo", "build", "--release").run()?;
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
    std::env::set_current_dir(shader_dir)?;
    cmd!("cargo", "build").run()?;
    println!("Shaders compiled to spirv");
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
        "--enable-validation-layer",
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
        "--enable-validation-layer",
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