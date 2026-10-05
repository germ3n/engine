use mlua::Lua;
use r#macro::document;

#[document(
    kind = "library",
    name = "vpk",
    realm = "shared",
    summary = "Mounts Source vpk archives at runtime. Mounted archives supply materials and textures to loads that start after the mount, so mount before loading a map that needs them. Files already loaded are not reloaded."
)]
fn vpk_lib() {}

#[document(
    parent = "vpk",
    name = "mount",
    kind = "function",
    realm = "shared",
    summary = "Mounts a vpk. The newest mount wins when several hold the same file, and mounts win over the game folder archives.",
    params = {
        path = { ty = "string", desc = "Path to the archive. For split archives this is the _dir.vpk file. A leading ~ is the home folder." },
    },
    returns = { ty = "boolean", desc = "True on success. On failure false, then a message such as the path is already mounted or cannot be opened." },
    example = "local ok, err = vpk.mount(\"tf/tf2_textures_dir.vpk\")",
)]
fn vpk_mount() {}

#[document(
    parent = "vpk",
    name = "unmount",
    kind = "function",
    realm = "shared",
    summary = "Unmounts a vpk by the path it was mounted with. A different spelling of the same file also works while the file exists.",
    params = {
        path = { ty = "string", desc = "The path given to vpk.mount." },
    },
    returns = { ty = "boolean", desc = "False when nothing was mounted at that path." },
)]
fn vpk_unmount() {}

#[document(
    parent = "vpk",
    name = "is_mounted",
    kind = "function",
    realm = "shared",
    summary = "Whether a vpk is mounted at this path.",
    params = {
        path = { ty = "string", desc = "The path given to vpk.mount." },
    },
    returns = { ty = "boolean", desc = "True when mounted." },
)]
fn vpk_is_mounted() {}

#[document(
    parent = "vpk",
    name = "discover",
    kind = "function",
    realm = "shared",
    summary = "Searches the steam libraries on this machine for _dir.vpk archives. Nothing is mounted, pass the results to vpk.mount.",
    returns = { ty = "table", desc = "A list of archive paths, sorted. Empty when steam or its games are not found." },
    example = "for _, path in ipairs(vpk.discover()) do vpk.mount(path) end",
)]
fn vpk_discover() {}

pub fn register_vpk_lib(lua: &Lua) {
    let table = lua.create_table().expect("Failed to create vpk table");

    table
        .set(
            "mount",
            lua.create_function(|_, path: String| {
                Ok(match crate::fs::vpk::mount(&path) {
                    Ok(_) => (true, None),
                    Err(err) => (false, Some(err)),
                })
            })
            .expect("[vpk] Failed to create mount"),
        )
        .expect("[vpk] Failed setting mount");

    table
        .set(
            "unmount",
            lua.create_function(|_, path: String| Ok(crate::fs::vpk::unmount(&path)))
                .expect("[vpk] Failed to create unmount"),
        )
        .expect("[vpk] Failed setting unmount");

    table
        .set(
            "is_mounted",
            lua.create_function(|_, path: String| Ok(crate::fs::vpk::is_mounted(&path)))
                .expect("[vpk] Failed to create is_mounted"),
        )
        .expect("[vpk] Failed setting is_mounted");

    table
        .set(
            "discover",
            lua.create_function(|_, ()| {
                Ok(crate::fs::vpk::discover()
                    .into_iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect::<Vec<_>>())
            })
            .expect("[vpk] Failed to create discover"),
        )
        .expect("[vpk] Failed setting discover");

    lua.globals()
        .set("vpk", table)
        .expect("Failed to set vpk table");
}
