use std::collections::HashMap;

use state::input::{DeviceEvent, EngineEvent};
use state::log;
use state::sdl2::{
    controller::GameController,
    event::{Event as SdlEvent, WindowEvent},
    haptic::Haptic,
    joystick::Joystick,
    keyboard::Keycode,
    EventPump, GameControllerSubsystem, HapticSubsystem, JoystickSubsystem,
};

pub struct InputSystem {
    joystick_subsystem: JoystickSubsystem,
    game_controller_subsystem: GameControllerSubsystem,
    haptic_subsystem: HapticSubsystem,
    pub game_controllers: HashMap<u32, GameController>,
    pub joysticks: HashMap<u32, Joystick>,
    pub haptic_devices: HashMap<u32, Haptic>,
    pub event_pump: EventPump,
}

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
    pub fn new(
        joystick_subsystem: JoystickSubsystem,
        game_controller_subsystem: GameControllerSubsystem,
        haptic_subsystem: HapticSubsystem,
        event_pump: EventPump,
    ) -> Result<Self, Error> {
        let joysticks = HashMap::new();
        let game_controllers = HashMap::new();
        let haptic_devices = HashMap::new();
        Ok(Self {
            joystick_subsystem,
            game_controller_subsystem,
            haptic_subsystem,
            game_controllers,
            joysticks,
            haptic_devices,
            event_pump,
        })
    }

    /// Evaluate an event from SDL, modify state internally and surface an engine event event if it's relevant to the event loop.
    pub fn evaluate_event(&mut self, event: SdlEvent) -> EngineEvent {
        log::info!("game event {:?}", event);
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
                let game_controller = match self.game_controller_subsystem.open(controller_index) {
                    Ok(game_controller) => {
                        log::info!("Added game controller {controller_index}",);
                        game_controller
                    }
                    Err(_) => {
                        log::error!("Unable to open game_controller {controller_index}");
                        todo!("handle this error better")
                    }
                };
                self.game_controllers
                    .insert(controller_index, game_controller);
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
                let joystick = match self.joystick_subsystem.open(joy_index) {
                    Ok(joystick) => {
                        let joy_power_level = joystick.power_level();
                        log::info!("Added joystick {joy_index} power level: {joy_power_level:?}",);
                        joystick
                    }
                    Err(_) => {
                        // We don't want to kill the game when we can't open a joystick.
                        log::error!("Unable to open joystick {joy_index}");
                        todo!("handle this error better")
                    }
                };
                self.joysticks.insert(joy_index, joystick);
                if let Ok(haptic) = self.haptic_subsystem.open_from_joystick_id(joy_index) {
                    self.haptic_devices.insert(joy_index, haptic);
                }
                return EngineEvent::InputDevice(DeviceEvent::JoystickAdded(joy_index));
            }
            SdlEvent::JoyDeviceRemoved {
                timestamp: _,
                which: joy_index,
            } => {
                log::info!("Joystick {joy_index} removed",);
                self.joysticks.remove(&joy_index);
                self.haptic_devices.remove(&joy_index);
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
