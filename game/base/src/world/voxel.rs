use super::{HitAll, TraceFilter};
use crate::script::libs::vector3::Vector3;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use wincode::{SchemaRead, SchemaWrite};

pub const CHUNK_EDGE: i32 = 16;
const MAX_FILL: i64 = 1_000_000;
const VOLUME: usize = (CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE) as usize;
const VOXEL_MAGIC: &[u8; 4] = b"VMAP";
const VOXEL_VERSION_V1: u32 = 1;
const VOXEL_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Block(pub u16);

impl Block {
    pub const AIR: Self = Self(0);
    pub const STONE: Self = Self(1);
    pub const DIRT: Self = Self(2);
    pub const GRASS: Self = Self(3);
    pub const SAND: Self = Self(4);
    pub const SANDSTONE: Self = Self(5);
    pub const SNOW: Self = Self(6);
    pub const WATER: Self = Self(7);
    pub const LOG: Self = Self(8);
    pub const LEAVES: Self = Self(9);

    #[inline]
    pub const fn is_air(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn is_solid(self) -> bool {
        self.0 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn from_world(point: Vector3) -> Self {
        Self {
            x: floor_i32(point.x),
            y: floor_i32(point.y),
            z: floor_i32(point.z),
        }
    }

    pub fn chunk(self) -> ChunkPos {
        ChunkPos {
            x: div_floor(self.x, CHUNK_EDGE),
            y: div_floor(self.y, CHUNK_EDGE),
            z: div_floor(self.z, CHUNK_EDGE),
        }
    }

    pub fn local(self) -> (i32, i32, i32) {
        (
            rem_floor(self.x, CHUNK_EDGE),
            rem_floor(self.y, CHUNK_EDGE),
            rem_floor(self.z, CHUNK_EDGE),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    NegX,
    PosX,
    NegY,
    PosY,
    NegZ,
    PosZ,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraceHit {
    pub block: BlockPos,
    pub face: Option<Face>,
    pub distance: f64,
    pub position: Vector3,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq, Eq)]
pub struct ChunkRun {
    pub block: u16,
    pub len: u16,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
struct StoredVoxelsV1 {
    scale: f64,
    chunks: Vec<ChunkUpdate>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
struct StoredVoxelsV2 {
    scale: f64,
    seed: i64,
    chunks: Vec<ChunkUpdate>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq, Eq)]
pub struct ChunkUpdate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub runs: Vec<ChunkRun>,
}

struct Chunk {
    blocks: Box<[u16]>,
    mesh: Option<Vec<f32>>,
    generated: bool,
}

impl Chunk {
    fn empty() -> Self {
        Self {
            blocks: vec![0; VOLUME].into_boxed_slice(),
            mesh: None,
            generated: false,
        }
    }

    fn index(local_x: i32, local_y: i32, local_z: i32) -> usize {
        (local_x + local_y * CHUNK_EDGE + local_z * CHUNK_EDGE * CHUNK_EDGE) as usize
    }

    fn is_empty(&self) -> bool {
        self.blocks.iter().all(|block| *block == 0)
    }

    fn runs(&self) -> Vec<ChunkRun> {
        let mut runs = Vec::new();
        let mut idx = 0;

        while idx < VOLUME {
            let block = self.blocks[idx];
            let mut end = idx + 1;

            while end < VOLUME && self.blocks[end] == block {
                end += 1;
            }

            runs.push(ChunkRun {
                block,
                len: (end - idx) as u16,
            });
            idx = end;
        }

        runs
    }
}

struct Axis {
    cell: i32,
    step: i32,
    t_max: f64,
    t_delta: f64,
}

pub struct VoxelWorld {
    chunks: HashMap<ChunkPos, Chunk>,
    dirty: HashSet<ChunkPos>,
    nonsolid: HashSet<u16>,
    scale: f64,
    seed: i64,
    scale_dirty: bool,
    revision: u64,
    mesh_builds: u64,
}

impl VoxelWorld {
    pub fn new() -> Self {
        Self::with_scale(1.0)
    }

    pub fn with_scale(scale: f64) -> Self {
        Self {
            chunks: HashMap::new(),
            dirty: HashSet::new(),
            nonsolid: default_nonsolid(),
            scale: finite_scale(scale).unwrap_or(1.0),
            seed: 1,
            scale_dirty: false,
            revision: 0,
            mesh_builds: 0,
        }
    }

    pub fn seed(&self) -> i64 {
        self.seed
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = seed;
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    pub fn set_scale(&mut self, scale: f64) -> bool {
        self.commit_scale(scale, true)
    }

    pub fn apply_scale(&mut self, scale: f64) -> bool {
        self.commit_scale(scale, false)
    }

    fn commit_scale(&mut self, scale: f64, dirty: bool) -> bool {
        let Some(scale) = finite_scale(scale) else {
            return false;
        };

        if scale == self.scale {
            return true;
        }

        let ratio = scale / self.scale;
        self.scale = scale;
        self.scale_meshes(ratio);
        self.scale_dirty = dirty;
        self.touch();

        true
    }

    fn scale_meshes(&mut self, ratio: f64) {
        let ratio = ratio as f32;

        for chunk in self.chunks.values_mut() {
            let Some(mesh) = chunk.mesh.as_mut() else {
                continue;
            };
            let mut vert = 0;

            while vert + super::STRIDE <= mesh.len() {
                mesh[vert] *= ratio;
                mesh[vert + 1] *= ratio;
                mesh[vert + 2] *= ratio;
                vert += super::STRIDE;
            }
        }
    }

    pub fn take_scale(&mut self) -> Option<f64> {
        if !self.scale_dirty {
            return None;
        }

        self.scale_dirty = false;

        Some(self.scale)
    }

    pub fn block_at(&self, point: Vector3) -> BlockPos {
        let scale = self.scale;

        BlockPos::from_world(Vector3::new(
            point.x / scale,
            point.y / scale,
            point.z / scale,
        ))
    }

    pub fn clear(&mut self) {
        let positions: Vec<ChunkPos> = self.chunks.keys().copied().collect();
        self.chunks.clear();
        self.dirty.clear();
        let mut idx = 0;

        while idx < positions.len() {
            self.dirty.insert(positions[idx]);
            idx += 1;
        }

        if self.scale != 1.0 {
            self.scale = 1.0;
            self.scale_dirty = true;
        }

        self.touch();
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn contains_chunk(&self, pos: ChunkPos) -> bool {
        self.chunks.contains_key(&pos)
    }

    pub fn get(&self, pos: BlockPos) -> Block {
        let Some(chunk) = self.chunks.get(&pos.chunk()) else {
            return Block::AIR;
        };

        let (local_x, local_y, local_z) = pos.local();

        Block(chunk.blocks[Chunk::index(local_x, local_y, local_z)])
    }

    pub fn is_solid(&self, pos: BlockPos) -> bool {
        let block = self.get(pos);

        block.is_solid() && !self.nonsolid.contains(&block.0)
    }

    pub fn nav_blocks(&self) -> Vec<super::nav::NavBlock> {
        let mut out = Vec::new();
        let scale = self.scale;

        for (pos, chunk) in &self.chunks {
            let mut idx = 0;

            while idx < VOLUME {
                let id = chunk.blocks[idx];

                if id != 0 {
                    let water = id == Block::WATER.0;
                    let solid = !self.nonsolid.contains(&id);

                    if water || solid {
                        let local_z = idx as i32 / (CHUNK_EDGE * CHUNK_EDGE);
                        let rem = idx as i32 % (CHUNK_EDGE * CHUNK_EDGE);
                        let local_y = rem / CHUNK_EDGE;
                        let local_x = rem % CHUNK_EDGE;
                        let x = pos.x * CHUNK_EDGE + local_x;
                        let y = pos.y * CHUNK_EDGE + local_y;
                        let z = pos.z * CHUNK_EDGE + local_z;
                        let min = Vector3::new(x as f64 * scale, y as f64 * scale, z as f64 * scale);
                        out.push(super::nav::NavBlock {
                            min,
                            max: Vector3::new(min.x + scale, min.y + scale, min.z + scale),
                            water,
                        });
                    }
                }

                idx += 1;
            }
        }

        out
    }

    pub fn occludes(&self, pos: BlockPos) -> bool {
        !self.get(pos).is_air()
    }

    pub fn set_block_solid(&mut self, id: u16, solid: bool) {
        if id == 0 {
            return;
        }

        if solid {
            self.nonsolid.remove(&id);

            return;
        }

        self.nonsolid.insert(id);
    }

    pub fn replace_nonsolid(&mut self, ids: &[u16]) {
        self.nonsolid.clear();
        let mut idx = 0;

        while idx < ids.len() {
            if ids[idx] != 0 {
                self.nonsolid.insert(ids[idx]);
            }

            idx += 1;
        }
    }

    pub fn set(&mut self, pos: BlockPos, block: Block) {
        if self.get(pos) == block {
            return;
        }

        let chunk_pos = pos.chunk();
        let (local_x, local_y, local_z) = pos.local();
        self.invalidate_edit(chunk_pos, (local_x, local_y, local_z));
        self.pin(chunk_pos);
        let slot = Chunk::index(local_x, local_y, local_z);

        if block.is_air() {
            let empty = {
                let Some(chunk) = self.chunks.get_mut(&chunk_pos) else {
                    return;
                };

                chunk.blocks[slot] = 0;

                chunk.is_empty()
            };

            self.dirty.insert(chunk_pos);

            if empty {
                self.chunks.remove(&chunk_pos);
            }

            self.touch();

            return;
        }

        let chunk = self.chunks.entry(chunk_pos).or_insert_with(Chunk::empty);
        chunk.blocks[slot] = block.0;
        self.dirty.insert(chunk_pos);
        self.touch();
    }

    pub fn fill(&mut self, min: BlockPos, max: BlockPos, block: Block) {
        let mut z = min.z;

        while z < max.z {
            let mut y = min.y;

            while y < max.y {
                let mut x = min.x;

                while x < max.x {
                    self.set(BlockPos::new(x, y, z), block);
                    x += 1;
                }

                y += 1;
            }

            z += 1;
        }
    }

    pub fn block_world(&self, point: Vector3) -> Block {
        let Some(pos) = self.block_of(point, false) else {
            return Block::AIR;
        };

        self.get(pos)
    }

    pub fn set_at(&mut self, point: Vector3, block: Block) -> bool {
        let Some(pos) = self.block_of(point, false) else {
            return false;
        };

        self.set(pos, block);

        true
    }

    pub fn fill_bounds(&mut self, min: Vector3, max: Vector3, block: Block) -> bool {
        let Some((min, max)) = self.world_box_blocks(min, max) else {
            return false;
        };

        self.fill(min, max, block);

        true
    }

    pub fn fill_sphere(&mut self, center: Vector3, radius: f64, block: Block) -> bool {
        if !is_finite(center) || !radius.is_finite() || radius <= 0.0 {
            return false;
        }

        let min = Vector3::new(center.x - radius, center.y - radius, center.z - radius);
        let max = Vector3::new(center.x + radius, center.y + radius, center.z + radius);
        let Some((min, max)) = self.world_box_blocks(min, max) else {
            return false;
        };
        let mut z = min.z;

        while z < max.z {
            let mut y = min.y;

            while y < max.y {
                let mut x = min.x;

                while x < max.x {
                    let pos = BlockPos::new(x, y, z);

                    if cell_hits_sphere(pos, center, radius, self.scale) {
                        self.set(pos, block);
                    }

                    x += 1;
                }

                y += 1;
            }

            z += 1;
        }

        true
    }

    fn world_box_blocks(&self, min: Vector3, max: Vector3) -> Option<(BlockPos, BlockPos)> {
        let a = self.block_of(min, false)?;
        let b = self.block_of(max, true)?;
        let min = BlockPos::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
        let max = BlockPos::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
        let volume = span(min.x, max.x)
            .saturating_mul(span(min.y, max.y))
            .saturating_mul(span(min.z, max.z));

        if volume > MAX_FILL {
            return None;
        }

        Some((min, max))
    }

    fn block_of(&self, point: Vector3, end: bool) -> Option<BlockPos> {
        Some(BlockPos::new(
            axis_block(point.x, self.scale, end)?,
            axis_block(point.y, self.scale, end)?,
            axis_block(point.z, self.scale, end)?,
        ))
    }

    pub fn apply(&mut self, update: &ChunkUpdate) -> bool {
        let pos = ChunkPos {
            x: update.x,
            y: update.y,
            z: update.z,
        };

        if update.runs.is_empty() {
            self.invalidate_around(pos, None);

            if self.chunks.remove(&pos).is_some() {
                self.touch();
            }

            return true;
        }

        let mut total = 0usize;

        for run in &update.runs {
            total = total.saturating_add(run.len as usize);
        }

        if total != VOLUME {
            return false;
        }

        let mut blocks = vec![0u16; VOLUME];
        let mut cursor = 0;

        for run in &update.runs {
            let end = cursor + run.len as usize;
            blocks[cursor..end].fill(run.block);
            cursor = end;
        }

        self.invalidate_around(pos, Some(&blocks));

        if blocks.iter().all(|block| *block == 0) {
            if self.chunks.remove(&pos).is_some() {
                self.touch();
            }

            return true;
        }

        self.chunks.insert(
            pos,
            Chunk {
                blocks: blocks.into_boxed_slice(),
                mesh: None,
                generated: false,
            },
        );
        self.touch();

        true
    }

    pub fn insert_generated(&mut self, pos: ChunkPos, blocks: Vec<u16>) -> bool {
        if blocks.len() != VOLUME || self.chunks.contains_key(&pos) {
            return false;
        }

        if blocks.iter().all(|block| *block == 0) {
            return true;
        }

        self.invalidate_around(pos, Some(&blocks));
        self.chunks.insert(
            pos,
            Chunk {
                blocks: blocks.into_boxed_slice(),
                mesh: None,
                generated: true,
            },
        );
        self.dirty.insert(pos);
        self.touch();

        true
    }

    pub fn clear_generated(&mut self) {
        let positions: Vec<ChunkPos> = self
            .chunks
            .iter()
            .filter(|(_, chunk)| chunk.generated)
            .map(|(pos, _)| *pos)
            .collect();
        let mut idx = 0;

        while idx < positions.len() {
            let pos = positions[idx];
            self.invalidate_around(pos, None);
            self.chunks.remove(&pos);
            self.dirty.insert(pos);
            idx += 1;
        }

        if !positions.is_empty() {
            self.touch();
        }
    }

    fn pin(&mut self, pos: ChunkPos) {
        let Some(chunk) = self.chunks.get_mut(&pos) else {
            return;
        };

        chunk.generated = false;
    }

    pub fn mesh(&mut self) -> Vec<f32> {
        self.mesh_at(Vector3::new(0.0, 0.0, 0.0))
    }

    pub fn mesh_at(&mut self, origin: Vector3) -> Vec<f32> {
        self.build_meshes(usize::MAX);

        self.assembled_mesh(origin)
    }

    pub fn build_meshes_for(&mut self, budget: std::time::Duration) -> bool {
        let pending: Vec<ChunkPos> = self
            .chunks
            .iter()
            .filter(|(_, chunk)| chunk.mesh.is_none())
            .map(|(pos, _)| *pos)
            .collect();
        let start = std::time::Instant::now();
        let mut built = 0;

        for pos in pending {
            if built > 0 && start.elapsed() >= budget {
                return true;
            }

            self.ensure_mesh(pos);
            built += 1;
        }

        false
    }

    pub fn build_meshes(&mut self, budget: usize) -> bool {
        let positions: Vec<ChunkPos> = self.chunks.keys().copied().collect();
        let mut built = 0;
        let mut idx = 0;

        while idx < positions.len() {
            let pending = self
                .chunks
                .get(&positions[idx])
                .map(|chunk| chunk.mesh.is_none())
                .unwrap_or(false);

            if pending {
                if built >= budget {
                    return true;
                }

                self.ensure_mesh(positions[idx]);
                built += 1;
            }

            idx += 1;
        }

        false
    }

    pub fn assembled_mesh(&self, origin: Vector3) -> Vec<f32> {
        let mut vertices = Vec::new();

        for chunk in self.chunks.values() {
            if let Some(mesh) = chunk.mesh.as_deref() {
                append_shifted(&mut vertices, mesh, origin);
            }
        }

        vertices
    }

    pub fn mesh_box(&mut self, origin: Vector3, min: Vector3, max: Vector3) -> Vec<f32> {
        let scale = self.scale;
        let mut positions = Vec::new();

        for pos in self.chunks.keys().copied() {
            if chunk_overlaps(pos, scale, min, max) {
                positions.push(pos);
            }
        }

        self.gather_mesh(&positions, origin)
    }

    fn gather_mesh(&mut self, positions: &[ChunkPos], origin: Vector3) -> Vec<f32> {
        let mut idx = 0;

        while idx < positions.len() {
            self.ensure_mesh(positions[idx]);
            idx += 1;
        }

        let mut vertices = Vec::new();
        idx = 0;

        while idx < positions.len() {
            if let Some(mesh) = self
                .chunks
                .get(&positions[idx])
                .and_then(|chunk| chunk.mesh.as_deref())
            {
                append_shifted(&mut vertices, mesh, origin);
            }

            idx += 1;
        }

        vertices
    }

    fn ensure_mesh(&mut self, pos: ChunkPos) {
        let ready = self
            .chunks
            .get(&pos)
            .map(|chunk| chunk.mesh.is_some())
            .unwrap_or(false);

        if ready {
            return;
        }

        if !self.chunks.contains_key(&pos) {
            return;
        }

        let mesh = self.build_chunk_mesh(pos);
        let Some(chunk) = self.chunks.get_mut(&pos) else {
            return;
        };

        chunk.mesh = Some(mesh);
        self.mesh_builds += 1;
    }

    fn build_chunk_mesh(&self, chunk_pos: ChunkPos) -> Vec<f32> {
        let mut vertices = Vec::new();
        let scale = self.scale;

        self.chunk_quads(chunk_pos, |face, id, base, ext| {
            push_quad(&mut vertices, face, id, base, ext, scale);
        });

        vertices
    }

    fn chunk_quads(&self, chunk_pos: ChunkPos, mut emit: impl FnMut(usize, u16, [i32; 3], [i32; 3])) {
        let Some(chunk) = self.chunks.get(&chunk_pos) else {
            return;
        };
        let blocks = &chunk.blocks[..];
        let edge = CHUNK_EDGE as usize;
        let origin = [
            chunk_pos.x * CHUNK_EDGE,
            chunk_pos.y * CHUNK_EDGE,
            chunk_pos.z * CHUNK_EDGE,
        ];
        let mut mask = vec![0u16; edge * edge];

        for face in 0..6 {
            let axis = face / 2;
            let positive = face % 2 == 0;
            let (u_axis, v_axis) = match axis {
                0 => (1, 2),
                1 => (0, 2),
                _ => (0, 1),
            };
            let (nx, ny, nz) = NEIGHBORS[face];
            let across = self.chunks.get(&ChunkPos {
                x: chunk_pos.x + nx,
                y: chunk_pos.y + ny,
                z: chunk_pos.z + nz,
            });
            let across = across.map(|chunk| &chunk.blocks[..]);

            for slice in 0..edge {
                let next = slice as i32 + if positive { 1 } else { -1 };
                let mut any = false;

                for v in 0..edge {
                    for u in 0..edge {
                        let mut local = [0usize; 3];
                        local[axis] = slice;
                        local[u_axis] = u;
                        local[v_axis] = v;
                        let id = blocks[local_index(local)];
                        let mut visible = id != 0;

                        if visible {
                            let mut beyond = local;

                            visible = if next >= 0 && (next as usize) < edge {
                                beyond[axis] = next as usize;

                                blocks[local_index(beyond)] == 0
                            } else {
                                beyond[axis] = if positive { 0 } else { edge - 1 };

                                match across {
                                    Some(other) => other[local_index(beyond)] == 0,
                                    None => true,
                                }
                            };
                        }

                        mask[v * edge + u] = if visible { id } else { 0 };
                        any |= visible;
                    }
                }

                if !any {
                    continue;
                }

                for v in 0..edge {
                    let mut u = 0;

                    while u < edge {
                        let id = mask[v * edge + u];

                        if id == 0 {
                            u += 1;

                            continue;
                        }

                        let mut width = 1;

                        while u + width < edge && mask[v * edge + u + width] == id {
                            width += 1;
                        }

                        let mut height = 1;

                        'grow: while v + height < edge {
                            for k in 0..width {
                                if mask[(v + height) * edge + u + k] != id {
                                    break 'grow;
                                }
                            }

                            height += 1;
                        }

                        for dv in 0..height {
                            mask[(v + dv) * edge + u..(v + dv) * edge + u + width].fill(0);
                        }

                        let mut base = origin;
                        base[axis] += slice as i32;
                        base[u_axis] += u as i32;
                        base[v_axis] += v as i32;
                        let mut ext = [1i32; 3];
                        ext[u_axis] = width as i32;
                        ext[v_axis] = height as i32;
                        emit(face, id, base, ext);
                        u += width;
                    }
                }
            }
        }
    }

    fn invalidate_around(&mut self, pos: ChunkPos, new: Option<&[u16]>) {
        self.drop_mesh(pos);
        let old = self
            .chunks
            .get(&pos)
            .map(|chunk| boundary_layers(&chunk.blocks))
            .unwrap_or([false; 6]);
        let new = new.map(boundary_layers).unwrap_or([false; 6]);
        let mut idx = 0;

        while idx < NEIGHBORS.len() {
            if old[idx] || new[idx] {
                let (x, y, z) = NEIGHBORS[idx];
                self.drop_mesh(ChunkPos {
                    x: pos.x + x,
                    y: pos.y + y,
                    z: pos.z + z,
                });
            }

            idx += 1;
        }
    }

    fn invalidate_edit(&mut self, pos: ChunkPos, local: (i32, i32, i32)) {
        self.drop_mesh(pos);
        let last = CHUNK_EDGE - 1;
        let touching = [
            local.0 == last,
            local.0 == 0,
            local.1 == last,
            local.1 == 0,
            local.2 == last,
            local.2 == 0,
        ];
        let mut idx = 0;

        while idx < NEIGHBORS.len() {
            if touching[idx] {
                let (x, y, z) = NEIGHBORS[idx];
                self.drop_mesh(ChunkPos {
                    x: pos.x + x,
                    y: pos.y + y,
                    z: pos.z + z,
                });
            }

            idx += 1;
        }
    }

    fn drop_mesh(&mut self, pos: ChunkPos) {
        let Some(chunk) = self.chunks.get_mut(&pos) else {
            return;
        };

        chunk.mesh = None;
    }

    pub fn baseline(&self) -> Vec<ChunkUpdate> {
        let mut updates = Vec::with_capacity(self.chunks.len());

        for pos in self.chunks.keys() {
            updates.push(self.encode(*pos));
        }

        updates
    }

    pub fn take_dirty_limited(&mut self, limit: usize) -> Vec<ChunkUpdate> {
        let picked: Vec<ChunkPos> = self.dirty.iter().take(limit).copied().collect();
        let mut updates = Vec::with_capacity(picked.len());

        for pos in picked {
            self.dirty.remove(&pos);
            updates.push(self.encode(pos));
        }

        updates
    }

    pub fn mark_dirty(&mut self, pos: ChunkPos) {
        self.dirty.insert(pos);
    }

    pub fn take_dirty(&mut self) -> Vec<ChunkUpdate> {
        let dirty = std::mem::take(&mut self.dirty);
        let mut updates = Vec::with_capacity(dirty.len());

        for pos in dirty {
            updates.push(self.encode(pos));
        }

        updates
    }

    pub fn save_file(&self, path: &Path) -> Result<(), String> {
        let bytes = encode_voxels(self.scale, self.seed, &self.baseline())?;

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("voxels {}: {err}", parent.display()))?;
            }
        }

        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &bytes).map_err(|err| format!("voxels {}: {err}", tmp.display()))?;
        let _ = std::fs::remove_file(path);
        std::fs::rename(&tmp, path).map_err(|err| format!("voxels {}: {err}", path.display()))?;

        Ok(())
    }

    pub fn load_file(&mut self, path: &Path) -> Result<(), String> {
        let bytes =
            std::fs::read(path).map_err(|err| format!("voxels {}: {err}", path.display()))?;

        self.load_bytes(&bytes)
    }

    pub fn load_resume(&mut self, path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }

        let bytes =
            std::fs::read(path).map_err(|err| format!("voxels {}: {err}", path.display()))?;
        let version = voxel_version(&bytes)?;

        if version != VOXEL_VERSION {
            return Ok(false);
        }

        self.load_bytes(&bytes)?;

        Ok(true)
    }

