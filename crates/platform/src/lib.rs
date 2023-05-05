use std::collections::HashMap;
use std::fmt::Debug;
use std::path::Path;

use image::GenericImageView;
use input::{Button, DeviceEvent, EngineEvent, InputEvent};
use logger::{info, Logger};
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
use sdl2::controller::GameController;
use sdl2::event::Event as SdlEvent;
use sdl2::haptic::Haptic;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{Texture, TextureCreator};
use sdl2::surface::Surface;
use sdl2::video::Window;

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
    pub raw_window_handle: RawWindowHandle,
}

impl Debug for WinPtr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WinPtr")
            .field("raw", &self.raw_window_handle)
            .finish()
    }
}

unsafe impl Send for WinPtr {}
unsafe impl Sync for WinPtr {}

unsafe impl HasRawWindowHandle for WinPtr {
    fn raw_window_handle(&self) -> RawWindowHandle {
        self.raw_window_handle
    }
}

pub struct PlatformContext {
    _sdl_context: sdl2::Sdl,
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
    logger: Logger,
}

impl PlatformContext {
    pub fn new(logger: &Logger) -> Result<Self, PlatformError> {
        let logger = logger.sub("platform");
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
            _sdl_context: sdl_context,
            _todo_audio_subsystem: audio_subsystem,

            haptic_subsystem,
            game_controller_subsystem,
            event_pump,
            video_subsystem,
            windows: Vec::new(),
            outgoing_events: Vec::with_capacity(50),
            game_controllers: HashMap::new(),
            haptic_devices: HashMap::new(),
            logger,
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
        let mut window = self
            .video_subsystem
            .window(title, width, height)
            .position(x, y)
            .resizable()
            .allow_highdpi()
            .vulkan()
            .build()
            .map_err(PlatformError::AddWindow)?;

        let icon = Self::load_png_image_as_surface("assets/icon.png").unwrap();
        window.set_icon(&icon);
        self.windows.push(window);
        Ok(idx)
    }

    fn load_png_image_as_surface<'a, P: AsRef<Path>>(path: P) -> Result<Surface<'a>, String> {
        let img = image::open(path).map_err(|e| e.to_string())?;
        let (width, height) = img.dimensions();
        let mut img_data = img.to_rgba8().into_raw();
        let temp_surface = Surface::from_data(
            &mut img_data,
            width,
            height,
            (width * 4) as u32, // 4 bytes per pixel
            PixelFormatEnum::RGBA32,
        )
        .map_err(|e| e.to_string())?;

        let mut surface =
            Surface::new(width, height, PixelFormatEnum::RGBA32).map_err(|e| e.to_string())?;
        temp_surface.blit(None, &mut surface, None)?;

        Ok(surface)
    }

    pub fn get_raw_window_handle(&self, index: usize) -> Option<WinPtr> {
        self.windows.get(index).map(|w| {
            let raw_window_handle = w.raw_window_handle();
            WinPtr { raw_window_handle }
        })
    }

    // Pump a maximum of 50 events.
    pub fn pump_events(&mut self) {
        let logger = self.logger.sub("pump_events");
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
                info!(logger, "max events reached");
                break 'poll_event;
            }
        }
    }

    pub fn peek_events(&self) -> &[EngineEvent] {
        &self.outgoing_events
    }

    // TODO: Probably we want either a Result<EngineEvent, ...> or
    /// Evaluate an event from SDL, modify state internally and then raise an
    /// engine event event if it's relevant to the event loop.
    fn evaluate_event(&mut self, event: &SdlEvent) -> EngineEvent {
        match event {
            SdlEvent::Quit { .. }
            | SdlEvent::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => return EngineEvent::ExitToDesktop,
            SdlEvent::KeyDown {
                keycode: Some(key), ..
            } => {
                return EngineEvent::Input(InputEvent::KeyPressed(keycode_to_button(*key)));
            }
            SdlEvent::KeyUp {
                keycode: Some(key), ..
            } => {
                return EngineEvent::Input(InputEvent::KeyReleased(keycode_to_button(*key)));
            }
            SdlEvent::ControllerButtonDown {
                timestamp: _,
                which,
                button,
            } => {
                return EngineEvent::Input(InputEvent::ButtonPressed(
                    *which as u8,
                    button_to_button(*button),
                ))
            }
            SdlEvent::ControllerButtonUp {
                timestamp: _,
                which,
                button,
            } => {
                return EngineEvent::Input(InputEvent::ButtonReleased(
                    *which as u8,
                    button_to_button(*button),
                ))
            }
            SdlEvent::ControllerAxisMotion {
                timestamp: _,
                which,
                axis,
                value,
            } => {
                return EngineEvent::Input(InputEvent::AxisMotion(
                    *which as u8,
                    *axis as u8,
                    (*value >> 8) as i8,
                ))
            }
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

fn keycode_to_button(button: sdl2::keyboard::Keycode) -> Button {
    match button {
        sdl2::keyboard::Keycode::Space => Button::Ok,
        sdl2::keyboard::Keycode::C => Button::Cancel,
        sdl2::keyboard::Keycode::Up => Button::Up,
        sdl2::keyboard::Keycode::Down => Button::Down,
        sdl2::keyboard::Keycode::Left => Button::Left,
        sdl2::keyboard::Keycode::Right => Button::Right,
        _ => Button::Unmapped,
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
