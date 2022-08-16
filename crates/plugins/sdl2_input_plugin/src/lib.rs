mod input_system;

use std::time::Duration;

use input::{
    input::{EngineEvent, InputEventSource},
    InputState,
};
use input_system::InputSystem;

use std::cell::RefCell;

thread_local! {
    static SDL_CONTEXT: RefCell<Option<sdl2::Sdl>> = RefCell::new(None);
    static HAPTIC_SUBSYSTEM: RefCell<Option<sdl2::HapticSubsystem>> = RefCell::new(None);
    static GAME_CONTROLLER_SUBSYSTEM: RefCell<Option<sdl2::GameControllerSubsystem>> = RefCell::new(None);
    static EVENT_PUMP: RefCell<Option<sdl2::EventPump>> = RefCell::new(None);
}

struct InputWrapper {
    outgoing_events: Vec<EngineEvent>,
    input_system: InputSystem,
}

impl InputEventSource for InputWrapper {
    fn update(&mut self) {
        let input_system = &mut self.input_system;
        self.outgoing_events.clear();
        while let Some(event) =
            EVENT_PUMP.with(|pump| pump.borrow_mut().as_mut().unwrap().poll_event())
        {
            let e = match input_system.evaluate_event(&event) {
                EngineEvent::Continue => {
                    //println!("unmapped event {:?}", event);
                    continue;
                }
                e => e,
            };
            self.outgoing_events.push(e);
        }
    }
    fn events(&self) -> &[EngineEvent] {
        &self.outgoing_events
    }
}

#[no_mangle]
pub extern "C" fn load(state: &mut InputState) {
    SDL_CONTEXT.with(|s| {
        let sdl_context = match s.borrow_mut().take() {
            Some(sdl) => sdl,
            None => sdl2::init().unwrap(),
        };
        HAPTIC_SUBSYSTEM.with(|h| {
            let haptic = match h.borrow_mut().take() {
                Some(thing) => thing,
                None => sdl_context.haptic().unwrap(),
            };
            *h.borrow_mut() = Some(haptic);
        });
        GAME_CONTROLLER_SUBSYSTEM.with(|g| {
            let controller = match g.borrow_mut().take() {
                Some(thing) => thing,
                None => sdl_context.game_controller().unwrap(),
            };
            *g.borrow_mut() = Some(controller)
        });
        EVENT_PUMP.with(|e| {
            let event_pump = match e.borrow_mut().take() {
                Some(thing) => thing,
                None => sdl_context.event_pump().unwrap(),
            };
            *e.borrow_mut() = Some(event_pump);
        });
        *s.borrow_mut() = Some(sdl_context);
    });

    let wrapper = InputWrapper {
        outgoing_events: Vec::new(),
        input_system: InputSystem::new(),
    };
    state.input_system = Some(Box::new(wrapper));

    input::writeln!(
        state,
        "loaded input system - thread id({:?}) - lots of thread_locals here!...",
        std::thread::current().id()
    );
}

#[no_mangle]
pub extern "C" fn update(state: &mut InputState, _dt: &Duration) {
    if let Some(ref mut input) = state.input_system {
        input.update();
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut InputState) {
    input::writeln!(state, "unloading input plugin...");
    input_system::GAME_CONTROLLERS.with(|j| j.borrow_mut().clear());
    input_system::HAPTIC_DEVICES.with(|j| j.borrow_mut().clear());
    HAPTIC_SUBSYSTEM.with(|x| x.borrow_mut().take());
    GAME_CONTROLLER_SUBSYSTEM.with(|x| x.borrow_mut().take());
    EVENT_PUMP.with(|x| x.borrow_mut().take());
    SDL_CONTEXT.with(|x| x.borrow_mut().take());
    // Required! Will live beyond the lifetime of the plugin then.
    drop(state.input_system.take());
}
