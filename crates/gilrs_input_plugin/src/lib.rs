use std::time::Duration;

use state::{
    input::{EngineEvent, InputEventSource},
    InputState,
};

struct InputWrapper {
    outgoing_events: Vec<EngineEvent>,
}

impl InputEventSource for InputWrapper {
    fn update(&mut self) {}
    fn events(&self) -> &[EngineEvent] {
        &self.outgoing_events
    }
}

#[no_mangle]
pub extern "C" fn load(state: &mut InputState) {
    state::writeln!(state, "loaded stick input system and stuff");

    state.spawn_with_shutdown(|shutdown| {
        Box::pin(async move {
            let mut ctr = 0;
            loop {
                ctr += 1;
                println!(
                    "{} long-lived task fired by ({:?})",
                    ctr,
                    std::thread::current().id()
                );
                smol::Timer::after(Duration::from_millis(250)).await;
                if shutdown.should_exit() {
                    break;
                }
            }
        })
    });

    let wrapper = InputWrapper {
        outgoing_events: Vec::new(),
    };
    state.input_system = Some(Box::new(wrapper))
}

#[no_mangle]
pub extern "C" fn update(state: &mut InputState, _dt: &Duration) {
    if let Some(ref mut input) = state.input_system {
        input.update();
    }
}

#[no_mangle]
pub extern "C" fn unload(state: &mut InputState) {
    state::writeln!(state, "unloading gilrs input plugin");
    state.block_and_kill_tasks();
    drop(state.input_system.take());
}