    fn load_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let decoded = decode_voxels(bytes)?;

        if finite_scale(decoded.scale).is_none() {
            return Err("voxel map scale is invalid".to_string());
        }

        let mut loaded = VoxelWorld::with_scale(decoded.scale);
        loaded.nonsolid = self.nonsolid.clone();
        loaded.seed = if decoded.version == VOXEL_VERSION_V1 {
            self.seed
        } else {
            decoded.seed
        };

        for update in &decoded.chunks {
            if !loaded.apply(update) {
                return Err(format!(
                    "voxel chunk {}, {}, {} is invalid",
                    update.x, update.y, update.z
                ));
            }
        }

        loaded.revision = self.revision.wrapping_add(1);
        *self = loaded;

        Ok(())
    }

    pub fn trace(&self, start: Vector3, end: Vector3) -> Option<TraceHit> {
        self.trace_filtered(start, end, &HitAll)
    }

    pub fn trace_filtered(
        &self,
        start: Vector3,
        end: Vector3,
        filter: &dyn TraceFilter,
    ) -> Option<TraceHit> {
        if !is_finite(start) || !is_finite(end) {
            return None;
        }

        let scale = self.scale;
        let hit = self.trace_grid(
            Vector3::new(start.x / scale, start.y / scale, start.z / scale),
            Vector3::new(end.x / scale, end.y / scale, end.z / scale),
            filter,
        )?;

        Some(TraceHit {
            block: hit.block,
            face: hit.face,
            distance: hit.distance * scale,
            position: Vector3::new(
                hit.position.x * scale,
                hit.position.y * scale,
                hit.position.z * scale,
            ),
        })
    }

    pub fn sweep(
        &self,
        start: Vector3,
        end: Vector3,
        mins: Vector3,
        maxs: Vector3,
    ) -> Option<TraceHit> {
        self.sweep_filtered(start, end, mins, maxs, &HitAll)
    }

    pub fn sweep_filtered(
        &self,
        start: Vector3,
        end: Vector3,
        mins: Vector3,
        maxs: Vector3,
        filter: &dyn TraceFilter,
    ) -> Option<TraceHit> {
        if !is_finite(start) || !is_finite(end) || !is_finite(mins) || !is_finite(maxs) {
            return None;
        }

        let scale = self.scale;

        if scale <= 0.0 {
            return None;
        }

        let travel_x = end.x - start.x;
        let travel_y = end.y - start.y;
        let travel_z = end.z - start.z;
        let max_dist = (travel_x * travel_x + travel_y * travel_y + travel_z * travel_z).sqrt();
        let (dir_x, dir_y, dir_z) = if max_dist > 0.0 {
            let inv = 1.0 / max_dist;

            (travel_x * inv, travel_y * inv, travel_z * inv)
        } else {
            (0.0, 0.0, 0.0)
        };

        let min_x = start.x.min(end.x) + mins.x;
        let min_y = start.y.min(end.y) + mins.y;
        let min_z = start.z.min(end.z) + mins.z;
        let max_x = start.x.max(end.x) + maxs.x;
        let max_y = start.y.max(end.y) + maxs.y;
        let max_z = start.z.max(end.z) + maxs.z;
        let x0 = floor_i32(min_x / scale);
        let y0 = floor_i32(min_y / scale);
        let z0 = floor_i32(min_z / scale);
        let mut x1 = floor_i32((max_x - 1e-9) / scale);
        let mut y1 = floor_i32((max_y - 1e-9) / scale);
        let mut z1 = floor_i32((max_z - 1e-9) / scale);

        if x1 < x0 {
            x1 = x0;
        }

        if y1 < y0 {
            y1 = y0;
        }

        if z1 < z0 {
            z1 = z0;
        }

        if x1 - x0 > 48 {
            x1 = x0 + 48;
        }

        if y1 - y0 > 48 {
            y1 = y0 + 48;
        }

        if z1 - z0 > 48 {
            z1 = z0 + 48;
        }

        let mut best_dist = f64::MAX;
        let mut best: Option<TraceHit> = None;
        let mut z = z0;

        while z <= z1 {
            let mut y = y0;

            while y <= y1 {
                let mut x = x0;

                while x <= x1 {
                    let pos = BlockPos { x, y, z };

                    if self.is_hittable(pos, filter) {
                        let cell_min =
                            Vector3::new(x as f64 * scale, y as f64 * scale, z as f64 * scale);
                        let cell_max = Vector3::new(
                            cell_min.x + scale,
                            cell_min.y + scale,
                            cell_min.z + scale,
                        );
                        let box_min = Vector3::new(
                            cell_min.x - maxs.x,
                            cell_min.y - maxs.y,
                            cell_min.z - maxs.z,
                        );
                        let box_max = Vector3::new(
                            cell_max.x - mins.x,
                            cell_max.y - mins.y,
                            cell_max.z - mins.z,
                        );

                        if let Some((distance, normal)) =
                            ray_box(start, dir_x, dir_y, dir_z, max_dist, box_min, box_max)
                        {
                            if distance < best_dist {
                                best_dist = distance;
                                best = Some(TraceHit {
                                    block: pos,
                                    face: normal_face(normal),
                                    distance,
                                    position: Vector3::new(
                                        start.x + dir_x * distance,
                                        start.y + dir_y * distance,
                                        start.z + dir_z * distance,
                                    ),
                                });
                            }
                        }
                    }

                    x += 1;
                }

                y += 1;
            }

            z += 1;
        }

        best
    }

    fn is_hittable(&self, pos: BlockPos, filter: &dyn TraceFilter) -> bool {
        self.is_solid(pos) && filter.should_hit_voxel(pos, self.get(pos))
    }

    fn trace_grid(
        &self,
        start: Vector3,
        end: Vector3,
        filter: &dyn TraceFilter,
    ) -> Option<TraceHit> {
        if !is_finite(start) || !is_finite(end) {
            return None;
        }

        let travel_x = end.x - start.x;
        let travel_y = end.y - start.y;
        let travel_z = end.z - start.z;
        let max_dist = (travel_x * travel_x + travel_y * travel_y + travel_z * travel_z).sqrt();

        if max_dist == 0.0 {
            let block = BlockPos::from_world(start);

            if !self.is_hittable(block, filter) {
                return None;
            }

            return Some(TraceHit {
                block,
                face: None,
                distance: 0.0,
                position: start,
            });
        }

        let inv = 1.0 / max_dist;
        let dir_x = travel_x * inv;
        let dir_y = travel_y * inv;
        let dir_z = travel_z * inv;
        let mut x = axis(start.x, dir_x);
        let mut y = axis(start.y, dir_y);
        let mut z = axis(start.z, dir_z);
        let origin = BlockPos {
            x: x.cell,
            y: y.cell,
            z: z.cell,
        };

        if self.is_hittable(origin, filter) {
            return Some(TraceHit {
                block: origin,
                face: None,
                distance: 0.0,
                position: start,
            });
        }

        let step_limit = (max_dist as usize).saturating_mul(4).saturating_add(8);
        let mut steps = 0usize;

        loop {
            if steps >= step_limit {
                return None;
            }

            steps += 1;

            if x.t_max > max_dist && y.t_max > max_dist && z.t_max > max_dist {
                return None;
            }

            let face;
            let t_hit;

            if x.t_max <= y.t_max && x.t_max <= z.t_max {
                x.cell += x.step;
                face = if x.step > 0 { Face::NegX } else { Face::PosX };
                t_hit = x.t_max;
                x.t_max += x.t_delta;
            } else if y.t_max <= z.t_max {
                y.cell += y.step;
                face = if y.step > 0 { Face::NegY } else { Face::PosY };
                t_hit = y.t_max;
                y.t_max += y.t_delta;
            } else {
                z.cell += z.step;
                face = if z.step > 0 { Face::NegZ } else { Face::PosZ };
                t_hit = z.t_max;
                z.t_max += z.t_delta;
            }

            let block = BlockPos {
                x: x.cell,
                y: y.cell,
                z: z.cell,
            };

            if self.is_hittable(block, filter) {
                return Some(TraceHit {
                    block,
                    face: Some(face),
                    distance: t_hit,
                    position: Vector3::new(
                        start.x + dir_x * t_hit,
                        start.y + dir_y * t_hit,
                        start.z + dir_z * t_hit,
                    ),
                });
            }
        }
    }

    fn encode(&self, pos: ChunkPos) -> ChunkUpdate {
        let runs = match self.chunks.get(&pos) {
            Some(chunk) => chunk.runs(),
            None => Vec::new(),
        };

        ChunkUpdate {
            x: pos.x,
            y: pos.y,
            z: pos.z,
            runs,
        }
    }
}

