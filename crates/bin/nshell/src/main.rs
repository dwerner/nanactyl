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

    let executor = CoreAffinityExecutor::new(4);
    let mut spawners = executor.spawners();

    let world = Arc::new(Mutex::new(world::World::new()));

    future::block_on(async move {
        world.lock().await.start_thing().emplace().unwrap();

        let mut input =
            Plugin::<InputState>::open_from_target_dir(spawners[0].clone(), "sdl2_input_plugin")
                .unwrap();
        let renderer =
            Plugin::<RenderState>::open_from_target_dir(spawners[1].clone(), "tui_renderer_plugin")
                .unwrap();

        let render_plugin = Arc::new(Mutex::new(renderer));

        // state needs to be dropped on the same thread as it was created
        let mut input_state = InputState::new(vec![spawners[2].clone()]);
        let render_state = RenderState::new();
        let render_state = Arc::new(Mutex::new(render_state));

        let mut frame_start;
        let mut last_frame_complete = Instant::now();
        'frame_loop: loop {
            frame_start = Instant::now();

            world.lock().await.start_thing().emplace().unwrap();
            spawners[3].spawn(update_render_state_from_world(&render_state, &world)).await.unwrap();

            // Input owns SDL handles and must be pumped on the main/owning thread.
            check_plugin(&mut input, &mut input_state);
            check_plugin_async(&render_plugin, &render_state).await;

            let last_frame_elapsed = last_frame_complete.elapsed();

            //let start = Instant::now();
            let _join_result = futures_util::future::join(
                input.call_update(&mut input_state, &last_frame_elapsed),
                spawners[3].spawn(render_task(
                    &render_plugin,
                    &render_state,
                    &last_frame_elapsed,
                )),
            )
            .await;

            //println!("joined input and render {join_result:?} in {:?}", start.elapsed());

            if let Some(EngineEvent::ExitToDesktop) = handle_input_events(&input_state) {
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

fn update_render_state_from_world<'a>(render_state: &Arc<Mutex<RenderState>>, world: &Arc<Mutex<World>>) -> Pin<Box<impl Future<Output=()> + Send + Sync>> {
    let render_state = Arc::clone(render_state);
    let world = Arc::clone(world);
    Box::pin(async move {
        render_state.lock().await.update(&mut *world.lock().await).await;
    })
}

fn render_task(
    render_plugin: &Arc<Mutex<Plugin<RenderState>>>,
    render_state: &Arc<Mutex<RenderState>>,
    last_frame_elapsed: &Duration,
) -> Pin<Box<impl Future<Output = Result<Duration, PluginError>>>> {
    let render_plugin = Arc::clone(render_plugin);
    let render_state = Arc::clone(render_state);
    let last_frame_elapsed = last_frame_elapsed.clone();
    Box::pin(async move {
        let renderer = &mut render_plugin.lock().await;
        // do some work
        for _ in 0..10000 {}
        renderer
            .call_update(&mut *(render_state.lock().await), &last_frame_elapsed)
            .await
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

async fn check_plugin_async<T>(plugin: &Arc<Mutex<Plugin<T>>>, state: &Arc<Mutex<T>>) where T: Send + Sync {
    check_plugin(&mut *plugin.lock().await, &mut *state.lock().await);
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
