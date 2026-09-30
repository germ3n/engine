mod event;
mod gamepad;
mod host;
mod sdl2_host;
mod surface;
mod winit_host;
mod xbox;

pub use event::*;
pub use gamepad::{GamepadState, PadDeadzones, PadPower};
pub use host::{Control, HostOps};
pub use sdl2_host::Sdl2Host;
pub use surface::Surface;
pub use winit_host::WinitHost;
pub use xbox::XboxHost;

#[cfg(target_os = "android")]
pub use winit_host::{redirect_stdio, remember};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostKind {
    Winit,
    Sdl2,
    Xbox,
}

impl HostKind {
    pub fn from_env() -> Self {
        match std::env::var("ENGINE_HOST") {
            Ok(value) if value.eq_ignore_ascii_case("sdl2") => Self::Sdl2,
            Ok(value) if value.eq_ignore_ascii_case("xbox") => Self::Xbox,
            Ok(value) if value.eq_ignore_ascii_case("winit") => Self::Winit,
            Ok(value) => {
                println!("[host] unknown ENGINE_HOST={value}, using winit");

                Self::Winit
            }
            Err(_) => Self::Winit,
        }
    }
}

pub enum PlatformHost {
    Winit(WinitHost),
    Sdl2(Sdl2Host),
    Xbox(XboxHost),
}

impl PlatformHost {
    pub fn open(kind: HostKind) -> Result<Self, String> {
        match kind {
            HostKind::Winit => Ok(Self::Winit(WinitHost::open()?)),
            HostKind::Sdl2 => Ok(Self::Sdl2(Sdl2Host::open()?)),
            HostKind::Xbox => Ok(Self::Xbox(XboxHost::open()?)),
        }
    }

    #[allow(dead_code)]
    pub fn kind(&self) -> HostKind {
        match self {
            Self::Winit(host) => host.kind(),
            Self::Sdl2(host) => host.kind(),
            Self::Xbox(host) => host.kind(),
        }
    }

    pub fn surface(&self) -> Option<&Surface> {
        match self {
            Self::Winit(host) => host.surface(),
            Self::Sdl2(host) => host.surface(),
            Self::Xbox(host) => host.surface(),
        }
    }

    pub fn set_title(&mut self, title: &str) {
        match self {
            Self::Winit(host) => host.set_title(title),
            Self::Sdl2(host) => host.set_title(title),
            Self::Xbox(host) => host.set_title(title),
        }
    }

    pub fn set_size(&mut self, w: u32, h: u32) {
        match self {
            Self::Winit(host) => host.set_size(w, h),
            Self::Sdl2(host) => host.set_size(w, h),
            Self::Xbox(host) => host.set_size(w, h),
        }
    }

    #[allow(dead_code)]
    pub fn size(&self) -> (u32, u32) {
        match self {
            Self::Winit(host) => host.size(),
            Self::Sdl2(host) => host.size(),
            Self::Xbox(host) => host.size(),
        }
    }

    #[allow(dead_code)]
    pub fn set_cursor_grabbed(&mut self, grabbed: bool) {
        match self {
            Self::Winit(host) => host.set_cursor_grabbed(grabbed),
            Self::Sdl2(host) => host.set_cursor_grabbed(grabbed),
            Self::Xbox(host) => host.set_cursor_grabbed(grabbed),
        }
    }

    #[allow(dead_code)]
    pub fn request_redraw(&mut self) {
        match self {
            Self::Winit(host) => host.request_redraw(),
            Self::Sdl2(host) => host.request_redraw(),
            Self::Xbox(host) => host.request_redraw(),
        }
    }

    pub fn run(self, on_event: impl FnMut(Event, &mut dyn HostOps, &mut dyn Control) + 'static) {
        match self {
            Self::Winit(host) => host.run(on_event),
            Self::Sdl2(host) => host.run(on_event),
            Self::Xbox(host) => host.run(on_event),
        }
    }
}
