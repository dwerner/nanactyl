mod input_system;

use std::time::Duration;

use input_system::InputSystem;
use state::log;
use state::{
    input::{EngineEvent, InputEventSource},
    State,
};

struct InputWrapper {
    outgoing_events: Vec<EngineEvent>,
    input_system: InputSystem,
}

impl InputEventSource for InputWrapper {
    fn update(&mut self) {
        let input_system = &mut self.input_system;
        self.outgoing_events.clear();
        while let Some(event) = input_system.event_pump.poll_event() {
            let to_publish = input_system.evaluate_event(event);
            self.outgoing_events.push(to_publish);
        }
    }
    fn events(&mut self) -> &[EngineEvent] {
        &self.outgoing_events
    }
}

#[no_mangle]
pub extern "C" fn load(state: &mut State) {
    state::writeln!(state, "loaded input system {}", 42);

    env_logger::init();

    let haptic_subsystem = state.sdl_context.haptic().unwrap();
    let game_controller_subsystem = state.sdl_context.game_controller().unwrap();
    let joystick_subsystem = state.sdl_context.joystick().unwrap();
    let event_pump = state.sdl_context.event_pump().unwrap();
    let input_system = input_system::InputSystem::new(
        joystick_subsystem,
        game_controller_subsystem,
        haptic_subsystem,
        event_pump,
    )
    .unwrap();
    let wrapper = InputWrapper {
        outgoing_events: Vec::new(),
        input_system,
    };
    state.input_system = Some(Box::new(wrapper))
}

#[no_mangle]
pub extern "C" fn update(state: &mut State, _dt: &Duration) {
    log::debug!("updating input plugin");
    if let Some(ref mut input) = state.input_system {
        input.update();
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut State) {
    log::info!("unloading input plugin");
    drop(state.input_system.take());
}
