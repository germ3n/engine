use super::surface::push_shaded_tri;
use super::{BrushMap, BrushPlane, VoxelWorld};
use crate::anchor::Anchor;
use crate::script::libs::vector3::Vector3;
use crate::script::Realm;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

const NAV_MAGIC: &[u8; 4] = b"NAVM";
const NAV_VERSION: u32 = 1;
const TILE: i32 = 64;
const LIFT: f64 = 0.04;
const MERGE_EPS: f64 = 1e-3;
const CLEAR_EPS: f64 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavInput {
    Brush,
    Voxel,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavTag {
    Brush,
    Voxel,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffKind {
    Jump,
    Drop,
}

#[derive(Clone, Copy, Debug)]
pub struct NavParams {
    pub radius: f64,
    pub height: f64,
    pub step: f64,
    pub slope: f64,
    pub jump_height: f64,
    pub jump_dist: f64,
    pub max_drop: f64,
}

impl NavParams {
    pub fn player() -> Self {
        Self {
            radius: 0.28,
            height: 1.65,
            step: 0.45,
            slope: 0.7,
            jump_height: 1.15,
            jump_dist: 2.0,
            max_drop: 8.0,
        }
    }

    pub fn cell(self) -> f64 {
        (self.radius * 0.5).max(1.0e-3)
    }
}

#[derive(Clone, Debug)]
pub struct NavBrush {
    pub planes: Vec<BrushPlane>,
    pub min: Vector3,
    pub max: Vector3,
}

#[derive(Clone, Copy, Debug)]
pub struct NavBlock {
    pub min: Vector3,
    pub max: Vector3,
    pub water: bool,
}

#[derive(Clone, Debug)]
pub struct NavSnapshot {
    pub brushes: Vec<NavBrush>,
    pub blocks: Vec<NavBlock>,
    pub input: NavInput,
}

#[derive(Clone, Debug)]
pub struct NavPoly {
    pub verts: Vec<Vector3>,
    pub tag: NavTag,
    pub neighbors: Vec<u32>,
    pub portals: Vec<(Vector3, Vector3)>,
}

#[derive(Clone, Debug)]
pub struct NavOff {
    pub from: u32,
    pub to: u32,
    pub kind: OffKind,
    pub start: Vector3,
    pub end: Vector3,
}

#[derive(Clone, Debug)]
pub struct NavMesh {
    pub polys: Vec<NavPoly>,
    pub offmesh: Vec<NavOff>,
    pub params: NavParams,
    pub input: NavInput,
}

impl NavMesh {
    pub fn empty(params: NavParams, input: NavInput) -> Self {
        Self {
            polys: Vec::new(),
            offmesh: Vec::new(),
            params,
            input,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(NAV_MAGIC);
        push_u32(&mut out, NAV_VERSION);
        push_u8(&mut out, input_byte(self.input));
        push_f64(&mut out, self.params.radius);
        push_f64(&mut out, self.params.height);
        push_f64(&mut out, self.params.step);
        push_f64(&mut out, self.params.slope);
        push_f64(&mut out, self.params.jump_height);
        push_f64(&mut out, self.params.jump_dist);
        push_f64(&mut out, self.params.max_drop);
        push_u32(&mut out, self.polys.len() as u32);
        let mut idx = 0;

        while idx < self.polys.len() {
            let poly = &self.polys[idx];
            push_u8(&mut out, tag_byte(poly.tag));
            push_u16(&mut out, poly.verts.len() as u16);
            let mut vert = 0;

            while vert < poly.verts.len() {
                push_vec3(&mut out, poly.verts[vert]);
                vert += 1;
            }

            push_u16(&mut out, poly.neighbors.len() as u16);
            let mut neighbor = 0;

            while neighbor < poly.neighbors.len() {
                push_u32(&mut out, poly.neighbors[neighbor]);
                push_vec3(&mut out, poly.portals[neighbor].0);
                push_vec3(&mut out, poly.portals[neighbor].1);
                neighbor += 1;
            }

            idx += 1;
        }

        push_u32(&mut out, self.offmesh.len() as u32);
        idx = 0;

        while idx < self.offmesh.len() {
            let link = &self.offmesh[idx];
            push_u32(&mut out, link.from);
            push_u32(&mut out, link.to);
            push_u8(&mut out, kind_byte(link.kind));
            push_vec3(&mut out, link.start);
            push_vec3(&mut out, link.end);
            idx += 1;
        }

        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let mut reader = Reader { data: bytes, at: 0 };
        let mut magic = [0u8; 4];
        let mut idx = 0;

        while idx < 4 {
            magic[idx] = reader.u8()?;
            idx += 1;
        }

        if &magic != NAV_MAGIC {
            return Err("bad navmesh magic".to_string());
        }

        let version = reader.u32()?;

        if version != NAV_VERSION {
            return Err(format!("unsupported navmesh version {version}"));
        }

        let input = input_from(reader.u8()?)?;
        let params = NavParams {
            radius: reader.f64()?,
            height: reader.f64()?,
            step: reader.f64()?,
            slope: reader.f64()?,
            jump_height: reader.f64()?,
            jump_dist: reader.f64()?,
            max_drop: reader.f64()?,
        };
        let poly_count = reader.u32()? as usize;
        let mut polys = Vec::with_capacity(poly_count);
        idx = 0;

        while idx < poly_count {
            let tag = tag_from(reader.u8()?)?;
            let verts_len = reader.u16()? as usize;
            let mut verts = Vec::with_capacity(verts_len);
            let mut vert = 0;

            while vert < verts_len {
                verts.push(reader.vec3()?);
                vert += 1;
            }

            let neighbor_len = reader.u16()? as usize;
            let mut neighbors = Vec::with_capacity(neighbor_len);
            let mut portals = Vec::with_capacity(neighbor_len);
            let mut neighbor = 0;

            while neighbor < neighbor_len {
                neighbors.push(reader.u32()?);
                let a = reader.vec3()?;
                let b = reader.vec3()?;
                portals.push((a, b));
                neighbor += 1;
            }

            polys.push(NavPoly {
                verts,
                tag,
                neighbors,
                portals,
            });
            idx += 1;
        }

        let off_count = reader.u32()? as usize;
        let mut offmesh = Vec::with_capacity(off_count);
        idx = 0;

        while idx < off_count {
            let from = reader.u32()?;
            let to = reader.u32()?;
            let kind = kind_from(reader.u8()?)?;
            let start = reader.vec3()?;
            let end = reader.vec3()?;
            offmesh.push(NavOff {
                from,
                to,
                kind,
                start,
                end,
            });
            idx += 1;
        }

        Ok(Self {
            polys,
            offmesh,
            params,
            input,
        })
    }

    pub fn find_path(&self, start: Vector3, goal: Vector3) -> Vec<Vector3> {
        let Some(start_poly) = locate(self, start) else {
            return Vec::new();
        };
        let Some(goal_poly) = locate(self, goal) else {
            return Vec::new();
        };
        let start_on = on_floor(&self.polys[start_poly], start);
        let goal_on = on_floor(&self.polys[goal_poly], goal);

        if start_poly == goal_poly {
            return dedupe(vec![start_on, goal_on]);
        }

        let centroids = centroids(self);
        let adj = adjacency(self, &centroids);
        let mut best = vec![f64::INFINITY; self.polys.len()];
        let mut parent: Vec<Option<Parent>> = vec![None; self.polys.len()];
        let mut heap = BinaryHeap::new();
        best[start_poly] = 0.0;
        heap.push(Rank {
            score: dist3(start_on, goal_on),
            cost: 0.0,
            poly: start_poly as u32,
        });

        while let Some(Rank { cost, poly, .. }) = heap.pop() {
            let poly = poly as usize;

            if cost > best[poly] {
                continue;
            }

            if poly == goal_poly {
                break;
            }

            let mut edge = 0;

            while edge < adj[poly].len() {
                let link = &adj[poly][edge];
                let next = cost + link.cost;

                if next < best[link.to as usize] {
                    best[link.to as usize] = next;
                    parent[link.to as usize] = Some(Parent {
                        prev: poly as u32,
                        hop: link.hop,
                    });
                    let guess = next + dist3(centroids[link.to as usize], goal_on);
                    heap.push(Rank {
                        score: guess,
                        cost: next,
                        poly: link.to,
                    });
                }

                edge += 1;
            }
        }

        if !best[goal_poly].is_finite() {
            return Vec::new();
        }

        let mut rev = Vec::new();
        let mut cursor = goal_poly;

        loop {
            rev.push(cursor);

            if cursor == start_poly {
                break;
            }

            let Some(prev) = parent[cursor] else {
                return Vec::new();
            };
            cursor = prev.prev as usize;

            if rev.len() > self.polys.len() + 1 {
                return Vec::new();
            }
        }

        rev.reverse();
        let mut path = Vec::new();
        let mut chunk = vec![rev[0]];
        let mut from = start_on;
        let mut idx = 0;

        while idx + 1 < rev.len() {
            let next = rev[idx + 1];
            let hop = parent[next].and_then(|parent| parent.hop);

            if let Some(hop) = hop {
                let link = &self.offmesh[hop];
                let pulled = funnel_chunk(self, &chunk, from, link.start);
                append_path(&mut path, pulled);
                path.push(link.end);
                from = link.end;
                chunk = vec![next];
            } else {
                chunk.push(next);
            }

            idx += 1;
        }

        append_path(&mut path, funnel_chunk(self, &chunk, from, goal_on));

        dedupe(path)
    }
}

#[derive(Clone, Copy)]
struct Parent {
    prev: u32,
    hop: Option<usize>,
}

struct Edge {
    to: u32,
    cost: f64,
    hop: Option<usize>,
}

struct Rank {
    score: f64,
    cost: f64,
    poly: u32,
}

impl PartialEq for Rank {
    fn eq(&self, other: &Self) -> bool {
        self.poly == other.poly && self.score.to_bits() == other.score.to_bits()
    }
}

impl Eq for Rank {}

impl PartialOrd for Rank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rank {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .score
            .total_cmp(&self.score)
            .then(self.poly.cmp(&other.poly))
    }
}

pub struct NavState {
    pub mesh: NavMesh,
    pub show: bool,
    pub debug_path: Vec<Vector3>,
    pub follow_path: Vec<Vector3>,
    pub loaded: bool,
    pub file_bytes: Vec<u8>,
    pub follow_gen: u64,
    pub draw_gen: u64,
    incoming: Vec<Option<Vec<u8>>>,
    incoming_parts: u16,
    incoming_got: u16,
}

impl NavState {
    pub fn new() -> Self {
        Self {
            mesh: NavMesh::empty(NavParams::player(), NavInput::Both),
            show: false,
            debug_path: Vec::new(),
            follow_path: Vec::new(),
            loaded: false,
            file_bytes: Vec::new(),
            follow_gen: 0,
            draw_gen: 1,
            incoming: Vec::new(),
            incoming_parts: 0,
            incoming_got: 0,
        }
    }

    pub fn clear_mesh(&mut self) {
        self.mesh = NavMesh::empty(self.mesh.params, self.mesh.input);
        self.debug_path.clear();
        self.follow_path.clear();
        self.loaded = false;
        self.file_bytes.clear();
        self.incoming.clear();
        self.incoming_parts = 0;
        self.incoming_got = 0;
        self.follow_gen = self.follow_gen.wrapping_add(1);
        self.draw_gen = self.draw_gen.wrapping_add(1);
    }

    pub fn ready(&self) -> bool {
        self.loaded && !self.mesh.polys.is_empty()
    }

    pub fn query(&self, start: Vector3, goal: Vector3) -> Vec<Vector3> {
        if !self.ready() {
            return Vec::new();
        }

        self.mesh.find_path(start, goal)
    }

    pub fn set_debug_path(&mut self, points: Vec<Vector3>) {
        self.debug_path = points;
        self.draw_gen = self.draw_gen.wrapping_add(1);
    }

    pub fn set_follow(&mut self, points: Vec<Vector3>) {
        if same_path(&self.follow_path, &points) {
            return;
        }

        self.follow_path = points;
        self.follow_gen = self.follow_gen.wrapping_add(1);
        self.draw_gen = self.draw_gen.wrapping_add(1);
    }

    pub fn set_show(&mut self, enabled: bool) {
        if self.show == enabled {
            return;
        }

        self.show = enabled;
        self.draw_gen = self.draw_gen.wrapping_add(1);
    }

    pub fn push_part(&mut self, part: u16, parts: u16, bytes: Vec<u8>) {
        if parts == 0 || part >= parts {
            return;
        }

        if part == 0 || self.incoming_parts != parts || self.incoming.len() != parts as usize {
            self.incoming = vec![None; parts as usize];
            self.incoming_parts = parts;
            self.incoming_got = 0;
        }

        let slot = part as usize;

        if self.incoming[slot].is_none() {
            self.incoming[slot] = Some(bytes);
            self.incoming_got = self.incoming_got.saturating_add(1);
        }

        if self.incoming_got != parts {
            return;
        }

        let mut packed = Vec::new();
        let mut idx = 0;

        while idx < self.incoming.len() {
            if let Some(chunk) = &self.incoming[idx] {
                packed.extend_from_slice(chunk);
            }

            idx += 1;
        }

        self.incoming.clear();
        self.incoming_parts = 0;
        self.incoming_got = 0;
        let Ok(raw) = decompress_nav(&packed) else {
            log::warn!("[nav] bad mesh payload");

            return;
        };
        let Ok(mesh) = NavMesh::from_bytes(&raw) else {
            log::warn!("[nav] bad mesh payload");

            return;
        };
        self.mesh = mesh;
        self.loaded = true;
        self.file_bytes = raw;
        self.draw_gen = self.draw_gen.wrapping_add(1);
    }
}

#[derive(Clone)]
struct PendingBuild {
    input: NavInput,
    params: NavParams,
}

pub struct NavHost {
    pub state: NavState,
    baker: Option<NavBaker>,
    pending: Option<PendingBuild>,
    follow_sent: u64,
}

impl NavHost {
    pub fn new() -> Self {
        Self {
            state: NavState::new(),
            baker: None,
            pending: None,
            follow_sent: 0,
        }
    }

    pub fn load_saved(&mut self, map_name: &str) {
        match load_nav(map_name) {
            Ok(Some((mesh, bytes))) => {
                self.state.mesh = mesh;
                self.state.file_bytes = bytes;
                self.state.loaded = true;
                self.state.draw_gen = self.state.draw_gen.wrapping_add(1);
                log::info!(
                    "[nav] loaded {} polys from {}",
                    self.state.mesh.polys.len(),
                    map_name
                );
            }
            Ok(None) => {}
            Err(err) => log::warn!("[nav] {err}"),
        }
    }

    pub fn request_build(&mut self, input: NavInput, params: NavParams) {
        if let Some(baker) = self.baker.as_mut() {
            baker.cancel();
        }

        self.pending = Some(PendingBuild { input, params });
        log::info!("[nav] build queued");
    }

    pub fn needs_settled(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.input != NavInput::Brush)
    }

    pub fn poll(
        &mut self,
        map_name: &str,
        brushes: &BrushMap,
        voxels: &VoxelWorld,
        settled: bool,
    ) -> bool {
        let ready = self.pending.as_ref().is_some_and(|pending| {
            pending.input == NavInput::Brush || settled
        });

        if ready {
            let pending = self.pending.take().expect("nav build");
            let snapshot = snapshot_world(brushes, voxels, pending.input);
            let baked = self.submit(snapshot, pending.params);

            if let Some(mesh) = baked {
                return self.install(mesh, map_name);
            }
        }

        let finished = self.baker.as_mut().and_then(|baker| baker.poll());

        if let Some(mesh) = finished {
            return self.install(mesh, map_name);
        }

        false
    }

    pub fn take_follow(&mut self) -> Option<Vec<Vector3>> {
        if self.follow_sent == self.state.follow_gen {
            return None;
        }

        self.follow_sent = self.state.follow_gen;

        Some(self.state.follow_path.clone())
    }

    fn submit(&mut self, snapshot: NavSnapshot, params: NavParams) -> Option<NavMesh> {
        if self.baker.is_none() {
            self.baker = Some(NavBaker::start());
        }

        self.baker
            .as_mut()
            .expect("nav baker")
            .submit(snapshot, params)
    }

    fn install(&mut self, mesh: NavMesh, map_name: &str) -> bool {
        let bytes = mesh.to_bytes();

        if let Err(err) = save_nav(map_name, &bytes) {
            log::warn!("[nav] {err}");
        }

        let count = mesh.polys.len();
        self.state.mesh = mesh;
        self.state.loaded = true;
        self.state.file_bytes = bytes;
        self.state.debug_path.clear();
        self.state.set_follow(Vec::new());
        self.state.draw_gen = self.state.draw_gen.wrapping_add(1);
        log::info!("[nav] baked {count} polys");

        true
    }
}

#[derive(Clone, Debug)]
pub enum NavCommand {
    Build { input: NavInput, params: NavParams },
    Show { enabled: bool },
    Path { start: Vector3, goal: Vector3 },
}

struct Queued {
    command: NavCommand,
}

static QUEUE: Mutex<Vec<Queued>> = Mutex::new(Vec::new());

pub fn console_line(tokens: &[String]) -> Result<(), String> {
    let realm = crate::demo::realm().ok_or("nav command has no realm")?;

    if !matches!(realm, Realm::Server) {
        return Err("nav commands are server only".to_string());
    }

    let command = parse_command(tokens)?;
    QUEUE
        .lock()
        .map_err(|_| "nav queue poisoned".to_string())?
        .push(Queued { command });

    Ok(())
}

pub fn drain() -> Vec<NavCommand> {
    let Ok(mut queue) = QUEUE.lock() else {
        return Vec::new();
    };
    let pending = std::mem::take(&mut *queue);
    let mut out = Vec::with_capacity(pending.len());
    let mut idx = 0;

    while idx < pending.len() {
        out.push(pending[idx].command.clone());
        idx += 1;
    }

    out
}

pub fn cwd_nav_path(name: &str) -> Option<PathBuf> {
    let stem = nav_stem(name);

    if stem.is_empty() || stem.contains("..") || stem.contains('/') || stem.contains('\\') {
        return None;
    }

    let cwd = std::env::current_dir().ok()?;

    Some(cwd.join("maps").join(format!("{stem}.nav")))
}

pub fn compress_nav(bytes: &[u8]) -> Vec<u8> {
    zstd::bulk::compress(bytes, 3).unwrap_or_else(|_| bytes.to_vec())
}

pub fn split_wire(bytes: &[u8], limit: usize) -> Vec<Vec<u8>> {
    let limit = limit.max(1);
    let mut out = Vec::new();

    if bytes.is_empty() {
        out.push(Vec::new());

        return out;
    }

    let mut idx = 0;

    while idx < bytes.len() {
        let end = (idx + limit).min(bytes.len());
        out.push(bytes[idx..end].to_vec());
        idx = end;
    }

    out
}

pub fn debug_vertices(state: &NavState, draw: Anchor) -> Vec<f32> {
    if !state.show {
        return Vec::new();
    }

    let mut vertices = Vec::new();
    let mut idx = 0;

    while idx < state.mesh.polys.len() {
        let poly = &state.mesh.polys[idx];
        let color = tag_color(poly.tag);
        let mut vert = 0;

        while vert < poly.verts.len() {
            let a = poly.verts[vert];
            let b = poly.verts[(vert + 1) % poly.verts.len()];
            push_world_ribbon(&mut vertices, a, b, 0.02, color, draw);
            vert += 1;
        }

        idx += 1;
    }

    idx = 0;

    while idx < state.mesh.offmesh.len() {
        let link = &state.mesh.offmesh[idx];
        let color = if link.kind == OffKind::Jump {
            [0.95, 0.45, 0.15]
        } else {
            [0.72, 0.38, 0.95]
        };
        push_arc(&mut vertices, link.start, link.end, state.mesh.params.jump_height, color, draw);
        idx += 1;
    }

    push_polyline(&mut vertices, &state.debug_path, [0.95, 0.95, 0.95], draw);
    push_polyline(&mut vertices, &state.follow_path, [0.95, 0.72, 0.28], draw);

    vertices
}

pub fn snapshot_world(brushes: &BrushMap, voxels: &VoxelWorld, input: NavInput) -> NavSnapshot {
    let brush_list = if input == NavInput::Voxel {
        Vec::new()
    } else {
        brushes.nav_brushes()
    };
    let block_list = if input == NavInput::Brush {
        Vec::new()
    } else {
        voxels.nav_blocks()
    };

    NavSnapshot {
        brushes: brush_list,
        blocks: block_list,
        input,
    }
}

fn bake(snapshot: &NavSnapshot, params: NavParams) -> NavMesh {
    let tiles = plan_tiles(snapshot, params);

    if tiles.is_empty() {
        return NavMesh::empty(params, snapshot.input);
    }

    let mut products = Vec::with_capacity(tiles.len());
    let mut idx = 0;

    while idx < tiles.len() {
        products.push(build_tile(snapshot, params, tiles[idx]));
        idx += 1;
    }

    assemble(snapshot.input, params, products)
}

fn plan_tiles(snapshot: &NavSnapshot, params: NavParams) -> Vec<TileRect> {
    let Some((min, max)) = snapshot_bounds(snapshot) else {
        return Vec::new();
    };
    let cell = params.cell();
    let Some(min_x) = column_floor(min.x, cell) else {
        return Vec::new();
    };
    let Some(min_y) = column_floor(min.y, cell) else {
        return Vec::new();
    };
    let Some(max_x) = column_ceil(max.x, cell) else {
        return Vec::new();
    };
    let Some(max_y) = column_ceil(max.y, cell) else {
        return Vec::new();
    };

    if max_x <= min_x || max_y <= min_y {
        return Vec::new();
    }

    let mut tiles = Vec::new();
    let mut ty = min_y;

    while ty < max_y {
        let mut tx = min_x;

        while tx < max_x {
            tiles.push(TileRect {
                x0: tx,
                x1: (tx + TILE).min(max_x),
                y0: ty,
                y1: (ty + TILE).min(max_y),
            });
            tx += TILE;
        }

        ty += TILE;
    }

    tiles
}

#[derive(Clone, Copy)]
struct TileRect {
    x0: i32,
    x1: i32,
    y0: i32,
    y1: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct SpanId {
    x: i32,
    y: i32,
    q: i32,
}

struct RawSpan {
    id: SpanId,
    floor: f64,
    tag: NavTag,
}

#[derive(Clone, Copy)]
struct RawOff {
    from: SpanId,
    to: SpanId,
    kind: OffKind,
    start: Vector3,
    end: Vector3,
}

struct TileProduct {
    spans: Vec<RawSpan>,
    links: Vec<(SpanId, SpanId)>,
    offmesh: Vec<RawOff>,
}

struct Solid {
    z0: f64,
    z1: f64,
    walkable: bool,
    tag: NavTag,
}

struct Column {
    solids: Vec<(f64, f64)>,
    floors: Vec<(f64, NavTag)>,
}

struct Kept {
    id: SpanId,
    floor: f64,
    tag: NavTag,
}

fn build_tile(snapshot: &NavSnapshot, params: NavParams, core: TileRect) -> TileProduct {
    let cell = params.cell();
    let radius_pad = (params.radius / cell).ceil() as i32 + 1;
    let jump_pad = (params.jump_dist / cell).ceil() as i32;
    let pad = radius_pad + jump_pad;
    let x0 = core.x0.saturating_sub(pad);
    let x1 = core.x1.saturating_add(pad);
    let y0 = core.y0.saturating_sub(pad);
    let y1 = core.y1.saturating_add(pad);
    let mut columns: HashMap<(i32, i32), Vec<Solid>> = HashMap::new();
    let world_min_x = x0 as f64 * cell;
    let world_max_x = x1 as f64 * cell;
    let world_min_y = y0 as f64 * cell;
    let world_max_y = y1 as f64 * cell;
    let mut idx = 0;

    while idx < snapshot.blocks.len() {
        let block = &snapshot.blocks[idx];
        idx += 1;

        if block.max.x < world_min_x
            || block.min.x > world_max_x
            || block.max.y < world_min_y
            || block.min.y > world_max_y
        {
            continue;
        }

        let Some((cx0, cx1)) = column_span(block.min.x, block.max.x, cell) else {
            continue;
        };
        let Some((cy0, cy1)) = column_span(block.min.y, block.max.y, cell) else {
            continue;
        };
        let mut y = cy0.max(y0);

        while y <= cy1.min(y1 - 1) {
            let mut x = cx0.max(x0);

            while x <= cx1.min(x1 - 1) {
                columns.entry((x, y)).or_default().push(Solid {
                    z0: block.min.z,
                    z1: block.max.z,
                    walkable: !block.water,
                    tag: NavTag::Voxel,
                });
                x += 1;
            }

            y += 1;
        }
    }

    idx = 0;

    while idx < snapshot.brushes.len() {
        let brush = &snapshot.brushes[idx];
        idx += 1;

        if brush.max.x < world_min_x - cell
            || brush.min.x > world_max_x + cell
            || brush.max.y < world_min_y - cell
            || brush.min.y > world_max_y + cell
        {
            continue;
        }

        let Some((cx0, cx1)) = column_span(brush.min.x - cell, brush.max.x + cell, cell) else {
            continue;
        };
        let Some((cy0, cy1)) = column_span(brush.min.y - cell, brush.max.y + cell, cell) else {
            continue;
        };
        let mut y = cy0.max(y0);

        while y <= cy1.min(y1 - 1) {
            let mut x = cx0.max(x0);

            while x <= cx1.min(x1 - 1) {
                let center_x = (x as f64 + 0.5) * cell;
                let center_y = (y as f64 + 0.5) * cell;

                if let Some(solid) = brush_column(brush, center_x, center_y, params.slope) {
                    columns.entry((x, y)).or_default().push(solid);
                }

                x += 1;
            }

            y += 1;
        }
    }

    let mut merged: HashMap<(i32, i32), Column> = HashMap::new();
    let mut keys: Vec<(i32, i32)> = columns.keys().copied().collect();
    keys.sort_unstable();
    idx = 0;

    while idx < keys.len() {
        let key = keys[idx];
        let mut solids = columns.remove(&key).unwrap_or_default();
        merge_solids(&mut solids);
        let mut floors = Vec::new();
        let mut solid_idx = 0;
        let mut intervals = Vec::with_capacity(solids.len());

        while solid_idx < solids.len() {
            let solid = &solids[solid_idx];
            intervals.push((solid.z0, solid.z1));

            if solid.walkable && open_above(&solids, solid.z1, params.height) {
                floors.push((solid.z1, solid.tag));
            }

            solid_idx += 1;
        }

        if !floors.is_empty() || !intervals.is_empty() {
            merged.insert(
                key,
                Column {
                    solids: intervals,
                    floors,
                },
            );
        }

        idx += 1;
    }

    let mut kept = Vec::new();
    let mut kept_keys: Vec<(i32, i32)> = merged.keys().copied().collect();
    kept_keys.sort_unstable();
    idx = 0;

    while idx < kept_keys.len() {
        let (x, y) = kept_keys[idx];
        let column = &merged[&(x, y)];
        let mut floor_idx = 0;

        while floor_idx < column.floors.len() {
            let (floor, tag) = column.floors[floor_idx];

            if survives(&merged, x, y, floor, params, cell) {
                kept.push(Kept {
                    id: SpanId {
                        x,
                        y,
                        q: quantize(floor),
                    },
                    floor,
                    tag,
                });
            }

            floor_idx += 1;
        }

        idx += 1;
    }

    kept.sort_by(|a, b| a.id.cmp(&b.id));
    let mut by_col: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    idx = 0;

    while idx < kept.len() {
        by_col.entry((kept[idx].id.x, kept[idx].id.y)).or_default().push(idx);
        idx += 1;
    }

    let reach = (params.jump_dist / cell).ceil() as i32;
    let mut product = TileProduct {
        spans: Vec::new(),
        links: Vec::new(),
        offmesh: Vec::new(),
    };
    idx = 0;

    while idx < kept.len() {
        let span = &kept[idx];

        if span.id.x < core.x0 || span.id.x >= core.x1 || span.id.y < core.y0 || span.id.y >= core.y1
        {
            idx += 1;

            continue;
        }

        product.spans.push(RawSpan {
            id: span.id,
            floor: span.floor,
            tag: span.tag,
        });
        let mut dir = 0;

        while dir < 8 {
            let (dx, dy) = DIRS[dir];
            dir += 1;
            let nx = span.id.x + dx;
            let ny = span.id.y + dy;

            if dx.abs() == 1 && dy.abs() == 1 {
                if !column_step(&by_col, &kept, span.id.x + dx, span.id.y, span.floor, params.step)
                    || !column_step(
                        &by_col,
                        &kept,
                        span.id.x,
                        span.id.y + dy,
                        span.floor,
                        params.step,
                    )
                {
                    continue;
                }
            }

            let Some(list) = by_col.get(&(nx, ny)) else {
                continue;
            };
            let mut list_idx = 0;

            while list_idx < list.len() {
                let other = &kept[list[list_idx]];
                list_idx += 1;

                if (other.floor - span.floor).abs() <= params.step {
                    product.links.push((span.id, other.id));
                }
            }
        }

        if !is_ledge(&by_col, &kept, span, params.step) {
            idx += 1;

            continue;
        }

        let mut jumps = Vec::new();
        let mut drops = Vec::new();
        let mut dy = -reach;

        while dy <= reach {
            let mut dx = -reach;

            while dx <= reach {
                if let Some(list) = by_col.get(&(span.id.x + dx, span.id.y + dy)) {
                    let mut list_idx = 0;

                    while list_idx < list.len() {
                        let other = &kept[list[list_idx]];
                        list_idx += 1;

                        if other.id == span.id {
                            continue;
                        }

                        let dist = span_dist(span, other, cell);

                        if dist > params.jump_dist || dist < cell * 0.5 {
                            continue;
                        }

                        let dz = other.floor - span.floor;
                        let kind = if dz > params.step && dz <= params.jump_height {
                            OffKind::Jump
                        } else if -dz > params.step && -dz <= params.max_drop {
                            OffKind::Drop
                        } else {
                            continue;
                        };

                        if kind == OffKind::Jump {
                            jumps.push((dist, other.id, other.floor));
                        } else {
                            drops.push((dist, other.id, other.floor));
                        }
                    }
                }

                dx += 1;
            }

            dy += 1;
        }

        jumps.sort_by(|a, b| a.0.total_cmp(&b.0));
        drops.sort_by(|a, b| a.0.total_cmp(&b.0));
        push_hops(
            &mut product.offmesh,
            &merged,
            span,
            &jumps,
            OffKind::Jump,
            params,
            cell,
            4,
        );
        push_hops(
            &mut product.offmesh,
            &merged,
            span,
            &drops,
            OffKind::Drop,
            params,
            cell,
            4,
        );
        idx += 1;
    }

    product
}

fn push_hops(
    out: &mut Vec<RawOff>,
    columns: &HashMap<(i32, i32), Column>,
    span: &Kept,
    hops: &[(f64, SpanId, f64)],
    kind: OffKind,
    params: NavParams,
    cell: f64,
    limit: usize,
) {
    let mut added = 0;
    let mut idx = 0;

    while idx < hops.len() && added < limit {
        let (_, id, floor) = hops[idx];
        idx += 1;
        let start = span_center(span.id.x, span.id.y, span.floor, cell);
        let end = span_center(id.x, id.y, floor, cell);

        if arc_clear(columns, start, end, cell, params.height, params.jump_height) {
            out.push(RawOff {
                from: span.id,
                to: id,
                kind,
                start,
                end,
            });
            added += 1;
        }
    }
}

fn assemble(input: NavInput, params: NavParams, tiles: Vec<TileProduct>) -> NavMesh {
    let cell = params.cell();
    let mut spans: HashMap<SpanId, RawSpan> = HashMap::new();
    let mut links = Vec::new();
    let mut offmesh = Vec::new();
    let mut idx = 0;

    while idx < tiles.len() {
        let tile = &tiles[idx];
        let mut span_idx = 0;

        while span_idx < tile.spans.len() {
            let span = &tile.spans[span_idx];
            spans.insert(span.id, RawSpan {
                id: span.id,
                floor: span.floor,
                tag: span.tag,
            });
            span_idx += 1;
        }

        links.extend_from_slice(&tile.links);
        offmesh.extend_from_slice(&tile.offmesh);
        idx += 1;
    }

    let mut groups: HashMap<(i32, u8), HashMap<(i32, i32), f64>> = HashMap::new();

    for span in spans.values() {
        groups
            .entry((span.id.q, tag_byte(span.tag)))
            .or_default()
            .insert((span.id.x, span.id.y), span.floor);
    }

    let mut group_keys: Vec<(i32, u8)> = groups.keys().copied().collect();
    group_keys.sort_unstable();
    let mut polys = Vec::new();
    let mut span_poly: HashMap<SpanId, u32> = HashMap::new();
    idx = 0;

    while idx < group_keys.len() {
        let key = group_keys[idx];
        let cells = groups.remove(&key).unwrap_or_default();
        let tag = tag_from(key.1).unwrap_or(NavTag::Brush);
        greedy_polys(&cells, key.0, tag, cell, &mut polys, &mut span_poly);
        idx += 1;
    }

    let mut doors: HashMap<(u32, u32), Door> = HashMap::new();
    idx = 0;

    while idx < links.len() {
        let (a, b) = links[idx];
        idx += 1;
        let (Some(a_span), Some(b_span)) = (spans.get(&a), spans.get(&b)) else {
            continue;
        };
        let (Some(&poly_a), Some(&poly_b)) = (span_poly.get(&a), span_poly.get(&b)) else {
            continue;
        };

        if poly_a == poly_b {
            continue;
        }

        let portal = shared_portal(a_span, b_span, cell);
        let pair = if poly_a < poly_b {
            (poly_a, poly_b)
        } else {
            (poly_b, poly_a)
        };
        let door = doors.entry(pair).or_insert_with(Door::point);
        door.absorb(portal);
    }

    for ((poly_a, poly_b), door) in doors {
        let (p0, p1) = door.segment();
        add_neighbor(&mut polys, poly_a, poly_b, p0, p1);
        add_neighbor(&mut polys, poly_b, poly_a, p0, p1);
    }

    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    idx = 0;

    while idx < offmesh.len() {
        let link = &offmesh[idx];
        idx += 1;
        let (Some(&from), Some(&to)) = (span_poly.get(&link.from), span_poly.get(&link.to)) else {
            continue;
        };

        if from == to {
            continue;
        }

        let key = (
            from,
            to,
            kind_byte(link.kind),
            quantize(link.start.x),
            quantize(link.start.y),
            quantize(link.end.x),
            quantize(link.end.y),
        );

        if !seen.insert(key) {
            continue;
        }

        resolved.push(NavOff {
            from,
            to,
            kind: link.kind,
            start: link.start,
            end: link.end,
        });
    }

    resolved.sort_by(|a, b| {
        a.from.cmp(&b.from).then(a.to.cmp(&b.to)).then(
            a.start
                .x
                .total_cmp(&b.start.x)
                .then(a.start.y.total_cmp(&b.start.y)),
        )
    });

    NavMesh {
        polys,
        offmesh: resolved,
        params,
        input,
    }
}

fn greedy_polys(
    cells: &HashMap<(i32, i32), f64>,
    q: i32,
    tag: NavTag,
    cell: f64,
    polys: &mut Vec<NavPoly>,
    span_poly: &mut HashMap<SpanId, u32>,
) {
    let mut order: Vec<(i32, i32)> = cells.keys().copied().collect();
    order.sort_unstable_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let mut visited = HashSet::new();
    let mut idx = 0;

    while idx < order.len() {
        let (x, y) = order[idx];
        idx += 1;

        if visited.contains(&(x, y)) {
            continue;
        }

        let mut width = 1;

        while cells.contains_key(&(x + width, y)) && !visited.contains(&(x + width, y)) {
            width += 1;
        }

        let mut height = 1;

        loop {
            let mut dx = 0;
            let mut row_ok = true;

            while dx < width {
                let key = (x + dx, y + height);

                if !cells.contains_key(&key) || visited.contains(&key) {
                    row_ok = false;

                    break;
                }

                dx += 1;
            }

            if !row_ok {
                break;
            }

            height += 1;
        }

        let floor = cells[&(x, y)];
        let x0 = x as f64 * cell;
        let y0 = y as f64 * cell;
        let x1 = (x + width) as f64 * cell;
        let y1 = (y + height) as f64 * cell;
        let poly_index = polys.len() as u32;
        polys.push(NavPoly {
            verts: vec![
                Vector3::new(x0, y0, floor),
                Vector3::new(x1, y0, floor),
                Vector3::new(x1, y1, floor),
                Vector3::new(x0, y1, floor),
            ],
            tag,
            neighbors: Vec::new(),
            portals: Vec::new(),
        });
        let mut dy = 0;

        while dy < height {
            let mut dx = 0;

            while dx < width {
                visited.insert((x + dx, y + dy));
                span_poly.insert(
                    SpanId {
                        x: x + dx,
                        y: y + dy,
                        q,
                    },
                    poly_index,
                );
                dx += 1;
            }

            dy += 1;
        }
    }
}

struct Door {
    orthogonal: Option<PortalSeg>,
    point: Option<(Vector3, Vector3)>,
}

impl Door {
    fn point() -> Self {
        Self {
            orthogonal: None,
            point: None,
        }
    }

    fn absorb(&mut self, portal: PortalSeg) {
        if portal.axis == 2 {
            if self.point.is_none() {
                self.point = Some(portal.segment());
            }

            return;
        }

        if let Some(existing) = &mut self.orthogonal {
            if existing.axis == portal.axis && (existing.fixed - portal.fixed).abs() <= MERGE_EPS {
                existing.min = existing.min.min(portal.min);
                existing.max = existing.max.max(portal.max);
                existing.z = (existing.z + portal.z) * 0.5;

                return;
            }

            let existing_len = existing.max - existing.min;
            let next_len = portal.max - portal.min;

            if next_len > existing_len {
                *existing = portal;
            }

            return;
        }

        self.orthogonal = Some(portal);
    }

    fn segment(&self) -> (Vector3, Vector3) {
        if let Some(portal) = &self.orthogonal {
            return portal.segment();
        }

        self.point.unwrap_or((
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
        ))
    }
}

struct PortalSeg {
    axis: u8,
    fixed: f64,
    min: f64,
    max: f64,
    z: f64,
}

impl PortalSeg {
    fn segment(&self) -> (Vector3, Vector3) {
        if self.axis == 0 {
            return (
                Vector3::new(self.fixed, self.min, self.z),
                Vector3::new(self.fixed, self.max, self.z),
            );
        }

        if self.axis == 1 {
            return (
                Vector3::new(self.min, self.fixed, self.z),
                Vector3::new(self.max, self.fixed, self.z),
            );
        }

        let point = Vector3::new(self.min, self.max, self.z);

        (point, point)
    }
}

fn shared_portal(a: &RawSpan, b: &RawSpan, cell: f64) -> PortalSeg {
    let dx = b.id.x - a.id.x;
    let dy = b.id.y - a.id.y;
    let z = (a.floor + b.floor) * 0.5;

    if dx.abs() + dy.abs() == 1 {
        if dx != 0 {
            let x = a.id.x.max(b.id.x) as f64 * cell;
            let y0 = a.id.y as f64 * cell;

            return PortalSeg {
                axis: 0,
                fixed: x,
                min: y0,
                max: y0 + cell,
                z,
            };
        }

        let y = a.id.y.max(b.id.y) as f64 * cell;
        let x0 = a.id.x as f64 * cell;

        return PortalSeg {
            axis: 1,
            fixed: y,
            min: x0,
            max: x0 + cell,
            z,
        };
    }

    PortalSeg {
        axis: 2,
        fixed: 0.0,
        min: a.id.x.max(b.id.x) as f64 * cell,
        max: a.id.y.max(b.id.y) as f64 * cell,
        z,
    }
}

fn add_neighbor(polys: &mut [NavPoly], from: u32, to: u32, a: Vector3, b: Vector3) {
    let poly = &mut polys[from as usize];
    let mut idx = 0;

    while idx < poly.neighbors.len() {
        if poly.neighbors[idx] == to {
            return;
        }

        idx += 1;
    }

    poly.neighbors.push(to);
    poly.portals.push((a, b));
}

const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

fn survives(
    columns: &HashMap<(i32, i32), Column>,
    x: i32,
    y: i32,
    floor: f64,
    params: NavParams,
    cell: f64,
) -> bool {
    let reach = (params.radius / cell).ceil() as i32;
    let mut dy = -reach;

    while dy <= reach {
        let mut dx = -reach;

        while dx <= reach {
            let dist_x = dx as f64 * cell;
            let dist_y = dy as f64 * cell;

            if dist_x * dist_x + dist_y * dist_y <= params.radius * params.radius + 1.0e-8 {
                let key = (x + dx, y + dy);
                let Some(column) = columns.get(&key) else {
                    return false;
                };

                if !column_has_floor(column, floor, params.step) {
                    return false;
                }

                if body_blocked_above(&column.solids, floor, params.height, floor + params.step) {
                    return false;
                }
            }

            dx += 1;
        }

        dy += 1;
    }

    true
}

fn column_has_floor(column: &Column, floor: f64, step: f64) -> bool {
    let mut idx = 0;

    while idx < column.floors.len() {
        if (column.floors[idx].0 - floor).abs() <= step {
            return true;
        }

        idx += 1;
    }

    false
}

fn column_step(
    by_col: &HashMap<(i32, i32), Vec<usize>>,
    kept: &[Kept],
    x: i32,
    y: i32,
    floor: f64,
    step: f64,
) -> bool {
    let Some(list) = by_col.get(&(x, y)) else {
        return false;
    };
    let mut idx = 0;

    while idx < list.len() {
        if (kept[list[idx]].floor - floor).abs() <= step {
            return true;
        }

        idx += 1;
    }

    false
}

fn is_ledge(
    by_col: &HashMap<(i32, i32), Vec<usize>>,
    kept: &[Kept],
    span: &Kept,
    step: f64,
) -> bool {
    let mut dir = 0;

    while dir < 8 {
        let (dx, dy) = DIRS[dir];
        dir += 1;

        if !column_step(by_col, kept, span.id.x + dx, span.id.y + dy, span.floor, step) {
            return true;
        }
    }

    false
}

fn span_dist(a: &Kept, b: &Kept, cell: f64) -> f64 {
    let dx = (b.id.x - a.id.x) as f64 * cell;
    let dy = (b.id.y - a.id.y) as f64 * cell;

    (dx * dx + dy * dy).sqrt()
}

fn span_center(x: i32, y: i32, floor: f64, cell: f64) -> Vector3 {
    Vector3::new((x as f64 + 0.5) * cell, (y as f64 + 0.5) * cell, floor)
}

fn arc_clear(
    columns: &HashMap<(i32, i32), Column>,
    start: Vector3,
    end: Vector3,
    cell: f64,
    height: f64,
    jump_height: f64,
) -> bool {
    let samples = 8;
    let mut step = 1;

    while step < samples {
        let t = step as f64 / samples as f64;
        let x = start.x + (end.x - start.x) * t;
        let y = start.y + (end.y - start.y) * t;
        let z = arc_z(start.z, end.z, t, jump_height);
        let col_x = (x / cell).floor() as i32;
        let col_y = (y / cell).floor() as i32;

        if let Some(column) = columns.get(&(col_x, col_y)) {
            if body_blocked_above(&column.solids, z, height, start.z.max(end.z)) {
                return false;
            }
        }

        step += 1;
    }

    true
}

fn arc_z(z0: f64, z1: f64, t: f64, jump_height: f64) -> f64 {
    let hop = (jump_height * 0.3).min(0.45);

    if z1 + 1.0e-4 >= z0 {
        let rise = (t / 0.65).clamp(0.0, 1.0);
        let base = z0 + (z1 - z0) * rise;

        return base + hop * 4.0 * t * (1.0 - t);
    }

    let fall = ((t - 0.2) / 0.8).clamp(0.0, 1.0);
    let base = z0 + (z1 - z0) * fall;

    base + hop * 4.0 * t * (1.0 - t)
}

fn body_blocked(solids: &[(f64, f64)], feet: f64, height: f64) -> bool {
    body_blocked_above(solids, feet, height, feet)
}

fn body_blocked_above(solids: &[(f64, f64)], feet: f64, height: f64, ground: f64) -> bool {
    let head = feet + height;
    let mut idx = 0;

    while idx < solids.len() {
        let (z0, z1) = solids[idx];

        if z1 > ground + CLEAR_EPS && z0 < head - CLEAR_EPS && z1 > feet + CLEAR_EPS {
            return true;
        }

        idx += 1;
    }

    false
}

fn open_above(solids: &[Solid], floor: f64, height: f64) -> bool {
    let head = floor + height;
    let mut idx = 0;

    while idx < solids.len() {
        let solid = &solids[idx];

        if solid.z0 < head - CLEAR_EPS && solid.z1 > floor + CLEAR_EPS {
            return false;
        }

        idx += 1;
    }

    true
}

fn merge_solids(list: &mut Vec<Solid>) {
    if list.len() < 2 {
        return;
    }

    list.sort_by(|a, b| a.z0.total_cmp(&b.z0).then(a.z1.total_cmp(&b.z1)));
    let mut merged = Vec::with_capacity(list.len());
    merged.push(list[0].clone_solid());
    let mut idx = 1;

    while idx < list.len() {
        let next = &list[idx];
        let prev = merged.len() - 1;

        if next.z0 <= merged[prev].z1 + MERGE_EPS {
            let top_delta = (next.z1 - merged[prev].z1).abs();

            if next.z1 > merged[prev].z1 + 1.0e-6 {
                if top_delta <= MERGE_EPS && next.walkable && merged[prev].walkable {
                    merged[prev].tag = combine_tag(merged[prev].tag, next.tag);
                } else if top_delta <= MERGE_EPS && next.walkable {
                    merged[prev].walkable = true;
                    merged[prev].tag = next.tag;
                } else if top_delta > MERGE_EPS {
                    merged[prev].walkable = next.walkable;
                    merged[prev].tag = next.tag;
                }

                merged[prev].z1 = next.z1;
            } else if top_delta <= MERGE_EPS && next.walkable {
                if merged[prev].walkable {
                    merged[prev].tag = combine_tag(merged[prev].tag, next.tag);
                } else {
                    merged[prev].walkable = true;
                    merged[prev].tag = next.tag;
                }
            }
        } else {
            merged.push(next.clone_solid());
        }

        idx += 1;
    }

    *list = merged;
}

impl Solid {
    fn clone_solid(&self) -> Self {
        Self {
            z0: self.z0,
            z1: self.z1,
            walkable: self.walkable,
            tag: self.tag,
        }
    }
}

fn combine_tag(a: NavTag, b: NavTag) -> NavTag {
    if a == b {
        return a;
    }

    NavTag::Both
}

fn brush_column(brush: &NavBrush, x: f64, y: f64, slope: f64) -> Option<Solid> {
    let mut z_lo = f64::NEG_INFINITY;
    let mut z_hi = f64::INFINITY;
    let mut top_nz = 0.0;
    let mut idx = 0;

    while idx < brush.planes.len() {
        let plane = &brush.planes[idx];
        idx += 1;
        let nx = plane.normal.x;
        let ny = plane.normal.y;
        let nz = plane.normal.z;
        let length = (nx * nx + ny * ny + nz * nz).sqrt();

        if length < 1.0e-8 {
            continue;
        }

        let nx = nx / length;
        let ny = ny / length;
        let nz = nz / length;
        let distance = plane.distance / length;
        let flat = distance - nx * x - ny * y;

        if nz.abs() < 1.0e-8 {
            if nx * x + ny * y > distance + 1.0e-4 {
                return None;
            }

            continue;
        }

        let z = flat / nz;

        if nz > 0.0 {
            if z < z_hi {
                z_hi = z;
                top_nz = nz;
            }
        } else if z > z_lo {
            z_lo = z;
        }
    }

    if !z_lo.is_finite() || !z_hi.is_finite() || z_hi - z_lo < 1.0e-4 {
        return None;
    }

    Some(Solid {
        z0: z_lo,
        z1: z_hi,
        walkable: top_nz >= slope,
        tag: NavTag::Brush,
    })
}

fn snapshot_bounds(snapshot: &NavSnapshot) -> Option<(Vector3, Vector3)> {
    let mut min = Vector3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut max = Vector3::new(f64::MIN, f64::MIN, f64::MIN);
    let mut any = false;
    let mut idx = 0;

    while idx < snapshot.brushes.len() {
        grow_bounds(&mut min, &mut max, snapshot.brushes[idx].min, snapshot.brushes[idx].max);
        any = true;
        idx += 1;
    }

    idx = 0;

    while idx < snapshot.blocks.len() {
        grow_bounds(&mut min, &mut max, snapshot.blocks[idx].min, snapshot.blocks[idx].max);
        any = true;
        idx += 1;
    }

    if !any {
        return None;
    }

    Some((min, max))
}

fn grow_bounds(min: &mut Vector3, max: &mut Vector3, next_min: Vector3, next_max: Vector3) {
    min.x = min.x.min(next_min.x);
    min.y = min.y.min(next_min.y);
    min.z = min.z.min(next_min.z);
    max.x = max.x.max(next_max.x);
    max.y = max.y.max(next_max.y);
    max.z = max.z.max(next_max.z);
}

fn column_floor(value: f64, cell: f64) -> Option<i32> {
    let scaled = (value / cell).floor();

    if !scaled.is_finite() || scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        return None;
    }

    Some(scaled as i32)
}

fn column_ceil(value: f64, cell: f64) -> Option<i32> {
    let scaled = (value / cell).ceil();

    if !scaled.is_finite() || scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        return None;
    }

    Some(scaled as i32)
}

