use crate::script::Realm;
use mlua::Lua;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

static FILES: OnceLock<Mutex<HashMap<String, Arc<[u8]>>>> = OnceLock::new();

static DIRTY: Mutex<Vec<String>> = Mutex::new(Vec::new());
static INBOX: Mutex<Option<Inbox>> = Mutex::new(None);

pub const CHUNK_SIZE: usize = 16 * 1024;

struct Inbox {
    path: String,
    parts: u16,
    next: u16,
    bytes: Vec<u8>,
}

fn files() -> &'static Mutex<HashMap<String, Arc<[u8]>>> {
    FILES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_string()
}

pub fn add(path: &str) -> Result<(), String> {
    let key = normalize(path);

    let bytes = crate::fs::read(&key)?;

    files()
        .lock()
        .expect("Couldn't lock lua cache")
        .insert(key.clone(), bytes.into());

    let mut dirty = DIRTY.lock().expect("Couldn't lock lua cache");

    if !dirty.contains(&key) {
        dirty.push(key);
    }

    Ok(())
}

pub fn take_dirty() -> Vec<String> {
    std::mem::take(&mut *DIRTY.lock().expect("Couldn't lock lua cache"))
}

pub fn paths() -> Vec<String> {
    files()
        .lock()
        .expect("Couldn't lock lua cache")
        .keys()
        .cloned()
        .collect()
}

pub fn chunks(path: &str) -> Option<Vec<Vec<u8>>> {
    let bytes = get(path)?;

    if bytes.is_empty() {
        return Some(vec![Vec::new()]);
    }

    Some(bytes.chunks(CHUNK_SIZE).map(<[u8]>::to_vec).collect())
}

pub fn receive(path: &str, part: u16, parts: u16, bytes: Vec<u8>) {
    let mut inbox = INBOX.lock().expect("Couldn't lock lua cache");

    if part == 0 {
        *inbox = Some(Inbox {
            path: normalize(path),
            parts,
            next: 0,
            bytes: Vec::new(),
        });
    }

    let Some(state) = inbox.as_mut() else {
        return;
    };

    if state.path != normalize(path) || state.parts != parts || state.next != part {
        log::warn!("[lua] dropped out of order chunk {part}/{parts} of {path}");
        *inbox = None;

        return;
    }

    state.bytes.extend_from_slice(&bytes);
    state.next += 1;

    if state.next < state.parts {
        return;
    }

    let done = inbox.take().expect("Inbox checked above");

    log::info!("[lua] cached {} ({} bytes)", done.path, done.bytes.len());
    files()
        .lock()
        .expect("Couldn't lock lua cache")
        .insert(done.path, done.bytes.into());
}

pub fn get(path: &str) -> Option<Arc<[u8]>> {
    files()
        .lock()
        .expect("Couldn't lock lua cache")
        .get(&normalize(path))
        .cloned()
}

pub fn contains(path: &str) -> bool {
    get(path).is_some()
}

pub fn clear() {
    files().lock().expect("Couldn't lock lua cache").clear();
}

pub fn register_cache_lib(lua: &Lua, realm: Realm) {
    if matches!(realm, Realm::Client) {
        lua.globals()
            .set(
                "add_client_file",
                lua.create_function(|_, _path: String| Ok((true, None::<String>)))
                    .expect("Failed to create add_client_file"),
            )
            .expect("Failed to set add_client_file");

        return;
    }

    lua.globals()
        .set(
            "add_client_file",
            lua.create_function(|_, path: String| {
                Ok(match add(&path) {
                    Ok(()) => (true, None),
                    Err(err) => (false, Some(err)),
                })
            })
            .expect("Failed to create add_client_file"),
        )
        .expect("Failed to set add_client_file");
}
