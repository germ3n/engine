use crate::r#enum::InputButtons;

#[derive(Clone, Copy, Debug)]
pub struct GamepadState {
    pub forward: f32,
    pub right: f32,
    pub look_x: f32,
    pub look_y: f32,
    pub buttons: InputButtons,
}

impl GamepadState {
    pub fn idle() -> Self {
        Self {
            forward: 0.0,
            right: 0.0,
            look_x: 0.0,
            look_y: 0.0,
            buttons: InputButtons::NONE,
        }
    }
}
