use crate::world::gen::{seed_from_f64, Noise};
use mlua::prelude::LuaUserDataMethods;
use mlua::{Error, Lua, UserData};
use r#macro::document;

struct LuaNoise(Noise);

impl UserData for LuaNoise {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("sample2", |_, this, (x, y): (f64, f64)| {
            Ok(this.0.sample2(x, y))
        });
        methods.add_method("sample3", |_, this, (x, y, z): (f64, f64, f64)| {
            Ok(this.0.sample3(x, y, z))
        });
    }
}

#[document(
    kind = "library",
    name = "noise",
    realm = "shared",
    summary = "Builds deterministic noise samplers. Every sample is a number in about -1 to 1."
)]
fn noise_lib() {}

#[document(
    parent = "noise",
    name = "new",
    kind = "function",
    realm = "shared",
    summary = "Creates a noise sampler from an integer seed.",
    params = {
        seed = { ty = "number", desc = "Seed. The same seed always rebuilds the same sampler." },
    },
    returns = { ty = "Noise", desc = "Sampler with sample2 and sample3." },
    example = "local n = noise.new(1)\nlocal value = n:sample3(x, y, z)",
)]
fn noise_new() {}

#[document(
    kind = "class",
    name = "Noise",
    realm = "shared",
    summary = "Deterministic value noise. sample2 and sample3 return numbers."
)]
fn noise_class() {}

#[document(
    parent = "Noise",
    name = "sample2",
    kind = "method",
    realm = "shared",
    summary = "Samples 2D noise.",
    params = {
        x = { ty = "number", desc = "X coordinate." },
        y = { ty = "number", desc = "Y coordinate." },
    },
    returns = { ty = "number", desc = "Value in about -1 to 1." },
)]
fn noise_sample2() {}

#[document(
    parent = "Noise",
    name = "sample3",
    kind = "method",
    realm = "shared",
    summary = "Samples 3D noise.",
    params = {
        x = { ty = "number", desc = "X coordinate." },
        y = { ty = "number", desc = "Y coordinate." },
        z = { ty = "number", desc = "Z coordinate." },
    },
    returns = { ty = "number", desc = "Value in about -1 to 1." },
)]
fn noise_sample3() {}

pub fn register_noise_lib(lua: &Lua) {
    let noise = lua.create_table().expect("Failed to create noise table");
    noise
        .set(
            "new",
            lua.create_function(|_, seed: f64| {
                let Some(seed) = seed_from_f64(seed) else {
                    return Err(Error::external("seed is invalid"));
                };

                Ok(LuaNoise(Noise::new(seed)))
            })
            .expect("[noise] Failed to create new"),
        )
        .expect("[noise] Failed setting new");
    lua.globals()
        .set("noise", noise)
        .expect("[noise] Failed to set noise table");
}
