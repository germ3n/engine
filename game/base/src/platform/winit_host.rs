use crate::input::{PadButton, PadButtons};
use crate::platform::event::{
    DeviceEvent, ElementState, Event, KeyCode, KeyboardInput, Modifiers, MouseButton,
    MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use crate::platform::gamepad::{GamepadState, PadDeadzones, PadPower};
use crate::platform::host::{Control, HostOps};
use crate::platform::surface::Surface;
use crate::platform::HostKind;
use gilrs::{ev::AxisOrBtn, ev::Code, Axis, Button, GamepadId, Gilrs, PowerInfo};
use std::collections::HashMap;
use winit::event::{
    DeviceEvent as WinitDeviceEvent, ElementState as WinitElementState, Event as WinitEvent,
    MouseButton as WinitMouseButton, MouseScrollDelta as WinitMouseScrollDelta,
    TouchPhase as WinitTouchPhase, WindowEvent as WinitWindowEvent,
};
#[cfg(target_os = "android")]
use winit::event_loop::EventLoopBuilder;
use winit::event_loop::{EventLoop, EventLoopWindowTarget};
use winit::keyboard::{KeyCode as WinitKeyCode, ModifiersState, PhysicalKey};
use winit::window::{CursorGrabMode, Window as WinitWindow, WindowBuilder};

#[cfg(target_os = "android")]
use std::io::Read;
#[cfg(target_os = "android")]
use std::os::fd::FromRawFd;
#[cfg(target_os = "android")]
use std::sync::Mutex;
#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;
#[cfg(target_os = "android")]
use winit::platform::android::EventLoopBuilderExtAndroid;

#[cfg(target_os = "android")]
static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);

#[cfg(target_os = "android")]
pub fn remember(app: AndroidApp) {
    *APP.lock().unwrap() = Some(app);
}

#[cfg(target_os = "android")]
pub fn redirect_stdio() {
    unsafe {
        let mut pipes = [0i32; 2];

        if libc::pipe(pipes.as_mut_ptr()) != 0 {
            return;
        }

        libc::dup2(pipes[1], libc::STDOUT_FILENO);
        libc::dup2(pipes[1], libc::STDERR_FILENO);
        libc::close(pipes[1]);
        let read_fd = pipes[0];

        std::thread::spawn(move || {
            let mut file = unsafe { std::fs::File::from_raw_fd(read_fd) };
            let mut buf = [0u8; 2048];

            loop {
                let count = match file.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                log_chunk(&buf[..count]);
            }
        });
    }
}

#[cfg(target_os = "android")]
fn log_chunk(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);

    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }

        let cleaned = line.replace('\0', "");
        let Ok(message) = std::ffi::CString::new(cleaned) else {
            continue;
        };

        unsafe {
            __android_log_write(4, c"engine".as_ptr(), message.as_ptr());
        }
    }
}

#[cfg(target_os = "android")]
unsafe extern "C" {
    fn __android_log_write(
        priority: i32,
        tag: *const libc::c_char,
        text: *const libc::c_char,
    ) -> i32;
}

pub struct WinitHost {
    event_loop: Option<EventLoop<()>>,
    window: Option<WinitWindow>,
    surface: Option<Surface>,
    pads: Option<Gilrs>,
    paddle_codes: HashMap<GamepadId, [Option<u32>; 4]>,
}

impl WinitHost {
    pub fn open() -> Result<Self, String> {
        let event_loop = build_event_loop()?;
        let pads = match Gilrs::new() {
            Ok(pads) => Some(pads),
            Err(err) => {
                println!("[pad] {err}");

                None
            }
        };

        #[cfg(target_os = "android")]
        {
            return Ok(Self {
                event_loop: Some(event_loop),
                window: None,
                surface: None,
                pads,
                paddle_codes: HashMap::new(),
            });
        }

        #[cfg(not(target_os = "android"))]
        {
            let window = WindowBuilder::new()
                .with_title("Starting...")
                .build(&event_loop)
                .map_err(|err| err.to_string())?;
            let surface = Surface::from_winit(&window)?;

            Ok(Self {
                event_loop: Some(event_loop),
                window: Some(window),
                surface: Some(surface),
                pads,
                paddle_codes: HashMap::new(),
            })
        }
    }

