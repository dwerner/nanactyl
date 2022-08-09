pub mod input;

pub use log;
pub use sdl2;

use sdl2::Sdl;

pub struct State {
    pub input_system: Option<Box<dyn input::InputEventSource>>,
    pub sdl_context: Sdl,
}

impl State {
    pub fn new(sdl_context: Sdl) -> Self {
        Self {
            sdl_context,
            input_system: Default::default(),
        }
    }
}
