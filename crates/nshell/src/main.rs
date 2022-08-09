use std::time::Instant;

use plugin_loader::Plugin;

use plugin_loader::PluginUpdate;
use state::log;
use state::sdl2;
use state::State;

fn main() {
    env_logger::init();
    log::info!("hello from main");
    let sdl = sdl2::init().unwrap();
    let mut input = Plugin::<State>::open_from_target_dir("input_plugin").unwrap();
    let mut state = State::new(sdl);
    let mut updated = Instant::now();
    'main_thread_game_loop: loop {
        match input.check(&mut state).unwrap() {
            PluginUpdate::Updated => log::info!("input plugin updated"),
            PluginUpdate::Unchanged => (),
        }
        let _update_duration = input.call_update(&mut state, &updated.elapsed()).unwrap();
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
    }
}