    pub fn run(
        mut self,
        mut on_event: impl FnMut(Event, &mut dyn HostOps, &mut dyn Control) + 'static,
    ) {
        let event_loop = self.event_loop.take().expect("event loop");

        event_loop
            .run(move |event, target| {
                if let WinitEvent::Resumed = &event {
                    if self.window.is_none() {
                        match WindowBuilder::new().with_title("Starting...").build(target) {
                            Ok(window) => match Surface::from_winit(&window) {
                                Ok(surface) => {
                                    self.window = Some(window);
                                    self.surface = Some(surface);
                                }
                                Err(err) => {
                                    println!("[host] surface {err}");
                                }
                            },
                            Err(err) => {
                                println!("[host] window {err}");
                            }
                        }
                    }
                }

                if let WinitEvent::Suspended = &event {
                    self.window = None;
                    self.surface = None;
                }

                if let WinitEvent::WindowEvent {
                    event: WinitWindowEvent::Resized(size),
                    ..
                } = &event
                {
                    if let Some(surface) = self.surface.as_mut() {
                        surface.width = size.width.max(1);
                        surface.height = size.height.max(1);
                    }

                    if let Some(window) = self.window.as_ref() {
                        if let Ok(next) = Surface::from_winit(window) {
                            self.surface = Some(next);
                        }
                    }
                }

                let mut control = WinitControl { target };
                let [first, second] = map_events(&event);

                if let Some(next) = first {
                    on_event(next, &mut self, &mut control);
                }

                if let Some(next) = second {
                    on_event(next, &mut self, &mut control);
                }
            })
            .unwrap();
    }
}

impl HostOps for WinitHost {
    fn kind(&self) -> HostKind {
        HostKind::Winit
    }

    fn surface(&self) -> Option<&Surface> {
        self.surface.as_ref()
    }

    fn set_title(&mut self, title: &str) {
        if let Some(window) = self.window.as_ref() {
            window.set_title(title);
        }
    }

    fn set_size(&mut self, w: u32, h: u32) {
        if let Some(window) = self.window.as_ref() {
            let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(w, h));
        }

        if let Some(surface) = self.surface.as_mut() {
            surface.width = w.max(1);
            surface.height = h.max(1);
        }
    }

    fn size(&self) -> (u32, u32) {
        if let Some(surface) = self.surface.as_ref() {
            return surface.size();
        }

        (1, 1)
    }

    fn set_cursor_grabbed(&mut self, grabbed: bool) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        if grabbed {
            if window.set_cursor_grab(CursorGrabMode::Locked).is_err() {
                let _ = window.set_cursor_grab(CursorGrabMode::Confined);
            }

            window.set_cursor_visible(false);

            return;
        }

        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
    }

    fn request_redraw(&mut self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn gamepad(&mut self, index: usize, deadzones: PadDeadzones) -> GamepadState {
        sample_pad(&mut self.pads, &mut self.paddle_codes, index, deadzones)
    }
}

struct WinitControl<'a> {
    target: &'a EventLoopWindowTarget<()>,
}

impl Control for WinitControl<'_> {
    fn poll(&mut self) {
        self.target
            .set_control_flow(winit::event_loop::ControlFlow::Poll);
    }

    fn exit(&mut self) {
        self.target.exit();
    }
}

fn build_event_loop() -> Result<EventLoop<()>, String> {
    #[cfg(target_os = "android")]
    {
        let app = APP.lock().unwrap().clone().expect("android app");
        let mut builder = EventLoopBuilder::new();
        builder.with_android_app(app);

        return builder.build().map_err(|err| err.to_string());
    }

    #[cfg(not(target_os = "android"))]
    {
        EventLoop::new().map_err(|err| err.to_string())
    }
}

fn map_events(event: &WinitEvent<()>) -> [Option<Event>; 2] {
    match event {
        WinitEvent::Resumed => [Some(Event::Resumed), None],
        WinitEvent::Suspended => [Some(Event::Suspended), None],
        WinitEvent::AboutToWait => [Some(Event::AboutToWait), None],
        WinitEvent::WindowEvent { event, .. } => {
            let [first, second] = map_window_events(event);

            [first.map(Event::Window), second.map(Event::Window)]
        }
        WinitEvent::DeviceEvent {
            event: WinitDeviceEvent::MouseMotion { delta },
            ..
        } => [
            Some(Event::Device(DeviceEvent::MouseMotion { delta: *delta })),
            None,
        ],
        _ => [None, None],
    }
}

