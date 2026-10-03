use super::voxel::{Block, BlockPos, ChunkPos, VoxelWorld, CHUNK_EDGE};
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub const DEFAULT_SEED: i64 = 1;
pub const DEFAULT_SEA_LEVEL: f64 = 32.0;
pub const DEFAULT_MIN_Z: i32 = -32;
pub const DEFAULT_MAX_Z: i32 = 96;
pub const DEFAULT_RADIUS: i32 = 4;

const VOLUME: usize = (CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE) as usize;
const COLUMNS: usize = (CHUNK_EDGE * CHUNK_EDGE) as usize;
const MAX_INFLIGHT: usize = 64;
const TREE_RADIUS: i32 = 2;

#[derive(Clone, Debug)]
pub struct Biome {
    pub name: String,
    pub temp_min: f64,
    pub temp_max: f64,
    pub surface: u16,
    pub soil: u16,
    pub stone: u16,
    pub liquid: u16,
    pub height: f64,
    pub trees: f64,
}

#[derive(Clone, Debug)]
pub struct GenConfig {
    pub seed: i64,
    pub sea_level: f64,
    pub min_z: i32,
    pub max_z: i32,
    pub radius: i32,
    pub biomes: Vec<Biome>,
    pub epoch: u64,
}

pub struct GenSettings {
    pub seed: i64,
    pub sea_level: f64,
    pub min_z: i32,
    pub max_z: i32,
    pub radius: i32,
    pub biomes: Vec<Biome>,
    pub nonsolid: Vec<u16>,
    pub map_name: String,
    pub epoch: u64,
    pub enabled: bool,
    pub running: bool,
    pub reseed: bool,
    pub forget: bool,
}

#[derive(Clone, Debug)]
pub struct ChunkDraft {
    pub pos: ChunkPos,
    pub blocks: Vec<u16>,
    pub temperature: Vec<f64>,
    pub biome: Vec<String>,
}

impl Default for ChunkDraft {
    fn default() -> Self {
        Self {
            pos: ChunkPos { x: 0, y: 0, z: 0 },
            blocks: Vec::new(),
            temperature: Vec::new(),
            biome: Vec::new(),
        }
    }
}

pub struct ChunkHandle(pub Arc<Mutex<ChunkDraft>>);

pub struct Noise {
    perm: [u8; 512],
}

struct Fields {
    temp: Noise,
    shape: Noise,
    cave: Noise,
}

struct Column {
    biome: usize,
    height: f64,
    ocean: bool,
}

struct Tree {
    base: i32,
    height: i32,
}

struct Work {
    pos: ChunkPos,
    config: Arc<GenConfig>,
    epoch: u64,
}

struct Done {
    epoch: u64,
    draft: ChunkDraft,
}

pub struct VoxelGen {
    settings: Arc<Mutex<GenSettings>>,
    job_tx: Option<Sender<Work>>,
    done_rx: Receiver<Done>,
    workers: Vec<JoinHandle<()>>,
    pending: HashSet<ChunkPos>,
    done: HashSet<ChunkPos>,
    inflight: usize,
    epoch: u64,
}

impl Noise {
    pub fn new(seed: i64) -> Self {
        let mut perm = [0u8; 512];
        let mut idx = 0;

        while idx < 256 {
            perm[idx] = idx as u8;
            idx += 1;
        }

        let mut state = seed as u64;

        if state == 0 {
            state = 0xA5A5_5A5A_1234_5678;
        }

        idx = 255;

        while idx > 0 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let swap = (state >> 33) as usize % (idx + 1);
            let tmp = perm[idx];
            perm[idx] = perm[swap];
            perm[swap] = tmp;
            idx -= 1;
        }

        idx = 0;

        while idx < 256 {
            perm[idx + 256] = perm[idx];
            idx += 1;
        }

