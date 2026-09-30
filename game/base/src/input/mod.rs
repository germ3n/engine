use crate::platform::{KeyCode, MouseButton};
use crate::r#enum::InputButtons;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Attack,
    Attack2,
    Use,
    Sprint,
    Walk,
    Duck,
    Jump,
    Reload,
    Forward,
    Back,
    Left,
    Right,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Attack => "attack",
            Self::Attack2 => "attack2",
            Self::Use => "use",
            Self::Sprint => "sprint",
            Self::Walk => "walk",
            Self::Duck => "duck",
            Self::Jump => "jump",
            Self::Reload => "reload",
            Self::Forward => "forward",
            Self::Back => "back",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "attack" | "+attack" => Self::Attack,
            "attack2" | "+attack2" => Self::Attack2,
            "use" | "+use" => Self::Use,
            "sprint" | "+sprint" => Self::Sprint,
            "walk" | "+walk" => Self::Walk,
            "duck" | "+duck" => Self::Duck,
            "jump" | "+jump" => Self::Jump,
            "reload" | "+reload" => Self::Reload,
            "forward" | "+forward" => Self::Forward,
            "back" | "+back" => Self::Back,
            "left" | "+left" => Self::Left,
            "right" | "+right" => Self::Right,
            _ => return None,
        })
    }

    pub fn to_button(self) -> Option<InputButtons> {
        Some(match self {
            Self::Attack => InputButtons::IN_ATTACK,
            Self::Attack2 => InputButtons::IN_ATTACK2,
            Self::Use => InputButtons::IN_USE,
            Self::Sprint => InputButtons::IN_SPRINT,
            Self::Walk => InputButtons::IN_WALK,
            Self::Duck => InputButtons::IN_DUCK,
            Self::Jump => InputButtons::IN_JUMP,
            Self::Reload => InputButtons::IN_RELOAD,
            Self::Forward | Self::Back | Self::Left | Self::Right => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadButton {
    South,
    East,
    West,
    North,
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    LeftThumb,
    RightThumb,
    Select,
    Start,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    PedalGas,
    PedalBrake,
    PedalClutch,
    Paddle1,
    Paddle2,
    Paddle3,
    Paddle4,
}

impl PadButton {
    pub fn name(self) -> &'static str {
        match self {
            Self::South => "pad_a",
            Self::East => "pad_b",
            Self::West => "pad_x",
            Self::North => "pad_y",
            Self::LeftTrigger => "pad_lb",
            Self::LeftTrigger2 => "pad_lt",
            Self::RightTrigger => "pad_rb",
            Self::RightTrigger2 => "pad_rt",
            Self::LeftThumb => "pad_ls",
            Self::RightThumb => "pad_rs",
            Self::Select => "pad_select",
            Self::Start => "pad_start",
            Self::DPadUp => "dpad_up",
            Self::DPadDown => "dpad_down",
            Self::DPadLeft => "dpad_left",
            Self::DPadRight => "dpad_right",
            Self::PedalGas => "pad_gas",
            Self::PedalBrake => "pad_brake",
            Self::PedalClutch => "pad_clutch",
            Self::Paddle1 => "pad_paddle1",
            Self::Paddle2 => "pad_paddle2",
            Self::Paddle3 => "pad_paddle3",
            Self::Paddle4 => "pad_paddle4",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "pad_a" | "pad_south" => Self::South,
            "pad_b" | "pad_east" => Self::East,
            "pad_x" | "pad_west" => Self::West,
            "pad_y" | "pad_north" => Self::North,
            "pad_lb" | "pad_l1" => Self::LeftTrigger,
            "pad_lt" | "pad_l2" => Self::LeftTrigger2,
            "pad_rb" | "pad_r1" => Self::RightTrigger,
            "pad_rt" | "pad_r2" => Self::RightTrigger2,
            "pad_ls" | "pad_l3" => Self::LeftThumb,
            "pad_rs" | "pad_r3" => Self::RightThumb,
            "pad_select" | "pad_back" => Self::Select,
            "pad_start" => Self::Start,
            "dpad_up" => Self::DPadUp,
            "dpad_down" => Self::DPadDown,
            "dpad_left" => Self::DPadLeft,
            "dpad_right" => Self::DPadRight,
            "pad_gas" | "pad_accelerator" | "pad_throttle" => Self::PedalGas,
            "pad_brake" => Self::PedalBrake,
            "pad_clutch" => Self::PedalClutch,
            "pad_paddle1" | "paddle1" | "pad_p1" => Self::Paddle1,
            "pad_paddle2" | "paddle2" | "pad_p2" => Self::Paddle2,
            "pad_paddle3" | "paddle3" | "pad_p3" => Self::Paddle3,
            "pad_paddle4" | "paddle4" | "pad_p4" => Self::Paddle4,
            _ => return None,
        })
    }

    pub const fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PadButtons(pub u32);

impl PadButtons {
    pub const NONE: Self = Self(0);

    #[inline]
    pub const fn contains(self, button: PadButton) -> bool {
        (self.0 & button.bit()) != 0
    }

    #[inline]
    pub fn insert(&mut self, button: PadButton) {
        self.0 |= button.bit();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
    Pad(PadButton),
}

impl Binding {
    pub fn name(self) -> String {
        match self {
            Self::Key(code) => key_name(code).to_string(),
            Self::Mouse(button) => mouse_name(button).to_string(),
            Self::Pad(button) => button.name().to_string(),
        }
    }

    pub fn parse(name: &str) -> Result<Vec<Self>, String> {
        let lower = name.to_ascii_lowercase();

        if lower == "shift" {
            return Ok(vec![
                Self::Key(KeyCode::ShiftLeft),
                Self::Key(KeyCode::ShiftRight),
            ]);
        }

        if lower == "ctrl" || lower == "control" {
            return Ok(vec![
                Self::Key(KeyCode::ControlLeft),
                Self::Key(KeyCode::ControlRight),
            ]);
        }

        if lower == "alt" {
            return Ok(vec![
                Self::Key(KeyCode::AltLeft),
                Self::Key(KeyCode::AltRight),
            ]);
        }

        if let Some(code) = parse_key(&lower) {
            return Ok(vec![Self::Key(code)]);
        }

        if let Some(button) = parse_mouse(&lower) {
            return Ok(vec![Self::Mouse(button)]);
        }

        if let Some(button) = PadButton::parse(&lower) {
            return Ok(vec![Self::Pad(button)]);
        }

        Err(format!("unknown control '{name}'"))
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Binds {
    map: HashMap<Binding, Action>,
}

impl Binds {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn defaults() -> Self {
        let mut binds = Self::new();
        binds.bind(Binding::Key(KeyCode::KeyW), Action::Forward);
        binds.bind(Binding::Key(KeyCode::KeyS), Action::Back);
        binds.bind(Binding::Key(KeyCode::KeyA), Action::Left);
        binds.bind(Binding::Key(KeyCode::KeyD), Action::Right);
        binds.bind(Binding::Key(KeyCode::Space), Action::Jump);
        binds.bind(Binding::Key(KeyCode::ShiftLeft), Action::Sprint);
        binds.bind(Binding::Key(KeyCode::ShiftRight), Action::Sprint);
        binds.bind(Binding::Key(KeyCode::ControlLeft), Action::Duck);
        binds.bind(Binding::Key(KeyCode::ControlRight), Action::Duck);
        binds.bind(Binding::Key(KeyCode::AltLeft), Action::Walk);
        binds.bind(Binding::Key(KeyCode::AltRight), Action::Walk);
        binds.bind(Binding::Mouse(MouseButton::Left), Action::Attack);
        binds.bind(Binding::Pad(PadButton::RightTrigger2), Action::Attack);
        binds.bind(Binding::Pad(PadButton::LeftTrigger2), Action::Attack2);
        binds.bind(Binding::Pad(PadButton::West), Action::Use);
        binds.bind(Binding::Pad(PadButton::RightTrigger), Action::Sprint);
        binds.bind(Binding::Pad(PadButton::LeftTrigger), Action::Walk);
        binds.bind(Binding::Pad(PadButton::LeftThumb), Action::Duck);
        binds.bind(Binding::Pad(PadButton::South), Action::Jump);
        binds.bind(Binding::Pad(PadButton::East), Action::Reload);
        binds.bind(Binding::Pad(PadButton::DPadUp), Action::Forward);
        binds.bind(Binding::Pad(PadButton::DPadDown), Action::Back);
        binds.bind(Binding::Pad(PadButton::DPadLeft), Action::Left);
        binds.bind(Binding::Pad(PadButton::DPadRight), Action::Right);

        binds
    }

    pub fn bind(&mut self, binding: Binding, action: Action) {
        self.map.insert(binding, action);
    }

    pub fn unbind(&mut self, binding: Binding) {
        self.map.remove(&binding);
    }

    pub fn unbind_all(&mut self) {
        self.map.clear();
    }

    pub fn get(&self, binding: Binding) -> Option<Action> {
        self.map.get(&binding).copied()
    }

    pub fn action_held(
        &self,
        action: Action,
        keys: &HashSet<KeyCode>,
        mouse: &HashSet<MouseButton>,
        pad: PadButtons,
    ) -> bool {
        for (binding, mapped) in &self.map {
            if *mapped != action {
                continue;
            }

            let pressed = match *binding {
                Binding::Key(code) => keys.contains(&code),
                Binding::Mouse(button) => mouse.contains(&button),
                Binding::Pad(button) => pad.contains(button),
            };

            if pressed {
                return true;
            }
        }

        false
    }

    pub fn buttons_held(
        &self,
        keys: &HashSet<KeyCode>,
        mouse: &HashSet<MouseButton>,
        pad: PadButtons,
    ) -> InputButtons {
        let mut buttons = InputButtons::NONE;

        for action in [
            Action::Attack,
            Action::Attack2,
            Action::Use,
            Action::Sprint,
            Action::Walk,
            Action::Duck,
            Action::Jump,
            Action::Reload,
        ] {
            if self.action_held(action, keys, mouse, pad) {
                if let Some(flag) = action.to_button() {
                    buttons |= flag;
                }
            }
        }

        buttons
    }

    pub fn axis_held(
        &self,
        keys: &HashSet<KeyCode>,
        mouse: &HashSet<MouseButton>,
        pad: PadButtons,
    ) -> (f32, f32) {
        let forward = self.action_held(Action::Forward, keys, mouse, pad) as i32 as f32
            - self.action_held(Action::Back, keys, mouse, pad) as i32 as f32;
        let right = self.action_held(Action::Right, keys, mouse, pad) as i32 as f32
            - self.action_held(Action::Left, keys, mouse, pad) as i32 as f32;

        (forward, right)
    }

    pub fn to_cfg_lines(&self) -> Vec<String> {
        let mut lines = vec!["unbindall".to_string()];
        let mut entries: Vec<_> = self.map.iter().collect();
        entries.sort_by(|(left, _), (right, _)| left.name().cmp(&right.name()));

        for (binding, action) in entries {
            lines.push(format!("bind {} {}", binding.name(), action.name()));
        }

        lines
    }

    pub fn write_cfg(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("cfg {}: {err}", parent.display()))?;
            }
        }

        let body = self.to_cfg_lines().join("\n") + "\n";
        std::fs::write(path, body).map_err(|err| format!("cfg {}: {err}", path.display()))
    }
}

pub fn binds_path() -> PathBuf {
    PathBuf::from("cfg/binds.cfg")
}

pub fn load_or_defaults(path: &Path) -> Binds {
    if path.is_file() {
        let mut binds = Binds::new();

        if let Err(err) = crate::console::exec_file(path, &mut binds) {
            println!("[binds] {err}");

            return Binds::defaults();
        }

        return binds;
    }

    let binds = Binds::defaults();

    if let Err(err) = binds.write_cfg(path) {
        println!("[binds] {err}");
    }

    binds
}

fn key_name(code: KeyCode) -> &'static str {
    match code {
        KeyCode::Escape => "escape",
        KeyCode::Delete => "delete",
        KeyCode::Backspace => "backspace",
        KeyCode::Tab => "tab",
        KeyCode::Space => "space",
        KeyCode::ShiftLeft => "lshift",
        KeyCode::ShiftRight => "rshift",
        KeyCode::ControlLeft => "lctrl",
        KeyCode::ControlRight => "rctrl",
        KeyCode::AltLeft => "lalt",
        KeyCode::AltRight => "ralt",
        KeyCode::SuperLeft => "lsuper",
        KeyCode::SuperRight => "rsuper",
        KeyCode::ArrowLeft => "leftarrow",
        KeyCode::ArrowRight => "rightarrow",
        KeyCode::ArrowUp => "uparrow",
        KeyCode::ArrowDown => "downarrow",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::KeyA => "a",
        KeyCode::KeyB => "b",
        KeyCode::KeyC => "c",
        KeyCode::KeyD => "d",
        KeyCode::KeyE => "e",
        KeyCode::KeyG => "g",
        KeyCode::KeyQ => "q",
        KeyCode::KeyS => "s",
        KeyCode::KeyT => "t",
        KeyCode::KeyV => "v",
        KeyCode::KeyW => "w",
        KeyCode::BracketLeft => "[",
        KeyCode::BracketRight => "]",
    }
}

fn parse_key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "escape" | "esc" => KeyCode::Escape,
        "delete" | "del" => KeyCode::Delete,
        "backspace" | "back" => KeyCode::Backspace,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Space,
        "lshift" => KeyCode::ShiftLeft,
        "rshift" => KeyCode::ShiftRight,
        "lctrl" | "lcontrol" => KeyCode::ControlLeft,
        "rctrl" | "rcontrol" => KeyCode::ControlRight,
        "lalt" => KeyCode::AltLeft,
        "ralt" => KeyCode::AltRight,
        "lsuper" | "lwin" | "lcmd" => KeyCode::SuperLeft,
        "rsuper" | "rwin" | "rcmd" => KeyCode::SuperRight,
        "leftarrow" | "arrowleft" => KeyCode::ArrowLeft,
        "rightarrow" | "arrowright" => KeyCode::ArrowRight,
        "uparrow" | "arrowup" => KeyCode::ArrowUp,
        "downarrow" | "arrowdown" => KeyCode::ArrowDown,
        "1" => KeyCode::Digit1,
        "2" => KeyCode::Digit2,
        "a" => KeyCode::KeyA,
        "b" => KeyCode::KeyB,
        "c" => KeyCode::KeyC,
        "d" => KeyCode::KeyD,
        "e" => KeyCode::KeyE,
        "g" => KeyCode::KeyG,
        "q" => KeyCode::KeyQ,
        "s" => KeyCode::KeyS,
        "t" => KeyCode::KeyT,
        "v" => KeyCode::KeyV,
        "w" => KeyCode::KeyW,
        "[" | "bracketleft" => KeyCode::BracketLeft,
        "]" | "bracketright" => KeyCode::BracketRight,
        _ => return None,
    })
}