fn map_window_events(event: &WinitWindowEvent) -> [Option<WindowEvent>; 2] {
    match event {
        WinitWindowEvent::CloseRequested => [Some(WindowEvent::CloseRequested), None],
        WinitWindowEvent::Resized(size) => [
            Some(WindowEvent::Resized {
                width: size.width,
                height: size.height,
            }),
            None,
        ],
        WinitWindowEvent::Focused(focused) => [Some(WindowEvent::Focused(*focused)), None],
        WinitWindowEvent::ModifiersChanged(next) => [
            Some(WindowEvent::ModifiersChanged(map_modifiers(next.state()))),
            None,
        ],
        WinitWindowEvent::CursorMoved { position, .. } => [
            Some(WindowEvent::CursorMoved {
                x: position.x,
                y: position.y,
            }),
            None,
        ],
        WinitWindowEvent::MouseWheel { delta, .. } => [
            Some(WindowEvent::MouseWheel {
                delta: map_scroll(*delta),
            }),
            None,
        ],
        WinitWindowEvent::MouseInput { state, button, .. } => [
            Some(WindowEvent::MouseInput {
                state: map_element_state(*state),
                button: map_mouse_button(*button),
            }),
            None,
        ],
        WinitWindowEvent::KeyboardInput { event, .. } => {
            let key_code = match event.physical_key {
                PhysicalKey::Code(code) => map_key_code(code),
                _ => None,
            };
            let input = WindowEvent::KeyboardInput(KeyboardInput {
                state: map_element_state(event.state),
                key_code,
                repeat: event.repeat,
            });
            let text = if event.state == WinitElementState::Pressed {
                event.text.as_ref().and_then(|text| {
                    let text: String = text.chars().filter(|ch| !ch.is_control()).collect();

                    if text.is_empty() {
                        None
                    } else {
                        Some(WindowEvent::TextInput { text })
                    }
                })
            } else {
                None
            };

            [Some(input), text]
        }
        WinitWindowEvent::Touch(touch) => [
            Some(WindowEvent::Touch(Touch {
                id: touch.id,
                phase: map_touch_phase(touch.phase),
                location: (touch.location.x, touch.location.y),
            })),
            None,
        ],
        WinitWindowEvent::RedrawRequested => [Some(WindowEvent::RedrawRequested), None],
        _ => [None, None],
    }
}

fn map_modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        control: state.control_key(),
        alt: state.alt_key(),
        super_key: state.super_key(),
    }
}

fn map_scroll(delta: WinitMouseScrollDelta) -> MouseScrollDelta {
    match delta {
        WinitMouseScrollDelta::LineDelta(x, y) => MouseScrollDelta::LineDelta(x, y),
        WinitMouseScrollDelta::PixelDelta(offset) => {
            MouseScrollDelta::PixelDelta(offset.x, offset.y)
        }
    }
}

fn map_element_state(state: WinitElementState) -> ElementState {
    match state {
        WinitElementState::Pressed => ElementState::Pressed,
        WinitElementState::Released => ElementState::Released,
    }
}

fn map_mouse_button(button: WinitMouseButton) -> MouseButton {
    match button {
        WinitMouseButton::Left => MouseButton::Left,
        WinitMouseButton::Right => MouseButton::Right,
        WinitMouseButton::Middle => MouseButton::Middle,
        WinitMouseButton::Back => MouseButton::Other(3),
        WinitMouseButton::Forward => MouseButton::Other(4),
        WinitMouseButton::Other(other) => MouseButton::Other(other),
    }
}

fn map_touch_phase(phase: WinitTouchPhase) -> TouchPhase {
    match phase {
        WinitTouchPhase::Started => TouchPhase::Started,
        WinitTouchPhase::Moved => TouchPhase::Moved,
        WinitTouchPhase::Ended => TouchPhase::Ended,
        WinitTouchPhase::Cancelled => TouchPhase::Cancelled,
    }
}

