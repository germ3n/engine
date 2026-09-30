#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Escape,
    Delete,
    Backspace,
    Tab,
    Space,
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
    Digit1,
    Digit2,
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyG,
    KeyQ,
    KeyS,
    KeyT,
    KeyV,
    KeyW,
    BracketLeft,
    BracketRight,
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
