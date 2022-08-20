use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use core_executor::CoreAffinityExecutor;
use futures_lite::future;
use input::input::EngineEvent;
use input::log;
use input::InputState;
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
                world.start_thing().with_physical(x as f32, y as f32, z as f32).emplace();
            }
        }
    }

    let world = Arc::new(Mutex::new(world));

    future::block_on(async move {
        let mut input_plugin =
            Plugin::<InputState>::open_from_target_dir(spawners[0].clone(), "sdl2_input_plugin")
                .unwrap();
        let render_plugin =
            Plugin::<RenderState>::open_from_target_dir(spawners[1].clone(), "tui_renderer_plugin")
                .unwrap().into_shared();
        let world_update_plugin =
            Plugin::<World>::open_from_target_dir(spawners[2].clone(), "world_update_plugin")
                .unwrap().into_shared();

        // state needs to be dropped on the same thread as it was created
        let mut input_state = InputState::new(vec![spawners[2].clone()]);
        let render_state = RenderState::new();
        let render_state = Arc::new(Mutex::new(render_state));

        let mut frame_start;
        let mut last_frame_complete = Instant::now();
        'frame_loop: loop {
            frame_start = Instant::now();

            spawners[3]
                .spawn(update_render_state_from_world(&render_state, &world))
                .await
                .unwrap();

            // Input owns SDL handles and must be pumped on the main/owning thread.
            check_plugin(&mut input_plugin, &mut input_state);
            let _check_plugins = futures_util::future::join(
                spawners[3].spawn(check_plugin_async(&render_plugin, &render_state)),
                spawners[4].spawn(check_plugin_async(&world_update_plugin, &world)),
            )
            .await;

            let last_frame_elapsed = last_frame_complete.elapsed();

            let _join_result = futures_util::future::join3(
                input_plugin.call_update(&mut input_state, &last_frame_elapsed),
                spawners[6].spawn(call_plugin_update_async(
                    &render_plugin,
                    &render_state,
                    &last_frame_elapsed,
                )),
                spawners[7].spawn(call_plugin_update_async(&world_update_plugin, &world, &last_frame_elapsed)),
            )
            .await;

            if let Some(EngineEvent::ExitToDesktop) = handle_input_events(&input_state) {
                println!("{:?}", world.lock().await.things);
                break 'frame_loop;
            }

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
            .update(&mut *world.lock().await)
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
        plugin.lock().await.call_update(&mut *state.lock().await, &dt).await
    })
}

fn handle_input_events(state: &InputState) -> Option<EngineEvent> {
    if let Some(events) = state
        .input_system
        .as_ref()
        .map(|input| input.events().to_vec())
    {
        if !events.is_empty() {
            //state::writeln!(state, "Processing {} events", events.len());
            for event in events {
                match event {
                    EngineEvent::Continue => input::writeln!(state, "nothing event"),
                    EngineEvent::InputDevice(input_device_event) => {
                        input::writeln!(state, "input device event {:?}", input_device_event);
                    }
                    EngineEvent::Input(input_event) => {
                        input::writeln!(state, "input event {:?}", input_event);
                    }
                    ret @ EngineEvent::ExitToDesktop => {
                        log::info!("Got {:?}", ret);
                        return Some(ret);
                    }
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