fn map_key_code(code: WinitKeyCode) -> Option<KeyCode> {
    Some(match code {
        WinitKeyCode::Escape => KeyCode::Escape,
        WinitKeyCode::Enter => KeyCode::Enter,
        WinitKeyCode::Delete => KeyCode::Delete,
        WinitKeyCode::Backspace => KeyCode::Backspace,
        WinitKeyCode::Tab => KeyCode::Tab,
        WinitKeyCode::Space => KeyCode::Space,
        WinitKeyCode::CapsLock => KeyCode::CapsLock,
        WinitKeyCode::ShiftLeft => KeyCode::ShiftLeft,
        WinitKeyCode::ShiftRight => KeyCode::ShiftRight,
        WinitKeyCode::ControlLeft => KeyCode::ControlLeft,
        WinitKeyCode::ControlRight => KeyCode::ControlRight,
        WinitKeyCode::AltLeft => KeyCode::AltLeft,
        WinitKeyCode::AltRight => KeyCode::AltRight,
        WinitKeyCode::SuperLeft => KeyCode::SuperLeft,
        WinitKeyCode::SuperRight => KeyCode::SuperRight,
        WinitKeyCode::ArrowLeft => KeyCode::ArrowLeft,
        WinitKeyCode::ArrowRight => KeyCode::ArrowRight,
        WinitKeyCode::ArrowUp => KeyCode::ArrowUp,
        WinitKeyCode::ArrowDown => KeyCode::ArrowDown,
        WinitKeyCode::Insert => KeyCode::Insert,
        WinitKeyCode::Home => KeyCode::Home,
        WinitKeyCode::End => KeyCode::End,
        WinitKeyCode::PageUp => KeyCode::PageUp,
        WinitKeyCode::PageDown => KeyCode::PageDown,
        WinitKeyCode::PrintScreen => KeyCode::PrintScreen,
        WinitKeyCode::ScrollLock => KeyCode::ScrollLock,
        WinitKeyCode::Pause => KeyCode::Pause,
        WinitKeyCode::Digit0 => KeyCode::Digit0,
        WinitKeyCode::Digit1 => KeyCode::Digit1,
        WinitKeyCode::Digit2 => KeyCode::Digit2,
        WinitKeyCode::Digit3 => KeyCode::Digit3,
        WinitKeyCode::Digit4 => KeyCode::Digit4,
        WinitKeyCode::Digit5 => KeyCode::Digit5,
        WinitKeyCode::Digit6 => KeyCode::Digit6,
        WinitKeyCode::Digit7 => KeyCode::Digit7,
        WinitKeyCode::Digit8 => KeyCode::Digit8,
        WinitKeyCode::Digit9 => KeyCode::Digit9,
        WinitKeyCode::KeyA => KeyCode::KeyA,
        WinitKeyCode::KeyB => KeyCode::KeyB,
        WinitKeyCode::KeyC => KeyCode::KeyC,
        WinitKeyCode::KeyD => KeyCode::KeyD,
        WinitKeyCode::KeyE => KeyCode::KeyE,
        WinitKeyCode::KeyF => KeyCode::KeyF,
        WinitKeyCode::KeyG => KeyCode::KeyG,
        WinitKeyCode::KeyH => KeyCode::KeyH,
        WinitKeyCode::KeyI => KeyCode::KeyI,
        WinitKeyCode::KeyJ => KeyCode::KeyJ,
        WinitKeyCode::KeyK => KeyCode::KeyK,
        WinitKeyCode::KeyL => KeyCode::KeyL,
        WinitKeyCode::KeyM => KeyCode::KeyM,
        WinitKeyCode::KeyN => KeyCode::KeyN,
        WinitKeyCode::KeyO => KeyCode::KeyO,
        WinitKeyCode::KeyP => KeyCode::KeyP,
        WinitKeyCode::KeyQ => KeyCode::KeyQ,
        WinitKeyCode::KeyR => KeyCode::KeyR,
        WinitKeyCode::KeyS => KeyCode::KeyS,
        WinitKeyCode::KeyT => KeyCode::KeyT,
        WinitKeyCode::KeyU => KeyCode::KeyU,
        WinitKeyCode::KeyV => KeyCode::KeyV,
        WinitKeyCode::KeyW => KeyCode::KeyW,
        WinitKeyCode::KeyX => KeyCode::KeyX,
        WinitKeyCode::KeyY => KeyCode::KeyY,
        WinitKeyCode::KeyZ => KeyCode::KeyZ,
        WinitKeyCode::F1 => KeyCode::F1,
        WinitKeyCode::F2 => KeyCode::F2,
        WinitKeyCode::F3 => KeyCode::F3,
        WinitKeyCode::F4 => KeyCode::F4,
        WinitKeyCode::F5 => KeyCode::F5,
        WinitKeyCode::F6 => KeyCode::F6,
        WinitKeyCode::F7 => KeyCode::F7,
        WinitKeyCode::F8 => KeyCode::F8,
        WinitKeyCode::F9 => KeyCode::F9,
        WinitKeyCode::F10 => KeyCode::F10,
        WinitKeyCode::F11 => KeyCode::F11,
        WinitKeyCode::F12 => KeyCode::F12,
        WinitKeyCode::Minus => KeyCode::Minus,
        WinitKeyCode::Equal => KeyCode::Equal,
        WinitKeyCode::BracketLeft => KeyCode::BracketLeft,
        WinitKeyCode::BracketRight => KeyCode::BracketRight,
        WinitKeyCode::Backslash => KeyCode::Backslash,
        WinitKeyCode::Semicolon => KeyCode::Semicolon,
        WinitKeyCode::Quote => KeyCode::Quote,
        WinitKeyCode::Backquote => KeyCode::Backquote,
        WinitKeyCode::Comma => KeyCode::Comma,
        WinitKeyCode::Period => KeyCode::Period,
        WinitKeyCode::Slash => KeyCode::Slash,
        WinitKeyCode::Numpad0 => KeyCode::Numpad0,
        WinitKeyCode::Numpad1 => KeyCode::Numpad1,
        WinitKeyCode::Numpad2 => KeyCode::Numpad2,
        WinitKeyCode::Numpad3 => KeyCode::Numpad3,
        WinitKeyCode::Numpad4 => KeyCode::Numpad4,
        WinitKeyCode::Numpad5 => KeyCode::Numpad5,
        WinitKeyCode::Numpad6 => KeyCode::Numpad6,
        WinitKeyCode::Numpad7 => KeyCode::Numpad7,
        WinitKeyCode::Numpad8 => KeyCode::Numpad8,
        WinitKeyCode::Numpad9 => KeyCode::Numpad9,
        WinitKeyCode::NumpadAdd => KeyCode::NumpadAdd,
        WinitKeyCode::NumpadSubtract => KeyCode::NumpadSubtract,
        WinitKeyCode::NumpadMultiply => KeyCode::NumpadMultiply,
        WinitKeyCode::NumpadDivide => KeyCode::NumpadDivide,
        WinitKeyCode::NumpadDecimal => KeyCode::NumpadDecimal,
        WinitKeyCode::NumpadEnter => KeyCode::NumpadEnter,
        WinitKeyCode::NumLock => KeyCode::NumLock,
        _ => return None,
    })
}

