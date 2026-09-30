use crate::platform::gamepad::{GamepadState, PadDeadzones};
use crate::platform::surface::Surface;
use crate::platform::HostKind;

pub trait Control {
    fn poll(&mut self);
    fn exit(&mut self);
}

pub trait HostOps {
    #[allow(dead_code)]
    fn kind(&self) -> HostKind;
    fn surface(&self) -> Option<&Surface>;
    fn set_title(&mut self, title: &str);
    fn set_size(&mut self, w: u32, h: u32);
    fn size(&self) -> (u32, u32);
    fn set_cursor_grabbed(&mut self, grabbed: bool);
    fn request_redraw(&mut self);
    fn gamepad(&mut self, index: usize, deadzones: PadDeadzones) -> GamepadState;
}
