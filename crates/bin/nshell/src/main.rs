use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_lock::Mutex;
use core_executor::CoreAffinityExecutor;
use futures_lite::future;
use input::EngineEvent;
use plugin_loader::Plugin;
use plugin_loader::PluginCheck;
use plugin_loader::PluginError;
use render::LockWorldAndRenderState;
use render::RenderState;
use world::thing::CameraFacet;
use world::thing::ModelFacet;
use world::thing::PhysicalFacet;
use world::thing::Thing;
use world::Vector3;
use world::World;

const FRAME_LENGTH_MS: u64 = 16;

#[derive(structopt::StructOpt, Debug)]
struct CliOpts {
    #[structopt(long, default_value = plugin_loader::RELATIVE_TARGET_DIR)]
    plugin_dir: String,

    #[structopt(long)]
    backtrace: bool,

    #[structopt(long)]
    disable_validation_layer: bool,
}

fn main() {
    let opts: CliOpts = structopt::StructOpt::from_args();
    if opts.backtrace {
        println!("Setting RUST_BACKTRACE=1 to enable stack traces.");
        std::env::set_var("RUST_BACKTRACE", "1");
        println!("PWD: {:?}", std::env::current_dir().unwrap());
    }

    plugin_loader::register_tls_dtor_hook!();

    let executor = CoreAffinityExecutor::new(8);
    let mut spawners = executor.spawners();

    let mut world = world::World::new();

    let ico_model = models::Model::load(
        "assets/models/static/ico.obj",
        "assets/shaders/vertex_rustgpu.spv",
        "assets/shaders/fragment_rustgpu.spv",
    )
    .unwrap();
    let ico_model_facet = ModelFacet::new(ico_model);
    let ico_model_idx = world.add_model(ico_model_facet);

    let cube_model = models::Model::load(
        "assets/models/static/cube.obj",
        "assets/shaders/vertex_rustgpu.spv",
        "assets/shaders/fragment_rustgpu.spv",
    )
    .unwrap();
    let cube_model_facet = ModelFacet::new(cube_model);
    let cube_model_idx = world.add_model(cube_model_facet);

    let physical = PhysicalFacet::new(0.0, 0.0, 0.0);
    let camera_idx = world.add_camera(CameraFacet::new(&physical));
    let phys_idx = world.add_physical(physical);
    let camera = Thing::camera(phys_idx, camera_idx);
    let camera_thing_id = world
        .add_thing(camera)
        .expect("unable to add thing to world");

    // TODO: special purpose hooks for object ids that are relevant?
    world.maybe_camera = Some(camera_thing_id);

    // initialize some state, lots of model_object entities
    for x in -5..5i32 {
        for y in -5..5i32 {
            let model_idx = if (x + y) % 2 == 0 {
                cube_model_idx
            } else {
                ico_model_idx
            };
            let (x, y) = (x as f32, y as f32);

            let mut physical = PhysicalFacet::new(x * 4.0, y * 4.0, 0.0);
            physical.linear_velocity = Vector3::new(x, y, 1.0);
            let physical_idx = world.add_physical(physical);
            let model_object = Thing::model_object(physical_idx, model_idx);
            world.add_thing(model_object).unwrap();
        }
    }

    let world = Arc::new(Mutex::new(world));

    future::block_on(async move {
        let mut platform_context = platform::PlatformContext::new().unwrap();

        let index = platform_context
            .add_vulkan_window("nshell", 0, 0, 500, 500)
            .unwrap();

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
        let world_render_update_plugin =
            Plugin::<render::LockWorldAndRenderState>::open_from_target_dir(
                spawners[0].clone(),
                &opts.plugin_dir,
                "world_render_update_plugin",
            )
            .unwrap()
            .into_shared();

        let render_exec = core_executor::CoreAffinityExecutor::new(4);
        // state needs to be dropped on the same thread as it was created
        let render_state = RenderState::new(
            win_ptr,
            !opts.disable_validation_layer,
            render_exec.spawners(),
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
        'frame_loop: loop {
            frame_start = Instant::now();

            platform_context.pump_events();
            if let Some(EngineEvent::ExitToDesktop) =
                handle_input_events(platform_context.peek_events())
            {
                break 'frame_loop;
            }

            // Essentially, check plugins for updates every 6 seconds
            if frame % (60 * 6) == 0 {
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

            let _duration = spawners[2]
                .spawn(call_world_render_state_update_plugin(
                    &render_state,
                    &world,
                    &world_render_update_plugin,
                    last_frame_elapsed,
                ))
                .await
                .unwrap();

            let _join_result = futures_util::future::join(
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

fn handle_input_events(events: &[EngineEvent]) -> Option<EngineEvent> {
    if !events.is_empty() {
        for event in events {
            match event {
                EngineEvent::Continue => {
                    //println!("nothing event");
                }
                EngineEvent::InputDevice(input_device_event) => {
                    println!("input device event {:?}", input_device_event);
                }
                EngineEvent::Input(input_event) => {
                    println!("input event {:?}", input_event);
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
