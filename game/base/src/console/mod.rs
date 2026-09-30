pub mod convar;

use crate::input::{binds_path, Action, Binding, Binds};
use clap::Parser;
pub use convar::{ConVar, ConVarValue};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Parser, Debug, Clone)]
#[command(name = "Base", version = "1.0", about = "An awesome networked game")]
pub struct CliArgs {
    #[arg(long)]
    pub map: Option<String>,

    #[arg(long, default_value_t = false)]
    pub editor: bool,

    #[arg(
        long = "compile-map",
        visible_alias = "compile_map",
        default_value_t = false
    )]
    pub compile_map: bool,

    #[arg(long, default_value_t = false)]
    pub dedicated: bool,

    #[arg(long, default_value_t = 60)]
    pub tickrate: u32,

    #[arg(long = "connect_lobby", visible_alias = "connect-lobby")]
    pub connect_lobby: Option<u64>,

    #[arg(long)]
    pub connect: Option<String>,
}

pub fn get_cmdline_args() -> CliArgs {
    #[cfg(target_os = "android")]
    {
        return CliArgs {
            map: None,
            editor: false,
            compile_map: false,
            dedicated: false,
            tickrate: 60,
            connect_lobby: None,
            connect: None,
        };
    }

    #[cfg(not(target_os = "android"))]
    {
        let processed_args = std::env::args().map(|arg| {
            if arg.starts_with('+') {
                format!("--{}", &arg[1..])
            } else {
                arg
            }
        });

        return CliArgs::parse_from(processed_args);
    }
}

pub fn float_cvar(cvars: &HashMap<String, Arc<ConVar>>, name: &str, default: f64) -> f64 {
    let Some(var) = cvars.get(name) else {
        return default;
    };

    match &*var.value.lock().unwrap() {
        ConVarValue::Float(value) => *value,
        ConVarValue::Integer(value) => *value as f64,
        _ => default,
    }
}

pub fn pad_deadzones(
    cvars: &HashMap<String, Arc<ConVar>>,
    index: usize,
) -> crate::platform::PadDeadzones {
    crate::platform::PadDeadzones {
        left: float_cvar(cvars, &format!("pad{index}_deadzone_left"), 0.15).max(0.0) as f32,
        right: float_cvar(cvars, &format!("pad{index}_deadzone_right"), 0.15).max(0.0) as f32,
        gas: float_cvar(cvars, &format!("pad{index}_deadzone_gas"), 0.05).max(0.0) as f32,
        brake: float_cvar(cvars, &format!("pad{index}_deadzone_brake"), 0.05).max(0.0) as f32,
        clutch: float_cvar(cvars, &format!("pad{index}_deadzone_clutch"), 0.05).max(0.0) as f32,
    }
}

pub fn exec_line(line: &str, binds: &mut Binds) -> Result<(), String> {
    let trimmed = strip_comment(line).trim();

    if trimmed.is_empty() {
        return Ok(());
    }

    let tokens = tokenize(trimmed);

    if tokens.is_empty() {
        return Ok(());
    }

    match tokens[0].as_str() {
        "bind" => {
            if tokens.len() != 3 {
                return Err("usage: bind <control> <action>".to_string());
            }

            let action = Action::parse(&tokens[2])
                .ok_or_else(|| format!("unknown action '{}'", tokens[2]))?;
            let controls = Binding::parse(&tokens[1])?;

            for control in controls {
                binds.bind(control, action);
            }

            Ok(())
        }
        "unbind" => {
            if tokens.len() != 2 {
                return Err("usage: unbind <control>".to_string());
            }

            let controls = Binding::parse(&tokens[1])?;

            for control in controls {
                binds.unbind(control);
            }

            Ok(())
        }
        "unbindall" => {
            if tokens.len() != 1 {
                return Err("usage: unbindall".to_string());
            }

            binds.unbind_all();

            Ok(())
        }
        "exec" => {
            if tokens.len() != 2 {
                return Err("usage: exec <path>".to_string());
            }

            exec_file(Path::new(&tokens[1]), binds)
        }
        "host_writeconfig" => {
            if tokens.len() != 1 {
                return Err("usage: host_writeconfig".to_string());
            }

            binds.write_cfg(&binds_path())
        }
        other => Err(format!("unknown command '{other}'")),
    }
}

pub fn exec_file(path: &Path, binds: &mut Binds) -> Result<(), String> {
    let text =
        std::fs::read_to_string(path).map_err(|err| format!("exec {}: {err}", path.display()))?;

    for (idx, line) in text.lines().enumerate() {
        exec_line(line, binds).map_err(|err| format!("{}:{}: {err}", path.display(), idx + 1))?;
    }

    Ok(())
}

fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }

            continue;
        }

        if ch == '"' {
            while let Some(next) = chars.next() {
                if next == '"' {
                    break;
                }

                current.push(next);
            }

            tokens.push(std::mem::take(&mut current));

            continue;
        }

        current.push(ch);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Action, Binding};
    use crate::platform::KeyCode;

    #[test]
    fn bind_and_unbind_commands() {
        let mut binds = Binds::new();
        exec_line("bind e use", &mut binds).unwrap();
        assert_eq!(binds.get(Binding::Key(KeyCode::KeyE)), Some(Action::Use));

        exec_line("unbind e", &mut binds).unwrap();
        assert_eq!(binds.get(Binding::Key(KeyCode::KeyE)), None);
    }

    #[test]
    fn host_writeconfig_writes_file() {
        let dir = std::env::temp_dir().join(format!("engine-binds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("binds.cfg");
        let mut binds = Binds::defaults();
        binds.write_cfg(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("unbindall"));
        assert!(text.contains("bind w forward"));

        let mut restored = Binds::new();
        exec_file(&path, &mut restored).unwrap();
        assert_eq!(
            restored.get(Binding::Key(KeyCode::KeyW)),
            Some(Action::Forward)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