fn column_span(min: f64, max: f64, cell: f64) -> Option<(i32, i32)> {
    if cell <= 0.0 || !min.is_finite() || !max.is_finite() || max <= min {
        return None;
    }

    let first = (min / cell - 0.5).ceil();
    let last = (max / cell - 0.5 - 1.0e-9).floor();

    if !first.is_finite() || !last.is_finite() || first > last {
        return None;
    }

    if first < i32::MIN as f64 || last > i32::MAX as f64 {
        return None;
    }

    Some((first as i32, last as i32))
}

fn quantize(value: f64) -> i32 {
    (value * 1000.0).round() as i32
}

fn locate(mesh: &NavMesh, point: Vector3) -> Option<usize> {
    let mut best = None;
    let mut best_dz = f64::MAX;
    let mut idx = 0;

    while idx < mesh.polys.len() {
        let poly = &mesh.polys[idx];
        let floor = poly_floor(poly);
        let dz = (point.z - floor).abs();

        if contains_xy(&poly.verts, point)
            && point.z >= floor - mesh.params.step
            && point.z <= floor + mesh.params.height
            && dz < best_dz
        {
            best = Some(idx);
            best_dz = dz;
        }

        idx += 1;
    }

    if best.is_some() {
        return best;
    }

    let mut nearest = None;
    let mut nearest_dist = f64::MAX;
    let limit = mesh.params.jump_dist.max(1.0);
    let z_limit = mesh.params.height.max(mesh.params.max_drop);
    idx = 0;

    while idx < mesh.polys.len() {
        let poly = &mesh.polys[idx];
        let center = centroid(&poly.verts);
        let floor = poly_floor(poly);
        let dx = center.x - point.x;
        let dy = center.y - point.y;
        let dist = (dx * dx + dy * dy).sqrt();

        if dist <= limit && (point.z - floor).abs() <= z_limit && dist < nearest_dist {
            nearest = Some(idx);
            nearest_dist = dist;
        }

        idx += 1;
    }

    nearest
}