pub fn cwd_vmap_path(name: &str) -> Option<PathBuf> {
    let stem = voxel_stem(name);

    if stem.is_empty() || stem.contains("..") || stem.contains('/') || stem.contains('\\') {
        return None;
    }

    let cwd = std::env::current_dir().ok()?;

    Some(cwd.join("maps").join(format!("{stem}.vmap")))
}

pub fn find_voxel_file(name: &str) -> Option<PathBuf> {
    let given = PathBuf::from(name);

    if given
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("vmap"))
        && given.exists()
    {
        return Some(given);
    }

    let stem = voxel_stem(name);

    for dir in super::content_dirs() {
        let path = dir.join(format!("{stem}.vmap"));

        if path.exists() {
            return Some(path);
        }
    }

    None
}

fn voxel_stem(name: &str) -> &str {
    let file = Path::new(name)
        .file_name()
        .and_then(|file| file.to_str())
        .unwrap_or(name);

    file.strip_suffix(".vmap")
        .or_else(|| file.strip_suffix(".map"))
        .or_else(|| file.strip_suffix(".cmap"))
        .unwrap_or(file)
}

fn encode_voxels(scale: f64, seed: i64, chunks: &[ChunkUpdate]) -> Result<Vec<u8>, String> {
    let stored = StoredVoxelsV2 {
        scale,
        seed,
        chunks: chunks.to_vec(),
    };
    let payload = wincode::serialize(&stored).map_err(|err| format!("{err}"))?;
    let mut bytes = Vec::with_capacity(8 + payload.len());
    bytes.extend_from_slice(VOXEL_MAGIC);
    bytes.extend_from_slice(&VOXEL_VERSION.to_le_bytes());
    bytes.extend(payload);

    Ok(bytes)
}

