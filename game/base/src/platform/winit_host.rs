use crate::platform::event::{
    DeviceEvent, ElementState, Event, KeyCode, KeyboardInput, Modifiers, MouseButton,
    MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use crate::platform::host::{Control, HostOps};
use crate::platform::surface::Surface;
use crate::platform::HostKind;
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
}

impl WinitHost {
    pub fn open() -> Result<Self, String> {
        let event_loop = build_event_loop()?;

        #[cfg(target_os = "android")]
        {
            return Ok(Self {
                event_loop: Some(event_loop),
                window: None,
                surface: None,
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

                let Some(mapped) = map_event(&event) else {
                    return;
                };

                let mut control = WinitControl { target };
                on_event(mapped, &mut self, &mut control);
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

fn map_event(event: &WinitEvent<()>) -> Option<Event> {
    match event {
        WinitEvent::Resumed => Some(Event::Resumed),
        WinitEvent::Suspended => Some(Event::Suspended),
        WinitEvent::AboutToWait => Some(Event::AboutToWait),
        WinitEvent::WindowEvent { event, .. } => map_window_event(event).map(Event::Window),
        WinitEvent::DeviceEvent {
            event: WinitDeviceEvent::MouseMotion { delta },
            ..
        } => Some(Event::Device(DeviceEvent::MouseMotion { delta: *delta })),
        _ => None,
    }
}

fn map_window_event(event: &WinitWindowEvent) -> Option<WindowEvent> {
    match event {
        WinitWindowEvent::CloseRequested => Some(WindowEvent::CloseRequested),
        WinitWindowEvent::Resized(size) => Some(WindowEvent::Resized {
            width: size.width,
            height: size.height,
        }),
        WinitWindowEvent::Focused(focused) => Some(WindowEvent::Focused(*focused)),
        WinitWindowEvent::ModifiersChanged(next) => {
            Some(WindowEvent::ModifiersChanged(map_modifiers(next.state())))
        }
        WinitWindowEvent::CursorMoved { position, .. } => Some(WindowEvent::CursorMoved {
            x: position.x,
            y: position.y,
        }),
        WinitWindowEvent::MouseWheel { delta, .. } => Some(WindowEvent::MouseWheel {
            delta: map_scroll(*delta),
        }),
        WinitWindowEvent::MouseInput { state, button, .. } => Some(WindowEvent::MouseInput {
            state: map_element_state(*state),
            button: map_mouse_button(*button),
        }),
        WinitWindowEvent::KeyboardInput { event, .. } => {
            let key_code = match event.physical_key {
                PhysicalKey::Code(code) => map_key_code(code),
                _ => None,
            };

            Some(WindowEvent::KeyboardInput(KeyboardInput {
                state: map_element_state(event.state),
                key_code,
                repeat: event.repeat,
            }))
        }
        WinitWindowEvent::Touch(touch) => Some(WindowEvent::Touch(Touch {
            id: touch.id,
            phase: map_touch_phase(touch.phase),
            location: (touch.location.x, touch.location.y),
        })),
        WinitWindowEvent::RedrawRequested => Some(WindowEvent::RedrawRequested),
        _ => None,
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
        WinitKeyCode::Delete => KeyCode::Delete,
        WinitKeyCode::Backspace => KeyCode::Backspace,
        WinitKeyCode::Tab => KeyCode::Tab,
        WinitKeyCode::Space => KeyCode::Space,
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
        WinitKeyCode::Digit1 => KeyCode::Digit1,
        WinitKeyCode::Digit2 => KeyCode::Digit2,
        WinitKeyCode::KeyA => KeyCode::KeyA,
        WinitKeyCode::KeyB => KeyCode::KeyB,
        WinitKeyCode::KeyC => KeyCode::KeyC,
        WinitKeyCode::KeyD => KeyCode::KeyD,
        WinitKeyCode::KeyE => KeyCode::KeyE,
        WinitKeyCode::KeyG => KeyCode::KeyG,
        WinitKeyCode::KeyQ => KeyCode::KeyQ,
        WinitKeyCode::KeyS => KeyCode::KeyS,
        WinitKeyCode::KeyT => KeyCode::KeyT,
        WinitKeyCode::KeyV => KeyCode::KeyV,
        WinitKeyCode::KeyW => KeyCode::KeyW,
        WinitKeyCode::BracketLeft => KeyCode::BracketLeft,
        WinitKeyCode::BracketRight => KeyCode::BracketRight,
        _ => return None,
    })
}
