mod pak;

pub use pak::{encode, is_pak, Archive};

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

static GLOBAL: OnceLock<Arc<Fs>> = OnceLock::new();

enum MountKind {
    Pak(Archive),
    Dir(PathBuf),
}

struct Mount {
    name: String,
    kind: MountKind,
}

pub struct Fs {
    mounts: Vec<Mount>,
}

impl Fs {
    pub fn new() -> Self {
        Self { mounts: Vec::new() }
    }

    pub fn boot() -> Result<Self, String> {
        log::info!("[fs] booting");
        let mut fs = Self::new();
        let mut mounted_base = false;

        for path in base_pak_candidates() {
            log::info!("[fs] looking for base.pak at {}", path.display());

            if !path.is_file() {
                continue;
            }

            match fs.mount_pak(&path) {
                Ok(name) => {
                    mounted_base = true;
                    let entries = match &fs.mounts.last().unwrap().kind {
                        MountKind::Pak(archive) => archive.len(),
                        MountKind::Dir(_) => 0,
                    };
                    log::info!(
                        "[fs] mounted base pack '{name}' from {} ({entries} entries)",
                        path.display()
                    );
                    break;
                }
                Err(err) => log::warn!("[fs] {}: {err}", path.display()),
            }
        }

        if !mounted_base {
            let embedded = Archive::from_bytes(EMBEDDED_BASE_PAK.to_vec())?;
            let entries = embedded.len();
            fs.mounts.push(Mount {
                name: "base".to_string(),
                kind: MountKind::Pak(embedded),
            });
            log::info!(
                "[fs] mounted embedded base.pak ({entries} entries, {} bytes)",
                EMBEDDED_BASE_PAK.len()
            );
        }

        for root in search_roots() {
            let addons = root.join("addons");

            if addons.is_dir() {
                log::info!("[fs] scanning addon paks in {}", addons.display());
            }

            fs.mount_addon_paks(&addons)?;
        }

        for root in search_roots() {
            let base_dir = root.join("game").join("base");

            if base_dir.is_dir() {
                match fs.mount_dir_named("game_base", &base_dir) {
                    Ok(name) => log::info!(
                        "[fs] mounted loose dir '{name}' from {}",
                        base_dir.display()
                    ),
                    Err(err) => log::info!("[fs] skip loose {}: {err}", base_dir.display()),
                }
            }

            let addons = root.join("addons");

            if addons.is_dir() {
                log::info!("[fs] scanning addon dirs in {}", addons.display());
            }

            fs.mount_addon_dirs(&addons)?;
        }

        let names: Vec<&str> = fs.mounts().collect();
        log::info!(
            "[fs] ready with {} mount(s): {}",
            names.len(),
            names.join(", ")
        );

        for path in [
            "lua/libs/hook.luac",
            "lua/libs/net.luac",
            "lua/libs/vector3.luac",
            "lua/libs/angle3.luac",
            "lua/libs/ents.luac",
            "lua/libs/scripted_ents.luac",
            "lua/libs/gui.luac",
            "lua/menu/menu.luac",
            "shaders/mesh.wgsl",
            "shaders/color.wgsl",
            "shaders/text.wgsl",
        ] {
            match fs.resolve(path) {
                Some((mount, size)) => {
                    log::info!("[fs]   {path} <- {mount} ({size} bytes)")
                }
                None => log::warn!("[fs]   {path} missing"),
            }
        }

        return Ok(fs);
    }

    pub fn mount_pak(&mut self, path: &Path) -> Result<String, String> {
        let name = mount_name(path)?;

        if self.find(&name).is_some() {
            return Err(format!("mount {name} is already present"));
        }

        let archive = Archive::open(path)?;
        self.mounts.push(Mount {
            name: name.clone(),
            kind: MountKind::Pak(archive),
        });

        return Ok(name);
    }

    pub fn mount_dir(&mut self, path: &Path) -> Result<String, String> {
        let name = mount_name(path)?;

        return self.mount_dir_named(&name, path);
    }

    pub fn mount_dir_named(&mut self, name: &str, path: &Path) -> Result<String, String> {
        pak::check_name(name)?;

        if self.find(name).is_some() {
            return Err(format!("mount {name} is already present"));
        }

        if !path.is_dir() {
            return Err(format!("mount dir {} is missing", path.display()));
        }

        self.mounts.push(Mount {
            name: name.to_string(),
            kind: MountKind::Dir(path.to_path_buf()),
        });

        return Ok(name.to_string());
    }