struct DecodedVoxels {
    version: u32,
    scale: f64,
    seed: i64,
    chunks: Vec<ChunkUpdate>,
}

fn voxel_version(bytes: &[u8]) -> Result<u32, String> {
    if bytes.len() < 8 || bytes[..4] != VOXEL_MAGIC[..] {
        return Err("voxel map header is invalid".to_string());
    }

    Ok(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
}

fn decode_voxels(bytes: &[u8]) -> Result<DecodedVoxels, String> {
    let version = voxel_version(bytes)?;

    if version == VOXEL_VERSION_V1 {
        let stored: StoredVoxelsV1 =
            wincode::deserialize(&bytes[8..]).map_err(|err| format!("{err}"))?;

        return Ok(DecodedVoxels {
            version,
            scale: stored.scale,
            seed: 1,
            chunks: stored.chunks,
        });
    }

    if version != VOXEL_VERSION {
        return Err(format!("voxel map version {version} is unsupported"));
    }

    let stored: StoredVoxelsV2 =
        wincode::deserialize(&bytes[8..]).map_err(|err| format!("{err}"))?;

    Ok(DecodedVoxels {
        version,
        scale: stored.scale,
        seed: stored.seed,
        chunks: stored.chunks,
    })
}

impl Default for VoxelWorld {
    fn default() -> Self {
        Self::new()
    }
}

const NEIGHBORS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

const QUADS: [[(i32, i32, i32); 4]; 6] = [
    [(1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1)],
    [(0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)],
    [(0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)],
    [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)],
    [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)],
    [(0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)],
];

