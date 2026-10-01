use crate::script::{pick_scripts, run_file, Realm};
use mlua::Lua;

const AUTORUN_PREFIX: &str = "lua/autorun/";

pub fn load_autorun(lua: &Lua, realm: Realm) {
    let Some(fs) = crate::fs::try_global() else {
        return;
    };

    let realm_dir = match realm {
        Realm::Server => "server",
        Realm::Client => "client",
        Realm::Menu => return,
    };
    let mut shared = Vec::new();
    let mut realm_files = Vec::new();

    for path in fs.list_prefix(AUTORUN_PREFIX) {
        let Some(rest) = path.strip_prefix(AUTORUN_PREFIX) else {
            continue;
        };

        let mut parts = rest.split('/');

        match (parts.next(), parts.next(), parts.next()) {
            (Some(_file), None, None) => shared.push(path.clone()),
            (Some(dir), Some(_file), None) if dir == realm_dir => realm_files.push(path.clone()),
            _ => {}
        }
    }

    let mut scripts = pick_scripts(&fs, &shared);
    scripts.extend(pick_scripts(&fs, &realm_files));

    for path in &scripts {
        match run_file(lua, path) {
            Ok(()) => log::info!("[autorun] ran {path}"),
            Err(err) => log::error!("[autorun] {path}: {err}"),
        }
    }
}
