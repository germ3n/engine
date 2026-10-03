mod brush;
mod bspvis;
mod material;
pub mod surface;
mod voxel;

use std::path::PathBuf;

pub use brush::{compile_map, BrushEdit, BrushHit, BrushMap, BrushPlane, CompiledMap};
pub use material::{image_from_vtf, image_rgba, read_texture};
pub use surface::{
    push_shaded_tri, DrawMesh, MapGraphics, SurfaceRange, CUBEMAP_NONE, MATERIAL_NONE, PASS_OPAQUE,
    STRIDE,
};
pub use voxel::{find_voxel_file, Block, BlockPos, ChunkUpdate, Face, TraceHit, VoxelWorld};

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
