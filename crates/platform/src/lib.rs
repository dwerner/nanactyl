use std::collections::HashMap;

use input::{Button, DeviceEvent, EngineEvent, InputEvent};
use sdl2::{
    controller::GameController, event::Event as SdlEvent, haptic::Haptic, keyboard::Keycode,
    sys::SDL_Window,
};

#[derive(thiserror::Error, Debug)]
pub enum PlatformError {
    #[error("sdl error {0:?}")]
    Sdl(String),
    #[error("haptic init error {0:?}")]
    HapticInit(String),
    #[error("game controller init error {0:?}")]
    GameControllerInit(String),
    #[error("event pump init error {0:?}")]
    EventPumpInit(String),
    #[error("audio init error {0:?}")]
    AudioInit(String),
    #[error("video init error {0:?}")]
    VideoInit(String),
    #[error("add window error {0:?}")]
    AddWindow(sdl2::video::WindowBuildError),
}

#[derive(Copy, Clone)]
pub struct WinPtr {
    pub raw: *const SDL_Window,
}

unsafe impl Send for WinPtr {}
unsafe impl Sync for WinPtr {}

pub struct PlatformContext {
    _todo_sdl_context: sdl2::Sdl,
    haptic_subsystem: sdl2::HapticSubsystem,
    game_controller_subsystem: sdl2::GameControllerSubsystem,
    event_pump: sdl2::EventPump,
    video_subsystem: sdl2::VideoSubsystem,
    _todo_audio_subsystem: sdl2::AudioSubsystem,

    //
    windows: Vec<sdl2::video::Window>,
    outgoing_events: Vec<EngineEvent>,
    game_controllers: HashMap<u32, GameController>,
    haptic_devices: HashMap<u32, Haptic>,
}

impl PlatformContext {
    pub fn new() -> Result<Self, PlatformError> {
        let sdl_context = sdl2::init().map_err(PlatformError::Sdl)?;
        let haptic_subsystem = sdl_context.haptic().map_err(PlatformError::HapticInit)?;
        let game_controller_subsystem = sdl_context
            .game_controller()
            .map_err(PlatformError::GameControllerInit)?;
        let event_pump = sdl_context
            .event_pump()
            .map_err(PlatformError::EventPumpInit)?;
        let audio_subsystem = sdl_context.audio().map_err(PlatformError::AudioInit)?;
        let video_subsystem = sdl_context.video().map_err(PlatformError::VideoInit)?;
        Ok(Self {
            _todo_sdl_context: sdl_context,
            _todo_audio_subsystem: audio_subsystem,

            haptic_subsystem,
            game_controller_subsystem,
            event_pump,
            video_subsystem,
            windows: Vec::new(),
            outgoing_events: Vec::with_capacity(50),
            game_controllers: HashMap::new(),
            haptic_devices: HashMap::new(),
        })
    }

    pub fn add_vulkan_window(
        &mut self,
        title: &str,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<usize, PlatformError> {
        let idx = self.windows.len();
        self.windows.push(
            self.video_subsystem
                .window(title, width, height)
                .position(x, y)
                .resizable()
                .allow_highdpi()
                .vulkan()
                .build()
                .map_err(PlatformError::AddWindow)?,
        );
        Ok(idx)
    }

    pub fn get_raw_window_handle(&self, index: usize) -> Option<WinPtr> {
        self.windows
            .get(index)
            .map(|handle| WinPtr { raw: handle.raw() })
    }

    // Pump a maximum of 50 events.
    pub fn pump_events(&mut self) {
        self.outgoing_events.clear();
        const MAX_EVENTS: usize = 50;
        let mut event_ctr = 0;
        'poll_event: while let Some(event) = self.event_pump.poll_event() {
            event_ctr += 1;
            let e = match self.evaluate_event(&event) {
                EngineEvent::Continue => {
                    continue;
                }
                e => e,
            };
            self.outgoing_events.push(e);
            if event_ctr == MAX_EVENTS {
                println!("max events reached");
                break 'poll_event;
            }
        }
    }

    pub fn peek_events(&self) -> &[EngineEvent] {
        &self.outgoing_events
    }

    // TODO: Probably we want either a Result<EngineEvent, ...> or Option<EngineEvent> here.
    /// Evaluate an event from SDL, modify state internally and surface an engine event event if it's relevant to the event loop.
    fn evaluate_event(&mut self, event: &SdlEvent) -> EngineEvent {
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
                let game_controller = match self.game_controller_subsystem.open(*controller_index) {
                    Ok(game_controller) => game_controller,
                    Err(_) => {
                        todo!("handle this error better")
                    }
                };
                self.game_controllers
                    .insert(*controller_index, game_controller);
                if let Ok(haptic) = self
                    .haptic_subsystem
                    .open_from_joystick_id(*controller_index)
                {
                    self.haptic_devices.insert(*controller_index, haptic);
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

    // TODO: evaluate this library's sound capabilities
    // fn play_sound(&self) {
    //     self.audio_subsystem
    //         .open_playback(device, spec, get_callback)
    // }
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
