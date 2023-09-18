//! Implements a simple shell entrypoint for the engine.

use std::fs::File;
use std::io::{BufReader, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_lock::Mutex;
use futures_lite::future;
use histogram::Histogram;
use input::wire::InputState;
use input::{DeviceEvent, EngineEvent};
use logger::{info, LogFilter, LogLevel, Logger};
use render::{Presenter, RenderState};
use serde::Deserialize;
use structopt::StructOpt;
use structopt_yaml::StructOptYaml;
use world::{AssetLoaderState, AssetLoaderStateAndWorldLock, WorldLockAndControllerState};

const FRAME_LENGTH_MS: u64 = 8;

#[derive(StructOpt, Debug, StructOptYaml, Deserialize)]
#[serde(default)]
struct CliOpts {
    #[structopt(long)]
    cwd: Option<PathBuf>,

    #[structopt(long)]
    backtrace: bool,

    #[structopt(long)]
    enable_validation_layer: bool,

    #[structopt(long)]
    connect_to_server: Option<SocketAddr>,

    #[structopt(long, default_value = "15")]
    check_system_interval: u64,

    #[structopt(long, default_value = "info")]
    log_level: LogLevel,

    #[structopt(long)]
    log_level_filter: Option<LogLevel>,

    #[structopt(long)]
    log_tag_filter: Option<String>,

    #[structopt(long)]
    net_disabled: bool,
}

impl CliOpts {
    fn load_with_overrides(logger: &Logger) -> CliOpts {
        let config_file = Path::new("nshell.yaml");
        let opts = if config_file.exists() {
            info!(logger, "Loading config from {:?}", config_file);
            let mut reader = BufReader::new(File::open(config_file).unwrap());
            let mut yaml_buf = String::new();
            reader.read_to_string(&mut yaml_buf).unwrap();
            CliOpts::from_args_with_yaml(&yaml_buf).unwrap()
        } else {
            info!(logger, "Loading config from CLI args");
            CliOpts::from_args()
        };

        if opts.backtrace {
            info!(logger, "Setting RUST_BACKTRACE=1 to enable stack traces.");
            std::env::set_var("RUST_BACKTRACE", "1");
            info!(logger, "PWD: {:?}", std::env::current_dir().unwrap());
        }
        if let Some(ref cwd) = opts.cwd {
            std::env::set_current_dir(cwd).expect("unable to set dir");
            info!(logger, "cwd set to {:?}", std::env::current_dir().unwrap());
        }
        opts
    }
}

fn main() {
    let logger = LogLevel::Info.logger().sub("nshell");

    let opts = CliOpts::load_with_overrides(&logger);
    match (opts.log_level_filter, opts.log_tag_filter) {
        (None, Some(tag)) => logger.set_filter(LogFilter::tag(&tag)),
        (Some(level), None) => logger.set_filter(LogFilter::level(level)),
        (Some(level), Some(prefix)) => logger.set_filter(LogFilter::level_and_tag(level, &prefix)),
        (None, None) => {}
    }

    let world = world::World::new(opts.connect_to_server, &logger, opts.net_disabled);
    let world = Arc::new(Mutex::new(world));

    let logger2 = logger.sub("main");
    future::block_on(async move {
        let mut frame_start;
        let mut last_frame_complete = Instant::now();

        let mut frame = 0u64;
        let own_controllers: [InputState; 2] = Default::default();
        let own_controllers = Arc::new(Mutex::new(own_controllers));
        let logger = logger2;
        let mut frame_histogram = Histogram::new();
        let mut platform_context = platform::PlatformContext::new(&logger).unwrap();

        let index = if opts.net_disabled {
            platform_context
                .add_vulkan_window("nshell (net disabled)", 0, 0, 640, 400)
                .unwrap()
        } else if opts.connect_to_server.is_some() {
            platform_context
                .add_vulkan_window("nshell-client", 640, 0, 640, 400)
                .unwrap()
        } else {
            platform_context
                .add_vulkan_window("nshell-server", 0, 0, 640, 400)
                .unwrap()
        };

        let win_ptr = platform_context.get_raw_window_handle(index).unwrap();
        let egui_context = egui::Context::new();

        let render_state = RenderState::new(
            win_ptr,
            opts.enable_validation_layer,
            opts.connect_to_server.is_none(),
            logger.sub("render_state"),
        )
        .into_shared();

        let mut ash_renderer_system = ash_renderer_system::VulkanRenderPluginState::default();
        ash_renderer_system.load(&mut *render_state.lock().await);

        let mut world_update_system = world_update_system::WorldUpdate::new();
        world_update_system.load(&mut *world.lock().await);

        let mut net_sync_system = if !opts.net_disabled {
            let world = Arc::clone(&world);
            let controller_state = Arc::clone(&own_controllers);
            let mut net = net_sync_system::NetSyncState::new();
            let mut state = WorldLockAndControllerState::lock(&world, &controller_state).await;
            net.load(&mut state);
            Some(net)
        } else {
            None
        };

        let asset_state = Arc::new(Mutex::new(AssetLoaderState::default()));
        let mut asset_loader = asset_loader_system::AssetLoader::new();
        asset_loader.load(&mut AssetLoaderStateAndWorldLock::lock(&world, &asset_state).await);

        'frame_loop: loop {
            frame_start = Instant::now();
            platform_context.pump_events();

            if let Some(EngineEvent::ExitToDesktop) = handle_input_events(
                platform_context.peek_events(),
                &mut *own_controllers.lock().await,
                logger.sub("handle_input_events"),
            ) {
                break 'frame_loop;
            }

            let last_frame_elapsed = last_frame_complete.elapsed();

            asset_loader.update(
                &mut AssetLoaderStateAndWorldLock::lock(&world, &asset_state).await,
                &last_frame_elapsed,
            );

            // This is a bit convoluted, but the renderer plugin allows us to fetch a
            // pointer to it's "state" which in this case is a dyn Renderer + Presenter
            // trait object
            render_state.lock().await.upload_untracked_graphics_prefabs(
                &*world.as_ref().lock().await,
                &mut ash_renderer_system,
            );

            match net_sync_system.as_mut() {
                Some(net_sync_system) => {
                    net_sync_system.update(
                        &mut WorldLockAndControllerState::lock(&world, &own_controllers).await,
                        &last_frame_elapsed,
                    );
                }
                // Net is not enabled, so just update the world with the controller state
                None => {
                    let controller_state = own_controllers.lock().await;
                    let world = &mut *world.lock().await;
                    world.set_server_controller_state(controller_state[0]);
                    world.set_client_controller_state(controller_state[1]);
                }
            }

            ash_renderer_system.present(&*world.as_ref().lock().await);

            // update the renderer and the world simultaneously
            ash_renderer_system.update(&mut *render_state.lock().await, &last_frame_elapsed);
            world_update_system.update(&mut *world.lock().await, &last_frame_elapsed);

            let elapsed = frame_start.elapsed();
            let last_frame_elapsed_micros = elapsed.as_micros();

            frame_histogram
                .increment(last_frame_elapsed_micros as u64)
                .unwrap();

            if frame % 1000 == 0 {
                info!(
                    logger,
                    "Frame time (µs): Min: {} Avg: {} Max: {} StdDev: {} 50%: {}, 90%: {}, 99%: {}, 99.9%:{}",
                    frame_histogram.minimum().unwrap(),
                    frame_histogram.mean().unwrap(),
                    frame_histogram.maximum().unwrap(),
                    frame_histogram.stddev().unwrap(),
                    frame_histogram.percentile(50.0).unwrap(),
                    frame_histogram.percentile(90.0).unwrap(),
                    frame_histogram.percentile(99.0).unwrap(),
                    frame_histogram.percentile(99.9).unwrap(),
                );
                frame_histogram.clear();
            }

            let delay = Duration::from_millis(FRAME_LENGTH_MS).saturating_sub(elapsed);
            last_frame_complete = Instant::now();

            smol::Timer::after(delay).await;

            frame += 1;
        } // 'frame_loop
    });

    info!(logger, "quitting.");
}

fn handle_input_events(
    events: &[EngineEvent],
    controllers: &mut [InputState; 2],
    logger: Logger,
) -> Option<EngineEvent> {
    if !events.is_empty() {
        for event in events {
            match event {
                EngineEvent::Continue => {}
                EngineEvent::InputDevice(DeviceEvent::GameControllerAdded(id)) => {
                    info!(logger, "gamepad {id} added");
                    controllers[*id as usize] = InputState::new(*id as u8);
                }
                EngineEvent::InputDevice(DeviceEvent::GameControllerRemoved(id)) => {
                    info!(logger, "gamepad {id} removed");
                    controllers[*id as usize] = Default::default();
                }
                EngineEvent::InputDevice(input_device_event) => {
                    info!(logger, "input device event {input_device_event:?}");
                }
                EngineEvent::Input(input_event) => {
                    controllers[0].update_from_event(input_event);
                }
                ret @ EngineEvent::ExitToDesktop => {
                    info!(logger, "Got exit with code {ret:?}");
                    return Some(ret.clone());
                }
            }
        }
    }
    None
}