const SHADES: [f32; 6] = [0.72, 0.62, 0.58, 0.5, 1.0, 0.4];

fn chunk_overlaps(pos: ChunkPos, scale: f64, min: Vector3, max: Vector3) -> bool {
    let edge = CHUNK_EDGE as f64 * scale;
    let x0 = pos.x as f64 * edge;
    let y0 = pos.y as f64 * edge;
    let z0 = pos.z as f64 * edge;

    x0 + edge >= min.x
        && x0 <= max.x
        && y0 + edge >= min.y
        && y0 <= max.y
        && z0 + edge >= min.z
        && z0 <= max.z
}

fn append_shifted(out: &mut Vec<f32>, mesh: &[f32], origin: Vector3) {
    if origin.x == 0.0 && origin.y == 0.0 && origin.z == 0.0 {
        out.extend_from_slice(mesh);

        return;
    }

    let start = out.len();
    out.extend_from_slice(mesh);
    let mut idx = start;

    while idx + super::STRIDE <= out.len() {
        out[idx] = (f64::from(out[idx]) - origin.x) as f32;
        out[idx + 1] = (f64::from(out[idx + 1]) - origin.y) as f32;
        out[idx + 2] = (f64::from(out[idx + 2]) - origin.z) as f32;
        idx += super::STRIDE;
    }
}

fn boundary_layers(blocks: &[u16]) -> [bool; 6] {
    let edge = CHUNK_EDGE as usize;
    let last = edge - 1;
    let mut layers = [false; 6];

    for v in 0..edge {
        for u in 0..edge {
            layers[0] |= blocks[local_index([last, u, v])] != 0;
            layers[1] |= blocks[local_index([0, u, v])] != 0;
            layers[2] |= blocks[local_index([u, last, v])] != 0;
            layers[3] |= blocks[local_index([u, 0, v])] != 0;
            layers[4] |= blocks[local_index([u, v, last])] != 0;
            layers[5] |= blocks[local_index([u, v, 0])] != 0;
        }
    }

    layers
}

fn local_index(local: [usize; 3]) -> usize {
    let edge = CHUNK_EDGE as usize;

    local[0] + local[1] * edge + local[2] * edge * edge
}

fn push_quad(
    vertices: &mut Vec<f32>,
    face: usize,
    id: u16,
    base: [i32; 3],
    ext: [i32; 3],
    scale: f64,
) {
    let x0 = base[0] as f64 * scale;
    let y0 = base[1] as f64 * scale;
    let z0 = base[2] as f64 * scale;
    let [red, green, blue] = block_rgb(id);
    let shade = SHADES[face];
    let color = [red * shade, green * shade, blue * shade];
    let quad = QUADS[face];
    let mut corners = [[0.0f32; 3]; 4];

    for corner in 0..4 {
        corners[corner] = [
            (x0 + quad[corner].0 as f64 * ext[0] as f64 * scale) as f32,
            (y0 + quad[corner].1 as f64 * ext[1] as f64 * scale) as f32,
            (z0 + quad[corner].2 as f64 * ext[2] as f64 * scale) as f32,
        ];
    }

    super::surface::push_shaded_tri(vertices, corners[0], corners[1], corners[2], color);
    super::surface::push_shaded_tri(vertices, corners[0], corners[2], corners[3], color);
}

pub fn block_rgb(id: u16) -> [f32; 3] {
    let color = match id {
        1 => [0.45, 0.45, 0.48],
        2 => [0.45, 0.32, 0.18],
        3 => [0.30, 0.55, 0.22],
        4 => [0.76, 0.70, 0.42],
        5 => [0.63, 0.55, 0.32],
        6 => [0.90, 0.92, 0.95],
        7 => [0.15, 0.35, 0.70],
        8 => [0.40, 0.26, 0.12],
        9 => [0.20, 0.48, 0.18],
        _ => {
            let mut n = (id as u32).wrapping_mul(1664525).wrapping_add(1013904223);
            let red = 0.35 + ((n >> 16) & 255) as f32 / 255.0 * 0.5;
            n = n.wrapping_mul(1664525).wrapping_add(1013904223);
            let green = 0.35 + ((n >> 16) & 255) as f32 / 255.0 * 0.5;
            n = n.wrapping_mul(1664525).wrapping_add(1013904223);
            let blue = 0.35 + ((n >> 16) & 255) as f32 / 255.0 * 0.5;

            return [red, green, blue];
        }
    };

    color
}

fn default_nonsolid() -> HashSet<u16> {
    let mut nonsolid = HashSet::new();
    nonsolid.insert(Block::WATER.0);

    nonsolid
}

fn floor_i32(value: f64) -> i32 {
    value.floor() as i32
}

fn axis_block(value: f64, scale: f64, end: bool) -> Option<i32> {
    if scale <= 0.0 {
        return None;
    }

    let scaled = value / scale;

    if !scaled.is_finite() {
        return None;
    }

    let rounded = if end { scaled.ceil() } else { scaled.floor() };

    if rounded < i32::MIN as f64 || rounded > i32::MAX as f64 {
        return None;
    }

    Some(rounded as i32)
}

fn span(min: i32, max: i32) -> i64 {
    max as i64 - min as i64
}

fn cell_hits_sphere(pos: BlockPos, center: Vector3, radius: f64, scale: f64) -> bool {
    let min_x = pos.x as f64 * scale;
    let min_y = pos.y as f64 * scale;
    let min_z = pos.z as f64 * scale;
    let x = center.x.clamp(min_x, min_x + scale);
    let y = center.y.clamp(min_y, min_y + scale);
    let z = center.z.clamp(min_z, min_z + scale);
    let dx = x - center.x;
    let dy = y - center.y;
    let dz = z - center.z;

    dx * dx + dy * dy + dz * dz <= radius * radius
}

fn div_floor(value: i32, size: i32) -> i32 {
    let quot = value / size;
    let rem = value % size;

    if rem < 0 {
        quot - 1
    } else {
        quot
    }
}

fn rem_floor(value: i32, size: i32) -> i32 {
    let rem = value % size;

    if rem < 0 {
        rem + size
    } else {
        rem
    }
}

fn finite_scale(scale: f64) -> Option<f64> {
    if scale.is_finite() && scale > 0.0 {
        Some(scale)
    } else {
        None
    }
}

