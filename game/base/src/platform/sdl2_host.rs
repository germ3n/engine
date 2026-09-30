use crate::platform::event::Event;
use crate::platform::gamepad::{GamepadState, PadDeadzones};
use crate::platform::host::{Control, HostOps};
use crate::platform::surface::Surface;
use crate::platform::HostKind;

pub struct Sdl2Host;

impl Sdl2Host {
    pub fn open() -> Result<Self, String> {
        Err("sdl2 host is not implemented".to_string())
    }

    pub fn run(self, _on_event: impl FnMut(Event, &mut dyn HostOps, &mut dyn Control) + 'static) {
        unimplemented!("sdl2 host")
    }
}

impl HostOps for Sdl2Host {
    fn kind(&self) -> HostKind {
        HostKind::Sdl2
    }

    fn surface(&self) -> Option<&Surface> {
        None
    }

    fn set_title(&mut self, _title: &str) {}

    fn set_size(&mut self, _w: u32, _h: u32) {}

    fn size(&self) -> (u32, u32) {
        (1, 1)
    }

    fn set_cursor_grabbed(&mut self, _grabbed: bool) {}

    fn request_redraw(&mut self) {}

    fn gamepad(&mut self, _index: usize, _deadzones: PadDeadzones) -> GamepadState {
        GamepadState::idle()
    }
}