fn mouse_name(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "mouse1",
        MouseButton::Right => "mouse2",
        MouseButton::Middle => "mouse3",
        MouseButton::Other(3) => "mouse4",
        MouseButton::Other(4) => "mouse5",
        MouseButton::Other(_) => "mouse",
    }
}

fn parse_mouse(name: &str) -> Option<MouseButton> {
    Some(match name {
        "mouse1" | "mouse_left" => MouseButton::Left,
        "mouse2" | "mouse_right" => MouseButton::Right,
        "mouse3" | "mouse_middle" => MouseButton::Middle,
        "mouse4" => MouseButton::Other(3),
        "mouse5" => MouseButton::Other(4),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_map_wasd_and_jump() {
        let binds = Binds::defaults();
        let mut keys = HashSet::new();
        keys.insert(KeyCode::KeyW);
        keys.insert(KeyCode::Space);

        let (forward, right) = binds.axis_held(&keys, &HashSet::new(), PadButtons::NONE);
        let buttons = binds.buttons_held(&keys, &HashSet::new(), PadButtons::NONE);

        assert_eq!(forward, 1.0);
        assert_eq!(right, 0.0);
        assert!(buttons.contains(InputButtons::IN_JUMP));
    }

    #[test]
    fn bind_replaces_and_unbind_clears() {
        let mut binds = Binds::new();
        binds.bind(Binding::Key(KeyCode::KeyE), Action::Use);
        assert_eq!(binds.get(Binding::Key(KeyCode::KeyE)), Some(Action::Use));

        binds.bind(Binding::Key(KeyCode::KeyE), Action::Reload);
        assert_eq!(binds.get(Binding::Key(KeyCode::KeyE)), Some(Action::Reload));

        binds.unbind(Binding::Key(KeyCode::KeyE));
        assert_eq!(binds.get(Binding::Key(KeyCode::KeyE)), None);
    }

    #[test]
    fn resolve_ors_key_and_pad() {
        let binds = Binds::defaults();
        let keys = HashSet::new();
        let mouse = HashSet::new();
        let mut pad = PadButtons::NONE;
        pad.insert(PadButton::South);

        let buttons = binds.buttons_held(&keys, &mouse, pad);
        assert!(buttons.contains(InputButtons::IN_JUMP));

        let mut keys = HashSet::new();
        keys.insert(KeyCode::Space);
        let buttons = binds.buttons_held(&keys, &mouse, PadButtons::NONE);
        assert!(buttons.contains(InputButtons::IN_JUMP));
    }

    #[test]
    fn cfg_round_trip() {
        let original = Binds::defaults();
        let mut restored = Binds::new();

        for line in original.to_cfg_lines() {
            crate::console::exec_line(&line, &mut restored).unwrap();
        }

        assert_eq!(original.map.len(), restored.map.len());

        for (binding, action) in &original.map {
            assert_eq!(restored.get(*binding), Some(*action));
        }
    }

    #[test]
    fn shift_parses_to_both_keys() {
        let bindings = Binding::parse("shift").unwrap();
        assert_eq!(
            bindings,
            vec![
                Binding::Key(KeyCode::ShiftLeft),
                Binding::Key(KeyCode::ShiftRight)
            ]
        );
    }

    #[test]
    fn pedal_names_parse() {
        assert_eq!(PadButton::parse("pad_gas"), Some(PadButton::PedalGas));
        assert_eq!(PadButton::parse("pad_brake"), Some(PadButton::PedalBrake));
        assert_eq!(PadButton::parse("pad_clutch"), Some(PadButton::PedalClutch));
        assert_eq!(
            Binding::parse("pad_throttle").unwrap(),
            vec![Binding::Pad(PadButton::PedalGas)]
        );
    }
}