fn poly_floor(poly: &NavPoly) -> f64 {
    if poly.verts.is_empty() {
        return 0.0;
    }

    poly.verts[0].z
}

fn on_floor(poly: &NavPoly, point: Vector3) -> Vector3 {
    Vector3::new(point.x, point.y, poly_floor(poly))
}

fn contains_xy(verts: &[Vector3], point: Vector3) -> bool {
    if verts.len() < 3 {
        return false;
    }

    let mut sign = 0.0f64;
    let mut idx = 0;

    while idx < verts.len() {
        let a = verts[idx];
        let b = verts[(idx + 1) % verts.len()];
        let cross = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);

        if cross.abs() > 1.0e-8 {
            if sign != 0.0 && cross.signum() != sign.signum() {
                return false;
            }

            sign = cross.signum();
        }

        idx += 1;
    }

    true
}

fn centroid(verts: &[Vector3]) -> Vector3 {
    if verts.is_empty() {
        return Vector3::new(0.0, 0.0, 0.0);
    }

    let mut sum = Vector3::new(0.0, 0.0, 0.0);
    let mut idx = 0;

    while idx < verts.len() {
        sum.x += verts[idx].x;
        sum.y += verts[idx].y;
        sum.z += verts[idx].z;
        idx += 1;
    }

    let scale = verts.len() as f64;

    Vector3::new(sum.x / scale, sum.y / scale, sum.z / scale)
}

