use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_lock::Mutex;
use core_executor::CoreAffinityExecutor;
use futures_lite::future;
use input::wire::ControllerState;
use input::DeviceEvent;
use input::EngineEvent;
use plugin_loader::Plugin;
use plugin_loader::PluginCheck;
use plugin_loader::PluginError;
use render::LockWorldAndRenderState;
use render::RenderState;
use world::World;

const FRAME_LENGTH_MS: u64 = 16;

#[derive(structopt::StructOpt, Debug)]
struct CliOpts {
    #[structopt(long, default_value = plugin_loader::RELATIVE_TARGET_DIR)]
    plugin_dir: String,

    #[structopt(long)]
    cwd: Option<PathBuf>,

    #[structopt(long)]
    backtrace: bool,

    #[structopt(long)]
    disable_validation_layer: bool,

    #[structopt(long)]
    connect_to_server: Option<SocketAddr>,
}

fn main() {
    let opts: CliOpts = structopt::StructOpt::from_args();
    if opts.backtrace {
        println!("Setting RUST_BACKTRACE=1 to enable stack traces.");
        std::env::set_var("RUST_BACKTRACE", "1");
        println!("PWD: {:?}", std::env::current_dir().unwrap());
    }
    if let Some(cwd) = opts.cwd {
        std::env::set_current_dir(cwd).expect("unable to set dir");
        println!("cwd set to {:?}", std::env::current_dir().unwrap());
    }

    plugin_loader::register_tls_dtor_hook!();

    let executor = CoreAffinityExecutor::new(8);
    let mut spawners = executor.spawners();

    let world = world::World::new(opts.connect_to_server, true);
    let world = Arc::new(Mutex::new(world));

    future::block_on(async move {
        let mut platform_context = platform::PlatformContext::new().unwrap();

        let index = if opts.connect_to_server.is_some() {
            platform_context
                .add_vulkan_window("nshell-client", 640, 0, 640, 400)
                .unwrap()
        } else {
            platform_context
                .add_vulkan_window("nshell-server", 0, 0, 640, 400)
                .unwrap()
        };

        let win_ptr = platform_context.get_raw_window_handle(index).unwrap();

        let ash_renderer_plugin = Plugin::<RenderState>::open_from_target_dir(
            spawners[0].clone(),
            &opts.plugin_dir,
            "ash_renderer_plugin",
        )
        .unwrap()
        .into_shared();
        let world_update_plugin = Plugin::<World>::open_from_target_dir(
            spawners[0].clone(),
            &opts.plugin_dir,
            "world_update_plugin",
        )
        .unwrap()
        .into_shared();
        let asset_loader_plugin = Plugin::<World>::open_from_target_dir(
            spawners[0].clone(),
            &opts.plugin_dir,
            "asset_loader_plugin",
        )
        .unwrap()
        .into_shared();
        let world_render_update_plugin =
            Plugin::<render::LockWorldAndRenderState>::open_from_target_dir(
                spawners[0].clone(),
                &opts.plugin_dir,
                "world_render_update_plugin",
            )
            .unwrap()
            .into_shared();

        // state needs to be dropped on the same thread as it was created
        let render_state = RenderState::new(
            win_ptr,
            !opts.disable_validation_layer,
            opts.connect_to_server.is_none(),
        )
        .into_shared();

        let mut frame_start;
        let mut last_frame_complete = Instant::now();

        {
            let mut world_render_update_state =
                LockWorldAndRenderState::lock(&world, &render_state).await;
            world_render_update_state.update_models();
        }

        let mut frame = 0u64;
        let mut controllers: [ControllerState; 2] = unsafe { std::mem::zeroed() };
        'frame_loop: loop {
            frame_start = Instant::now();

            platform_context.pump_events();
            if let Some(EngineEvent::ExitToDesktop) =
                handle_input_events(platform_context.peek_events(), &mut controllers)
            {
                break 'frame_loop;
            }

            // Essentially, check plugins for updates every 2 seconds
            if frame % (60 * 2) == 0 {
                check_plugin_async(&asset_loader_plugin, &world).await;

                check_plugin(
                    &mut *world_render_update_plugin.lock().await,
                    &mut LockWorldAndRenderState::lock(&world, &render_state).await,
                );

                let _check_plugins = futures_util::future::join(
                    spawners[3].spawn(check_plugin_async(&ash_renderer_plugin, &render_state)),
                    spawners[5].spawn(check_plugin_async(&world_update_plugin, &world)),
                )
                .await;
            }

            let last_frame_elapsed = last_frame_complete.elapsed();

            let _asset_loader_duration = spawners[1]
                .spawn(call_plugin_update_async(
                    &asset_loader_plugin,
                    &world,
                    &last_frame_elapsed,
                ))
                .await;

            let _duration = spawners[2]
                .spawn(call_world_render_state_update_plugin(
                    &render_state,
                    &world,
                    &world_render_update_plugin,
                    last_frame_elapsed,
                ))
                .await
                .unwrap();

            let controllers = controllers.clone();

            let nworld = Arc::clone(&world);
            let _join_result = futures_util::future::join3(
                spawners[1].spawn(call_plugin_update_async(
                    &ash_renderer_plugin,
                    &render_state,
                    &last_frame_elapsed,
                )),
                spawners[3].spawn(call_plugin_update_async(
                    &world_update_plugin,
                    &world,
                    &last_frame_elapsed,
                )),
                spawners[3].spawn(Box::pin(async move {
                    let mut world = nworld.lock().await;

                    // TODO: fix sized issue (> 96)
                    if world.is_server() && world.things.len() >= 96 {
                        match world.pump_connection_as_server().await {
                            Ok(controller_state) => {
                                //println!("got controller state from client {controller_state:?}");
                                // TODO: support N controllers, or just one per client?
                                world.set_client_controller_state(controller_state[0]);
                            }
                            Err(err) => println!("error pumping connection {:?}", err),
                        }
                    } else {
                        match world.pump_connection_as_client(controllers).await {
                            Err(world::WorldError::Network(network::RpcError::Receive(kind)))
                                if kind.kind() == std::io::ErrorKind::TimedOut => {}
                            Err(err) => {
                                println!("error pumping connection {:?}", err);
                            }
                            _ => (),
                        }
                    };
                })),
            )
            .await;

            let elapsed = frame_start.elapsed();
            let delay = Duration::from_millis(FRAME_LENGTH_MS).saturating_sub(elapsed);
            last_frame_complete = Instant::now();
            smol::Timer::after(delay).await;

            frame += 1;
        }
    });
    println!("nshell closed");
}

