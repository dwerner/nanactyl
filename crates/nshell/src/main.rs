use std::time::Duration;
use std::time::Instant;

use core_executor::CoreExecutor;
use futures_lite::future;
use plugin_loader::Plugin;

use plugin_loader::PluginCheck;
use plugin_loader::PluginError;

use state::log;
use state::State;

#[smol_potat::main(threads = 4)]
async fn main() {
    plugin_loader::register_tls_dtor_hook!();
    state::init_logger();

    log::info!("main thread startup");

    let mut input = Plugin::<State>::open_from_target_dir("gilrs_input_plugin").unwrap();

    let mut updated = Instant::now();

    // For now it's just the main thread and a secondary, but should be a pool at some point.
    let (mut main_thread_spawner, _main_thread_executor) = CoreExecutor::new();
    let (thread_spawner, _thread_executor) = CoreExecutor::new();

    let mut state = State::new(thread_spawner);
    future::block_on(main_thread_spawner.spawn(async move {
        'main_game_loop: loop {
            match input.check(&mut state) {
                Ok(PluginCheck::FoundNewVersion) => log::info!("input plugin updated"),
                Ok(PluginCheck::Unchanged) => (),
                Err(m @ PluginError::MetadataIo(_)) => {
                    log::warn!(
                        "error getting file metadata for plugin {}: {:?}",
                        input.name(),
                        m
                    );
                }
                Err(o @ PluginError::ErrorOnOpen(_)) => {
                    log::warn!("error opening plugin: {:?}", o);
                }
                Err(err) => panic!("unexpected error checking plugin - {:?}", err),
            }

            let _elapsed = input.call_update(&mut state, &updated.elapsed()).await;

            updated = Instant::now();

            if let Some(events) = state
                .input_system
                .as_ref()
                .map(|input| input.events().to_vec())
            {
                if !events.is_empty() {
                    state::writeln!(state, "Processing {} events", events.len());
                    for event in events {
                        match event {
                            state::input::EngineEvent::Continue => (),
                            state::input::EngineEvent::InputDevice(input_device_event) => {
                                state::writeln!(
                                    state,
                                    "input device event {:?}",
                                    input_device_event
                                )
                            }
                            state::input::EngineEvent::Input(input_event) => {
                                state::writeln!(state, "input event {:?}", input_event)
                            }
                            state::input::EngineEvent::ExitToDesktop => break 'main_game_loop,
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
    }))
    .unwrap();
}
