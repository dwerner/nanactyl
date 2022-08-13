use std::collections::HashMap;

use sdl2::{
    controller::GameController,
    event::{Event as SdlEvent, WindowEvent},
    haptic::Haptic,
    joystick::Joystick,
    keyboard::Keycode,
};
use state::input::{DeviceEvent, EngineEvent};
use state::log;

use crate::{GAME_CONTROLLER_SUBSYSTEM, HAPTIC_SUBSYSTEM, JOYSTICK_SUBSYSTEM};

use std::cell::RefCell;

thread_local! {
    pub(crate) static GAME_CONTROLLERS: RefCell<HashMap<u32, GameController>> = RefCell::new(HashMap::new());
    pub(crate) static JOYSTICKS: RefCell<HashMap<u32, Joystick>> = RefCell::new(HashMap::new());
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
    pub fn evaluate_event(&mut self, event: SdlEvent) -> EngineEvent {
        match event {
            SdlEvent::JoyHatMotion {
                timestamp,
                which,
                hat_idx: _,
                state,
            } => {
                log::info!("sdl hat motion {} {} {:?}", timestamp, which, state);
            }
            SdlEvent::ControllerDeviceAdded {
                timestamp: _,
                which: controller_index,
            } => {
                let game_controller = match GAME_CONTROLLER_SUBSYSTEM
                    .with(|g| g.borrow_mut().as_mut().unwrap().open(controller_index))
                {
                    Ok(game_controller) => {
                        log::info!("Added game controller {controller_index}.",);
                        game_controller
                    }
                    Err(_) => {
                        log::error!("Unable to open game_controller {controller_index}...");
                        todo!("handle this error better")
                    }
                };
                GAME_CONTROLLERS.with(|controllers| {
                    controllers
                        .borrow_mut()
                        .insert(controller_index, game_controller)
                });

                return EngineEvent::InputDevice(DeviceEvent::GameControllerAdded(
                    controller_index,
                ));
            }
            SdlEvent::ControllerDeviceRemoved {
                timestamp: _,
                which,
            } => {
                return EngineEvent::InputDevice(DeviceEvent::GameControllerRemoved(which));
            }
            SdlEvent::JoyDeviceAdded {
                timestamp: _,
                which: joy_index,
            } => {
                let joystick = match JOYSTICK_SUBSYSTEM
                    .with(|j| j.borrow_mut().as_mut().unwrap().open(joy_index))
                {
                    Ok(joystick) => {
                        let joy_power_level = joystick.power_level();
                        log::info!("Added joystick {joy_index} power level: {joy_power_level:?}",);
                        joystick
                    }
                    Err(_) => {
                        // We don't want to kill the game when we can't open a joystick.
                        log::error!("Unable to open joystick {joy_index}");
                        return EngineEvent::Continue;
                    }
                };
                JOYSTICKS.with(|j| j.borrow_mut().insert(joy_index, joystick));
                if let Ok(haptic) = HAPTIC_SUBSYSTEM.with(|h| {
                    h.borrow_mut()
                        .as_mut()
                        .unwrap()
                        .open_from_joystick_id(joy_index)
                }) {
                    HAPTIC_DEVICES.with(|h| h.borrow_mut().insert(joy_index, haptic));
                }
                return EngineEvent::InputDevice(DeviceEvent::JoystickAdded(joy_index));
            }
            SdlEvent::JoyDeviceRemoved {
                timestamp: _,
                which: joy_index,
            } => {
                log::info!("Joystick {joy_index} removed.",);
                JOYSTICKS.with(|j| j.borrow_mut().remove(&joy_index));
                HAPTIC_DEVICES.with(|h| h.borrow_mut().remove(&joy_index));
                return EngineEvent::InputDevice(DeviceEvent::JoystickRemoved(joy_index));
            }
            SdlEvent::Window {
                window_id: _,
                timestamp: _,
                win_event: WindowEvent::Resized(_width, _height),
            } => {
                //self.config.width = width as u32;
                //self.config.height = height as u32;
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
