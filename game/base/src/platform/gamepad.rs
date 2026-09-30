use crate::input::PadButtons;

#[derive(Clone, Copy, Debug)]
pub struct GamepadState {
    pub forward: f32,
    pub right: f32,
    pub look_x: f32,
    pub look_y: f32,
    pub buttons: PadButtons,
}

impl GamepadState {
    pub fn idle() -> Self {
        Self {
            forward: 0.0,
            right: 0.0,
            look_x: 0.0,
            look_y: 0.0,
            buttons: PadButtons::NONE,
        }
    }
}

pub fn stick(value: f32, deadzone: f32) -> f32 {
    if value.abs() <= deadzone {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::stick;

    #[test]
    fn stick_deadzone_drops_a_resting_axis() {
        assert_eq!(stick(0.1, 0.15), 0.0);
        assert_eq!(stick(-0.15, 0.15), 0.0);
        assert_eq!(stick(0.5, 0.15), 0.5);
        assert_eq!(stick(-1.0, 0.15), -1.0);
    }
}
