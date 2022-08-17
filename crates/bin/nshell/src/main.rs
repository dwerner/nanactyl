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

const FRAME_LENGTH_MS: u64 = 16;

fn main() {
    plugin_loader::register_tls_dtor_hook!();

    let executor = CoreAffinityExecutor::new(4);
    let mut spawners = executor.spawners();

    let mut world = world::World::new();
    world.start_thing().build();


    future::block_on(async move {
        let mut input = Plugin::<InputState>::open_from_target_dir(spawners[0].clone(), "sdl2_input_plugin").unwrap();
        let renderer =
            Plugin::<RenderState>::open_from_target_dir(spawners[1].clone(), "tui_renderer_plugin").unwrap();

        let renderer = Arc::new(Mutex::new(renderer));

        // state needs to be dropped on the same thread as it was created
        let mut input_state = InputState::new(vec![spawners[2].clone()]);
        let render_state = RenderState::new();

        let render_state = Arc::new(Mutex::new(render_state));

        let mut frame_start;
        let mut last_frame_complete = Instant::now();
        'frame_loop: loop {
            frame_start = Instant::now();

            let renderer = Arc::clone(&renderer);
            let render_state = Arc::clone(&render_state);

            check_plugin(&mut input, &mut input_state);
            check_plugin(&mut*(renderer.lock().await), &mut *(render_state.lock().await));
            render_state.lock().await.update(&world).unwrap();

            let renderer_task = Arc::clone(&renderer);
            let rs_task = Arc::clone(&render_state);

            let last_frame_elapsed = last_frame_complete.elapsed();

            //let start = Instant::now();
            let _join_result = futures_util::future::join(
                input.call_update(&mut input_state, &last_frame_elapsed),
                spawners[3].spawn(Box::pin(
                    async move {
                        let renderer = &mut renderer_task.lock().await;
                        for _ in 0..10000 { }
                        renderer.call_update(&mut *(rs_task.lock().await), &last_frame_elapsed).await
                    }
                ))
            ).await;
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
