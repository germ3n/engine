use crate::anim::AnimAssets;
use crate::entities::list::EntityList;
use crate::lagcomp::{LagComp, LagCompAccess};
use crate::script::libs::ents::{AnimAccess, EntityAccess};
use mlua::{Error, Lua, Result};
use r#macro::document;
use std::sync::atomic::Ordering;

#[document(
    kind = "library",
    name = "lagcomp",
    realm = "server",
    summary = "Lag compensation. Every tick the server records each entity's origin, angles and all bones. Between lagcomp.start and lagcomp.finish, every entity except the shooter and the shooter's own entities is put back where the shooter saw it, so get_position, get_angles, get_bone_position and get_bone_angles answer for that moment. Only usable while a player command is running, such as inside the Move hook. A rewind left open is closed when the command ends. Do not move or remove entities while it is active."
)]
fn lagcomp_lib() {}

#[document(
    parent = "lagcomp",
    name = "start",
    kind = "function",
    realm = "server",
    summary = "Rewinds entities to the time the running command's player was seeing them, up to one second back. Does nothing when the command carries no view time or the view is already current.",
    returns = { ty = "number", desc = "How many entities were moved." },
    panics = "Errors when no player command is running or lag compensation is already active.",
    example = "hook.add(\"Move\", \"shoot\", function(ply, cmd)\n    lagcomp.start()\n    -- trace and test bones here\n    lagcomp.finish()\nend)",
    see_also = "lagcomp.finish, lagcomp.active",
)]
fn lagcomp_start() {}

#[document(
    parent = "lagcomp",
    name = "finish",
    kind = "function",
    realm = "server",
    summary = "Puts every rewound entity back to its present state. Safe to call when nothing is rewound.",
    returns = { ty = "number", desc = "How many entities were restored." },
    see_also = "lagcomp.start",
)]
fn lagcomp_finish() {}

#[document(
    parent = "lagcomp",
    name = "active",
    kind = "function",
    realm = "server",
    summary = "Whether entities are currently rewound.",
    returns = { ty = "boolean", desc = "True between start and finish." },
)]
fn lagcomp_active() {}

fn parts<'a>(
    entities: &EntityAccess,
    anims: &AnimAccess,
    lag: &LagCompAccess,
) -> Result<(&'a mut EntityList, &'a mut AnimAssets, &'a mut LagComp)> {
    let entities = unsafe { entities.load(Ordering::Relaxed).as_mut() };
    let anims = unsafe { anims.load(Ordering::Relaxed).as_mut() };
    let lag = unsafe { lag.load(Ordering::Relaxed).as_mut() };

    match (entities, anims, lag) {
        (Some(entities), Some(anims), Some(lag)) => Ok((entities, anims, lag)),
        _ => Err(Error::RuntimeError(
            "lag compensation is not available".to_string(),
        )),
    }
}

pub fn register_lagcomp_lib(
    lua: &Lua,
    entities: EntityAccess,
    anims: AnimAccess,
    lag: LagCompAccess,
) {
    let table = lua.create_table().expect("Failed to create lagcomp table");

    let (e, a, l) = (entities.clone(), anims.clone(), lag.clone());
    table
        .set(
            "start",
            lua.create_function(move |_, ()| {
                let (entities, anims, lag) = parts(&e, &a, &l)?;

                lag.start(entities, anims).map_err(Error::RuntimeError)
            })
            .expect("[lagcomp] Failed to create start"),
        )
        .expect("[lagcomp] Failed setting start");

    let (e, a, l) = (entities.clone(), anims.clone(), lag.clone());
    table
        .set(
            "finish",
            lua.create_function(move |_, ()| {
                let (entities, anims, lag) = parts(&e, &a, &l)?;

                Ok(lag.finish(entities, anims))
            })
            .expect("[lagcomp] Failed to create finish"),
        )
        .expect("[lagcomp] Failed setting finish");

    let l = lag.clone();
    table
        .set(
            "active",
            lua.create_function(move |_, ()| {
                let lag = unsafe { l.load(Ordering::Relaxed).as_ref() };

                Ok(lag.is_some_and(|lag| lag.is_active()))
            })
            .expect("[lagcomp] Failed to create active"),
        )
        .expect("[lagcomp] Failed setting active");

    lua.globals()
        .set("lagcomp", table)
        .expect("Failed to set lagcomp table");
}