        Self { perm }
    }

    pub fn sample2(&self, x: f64, y: f64) -> f64 {
        self.sample3(x, y, 0.0)
    }

    pub fn sample3(&self, x: f64, y: f64, z: f64) -> f64 {
        if floor_coord(x).is_none() || floor_coord(y).is_none() || floor_coord(z).is_none() {
            return 0.0;
        }

        self.sample3_fast(x, y, z)
    }

    fn sample3_fast(&self, x: f64, y: f64, z: f64) -> f64 {
        let x0 = x.floor();
        let y0 = y.floor();
        let z0 = z.floor();
        let ix = x0 as i32;
        let iy = y0 as i32;
        let iz = z0 as i32;
        let tx = fade(x - x0);
        let ty = fade(y - y0);
        let tz = fade(z - z0);
        let v000 = lattice(&self.perm, ix, iy, iz);
        let v100 = lattice(&self.perm, ix + 1, iy, iz);
        let v010 = lattice(&self.perm, ix, iy + 1, iz);
        let v110 = lattice(&self.perm, ix + 1, iy + 1, iz);
        let v001 = lattice(&self.perm, ix, iy, iz + 1);
        let v101 = lattice(&self.perm, ix + 1, iy, iz + 1);
        let v011 = lattice(&self.perm, ix, iy + 1, iz + 1);
        let v111 = lattice(&self.perm, ix + 1, iy + 1, iz + 1);
        let x00 = lerp(v000, v100, tx);
        let x10 = lerp(v010, v110, tx);
        let x01 = lerp(v001, v101, tx);
        let x11 = lerp(v011, v111, tx);
        let y0 = lerp(x00, x10, ty);
        let y1 = lerp(x01, x11, ty);

        lerp(y0, y1, tz) * 2.0 - 1.0
    }
}

impl Default for GenSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl GenSettings {
    pub fn new() -> Self {
        Self {
            seed: DEFAULT_SEED,
            sea_level: DEFAULT_SEA_LEVEL,
            min_z: DEFAULT_MIN_Z,
            max_z: DEFAULT_MAX_Z,
            radius: DEFAULT_RADIUS,
            biomes: builtin_biomes(),
            nonsolid: vec![Block::WATER.0],
            map_name: String::new(),
            epoch: 0,
            enabled: false,
            running: false,
            reseed: false,
            forget: false,
        }
    }

    pub fn snapshot(&self) -> Arc<GenConfig> {
        Arc::new(GenConfig {
            seed: self.seed,
            sea_level: self.sea_level,
            min_z: self.min_z,
            max_z: self.max_z,
            radius: self.radius,
            biomes: self.biomes.clone(),
            epoch: self.epoch,
        })
    }

    pub fn adopt_seed(&mut self, seed: i64) {
        self.seed = seed;
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = seed;
        self.epoch = self.epoch.wrapping_add(1);

        if self.running {
            self.reseed = true;
        }
    }

