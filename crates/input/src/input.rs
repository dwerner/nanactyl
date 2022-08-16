// This trait needs to exist so we can box an "InputEventSource" plugin and let it call itself.
pub trait InputEventSource: Send + Sync {
    fn events(&self) -> &[EngineEvent];
    fn update(&mut self);
}

/// Input events
#[derive(Debug, Clone, PartialEq)]
pub enum Button {
    Left,
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

#[derive(PartialEq, Debug, Clone)]
pub enum InputEvent {
    ButtonPressed(Button),
    ButtonReleased(Button),
}

#[derive(Debug, PartialEq, Clone)]
pub enum DeviceEvent {
    JoystickAdded(u32),
    JoystickRemoved(u32),
    GameControllerAdded(u32),
    GameControllerRemoved(u32),
}

/// Control flow for the game loop
#[derive(PartialEq, Debug, Clone)]
pub enum EngineEvent {
    /// Continue execution of the game loop.
    Continue,

    /// Specific events, like devices being added/removed should notifiy the game loop.
    InputDevice(DeviceEvent),

    /// Input events
    Input(InputEvent),

    /// Game loop should break and we should exit.
    ExitToDesktop,
}
