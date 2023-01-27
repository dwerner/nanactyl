use std::io::{BufRead, Read};
use std::time::Duration;

use duct::cmd;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
enum Command {
    RunServerAndClient,
    Lint,
    BuildShaders,
    BuildModules,
    BuildAll,
}

#[derive(StructOpt)]
struct Opts {
    #[structopt(subcommand)]
    cmd: Option<Command>,
}

fn main() {
    std::env::set_var("RUST_BACKTRACE", "full");
    let opts = Opts::from_args();
    println!("command: {:?}", opts.cmd);
    (|| -> Result<(), std::io::Error> {
        match opts.cmd {
            Some(Command::RunServerAndClient) => {
                run_server_and_client()?;
                Ok(())
            }
            Some(Command::Lint) => {
                cmd!("cargo", "+nightly", "fmt").run()?;
                cmd!("cargo", "clippy").run()?;
                Ok(())
            }
            Some(Command::BuildModules) => {
                cmd!("cargo", "build", "--release").run()?;
                Ok(())
            }
            Some(Command::BuildShaders) => {
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
            Some(Command::BuildAll) => {
                println!("Not yet implemented.");
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

fn run_server_and_client() -> Result<(), std::io::Error> {
    let server_proc = cmd!("cargo", "run", "--release", "--bin", "nshell").reader()?;
    std::thread::sleep(Duration::from_secs(1));
    let client_proc = cmd!(
        "cargo",
        "run",
        "--release",
        "--bin",
        "nshell",
        "--",
        "--connect-to-server",
        "127.0.0.1:12002"
    )
    .reader()?;
    let jh = std::thread::spawn(move || {
        for line in std::io::BufReader::new(server_proc).lines() {
            println!("SERVER {}", line.unwrap());
        }
    });
    let ch = std::thread::spawn(move || {
        for line in std::io::BufReader::new(client_proc).lines() {
            println!("CLIENT {}", line.unwrap());
        }
    });

    jh.join().unwrap();
    ch.join().unwrap();
    Ok(())
}
