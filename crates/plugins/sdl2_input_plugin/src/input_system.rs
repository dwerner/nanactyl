use std::cell::RefCell;
use std::collections::HashMap;

use sdl2::{
    controller::GameController, event::Event as SdlEvent, haptic::Haptic, keyboard::Keycode,
};

use crate::{GAME_CONTROLLER_SUBSYSTEM, HAPTIC_SUBSYSTEM};
use input::input::{Button, DeviceEvent, EngineEvent, InputEvent};

thread_local! {
    pub(crate) static GAME_CONTROLLERS: RefCell<HashMap<u32, GameController>> = RefCell::new(HashMap::new());
    pub(crate) static HAPTIC_DEVICES: RefCell<HashMap<u32, Haptic>> = RefCell::new(HashMap::new());
}

pub struct InputSystem {}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("SDL Error: {0}")]
    SdlStringError(String),
}

impl From<String> for Error {
    fn from(err_string: String) -> Self {
        Error::SdlStringError(err_string)
    }
}

impl InputSystem {
    #[must_use]
    pub fn new() -> Self {
        Self {}
    }

    // TODO: Probably we want either a Result<EngineEvent, ...> or Option<EngineEvent> here.
    /// Evaluate an event from SDL, modify state internally and surface an engine event event if it's relevant to the event loop.
    pub fn evaluate_event(&mut self, event: &SdlEvent) -> EngineEvent {
        match event {
            SdlEvent::ControllerButtonDown {
                timestamp: _,
                which: _,
                button,
            } => return EngineEvent::Input(InputEvent::ButtonPressed(button_to_button(*button))),
            SdlEvent::ControllerButtonUp {
                timestamp: _,
                which: _,
                button,
            } => return EngineEvent::Input(InputEvent::ButtonReleased(button_to_button(*button))),
            SdlEvent::ControllerDeviceAdded {
                timestamp: _,
                which: controller_index,
            } => {
                let game_controller = match GAME_CONTROLLER_SUBSYSTEM
                    .with(|g| g.borrow_mut().as_mut().unwrap().open(*controller_index))
                {
                    Ok(game_controller) => {
                        log::info!("Added game controller {controller_index}");
                        game_controller
                    }
                    Err(_) => {
                        log::error!("Unable to open game_controller {controller_index}");
                        todo!("handle this error better")
                    }
                };
                GAME_CONTROLLERS.with(|controllers| {
                    controllers
                        .borrow_mut()
                        .insert(*controller_index, game_controller)
                });
                if let Ok(haptic) = HAPTIC_SUBSYSTEM.with(|h| {
                    h.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .open_from_joystick_id(*controller_index)
                }) {
                    HAPTIC_DEVICES.with(|h| h.borrow_mut().insert(*controller_index, haptic));
                }

                return EngineEvent::InputDevice(DeviceEvent::GameControllerAdded(
                    *controller_index,
                ));
            }
            SdlEvent::ControllerDeviceRemoved {
                timestamp: _,
                which,
            } => {
                return EngineEvent::InputDevice(DeviceEvent::GameControllerRemoved(*which));
            }
            SdlEvent::Quit { .. }
            | SdlEvent::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => return EngineEvent::ExitToDesktop,
            _ => {}
        }
        EngineEvent::Continue
    }
}

fn button_to_button(button: sdl2::controller::Button) -> Button {
    match button {
        sdl2::controller::Button::A => Button::Ok,
        sdl2::controller::Button::B => Button::Cancel,
        sdl2::controller::Button::DPadUp => Button::Up,
        sdl2::controller::Button::DPadDown => Button::Down,
        sdl2::controller::Button::DPadLeft => Button::Left,
        sdl2::controller::Button::DPadRight => Button::Right,

        // as yet unmapped
        sdl2::controller::Button::X => Button::Unmapped,
        sdl2::controller::Button::Y => Button::Unmapped,
        sdl2::controller::Button::Back => Button::Unmapped,
        sdl2::controller::Button::Guide => Button::Unmapped,
        sdl2::controller::Button::Start => Button::Unmapped,
        sdl2::controller::Button::LeftStick => Button::Unmapped,
        sdl2::controller::Button::RightStick => Button::Unmapped,
        sdl2::controller::Button::LeftShoulder => Button::Unmapped,
        sdl2::controller::Button::RightShoulder => Button::Unmapped,
        sdl2::controller::Button::Misc1 => Button::Unmapped,
        sdl2::controller::Button::Paddle1 => Button::Unmapped,
        sdl2::controller::Button::Paddle2 => Button::Unmapped,
        sdl2::controller::Button::Paddle3 => Button::Unmapped,
        sdl2::controller::Button::Paddle4 => Button::Unmapped,
        sdl2::controller::Button::Touchpad => Button::Unmapped,
    }
}
