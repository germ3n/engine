pub mod convar;

pub use convar::{ConVar, ConVarValue};
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "Base", version = "1.0", about = "An awesome networked game")]
pub struct CliArgs {
    #[arg(long)]
    pub map: Option<String>,

    #[arg(long, default_value_t = false)]
    pub editor: bool,

    #[arg(long = "compile-map", visible_alias = "compile_map", default_value_t = false)]
    pub compile_map: bool,

    #[arg(long, default_value_t = false)]
    pub dedicated: bool,

    #[arg(long, default_value_t = 60)]
    pub tickrate: u32,
}

pub fn get_cmdline_args() -> CliArgs {
    let processed_args = std::env::args().map(|arg| {
        if arg.starts_with('+') {
            format!("--{}", &arg[1..])
        } else {
            arg
        }
    });

    CliArgs::parse_from(processed_args)
}