fn sample_pad(
    pads: &mut Option<Gilrs>,
    paddle_codes: &mut HashMap<GamepadId, [Option<u32>; 4]>,
    index: usize,
    deadzones: PadDeadzones,
) -> GamepadState {
    let Some(pads) = pads.as_mut() else {
        return GamepadState::idle();
    };

    while pads.next_event().is_some() {}

    let Some((id, pad)) = pads.gamepads().nth(index) else {
        return GamepadState::idle();
    };

    let gas = crate::platform::gamepad::pedal(pad.value(Axis::RightZ), deadzones.gas);
    let brake = crate::platform::gamepad::pedal(pad.value(Axis::LeftZ), deadzones.brake);
    let clutch = clutch_value(&pad, deadzones.clutch);
    let mut buttons = pad_buttons(&pad);
    let slots = paddle_codes.entry(id).or_insert([None; 4]);
    note_paddle_codes(slots, &pad);
    insert_paddles(&mut buttons, slots, &pad);

    if gas > 0.0 {
        buttons.insert(PadButton::PedalGas);
    }

    if brake > 0.0 {
        buttons.insert(PadButton::PedalBrake);
    }

    if clutch > 0.0 {
        buttons.insert(PadButton::PedalClutch);
    }

    GamepadState {
        forward: crate::platform::gamepad::stick(pad.value(Axis::LeftStickY), deadzones.left),
        right: crate::platform::gamepad::stick(pad.value(Axis::LeftStickX), deadzones.left),
        look_x: crate::platform::gamepad::stick(pad.value(Axis::RightStickX), deadzones.right),
        look_y: crate::platform::gamepad::stick(pad.value(Axis::RightStickY), deadzones.right),
        gas,
        brake,
        clutch,
        power: map_power(pad.power_info()),
        buttons,
    }
}

fn map_power(info: PowerInfo) -> PadPower {
    match info {
        PowerInfo::Unknown => PadPower::Unknown,
        PowerInfo::Wired => PadPower::Wired,
        PowerInfo::Discharging(level) => PadPower::Discharging(level),
        PowerInfo::Charging(level) => PadPower::Charging(level),
        PowerInfo::Charged => PadPower::Charged,
    }
}