    pub fn mounts(&self) -> impl Iterator<Item = &str> + '_ {
        self.mounts.iter().map(|mount| mount.name.as_str())
    }

    pub fn exists(&self, path: &str) -> bool {
        if pak::check_name(path).is_err() {
            return false;
        }

        let mut idx = self.mounts.len();

        while idx > 0 {
            idx -= 1;

            if mount_exists(&self.mounts[idx], path) {
                return true;
            }
        }

        return false;
    }

    pub fn priority(&self, path: &str) -> Option<usize> {
        if pak::check_name(path).is_err() {
            return None;
        }

        let mut idx = self.mounts.len();

        while idx > 0 {
            idx -= 1;

            if mount_exists(&self.mounts[idx], path) {
                return Some(idx);
            }
        }

        return None;
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        pak::check_name(path)?;
        let mut idx = self.mounts.len();

        while idx > 0 {
            idx -= 1;

            if let Some(bytes) = mount_read(&self.mounts[idx], path)? {
                log::info!(
                    "[fs] read {path} from '{}' ({} bytes)",
                    self.mounts[idx].name,
                    bytes.len()
                );

                return Ok(bytes);
            }
        }

        return Err(format!("fs entry {path} was not found"));
    }

    pub fn read_string(&self, path: &str) -> Result<String, String> {
        let bytes = self.read(path)?;

        return String::from_utf8(bytes).map_err(|err| format!("fs entry {path}: {err}"));
    }

    pub fn resolve(&self, path: &str) -> Option<(&str, usize)> {
        if pak::check_name(path).is_err() {
            return None;
        }

        let mut idx = self.mounts.len();

        while idx > 0 {
            idx -= 1;

            match mount_read(&self.mounts[idx], path) {
                Ok(Some(bytes)) => return Some((self.mounts[idx].name.as_str(), bytes.len())),
                Ok(None) => {}
                Err(_) => {}
            }
        }

        return None;
    }

    pub fn list_prefix(&self, prefix: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut idx = 0;

        while idx < self.mounts.len() {
            for name in mount_list_prefix(&self.mounts[idx], prefix) {
                if !out.iter().any(|existing| existing == &name) {
                    out.push(name);
                }
            }

            idx += 1;
        }

        out.sort();

        return out;
    }

    fn mount_addon_paks(&mut self, addons: &Path) -> Result<(), String> {
        if !addons.is_dir() {
            return Ok(());
        }

        let mut paths = Vec::new();
        let listing = std::fs::read_dir(addons).map_err(io_err)?;

        for entry in listing {
            let entry = entry.map_err(io_err)?;
            let path = entry.path();

            if is_pak(&path) {
                paths.push(path);
            }
        }

        paths.sort();
        let mut idx = 0;

        while idx < paths.len() {
            match self.mount_pak(&paths[idx]) {
                Ok(name) => log::info!("[fs] mounted addon {name}"),
                Err(err) => log::warn!("[fs] {}: {err}", paths[idx].display()),
            }

            idx += 1;
        }

        return Ok(());
    }

    fn mount_addon_dirs(&mut self, addons: &Path) -> Result<(), String> {
        if !addons.is_dir() {
            return Ok(());
        }

        let mut paths = Vec::new();
        let listing = std::fs::read_dir(addons).map_err(io_err)?;

        for entry in listing {
            let entry = entry.map_err(io_err)?;
            let path = entry.path();

            if path.is_dir() {
                paths.push(path);
            }
        }

        paths.sort();
        let mut idx = 0;

        while idx < paths.len() {
            match self.mount_dir(&paths[idx]) {
                Ok(name) => log::info!("[fs] mounted addon dir {name}"),
                Err(err) => log::warn!("[fs] {}: {err}", paths[idx].display()),
            }

            idx += 1;
        }

        return Ok(());
    }

    fn find(&self, name: &str) -> Option<usize> {
        let mut idx = 0;

        while idx < self.mounts.len() {
            if self.mounts[idx].name == name {
                return Some(idx);
            }

            idx += 1;
        }

        return None;
    }
}

pub fn set_global(fs: Arc<Fs>) {
    let _ = GLOBAL.set(fs);
}

pub fn global() -> Arc<Fs> {
    GLOBAL.get().cloned().unwrap_or_else(|| Arc::new(Fs::new()))
}

pub fn try_global() -> Option<Arc<Fs>> {
    GLOBAL.get().cloned()
}

pub fn read(path: &str) -> Result<Vec<u8>, String> {
    global().read(path)
}

pub fn read_string(path: &str) -> Result<String, String> {
    global().read_string(path)
}

pub fn exists(path: &str) -> bool {
    global().exists(path)
}

