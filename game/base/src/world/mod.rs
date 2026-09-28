mod brush;
mod voxel;

use std::path::PathBuf;

pub use brush::{compile_map, BrushHit, BrushMap, CompiledMap};
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
