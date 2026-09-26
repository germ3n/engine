use std::collections::{HashMap, HashSet};
use wincode::{SchemaRead, SchemaWrite};
use crate::script::libs::vector3::Vector3;

pub const CHUNK_EDGE: i32 = 16;
const VOLUME: usize = (CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE) as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Block(pub u16);

impl Block {
    pub const AIR: Self = Self(0);

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

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq, Eq)]
pub struct ChunkUpdate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub runs: Vec<ChunkRun>,
}

struct Chunk {
    blocks: Box<[u16]>,
}

impl Chunk {
    fn empty() -> Self {
        Self { blocks: vec![0; VOLUME].into_boxed_slice() }
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

            runs.push(ChunkRun { block, len: (end - idx) as u16 });
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
    scale: f64,
    scale_dirty: bool,
}

impl VoxelWorld {
    pub fn new() -> Self {
        Self::with_scale(1.0)
    }

    pub fn with_scale(scale: f64) -> Self {
        Self {
            chunks: HashMap::new(),
            dirty: HashSet::new(),
            scale: finite_scale(scale).unwrap_or(1.0),
            scale_dirty: false,
        }
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    pub fn set_scale(&mut self, scale: f64) -> bool {
        let Some(scale) = finite_scale(scale) else {
            return false;
        };

        if scale == self.scale {
            return true;
        }

        self.scale = scale;
        self.scale_dirty = true;

        true
    }

    pub fn apply_scale(&mut self, scale: f64) -> bool {
        let Some(scale) = finite_scale(scale) else {
            return false;
        };

        self.scale = scale;
        self.scale_dirty = false;

        true
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

        BlockPos::from_world(Vector3::new(point.x / scale, point.y / scale, point.z / scale))
    }

    pub fn clear(&mut self) {
        self.chunks.clear();
        self.dirty.clear();

        if self.scale != 1.0 {
            self.scale = 1.0;
            self.scale_dirty = true;
        }
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn get(&self, pos: BlockPos) -> Block {
        let Some(chunk) = self.chunks.get(&pos.chunk()) else {
            return Block::AIR;
        };

        let (local_x, local_y, local_z) = pos.local();

        Block(chunk.blocks[Chunk::index(local_x, local_y, local_z)])
    }

    pub fn is_solid(&self, pos: BlockPos) -> bool {
        self.get(pos).is_solid()
    }

    pub fn set(&mut self, pos: BlockPos, block: Block) {
        if self.get(pos) == block {
            return;
        }

        let chunk_pos = pos.chunk();
        let (local_x, local_y, local_z) = pos.local();
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

            return;
        }

        let chunk = self.chunks.entry(chunk_pos).or_insert_with(Chunk::empty);
        chunk.blocks[slot] = block.0;
        self.dirty.insert(chunk_pos);
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

    pub fn apply(&mut self, update: &ChunkUpdate) -> bool {
        let pos = ChunkPos { x: update.x, y: update.y, z: update.z };

        if update.runs.is_empty() {
            self.chunks.remove(&pos);

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

        if blocks.iter().all(|block| *block == 0) {
            self.chunks.remove(&pos);

            return true;
        }

        self.chunks.insert(pos, Chunk { blocks: blocks.into_boxed_slice() });

        true
    }

    pub fn baseline(&self) -> Vec<ChunkUpdate> {
        let mut updates = Vec::with_capacity(self.chunks.len());

        for pos in self.chunks.keys() {
            updates.push(self.encode(*pos));
        }

        updates
    }

    pub fn take_dirty(&mut self) -> Vec<ChunkUpdate> {
        let dirty = std::mem::take(&mut self.dirty);
        let mut updates = Vec::with_capacity(dirty.len());

        for pos in dirty {
            updates.push(self.encode(pos));
        }

        updates
    }

    pub fn trace(&self, start: Vector3, end: Vector3) -> Option<TraceHit> {
        if !is_finite(start) || !is_finite(end) {
            return None;
        }

        let scale = self.scale;
        let hit = self.trace_grid(
            Vector3::new(start.x / scale, start.y / scale, start.z / scale),
            Vector3::new(end.x / scale, end.y / scale, end.z / scale),
        )?;

        Some(TraceHit {
            block: hit.block,
            face: hit.face,
            distance: hit.distance * scale,
            position: Vector3::new(hit.position.x * scale, hit.position.y * scale, hit.position.z * scale),
        })
    }

    fn trace_grid(&self, start: Vector3, end: Vector3) -> Option<TraceHit> {
        if !is_finite(start) || !is_finite(end) {
            return None;
        }

        let travel_x = end.x - start.x;
        let travel_y = end.y - start.y;
        let travel_z = end.z - start.z;
        let max_dist = (travel_x * travel_x + travel_y * travel_y + travel_z * travel_z).sqrt();

        if max_dist == 0.0 {
            let block = BlockPos::from_world(start);

            if !self.is_solid(block) {
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
        let origin = BlockPos { x: x.cell, y: y.cell, z: z.cell };

        if self.is_solid(origin) {
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

            let block = BlockPos { x: x.cell, y: y.cell, z: z.cell };

            if self.is_solid(block) {
                return Some(TraceHit {
                    block,
                    face: Some(face),
                    distance: t_hit,
                    position: Vector3::new(start.x + dir_x * t_hit, start.y + dir_y * t_hit, start.z + dir_z * t_hit),
                });
            }
        }
    }

    fn encode(&self, pos: ChunkPos) -> ChunkUpdate {
        let runs = match self.chunks.get(&pos) {
            Some(chunk) => chunk.runs(),
            None => Vec::new(),
        };

        ChunkUpdate { x: pos.x, y: pos.y, z: pos.z, runs }
    }
}

impl Default for VoxelWorld {
    fn default() -> Self {
        Self::new()
    }
}

fn floor_i32(value: f64) -> i32 {
    value.floor() as i32
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

fn axis(origin: f64, dir: f64) -> Axis {
    let cell = floor_i32(origin);

    if dir > 0.0 {
        let next = cell as f64 + 1.0;
        let mut t_max = (next - origin) / dir;

        if t_max < 0.0 {
            t_max = 0.0;
        }

        return Axis { cell, step: 1, t_max, t_delta: 1.0 / dir };
    }

    if dir < 0.0 {
        let next = cell as f64;
        let mut t_max = (next - origin) / dir;

        if t_max < 0.0 {
            t_max = 0.0;
        }

        return Axis { cell, step: -1, t_max, t_delta: -1.0 / dir };
    }

    Axis { cell, step: 0, t_max: f64::INFINITY, t_delta: f64::INFINITY }
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
        let hit = world.trace(Vector3::new(-1.5, 0.5, 0.5), Vector3::new(1.5, 0.5, 0.5)).unwrap();

        assert_eq!(hit.block, BlockPos::new(0, 0, 0));
        assert_eq!(hit.face, Some(Face::NegX));
        assert!(near(hit.distance, 1.5));
        assert!(near(hit.position.x, 0.0));

        let down = world.trace(Vector3::new(0.5, 0.5, 2.5), Vector3::new(0.5, 0.5, -1.0)).unwrap();

        assert_eq!(down.block, BlockPos::new(0, 0, 0));
        assert_eq!(down.face, Some(Face::PosZ));
        assert!(near(down.distance, 1.5));
        assert!(near(down.position.z, 1.0));
    }

    #[test]
    fn trace_from_inside_and_misses() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let inside = world.trace(Vector3::new(0.5, 0.5, 0.5), Vector3::new(4.0, 0.5, 0.5)).unwrap();

        assert_eq!(inside.block, BlockPos::new(0, 0, 0));
        assert_eq!(inside.face, None);
        assert!(near(inside.distance, 0.0));
        assert!(world.trace(Vector3::new(1.5, 0.5, 0.5), Vector3::new(3.5, 0.5, 0.5)).is_none());
    }

    #[test]
    fn trace_crosses_into_the_next_chunk() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(16, 0, 0), Block(1));
        world.set(BlockPos::new(-1, 0, 0), Block(1));
        let forward = world.trace(Vector3::new(15.5, 0.5, 0.5), Vector3::new(17.5, 0.5, 0.5)).unwrap();

        assert_eq!(forward.block, BlockPos::new(16, 0, 0));
        assert_eq!(forward.face, Some(Face::NegX));
        assert!(near(forward.distance, 0.5));

        let back = world.trace(Vector3::new(0.5, 0.5, 0.5), Vector3::new(-1.5, 0.5, 0.5)).unwrap();

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
        assert_eq!(world.block_at(Vector3::new(0.49, 0.0, 0.0)), BlockPos::new(0, 0, 0));
        assert_eq!(world.block_at(Vector3::new(0.5, 0.0, 0.0)), BlockPos::new(1, 0, 0));

        let hit = world.trace(Vector3::new(-0.25, 0.25, 0.25), Vector3::new(1.0, 0.25, 0.25)).unwrap();

        assert_eq!(hit.block, BlockPos::new(0, 0, 0));
        assert_eq!(hit.face, Some(Face::NegX));
        assert!(near(hit.distance, 0.25));
        assert!(near(hit.position.x, 0.0));

        let mut world = VoxelWorld::new();
        assert!(world.set_scale(2.0));
        assert_eq!(world.take_scale(), Some(2.0));
        assert!(world.take_scale().is_none());
        world.set(BlockPos::new(0, 0, 0), Block(1));
        assert_eq!(world.block_at(Vector3::new(1.9, 0.0, 0.0)), BlockPos::new(0, 0, 0));
        assert_eq!(world.block_at(Vector3::new(2.0, 0.0, 0.0)), BlockPos::new(1, 0, 0));

        let hit = world.trace(Vector3::new(-1.0, 1.0, 1.0), Vector3::new(4.0, 1.0, 1.0)).unwrap();

        assert_eq!(hit.block, BlockPos::new(0, 0, 0));
        assert_eq!(hit.face, Some(Face::NegX));
        assert!(near(hit.distance, 1.0));
        assert!(near(hit.position.x, 0.0));

        world.clear();

        assert_eq!(world.scale(), 1.0);
        assert_eq!(world.take_scale(), Some(1.0));
    }
}