    pub fn set_radius(&mut self, radius: i32) {
        self.radius = radius.clamp(1, 32);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn set_bounds(&mut self, min_z: i32, max_z: i32) -> bool {
        if min_z >= max_z {
            return false;
        }

        self.min_z = min_z;
        self.max_z = max_z;

        true
    }

    pub fn set_sea_level(&mut self, level: f64) -> bool {
        if !level.is_finite() {
            return false;
        }

        self.sea_level = level;

        true
    }

    pub fn set_block_solid(&mut self, id: u16, solid: bool) {
        if id == 0 {
            return;
        }

        if solid {
            self.nonsolid.retain(|block| *block != id);

            return;
        }

        if !self.nonsolid.contains(&id) {
            self.nonsolid.push(id);
        }
    }

    pub fn add_biome(&mut self, biome: Biome) -> bool {
        if biome.name.is_empty() || !biome.temp_min.is_finite() || !biome.temp_max.is_finite() {
            return false;
        }

        if biome.temp_min > biome.temp_max || !biome.height.is_finite() || !biome.trees.is_finite()
        {
            return false;
        }

        if biome.trees < 0.0 {
            return false;
        }

        let mut idx = 0;

        while idx < self.biomes.len() {
            if self.biomes[idx].name == biome.name {
                self.biomes[idx] = biome;

                return true;
            }

            idx += 1;
        }

        self.biomes.push(biome);

        true
    }

    pub fn clear_biomes(&mut self) {
        self.biomes.clear();
    }
}

impl Fields {
    fn new(seed: i64) -> Self {
        Self {
            temp: Noise::new(seed),
            shape: Noise::new(seed.wrapping_add(101)),
            cave: Noise::new(seed.wrapping_add(907)),
        }
    }
}

impl Drop for VoxelGen {
    fn drop(&mut self) {
        self.job_tx = None;
        let mut workers = std::mem::take(&mut self.workers);

        while let Some(worker) = workers.pop() {
            let _ = worker.join();
        }
    }
}

impl VoxelGen {
    pub fn start(settings: Arc<Mutex<GenSettings>>) -> Self {
        let epoch = settings.lock().expect("gen settings").epoch;
        let (job_tx, job_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let mut workers = Vec::new();
        let mut idx = 0;
        let count = worker_count();

        while idx < count {
            let jobs = Arc::clone(&job_rx);
            let done_tx = done_tx.clone();
            workers.push(std::thread::spawn(move || worker_loop(jobs, done_tx)));
            idx += 1;
        }

        drop(done_tx);

        Self {
            settings,
            job_tx: Some(job_tx),
            done_rx,
            workers,
            pending: HashSet::new(),
            done: HashSet::new(),
            inflight: 0,
            epoch,
        }
    }

    pub fn prepare(&mut self, world: &mut VoxelWorld) {
        let mut settings = self.settings.lock().expect("gen settings");
        world.replace_nonsolid(&settings.nonsolid);
        world.set_seed(settings.seed);

        if settings.reseed {
            settings.reseed = false;
            self.epoch = settings.epoch;
            drop(settings);
            world.clear_generated();
            self.done.clear();
            self.pending.clear();

            return;
        }

        if settings.forget {
            settings.forget = false;
            self.epoch = settings.epoch;
            drop(settings);
            self.done.clear();
            self.pending.clear();

            return;
        }

        self.epoch = settings.epoch;
    }

    pub fn enqueue(&mut self, world: &VoxelWorld, centers: &[ChunkPos]) {
        if centers.is_empty() || self.inflight >= MAX_INFLIGHT {
            return;
        }

        let config = self.settings.lock().expect("gen settings").snapshot();
        let mut wanted = wanted_chunks(&config, centers, world, &self.done, &self.pending);
        wanted.sort_by_key(|pos| nearest(*pos, centers));
        let mut idx = 0;

        while idx < wanted.len() && self.inflight < MAX_INFLIGHT {
            if !self.submit_work(wanted[idx], Arc::clone(&config)) {
                break;
            }

            idx += 1;
        }
    }

    pub fn submit(&mut self, pos: ChunkPos) -> bool {
        let config = self.settings.lock().expect("gen settings").snapshot();

        self.submit_work(pos, config)
    }

    pub fn is_settled(&self, world: &VoxelWorld, centers: &[ChunkPos]) -> bool {
        if self.inflight != 0 || !self.pending.is_empty() {
            return false;
        }

        let config = self.settings.lock().expect("gen settings").snapshot();

        wanted_chunks(&config, centers, world, &self.done, &self.pending).is_empty()
    }

    pub fn take_commits(&mut self, limit: usize) -> Vec<ChunkDraft> {
        let mut out = Vec::new();
        let mut pulled = 0;

        while out.len() < limit && pulled < MAX_INFLIGHT {
            let Ok(done) = self.done_rx.try_recv() else {
                break;
            };

            pulled += 1;
            self.inflight = self.inflight.saturating_sub(1);

            if done.epoch != self.epoch {
                continue;
            }

            self.pending.remove(&done.draft.pos);
            out.push(done.draft);
        }

        out
    }

    pub fn commit(&mut self, world: &mut VoxelWorld, draft: ChunkDraft) -> bool {
        commit_draft(world, &mut self.done, draft)
    }

    fn submit_work(&mut self, pos: ChunkPos, config: Arc<GenConfig>) -> bool {
        if self.pending.contains(&pos) || self.inflight >= MAX_INFLIGHT {
            return false;
        }

        let Some(tx) = self.job_tx.as_ref() else {
            return false;
        };
        let epoch = config.epoch;

        if tx.send(Work { pos, config, epoch }).is_err() {
            return false;
        }

        self.pending.insert(pos);
        self.inflight += 1;

        true
    }
}

pub fn seed_from_f64(value: f64) -> Option<i64> {
    if !value.is_finite() || value < i64::MIN as f64 || value > i64::MAX as f64 {
        return None;
    }

    Some(value as i64)
}

pub fn builtin_biomes() -> Vec<Biome> {
    vec![
        Biome {
            name: "tundra".to_string(),
            temp_min: 0.0,
            temp_max: 0.18,
            surface: Block::SNOW.0,
            soil: Block::DIRT.0,
            stone: Block::STONE.0,
            liquid: Block::WATER.0,
            height: 18.0,
            trees: 0.0,
        },
        Biome {
            name: "plains".to_string(),
            temp_min: 0.18,
            temp_max: 0.42,
            surface: Block::GRASS.0,
            soil: Block::DIRT.0,
            stone: Block::STONE.0,
            liquid: Block::WATER.0,
            height: 14.0,
            trees: 0.0,
        },
        Biome {
            name: "forest".to_string(),
            temp_min: 0.42,
            temp_max: 0.62,
            surface: Block::GRASS.0,
            soil: Block::DIRT.0,
            stone: Block::STONE.0,
            liquid: Block::WATER.0,
            height: 16.0,
            trees: 0.01,
        },
        Biome {
            name: "desert".to_string(),
            temp_min: 0.62,
            temp_max: 0.82,
            surface: Block::SAND.0,
            soil: Block::SANDSTONE.0,
            stone: Block::STONE.0,
            liquid: Block::WATER.0,
            height: 10.0,
            trees: 0.0,
        },
        Biome {
            name: "ocean".to_string(),
            temp_min: 0.82,
            temp_max: 1.0,
            surface: Block::SAND.0,
            soil: Block::SAND.0,
            stone: Block::STONE.0,
            liquid: Block::WATER.0,
            height: 6.0,
            trees: 0.0,
        },
    ]
}

pub fn pick_biome_index(biomes: &[Biome], temp: f64) -> Option<usize> {
    let mut idx = 0;

    while idx < biomes.len() {
        let biome = &biomes[idx];

        if temp >= biome.temp_min && temp <= biome.temp_max {
            return Some(idx);
        }

        idx += 1;
    }

    if biomes.is_empty() {
        return None;
    }

    let mut best = 0;
    let mut best_dist = f64::MAX;
    idx = 0;

    while idx < biomes.len() {
        let biome = &biomes[idx];
        let dist = if temp < biome.temp_min {
            biome.temp_min - temp
        } else {
            temp - biome.temp_max
        };

        if dist < best_dist {
            best_dist = dist;
            best = idx;
        }

        idx += 1;
    }

    Some(best)
}

pub fn generate_chunk(config: &GenConfig, pos: ChunkPos) -> ChunkDraft {
    let mut blocks = vec![0u16; VOLUME];
    let mut temperature = vec![0.0f64; COLUMNS];
    let mut biome_names = vec![String::new(); COLUMNS];
    let fields = Fields::new(config.seed);
    let origin_x = pos.x * CHUNK_EDGE;
    let origin_y = pos.y * CHUNK_EDGE;
    let origin_z = pos.z * CHUNK_EDGE;
    let mut columns = Vec::with_capacity(COLUMNS);
    let mut ly = 0;

    while ly < CHUNK_EDGE {
        let mut lx = 0;

        while lx < CHUNK_EDGE {
            let wx = origin_x + lx;
            let wy = origin_y + ly;
            let temp = temperature_at(&fields, wx, wy);
            let slot = (lx + ly * CHUNK_EDGE) as usize;
            temperature[slot] = temp;

            if let Some(index) = pick_biome_index(&config.biomes, temp) {
                let biome = &config.biomes[index];
                biome_names[slot] = biome.name.clone();
                columns.push(Some(Column {
                    biome: index,
                    height: terrain_height(&fields, config, biome, wx, wy),
                    ocean: biome.name == "ocean",
                }));
            } else {
                columns.push(None);
            }

            lx += 1;
        }

        ly += 1;
    }

    let mut lz = 0;

    while lz < CHUNK_EDGE {
        let wz = origin_z + lz;
        let mut column = 0;

        while column < COLUMNS {
            let Some(info) = columns[column].as_ref() else {
                column += 1;

                continue;
            };
            let lx = (column as i32) % CHUNK_EDGE;
            let ly = (column as i32) / CHUNK_EDGE;
            let id = block_id(config, &fields, info, origin_x + lx, origin_y + ly, wz);
            blocks[block_index(lx, ly, lz)] = id;
            column += 1;
        }

        lz += 1;
    }

    paint_trees(config, &fields, pos, &mut blocks);

    ChunkDraft {
        pos,
        blocks,
        temperature,
        biome: biome_names,
    }
}

pub fn commit_draft(
    world: &mut VoxelWorld,
    done: &mut HashSet<ChunkPos>,
    draft: ChunkDraft,
) -> bool {
    if world.contains_chunk(draft.pos) {
        done.insert(draft.pos);

        return false;
    }

    let pos = draft.pos;

    if !world.insert_generated(pos, draft.blocks) {
        return false;
    }

    done.insert(pos);

    true
}

fn worker_loop(jobs: Arc<Mutex<Receiver<Work>>>, done_tx: Sender<Done>) {
    loop {
        let work = {
            let rx = jobs.lock().expect("gen jobs");

            rx.recv()
        };
        let Ok(work) = work else {
            break;
        };
        let draft = generate_chunk(&work.config, work.pos);

        if done_tx
            .send(Done {
                epoch: work.epoch,
                draft,
            })
            .is_err()
        {
            break;
        }
    }
}

fn worker_count() -> usize {
    let count = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    let count = count.saturating_sub(1).max(1);

    count.clamp(1, 4)
}

fn wanted_chunks(
    config: &GenConfig,
    centers: &[ChunkPos],
    world: &VoxelWorld,
    done: &HashSet<ChunkPos>,
    pending: &HashSet<ChunkPos>,
) -> Vec<ChunkPos> {
    let mut unique = HashSet::new();
    let (z0, z1) = z_span(config);
    let radius = config.radius.max(0);
    let radius_sq = radius as i64 * radius as i64;
    let mut center_idx = 0;

    while center_idx < centers.len() {
        let center = centers[center_idx];
        let mut dy = -radius;

        while dy <= radius {
            let mut dx = -radius;

            while dx <= radius {
                let dist = dx as i64 * dx as i64 + dy as i64 * dy as i64;

                if dist <= radius_sq {
                    let mut z = z0;

                    while z <= z1 {
                        let pos = ChunkPos {
                            x: center.x + dx,
                            y: center.y + dy,
                            z,
                        };

                        if !world.contains_chunk(pos)
                            && !done.contains(&pos)
                            && !pending.contains(&pos)
                        {
                            unique.insert(pos);
                        }

                        z += 1;
                    }
                }

                dx += 1;
            }

            dy += 1;
        }

        center_idx += 1;
    }

    unique.into_iter().collect()
}

fn z_span(config: &GenConfig) -> (i32, i32) {
    let z0 = div_floor(config.min_z, CHUNK_EDGE);
    let z1 = div_floor(config.max_z - 1, CHUNK_EDGE);

    if z1 < z0 {
        return (z0, z0);
    }

    (z0, z1)
}

fn nearest(pos: ChunkPos, centers: &[ChunkPos]) -> i64 {
    let mut best = i64::MAX;
    let mut idx = 0;

    while idx < centers.len() {
        let center = centers[idx];
        let dx = pos.x as i64 - center.x as i64;
        let dy = pos.y as i64 - center.y as i64;
        let dz = pos.z as i64 - center.z as i64;
        let dist = dx * dx + dy * dy + dz * dz;

        if dist < best {
            best = dist;
        }

        idx += 1;
    }

    best
}

fn temperature_at(fields: &Fields, x: i32, y: i32) -> f64 {
    let sample = fbm2(
        &fields.temp,
        x as f64 * 0.0035 + 180.0,
        y as f64 * 0.0035 - 40.0,
    );

    (sample * 0.5 + 0.5).clamp(0.0, 1.0)
}

fn terrain_height(fields: &Fields, config: &GenConfig, biome: &Biome, x: i32, y: i32) -> f64 {
    let continent = fbm2(&fields.shape, x as f64 * 0.008, y as f64 * 0.008);

    if biome.name == "ocean" {
        return config.sea_level - 8.0 + continent * biome.height * 0.35;
    }

    config.sea_level + continent * biome.height
}

fn block_id(config: &GenConfig, fields: &Fields, column: &Column, x: i32, y: i32, z: i32) -> u16 {
    if z < config.min_z || z >= config.max_z {
        return Block::AIR.0;
    }

    if z == config.min_z {
        return column_stone(config, column);
    }

    let biome = &config.biomes[column.biome];
    let depth = column.height - z as f64;
    let water = column.ocean && (z as f64) < config.sea_level && depth < 1.0;

    if depth > 8.0 {
        if cave_at(fields, x, y, z) {
            return Block::AIR.0;
        }

        return biome.stone;
    }

    if depth < -4.0 {
        if water {
            return biome.liquid;
        }

        return Block::AIR.0;
    }

    let overhang =
        fields
            .shape
            .sample3_fast(x as f64 * 0.05 + 30.0, y as f64 * 0.05, z as f64 * 0.05)
            * 4.0;
    let density = depth + overhang;
    let carved = depth > 5.0 && cave_at(fields, x, y, z);

    if density > 0.0 && !carved {
        if depth < 1.5 {
            return biome.surface;
        }

        if depth < 4.5 {
            return biome.soil;
        }

        return biome.stone;
    }

    if water {
        return biome.liquid;
    }

    Block::AIR.0
}

fn cave_at(fields: &Fields, x: i32, y: i32, z: i32) -> bool {
    fields
        .cave
        .sample3_fast(x as f64 * 0.08, y as f64 * 0.08, z as f64 * 0.08)
        > 0.55
}

fn column_stone(config: &GenConfig, column: &Column) -> u16 {
    config.biomes[column.biome].stone
}

fn paint_trees(config: &GenConfig, fields: &Fields, pos: ChunkPos, blocks: &mut [u16]) {
    let mut idx = 0;
    let mut trees = false;

    while idx < config.biomes.len() {
        if config.biomes[idx].trees > 0.0 {
            trees = true;

            break;
        }

        idx += 1;
    }

    if !trees {
        return;
    }

    let origin_x = pos.x * CHUNK_EDGE;
    let origin_y = pos.y * CHUNK_EDGE;
    let origin_z = pos.z * CHUNK_EDGE;
    let x0 = origin_x - TREE_RADIUS;
    let x1 = origin_x + CHUNK_EDGE + TREE_RADIUS;
    let y0 = origin_y - TREE_RADIUS;
    let y1 = origin_y + CHUNK_EDGE + TREE_RADIUS;
    let mut y = y0;

    while y < y1 {
        let mut x = x0;

        while x < x1 {
            if let Some(tree) = tree_at(config, fields, x, y) {
                paint_tree(blocks, origin_x, origin_y, origin_z, x, y, &tree);
            }

            x += 1;
        }

        y += 1;
    }
}

fn tree_at(config: &GenConfig, fields: &Fields, x: i32, y: i32) -> Option<Tree> {
    let temp = temperature_at(fields, x, y);
    let index = pick_biome_index(&config.biomes, temp)?;
    let biome = &config.biomes[index];

    if biome.trees <= 0.0 {
        return None;
    }

    let hash = column_hash(config.seed, x, y);
    let chance = (hash % 10_000) as f64 / 10_000.0;

    if chance >= biome.trees {
        return None;
    }

    let ground = terrain_height(fields, config, biome, x, y).floor() as i32;
    let trunk = 4 + ((hash / 10_000) % 3) as i32;

    Some(Tree {
        base: ground + 1,
        height: trunk,
    })
}

fn paint_tree(
    blocks: &mut [u16],
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    x: i32,
    y: i32,
    tree: &Tree,
) {
    let mut tz = 0;

    while tz < tree.height {
        write_air(
            blocks,
            origin_x,
            origin_y,
            origin_z,
            x,
            y,
            tree.base + tz,
            Block::LOG.0,
        );
        tz += 1;
    }

    let top = tree.base + tree.height - 1;
    let mut dz = -TREE_RADIUS;

    while dz <= TREE_RADIUS {
        let mut dy = -TREE_RADIUS;

        while dy <= TREE_RADIUS {
            let mut dx = -TREE_RADIUS;

            while dx <= TREE_RADIUS {
                let dist = dx * dx + dy * dy + dz * dz;
                let trunk = dx == 0 && dy == 0 && dz <= 0;

                if dist <= TREE_RADIUS * TREE_RADIUS && !trunk {
                    write_air(
                        blocks,
                        origin_x,
                        origin_y,
                        origin_z,
                        x + dx,
                        y + dy,
                        top + dz,
                        Block::LEAVES.0,
                    );
                }

                dx += 1;
            }

            dy += 1;
        }

        dz += 1;
    }
}

fn write_air(
    blocks: &mut [u16],
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    x: i32,
    y: i32,
    z: i32,
    id: u16,
) {
    let lx = x - origin_x;
    let ly = y - origin_y;
    let lz = z - origin_z;

    if lx < 0 || ly < 0 || lz < 0 || lx >= CHUNK_EDGE || ly >= CHUNK_EDGE || lz >= CHUNK_EDGE {
        return;
    }

    let slot = block_index(lx, ly, lz);

    if blocks[slot] != Block::AIR.0 {
        return;
    }

    blocks[slot] = id;
}

fn column_hash(seed: i64, x: i32, y: i32) -> u64 {
    let mut n = seed as u64;
    n = n
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(x as u32 as u64);
    n = n
        .wrapping_mul(0xBF58476D1CE4E5B9)
        .wrapping_add(y as u32 as u64);
    n ^= n >> 30;
    n = n.wrapping_mul(0x94D049BB133111EB);
    n ^= n >> 31;

    n
}

fn fbm2(noise: &Noise, x: f64, y: f64) -> f64 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut weight = 0.0;
    let mut idx = 0;

