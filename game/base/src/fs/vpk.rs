use source_vpk::Vpk;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

struct Mounted {
    given: String,
    canonical: Option<PathBuf>,
    vpk: Arc<Vpk>,
}

static MOUNTED: Mutex<Vec<Mounted>> = Mutex::new(Vec::new());

fn canonical(path: &PathBuf) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

fn same(entry: &Mounted, given: &str, canonical: &Option<PathBuf>) -> bool {
    entry.given == given || (canonical.is_some() && entry.canonical == *canonical)
}

/// Mounts a vpk (the `_dir.vpk` file for split archives). Files in it are visible to material
/// loads that start after this call. The newest mount wins when several hold the same file.
pub fn mount(path: &str) -> Result<usize, String> {
    let resolved = crate::world::expand_home(path.trim());
    let canonical = canonical(&resolved);
    let mut mounted = MOUNTED.lock().unwrap();

    if mounted.iter().any(|entry| same(entry, path, &canonical)) {
        return Err(format!("{path} is already mounted"));
    }

    let vpk = Vpk::open(&resolved).map_err(|err| format!("vpk {path}: {err}"))?;
    let files = vpk.len();

    log::info!("[vpk] mounted {} ({files} files)", resolved.display());
    mounted.push(Mounted {
        given: path.to_string(),
        canonical,
        vpk: Arc::new(vpk),
    });

    Ok(files)
}

/// Unmounts by the same path given to `mount`. Returns false when it was not mounted.
pub fn unmount(path: &str) -> bool {
    let canonical = canonical(&crate::world::expand_home(path.trim()));
    let mut mounted = MOUNTED.lock().unwrap();
    let before = mounted.len();

    mounted.retain(|entry| !same(entry, path, &canonical));

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
        .any(|entry| same(entry, path, &canonical))
}

/// Reads a normalized path from the newest mount that has it.
pub fn read(key: &str) -> Option<Vec<u8>> {
    let vpks: Vec<Arc<Vpk>> = MOUNTED
        .lock()
        .unwrap()
        .iter()
        .rev()
        .map(|entry| entry.vpk.clone())
        .collect();

    vpks.iter().find_map(|vpk| vpk.read(key).ok())
}

const ENV_DIR: &str = "ENGINE_VPK_DIR";
const SCAN_DEPTH: u32 = 3;

/// Mounts every `_dir.vpk` found under the folders in `ENGINE_VPK_DIR`. Several folders are
/// separated like PATH (`:` on unix, `;` on windows). Returns how many archives were mounted.
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
