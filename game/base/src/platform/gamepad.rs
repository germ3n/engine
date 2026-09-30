use crate::input::PadButtons;

pub const PAD_COUNT: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct PadDeadzones {
    pub left: f32,
    pub right: f32,
    pub gas: f32,
    pub brake: f32,
    pub clutch: f32,
}

impl Default for PadDeadzones {
    fn default() -> Self {
        Self {
            left: 0.15,
            right: 0.15,
            gas: 0.05,
            brake: 0.05,
            clutch: 0.05,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadPower {
    Unknown,
    Wired,
    Discharging(u8),
    Charging(u8),
    Charged,
}

#[derive(Clone, Copy, Debug)]
pub struct GamepadState {
    pub forward: f32,
    pub right: f32,
    pub look_x: f32,
    pub look_y: f32,
    pub gas: f32,
    pub brake: f32,
    pub clutch: f32,
    pub power: PadPower,
    pub buttons: PadButtons,
}

impl GamepadState {
    pub fn idle() -> Self {
        Self {
            forward: 0.0,
            right: 0.0,
            look_x: 0.0,
            look_y: 0.0,
            gas: 0.0,
            brake: 0.0,
            clutch: 0.0,
            power: PadPower::Unknown,
            buttons: PadButtons::NONE,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PadCache {
    states: [GamepadState; PAD_COUNT],
}

impl PadCache {
    pub fn new() -> Self {
        Self {
            states: [GamepadState::idle(); PAD_COUNT],
        }
    }

    pub fn set(&mut self, index: usize, state: GamepadState) {
        if let Some(slot) = self.states.get_mut(index) {
            *slot = state;
        }
    }

    pub fn get(&self, index: usize) -> GamepadState {
        self.states
            .get(index)
            .copied()
            .unwrap_or_else(GamepadState::idle)
    }
}

impl Default for PadCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn stick(value: f32, deadzone: f32) -> f32 {
    if value.abs() <= deadzone {
        0.0
    } else {
        value
    }
}

pub fn pedal(value: f32, deadzone: f32) -> f32 {
    let amount = if value < 0.0 {
        (value + 1.0) * 0.5
    } else {
        value
    }
    .clamp(0.0, 1.0);

    if amount <= deadzone {
        0.0
    } else {
        amount
    }
}

#[cfg(test)]
mod tests {
    use super::{pedal, stick};

    #[test]
    fn stick_deadzone_drops_a_resting_axis() {
        assert_eq!(stick(0.1, 0.15), 0.0);
        assert_eq!(stick(-0.15, 0.15), 0.0);
        assert_eq!(stick(0.5, 0.15), 0.5);
        assert_eq!(stick(-1.0, 0.15), -1.0);
    }

    #[test]
    fn pedal_maps_rest_and_press() {
        assert_eq!(pedal(0.0, 0.05), 0.0);
        assert_eq!(pedal(0.04, 0.05), 0.0);
        assert_eq!(pedal(0.5, 0.05), 0.5);
        assert_eq!(pedal(1.0, 0.05), 1.0);
        assert_eq!(pedal(-1.0, 0.05), 0.0);
        assert!((pedal(-0.5, 0.05) - 0.25).abs() < 0.001);
    }
}
