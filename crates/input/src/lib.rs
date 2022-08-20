/// Input events
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum InputEvent {
    ButtonPressed(Button),
    ButtonReleased(Button),
}

#[derive(Debug, PartialEq, Eq, Clone)]
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

    /// Specific events, like devices being added/removed should notifiy the game loop.
    InputDevice(DeviceEvent),

    /// Input events
    Input(InputEvent),

    /// Game loop should break and we should exit.
    ExitToDesktop,
}
