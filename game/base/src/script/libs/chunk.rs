use crate::world::gen::ChunkHandle;
use crate::world::CHUNK_EDGE;
use mlua::prelude::LuaUserDataMethods;
use mlua::UserData;
use r#macro::document;

#[document(
    kind = "class",
    name = "Chunk",
    realm = "server",
    summary = "One generated voxel chunk before it is inserted. Local coordinates are 0 to 15. Edit it from VoxelChunkGenerated and return nil so later hooks still run."
)]
fn chunk_class() {}

#[document(
    parent = "hook",
    name = "VoxelChunkGenerated",
    kind = "hook",
    realm = "server",
    summary = "Called on the server after a chunk is generated and before it is inserted. The chunk userdata can change blocks. Return nil so other hooks still run. A hook error still commits the chunk.",
    params = {
        chunk = { ty = "Chunk", desc = "Generated chunk. get and set use local block coordinates." },
    },
    returns = { ty = "nil", desc = "Return nil so later callbacks run." },
    example = "hook.add(\"VoxelChunkGenerated\", \"moss\", function(chunk)\n  chunk:set(0, 0, 0, 3)\nend)",
)]
fn voxel_chunk_generated() {}

#[document(
    parent = "Chunk",
    name = "get",
    kind = "method",
    realm = "server",
    summary = "Block id at local coordinates.",
    params = {
        x = { ty = "number", desc = "Local X, 0 to 15." },
        y = { ty = "number", desc = "Local Y, 0 to 15." },
        z = { ty = "number", desc = "Local Z, 0 to 15." },
    },
    returns = { ty = "number", desc = "Block id, or nil when the coordinate is outside the chunk." },
)]
fn chunk_get() {}

#[document(
    parent = "Chunk",
    name = "set",
    kind = "method",
    realm = "server",
    summary = "Writes a block id at local coordinates.",
    params = {
        x = { ty = "number", desc = "Local X, 0 to 15." },
        y = { ty = "number", desc = "Local Y, 0 to 15." },
        z = { ty = "number", desc = "Local Z, 0 to 15." },
        block = { ty = "number", desc = "Block id. 0 is air." },
    },
    returns = { ty = "boolean", desc = "False when the coordinate or block id is invalid." },
)]
fn chunk_set() {}

#[document(
    parent = "Chunk",
    name = "temperature",
    kind = "method",
    realm = "server",
    summary = "Temperature used to pick the biome for a column in this chunk.",
    params = {
        x = { ty = "number", desc = "Local X, 0 to 15." },
        y = { ty = "number", desc = "Local Y, 0 to 15." },
    },
    returns = { ty = "number", desc = "Temperature from 0 to 1, or nil when the column is outside the chunk." },
)]
fn chunk_temperature() {}

#[document(
    parent = "Chunk",
    name = "biome",
    kind = "method",
    realm = "server",
    summary = "Biome name for a column in this chunk.",
    params = {
        x = { ty = "number", desc = "Local X, 0 to 15." },
        y = { ty = "number", desc = "Local Y, 0 to 15." },
    },
    returns = { ty = "string", desc = "Biome name, or nil when the column is outside the chunk." },
)]
fn chunk_biome() {}

impl UserData for ChunkHandle {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("x", |_, this, ()| Ok(this.0.lock().expect("chunk").pos.x));
        methods.add_method("y", |_, this, ()| Ok(this.0.lock().expect("chunk").pos.y));
        methods.add_method("z", |_, this, ()| Ok(this.0.lock().expect("chunk").pos.z));
        methods.add_method("get", |_, this, (x, y, z): (f64, f64, f64)| {
            let draft = this.0.lock().expect("chunk");
            let Some(slot) = block_slot(x, y, z) else {
                return Ok(None);
            };

            Ok(Some(draft.blocks[slot] as f64))
        });
        methods.add_method("set", |_, this, (x, y, z, block): (f64, f64, f64, f64)| {
            let Some(slot) = block_slot(x, y, z) else {
                return Ok(false);
            };
            let Some(id) = block_id(block) else {
                return Ok(false);
            };
            let mut draft = this.0.lock().expect("chunk");
            draft.blocks[slot] = id;

            Ok(true)
        });
        methods.add_method("temperature", |_, this, (x, y): (f64, f64)| {
            let draft = this.0.lock().expect("chunk");
            let Some(slot) = column_slot(x, y) else {
                return Ok(None);
            };

            Ok(Some(draft.temperature[slot]))
        });
        methods.add_method("biome", |_, this, (x, y): (f64, f64)| {
            let draft = this.0.lock().expect("chunk");
            let Some(slot) = column_slot(x, y) else {
                return Ok(None);
            };

            Ok(Some(draft.biome[slot].clone()))
        });
    }
}

fn column_slot(x: f64, y: f64) -> Option<usize> {
    let x = local_coord(x)?;
    let y = local_coord(y)?;

    Some((x + y * CHUNK_EDGE) as usize)
}

fn block_slot(x: f64, y: f64, z: f64) -> Option<usize> {
    let x = local_coord(x)?;
    let y = local_coord(y)?;
    let z = local_coord(z)?;

    Some((x + y * CHUNK_EDGE + z * CHUNK_EDGE * CHUNK_EDGE) as usize)
}

fn local_coord(value: f64) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }

    let coord = value as i32;

    if coord as f64 != value || coord < 0 || coord >= CHUNK_EDGE {
        return None;
    }

    Some(coord)
}

fn block_id(value: f64) -> Option<u16> {
    if !value.is_finite() || value < 0.0 || value > u16::MAX as f64 {
        return None;
    }

    Some(value as u16)
}