fn centroids(mesh: &NavMesh) -> Vec<Vector3> {
    let mut out = Vec::with_capacity(mesh.polys.len());
    let mut idx = 0;

    while idx < mesh.polys.len() {
        out.push(centroid(&mesh.polys[idx].verts));
        idx += 1;
    }

    out
}

fn adjacency(mesh: &NavMesh, centroids: &[Vector3]) -> Vec<Vec<Edge>> {
    let mut adj = Vec::with_capacity(mesh.polys.len());
    let mut idx = 0;

    while idx < mesh.polys.len() {
        let poly = &mesh.polys[idx];
        let mut edges = Vec::with_capacity(poly.neighbors.len());
        let mut neighbor = 0;

        while neighbor < poly.neighbors.len() {
            let to = poly.neighbors[neighbor];
            edges.push(Edge {
                to,
                cost: dist3(centroids[idx], centroids[to as usize]),
                hop: None,
            });
            neighbor += 1;
        }

        adj.push(edges);
        idx += 1;
    }

    idx = 0;

    while idx < mesh.offmesh.len() {
        let link = &mesh.offmesh[idx];
        let from = link.from as usize;

        if from < adj.len() {
            adj[from].push(Edge {
                to: link.to,
                cost: dist3(link.start, link.end) * 1.15,
                hop: Some(idx),
            });
        }

        idx += 1;
    }

    adj
}

