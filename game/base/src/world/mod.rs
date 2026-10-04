mod brush;
mod bspvis;
pub mod gen;
mod material;
pub mod nav;
pub mod surface;
mod trace;
mod voxel;

use std::path::PathBuf;

pub use brush::{
    compile_map, texture_name_ok, BrushEdit, BrushHit, BrushMap, BrushPlane, CompiledBrush,
    CompiledEntity, CompiledMap,
};
pub use material::{image_from_vtf, image_rgba, read_texture};
pub use surface::{
    push_shaded_tri, DrawMesh, MapGraphics, SurfaceRange, CUBEMAP_NONE, MATERIAL_NONE, PASS_OPAQUE,
    STRIDE,
};
pub use trace::{HitAll, TraceFilter};
pub use voxel::{
    block_rgb, cwd_vmap_path, find_voxel_file, Block, BlockPos, ChunkPos, ChunkUpdate, Face,
    TraceHit, VoxelWorld, CHUNK_EDGE,
};

pub(crate) fn content_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("maps"));
        dirs.push(cwd.join("game/base/maps"));
        dirs.push(cwd);
    }

    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("maps"));

    dirs
}
