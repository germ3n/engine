pub mod convar;

use clap::Parser;
pub use convar::{ConVar, ConVarValue};
use std::collections::HashMap;
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

pub fn pad_deadzones(cvars: &HashMap<String, Arc<ConVar>>, index: usize) -> (f32, f32) {
    let left = float_cvar(cvars, &format!("pad{index}_deadzone_left"), 0.15).max(0.0) as f32;
    let right = float_cvar(cvars, &format!("pad{index}_deadzone_right"), 0.15).max(0.0) as f32;

    (left, right)
}
