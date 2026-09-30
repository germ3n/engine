#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Escape,
    Enter,
    Delete,
    Backspace,
    Tab,
    Space,
    CapsLock,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    SuperLeft,
    SuperRight,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    PrintScreen,
    ScrollLock,
    Pause,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Backquote,
    Comma,
    Period,
    Slash,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadDecimal,
    NumpadEnter,
    NumLock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementState {
    Pressed,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

#[derive(Clone, Copy, Debug)]
pub struct Touch {
    pub id: u64,
    pub phase: TouchPhase,
    pub location: (f64, f64),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl Modifiers {
    pub fn shift_key(self) -> bool {
        self.shift
    }

    pub fn control_key(self) -> bool {
        self.control
    }

    pub fn super_key(self) -> bool {
        self.super_key
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MouseScrollDelta {
    LineDelta(f32, f32),
    PixelDelta(f64, f64),
}

#[derive(Clone, Copy, Debug)]
pub struct KeyboardInput {
    pub state: ElementState,
    pub key_code: Option<KeyCode>,
    pub repeat: bool,
}

#[derive(Clone, Debug)]
pub enum WindowEvent {
    CloseRequested,
    Resized {
        width: u32,
        height: u32,
    },
    Focused(bool),
    ModifiersChanged(Modifiers),
    CursorMoved {
        x: f64,
        y: f64,
    },
    MouseWheel {
        delta: MouseScrollDelta,
    },
    MouseInput {
        state: ElementState,
        button: MouseButton,
    },
    KeyboardInput(KeyboardInput),
    TextInput {
        text: String,
    },
    Touch(Touch),
    RedrawRequested,
}

#[derive(Clone, Debug)]
pub enum DeviceEvent {
    MouseMotion { delta: (f64, f64) },
}

#[derive(Clone, Debug)]
pub enum Event {
    Resumed,
    Suspended,
    Window(WindowEvent),
    Device(DeviceEvent),
    AboutToWait,
}
