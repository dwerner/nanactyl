use std::time::Duration;
use std::time::Instant;

use plugin_loader::Plugin;

use plugin_loader::PluginCheck;
use plugin_loader::PluginError;

use state::log;
use state::sdl2;
use state::State;

#[smol_potat::main(threads = 4)]
async fn main() {
    plugin_loader::register_tls_dtor_hook!();
    state::init_logger();

    log::info!("main thread startup");

    let mut input = Plugin::<State>::open_from_target_dir("input_plugin").unwrap();
    let mut state = State::new();
    let mut updated = Instant::now();
    'main_thread_game_loop: loop {
        match input.check(&mut state) {
            Ok(PluginCheck::FoundNewVersion) => log::info!("input plugin updated"),
            Ok(PluginCheck::Unchanged) => (),
            Err(m @ PluginError::MetadataIo(_)) => {
                log::warn!("error gettin file metadata for plugin: {:?}", m);
            }
            Err(o @ PluginError::ErrorOnOpen(_)) => {
                log::warn!("error opening plugin: {:?}", o);
            }
            Err(err) => panic!("unexpected error checking plugin - {:?}", err),
        }

        let _update_duration = input
            .call_update(&mut state, &updated.elapsed())
            .await
            .unwrap();
        updated = Instant::now();

        if let Some(ref mut input) = state.input_system {
            let events = input.events();
            if !events.is_empty() {
                log::info!("Processing {} events", events.len());
                for event in events {
                    match event {
                        state::input::EngineEvent::Continue => (),
                        state::input::EngineEvent::InputDevice(input_device_event) => {
                            log::info!("input device event {:?}", input_device_event)
                        }
                        state::input::EngineEvent::Input(input_event) => {
                            log::info!("input event {:?}", input_event)
                        }
                        state::input::EngineEvent::ExitToDesktop => break 'main_thread_game_loop,
                    }
                }
            }
        }

        {
            let elapsed = updated.elapsed();
            let delay = Duration::from_millis(16).saturating_sub(elapsed);
            std::thread::sleep(delay);
        }
    }
}
