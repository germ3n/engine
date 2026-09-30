use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_renderer")]
    pub renderer: String,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_map")]
    pub map: String,
    #[serde(default = "default_tickrate")]
    pub tickrate: u32,
    #[serde(default)]
    pub editor: bool,
}

fn default_renderer() -> String {
    "auto".to_string()
}

fn default_host() -> String {
    "winit".to_string()
}

fn default_map() -> String {
    "hall".to_string()
}

fn default_tickrate() -> u32 {
    60
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            renderer: default_renderer(),
            host: default_host(),
            map: default_map(),
            tickrate: default_tickrate(),
            editor: false,
        }
    }
}

pub fn config_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("launcher.json");
        }
    }

    PathBuf::from("launcher.json")
}

pub fn load() -> Settings {
    let path = config_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return Settings::default();
    };

    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let path = config_path();
    let text = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    fs::write(&path, text).map_err(|err| err.to_string())
}

pub fn renderers() -> Vec<&'static str> {
    let mut names = vec!["auto"];

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    names.push("metal");

    #[cfg(windows)]
    {
        names.push("d3d12");
        names.push("d3d11");
    }

    #[cfg(not(target_os = "ios"))]
    {
        names.push("vulkan");
        names.push("opengl");
    }

    names
}

pub fn hosts() -> &'static [&'static str] {
    &["winit", "sdl2", "xbox"]
}

pub fn resolve_base() -> Option<PathBuf> {
    let name = if cfg!(windows) { "base.exe" } else { "base" };

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let same = dir.join(name);

            if same.is_file() {
                return Some(same);
            }

            let sibling = dir.join("..").join(name);

            if sibling.is_file() {
                return Some(sibling);
            }
        }
    }

    let cwd = Path::new(name);

    if cwd.is_file() {
        return Some(cwd.to_path_buf());
    }

    None
}
