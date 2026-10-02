use crate::entities::EntityHandle;
use crate::script::libs::vector3::Vector3;
use crate::sound::{world, SoundAccess};
use mlua::{Lua, Table};

fn first_time(lua: &Lua) -> bool
{
    let Ok(engine) = lua.globals().get::<Table>("engine") else
    {
        return true;
    };

    return engine.get::<bool>("first_time_predicted").unwrap_or(true);
}

pub fn register_sound_lib(lua: &Lua, access: SoundAccess)
{
    crate::script::exec(lua, "sound.lua", "lua/libs/sound.luac");
    let sound: Table = lua.globals().get("sound").expect("[sound] Couldn't get sound table");

    let shared = access.clone();
    sound
        .set(
            "_add",
            lua.create_function(
                move |_,
                      (name, channel, level, vol_min, vol_max, pitch_min, pitch_max, waves, bus, looping, stream): (
                    String,
                    String,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    Vec<String>,
                    String,
                    bool,
                    bool,
                )| {
                    let Some(sound) = world(&shared) else
                    {
                        return Ok(());
                    };
                    sound.add_def(
                        &name,
                        &channel,
                        level,
                        vol_min,
                        vol_max,
                        pitch_min,
                        pitch_max,
                        &waves,
                        &bus,
                        looping,
                        stream,
                    );

                    return Ok(());
                },
            )
            .expect("[sound] Failed to create _add"),
        )
        .expect("[sound] Failed setting _add");

    let shared = access.clone();
    sound
        .set(
            "_play",
            lua.create_function(
                move |lua,
                      (name, has_pos, x, y, z, volume, pitch, entity, channel): (
                    String,
                    bool,
                    f64,
                    f64,
                    f64,
                    f32,
                    f32,
                    u32,
                    String,
                )| {
                    let Some(sound) = world(&shared) else
                    {
                        return Ok(());
                    };

                    if !sound.allows_local(first_time(lua))
                    {
                        return Ok(());
                    }

                    let position = if has_pos { Some(Vector3::new(x, y, z)) } else { None };
                    sound.play(&name, position, volume, pitch, EntityHandle(entity), &channel);

                    return Ok(());
                },
            )
            .expect("[sound] Failed to create _play"),
        )
        .expect("[sound] Failed setting _play");

    let shared = access.clone();
    sound
        .set(
            "_halt",
            lua.create_function(move |lua, (entity, name): (u32, Option<String>)| {
                let Some(sound) = world(&shared) else
                {
                    return Ok(());
                };

                if !sound.allows_local(first_time(lua))
                {
                    return Ok(());
                }

                sound.stop(EntityHandle(entity), name.as_deref());

                return Ok(());
            })
            .expect("[sound] Failed to create _halt"),
        )
        .expect("[sound] Failed setting _halt");

    let shared = access.clone();
    sound
        .set(
            "_scape_add",
            lua.create_function(move |_, (name, room, sounds): (String, String, Vec<String>)| {
                let Some(sound) = world(&shared) else
                {
                    return Ok(());
                };
                sound.add_scape(&name, &room, &sounds);

                return Ok(());
            })
            .expect("[sound] Failed to create _scape_add"),
        )
        .expect("[sound] Failed setting _scape_add");

    let shared = access.clone();
    sound
        .set(
            "_scape_box",
            lua.create_function(
                move |_, (name, minx, miny, minz, maxx, maxy, maxz): (String, f64, f64, f64, f64, f64, f64)| {
                    let Some(sound) = world(&shared) else
                    {
                        return Ok(());
                    };
                    sound.add_box(
                        &name,
                        Vector3::new(minx, miny, minz),
                        Vector3::new(maxx, maxy, maxz),
                    );

                    return Ok(());
                },
            )
            .expect("[sound] Failed to create _scape_box"),
        )
        .expect("[sound] Failed setting _scape_box");

    let shared = access.clone();
    sound
        .set(
            "_set_room",
            lua.create_function(move |_, name: String| {
                let Some(sound) = world(&shared) else
                {
                    return Ok(());
                };
                sound.set_room(&name);

                return Ok(());
            })
            .expect("[sound] Failed to create _set_room"),
        )
        .expect("[sound] Failed setting _set_room");
}
