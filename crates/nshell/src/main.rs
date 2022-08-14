use std::time::Duration;
use std::time::Instant;

use core_executor::CoreAffinityExecutor;
use plugin_loader::Plugin;

use plugin_loader::PluginCheck;
use plugin_loader::PluginError;

use state::input::EngineEvent;
use state::log;
use state::InputState;

fn main() {
    plugin_loader::register_tls_dtor_hook!();
    state::init_logger();

    log::info!("main thread startup");

    let mut input = Plugin::<InputState>::open_from_target_dir("gilrs_input_plugin").unwrap();
    let mut input2 = Plugin::<InputState>::open_from_target_dir("sdl2_input_plugin").unwrap();

    let executor = CoreAffinityExecutor::new(4);
    let spawners = executor.spawners();
    let (spawners1, spawners2) = spawners.split_at(2);

    let mut state = InputState::new(spawners1.to_owned());
    let mut state2 = InputState::new(spawners2.to_owned());

    let mut frame_start;
    'frame_loop: loop {
        frame_start = Instant::now();
        check_plugin(&mut input, &mut state);
        check_plugin(&mut input2, &mut state2);

        let _ = smol::block_on(futures::future::join(
            input.call_update(&mut state, &frame_start.elapsed()),
            input2.call_update(&mut state2, &frame_start.elapsed()),
        ));

        if let Some(EngineEvent::ExitToDesktop) = handle_input_events(&state) {
            break 'frame_loop;
        }
        if let Some(EngineEvent::ExitToDesktop) = handle_input_events(&state2) {
            break 'frame_loop;
        }

        let elapsed = frame_start.elapsed();
        let delay = Duration::from_millis(16).saturating_sub(elapsed);
        std::thread::sleep(delay);
    }
    log::info!("nshell closed");
}

fn handle_input_events(state: &InputState) -> Option<EngineEvent> {
    if let Some(events) = state
        .input_system
        .as_ref()
        .map(|input| input.events().to_vec())
    {
        if !events.is_empty() {
            state::writeln!(state, "Processing {} events", events.len());
            for event in events {
                match event {
                    EngineEvent::Continue => (),
                    EngineEvent::InputDevice(input_device_event) => {
                        state::writeln!(state, "input device event {:?}", input_device_event);
                    }
                    EngineEvent::Input(input_event) => {
                        state::writeln!(state, "input event {:?}", input_event);
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
        Err(m @ PluginError::MetadataIo(_)) => {
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