fn clutch_value(pad: &gilrs::Gamepad, deadzone: f32) -> f32 {
    let from_c = pad
        .button_data(Button::C)
        .map(|data| data.value())
        .unwrap_or(0.0);
    let from_z = pad
        .button_data(Button::Z)
        .map(|data| data.value())
        .unwrap_or(0.0);
    let pressed = if pad.is_pressed(Button::C) || pad.is_pressed(Button::Z) {
        1.0
    } else {
        0.0
    };

    crate::platform::gamepad::pedal(from_c.max(from_z).max(pressed), deadzone)
}

fn is_paddle_candidate(pad: &gilrs::Gamepad, code: Code) -> bool {
    match pad.axis_or_btn_name(code) {
        Some(AxisOrBtn::Btn(Button::Unknown))
        | Some(AxisOrBtn::Btn(Button::C))
        | Some(AxisOrBtn::Btn(Button::Z)) => true,
        None => true,
        _ => false,
    }
}

fn note_paddle_codes(slots: &mut [Option<u32>; 4], pad: &gilrs::Gamepad) {
    let mut known: Vec<u32> = slots.iter().filter_map(|slot| *slot).collect();

    for (code, _) in pad.state().buttons() {
        if !is_paddle_candidate(pad, code) {
            continue;
        }

        let value = code.into_u32();

        if known.contains(&value) {
            continue;
        }

        known.push(value);
    }

    if pad.is_pressed(Button::C) {
        if let Some(code) = pad.button_code(Button::C).or_else(|| Button::C.to_nec()) {
            let value = code.into_u32();

            if !known.contains(&value) {
                known.push(value);
            }
        }
    }

    if pad.is_pressed(Button::Z) {
        if let Some(code) = pad.button_code(Button::Z).or_else(|| Button::Z.to_nec()) {
            let value = code.into_u32();

            if !known.contains(&value) {
                known.push(value);
            }
        }
    }

    known.sort_unstable();
    known.dedup();

    for idx in 0..4 {
        slots[idx] = known.get(idx).copied();
    }
}

fn insert_paddles(buttons: &mut PadButtons, slots: &[Option<u32>; 4], pad: &gilrs::Gamepad) {
    const PADDLES: [PadButton; 4] = [
        PadButton::Paddle1,
        PadButton::Paddle2,
        PadButton::Paddle3,
        PadButton::Paddle4,
    ];

    for (idx, slot) in slots.iter().enumerate() {
        let Some(expected) = *slot else {
            continue;
        };

        let pressed = pad.state().buttons().any(|(code, data)| {
            data.is_pressed() && code.into_u32() == expected
        });

        if pressed {
            buttons.insert(PADDLES[idx]);
        }
    }

    if pad.is_pressed(Button::C) && slots.iter().all(|slot| slot.is_none()) {
        buttons.insert(PadButton::Paddle1);
    }

    if pad.is_pressed(Button::Z) && slots.iter().all(|slot| slot.is_none()) {
        buttons.insert(PadButton::Paddle2);
    }
}

fn pad_buttons(pad: &gilrs::Gamepad) -> PadButtons {
    let mut buttons = PadButtons::NONE;
    const PAIRS: [(Button, PadButton); 16] = [
        (Button::South, PadButton::South),
        (Button::East, PadButton::East),
        (Button::West, PadButton::West),
        (Button::North, PadButton::North),
        (Button::LeftTrigger, PadButton::LeftTrigger),
        (Button::LeftTrigger2, PadButton::LeftTrigger2),
        (Button::RightTrigger, PadButton::RightTrigger),
        (Button::RightTrigger2, PadButton::RightTrigger2),
        (Button::LeftThumb, PadButton::LeftThumb),
        (Button::RightThumb, PadButton::RightThumb),
        (Button::Select, PadButton::Select),
        (Button::Start, PadButton::Start),
        (Button::DPadUp, PadButton::DPadUp),
        (Button::DPadDown, PadButton::DPadDown),
        (Button::DPadLeft, PadButton::DPadLeft),
        (Button::DPadRight, PadButton::DPadRight),
    ];

    for (source, mapped) in PAIRS {
        if pad.is_pressed(source) {
            buttons.insert(mapped);
        }
    }

    buttons
}
