use source_vpk::Vpk;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct Mounted {
    given: String,
    canonical: Option<PathBuf>,
    vpk: Vpk,
}

static MOUNTED: Mutex<Vec<Mounted>> = Mutex::new(Vec::new());

fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

fn same(entry: &Mounted, given: &str, canonical: Option<&Path>) -> bool {
    entry.given == given || (canonical.is_some() && entry.canonical.as_deref() == canonical)
}

pub fn mount(path: &str) -> Result<usize, String> {
    let resolved = crate::world::expand_home(path.trim());
    let canonical = canonical(&resolved);
    let mut mounted = MOUNTED.lock().unwrap();

    if mounted.iter().any(|entry| same(entry, path, canonical.as_deref())) {
        return Err(format!("{path} is already mounted"));
    }

    let vpk = Vpk::open(&resolved).map_err(|err| format!("vpk {path}: {err}"))?;
    let files = vpk.len();

    log::info!("[vpk] mounted {} ({files} files)", resolved.display());
    mounted.push(Mounted {
        given: path.to_string(),
        canonical,
        vpk,
    });

    Ok(files)
}

pub fn unmount(path: &str) -> bool {
    let canonical = canonical(&crate::world::expand_home(path.trim()));
    let mut mounted = MOUNTED.lock().unwrap();
    let before = mounted.len();

    mounted.retain(|entry| !same(entry, path, canonical.as_deref()));

    let removed = mounted.len() != before;

    if removed {
        log::info!("[vpk] unmounted {path}");
    }

    removed
}

pub fn is_mounted(path: &str) -> bool {
    let canonical = canonical(&crate::world::expand_home(path.trim()));

    MOUNTED
        .lock()
        .unwrap()
        .iter()
        .any(|entry| same(entry, path, canonical.as_deref()))
}

pub fn read(key: &str) -> Option<Vec<u8>> {
    MOUNTED
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|entry| entry.vpk.read(key).ok())
}

const ENV_DIR: &str = "ENGINE_VPK_DIR";
const SCAN_DEPTH: u32 = 3;

pub fn mount_env() -> usize {
    let Some(value) = std::env::var_os(ENV_DIR).filter(|value| !value.is_empty()) else {
        return 0;
    };
    let mut mounted = 0;

    for root in std::env::split_paths(&value) {
        let root = crate::world::expand_home(&root.to_string_lossy());

        if !root.is_dir() {
            log::warn!("[vpk] {ENV_DIR} folder {} was not found", root.display());

            continue;
        }

        let mut found = Vec::new();
        find_archives(&root, 0, &mut found);
        found.sort();

        if found.is_empty() {
            log::warn!("[vpk] no _dir.vpk files under {}", root.display());
        }

        for path in found {
            match mount(&path.to_string_lossy()) {
                Ok(_) => mounted += 1,
                Err(err) => log::warn!("[vpk] {err}"),
            }
        }
    }

    mounted
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let home = crate::world::expand_home("~");

    if cfg!(target_os = "macos") {
        roots.push(home.join("Library/Application Support/Steam"));
    } else if cfg!(windows) {
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(base) = std::env::var_os(var) {
                roots.push(PathBuf::from(base).join("Steam"));
            }
        }

        roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    } else {
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }

    roots
}

fn library_paths(root: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) else {
        return Vec::new();
    };

    text.lines()
        .filter_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);

            if quoted.next()? != "path" {
                return None;
            }

            Some(quoted.next()?.replace("\\\\", "\\"))
        })
        .map(PathBuf::from)
        .collect()
}

pub fn discover() -> Vec<PathBuf> {
    let mut libraries = Vec::new();

    for root in steam_roots() {
        if !root.is_dir() {
            continue;
        }

        libraries.extend(library_paths(&root));
        libraries.push(root);
    }

    let mut seen = HashSet::new();
    let mut found = Vec::new();

    for library in libraries {
        let Some(real) = canonical(&library) else {
            continue;
        };

        if !seen.insert(real.clone()) {
            continue;
        }

        find_archives(&real.join("steamapps/common"), 0, &mut found);
    }

    found.sort();

    found
}

pub fn mount_discovered() -> usize {
    let mut mounted = 0;

    for path in discover() {
        match mount(&path.to_string_lossy()) {
            Ok(_) => mounted += 1,
            Err(err) => log::warn!("[vpk] {err}"),
        }
    }

    mounted
}

fn find_archives(dir: &Path, depth: u32, found: &mut Vec<PathBuf>) {
    if depth > SCAN_DEPTH {
        return;
    }

    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in read.flatten() {
        let path = entry.path();

        if path.is_dir() {
            find_archives(&path, depth + 1, found);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_dir.vpk"))
        {
            found.push(path);
        }
    }
}
