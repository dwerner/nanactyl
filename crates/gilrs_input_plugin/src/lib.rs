mod input_system;

use std::time::Duration;

use input_system::InputSystem;
use state::{
    futures_lite::future,
    input::{EngineEvent, InputEventSource},
    State,
};

thread_local! {}

struct InputWrapper {
    outgoing_events: Vec<EngineEvent>,
    //input_system: InputSystem,
}

impl InputEventSource for InputWrapper {
    fn update(&mut self) {
        //self.input_system.listener.
    }
    fn events(&self) -> &[EngineEvent] {
        &self.outgoing_events
    }
}

#[no_mangle]
pub extern "C" fn load(state: &mut State) {
    state::writeln!(
        state,
        "loaded stick input system - thread id({:?})",
        std::thread::current().id()
    );

    future::block_on(state.exec.spawn(async move {
        println!(
            "hello from thread {:?}, spawned from module load event",
            std::thread::current().id()
        );
    }))
    .unwrap();

    let wrapper = InputWrapper {
        outgoing_events: Vec::new(),
        //input_system: InputSystem::new().unwrap(),
    };
    state.input_system = Some(Box::new(wrapper))
}

#[no_mangle]
pub extern "C" fn update(state: &mut State, _dt: &Duration) {
    if let Some(ref mut input) = state.input_system {
        input.update();
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut State) {
    state::writeln!(state, "unloading stick input plugin");
    drop(state.input_system.take());
}
