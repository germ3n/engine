use crate::world::gen::{Biome, GenSettings};
use crate::world::Block;
use mlua::{Error, Lua, Table};
use r#macro::document;
use std::sync::{Arc, Mutex};

#[document(
    kind = "library",
    name = "biome",
    realm = "server",
    summary = "Registers temperature biomes used by voxel generation. The first range that contains a temperature wins."
)]
fn biome_lib() {}

#[document(
    parent = "biome",
    name = "add",
    kind = "function",
    realm = "server",
    summary = "Adds a biome, or replaces one with the same name. Later chunks use the new list. Fields are name, temp_min, temp_max, surface, soil, stone, height, and optional liquid and trees.",
    params = {
        def = { ty = "table", desc = "Biome definition." },
    },
    returns = { ty = "boolean", desc = "False when the caller is not the server or the definition is invalid." },
    example = "biome.add({ name = \"marsh\", temp_min = 0.42, temp_max = 0.5, surface = 3, soil = 2, stone = 1, height = 8, trees = 0 })",
)]
fn biome_add() {}

#[document(
    parent = "biome",
    name = "clear",
    kind = "function",
    realm = "server",
    summary = "Removes every biome, including the builtins, so a script can register its own list.",
    returns = { ty = "boolean", desc = "False when the caller is not the server." },
)]
fn biome_clear() {}

pub fn register_biome_lib(lua: &Lua, settings: Arc<Mutex<GenSettings>>, server: bool) {
    let biome = lua.create_table().expect("Failed to create biome table");
    let gen = Arc::clone(&settings);
    biome
        .set(
            "add",
            lua.create_function(move |_, def: Table| {
                if !server {
                    return Ok(false);
                }

                let biome = read_biome(def)?;

                Ok(gen.lock().expect("gen settings").add_biome(biome))
            })
            .expect("[biome] Failed to create add"),
        )
        .expect("[biome] Failed setting add");
    let gen = settings;
    biome
        .set(
            "clear",
            lua.create_function(move |_, ()| {
                if !server {
                    return Ok(false);
                }

                gen.lock().expect("gen settings").clear_biomes();

                Ok(true)
            })
            .expect("[biome] Failed to create clear"),
        )
        .expect("[biome] Failed setting clear");
    lua.globals()
        .set("biome", biome)
        .expect("[biome] Failed to set biome table");
}

fn read_biome(table: Table) -> mlua::Result<Biome> {
    let name: String = table.get("name")?;
    let temp_min: f64 = table.get("temp_min")?;
    let temp_max: f64 = table.get("temp_max")?;
    let surface = block_id(table.get("surface")?)?;
    let soil = block_id(table.get("soil")?)?;
    let stone = block_id(table.get("stone")?)?;
    let liquid = match table.get::<Option<f64>>("liquid")? {
        Some(id) => block_id(id)?,
        None => Block::WATER.0,
    };
    let height: f64 = table.get("height")?;
    let trees = table.get::<Option<f64>>("trees")?.unwrap_or(0.0);

    Ok(Biome {
        name,
        temp_min,
        temp_max,
        surface,
        soil,
        stone,
        liquid,
        height,
        trees,
    })
}

fn block_id(value: f64) -> mlua::Result<u16> {
    if !value.is_finite() || value < 0.0 || value > u16::MAX as f64 {
        return Err(Error::external("block id is invalid"));
    }

    Ok(value as u16)
}
