use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use core_executor::CoreAffinityExecutor;
use futures_lite::future;
use input::EngineEvent;
use plugin_loader::Plugin;
use plugin_loader::PluginCheck;
use plugin_loader::PluginError;
use render::RenderState;
use smol::lock::Mutex;
use world::World;

const FRAME_LENGTH_MS: u64 = 16;

fn main() {
    plugin_loader::register_tls_dtor_hook!();

    let executor = CoreAffinityExecutor::new(8);
    let mut spawners = executor.spawners();

    let mut world = world::World::new();

    // initialize some state, in this case a lot of physical entities
    for x in 0..10u32 {
        for y in 0..10u32 {
            for z in 0..10u32 {
                world
                    .start_thing()
                    .with_physical(x as f32, y as f32, z as f32)
                    .emplace();
            }
        }
    }

    let world = Arc::new(Mutex::new(world));

    future::block_on(async move {
        let mut platform_context = platform::PlatformContext::new().unwrap();

        let index = platform_context
            .add_vulkan_window("nshell", 0, 0, 640, 480)
            .unwrap();

        let win_ptr = platform_context.get_raw_window_handle(index).unwrap();

        let ash_renderer_plugin =
            Plugin::<RenderState>::open_from_target_dir(spawners[0].clone(), "ash_renderer_plugin")
                .unwrap()
                .into_shared();
        let world_update_plugin =
            Plugin::<World>::open_from_target_dir(spawners[0].clone(), "world_update_plugin")
                .unwrap()
                .into_shared();

        // state needs to be dropped on the same thread as it was created
        let render_state = RenderState::new(win_ptr).into_shared();

        let mut frame_start;
        let mut last_frame_complete = Instant::now();
        'frame_loop: loop {
            frame_start = Instant::now();

            platform_context.pump_events();
            if let Some(EngineEvent::ExitToDesktop) =
                handle_input_events(platform_context.peek_events())
            {
                break 'frame_loop;
            }

            spawners[3]
                .spawn(update_render_state_from_world(&render_state, &world))
                .await
                .unwrap();

            // Input owns SDL handles and must be pumped on the main/owning thread.
            let _check_plugins = futures_util::future::join(
                spawners[3].spawn(check_plugin_async(&ash_renderer_plugin, &render_state)),
                spawners[5].spawn(check_plugin_async(&world_update_plugin, &world)),
            )
            .await;

            let last_frame_elapsed = last_frame_complete.elapsed();

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
            if render_state.lock().await.updates % 60 == 0 {
                println!("{:?}", elapsed);
            }
            smol::Timer::after(delay).await;
        }
    });
    log::info!("nshell closed");
}

fn update_render_state_from_world<'a>(
    render_state: &Arc<Mutex<RenderState>>,
    world: &Arc<Mutex<World>>,
) -> Pin<Box<impl Future<Output = ()> + Send + Sync>> {
    let render_state = Arc::clone(render_state);
    let world = Arc::clone(world);
    Box::pin(async move {
        render_state
            .lock()
            .await
            .update_from_world(&mut *world.lock().await)
            .await;
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
    let dt = dt.clone();
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
        //state::writeln!(state, "Processing {} events", events.len());
        for event in events {
            match event {
                EngineEvent::Continue => log::debug!("nothing event"),
                EngineEvent::InputDevice(input_device_event) => {
                    log::info!("input device event {:?}", input_device_event);
                }
                EngineEvent::Input(input_event) => {
                    log::info!("input event {:?}", input_event);
                }
                ret @ EngineEvent::ExitToDesktop => {
                    log::info!("Got {:?}", ret);
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
        Ok(PluginCheck::FoundNewVersion) => log::info!(
            "{} plugin found new version {}",
            plugin.name(),
            plugin.version()
        ),
        Ok(PluginCheck::Unchanged) => (),
        Err(m @ PluginError::MetadataIo { .. }) => {
            log::warn!(
                "error getting file metadata for plugin {}: {:?}",
                plugin.name(),
                m
            );
        }
        Err(o @ PluginError::ErrorOnOpen(_)) => {
            log::warn!("error opening plugin {}: {:?}", plugin.name(), o);
        }
        Err(err) => panic!("unexpected error checking plugin - {:?}", err),
    }
}