fn is_finite(point: Vector3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn point_inside(point: Vector3, min: Vector3, max: Vector3) -> bool {
    point.x > min.x + 1e-8
        && point.x < max.x - 1e-8
        && point.y > min.y + 1e-8
        && point.y < max.y - 1e-8
        && point.z > min.z + 1e-8
        && point.z < max.z - 1e-8
}

fn normal_face(normal: Vector3) -> Option<Face> {
    if normal.x < -0.5 {
        return Some(Face::NegX);
    }

    if normal.x > 0.5 {
        return Some(Face::PosX);
    }

    if normal.y < -0.5 {
        return Some(Face::NegY);
    }

    if normal.y > 0.5 {
        return Some(Face::PosY);
    }

    if normal.z < -0.5 {
        return Some(Face::NegZ);
    }

    if normal.z > 0.5 {
        return Some(Face::PosZ);
    }

    None
}

fn ray_box(
    start: Vector3,
    dir_x: f64,
    dir_y: f64,
    dir_z: f64,
    max_dist: f64,
    min: Vector3,
    max: Vector3,
) -> Option<(f64, Vector3)> {
    if min.x >= max.x || min.y >= max.y || min.z >= max.z {
        return None;
    }

    if max_dist == 0.0 {
        if point_inside(start, min, max) {
            return Some((0.0, Vector3::new(0.0, 0.0, 0.0)));
        }

        return None;
    }

    let mut t_min = 0.0;
    let mut t_max = max_dist;
    let mut normal = Vector3::new(0.0, 0.0, 0.0);
    let axes = [
        (
            start.x,
            dir_x,
            min.x,
            max.x,
            Vector3::new(-1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        (
            start.y,
            dir_y,
            min.y,
            max.y,
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ),
        (
            start.z,
            dir_z,
            min.z,
            max.z,
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
    ];
    let mut idx = 0;

    while idx < axes.len() {
        let (origin, dir, lo, hi, neg, pos) = axes[idx];

        if dir.abs() <= 1e-12 {
            if origin < lo || origin > hi {
                return None;
            }
        } else {
            let inv = 1.0 / dir;
            let mut near = (lo - origin) * inv;
            let mut far = (hi - origin) * inv;
            let mut enter = neg;

            if near > far {
                let swap = near;
                near = far;
                far = swap;
                enter = pos;
            }

            if near > t_min {
                t_min = near;
                normal = enter;
            }

            if far < t_max {
                t_max = far;
            }

            if t_min > t_max {
                return None;
            }
        }

        idx += 1;
    }

    if t_max < 0.0 || t_min > max_dist {
        return None;
    }

    if t_min < 0.0 {
        if point_inside(start, min, max) {
            return Some((0.0, Vector3::new(0.0, 0.0, 0.0)));
        }

        return None;
    }

    Some((t_min, normal))
}

fn axis(origin: f64, dir: f64) -> Axis {
    let cell = floor_i32(origin);

    if dir > 0.0 {
        let next = cell as f64 + 1.0;
        let mut t_max = (next - origin) / dir;

        if t_max < 0.0 {
            t_max = 0.0;
        }

        return Axis {
            cell,
            step: 1,
            t_max,
            t_delta: 1.0 / dir,
        };
    }

    if dir < 0.0 {
        let next = cell as f64;
        let mut t_max = (next - origin) / dir;

        if t_max < 0.0 {
            t_max = 0.0;
        }

        return Axis {
            cell,
            step: -1,
            t_max,
            t_delta: -1.0 / dir,
        };
    }

    Axis {
        cell,
        step: 0,
        t_max: f64::INFINITY,
        t_delta: f64::INFINITY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 1e-6
    }

    #[test]
    fn set_get_crosses_chunk_borders() {
        let mut world = VoxelWorld::new();
        let stone = Block(1);
        let at = BlockPos::new(-1, -17, 32);

        assert_eq!(world.get(at), Block::AIR);
        world.set(at, stone);

        assert_eq!(world.get(at), stone);
        assert!(world.is_solid(at));
        assert_eq!(at.chunk(), ChunkPos { x: -1, y: -2, z: 2 });
        assert_eq!(at.local(), (15, 15, 0));
        assert_eq!(world.chunk_count(), 1);

        world.set(at, Block::AIR);

        assert_eq!(world.get(at), Block::AIR);
        assert_eq!(world.chunk_count(), 0);
    }

    #[test]
    fn dirty_chunks_round_trip() {
        let mut server = VoxelWorld::new();
        server.fill(BlockPos::new(-20, -2, 0), BlockPos::new(20, 3, 4), Block(3));
        let updates = server.take_dirty();
        let mut client = VoxelWorld::new();

        for update in &updates {
            assert!(client.apply(update));
        }

        assert_eq!(client.get(BlockPos::new(-20, -2, 0)), Block(3));
        assert_eq!(client.get(BlockPos::new(19, 2, 3)), Block(3));
        assert_eq!(client.get(BlockPos::new(20, 2, 3)), Block::AIR);
        assert_eq!(client.chunk_count(), server.chunk_count());
        assert!(server.take_dirty().is_empty());

        server.set(BlockPos::new(-20, -2, 0), Block::AIR);
        let cleared = server.take_dirty();
        assert_eq!(cleared.len(), 1);

        for update in &cleared {
            assert!(client.apply(update));
        }

        assert_eq!(client.get(BlockPos::new(-20, -2, 0)), Block::AIR);
        assert_eq!(client.get(BlockPos::new(-19, -2, 0)), Block(3));
    }

    #[test]
    fn short_chunk_is_rejected() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(1, 2, 3), Block(4));
        let update = ChunkUpdate {
            x: 0,
            y: 0,
            z: 0,
            runs: vec![ChunkRun { block: 9, len: 3 }],
        };

        assert!(!world.apply(&update));
        assert_eq!(world.get(BlockPos::new(1, 2, 3)), Block(4));
    }

    #[test]
    fn trace_hits_the_entered_face() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let hit = world
            .trace(Vector3::new(-1.5, 0.5, 0.5), Vector3::new(1.5, 0.5, 0.5))
            .unwrap();

        assert_eq!(hit.block, BlockPos::new(0, 0, 0));
        assert_eq!(hit.face, Some(Face::NegX));
        assert!(near(hit.distance, 1.5));
        assert!(near(hit.position.x, 0.0));

        let down = world
            .trace(Vector3::new(0.5, 0.5, 2.5), Vector3::new(0.5, 0.5, -1.0))
            .unwrap();

        assert_eq!(down.block, BlockPos::new(0, 0, 0));
        assert_eq!(down.face, Some(Face::PosZ));
        assert!(near(down.distance, 1.5));
        assert!(near(down.position.z, 1.0));
    }

    #[test]
    fn sweep_expands_the_block_by_the_hull() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let mins = Vector3::new(-0.3, -0.3, 0.0);
        let maxs = Vector3::new(0.3, 0.3, 1.6);
        let hit = world
            .sweep(
                Vector3::new(-1.5, 0.5, 0.5),
                Vector3::new(1.5, 0.5, 0.5),
                mins,
                maxs,
            )
            .unwrap();

        assert_eq!(hit.face, Some(Face::NegX));
        assert!(near(hit.distance, 1.2));
        assert!(near(hit.position.x, -0.3));
    }

    #[test]
    fn trace_from_inside_and_misses() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let inside = world
            .trace(Vector3::new(0.5, 0.5, 0.5), Vector3::new(4.0, 0.5, 0.5))
            .unwrap();

        assert_eq!(inside.block, BlockPos::new(0, 0, 0));
        assert_eq!(inside.face, None);
        assert!(near(inside.distance, 0.0));
        assert!(world
            .trace(Vector3::new(1.5, 0.5, 0.5), Vector3::new(3.5, 0.5, 0.5))
            .is_none());
    }

    #[test]
    fn trace_crosses_into_the_next_chunk() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(16, 0, 0), Block(1));
        world.set(BlockPos::new(-1, 0, 0), Block(1));
        let forward = world
            .trace(Vector3::new(15.5, 0.5, 0.5), Vector3::new(17.5, 0.5, 0.5))
            .unwrap();

        assert_eq!(forward.block, BlockPos::new(16, 0, 0));
        assert_eq!(forward.face, Some(Face::NegX));
        assert!(near(forward.distance, 0.5));

        let back = world
            .trace(Vector3::new(0.5, 0.5, 0.5), Vector3::new(-1.5, 0.5, 0.5))
            .unwrap();

        assert_eq!(back.block, BlockPos::new(-1, 0, 0));
        assert_eq!(back.face, Some(Face::PosX));
        assert!(near(back.distance, 0.5));
    }

    #[test]
    fn scale_changes_the_world_size_of_a_block() {
        let mut world = VoxelWorld::with_scale(0.5);

        assert_eq!(world.scale(), 0.5);
        assert!(!world.set_scale(0.0));
        assert!(!world.set_scale(f64::NAN));
        assert_eq!(world.scale(), 0.5);
        world.set(BlockPos::new(0, 0, 0), Block(1));
        assert_eq!(
            world.block_at(Vector3::new(0.49, 0.0, 0.0)),
            BlockPos::new(0, 0, 0)
        );
        assert_eq!(
            world.block_at(Vector3::new(0.5, 0.0, 0.0)),
            BlockPos::new(1, 0, 0)
        );

        let hit = world
            .trace(
                Vector3::new(-0.25, 0.25, 0.25),
                Vector3::new(1.0, 0.25, 0.25),
            )
            .unwrap();

        assert_eq!(hit.block, BlockPos::new(0, 0, 0));
        assert_eq!(hit.face, Some(Face::NegX));
        assert!(near(hit.distance, 0.25));
        assert!(near(hit.position.x, 0.0));

        let mut world = VoxelWorld::new();
        assert!(world.set_scale(2.0));
        assert_eq!(world.take_scale(), Some(2.0));
        assert!(world.take_scale().is_none());
        world.set(BlockPos::new(0, 0, 0), Block(1));
        assert_eq!(
            world.block_at(Vector3::new(1.9, 0.0, 0.0)),
            BlockPos::new(0, 0, 0)
        );
        assert_eq!(
            world.block_at(Vector3::new(2.0, 0.0, 0.0)),
            BlockPos::new(1, 0, 0)
        );

        let hit = world
            .trace(Vector3::new(-1.0, 1.0, 1.0), Vector3::new(4.0, 1.0, 1.0))
            .unwrap();

        assert_eq!(hit.block, BlockPos::new(0, 0, 0));
        assert_eq!(hit.face, Some(Face::NegX));
        assert!(near(hit.distance, 1.0));
        assert!(near(hit.position.x, 0.0));

        world.clear();

        assert_eq!(world.scale(), 1.0);
        assert_eq!(world.take_scale(), Some(1.0));
    }

    #[test]
    fn mesh_hides_shared_faces_and_faces_outward() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let mesh = world.mesh();

        assert_eq!(mesh.len(), 36 * crate::world::STRIDE);
        assert!(faces_point_outward(&mesh, 0.5));

        // A different block next to it: the shared faces are hidden and nothing can merge.
        world.set(BlockPos::new(1, 0, 0), Block(2));

        assert_eq!(world.mesh().len(), 60 * crate::world::STRIDE);
        assert!(faces_point_outward(&world.mesh(), 0.5));

        // The same block next to it: the matching faces merge, leaving a single 2x1x1 box.
        let mut pair = VoxelWorld::new();
        pair.set(BlockPos::new(0, 0, 0), Block(1));
        pair.set(BlockPos::new(1, 0, 0), Block(1));

        assert_eq!(pair.mesh().len(), 36 * crate::world::STRIDE);

        // A solid cube of one block is just its six outer faces.
        let mut solid = VoxelWorld::new();
        solid.fill(BlockPos::new(0, 0, 0), BlockPos::new(3, 3, 3), Block(1));

        assert_eq!(solid.mesh().len(), 36 * crate::world::STRIDE);
    }

    #[test]
    fn scale_multiplies_cached_mesh_and_keeps_blocks() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(1, 2, 3), Block(4));
        let before = world.mesh();
        let builds = world.mesh_builds;
        assert!(world.set_scale(2.0));
        assert_eq!(world.get(BlockPos::new(1, 2, 3)), Block(4));
        assert_eq!(world.mesh_builds, builds);
        let after = world.mesh();
        assert!((after[0] - before[0] * 2.0).abs() < 1e-3);
        assert!((after[1] - before[1] * 2.0).abs() < 1e-3);
        assert!((after[2] - before[2] * 2.0).abs() < 1e-3);

        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(1, 0, 0), Block(1));
        let before = world.mesh()[0];
        assert!(world.apply_scale(2.0));
        assert!(world.take_scale().is_none());
        assert!((world.mesh()[0] - before * 2.0).abs() < 1e-3);
        assert_eq!(world.mesh_builds, 1);
    }

    #[test]
    fn block_edit_rebuilds_only_the_touched_chunks() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        world.set(BlockPos::new(16, 0, 0), Block(1));
        world.set(BlockPos::new(48, 0, 0), Block(1));
        let _ = world.mesh();
        assert_eq!(world.mesh_builds, 3);
        // Local (0, 0, 1) is on the -x and -y boundary, so the +x chunk is left alone.
        world.set(BlockPos::new(0, 0, 1), Block(1));
        let _ = world.mesh();
        assert_eq!(world.mesh_builds, 4);
    }

    #[test]
    fn an_edit_only_remeshes_the_neighbors_it_touches() {
        let mut world = VoxelWorld::new();
        let edge = CHUNK_EDGE;

        // A solid 3x3x3 block of chunks, so every neighbor of the middle one exists.
        for z in 0..3 * edge {
            for y in 0..3 * edge {
                for x in 0..3 * edge {
                    if (x + y + z) % 7 == 0 {
                        world.set(BlockPos::new(x, y, z), Block(1));
                    }
                }
            }
        }

        let middle = edge + edge / 2;
        let cases = [
            // (block, chunks that should rebuild)
            (BlockPos::new(middle, middle, middle), 1),
            (BlockPos::new(edge, middle, middle), 2),
            (BlockPos::new(2 * edge - 1, middle, middle), 2),
            (BlockPos::new(middle, edge, middle), 2),
            (BlockPos::new(middle, middle, 2 * edge - 1), 2),
            (BlockPos::new(edge, edge, middle), 3),
            (BlockPos::new(edge, edge, edge), 4),
            (BlockPos::new(2 * edge - 1, 2 * edge - 1, 2 * edge - 1), 4),
        ];

        for (pos, expected) in cases {
            let _ = world.mesh();
            let before = world.mesh_builds;
            let block = if world.get(pos) == Block(2) { Block(3) } else { Block(2) };
            world.set(pos, block);
            let _ = world.mesh();

            assert_eq!(world.mesh_builds - before, expected, "edit at {pos:?}");
        }
    }

    #[test]
    fn voxel_file_roundtrip_keeps_blocks_and_scale() {
        let dir = std::env::temp_dir().join(format!("engine-vmap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("yard.vmap");
        let mut world = VoxelWorld::with_scale(2.0);
        world.set(BlockPos::new(-1, 4, 2), Block(3));
        world.fill(BlockPos::new(0, 0, 0), BlockPos::new(2, 2, 1), Block(1));
        world.set_seed(42);
        world.save_file(&path).unwrap();

        let mut loaded = VoxelWorld::new();
        loaded.set_seed(7);
        loaded.load_file(&path).unwrap();

        assert_eq!(loaded.scale(), 2.0);
        assert_eq!(loaded.seed(), 42);
        assert_eq!(loaded.get(BlockPos::new(-1, 4, 2)), Block(3));
        assert_eq!(loaded.get(BlockPos::new(1, 1, 0)), Block(1));
        assert_eq!(loaded.get(BlockPos::new(3, 0, 0)), Block::AIR);
        assert_eq!(find_voxel_file(path.to_str().unwrap()).unwrap(), path);

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            2
        );

        let bad = dir.join("bad.vmap");
        std::fs::write(&bad, b"nope").unwrap();

        assert!(VoxelWorld::new().load_file(&bad).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn voxel_file_version_one_keeps_the_current_seed() {
        let dir = std::env::temp_dir().join(format!("engine-vmap-v1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.vmap");
        let stored = StoredVoxelsV1 {
            scale: 1.5,
            chunks: vec![ChunkUpdate {
                x: 0,
                y: 0,
                z: 0,
                runs: vec![ChunkRun {
                    block: 4,
                    len: VOLUME as u16,
                }],
            }],
        };
        let payload = wincode::serialize(&stored).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(VOXEL_MAGIC);
        bytes.extend_from_slice(&VOXEL_VERSION_V1.to_le_bytes());
        bytes.extend(payload);
        std::fs::write(&path, bytes).unwrap();

        let mut world = VoxelWorld::new();
        world.set_seed(9);
        world.load_file(&path).unwrap();

        assert_eq!(world.scale(), 1.5);
        assert_eq!(world.seed(), 9);
        assert_eq!(world.get(BlockPos::new(0, 0, 0)), Block(4));
        assert!(!world.load_resume(&path).unwrap());
        assert_eq!(world.seed(), 9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn water_is_not_solid_and_still_occludes() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block::WATER);
        world.set(BlockPos::new(1, 0, 0), Block::STONE);

        assert!(!world.is_solid(BlockPos::new(0, 0, 0)));
        assert!(world.occludes(BlockPos::new(0, 0, 0)));
        assert!(world.is_solid(BlockPos::new(1, 0, 0)));
        world.set_block_solid(Block::WATER.0, true);
        assert!(world.is_solid(BlockPos::new(0, 0, 0)));
    }

    #[test]
    fn clear_generated_keeps_loaded_blocks() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block::STONE);
        let mut blocks = vec![0u16; VOLUME];
        blocks[0] = Block::DIRT.0;
        assert!(world.insert_generated(ChunkPos { x: 2, y: 0, z: 0 }, blocks));
        world.clear_generated();

        assert_eq!(world.get(BlockPos::new(0, 0, 0)), Block::STONE);
        assert!(world.get(BlockPos::new(32, 0, 0)).is_air());
    }

    #[test]
    fn world_fill_uses_blocks_the_box_touches() {
        let mut world = VoxelWorld::with_scale(2.0);
        assert!(world.fill_bounds(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(4.0, 2.0, 2.0),
            Block(3),
        ));
        assert_eq!(world.get(BlockPos::new(0, 0, 0)), Block(3));
        assert_eq!(world.get(BlockPos::new(1, 0, 0)), Block(3));
        assert!(world.get(BlockPos::new(2, 0, 0)).is_air());
        assert!(world.set_at(Vector3::new(5.0, 0.2, 0.2), Block(4)));
        assert_eq!(world.block_world(Vector3::new(5.0, 0.2, 0.2)), Block(4));
        assert!(!world.fill_bounds(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(400.0, 400.0, 400.0),
            Block(1),
        ));
        assert!(world.get(BlockPos::new(10, 10, 10)).is_air());
        let mut round = VoxelWorld::new();
        assert!(round.fill_sphere(Vector3::new(0.5, 0.5, 0.5), 0.2, Block(5)));
        assert_eq!(round.get(BlockPos::new(0, 0, 0)), Block(5));
        assert!(round.get(BlockPos::new(1, 0, 0)).is_air());
        assert!(!round.fill_sphere(Vector3::new(0.0, 0.0, 0.0), 0.0, Block(1)));
        round.clear();
        let cleared = round.take_dirty();
        assert_eq!(cleared.len(), 1);
        assert!(cleared[0].runs.is_empty());
        assert!(round.get(BlockPos::new(0, 0, 0)).is_air());
    }

    fn faces_point_outward(mesh: &[f32], center: f32) -> bool {
        let mut idx = 0;

        let stride = crate::world::STRIDE;

        while idx + stride * 3 <= mesh.len() {
            let ax = mesh[idx];
            let ay = mesh[idx + 1];
            let az = mesh[idx + 2];
            let bx = mesh[idx + stride];
            let by = mesh[idx + stride + 1];
            let bz = mesh[idx + stride + 2];
            let cx = mesh[idx + stride * 2];
            let cy = mesh[idx + stride * 2 + 1];
            let cz = mesh[idx + stride * 2 + 2];
            let nx = (by - ay) * (cz - az) - (bz - az) * (cy - ay);
            let ny = (bz - az) * (cx - ax) - (bx - ax) * (cz - az);
            let nz = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            let toward_x = (ax + bx + cx) / 3.0 - center;
            let toward_y = (ay + by + cy) / 3.0 - center;
            let toward_z = (az + bz + cz) / 3.0 - center;

            if nx * toward_x + ny * toward_y + nz * toward_z <= 0.0 {
                return false;
            }

            idx += stride * 3;
        }

        true
    }

    fn visible_faces_by_brute_force(world: &VoxelWorld, chunk: ChunkPos) -> HashSet<(usize, i32, i32, i32, u16)> {
        let mut out = HashSet::new();
        let edge = CHUNK_EDGE;

        for z in 0..edge {
            for y in 0..edge {
                for x in 0..edge {
                    let pos = BlockPos::new(chunk.x * edge + x, chunk.y * edge + y, chunk.z * edge + z);
                    let id = world.get(pos).0;

                    if id == 0 {
                        continue;
                    }

                    for (face, (nx, ny, nz)) in NEIGHBORS.iter().enumerate() {
                        let next = BlockPos::new(pos.x + nx, pos.y + ny, pos.z + nz);

                        if !world.occludes(next) {
                            out.insert((face, pos.x, pos.y, pos.z, id));
                        }
                    }
                }
            }
        }

        out
    }

    #[test]
    fn greedy_quads_cover_exactly_the_visible_faces() {
        let mut world = VoxelWorld::new();
        let mut state = 0x9E37_79B9u32;

        for z in -16i32..32 {
            for y in -16i32..32 {
                for x in -16i32..32 {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    let roll = (state >> 24) % 10;
                    let flat = z < 4 || (x > 4 && x < 12 && y > 2 && y < 20 && z < 9);
                    let id = if flat {
                        1 + ((x / 7).rem_euclid(3)) as u16
                    } else if roll == 0 {
                        4
                    } else {
                        0
                    };

                    if id != 0 {
                        world.set(BlockPos::new(x, y, z), Block(id));
                    }
                }
            }
        }

        let mut quads = 0usize;
        let mut faces = 0usize;

        for chunk in world.chunks.keys().copied().collect::<Vec<_>>() {
            let expected = visible_faces_by_brute_force(&world, chunk);
            let mut covered = HashSet::new();

            world.chunk_quads(chunk, |face, id, base, ext| {
                quads += 1;
                assert_eq!(ext[face / 2], 1);

                for dz in 0..ext[2] {
                    for dy in 0..ext[1] {
                        for dx in 0..ext[0] {
                            let cell = (face, base[0] + dx, base[1] + dy, base[2] + dz, id);

                            assert!(covered.insert(cell), "cell covered twice: {cell:?}");
                        }
                    }
                }
            });

            faces += expected.len();
            assert_eq!(covered, expected, "chunk {chunk:?}");
        }

        assert!(faces > 0);
        assert!(quads < faces, "merging should shrink the mesh: {quads} quads for {faces} faces");
    }

    #[test]
    fn a_flat_floor_merges_into_one_quad_per_chunk_face() {
        let mut world = VoxelWorld::new();

        for y in 0..32 {
            for x in 0..32 {
                world.set(BlockPos::new(x, y, 0), Block(1));
            }
        }

        let mut quads = 0;

        for chunk in world.chunks.keys().copied().collect::<Vec<_>>() {
            world.chunk_quads(chunk, |_, _, _, _| quads += 1);
        }

        // 4 chunks in a 2x2 block, each with a full top, a full bottom, and one merged strip on
        // each of its two outward sides.
        assert_eq!(quads, 4 * 4);
    }

    #[test]
    fn chunk_mesh_has_two_triangles_per_quad() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let mesh = world.build_chunk_mesh(ChunkPos { x: 0, y: 0, z: 0 });

        assert_eq!(mesh.len(), 6 * 2 * 3 * super::super::STRIDE);
    }

    #[test]
    fn limited_dirty_take_leaves_the_rest() {
        let mut world = VoxelWorld::new();

        for x in 0..5 {
            world.set(BlockPos::new(x * CHUNK_EDGE, 0, 0), Block(1));
        }

        let first = world.take_dirty_limited(2);
        assert_eq!(first.len(), 2);

        let rest = world.take_dirty_limited(10);
        assert_eq!(rest.len(), 3);
        assert!(world.take_dirty_limited(10).is_empty());

        world.mark_dirty(ChunkPos { x: 0, y: 0, z: 0 });
        assert_eq!(world.take_dirty().len(), 1);
    }

    #[test]
    fn a_chunk_that_is_air_along_a_boundary_keeps_its_neighbors_meshes() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        world.build_meshes(usize::MAX);
        let builds = world.mesh_builds;

        // Next chunk over in +x holds one block far from the shared boundary.
        let mut far = vec![0u16; VOLUME];
        far[Chunk::index(8, 8, 8)] = 1;
        assert!(world.insert_generated(ChunkPos { x: 1, y: 0, z: 0 }, far));
        world.build_meshes(usize::MAX);
        assert_eq!(world.mesh_builds, builds + 1, "only the new chunk needs a mesh");

        // One with a block on the boundary layer next to it does change the neighbor.
        let mut edge = vec![0u16; VOLUME];
        edge[Chunk::index(0, 0, 0)] = 1;
        assert!(world.insert_generated(ChunkPos { x: 0, y: 1, z: 0 }, edge.clone()));
        let before = world.mesh_builds;
        world.build_meshes(usize::MAX);
        assert_eq!(world.mesh_builds, before + 2, "the new chunk and the one it touches");
    }

    #[test]
    fn cached_meshes_stay_equal_to_fresh_ones_after_edits() {
        let mut world = VoxelWorld::new();
        let edge = CHUNK_EDGE;
        let mut state = 0xC0FF_EE11u32;
        let mut next = |range: i32| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);

            ((state >> 8) % range as u32) as i32
        };

        for _ in 0..600 {
            world.set(BlockPos::new(next(3 * edge), next(3 * edge), next(3 * edge)), Block(1 + next(3) as u16));
        }

        let _ = world.mesh();

        for step in 0..400 {
            // Half the edits land on a chunk boundary layer, including corners.
            let pick = |value: i32, force: bool| {
                if force {
                    (value / edge) * edge + if value % 2 == 0 { 0 } else { edge - 1 }
                } else {
                    value
                }
            };
            let force = step % 2 == 0;
            let pos = BlockPos::new(
                pick(next(3 * edge), force),
                pick(next(3 * edge), force),
                pick(next(3 * edge), force && step % 4 == 0),
            );
            let block = if next(3) == 0 { Block::AIR } else { Block(1 + next(3) as u16) };
            world.set(pos, block);

            if step % 7 == 0 {
                let _ = world.mesh();

                for chunk in world.chunks.keys().copied().collect::<Vec<_>>() {
                    let cached = world.chunks[&chunk].mesh.clone().expect("meshed");

                    assert_eq!(cached, world.build_chunk_mesh(chunk), "stale mesh in {chunk:?} after edit {step} at {pos:?}");
                }
            }
        }
    }
}
