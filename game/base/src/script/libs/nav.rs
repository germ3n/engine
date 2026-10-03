use crate::script::libs::vector3::Vector3;
use crate::world::nav::NavState;
use mlua::Lua;
use r#macro::document;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;

pub type NavAccess = Arc<AtomicPtr<NavState>>;

pub struct NavScope<'a> {
    access: &'a AtomicPtr<NavState>,
    previous: *mut NavState,
}

impl<'a> NavScope<'a> {
    pub fn new(access: &'a AtomicPtr<NavState>, state: *mut NavState) -> Self {
        Self {
            access,
            previous: access.swap(state, Ordering::Relaxed),
        }
    }
}

impl Drop for NavScope<'_> {
    fn drop(&mut self) {
        self.access.store(self.previous, Ordering::Relaxed);
    }
}

#[document(
    kind = "library",
    name = "nav",
    realm = "server",
    summary = "Queries the server navmesh. The mesh is built with nav_build."
)]
fn nav_lib() {}

#[document(
    parent = "nav",
    name = "ready",
    kind = "function",
    realm = "server",
    summary = "True when a navmesh with walkable polygons is loaded.",
    returns = { ty = "boolean", desc = "False on the client, and before the first successful bake." },
)]
fn nav_ready() {}

#[document(
    parent = "nav",
    name = "path",
    kind = "function",
    realm = "server",
    summary = "Walks the active navmesh from start to goal.",
    params = {
        start = { ty = "Vector3", desc = "Start position in world units." },
        goal = { ty = "Vector3", desc = "Goal position in world units." },
    },
    returns = { ty = "table", desc = "Waypoint Vector3 values. Empty when no mesh or no route exists." },
    example = "local points = nav.path(self:get_pos(), target:get_pos())",
)]
fn nav_path() {}

#[document(
    parent = "nav",
    name = "set_follow",
    kind = "function",
    realm = "server",
    summary = "Publishes a polyline for nav_show to draw on clients.",
    params = {
        points = { ty = "table", desc = "Waypoint Vector3 values. Pass an empty table to clear it." },
    },
)]
fn nav_set_follow() {}

pub fn register_nav_lib(lua: &Lua, access: NavAccess, server: bool) {
    let nav = lua.create_table().expect("Failed to create nav table");
    let ready_access = access.clone();
    nav.set(
        "ready",
        lua.create_function(move |_, ()| {
            if !server {
                return Ok(false);
            }

            Ok(state_ready(&ready_access))
        })
        .expect("[nav] Failed to create ready"),
    )
    .expect("[nav] Failed setting ready");
    let path_access = access.clone();
    nav.set(
        "path",
        lua.create_function(move |_, (start, goal): (Vector3, Vector3)| {
            if !server {
                return Ok(Vec::new());
            }

            Ok(nav_query(&path_access, start, goal))
        })
        .expect("[nav] Failed to create path"),
    )
    .expect("[nav] Failed setting path");
    let follow_access = access;
    nav.set(
        "set_follow",
        lua.create_function(move |_, points: Vec<Vector3>| {
            if !server {
                return Ok(());
            }

            nav_follow(&follow_access, points);

            Ok(())
        })
        .expect("[nav] Failed to create set_follow"),
    )
    .expect("[nav] Failed setting set_follow");
    lua.globals()
        .set("nav", nav)
        .expect("[nav] Failed to set nav table");
}

fn state_ready(access: &NavAccess) -> bool {
    unsafe { access.load(Ordering::Relaxed).as_ref() }
        .map(NavState::ready)
        .unwrap_or(false)
}

fn nav_query(access: &NavAccess, start: Vector3, goal: Vector3) -> Vec<Vector3> {
    unsafe { access.load(Ordering::Relaxed).as_ref() }
        .map(|state| state.query(start, goal))
        .unwrap_or_default()
}

fn nav_follow(access: &NavAccess, points: Vec<Vector3>) {
    if let Some(state) = unsafe { access.load(Ordering::Relaxed).as_mut() } {
        state.set_follow(points);
    }
}