fn funnel_chunk(mesh: &NavMesh, polys: &[usize], from: Vector3, to: Vector3) -> Vec<Vector3> {
    if polys.is_empty() {
        return vec![to];
    }

    if polys.len() == 1 {
        return dedupe(vec![from, to]);
    }

    let mut portals = Vec::new();
    let mut idx = 0;

    while idx + 1 < polys.len() {
        portals.push(oriented_portal(mesh, polys[idx], polys[idx + 1]));
        idx += 1;
    }

    string_pull(from, to, &portals)
}

fn oriented_portal(mesh: &NavMesh, from: usize, to: usize) -> (Vector3, Vector3) {
    let poly = &mesh.polys[from];
    let mut idx = 0;

    while idx < poly.neighbors.len() {
        if poly.neighbors[idx] == to as u32 {
            let portal = poly.portals[idx];
            let a = centroid(&poly.verts);
            let b = centroid(&mesh.polys[to].verts);

            return orient_portal(a, b, portal.0, portal.1);
        }

        idx += 1;
    }

    let center = centroid(&poly.verts);

    (center, center)
}

fn orient_portal(from: Vector3, to: Vector3, a: Vector3, b: Vector3) -> (Vector3, Vector3) {
    let dir_x = to.x - from.x;
    let dir_y = to.y - from.y;
    let left_x = -dir_y;
    let left_y = dir_x;
    let mid_x = (a.x + b.x) * 0.5;
    let mid_y = (a.y + b.y) * 0.5;
    let da = (a.x - mid_x) * left_x + (a.y - mid_y) * left_y;
    let db = (b.x - mid_x) * left_x + (b.y - mid_y) * left_y;

    if da >= db {
        (a, b)
    } else {
        (b, a)
    }
}

