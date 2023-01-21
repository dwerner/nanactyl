/// Input events
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Button {
    Left = 0,
    Up,
    LeftUp,
    RightUp,
    Down,
    LeftDown,
    RightDown,
    Right,
    Ok,
    Cancel,
    Unmapped,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum InputEvent {
    KeyPressed(Button),
    KeyReleased(Button),
    ButtonPressed(u8, Button),
    ButtonReleased(u8, Button),
    AxisMotion(u8, u8, i8),
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum DeviceEvent {
    JoystickAdded(u32),
    JoystickRemoved(u32),
    GameControllerAdded(u32),
    GameControllerRemoved(u32),
}

/// Control flow for the game loop
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum EngineEvent {
    /// Continue execution of the game loop.
    Continue,

    /// Specific events, like devices being added/removed should notifiy the
    /// game loop.
    InputDevice(DeviceEvent),

    /// Input events
    Input(InputEvent),

    /// Game loop should break and we should exit.
    ExitToDesktop,
}

pub mod wire {
    use bitvec::view::BitView;
    use bytemuck::{Pod, Zeroable};

    use super::*;

    #[derive(Debug, Default, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct Axis {
        value: i8,
    }

    /// Wire representation of a controller with axes and buttons. Scaled down
    /// data types are used for compact representation.
    #[derive(Debug, Default, Copy, Clone, Pod, Zeroable)]
    #[repr(C)]
    pub struct InputState {
        id: u8,
        axes: [Axis; 7],
        // bitvec
        buttons: u16,
    }

    impl InputState {
        /// Create a new `InputState`.
        pub fn new(id: u8) -> Self {
            Self {
                id,
                axes: [Axis { value: 0 }; 7],
                buttons: 0b0000000000000000,
            }
        }

        /// Update the state from a given `InputEvent`.
        pub fn update_from_event(&mut self, event: &InputEvent) {
            match event {
                InputEvent::ButtonPressed(id, button) if self.id == *id => {
                    self.set_button_bit(*button as u8, true);
                }
                InputEvent::ButtonReleased(id, button) if self.id == *id => {
                    self.set_button_bit(*button as u8, false);
                }
                InputEvent::AxisMotion(id, axis, value) if self.id == *id => {
                    self.axes[*axis as usize].value = *value;
                }

                // FOR NOW: we send keypresses as button presses for the equivalent controls
                InputEvent::KeyPressed(key) => {
                    self.set_button_bit(*key as u8, true);
                }
                InputEvent::KeyReleased(key) => {
                    self.set_button_bit(*key as u8, false);
                }
                _ => {}
            }
        }

        /// Set a bit representing the state of a button.
        fn set_button_bit(&mut self, button: u8, value: bool) {
            let buttons = self.buttons.view_bits_mut::<bitvec::prelude::Lsb0>();
            buttons.set(button as usize, value);
        }

        /// Read if a button is down from the state bits.
        pub fn is_button_down(&self, button: Button) -> bool {
            let buttons = self.buttons.view_bits::<bitvec::prelude::Lsb0>();
            match buttons.get(button as usize) {
                Some(val) => *val,
                None => false,
            }
        }
    }
}