    while idx < 2 {
        sum += amp * noise.sample2(x * freq, y * freq);
        weight += amp;
        amp *= 0.5;
        freq *= 2.0;
        idx += 1;
    }

    if weight == 0.0 {
        return 0.0;
    }

    sum / weight
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn lattice(perm: &[u8; 512], x: i32, y: i32, z: i32) -> f64 {
    let xi = (x as u32 as usize) & 255;
    let yi = (y as u32 as usize) & 255;
    let zi = (z as u32 as usize) & 255;
    let hashed = perm[xi + perm[yi + perm[zi] as usize] as usize] as f64;

    hashed / 255.0
}

fn floor_coord(value: f64) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }

    let floored = value.floor();

    if floored < i32::MIN as f64 || floored > i32::MAX as f64 {
        return None;
    }

    Some(floored as i32)
}

fn block_index(local_x: i32, local_y: i32, local_z: i32) -> usize {
    (local_x + local_y * CHUNK_EDGE + local_z * CHUNK_EDGE * CHUNK_EDGE) as usize
}

fn div_floor(value: i32, size: i32) -> i32 {
    let quot = value / size;
    let rem = value % size;

    if rem < 0 {
        return quot - 1;
    }

    quot
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GenConfig {
        GenSettings::new().snapshot().as_ref().clone()
    }

    #[test]
    fn noise_is_deterministic_f64() {
        let noise = Noise::new(1);
        let again = Noise::new(1);
        let sample = noise.sample2(1.25, -4.5);
        let spatial = noise.sample3(0.2, 0.4, 0.6);

        assert_eq!(sample, again.sample2(1.25, -4.5));
        assert_eq!(spatial, again.sample3(0.2, 0.4, 0.6));
        assert!(sample.is_finite());
        assert!(spatial.is_finite());
        assert!((-1.0..=1.0).contains(&sample));
        assert!((-1.0..=1.0).contains(&spatial));

        let mut idx = 0;
        let mut differed = false;

        while idx < 8 {
            let value = noise.sample2(idx as f64 * 1.7, idx as f64 * -0.4);
            assert!(value.is_finite());
            assert!((-1.0..=1.0).contains(&value));

            if (value - sample).abs() > 1e-9 {
                differed = true;
            }

            idx += 1;
        }

        assert!(differed);
        assert_ne!(
            Noise::new(1).sample3(0.2, 0.4, 0.6),
            Noise::new(2).sample3(0.2, 0.4, 0.6)
        );
    }

    #[test]
    fn same_seed_rebuilds_the_same_chunk() {
        let config = config();
        let pos = ChunkPos { x: 0, y: 1, z: 2 };
        let first = generate_chunk(&config, pos);
        let second = generate_chunk(&config, pos);

        assert_eq!(first.blocks, second.blocks);
        assert_eq!(first.temperature, second.temperature);
        assert_eq!(first.biome, second.biome);
        assert!(first.blocks.iter().any(|block| *block != 0));
        assert!(first.blocks.iter().any(|block| *block == 0));
    }

    #[test]
    fn temperature_selects_the_expected_biome() {
        let biomes = builtin_biomes();

        assert_eq!(
            biomes[pick_biome_index(&biomes, 0.0).unwrap()].name,
            "tundra"
        );
        assert_eq!(
            biomes[pick_biome_index(&biomes, 0.18).unwrap()].name,
            "tundra"
        );
        assert_eq!(
            biomes[pick_biome_index(&biomes, 0.3).unwrap()].name,
            "plains"
        );
        assert_eq!(
            biomes[pick_biome_index(&biomes, 0.5).unwrap()].name,
            "forest"
        );
        assert_eq!(
            biomes[pick_biome_index(&biomes, 0.7).unwrap()].name,
            "desert"
        );
        assert_eq!(
            biomes[pick_biome_index(&biomes, 0.9).unwrap()].name,
            "ocean"
        );
        assert_eq!(
            biomes[pick_biome_index(&biomes, 1.0).unwrap()].name,
            "ocean"
        );
        assert_eq!(
            biomes[pick_biome_index(&biomes, -1.0).unwrap()].name,
            "tundra"
        );
        assert_eq!(
            biomes[pick_biome_index(&biomes, 2.0).unwrap()].name,
            "ocean"
        );
    }

    #[test]
    fn forest_trees_place_logs() {
        let mut config = config();
        config.biomes.clear();
        config.biomes.push(Biome {
            name: "forest".to_string(),
            temp_min: 0.0,
            temp_max: 1.0,
            surface: Block::GRASS.0,
            soil: Block::DIRT.0,
            stone: Block::STONE.0,
            liquid: Block::WATER.0,
            height: 8.0,
            trees: 1.0,
        });
        let fields = Fields::new(config.seed);
        let ground = terrain_height(&fields, &config, &config.biomes[0], 0, 0).floor() as i32;
        let pos = ChunkPos {
            x: 0,
            y: 0,
            z: div_floor(ground + 1, CHUNK_EDGE),
        };
        let draft = generate_chunk(&config, pos);

        assert!(draft.blocks.contains(&Block::LOG.0));
        assert!(draft.blocks.contains(&Block::LEAVES.0));
    }

    #[test]
    fn caves_open_below_the_surface() {
        let config = config();
        let mut pos = ChunkPos { x: -1, y: 0, z: 0 };
        let mut air = 0usize;
        let mut solid = 0usize;

        while pos.x <= 1 {
            let draft = generate_chunk(&config, pos);
            let mut idx = 0;

            while idx < draft.blocks.len() {
                if draft.blocks[idx] == 0 {
                    air += 1;
                } else {
                    solid += 1;
                }

                idx += 1;
            }

            pos.x += 1;
        }

        assert!(solid > 0);
        assert!(air > 0);
    }

    #[test]
    fn ocean_basin_holds_water() {
        let mut config = config();
        config.biomes.retain(|biome| biome.name == "ocean");
        config.biomes[0].temp_min = 0.0;
        config.biomes[0].temp_max = 1.0;
        let fields = Fields::new(config.seed);
        let height = terrain_height(&fields, &config, &config.biomes[0], 4, -3);
        let mut z = height.floor() as i32 + 2;
        let mut found = false;

        while (z as f64) < config.sea_level {
            let column = Column {
                biome: 0,
                height,
                ocean: true,
            };
            let id = block_id(&config, &fields, &column, 4, -3, z);

            if id == Block::WATER.0 {
                found = true;
            }

            z += 1;
        }

        assert!(found);
        assert!(height < config.sea_level);
    }

    #[test]
    fn loaded_chunk_is_not_replaced() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block::SAND);
        let mut done = HashSet::new();
        let draft = generate_chunk(&config(), ChunkPos { x: 0, y: 0, z: 0 });

        assert!(!commit_draft(&mut world, &mut done, draft));
        assert_eq!(world.get(BlockPos::new(0, 0, 0)), Block::SAND);
        assert!(done.contains(&ChunkPos { x: 0, y: 0, z: 0 }));
    }

    #[test]
    fn pool_job_keeps_a_loaded_chunk() {
        let settings = Arc::new(Mutex::new(GenSettings::new()));
        let mut gen = VoxelGen::start(settings);
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block::SAND);
        let pos = ChunkPos { x: 0, y: 0, z: 0 };

        assert!(gen.submit(pos));

        let start = std::time::Instant::now();
        let mut drafted = false;

        while !drafted && start.elapsed() < std::time::Duration::from_secs(5) {
            let ready = gen.take_commits(4);
            let mut idx = 0;

            while idx < ready.len() {
                assert!(!gen.commit(&mut world, ready[idx].clone()));
                drafted = true;
                idx += 1;
            }

            if !drafted {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }

        assert!(drafted);
        assert_eq!(world.get(BlockPos::new(0, 0, 0)), Block::SAND);
    }
}