fn string_pull(start: Vector3, goal: Vector3, portals: &[(Vector3, Vector3)]) -> Vec<Vector3> {
    let mut lefts = Vec::with_capacity(portals.len() + 2);
    let mut rights = Vec::with_capacity(portals.len() + 2);
    lefts.push(start);
    rights.push(start);
    let mut idx = 0;

    while idx < portals.len() {
        lefts.push(portals[idx].0);
        rights.push(portals[idx].1);
        idx += 1;
    }

    lefts.push(goal);
    rights.push(goal);
    let mut path = Vec::new();
    path.push(start);
    let mut apex = start;
    let mut left = start;
    let mut right = start;
    let mut apex_i = 0usize;
    let mut left_i = 0usize;
    let mut right_i = 0usize;
    let mut cursor = 1usize;
    let mut spins = 0usize;

    while cursor < lefts.len() {
        spins += 1;

        if spins > lefts.len() * lefts.len() + 8 {
            break;
        }
        let next_right = rights[cursor];
        let next_left = lefts[cursor];

        if tri_area(apex, right, next_right) <= 0.0 {
            if same_point(apex, right) || tri_area(apex, left, next_right) > 0.0 {
                right = next_right;
                right_i = cursor;
            } else {
                path.push(left);
                apex = left;
                apex_i = left_i;
                left = apex;
                right = apex;
                left_i = apex_i;
                right_i = apex_i;
                cursor = apex_i + 1;

                continue;
            }
        }

        if tri_area(apex, left, next_left) >= 0.0 {
            if same_point(apex, left) || tri_area(apex, right, next_left) < 0.0 {
                left = next_left;
                left_i = cursor;
            } else {
                path.push(right);
                apex = right;
                apex_i = right_i;
                left = apex;
                right = apex;
                left_i = apex_i;
                right_i = apex_i;
                cursor = apex_i + 1;

                continue;
            }
        }

        cursor += 1;
    }

    path.push(goal);

    path
}

fn tri_area(a: Vector3, b: Vector3, c: Vector3) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)
}

fn same_point(a: Vector3, b: Vector3) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;

    dx * dx + dy * dy <= 1.0e-10
}

fn append_path(path: &mut Vec<Vector3>, extra: Vec<Vector3>) {
    let mut idx = 0;

    while idx < extra.len() {
        path.push(extra[idx]);
        idx += 1;
    }
}

fn dedupe(points: Vec<Vector3>) -> Vec<Vector3> {
    let mut out = Vec::new();
    let mut idx = 0;

    while idx < points.len() {
        let point = points[idx];

        if out.last().is_none_or(|prev: &Vector3| dist3(*prev, point) > 0.02) {
            out.push(point);
        }

        idx += 1;
    }

    out
}

fn dist3(a: Vector3, b: Vector3) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;

    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn same_path(a: &[Vector3], b: &[Vector3]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut idx = 0;

    while idx < a.len() {
        if dist3(a[idx], b[idx]) > 1.0e-3 {
            return false;
        }

        idx += 1;
    }

    true
}

struct Job {
    epoch: u64,
    snapshot: Arc<NavSnapshot>,
    params: NavParams,
    tile: TileRect,
}

struct Done {
    epoch: u64,
    tile: TileProduct,
}

struct NavBaker {
    job_tx: Option<Sender<Job>>,
    done_rx: Receiver<Done>,
    workers: Vec<JoinHandle<()>>,
    epoch: u64,
    expected: usize,
    tiles: Vec<TileProduct>,
    params: NavParams,
    input: NavInput,
}

impl Drop for NavBaker {
    fn drop(&mut self) {
        self.job_tx = None;
        let mut workers = std::mem::take(&mut self.workers);

        while let Some(worker) = workers.pop() {
            let _ = worker.join();
        }
    }
}

impl NavBaker {
    fn start() -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let mut workers = Vec::new();
        let count = worker_count();
        let mut idx = 0;

        while idx < count {
            let jobs = Arc::clone(&job_rx);
            let done_tx = done_tx.clone();
            workers.push(std::thread::spawn(move || worker_loop(jobs, done_tx)));
            idx += 1;
        }

        drop(done_tx);

