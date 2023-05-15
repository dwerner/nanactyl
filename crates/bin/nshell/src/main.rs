//! Implements a simple shell entrypoint for the engine.

use std::fs::File;
use std::future::Future;
use std::io::{BufReader, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_lock::Mutex;
use core_executor::ThreadPoolExecutor;
use futures_lite::future;
use histogram::Histogram;
use input::wire::InputState;
use input::{DeviceEvent, EngineEvent};
use logger::{error, info, LogLevel, Logger};
use plugin_loader::{Plugin, PluginCheck, PluginError};
use render::RenderState;
use serde::Deserialize;
use structopt::StructOpt;
use structopt_yaml::StructOptYaml;
use world::{AssetLoaderState, AssetLoaderStateAndWorldLock, World, WorldLockAndControllerState};

const FRAME_LENGTH_MS: u64 = 8;

#[derive(StructOpt, Debug, StructOptYaml, Deserialize)]
#[serde(default)]
struct CliOpts {
    #[structopt(long, default_value = plugin_loader::RELATIVE_TARGET_DIR)]
    plugin_dir: PathBuf,

    #[structopt(long)]
    cwd: Option<PathBuf>,

    #[structopt(long)]
    backtrace: bool,

    #[structopt(long)]
    enable_validation_layer: bool,

    #[structopt(long)]
    connect_to_server: Option<SocketAddr>,

    #[structopt(long, default_value = "15")]
    check_plugin_interval: u64,

    #[structopt(long, default_value = "info")]
    log_level: LogLevel,

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
    plugin_loader::register_tls_dtor_hook!();

    let opts = CliOpts::load_with_overrides(&logger);

    let mut executor = ThreadPoolExecutor::new(8);

    // FIXME: currently the server-side must be started first, and waits for a
    // client to connect here.
    let world = world::World::new(opts.connect_to_server, &logger, opts.net_disabled);
    let world = Arc::new(Mutex::new(world));

    let logger2 = logger.sub("main");
    future::block_on(async move {
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

        // ash renderer
        let ash_renderer_plugin =
            Plugin::<RenderState>::open_from_target_dir(&opts.plugin_dir, "ash_renderer_plugin")
                .unwrap()
                .into_shared();

        // world update
        let world_update_plugin =
            Plugin::<World>::open_from_target_dir(&opts.plugin_dir, "world_update_plugin")
                .unwrap()
                .into_shared();

        // net sync
        let net_sync_plugin = if !opts.net_disabled {
            Some(
                Plugin::<WorldLockAndControllerState>::open_from_target_dir(
                    &opts.plugin_dir,
                    "net_sync_plugin",
                )
                .unwrap()
                .into_shared(),
            )
        } else {
            None
        };

        // asset loader
        let asset_loader_state = Arc::new(Mutex::new(AssetLoaderState::default()));
        let asset_loader_plugin = Plugin::<AssetLoaderStateAndWorldLock>::open_from_target_dir(
            &opts.plugin_dir,
            "asset_loader_plugin",
        )
        .unwrap()
        .into_shared();

        // state needs to be dropped on the same thread as it was created
        let render_state = RenderState::new(
            win_ptr,
            opts.enable_validation_layer,
            opts.connect_to_server.is_none(),
        )
        .into_shared();

        let mut frame_start;
        let mut last_frame_complete = Instant::now();

        {
            render_state
                .lock()
                .await
                .upload_untracked_graphics(&*world.lock().await);
        }

        let mut frame = 0u64;
        let own_controllers: [InputState; 2] = Default::default();
        let own_controllers = Arc::new(Mutex::new(own_controllers));

        'frame_loop: loop {
            let world = Arc::clone(&world);
            frame_start = Instant::now();

            platform_context.pump_events();

            if let Some(EngineEvent::ExitToDesktop) = handle_input_events(
                platform_context.peek_events(),
                &mut *own_controllers.lock().await,
                logger.sub("handle_input_events"),
            ) {
                break 'frame_loop;
            }

            // Essentially, check for updated versions of plugins every 2 seconds
            if frame % opts.check_plugin_interval == 0 {
                check_plugin(
                    &mut *asset_loader_plugin.lock().await,
                    &mut AssetLoaderStateAndWorldLock::lock(&world, &asset_loader_state).await,
                    &logger,
                );

                if let Some(net_sync_plugin) = &net_sync_plugin {
                    check_plugin(
                        &mut *net_sync_plugin.lock().await,
                        &mut WorldLockAndControllerState::lock(&world, &own_controllers).await,
                        &logger,
                    );
                }

                let _check_plugins = futures_util::future::join(
                    executor.spawn_on_core(
                        3,
                        check_plugin_async(&ash_renderer_plugin, &render_state, &logger),
                    ),
                    executor.spawn_on_core(
                        5,
                        check_plugin_async(&world_update_plugin, &world, &logger),
                    ),
                )
                .await;
            }

            let last_frame_elapsed = last_frame_complete.elapsed();

            let _asset_loader_duration = executor
                .spawn_on_core(1, {
                    let plugin = Arc::clone(&asset_loader_plugin);
                    let asset_loader_state = Arc::clone(&asset_loader_state);
                    let world = Arc::clone(&world);
                    Box::pin(async move {
                        let mut state_and_world =
                            AssetLoaderStateAndWorldLock::lock(&world, &asset_loader_state).await;
                        plugin
                            .lock()
                            .await
                            .call_update(&mut state_and_world, &last_frame_elapsed)
                            .await
                    })
                })
                .await
                .unwrap();

            {
                // TODO: stop copying state around.
                let state = &mut *render_state.lock().await;
                let world = &*world.as_ref().lock().await;
                state.upload_untracked_graphics(world);
                state.update_render_scene(world).unwrap();
            }

            match net_sync_plugin {
                Some(ref net_sync_plugin) => {
                    let _update_result = executor
                        .spawn_on_core(3, {
                            let world = Arc::clone(&world);
                            let controller_state = Arc::clone(&own_controllers);
                            let net_sync_plugin = Arc::clone(net_sync_plugin);
                            Box::pin(async move {
                                let mut state =
                                    WorldLockAndControllerState::lock(&world, &controller_state)
                                        .await;
                                net_sync_plugin
                                    .lock()
                                    .await
                                    .call_update(&mut state, &last_frame_elapsed)
                                    .await
                            })
                        })
                        .await;
                }
                // Net is not enabled, so just update the world with the controller state
                None => {
                    let controller_state = own_controllers.lock().await;
                    let world = &mut *world.lock().await;
                    world.set_server_controller_state(controller_state[0]);
                    world.set_client_controller_state(controller_state[1]);
                }
            }

            let _join_result = futures_util::future::join(
                executor.spawn_on_core(
                    1,
                    call_plugin_update_async(
                        &ash_renderer_plugin,
                        &render_state,
                        &last_frame_elapsed,
                    ),
                ),
                executor.spawn_on_core(
                    3,
                    call_plugin_update_async(&world_update_plugin, &world, &last_frame_elapsed),
                ),
            )
            .await;

            let elapsed = frame_start.elapsed();
            let last_frame_elapsed_micros = elapsed.as_micros();

            frame_histogram
                .increment(last_frame_elapsed_micros as u64)
                .unwrap();

            if frame % 1000 == 0 {
                info!(
                    logger,
                    "Frame time (us): Min: {} Avg: {} Max: {} StdDev: {} 50%: {}, 90%: {}, 99%: {}, 99.9%:{}",
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

        world_update_plugin
            .lock()
            .await
            .call_unload(&mut *world.lock().await)
            .unwrap();
        drop(world_update_plugin);

        // Unload stateful plugins
        ash_renderer_plugin
            .lock()
            .await
            .call_unload(&mut *render_state.lock().await)
            .unwrap();
    });

    info!(logger, "quitting.");
}

fn call_plugin_update_async<T>(
    plugin: &Arc<Mutex<Plugin<T>>>,
    state: &Arc<Mutex<T>>,
    dt: &Duration,
) -> Pin<Box<impl Future<Output = Result<Duration, PluginError>> + Send + Sync>>
where
    T: Send + Sync,
{
    let plugin = Arc::clone(plugin);
    let state = Arc::clone(state);
    let dt = *dt;
    Box::pin(async move {
        plugin
            .lock()
            .await
            .call_update(&mut *state.lock().await, &dt)
            .await
    })
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

fn check_plugin_async<T>(
    plugin: &Arc<Mutex<Plugin<T>>>,
    state: &Arc<Mutex<T>>,
    logger: &Logger,
) -> Pin<Box<impl Future<Output = ()> + Send + Sync>>
where
    T: Send + Sync,
{
    let logger = logger.sub("check_plugin_async");
    let plugin = Arc::clone(plugin);
    let state = Arc::clone(state);
    Box::pin(async move {
        check_plugin(&mut *plugin.lock().await, &mut *state.lock().await, &logger);
    })
}

// Main loop policy for handling plugin errors
fn check_plugin<T>(plugin: &mut Plugin<T>, state: &mut T, logger: &Logger)
where
    T: Send + Sync,
{
    let logger = logger.sub("check_plugin");
    match plugin.check(state) {
        Ok(PluginCheck::FoundNewVersion) => info!(
            logger,
            "found new version ({}) of plugin: {}",
            plugin.version(),
            plugin.name(),
        ),
        Ok(PluginCheck::Unchanged) => (),
        Err(m @ PluginError::MetadataIo { .. }) => {
            error!(
                logger,
                "error getting file metadata for plugin {}: {:?}",
                plugin.name(),
                m
            );
        }
        Err(o @ PluginError::ErrorOnOpen(_)) => {
            error!(logger, "error opening plugin {}: {:?}", plugin.name(), o);
        }
        Err(err) => panic!("unexpected error checking plugin - {err:?}"),
    }
}