fn call_world_render_state_update_plugin(
    render_state: &Arc<Mutex<RenderState>>,
    world: &Arc<Mutex<World>>,
    plugin: &Arc<Mutex<Plugin<render::LockWorldAndRenderState>>>,
    dt: Duration,
) -> Pin<Box<impl Future<Output = Result<Duration, PluginError>> + Send + Sync>> {
    let render_state = Arc::clone(render_state);
    let world = Arc::clone(world);
    let plugin = Arc::clone(plugin);
    Box::pin(async move {
        let mut state = render::LockWorldAndRenderState::lock(&world, &render_state).await;
        plugin.lock().await.call_update(&mut state, &dt).await
    })
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
    controllers: &mut [ControllerState; 2],
) -> Option<EngineEvent> {
    if !events.is_empty() {
        for event in events {
            match event {
                EngineEvent::Continue => {
                    //println!("nothing event");
                }
                EngineEvent::InputDevice(DeviceEvent::GameControllerAdded(id)) => {
                    println!("gamepad {id} added");
                    controllers[*id as usize] = ControllerState::new(*id as u8);
                }
                EngineEvent::InputDevice(DeviceEvent::GameControllerRemoved(id)) => {
                    println!("gamepad {id} removed");
                    controllers[*id as usize] = unsafe { std::mem::zeroed() };
                }
                EngineEvent::InputDevice(input_device_event) => {
                    println!("input device event {:?}", input_device_event);
                }
                EngineEvent::Input(input_event) => {
                    let id = input_event.id() as u32;
                    controllers.get_mut(id as usize).map(|controller| {
                        controller.update_with_event(input_event);
                    });
                }
                ret @ EngineEvent::ExitToDesktop => {
                    println!("Got {:?}", ret);
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
) -> Pin<Box<impl Future<Output = ()> + Send + Sync>>
where
    T: Send + Sync,
{
    let plugin = Arc::clone(plugin);
    let state = Arc::clone(state);
    Box::pin(async move {
        check_plugin(&mut *plugin.lock().await, &mut *state.lock().await);
    })
}

// Main loop policy for handling plugin errors
fn check_plugin<T>(plugin: &mut Plugin<T>, state: &mut T)
where
    T: Send + Sync,
{
    match plugin.check(state) {
        Ok(PluginCheck::FoundNewVersion) => println!(
            "{} plugin found new version {}",
            plugin.name(),
            plugin.version()
        ),
        Ok(PluginCheck::Unchanged) => (),
        Err(m @ PluginError::MetadataIo { .. }) => {
            println!(
                "error getting file metadata for plugin {}: {:?}",
                plugin.name(),
                m
            );
        }
        Err(o @ PluginError::ErrorOnOpen(_)) => {
            println!("error opening plugin {}: {:?}", plugin.name(), o);
        }
        Err(err) => panic!("unexpected error checking plugin - {:?}", err),
    }
}