        Self {
            job_tx: Some(job_tx),
            done_rx,
            workers,
            epoch: 0,
            expected: 0,
            tiles: Vec::new(),
            params: NavParams::player(),
            input: NavInput::Both,
        }
    }

    fn cancel(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.expected = 0;
        self.tiles.clear();
    }

    fn submit(&mut self, snapshot: NavSnapshot, params: NavParams) -> Option<NavMesh> {
        self.epoch = self.epoch.wrapping_add(1);
        self.tiles.clear();
        self.expected = 0;
        self.params = params;
        self.input = snapshot.input;
        let tiles = plan_tiles(&snapshot, params);

        if tiles.is_empty() {
            return Some(NavMesh::empty(params, snapshot.input));
        }

        let Some(tx) = self.job_tx.as_ref() else {
            return Some(NavMesh::empty(params, snapshot.input));
        };
        let epoch = self.epoch;
        let snapshot = Arc::new(snapshot);
        let mut idx = 0;

        while idx < tiles.len() {
            if tx
                .send(Job {
                    epoch,
                    snapshot: Arc::clone(&snapshot),
                    params,
                    tile: tiles[idx],
                })
                .is_err()
            {
                log::warn!("[nav] worker stopped");
                self.expected = 0;

                return Some(NavMesh::empty(params, self.input));
            }

            idx += 1;
        }

        self.expected = tiles.len();
        log::info!("[nav] baking {} tiles", tiles.len());

        None
    }

    fn poll(&mut self) -> Option<NavMesh> {
        loop {
            let Ok(done) = self.done_rx.try_recv() else {
                break;
            };

            if done.epoch == self.epoch && self.expected > 0 {
                self.tiles.push(done.tile);
            }
        }

        if self.expected == 0 || self.tiles.len() < self.expected {
            return None;
        }

        let tiles = std::mem::take(&mut self.tiles);
        self.expected = 0;

        Some(assemble(self.input, self.params, tiles))
    }
}