fn mount_exists(mount: &Mount, path: &str) -> bool {
    match &mount.kind {
        MountKind::Pak(archive) => archive.contains(path),
        MountKind::Dir(root) => dir_path(root, path)
            .map(|path| path.is_file())
            .unwrap_or(false),
    }
}

fn mount_read(mount: &Mount, path: &str) -> Result<Option<Vec<u8>>, String> {
    match &mount.kind {
        MountKind::Pak(archive) => {
            if !archive.contains(path) {
                return Ok(None);
            }

            return Ok(Some(archive.read(path)?));
        }
        MountKind::Dir(root) => {
            let Some(file) = dir_path(root, path) else {
                return Ok(None);
            };

            if !file.is_file() {
                return Ok(None);
            }

            let bytes = std::fs::read(&file).map_err(io_err)?;

            return Ok(Some(bytes));
        }
    }
}

fn mount_list_prefix(mount: &Mount, prefix: &str) -> Vec<String> {
    match &mount.kind {
        MountKind::Pak(archive) => archive
            .list_prefix(prefix)
            .into_iter()
            .map(|name| name.to_string())
            .collect(),
        MountKind::Dir(root) => list_dir(root, prefix),
    }
}

fn list_dir(root: &Path, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    walk_dir(root, "", prefix, &mut out);

    return out;
}

fn walk_dir(dir: &Path, relative: &str, prefix: &str, out: &mut Vec<String>) {
    let walk = match std::fs::read_dir(dir) {
        Ok(listing) => listing,
        Err(_) => return,
    };

    for entry in walk.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name,
            None => continue,
        };
        let virtual_path = if relative.is_empty() {
            name.to_string()
        } else {
            format!("{relative}/{name}")
        };

        if path.is_dir() {
            if prefix.is_empty()
                || virtual_path.starts_with(prefix)
                || prefix.starts_with(&format!("{virtual_path}/"))
            {
                walk_dir(&path, &virtual_path, prefix, out);
            }
        } else if path.is_file() && (prefix.is_empty() || virtual_path.starts_with(prefix)) {
            out.push(virtual_path);
        }
    }
}

fn dir_path(root: &Path, path: &str) -> Option<PathBuf> {
    if pak::check_name(path).is_err() {
        return None;
    }

    let mut out = root.to_path_buf();

    for part in path.split('/') {
        out.push(part);
    }

    return Some(out);
}

fn mount_name(path: &Path) -> Result<String, String> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| format!("mount path {} is invalid", path.display()))?;
    pak::check_name(stem)?;

    return Ok(stem.to_string());
}

pub(crate) fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        push_unique(&mut roots, cwd);
    }

    if let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
    {
        push_unique(&mut roots, exe);
    }

    push_unique(&mut roots, PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    push_unique(
        &mut roots,
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".."),
    );

    return roots;
}

fn base_pak_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for root in search_roots() {
        push_unique(&mut paths, root.join("base.pak"));
        push_unique(&mut paths, root.join("game").join("base.pak"));
        push_unique(&mut paths, root.join("game").join("base").join("base.pak"));
    }

    return paths;
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn io_err(err: std::io::Error) -> String {
    format!("fs: {err}")
}

const EMBEDDED_BASE_PAK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/base.pak"));

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("engine-fs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        return dir;
    }

    #[test]
    fn later_mounts_override_earlier_ones() {
        let dir = scratch("override");
        let base = dir.join("base.pak");
        let addon = dir.join("addon.pak");
        Archive::create(&base, &[("lua/menu/menu.luac", b"base")]).unwrap();
        Archive::create(&addon, &[("lua/menu/menu.luac", b"addon")]).unwrap();

        let mut fs = Fs::new();
        fs.mount_pak(&base).unwrap();
        fs.mount_pak(&addon).unwrap();
        assert_eq!(fs.read("lua/menu/menu.luac").unwrap(), b"addon");

        let loose = dir.join("loose");
        std::fs::create_dir_all(loose.join("lua/menu")).unwrap();
        std::fs::write(loose.join("lua/menu/menu.luac"), b"loose").unwrap();
        fs.mount_dir_named("loose", &loose).unwrap();
        assert_eq!(fs.read("lua/menu/menu.luac").unwrap(), b"loose");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_serves_bundled_lua_and_shaders() {
        let fs = Fs::boot().unwrap();
        let hook = fs.read("lua/libs/hook.luac").unwrap();
        assert!(hook.starts_with(b"\x1bLJ"));
        let mesh = fs.read_string("shaders/mesh.wgsl").unwrap();
        assert!(mesh.contains("vs_main"));
        assert!(fs.exists("lua/menu/menu.luac"));
        assert!(fs.exists("shaders/text.wgsl"));
    }
}
