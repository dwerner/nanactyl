mod input_system;

use std::time::Duration;

use input_system::InputSystem;
use state::{
    input::{EngineEvent, InputEventSource},
    State,
};

use std::cell::RefCell;

thread_local! {
    pub static SDL_CONTEXT: RefCell<Option<sdl2::Sdl>> = RefCell::new(None);
    static HAPTIC_SUBSYSTEM: RefCell<Option<sdl2::HapticSubsystem>> = RefCell::new(None);
    static GAME_CONTROLLER_SUBSYSTEM: RefCell<Option<sdl2::GameControllerSubsystem>> = RefCell::new(None);
    static JOYSTICK_SUBSYSTEM: RefCell<Option<sdl2::JoystickSubsystem>> = RefCell::new(None);
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
            let to_publish = input_system.evaluate_event(event);
            self.outgoing_events.push(to_publish);
        }
    }
    fn events(&self) -> &[EngineEvent] {
        &self.outgoing_events
    }
}

#[no_mangle]
pub extern "C" fn load(state: &mut State) {
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
        JOYSTICK_SUBSYSTEM.with(|j| {
            let joystick = match j.borrow_mut().take() {
                Some(thing) => thing,
                None => sdl_context.joystick().unwrap(),
            };
            *j.borrow_mut() = Some(joystick);
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

    state::writeln!(
        state,
        "loaded input system - thread id({:?}) - lots of thread_locals here!",
        std::thread::current().id()
    );
}

#[no_mangle]
pub extern "C" fn update(state: &mut State, _dt: &Duration) {
    if let Some(ref mut input) = state.input_system {
        input.update();
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut State) {
    state::writeln!(state, "unloading input plugin");
    drop((
        input_system::JOYSTICKS,
        input_system::GAME_CONTROLLERS,
        input_system::HAPTIC_DEVICES,
        HAPTIC_SUBSYSTEM,
        GAME_CONTROLLER_SUBSYSTEM,
        JOYSTICK_SUBSYSTEM,
        EVENT_PUMP,
        SDL_CONTEXT,
    ));
}