fn worker_loop(jobs: Arc<Mutex<Receiver<Job>>>, done_tx: Sender<Done>) {
    loop {
        let job = {
            let guard = jobs.lock().expect("nav jobs");
            guard.recv()
        };
        let Ok(job) = job else {
            break;
        };
        let tile = build_tile(&job.snapshot, job.params, job.tile);

        if done_tx
            .send(Done {
                epoch: job.epoch,
                tile,
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

fn parse_command(tokens: &[String]) -> Result<NavCommand, String> {
    if tokens.is_empty() {
        return Err("empty nav command".to_string());
    }

    match tokens[0].as_str() {
        "nav_build" => parse_build(tokens),
        "nav_show" => {
            if tokens.len() != 2 {
                return Err("usage: nav_show <0|1>".to_string());
            }

            let enabled = match tokens[1].as_str() {
                "0" => false,
                "1" => true,
                _ => return Err("usage: nav_show <0|1>".to_string()),
            };

            Ok(NavCommand::Show { enabled })
        }
        "nav_path" => {
            if tokens.len() != 7 {
                return Err("usage: nav_path <x y z> <x y z>".to_string());
            }

            let start = Vector3::new(
                parse_number(&tokens[1])?,
                parse_number(&tokens[2])?,
                parse_number(&tokens[3])?,
            );
            let goal = Vector3::new(
                parse_number(&tokens[4])?,
                parse_number(&tokens[5])?,
                parse_number(&tokens[6])?,
            );

            Ok(NavCommand::Path { start, goal })
        }
        _ => Err(format!("unknown command '{}'", tokens[0])),
    }
}

fn parse_build(tokens: &[String]) -> Result<NavCommand, String> {
    if tokens.len() < 2 || tokens.len() > 9 {
        return Err(
            "usage: nav_build <brush|voxel|both> [radius height step slope jump_height jump_dist max_drop]"
                .to_string(),
        );
    }

    let input = match tokens[1].as_str() {
        "brush" => NavInput::Brush,
        "voxel" => NavInput::Voxel,
        "both" => NavInput::Both,
        _ => return Err("nav_build source must be brush, voxel, or both".to_string()),
    };
    let mut params = NavParams::player();
    let mut values = [
        params.radius,
        params.height,
        params.step,
        params.slope.acos().to_degrees(),
        params.jump_height,
        params.jump_dist,
        params.max_drop,
    ];
    let mut idx = 2;
    let mut slot = 0;

    while idx < tokens.len() {
        values[slot] = parse_number(&tokens[idx])?;
        slot += 1;
        idx += 1;
    }

    if values[0] <= 0.0 || values[1] <= 0.0 || values[2] < 0.0 {
        return Err("radius and height must be positive, step must be >= 0".to_string());
    }

    if values[3] <= 0.0 || values[3] > 90.0 {
        return Err("slope must be within (0, 90] degrees".to_string());
    }

    if values[4] <= 0.0 || values[5] <= 0.0 || values[6] <= 0.0 {
        return Err("jump height, jump distance, and max drop must be positive".to_string());
    }

    params.radius = values[0];
    params.height = values[1];
    params.step = values[2];
    params.slope = values[3].to_radians().cos();
    params.jump_height = values[4];
    params.jump_dist = values[5];
    params.max_drop = values[6];

    Ok(NavCommand::Build { input, params })
}

fn parse_number(token: &str) -> Result<f64, String> {
    let value: f64 = token
        .parse()
        .map_err(|_| format!("bad number '{token}'"))?;

    if !value.is_finite() {
        return Err(format!("bad number '{token}'"));
    }

    Ok(value)
}

fn save_nav(map_name: &str, bytes: &[u8]) -> Result<(), String> {
    let path = cwd_nav_path(map_name).ok_or_else(|| format!("bad map name '{map_name}'"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    std::fs::write(&path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}

fn load_nav(map_name: &str) -> Result<Option<(NavMesh, Vec<u8>)>, String> {
    let Some(path) = cwd_nav_path(map_name) else {
        return Ok(None);
    };

    if !path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mesh = NavMesh::from_bytes(&bytes)?;

    Ok(Some((mesh, bytes)))
}

fn decompress_nav(bytes: &[u8]) -> Result<Vec<u8>, String> {
    zstd::bulk::decompress(bytes, 256 * 1024 * 1024).map_err(|err| err.to_string())
}

fn nav_stem(name: &str) -> &str {
    let file = std::path::Path::new(name)
        .file_name()
        .and_then(|file| file.to_str())
        .unwrap_or(name);

    file.strip_suffix(".nav")
        .or_else(|| file.strip_suffix(".vmap"))
        .or_else(|| file.strip_suffix(".map"))
        .or_else(|| file.strip_suffix(".cmap"))
        .unwrap_or(file)
}

fn input_byte(input: NavInput) -> u8 {
    match input {
        NavInput::Brush => 1,
        NavInput::Voxel => 2,
        NavInput::Both => 3,
    }
}

fn input_from(value: u8) -> Result<NavInput, String> {
    match value {
        1 => Ok(NavInput::Brush),
        2 => Ok(NavInput::Voxel),
        3 => Ok(NavInput::Both),
        _ => Err(format!("bad nav source {value}")),
    }
}

fn tag_byte(tag: NavTag) -> u8 {
    match tag {
        NavTag::Brush => 1,
        NavTag::Voxel => 2,
        NavTag::Both => 3,
    }
}

fn tag_from(value: u8) -> Result<NavTag, String> {
    match value {
        1 => Ok(NavTag::Brush),
        2 => Ok(NavTag::Voxel),
        3 => Ok(NavTag::Both),
        _ => Err(format!("bad nav tag {value}")),
    }
}

fn kind_byte(kind: OffKind) -> u8 {
    match kind {
        OffKind::Jump => 1,
        OffKind::Drop => 2,
    }
}

fn kind_from(value: u8) -> Result<OffKind, String> {
    match value {
        1 => Ok(OffKind::Jump),
        2 => Ok(OffKind::Drop),
        _ => Err(format!("bad nav link {value}")),
    }
}

fn tag_color(tag: NavTag) -> [f32; 3] {
    match tag {
        NavTag::Brush => [0.25, 0.72, 0.95],
        NavTag::Voxel => [0.30, 0.86, 0.42],
        NavTag::Both => [0.95, 0.84, 0.28],
    }
}

fn push_polyline(vertices: &mut Vec<f32>, points: &[Vector3], color: [f32; 3], draw: Anchor) {
    if points.is_empty() {
        return;
    }

    let mut idx = 0;

    while idx + 1 < points.len() {
        push_world_ribbon(vertices, points[idx], points[idx + 1], 0.035, color, draw);
        idx += 1;
    }

    push_marker(vertices, points[0], color, draw);
    push_marker(vertices, points[points.len() - 1], color, draw);
}

fn push_arc(
    vertices: &mut Vec<f32>,
    start: Vector3,
    end: Vector3,
    jump_height: f64,
    color: [f32; 3],
    draw: Anchor,
) {
    let samples = 6;
    let mut prev = start;
    let mut step = 1;

    while step <= samples {
        let t = step as f64 / samples as f64;
        let point = Vector3::new(
            start.x + (end.x - start.x) * t,
            start.y + (end.y - start.y) * t,
            arc_z(start.z, end.z, t, jump_height),
        );
        push_world_ribbon(vertices, prev, point, 0.02, color, draw);
        prev = point;
        step += 1;
    }
}

fn push_world_ribbon(
    vertices: &mut Vec<f32>,
    a: Vector3,
    b: Vector3,
    half_width: f32,
    color: [f32; 3],
    draw: Anchor,
) {
    let ax = a.x;
    let ay = a.y;
    let az = a.z + LIFT;
    let bx = b.x;
    let by = b.y;
    let bz = b.z + LIFT;
    let dx = (bx - ax) as f32;
    let dy = (by - ay) as f32;
    let dz = (bz - az) as f32;
    let len = (dx * dx + dy * dy + dz * dz).sqrt();

    if len < 1.0e-5 {
        return;
    }

    let mut sx = -dy;
    let mut sy = dx;
    let mut sz = 0.0;
    let side = (sx * sx + sy * sy + sz * sz).sqrt();

    if side < 1.0e-5 {
        sx = 1.0;
        sy = 0.0;
        sz = 0.0;
    } else {
        sx /= side;
        sy /= side;
        sz /= side;
    }

    sx *= half_width;
    sy *= half_width;
    sz *= half_width;
    let p0 = draw.relative(ax - sx as f64, ay - sy as f64, az - sz as f64);
    let p1 = draw.relative(bx - sx as f64, by - sy as f64, bz - sz as f64);
    let p2 = draw.relative(bx + sx as f64, by + sy as f64, bz + sz as f64);
    let p3 = draw.relative(ax + sx as f64, ay + sy as f64, az + sz as f64);
    push_shaded_tri(vertices, p0, p1, p2, color);
    push_shaded_tri(vertices, p0, p2, p3, color);
}

fn push_marker(vertices: &mut Vec<f32>, point: Vector3, color: [f32; 3], draw: Anchor) {
    let size = 0.08;
    let min = Vector3::new(point.x - size, point.y - size, point.z - size);
    let max = Vector3::new(point.x + size, point.y + size, point.z + size);
    let corners = [
        Vector3::new(min.x, min.y, min.z),
        Vector3::new(max.x, min.y, min.z),
        Vector3::new(max.x, max.y, min.z),
        Vector3::new(min.x, max.y, min.z),
        Vector3::new(min.x, min.y, max.z),
        Vector3::new(max.x, min.y, max.z),
        Vector3::new(max.x, max.y, max.z),
        Vector3::new(min.x, max.y, max.z),
    ];
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    let mut idx = 0;

    while idx < edges.len() {
        let (a, b) = edges[idx];
        push_world_ribbon(vertices, corners[a], corners[b], 0.012, color, draw);
        idx += 1;
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let byte = *self.data.get(self.at).ok_or("truncated navmesh")?;
        self.at += 1;

        Ok(byte)
    }

    fn u16(&mut self) -> Result<u16, String> {
        let mut bytes = [0u8; 2];
        bytes[0] = self.u8()?;
        bytes[1] = self.u8()?;

        Ok(u16::from_le_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let mut bytes = [0u8; 4];
        let mut idx = 0;

        while idx < 4 {
            bytes[idx] = self.u8()?;
            idx += 1;
        }

        Ok(u32::from_le_bytes(bytes))
    }

    fn f64(&mut self) -> Result<f64, String> {
        let mut bytes = [0u8; 8];
        let mut idx = 0;

        while idx < 8 {
            bytes[idx] = self.u8()?;
            idx += 1;
        }

        Ok(f64::from_le_bytes(bytes))
    }

    fn vec3(&mut self) -> Result<Vector3, String> {
        Ok(Vector3::new(self.f64()?, self.f64()?, self.f64()?))
    }
}

fn push_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_f64(out: &mut Vec<u8>, value: f64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_vec3(out: &mut Vec<u8>, value: Vector3) {
    push_f64(out, value.x);
    push_f64(out, value.y);
    push_f64(out, value.z);
}

#[cfg(test)]
mod tests {
    use super::super::{Block, BlockPos, BrushMap, VoxelWorld};
    use super::{
        bake, NavBlock, NavBrush, NavInput, NavMesh, NavParams, NavSnapshot, NavTag, OffKind,
    };
    use crate::script::libs::vector3::Vector3;
    use crate::world::BrushPlane;

    fn block(x: i32, y: i32, z: i32, water: bool) -> NavBlock {
        NavBlock {
            min: Vector3::new(x as f64, y as f64, z as f64),
            max: Vector3::new(x as f64 + 1.0, y as f64 + 1.0, z as f64 + 1.0),
            water,
        }
    }

    fn fill(blocks: &mut Vec<NavBlock>, x0: i32, x1: i32, y0: i32, y1: i32, z: i32, water: bool) {
        let mut y = y0;

        while y < y1 {
            let mut x = x0;

            while x < x1 {
                blocks.push(block(x, y, z, water));
                x += 1;
            }

            y += 1;
        }
    }

    fn box_brush(min: Vector3, max: Vector3) -> NavBrush {
        NavBrush {
            min,
            max,
            planes: vec![
                BrushPlane {
                    normal: Vector3::new(1.0, 0.0, 0.0),
                    distance: max.x,
                },
                BrushPlane {
                    normal: Vector3::new(-1.0, 0.0, 0.0),
                    distance: -min.x,
                },
                BrushPlane {
                    normal: Vector3::new(0.0, 1.0, 0.0),
                    distance: max.y,
                },
                BrushPlane {
                    normal: Vector3::new(0.0, -1.0, 0.0),
                    distance: -min.y,
                },
                BrushPlane {
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    distance: max.z,
                },
                BrushPlane {
                    normal: Vector3::new(0.0, 0.0, -1.0),
                    distance: -min.z,
                },
            ],
        }
    }

    #[test]
    fn water_is_not_a_floor_and_blocks_clearance() {
        let mut world = VoxelWorld::with_scale(1.0);
        world.set(BlockPos::new(1, 1, 0), Block::STONE);
        world.set(BlockPos::new(1, 1, 1), Block::WATER);
        let copied = world.nav_blocks();
        assert!(copied.iter().any(|block| block.water));
        assert!(copied.iter().any(|block| !block.water));

        let mut blocks = Vec::new();
        fill(&mut blocks, 0, 4, 0, 4, 0, false);
        fill(&mut blocks, 8, 12, 0, 4, 0, false);
        fill(&mut blocks, 8, 12, 0, 4, 1, true);
        fill(&mut blocks, 14, 16, 0, 2, 2, true);
        let mesh = bake(
            &NavSnapshot {
                brushes: Vec::new(),
                blocks,
                input: NavInput::Voxel,
            },
            NavParams::player(),
        );
        assert!(!mesh.polys.is_empty());
        let mut idx = 0;

        while idx < mesh.polys.len() {
            let mut vert = 0;

            while vert < mesh.polys[idx].verts.len() {
                assert!(mesh.polys[idx].verts[vert].x < 6.0);
                assert!(mesh.polys[idx].verts[vert].z < 1.5);
                vert += 1;
            }

            idx += 1;
        }
    }

    #[test]
    fn steep_brush_is_rejected() {
        let length = (4.0_f64 + 1.0).sqrt();
        let mesh = bake(
            &NavSnapshot {
                brushes: vec![NavBrush {
                    min: Vector3::new(0.0, 0.0, 0.0),
                    max: Vector3::new(1.0, 1.0, 2.0),
                    planes: vec![
                        BrushPlane {
                            normal: Vector3::new(-1.0, 0.0, 0.0),
                            distance: 0.0,
                        },
                        BrushPlane {
                            normal: Vector3::new(1.0, 0.0, 0.0),
                            distance: 1.0,
                        },
                        BrushPlane {
                            normal: Vector3::new(0.0, -1.0, 0.0),
                            distance: 0.0,
                        },
                        BrushPlane {
                            normal: Vector3::new(0.0, 1.0, 0.0),
                            distance: 1.0,
                        },
                        BrushPlane {
                            normal: Vector3::new(0.0, 0.0, -1.0),
                            distance: 0.0,
                        },
                        BrushPlane {
                            normal: Vector3::new(-2.0 / length, 0.0, 1.0 / length),
                            distance: 0.0,
                        },
                    ],
                }],
                blocks: Vec::new(),
                input: NavInput::Brush,
            },
            NavParams::player(),
        );

        assert!(mesh.polys.is_empty());
    }

    #[test]
    fn step_jump_drop_and_mixed_sources() {
        let mut blocks = Vec::new();
        fill(&mut blocks, 0, 4, 0, 4, 0, false);
        fill(&mut blocks, 5, 9, 0, 4, 1, false);
        fill(&mut blocks, 10, 14, 0, 4, -1, false);
        fill(&mut blocks, 0, 4, 8, 12, 0, false);
        fill(&mut blocks, 20, 24, 0, 4, 0, false);
        let mesh = bake(
            &NavSnapshot {
                brushes: vec![
                    box_brush(
                        Vector3::new(2.0, 8.0, 0.0),
                        Vector3::new(6.0, 12.0, 1.4),
                    ),
                    box_brush(
                        Vector3::new(20.0, 0.0, 0.0),
                        Vector3::new(24.0, 4.0, 1.0),
                    ),
                ],
                blocks,
                input: NavInput::Both,
            },
            NavParams::player(),
        );
        assert!(mesh.polys.iter().any(|poly| poly.tag == NavTag::Brush));
        assert!(mesh.polys.iter().any(|poly| poly.tag == NavTag::Voxel));
        assert!(mesh.polys.iter().any(|poly| poly.tag == NavTag::Both));
        assert!(mesh.offmesh.iter().any(|link| {
            link.kind == OffKind::Jump && link.end.z > link.start.z + 0.5
        }));
        assert!(mesh.offmesh.iter().any(|link| {
            link.kind == OffKind::Drop && link.start.z > link.end.z + 1.0
        }));
        let mut linked = false;
        let mut idx = 0;

        while idx < mesh.polys.len() {
            if mesh.polys[idx].tag == NavTag::Brush {
                let mut neighbor = 0;

                while neighbor < mesh.polys[idx].neighbors.len() {
                    let other = mesh.polys[idx].neighbors[neighbor] as usize;

                    if mesh.polys[other].tag == NavTag::Voxel {
                        linked = true;
                    }

                    neighbor += 1;
                }
            }

            idx += 1;
        }

        assert!(linked);
    }

    #[test]
    fn path_goes_around_a_corner() {
        let mut blocks = Vec::new();
        fill(&mut blocks, 0, 8, 0, 2, 0, false);
        fill(&mut blocks, 6, 8, 0, 8, 0, false);
        fill(&mut blocks, 0, 8, 6, 8, 0, false);
        let mut params = NavParams::player();
        params.radius = 0.2;
        params.height = 1.5;
        params.jump_dist = 0.6;
        params.max_drop = 0.6;
        let mesh = bake(
            &NavSnapshot {
                brushes: Vec::new(),
                blocks,
                input: NavInput::Voxel,
            },
            params,
        );
        let path = mesh.find_path(
            Vector3::new(1.0, 1.0, 1.0),
            Vector3::new(1.0, 7.0, 1.0),
        );
        assert!(path.len() >= 2);
        assert!(path.iter().any(|point| point.x > 5.0));
    }

    #[test]
    fn navm_roundtrip() {
        let mut map = BrushMap::new();
        assert!(map.add_box(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(4.0, 4.0, 1.0),
            1
        ));
        let mesh = bake(
            &NavSnapshot {
                brushes: map.nav_brushes(),
                blocks: Vec::new(),
                input: NavInput::Brush,
            },
            NavParams::player(),
        );
        assert!(!mesh.polys.is_empty());
        let bytes = mesh.to_bytes();
        let loaded = NavMesh::from_bytes(&bytes).expect("nav roundtrip");
        assert_eq!(loaded.polys.len(), mesh.polys.len());
        assert_eq!(loaded.offmesh.len(), mesh.offmesh.len());
        assert_eq!(loaded.polys[0].verts.len(), mesh.polys[0].verts.len());
        assert!((loaded.polys[0].verts[0].z - mesh.polys[0].verts[0].z).abs() < 1.0e-6);
        assert_eq!(loaded.input, NavInput::Brush);
    }
}
