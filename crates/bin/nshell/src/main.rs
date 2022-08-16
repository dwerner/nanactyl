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

fn main() {
    plugin_loader::register_tls_dtor_hook!();

    let executor = CoreAffinityExecutor::new(4);
    let spawners = executor.spawners();

    let mut world = world::World::new();
    world.start_thing().build();

    let mut render_state = RenderState::new();

    future::block_on(async move {
        let mut input = Plugin::<InputState>::open_from_target_dir("sdl2_input_plugin").unwrap();
        let mut renderer =
            Plugin::<RenderState>::open_from_target_dir("tui_renderer_plugin").unwrap();

        // state needs to be dropped on the same thread as it was created
        let mut input_state = InputState::new(spawners.to_owned());

        let mut frame_start = Instant::now();
        let mut last_frame_complete = Instant::now();
        'frame_loop: loop {
            frame_start = Instant::now();

            check_plugin(&mut input, &mut input_state);
            check_plugin(&mut renderer, &mut render_state);

            render_state.update(&world).unwrap();

            let _ = input
                .call_update(&mut input_state, &last_frame_complete.elapsed())
                .await;

            let _ = renderer
                .call_update(&mut render_state, &last_frame_complete.elapsed())
                .await;

            if let Some(EngineEvent::ExitToDesktop) = handle_input_events(&input_state) {
                break 'frame_loop;
            }

            let elapsed = frame_start.elapsed();
            let delay = Duration::from_millis(16).saturating_sub(elapsed);
            last_frame_complete = Instant::now();
            if render_state.updates % 60 == 0 {
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
