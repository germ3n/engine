use super::material::FileSource;
use super::material::MaterialBank;
use super::surface::{
    push_vertex, tri_normal, tri_tangent, MapGraphics, SurfaceRange, MATERIAL_NONE, PASS_OPAQUE,
    STRIDE,
};
use super::DrawMesh;
use crate::script::libs::vector3::Vector3;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use wincode::{SchemaRead, SchemaWrite};

const MAX_PLANES: usize = 128;
const GRID_CELL: f64 = 256.0;
const PLANE_EPS: f64 = 1e-4;
const LENGTH_EPS: f64 = 1e-8;
const RAY_EPS: f64 = 1e-8;
const AREA_EPS: f64 = 1e-10;
const COMPILED_MAGIC: &[u8; 4] = b"CMAP";
const COMPILED_VERSION: u32 = 2;
const COMPILED_VERSION_V1: u32 = 1;

#[derive(SchemaWrite, SchemaRead, Clone, Copy, Debug, PartialEq)]
pub struct BrushPlane {
    pub normal: Vector3,
    pub distance: f64,
}

#[derive(SchemaWrite, SchemaRead, Clone, Copy, Debug, PartialEq)]
pub struct BrushBox {
    pub min: Vector3,
    pub max: Vector3,
    pub material: u16,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub enum BrushEdit {
    Box(BrushBox),
    Convex {
        planes: Vec<BrushPlane>,
        material: u16,
    },
    Remove(u32),
    Move {
        index: u32,
        delta: Vector3,
    },
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushHit {
    pub brush: usize,
    pub distance: f64,
    pub position: Vector3,
    pub normal: Option<Vector3>,
}

#[derive(Clone, Copy)]
struct Plane {
    normal: Vector3,
    distance: f64,
    material: u16,
    tex: u16,
    axis_u: Vector3,
    axis_v: Vector3,
    shift_u: f64,
    shift_v: f64,
    scale_u: f64,
    scale_v: f64,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub struct CompiledFace {
    pub texture: String,
    pub normal: Vector3,
    pub distance: f64,
    pub material: u16,
    pub axis_u: Vector3,
    pub axis_v: Vector3,
    pub shift_u: f64,
    pub shift_v: f64,
    pub scale_u: f64,
    pub scale_v: f64,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
struct CompiledFaceV1 {
    texture: String,
    normal: Vector3,
    distance: f64,
    material: u16,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
struct CompiledBrushV1 {
    faces: Vec<CompiledFaceV1>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
struct CompiledEntityV1 {
    keys: Vec<CompiledPair>,
    brushes: Vec<CompiledBrushV1>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
struct CompiledMapV1 {
    entities: Vec<CompiledEntityV1>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub struct CompiledBrush {
    pub faces: Vec<CompiledFace>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub struct CompiledPair {
    pub key: String,
    pub value: String,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub struct CompiledEntity {
    pub keys: Vec<CompiledPair>,
    pub brushes: Vec<CompiledBrush>,
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub struct CompiledMap {
    pub entities: Vec<CompiledEntity>,
}

struct SourceFace {
    texture: String,
    plane: Plane,
    axis_u: Vector3,
    axis_v: Vector3,
    shift_u: f64,
    shift_v: f64,
    scale_u: f64,
    scale_v: f64,
}

struct SourceBrush {
    faces: Vec<SourceFace>,
}

struct SourceEntity {
    keys: Vec<(String, String)>,
    brushes: Vec<SourceBrush>,
}

struct Poly {
    normal: Vector3,
    points: Vec<Vector3>,
    material: u16,
    tex: u16,
    axis_u: Vector3,
    axis_v: Vector3,
    shift_u: f64,
    shift_v: f64,
    scale_u: f64,
    scale_v: f64,
    #[allow(dead_code)]
    width: f64,
    #[allow(dead_code)]
    height: f64,
}

pub struct Brush {
    planes: Vec<Plane>,
}

impl Brush {
    pub fn aabb(min: Vector3, max: Vector3, material: u16) -> Option<Self> {
        if !finite(min) || !finite(max) {
            return None;
        }

        if min.x >= max.x || min.y >= max.y || min.z >= max.z {
            return None;
        }

        Self::convex(
            vec![
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
            material,
        )
    }

    pub fn convex(planes: Vec<BrushPlane>, material: u16) -> Option<Self> {
        if planes.len() < 4 || planes.len() > MAX_PLANES {
            return None;
        }

        let mut stored = Vec::with_capacity(planes.len());

        for plane in planes {
            let Some(plane) = Plane::new(plane.normal, plane.distance, material) else {
                return None;
            };

            stored.push(plane);
        }

        Self::from_planes(stored)
    }

    fn from_planes(planes: Vec<Plane>) -> Option<Self> {
        if planes.len() < 4 || planes.len() > MAX_PLANES {
            return None;
        }

        if !closed(&planes) {
            return None;
        }

        Some(Self { planes })
    }

    fn from_planes_trusted(planes: Vec<Plane>) -> Option<Self> {
        if planes.len() < 4 || planes.len() > MAX_PLANES {
            return None;
        }

        Some(Self { planes })
    }
}

#[derive(Clone, Copy)]
struct Aabb {
    min: Vector3,
    max: Vector3,
}

struct BrushGrid {
    cells: HashMap<(i32, i32, i32), Vec<usize>>,
}

impl BrushGrid {
    fn new() -> Self {
        Self {
            cells: HashMap::new(),
        }
    }

    fn clear(&mut self) {
        self.cells.clear();
    }

    fn rebuild(&mut self, bounds: &[Aabb], cell: f64) {
        self.cells.clear();

        for (idx, aabb) in bounds.iter().enumerate() {
            let x0 = cell_coord(aabb.min.x, cell);
            let y0 = cell_coord(aabb.min.y, cell);
            let z0 = cell_coord(aabb.min.z, cell);
            let x1 = cell_coord(aabb.max.x, cell);
            let y1 = cell_coord(aabb.max.y, cell);
            let z1 = cell_coord(aabb.max.z, cell);
            let mut z = z0;

            while z <= z1 {
                let mut y = y0;

                while y <= y1 {
                    let mut x = x0;

                    while x <= x1 {
                        self.cells.entry((x, y, z)).or_default().push(idx);
                        x += 1;
                    }

                    y += 1;
                }

                z += 1;
            }
        }
    }

    fn query(&self, min: Vector3, max: Vector3, brush_count: usize, cell: f64) -> Vec<usize> {
        if brush_count == 0 {
            return Vec::new();
        }

        let x0 = cell_coord(min.x, cell);
        let y0 = cell_coord(min.y, cell);
        let z0 = cell_coord(min.z, cell);
        let x1 = cell_coord(max.x, cell);
        let y1 = cell_coord(max.y, cell);
        let z1 = cell_coord(max.z, cell);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let span_x = (x1 as i64 - x0 as i64).saturating_add(1).max(0);
        let span_y = (y1 as i64 - y0 as i64).saturating_add(1).max(0);
        let span_z = (z1 as i64 - z0 as i64).saturating_add(1).max(0);
        let volume = span_x.saturating_mul(span_y).saturating_mul(span_z);

        if volume > self.cells.len() as i64 {
            for (&(x, y, z), list) in &self.cells {
                if x < x0 || x > x1 || y < y0 || y > y1 || z < z0 || z > z1 {
                    continue;
                }

                for &idx in list {
                    if seen.insert(idx) {
                        out.push(idx);
                    }
                }
            }

            return out;
        }

        let mut z = z0;

        while z <= z1 {
            let mut y = y0;

            while y <= y1 {
                let mut x = x0;

                while x <= x1 {
                    if let Some(list) = self.cells.get(&(x, y, z)) {
                        for &idx in list {
                            if seen.insert(idx) {
                                out.push(idx);
                            }
                        }
                    }

                    x += 1;
                }

                y += 1;
            }

            z += 1;
        }

        out
    }
}

fn cell_coord(value: f64, cell: f64) -> i32 {
    (value / cell).floor() as i32
}

pub struct BrushMap {
    brushes: Vec<Brush>,
    bounds: Vec<Aabb>,
    spawns: Vec<Vector3>,
    revision: u64,
    grid: BrushGrid,
    mesh_cache: Option<Vec<f32>>,
    ranges: Vec<SurfaceRange>,
    graphics: MapGraphics,
    scale: f64,
    scale_dirty: bool,
    pending_edits: Vec<BrushEdit>,
    edits: Vec<BrushEdit>,
}

impl BrushMap {
    pub fn new() -> Self {
        Self {
            brushes: Vec::new(),
            bounds: Vec::new(),
            spawns: Vec::new(),
            revision: 0,
            grid: BrushGrid::new(),
            mesh_cache: None,
            ranges: Vec::new(),
            graphics: MapGraphics::plain(),
            scale: 1.0,
            scale_dirty: false,
            pending_edits: Vec::new(),
            edits: Vec::new(),
        }
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    pub fn set_scale(&mut self, scale: f64) -> bool {
        let Some(scale) = positive_scale(scale) else {
            return false;
        };

        if (scale - self.scale).abs() <= 1e-12 {
            return true;
        }

        let ratio = scale / self.scale;
        self.scale = scale;
        self.scale_dirty = true;
        self.apply_ratio(ratio);

        true
    }

    pub fn take_scale(&mut self) -> Option<f64> {
        if !self.scale_dirty {
            return None;
        }

        self.scale_dirty = false;

        Some(self.scale)
    }

    fn apply_ratio(&mut self, ratio: f64) {
        let mut idx = 0;

        while idx < self.brushes.len() {
            let mut plane = 0;

            while plane < self.brushes[idx].planes.len() {
                self.brushes[idx].planes[plane].distance *= ratio;
                self.brushes[idx].planes[plane].scale_u *= ratio;
                self.brushes[idx].planes[plane].scale_v *= ratio;
                plane += 1;
            }

            self.bounds[idx].min.x *= ratio;
            self.bounds[idx].min.y *= ratio;
            self.bounds[idx].min.z *= ratio;
            self.bounds[idx].max.x *= ratio;
            self.bounds[idx].max.y *= ratio;
            self.bounds[idx].max.z *= ratio;
            idx += 1;
        }

        idx = 0;

        while idx < self.spawns.len() {
            self.spawns[idx].x *= ratio;
            self.spawns[idx].y *= ratio;
            self.spawns[idx].z *= ratio;
            idx += 1;
        }

        if let Some(cache) = &mut self.mesh_cache {
            let ratio = ratio as f32;
            let mut vert = 0;

            while vert + 2 < cache.len() {
                cache[vert] *= ratio;
                cache[vert + 1] *= ratio;
                cache[vert + 2] *= ratio;
                vert += STRIDE;
            }
        }

        scale_edits(&mut self.pending_edits, ratio);
        scale_edits(&mut self.edits, ratio);
        self.touch();
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.brushes.len()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn bounds(&self, index: usize) -> Option<(Vector3, Vector3)> {
        let aabb = self.bounds.get(index)?;

        Some((aabb.min, aabb.max))
    }

    #[allow(dead_code)]
    pub fn hulls(&self) -> Vec<Vec<Vector3>> {
        let mut out = Vec::with_capacity(self.brushes.len());
        let mut idx = 0;

        while idx < self.brushes.len() {
            out.push(vertices(&self.brushes[idx].planes));
            idx += 1;
        }

        out
    }

    pub fn hulls_in(&self, min: Vector3, max: Vector3) -> Vec<Vec<Vector3>> {
        let indices = self.grid.query(min, max, self.brushes.len(), self.cell());
        let mut out = Vec::new();
        let mut idx = 0;

        while idx < indices.len() {
            let brush_idx = indices[idx];
            idx += 1;
            let bounds = &self.bounds[brush_idx];

            if bounds.max.x < min.x
                || bounds.min.x > max.x
                || bounds.max.y < min.y
                || bounds.min.y > max.y
                || bounds.max.z < min.z
                || bounds.min.z > max.z
            {
                continue;
            }

            out.push(vertices(&self.brushes[brush_idx].planes));
        }

        out
    }

    pub fn nav_brushes(&self) -> Vec<super::nav::NavBrush> {
        let mut out = Vec::with_capacity(self.brushes.len());
        let mut idx = 0;

        while idx < self.brushes.len() {
            let brush = &self.brushes[idx];
            let bounds = &self.bounds[idx];
            let mut planes = Vec::with_capacity(brush.planes.len());
            let mut plane = 0;

            while plane < brush.planes.len() {
                let src = &brush.planes[plane];
                planes.push(BrushPlane {
                    normal: src.normal,
                    distance: src.distance,
                });
                plane += 1;
            }

            out.push(super::nav::NavBrush {
                planes,
                min: bounds.min,
                max: bounds.max,
            });
            idx += 1;
        }

        out
    }

    #[allow(dead_code)]
    pub fn add_box(&mut self, min: Vector3, max: Vector3, material: u16) -> bool {
        self.add_box_index(min, max, material).is_some()
    }

    pub fn add_box_index(&mut self, min: Vector3, max: Vector3, material: u16) -> Option<usize> {
        if !self.place_box(min, max, material) {
            return None;
        }

        let index = self.brushes.len() - 1;
        self.note(BrushEdit::Box(BrushBox { min, max, material }));

        Some(index)
    }

    pub fn place_box(&mut self, min: Vector3, max: Vector3, material: u16) -> bool {
        let Some(brush) = Brush::aabb(min, max, material) else {
            return false;
        };

        self.push(brush)
    }

    #[allow(dead_code)]
    pub fn add_convex(&mut self, planes: Vec<BrushPlane>, material: u16) -> bool {
        self.add_convex_index(planes, material).is_some()
    }

    pub fn add_convex_index(&mut self, planes: Vec<BrushPlane>, material: u16) -> Option<usize> {
        if !self.apply_convex(&planes, material) {
            return None;
        }

        let index = self.brushes.len() - 1;
        self.note(BrushEdit::Convex { planes, material });

        Some(index)
    }

    pub fn apply_convex(&mut self, planes: &[BrushPlane], material: u16) -> bool {
        let Some(brush) = Brush::convex(planes.to_vec(), material) else {
            return false;
        };

        self.push(brush)
    }

    pub fn remove_brush(&mut self, index: usize) -> bool {
        let Ok(stored) = u32::try_from(index) else {
            return false;
        };

        if !self.apply_remove(index) {
            return false;
        }

        self.note(BrushEdit::Remove(stored));

        true
    }

    pub fn apply_remove(&mut self, index: usize) -> bool {
        if index >= self.brushes.len() {
            return false;
        }

        self.brushes.remove(index);
        self.bounds.remove(index);
        self.mesh_cache = None;
        self.touch();

        true
    }

    pub fn move_brush(&mut self, index: usize, delta: Vector3) -> bool {
        if !finite(delta) {
            return false;
        }

        let Ok(stored) = u32::try_from(index) else {
            return false;
        };

        if delta.x == 0.0 && delta.y == 0.0 && delta.z == 0.0 {
            return index < self.brushes.len();
        }

        if !self.apply_move(index, delta) {
            return false;
        }

        self.note(BrushEdit::Move {
            index: stored,
            delta,
        });

        true
    }

    pub fn apply_move(&mut self, index: usize, delta: Vector3) -> bool {
        if !finite(delta) || index >= self.brushes.len() {
            return false;
        }

        let mut plane = 0;

        while plane < self.brushes[index].planes.len() {
            let normal = self.brushes[index].planes[plane].normal;
            self.brushes[index].planes[plane].distance += normal.dot(delta);
            plane += 1;
        }

        let Some(aabb) = brush_aabb(&self.brushes[index]) else {
            return false;
        };

        self.bounds[index] = aabb;
        self.mesh_cache = None;
        self.touch();

        true
    }

    pub fn edits(&self) -> &[BrushEdit] {
        &self.edits
    }

    pub fn take_edits(&mut self) -> Vec<BrushEdit> {
        std::mem::take(&mut self.pending_edits)
    }

    pub fn apply_edit(&mut self, edit: &BrushEdit) -> bool {
        match edit {
            BrushEdit::Box(brush) => self.place_box(brush.min, brush.max, brush.material),
            BrushEdit::Convex { planes, material } => self.apply_convex(planes, *material),
            BrushEdit::Remove(index) => self.apply_remove(*index as usize),
            BrushEdit::Move { index, delta } => self.apply_move(*index as usize, *delta),
            BrushEdit::Clear => {
                self.apply_clear();

                true
            }
        }
    }

    fn note(&mut self, edit: BrushEdit) {
        self.pending_edits.push(edit.clone());
        self.edits.push(edit);
    }

    pub fn load_file(&mut self, name: &str) -> Result<(), String> {
        if let Some(path) = find_map(name) {
            if is_bsp(&path) {
                let bytes =
                    std::fs::read(&path).map_err(|err| format!("map {}: {err}", path.display()))?;

                return self.install_bsp(&bytes, &path);
            }

            let compiled_path = if is_compiled(&path) {
                path
            } else {
                ensure_compiled(&path)?
            };
            let bytes = std::fs::read(&compiled_path)
                .map_err(|err| format!("map {}: {err}", compiled_path.display()))?;

            return self.install_compiled(&bytes);
        }

        if let Some(bytes) = bundled_map(name) {
            return self.install_compiled(bytes);
        }

        Err(format!("map {name} was not found"))
    }

    fn install_compiled(&mut self, bytes: &[u8]) -> Result<(), String> {
        let compiled = decode_compiled(bytes)?;
        let mut loaded = brush_map_from_compiled(compiled)?;
        loaded.revision = self.revision.wrapping_add(loaded.revision).wrapping_add(1);
        *self = loaded;

        Ok(())
    }

    fn install_bsp(&mut self, bytes: &[u8], path: &Path) -> Result<(), String> {
        let mut loaded = brush_map_from_bsp(bytes, path)?;
        loaded.revision = self.revision.wrapping_add(loaded.revision).wrapping_add(1);
        *self = loaded;

        Ok(())
    }

    pub fn load_document(&mut self, map: &CompiledMap) -> Result<(), String> {
        let mut loaded = brush_map_from_compiled(map.clone())?;
        loaded.revision = self.revision.wrapping_add(1);
        *self = loaded;

        Ok(())
    }

    pub fn push(&mut self, brush: Brush) -> bool {
        let Some(aabb) = brush_aabb(&brush) else {
            return false;
        };

        self.brushes.push(brush);
        self.bounds.push(aabb);
        self.mesh_cache = None;
        self.touch();

        true
    }

    fn push_quiet(&mut self, brush: Brush) -> bool {
        let Some(aabb) = brush_aabb(&brush) else {
            return false;
        };

        self.brushes.push(brush);
        self.bounds.push(aabb);

        true
    }

    pub fn clear(&mut self) {
        if self.brushes.is_empty() && self.spawns.is_empty() && self.mesh_cache.is_none() {
            return;
        }

        self.apply_clear();
        self.pending_edits.clear();
        self.edits.clear();
        self.note(BrushEdit::Clear);
    }

    pub fn apply_clear(&mut self) {
        self.brushes.clear();
        self.bounds.clear();
        self.spawns.clear();
        self.mesh_cache = None;
        self.ranges.clear();
        self.graphics = MapGraphics::plain();
        self.grid.clear();
        self.touch();
    }

    pub fn spawns(&self) -> &[Vector3] {
        &self.spawns
    }

    pub fn mesh(&self) -> Vec<f32> {
        self.mesh_at(Vector3::new(0.0, 0.0, 0.0))
    }

    pub fn graphics(&self) -> &MapGraphics {
        &self.graphics
    }

    pub fn draw_at(&self, origin: Vector3) -> DrawMesh {
        if let Some(cache) = &self.mesh_cache {
            return DrawMesh {
                vertices: shift_cached(cache, origin),
                ranges: self.ranges.clone(),
            };
        }

        self.build_draw(None, origin)
    }

    pub fn mesh_at(&self, origin: Vector3) -> Vec<f32> {
        self.draw_at(origin).vertices
    }

    #[allow(dead_code)]
    pub fn mesh_highlight(&self, selected: usize) -> Vec<f32> {
        self.mesh_highlight_at(selected, Vector3::new(0.0, 0.0, 0.0))
    }

    pub fn mesh_highlight_at(&self, selected: usize, origin: Vector3) -> Vec<f32> {
        self.build_mesh(Some(selected), origin)
    }

    fn build_mesh(&self, selected: Option<usize>, origin: Vector3) -> Vec<f32> {
        self.build_draw(selected, origin).vertices
    }

    fn build_draw(&self, selected: Option<usize>, origin: Vector3) -> DrawMesh {
        let mut vertices = Vec::new();
        let mut ranges: Vec<SurfaceRange> = Vec::new();
        let mut current: Option<(u16, u8, u32)> = None;

        for (idx, brush) in self.brushes.iter().enumerate() {
            for poly in polygons(brush) {
                if buried(&poly.points, idx, &self.brushes) {
                    continue;
                }

                let pass = PASS_OPAQUE;
                let material = if selected == Some(idx) {
                    MATERIAL_NONE
                } else {
                    poly.tex
                };
                let start = (vertices.len() / STRIDE) as u32;

                let (width, height) = self
                    .graphics
                    .materials
                    .get(poly.tex as usize)
                    .map(|material| (material.width.max(1) as f64, material.height.max(1) as f64))
                    .unwrap_or((1.0, 1.0));

                if selected == Some(idx) {
                    push_poly_color(
                        &mut vertices,
                        &poly,
                        [1.0, 0.86, 0.28],
                        origin,
                        width,
                        height,
                    );
                } else {
                    push_poly(&mut vertices, &poly, origin, width, height);
                }

                let count = (vertices.len() / STRIDE) as u32 - start;

                if count == 0 {
                    continue;
                }

                if let Some((have, have_pass, first)) = current {
                    if have == material && have_pass == pass {
                        ranges.last_mut().unwrap().count += count;

                        continue;
                    }

                    let _ = first;
                }

                ranges.push(SurfaceRange {
                    first: start,
                    count,
                    material,
                    cubemap: super::surface::CUBEMAP_NONE,
                    pass,
                });
                current = Some((material, pass, start));
            }
        }

        DrawMesh { vertices, ranges }
    }

    pub fn trace(&self, start: Vector3, end: Vector3) -> Option<BrushHit> {
        if !finite(start) || !finite(end) {
            return None;
        }

        let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let max_dist = delta.len();
        let (min, max) = segment_aabb(start, end);
        let candidates = self.grid.query(min, max, self.brushes.len(), self.cell());

        if max_dist == 0.0 {
            for idx in candidates {
                if contains(&self.brushes[idx].planes, start) {
                    return Some(BrushHit {
                        brush: idx,
                        distance: 0.0,
                        position: start,
                        normal: None,
                    });
                }
            }

            return None;
        }

        let inv = 1.0 / max_dist;
        let dir = Vector3::new(delta.x * inv, delta.y * inv, delta.z * inv);
        let mut best: Option<BrushHit> = None;

        for idx in candidates {
            let Some((distance, normal)) = hit_brush(&self.brushes[idx], start, dir, max_dist)
            else {
                continue;
            };

            if best
                .as_ref()
                .map(|hit| distance < hit.distance)
                .unwrap_or(true)
            {
                best = Some(BrushHit {
                    brush: idx,
                    distance,
                    position: Vector3::new(
                        start.x + dir.x * distance,
                        start.y + dir.y * distance,
                        start.z + dir.z * distance,
                    ),
                    normal,
                });
            }
        }

        best
    }

    pub fn sweep(
        &self,
        start: Vector3,
        end: Vector3,
        mins: Vector3,
        maxs: Vector3,
    ) -> Option<BrushHit> {
        if !finite(start) || !finite(end) || !finite(mins) || !finite(maxs) {
            return None;
        }

        let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let max_dist = delta.len();
        let (seg_min, seg_max) = segment_aabb(start, end);
        let query_min = Vector3::new(seg_min.x + mins.x, seg_min.y + mins.y, seg_min.z + mins.z);
        let query_max = Vector3::new(seg_max.x + maxs.x, seg_max.y + maxs.y, seg_max.z + maxs.z);
        let candidates = self
            .grid
            .query(query_min, query_max, self.brushes.len(), self.cell());
        let mut best: Option<BrushHit> = None;

        if max_dist == 0.0 {
            for idx in candidates {
                let expanded = expand_brush(&self.brushes[idx], mins, maxs);

                if contains(&expanded, start) {
                    return Some(BrushHit {
                        brush: idx,
                        distance: 0.0,
                        position: start,
                        normal: None,
                    });
                }
            }

            return None;
        }

        let inv = 1.0 / max_dist;
        let dir = Vector3::new(delta.x * inv, delta.y * inv, delta.z * inv);

        for idx in candidates {
            let expanded = expand_brush(&self.brushes[idx], mins, maxs);
            let Some((distance, normal)) = hit_planes(&expanded, start, dir, max_dist) else {
                continue;
            };

            if best
                .as_ref()
                .map(|hit| distance < hit.distance)
                .unwrap_or(true)
            {
                best = Some(BrushHit {
                    brush: idx,
                    distance,
                    position: Vector3::new(
                        start.x + dir.x * distance,
                        start.y + dir.y * distance,
                        start.z + dir.z * distance,
                    ),
                    normal,
                });
            }
        }

        best
    }

    fn cell(&self) -> f64 {
        (GRID_CELL * self.scale).max(1.0e-4)
    }

    pub fn cached_mesh(&self) -> Option<&[f32]> {
        self.mesh_cache.as_deref()
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.grid.rebuild(&self.bounds, self.cell());
    }

    fn finalize(&mut self) {
        self.grid.rebuild(&self.bounds, self.cell());
    }
}

impl Default for BrushMap {
    fn default() -> Self {
        Self::new()
    }
}

pub fn compile_map(name: &str) -> Result<PathBuf, String> {
    let path = find_map(name).ok_or_else(|| format!("map {name} was not found"))?;

    if is_compiled(&path) {
        return Err(format!("map {} is already compiled", path.display()));
    }

    let dest = compiled_path(&path);
    write_compiled(&path, &dest)?;

    Ok(dest)
}

fn find_map(name: &str) -> Option<PathBuf> {
    resolve_map(name, &super::content_dirs())
}

fn bundled_map(name: &str) -> Option<&'static [u8]> {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);

    if stem.eq_ignore_ascii_case("hall") {
        return Some(include_bytes!("../../maps/hall.cmap"));
    }

    None
}

fn resolve_map(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let given = PathBuf::from(name);

    if given.exists() {
        return Some(given);
    }

    let want_compiled = name.ends_with(".cmap");
    let want_source = name.ends_with(".map");
    let want_bsp = name.ends_with(".bsp");
    let stem = name
        .strip_suffix(".map")
        .or_else(|| name.strip_suffix(".cmap"))
        .or_else(|| name.strip_suffix(".bsp"))
        .unwrap_or(name);

    for dir in dirs {
        if !want_compiled && !want_bsp {
            let source = dir.join(format!("{stem}.map"));

            if source.exists() {
                return Some(source);
            }
        }

        if !want_source && !want_bsp {
            let compiled = dir.join(format!("{stem}.cmap"));

            if compiled.exists() {
                return Some(compiled);
            }
        }

        if !want_source && !want_compiled {
            let bsp = dir.join(format!("{stem}.bsp"));

            if bsp.exists() {
                return Some(bsp);
            }
        }
    }

    None
}

fn is_compiled(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("cmap")
}

fn is_bsp(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("bsp")
}

fn compiled_path(source: &Path) -> PathBuf {
    source.with_extension("cmap")
}

fn ensure_compiled(source: &Path) -> Result<PathBuf, String> {
    let dest = compiled_path(source);

    if compiled_is_fresh(source, &dest)? {
        return Ok(dest);
    }

    write_compiled(source, &dest)?;

    Ok(dest)
}

fn compiled_is_fresh(source: &Path, dest: &Path) -> Result<bool, String> {
    let source_meta =
        std::fs::metadata(source).map_err(|err| format!("map {}: {err}", source.display()))?;
    let Ok(dest_meta) = std::fs::metadata(dest) else {
        return Ok(false);
    };
    let source_time = source_meta
        .modified()
        .map_err(|err| format!("map {}: {err}", source.display()))?;
    let dest_time = dest_meta
        .modified()
        .map_err(|err| format!("map {}: {err}", dest.display()))?;

    Ok(dest_time >= source_time)
}

fn write_compiled(source: &Path, dest: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(source)
        .map_err(|err| format!("map {}: {err}", source.display()))?;
    let entities = parse_source(&text)?;
    let bytes = encode_compiled(&compile_source(&entities))?;
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, &bytes).map_err(|err| format!("map {}: {err}", tmp.display()))?;
    let _ = std::fs::remove_file(dest);
    std::fs::rename(&tmp, dest).map_err(|err| format!("map {}: {err}", dest.display()))?;
    log::info!("[map] compiled {}", dest.display());

    Ok(())
}

fn compile_source(entities: &[SourceEntity]) -> CompiledMap {
    let mut compiled = Vec::with_capacity(entities.len());

    for entity in entities {
        let mut keys = Vec::with_capacity(entity.keys.len());

        for (key, value) in &entity.keys {
            keys.push(CompiledPair {
                key: key.clone(),
                value: value.clone(),
            });
        }

        let mut brushes = Vec::with_capacity(entity.brushes.len());

        for brush in &entity.brushes {
            let mut faces = Vec::with_capacity(brush.faces.len());

            for face in &brush.faces {
                faces.push(CompiledFace {
                    texture: face.texture.clone(),
                    normal: face.plane.normal,
                    distance: face.plane.distance,
                    material: face.plane.material,
                    axis_u: face.axis_u,
                    axis_v: face.axis_v,
                    shift_u: face.shift_u,
                    shift_v: face.shift_v,
                    scale_u: face.scale_u,
                    scale_v: face.scale_v,
                });
            }

            brushes.push(CompiledBrush { faces });
        }

        compiled.push(CompiledEntity { keys, brushes });
    }

    CompiledMap { entities: compiled }
}

fn encode_compiled(map: &CompiledMap) -> Result<Vec<u8>, String> {
    let payload = wincode::serialize(map).map_err(|err| format!("{err}"))?;
    let mut bytes = Vec::with_capacity(8 + payload.len());
    bytes.extend_from_slice(COMPILED_MAGIC);
    bytes.extend_from_slice(&COMPILED_VERSION.to_le_bytes());
    bytes.extend(payload);

    Ok(bytes)
}

fn decode_compiled(bytes: &[u8]) -> Result<CompiledMap, String> {
    if bytes.len() < 8 || bytes[..4] != COMPILED_MAGIC[..] {
        return Err("compiled map header is invalid".to_string());
    }

    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);

    if version == COMPILED_VERSION_V1 {
        let legacy: CompiledMapV1 =
            wincode::deserialize(&bytes[8..]).map_err(|err| format!("{err}"))?;

        return Ok(upgrade_compiled(legacy));
    }

    if version != COMPILED_VERSION {
        return Err(format!("compiled map version {version} is unsupported"));
    }

    wincode::deserialize(&bytes[8..]).map_err(|err| format!("{err}"))
}

fn upgrade_compiled(legacy: CompiledMapV1) -> CompiledMap {
    let mut entities = Vec::with_capacity(legacy.entities.len());

    for entity in legacy.entities {
        let mut brushes = Vec::with_capacity(entity.brushes.len());

        for brush in entity.brushes {
            let mut faces = Vec::with_capacity(brush.faces.len());

            for face in brush.faces {
                let (axis_u, axis_v) = basis(face.normal);
                faces.push(CompiledFace {
                    texture: face.texture,
                    normal: face.normal,
                    distance: face.distance,
                    material: face.material,
                    axis_u,
                    axis_v,
                    shift_u: 0.0,
                    shift_v: 0.0,
                    scale_u: 1.0,
                    scale_v: 1.0,
                });
            }

            brushes.push(CompiledBrush { faces });
        }

        entities.push(CompiledEntity {
            keys: entity.keys,
            brushes,
        });
    }

    CompiledMap { entities }
}

fn brush_map_from_compiled(compiled: CompiledMap) -> Result<BrushMap, String> {
    let mut map = BrushMap::new();
    let mut bank = MaterialBank::new(FileSource::game());

    for entity in compiled.entities {
        if let Some(origin) = player_start(&entity) {
            map.spawns.push(origin);
        }

        for brush in entity.brushes {
            let mut planes = Vec::with_capacity(brush.faces.len());

            for face in brush.faces {
                let Some(mut plane) = Plane::new(face.normal, face.distance, face.material) else {
                    return Err("compiled face is invalid".to_string());
                };
                plane.tex = bank.load(&face.texture);
                plane.axis_u = face.axis_u;
                plane.axis_v = face.axis_v;
                plane.shift_u = face.shift_u;
                plane.shift_v = face.shift_v;
                plane.scale_u = face.scale_u;
                plane.scale_v = face.scale_v;
                planes.push(plane);
            }

            let Some(brush) = Brush::from_planes(planes) else {
                return Err("compiled brush is not a closed solid".to_string());
            };

            if !map.push_quiet(brush) {
                return Err("compiled brush bounds are invalid".to_string());
            }
        }
    }

    map.graphics.material_names = bank.ordered_names();
    map.graphics.materials = bank.into_materials();
    map.finalize();

    Ok(map)
}

fn brush_map_from_bsp(bytes: &[u8], path: &Path) -> Result<BrushMap, String> {
    let bsp = vbsp::Bsp::read(bytes).map_err(|err| format!("bsp: {err}"))?;
    let mut map = BrushMap::new();
    let solid = vbsp::data::BrushFlags::SOLID
        | vbsp::data::BrushFlags::PLAYERCLIP
        | vbsp::data::BrushFlags::GRATE
        | vbsp::data::BrushFlags::WINDOW
        | vbsp::data::BrushFlags::MOVEABLE;
    let non_solid_surf = vbsp::data::TextureFlags::TRIGGER
        | vbsp::data::TextureFlags::SKIP
        | vbsp::data::TextureFlags::HINT
        | vbsp::data::TextureFlags::SKY
        | vbsp::data::TextureFlags::SKY2D;

    for brush in &bsp.brushes {
        if !brush.flags.intersects(solid) {
            continue;
        }

        let start = brush.brush_side as usize;
        let end = start + brush.num_brush_sides as usize;
        let Some(sides) = bsp.brush_sides.get(start..end) else {
            continue;
        };
        let mut collides = false;

        for side in sides {
            if side.bevel != 0 {
                continue;
            }

            if side.texture_info < 0 {
                collides = true;

                break;
            }

            let Some(info) = bsp.texture_info(side.texture_info as usize) else {
                collides = true;

                break;
            };

            if !info.flags.intersects(non_solid_surf) {
                collides = true;

                break;
            }
        }

        if !collides {
            continue;
        }

        let mut planes = Vec::new();

        for side in sides {
            if side.bevel != 0 {
                continue;
            }

            let Some(plane) = bsp.planes.get(side.plane as usize) else {
                continue;
            };

            let material = if side.texture_info >= 0 {
                bsp.texture_info(side.texture_info as usize)
                    .map(|info| texture_material(info.name()))
                    .unwrap_or(1)
            } else {
                1
            };
            let normal = Vector3::new(
                plane.normal.x as f64,
                plane.normal.y as f64,
                plane.normal.z as f64,
            );
            let Some(stored) = Plane::new(normal, plane.dist as f64, material) else {
                continue;
            };

            planes.push(stored);
        }

        let Some(solid_brush) = Brush::from_planes_trusted(planes) else {
            continue;
        };

        let _ = map.push_quiet(solid_brush);
    }

    for entity in bsp.entities.iter() {
        let classname = entity.prop("classname").unwrap_or("");

        if classname != "info_player_start" && classname != "info_player_teamspawn" {
            continue;
        }

        if let Some(origin) = entity.prop("origin").and_then(parse_origin) {
            map.spawns.push(origin);
        }
    }

    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("map");
    let visual = super::bspvis::build(bytes, stem)?;
    map.mesh_cache = Some(visual.mesh.vertices);
    map.ranges = visual.mesh.ranges;
    map.graphics = visual.graphics;
    map.finalize();

    Ok(map)
}

impl CompiledMap {
    pub fn worldspawn() -> Self {
        Self {
            entities: vec![CompiledEntity {
                keys: vec![CompiledPair {
                    key: "classname".to_string(),
                    value: "worldspawn".to_string(),
                }],
                brushes: Vec::new(),
            }],
        }
    }

    pub fn open_source(name: &str) -> Result<(PathBuf, Self), String> {
        let path = find_map(name).ok_or_else(|| format!("map {name} was not found"))?;

        if is_compiled(&path) {
            let bytes =
                std::fs::read(&path).map_err(|err| format!("map {}: {err}", path.display()))?;
            let map = decode_compiled(&bytes)?;

            return Ok((path.with_extension("map"), map));
        }

        let text = std::fs::read_to_string(&path)
            .map_err(|err| format!("map {}: {err}", path.display()))?;
        let entities = parse_source(&text)?;

        Ok((path, compile_source(&entities)))
    }

    pub fn save_source(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("map {}: {err}", parent.display()))?;
            }
        }

        let text = map_text(self)?;
        std::fs::write(path, text).map_err(|err| format!("map {}: {err}", path.display()))?;
        let text_path = path
            .to_str()
            .ok_or_else(|| "map path is not utf-8".to_string())?;

        compile_map(text_path)
    }

    pub fn brush_count(&self) -> usize {
        let mut count = 0;

        for entity in &self.entities {
            count += entity.brushes.len();
        }

        count
    }

    pub fn add_box(&mut self, min: Vector3, max: Vector3, texture: &str) -> Option<usize> {
        let brush = box_brush(min, max, texture)?;
        let entity_index = self.worldspawn_index();
        let flat = self.flat_index(entity_index, self.entities[entity_index].brushes.len());
        self.entities[entity_index].brushes.push(brush);

        Some(flat)
    }

    pub fn remove_brush(&mut self, index: usize) -> bool {
        let mut cursor = 0;

        for entity in &mut self.entities {
            if index < cursor + entity.brushes.len() {
                entity.brushes.remove(index - cursor);

                return true;
            }

            cursor += entity.brushes.len();
        }

        false
    }

    pub fn translate_brush(&mut self, index: usize, delta: Vector3) -> bool {
        if !finite(delta) {
            return false;
        }

        let Some(brush) = self.brush_mut(index) else {
            return false;
        };

        for face in &mut brush.faces {
            face.distance +=
                face.normal.x * delta.x + face.normal.y * delta.y + face.normal.z * delta.z;
        }

        true
    }

    pub fn brush(&self, index: usize) -> Option<&CompiledBrush> {
        let mut cursor = 0;

        for entity in &self.entities {
            if index < cursor + entity.brushes.len() {
                return Some(&entity.brushes[index - cursor]);
            }

            cursor += entity.brushes.len();
        }

        None
    }

    pub fn brush_owner(&self, index: usize) -> Option<&CompiledEntity> {
        let mut cursor = 0;

        for entity in &self.entities {
            if index < cursor + entity.brushes.len() {
                return Some(entity);
            }

            cursor += entity.brushes.len();
        }

        None
    }

    pub fn set_brush_texture(&mut self, index: usize, texture: &str) -> bool {
        if !valid_texture(texture) {
            return false;
        }

        let material = texture_material(texture);
        let Some(brush) = self.brush_mut(index) else {
            return false;
        };
        let mut changed = false;

        for face in &mut brush.faces {
            if face.texture != texture {
                face.texture = texture.to_string();
                face.material = material;
                changed = true;
            }
        }

        changed
    }

    pub fn duplicate_brush(&mut self, index: usize, offset: Vector3) -> Option<usize> {
        if !finite(offset) {
            return None;
        }

        let mut cursor = 0;
        let mut idx = 0;

        while idx < self.entities.len() {
            let count = self.entities[idx].brushes.len();

            if index < cursor + count {
                let mut copy = self.entities[idx].brushes[index - cursor].clone();

                for face in &mut copy.faces {
                    face.distance += face.normal.x * offset.x
                        + face.normal.y * offset.y
                        + face.normal.z * offset.z;
                }

                self.entities[idx].brushes.push(copy);

                return Some(cursor + count);
            }

            cursor += count;
            idx += 1;
        }

        None
    }

    pub fn brush_box(&self, index: usize) -> Option<(Vector3, Vector3)> {
        let brush = self.brush(index)?;
        let mut min = [f64::NAN; 3];
        let mut max = [f64::NAN; 3];

        for face in &brush.faces {
            let normal = [face.normal.x, face.normal.y, face.normal.z];
            let axis = box_axis(normal)?;

            if normal[axis] > 0.0 {
                max[axis] = face.distance;
            } else {
                min[axis] = -face.distance;
            }
        }

        let mut axis = 0;

        while axis < 3 {
            if !min[axis].is_finite() || !max[axis].is_finite() || max[axis] <= min[axis] {
                return None;
            }

            axis += 1;
        }

        Some((
            Vector3::new(min[0], min[1], min[2]),
            Vector3::new(max[0], max[1], max[2]),
        ))
    }

    pub fn set_brush_box(&mut self, index: usize, min: Vector3, max: Vector3) -> bool {
        if !finite(min) || !finite(max) || max.x <= min.x || max.y <= min.y || max.z <= min.z {
            return false;
        }

        if self.brush_box(index).is_none() {
            return false;
        }

        let Some(brush) = self.brush_mut(index) else {
            return false;
        };
        let low = [min.x, min.y, min.z];
        let high = [max.x, max.y, max.z];

        for face in &mut brush.faces {
            let normal = [face.normal.x, face.normal.y, face.normal.z];
            let Some(axis) = box_axis(normal) else {
                return false;
            };

            face.distance = if normal[axis] > 0.0 {
                high[axis]
            } else {
                -low[axis]
            };
        }

        true
    }

    pub fn brush_bounds(&self, index: usize) -> Option<(Vector3, Vector3)> {
        compiled_bounds(self.brush(index)?)
    }

    pub fn set_brush_size(&mut self, index: usize, size: Vector3) -> bool {
        if !finite(size) || size.x <= 1e-4 || size.y <= 1e-4 || size.z <= 1e-4 {
            return false;
        }

        let Some((min, max)) = self.brush_bounds(index) else {
            return false;
        };
        let old = Vector3::new(max.x - min.x, max.y - min.y, max.z - min.z);

        if (old.x - size.x).abs() < 1e-9
            && (old.y - size.y).abs() < 1e-9
            && (old.z - size.z).abs() < 1e-9
        {
            return false;
        }

        if self.brush_box(index).is_some() {
            return self.set_brush_box(
                index,
                min,
                Vector3::new(min.x + size.x, min.y + size.y, min.z + size.z),
            );
        }

        let scale = [size.x / old.x, size.y / old.y, size.z / old.z];
        let Some(brush) = self.brush_mut(index) else {
            return false;
        };
        let backup = brush.faces.clone();

        for face in &mut brush.faces {
            let normal = face.normal;
            let raw = Vector3::new(
                normal.x / scale[0],
                normal.y / scale[1],
                normal.z / scale[2],
            );
            let len = raw.len();

            if len <= LENGTH_EPS {
                brush.faces = backup;

                return false;
            }

            let inv = 1.0 / len;
            let rhs = face.distance - normal.dot(min) + raw.dot(min);
            face.normal = Vector3::new(raw.x * inv, raw.y * inv, raw.z * inv);
            face.distance = rhs * inv;
        }

        if compiled_bounds(brush).is_none() {
            brush.faces = backup;

            return false;
        }

        true
    }

    pub fn snap_brush(&mut self, index: usize, grid: f64) -> bool {
        if !grid.is_finite() || grid <= 0.0 {
            return false;
        }

        let Some((min, _)) = self.brush_bounds(index) else {
            return false;
        };
        let snapped = Vector3::new(
            (min.x / grid).round() * grid,
            (min.y / grid).round() * grid,
            (min.z / grid).round() * grid,
        );
        let delta = Vector3::new(snapped.x - min.x, snapped.y - min.y, snapped.z - min.z);

        if delta.x.abs() < 1e-9 && delta.y.abs() < 1e-9 && delta.z.abs() < 1e-9 {
            return false;
        }

        self.translate_brush(index, delta)
    }

    pub fn flip_brush(&mut self, index: usize, axis: usize) -> bool {
        if axis > 2 {
            return false;
        }

        let Some((min, max)) = self.brush_bounds(index) else {
            return false;
        };
        let center = [
            (min.x + max.x) * 0.5,
            (min.y + max.y) * 0.5,
            (min.z + max.z) * 0.5,
        ];
        let Some(brush) = self.brush_mut(index) else {
            return false;
        };
        let backup = brush.faces.clone();

        for face in &mut brush.faces {
            let normal = [face.normal.x, face.normal.y, face.normal.z];
            let mut flipped = normal;
            flipped[axis] = -flipped[axis];
            face.normal = Vector3::new(flipped[0], flipped[1], flipped[2]);
            face.distance -= 2.0 * normal[axis] * center[axis];
            face.axis_u = flip_axis(face.axis_u, axis);
            face.axis_v = flip_axis(face.axis_v, axis);
        }

        if compiled_bounds(brush).is_none() {
            brush.faces = backup;

            return false;
        }

        true
    }

    pub fn hollow_brush(&mut self, index: usize, thickness: f64) -> Option<usize> {
        if !thickness.is_finite() || thickness <= 0.0 {
            return None;
        }

        let (min, max) = self.brush_box(index)?;
        let span = [max.x - min.x, max.y - min.y, max.z - min.z];

        if thickness * 2.0 >= span[0] || thickness * 2.0 >= span[1] || thickness * 2.0 >= span[2] {
            return None;
        }

        let texture = self.brush(index)?.faces.first()?.texture.clone();
        let (entity, local) = self.brush_place(index)?;
        let low_z = min.z + thickness;
        let high_z = max.z - thickness;
        let low_y = min.y + thickness;
        let high_y = max.y - thickness;
        let walls = [
            (
                Vector3::new(min.x, min.y, min.z),
                Vector3::new(max.x, max.y, low_z),
            ),
            (
                Vector3::new(min.x, min.y, high_z),
                Vector3::new(max.x, max.y, max.z),
            ),
            (
                Vector3::new(min.x, min.y, low_z),
                Vector3::new(max.x, low_y, high_z),
            ),
            (
                Vector3::new(min.x, high_y, low_z),
                Vector3::new(max.x, max.y, high_z),
            ),
            (
                Vector3::new(min.x, low_y, low_z),
                Vector3::new(min.x + thickness, high_y, high_z),
            ),
            (
                Vector3::new(max.x - thickness, low_y, low_z),
                Vector3::new(max.x, high_y, high_z),
            ),
        ];
        let mut built = Vec::with_capacity(walls.len());

        for (low, high) in walls {
            built.push(box_brush(low, high, &texture)?);
        }

        self.entities[entity].brushes.remove(local);
        let mut offset = 0;

        while offset < built.len() {
            let brush = built[offset].clone();
            self.entities[entity].brushes.insert(local + offset, brush);
            offset += 1;
        }

        Some(self.flat_index(entity, local))
    }

    pub fn paste_brush(&mut self, source: &CompiledBrush, offset: Vector3) -> Option<usize> {
        if !finite(offset) || source.faces.len() < 4 {
            return None;
        }

        let mut copy = source.clone();

        for face in &mut copy.faces {
            if !valid_texture(&face.texture) || !finite(face.normal) || !face.distance.is_finite() {
                return None;
            }

            face.distance += face.normal.dot(offset);
        }

        if compiled_bounds(&copy).is_none() {
            return None;
        }

        let entity = self.worldspawn_index();
        let local = self.entities[entity].brushes.len();
        let flat = self.flat_index(entity, local);
        self.entities[entity].brushes.push(copy);

        Some(flat)
    }

    pub fn tie_brush(&mut self, index: usize, classname: &str) -> Option<usize> {
        if classname.is_empty() || !valid_line(classname) {
            return None;
        }

        let (entity, local) = self.brush_place(index)?;
        let already = self.entities[entity].brushes.len() == 1
            && !self.entity_is_worldspawn(entity)
            && self.entities[entity]
                .keys
                .iter()
                .any(|pair| pair.key == "classname" && pair.value == classname);

        if already {
            return None;
        }

        let brush = self.entities[entity].brushes.remove(local);
        self.drop_empty_entity(entity);
        self.entities.push(CompiledEntity {
            keys: vec![CompiledPair {
                key: "classname".to_string(),
                value: classname.to_string(),
            }],
            brushes: vec![brush],
        });

        Some(self.brush_count() - 1)
    }

    pub fn move_brush_to_world(&mut self, index: usize) -> Option<usize> {
        let (entity, local) = self.brush_place(index)?;

        if self.entity_is_worldspawn(entity) {
            return None;
        }

        let brush = self.entities[entity].brushes.remove(local);
        self.drop_empty_entity(entity);
        let world = self.worldspawn_index();
        let local = self.entities[world].brushes.len();
        let flat = self.flat_index(world, local);
        self.entities[world].brushes.push(brush);

        Some(flat)
    }

    pub fn set_entity_keys(&mut self, index: usize, keys: Vec<(String, String)>) -> bool {
        if keys.is_empty() {
            return false;
        }

        for (key, value) in &keys {
            if key.is_empty() || !valid_line(key) || !valid_line(value) {
                return false;
            }
        }

        let Some((entity, _)) = self.brush_place(index) else {
            return false;
        };
        let same = self.entities[entity].keys.len() == keys.len()
            && self.entities[entity]
                .keys
                .iter()
                .zip(&keys)
                .all(|(pair, (key, value))| pair.key == *key && pair.value == *value);

        if same {
            return false;
        }

        self.entities[entity].keys = keys
            .into_iter()
            .map(|(key, value)| CompiledPair { key, value })
            .collect();

        true
    }

    pub fn set_face_texture(&mut self, index: usize, face: usize, texture: &str) -> bool {
        if !valid_texture(texture) {
            return false;
        }

        let material = texture_material(texture);
        let Some(brush) = self.brush_mut(index) else {
            return false;
        };
        let Some(face) = brush.faces.get_mut(face) else {
            return false;
        };

        if face.texture == texture {
            return false;
        }

        face.texture = texture.to_string();
        face.material = material;

        true
    }

    pub fn set_face_scale(
        &mut self,
        index: usize,
        face: usize,
        scale_u: f64,
        scale_v: f64,
    ) -> bool {
        if !scale_u.is_finite()
            || !scale_v.is_finite()
            || scale_u.abs() < 1e-6
            || scale_v.abs() < 1e-6
        {
            return false;
        }

        let Some(brush) = self.brush_mut(index) else {
            return false;
        };
        let Some(face) = brush.faces.get_mut(face) else {
            return false;
        };

        if (face.scale_u - scale_u).abs() < 1e-12 && (face.scale_v - scale_v).abs() < 1e-12 {
            return false;
        }

        face.scale_u = scale_u;
        face.scale_v = scale_v;

        true
    }

    pub fn set_face_shift(
        &mut self,
        index: usize,
        face: usize,
        shift_u: f64,
        shift_v: f64,
    ) -> bool {
        if !shift_u.is_finite() || !shift_v.is_finite() {
            return false;
        }

        let Some(brush) = self.brush_mut(index) else {
            return false;
        };
        let Some(face) = brush.faces.get_mut(face) else {
            return false;
        };

        if (face.shift_u - shift_u).abs() < 1e-12 && (face.shift_v - shift_v).abs() < 1e-12 {
            return false;
        }

        face.shift_u = shift_u;
        face.shift_v = shift_v;

        true
    }

    pub fn replace_texture(&mut self, from: &str, to: &str, only: Option<usize>) -> usize {
        if from == to || !valid_texture(to) {
            return 0;
        }

        let material = texture_material(to);
        let mut count = 0;
        let mut flat = 0;

        for entity in &mut self.entities {
            for brush in &mut entity.brushes {
                let hit = match only {
                    Some(index) => index == flat,
                    None => true,
                };

                if hit {
                    for face in &mut brush.faces {
                        if face.texture == from {
                            face.texture = to.to_string();
                            face.material = material;
                            count += 1;
                        }
                    }
                }

                flat += 1;
            }
        }

        count
    }

    pub fn problems(&self) -> Vec<(Option<usize>, String)> {
        let mut out = Vec::new();
        let mut flat = 0;
        let mut entity_index = 0;

        while entity_index < self.entities.len() {
            let entity = &self.entities[entity_index];
            let mut class = String::new();
            let mut key = 0;

            while key < entity.keys.len() {
                let pair = &entity.keys[key];

                if pair.key == "classname" {
                    class = pair.value.clone();
                }

                if !valid_line(&pair.key) || !valid_line(&pair.value) {
                    out.push((
                        None,
                        format!("entity {entity_index} has a key that cannot be saved"),
                    ));
                }

                key += 1;
            }

            if class.is_empty() {
                out.push((None, format!("entity {entity_index} has no classname")));
            }

            if entity.brushes.is_empty() && class != "worldspawn" {
                out.push((
                    None,
                    format!("entity {entity_index} ({class}) has no brushes"),
                ));
            }

            let mut local = 0;

            while local < entity.brushes.len() {
                let brush = &entity.brushes[local];

                if brush.faces.len() < 4 {
                    out.push((Some(flat), format!("brush {flat} has fewer than 4 faces")));
                }

                if compiled_bounds(brush).is_none() {
                    out.push((Some(flat), format!("brush {flat} has no volume")));
                }

                for face in &brush.faces {
                    if !valid_texture(&face.texture) {
                        out.push((Some(flat), format!("brush {flat} has an invalid texture")));
                    }

                    if face.scale_u.abs() < 1e-8 || face.scale_v.abs() < 1e-8 {
                        out.push((Some(flat), format!("brush {flat} has a zero texture scale")));
                    }
                }

                flat += 1;
                local += 1;
            }

            entity_index += 1;
        }

        out
    }

    pub fn textures(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();

        for entity in &self.entities {
            for brush in &entity.brushes {
                for face in &brush.faces {
                    if !out.iter().any(|name| name == &face.texture) {
                        out.push(face.texture.clone());
                    }
                }
            }
        }

        out
    }

    fn worldspawn_index(&mut self) -> usize {
        let mut idx = 0;

        while idx < self.entities.len() {
            let mut key = 0;

            while key < self.entities[idx].keys.len() {
                let pair = &self.entities[idx].keys[key];

                if pair.key == "classname" && pair.value == "worldspawn" {
                    return idx;
                }

                key += 1;
            }

            idx += 1;
        }

        self.entities.insert(
            0,
            CompiledEntity {
                keys: vec![CompiledPair {
                    key: "classname".to_string(),
                    value: "worldspawn".to_string(),
                }],
                brushes: Vec::new(),
            },
        );

        0
    }

    fn brush_mut(&mut self, index: usize) -> Option<&mut CompiledBrush> {
        let mut cursor = 0;

        for entity in &mut self.entities {
            if index < cursor + entity.brushes.len() {
                return Some(&mut entity.brushes[index - cursor]);
            }

            cursor += entity.brushes.len();
        }

        None
    }

    fn brush_place(&self, index: usize) -> Option<(usize, usize)> {
        let mut cursor = 0;
        let mut entity = 0;

        while entity < self.entities.len() {
            let count = self.entities[entity].brushes.len();

            if index < cursor + count {
                return Some((entity, index - cursor));
            }

            cursor += count;
            entity += 1;
        }

        None
    }

    fn flat_index(&self, entity: usize, local: usize) -> usize {
        let mut flat = local;
        let mut idx = 0;

        while idx < entity && idx < self.entities.len() {
            flat += self.entities[idx].brushes.len();
            idx += 1;
        }

        flat
    }

    fn entity_is_worldspawn(&self, entity: usize) -> bool {
        self.entities.get(entity).is_some_and(|entity| {
            entity
                .keys
                .iter()
                .any(|pair| pair.key == "classname" && pair.value == "worldspawn")
        })
    }

    fn drop_empty_entity(&mut self, entity: usize) {
        if self.entity_is_worldspawn(entity) {
            return;
        }

        if self
            .entities
            .get(entity)
            .is_some_and(|entity| entity.brushes.is_empty())
        {
            self.entities.remove(entity);
        }
    }
}

fn map_text(map: &CompiledMap) -> Result<String, String> {
    let mut out = String::new();

    for entity in &map.entities {
        out.push_str("{\n");

        for pair in &entity.keys {
            if !valid_line(&pair.key) || !valid_line(&pair.value) {
                return Err("entity key cannot span lines".to_string());
            }

            out.push('"');
            push_escaped(&mut out, &pair.key);
            out.push_str("\" \"");
            push_escaped(&mut out, &pair.value);
            out.push_str("\"\n");
        }

        for brush in &entity.brushes {
            out.push_str("{\n");

            for face in &brush.faces {
                if !valid_texture(&face.texture) {
                    return Err(format!("texture {} cannot be written", face.texture));
                }

                let (p0, p1, p2) = face_points(face.normal, face.distance)
                    .ok_or_else(|| "face plane is invalid".to_string())?;
                out.push_str(&format!(
                    "( {} {} {} ) ( {} {} {} ) ( {} {} {} ) {} [ {} {} {} {} ] [ {} {} {} {} ] 0 {} {}\n",
                    format_component(p0.x),
                    format_component(p0.y),
                    format_component(p0.z),
                    format_component(p1.x),
                    format_component(p1.y),
                    format_component(p1.z),
                    format_component(p2.x),
                    format_component(p2.y),
                    format_component(p2.z),
                    face.texture,
                    format_component(face.axis_u.x),
                    format_component(face.axis_u.y),
                    format_component(face.axis_u.z),
                    format_component(face.shift_u),
                    format_component(face.axis_v.x),
                    format_component(face.axis_v.y),
                    format_component(face.axis_v.z),
                    format_component(face.shift_v),
                    format_component(face.scale_u),
                    format_component(face.scale_v),
                ));
            }

            out.push_str("}\n");
        }

        out.push_str("}\n");
    }

    Ok(out)
}

fn face_points(normal: Vector3, distance: f64) -> Option<(Vector3, Vector3, Vector3)> {
    let len_sq = normal.len_sq();

    if len_sq <= LENGTH_EPS * LENGTH_EPS {
        return None;
    }

    let inv = 1.0 / len_sq;
    let origin = Vector3::new(
        normal.x * distance * inv,
        normal.y * distance * inv,
        normal.z * distance * inv,
    );
    let len = len_sq.sqrt();
    let unit_normal = Vector3::new(normal.x / len, normal.y / len, normal.z / len);
    let (tangent, bitangent) = basis(unit_normal);

    if tangent.len_sq() <= LENGTH_EPS || bitangent.len_sq() <= LENGTH_EPS {
        return None;
    }

    Some((
        origin,
        Vector3::new(
            origin.x + tangent.x,
            origin.y + tangent.y,
            origin.z + tangent.z,
        ),
        Vector3::new(
            origin.x + bitangent.x,
            origin.y + bitangent.y,
            origin.z + bitangent.z,
        ),
    ))
}

fn format_component(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }

    let rounded = value.round();

    if (value - rounded).abs() < 1e-6 && rounded.abs() < 1.0e15 {
        return format!("{}", rounded as i64);
    }

    format!("{value:.17}")
}

fn box_brush(min: Vector3, max: Vector3, texture: &str) -> Option<CompiledBrush> {
    if Brush::aabb(min, max, 1).is_none() || !valid_texture(texture) {
        return None;
    }

    let material = texture_material(texture);
    let texture = texture.to_string();
    let specs = [
        (Vector3::new(1.0, 0.0, 0.0), max.x),
        (Vector3::new(-1.0, 0.0, 0.0), -min.x),
        (Vector3::new(0.0, 1.0, 0.0), max.y),
        (Vector3::new(0.0, -1.0, 0.0), -min.y),
        (Vector3::new(0.0, 0.0, 1.0), max.z),
        (Vector3::new(0.0, 0.0, -1.0), -min.z),
    ];
    let mut faces = Vec::with_capacity(specs.len());

    for (normal, distance) in specs {
        let (axis_u, axis_v) = quake_axes(normal);
        faces.push(CompiledFace {
            texture: texture.clone(),
            normal,
            distance,
            material,
            axis_u,
            axis_v,
            shift_u: 0.0,
            shift_v: 0.0,
            scale_u: 1.0,
            scale_v: 1.0,
        });
    }

    Some(CompiledBrush { faces })
}

fn compiled_bounds(brush: &CompiledBrush) -> Option<(Vector3, Vector3)> {
    let mut planes = Vec::with_capacity(brush.faces.len());

    for face in &brush.faces {
        planes.push(Plane::new(face.normal, face.distance, face.material)?);
    }

    let points = vertices(&planes);

    if points.is_empty() {
        return None;
    }

    let mut min = points[0];
    let mut max = points[0];

    for point in &points[1..] {
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        min.z = min.z.min(point.z);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
        max.z = max.z.max(point.z);
    }

    if !finite(min) || !finite(max) || max.x <= min.x || max.y <= min.y || max.z <= min.z {
        return None;
    }

    Some((min, max))
}

fn flip_axis(value: Vector3, axis: usize) -> Vector3 {
    let mut coords = [value.x, value.y, value.z];
    coords[axis] = -coords[axis];

    Vector3::new(coords[0], coords[1], coords[2])
}

fn box_axis(normal: [f64; 3]) -> Option<usize> {
    let mut axis = 0;

    while axis < 3 {
        let other_a = normal[(axis + 1) % 3];
        let other_b = normal[(axis + 2) % 3];

        if (normal[axis].abs() - 1.0).abs() < 1e-9 && other_a.abs() < 1e-9 && other_b.abs() < 1e-9 {
            return Some(axis);
        }

        axis += 1;
    }

    None
}

pub fn texture_name_ok(texture: &str) -> bool {
    valid_texture(texture)
}

fn valid_texture(texture: &str) -> bool {
    if texture.is_empty() {
        return false;
    }

    for ch in texture.chars() {
        if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '{' || ch == '}' || ch == '"' {
            return false;
        }
    }

    true
}

fn valid_line(text: &str) -> bool {
    !text.chars().any(|ch| ch == '\n' || ch == '\r')
}

fn push_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        if ch == '\\' || ch == '"' {
            out.push('\\');
        }

        out.push(ch);
    }
}

fn parse_source(text: &str) -> Result<Vec<SourceEntity>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parser = Parser {
        text,
        idx: 0,
        line: 1,
    };
    let mut entities = Vec::new();
    parser.skip();

    while !parser.eof() {
        entities.push(parser.entity()?);
        parser.skip();
    }

    Ok(entities)
}

#[allow(dead_code)]
fn parse_map(text: &str) -> Result<BrushMap, String> {
    let entities = parse_source(text)?;

    brush_map_from_compiled(compile_source(&entities))
}

struct Parser<'a> {
    text: &'a str,
    idx: usize,
    line: usize,
}

impl<'a> Parser<'a> {
    fn entity(&mut self) -> Result<SourceEntity, String> {
        self.expect('{')?;
        let mut keys = Vec::new();
        let mut brushes = Vec::new();
        let mut closed = false;

        while !self.eof() {
            self.skip();

            if self.peek() == Some('}') {
                self.bump();
                closed = true;

                break;
            }

            if self.peek() == Some('{') {
                brushes.push(self.brush()?);

                continue;
            }

            if self.peek() == Some('"') {
                let key = self.string()?;
                self.skip();
                let value = self.string()?;
                keys.push((key, value));

                continue;
            }

            return Err(self.err("expected a brush or a key"));
        }

        if !closed {
            return Err(self.err("unclosed entity"));
        }

        Ok(SourceEntity { keys, brushes })
    }

    fn brush(&mut self) -> Result<SourceBrush, String> {
        self.expect('{')?;
        let mut faces = Vec::new();

        loop {
            self.skip();

            if self.peek() == Some('}') {
                self.bump();

                break;
            }

            let p0 = self.point()?;
            let p1 = self.point()?;
            let p2 = self.point()?;
            let texture = self.texture()?;
            let Some(mut plane) = plane_from_points(p0, p1, p2, texture_material(&texture)) else {
                return Err(self.err("face points are colinear"));
            };
            let axes = self.axes(plane.normal)?;
            plane.axis_u = axes.axis_u;
            plane.axis_v = axes.axis_v;
            plane.shift_u = axes.shift_u;
            plane.shift_v = axes.shift_v;
            plane.scale_u = axes.scale_u;
            plane.scale_v = axes.scale_v;

            faces.push(SourceFace {
                texture,
                plane,
                axis_u: axes.axis_u,
                axis_v: axes.axis_v,
                shift_u: axes.shift_u,
                shift_v: axes.shift_v,
                scale_u: axes.scale_u,
                scale_v: axes.scale_v,
            });
        }

        let planes = faces.iter().map(|face| face.plane).collect();

        if Brush::from_planes(planes).is_none() {
            return Err(self.err("brush is not a closed solid"));
        }

        Ok(SourceBrush { faces })
    }

    fn point(&mut self) -> Result<Vector3, String> {
        self.skip();
        self.expect('(')?;
        let x = self.number()?;
        let y = self.number()?;
        let z = self.number()?;
        self.skip();
        self.expect(')')?;

        Ok(Vector3::new(x, y, z))
    }

    fn texture(&mut self) -> Result<String, String> {
        self.skip_inline();
        let start = self.idx;

        while let Some(ch) = self.peek() {
            if ch.is_whitespace() || ch == '(' || ch == ')' || ch == '{' || ch == '}' || ch == '"' {
                break;
            }

            self.bump();
        }

        if start == self.idx {
            return Err(self.err("expected a texture"));
        }

        Ok(self.text[start..self.idx].to_string())
    }

    fn axes(&mut self, normal: Vector3) -> Result<FaceAxes, String> {
        self.skip_inline();

        if self.peek() == Some('[') {
            let axis_u = self.bracket()?;
            let shift_u = self.number()?;
            self.skip_inline();
            self.expect(']')?;
            self.skip_inline();
            self.expect('[')?;
            let axis_v = self.bracket_vec()?;
            let shift_v = self.number()?;
            self.skip_inline();
            self.expect(']')?;
            let _rotation = self.number().unwrap_or(0.0);
            let scale_u = nonzero_scale(self.number().unwrap_or(1.0));
            let scale_v = nonzero_scale(self.number().unwrap_or(1.0));
            self.skip_line();

            return Ok(FaceAxes {
                axis_u,
                axis_v,
                shift_u,
                shift_v,
                scale_u,
                scale_v,
            });
        }

        let shift_u = self.number().unwrap_or(0.0);
        let shift_v = self.number().unwrap_or(0.0);
        let rotation = self.number().unwrap_or(0.0);
        let scale_u = nonzero_scale(self.number().unwrap_or(1.0));
        let scale_v = nonzero_scale(self.number().unwrap_or(1.0));
        self.skip_line();
        let (axis_u, axis_v) = rotate_axes(quake_axes(normal), normal, rotation);

        Ok(FaceAxes {
            axis_u,
            axis_v,
            shift_u,
            shift_v,
            scale_u,
            scale_v,
        })
    }

    fn bracket(&mut self) -> Result<Vector3, String> {
        self.skip_inline();
        self.expect('[')?;

        self.bracket_vec()
    }

    fn bracket_vec(&mut self) -> Result<Vector3, String> {
        let x = self.number()?;
        let y = self.number()?;
        let z = self.number()?;

        Ok(Vector3::new(x, y, z))
    }

    fn string(&mut self) -> Result<String, String> {
        self.skip();
        self.expect('"')?;
        let mut out = String::new();

        loop {
            let Some(ch) = self.bump() else {
                return Err(self.err("unterminated string"));
            };

            if ch == '"' {
                break;
            }

            if ch == '\\' {
                if let Some(next) = self.bump() {
                    out.push(next);
                }

                continue;
            }

            out.push(ch);
        }

        Ok(out)
    }

    fn number(&mut self) -> Result<f64, String> {
        self.skip();
        let start = self.idx;

        if self.peek() == Some('-') || self.peek() == Some('+') {
            self.bump();
        }

        let mut saw_digit = false;
        let mut saw_dot = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                saw_digit = true;
                self.bump();

                continue;
            }

            if ch == '.' && !saw_dot {
                saw_dot = true;
                self.bump();

                continue;
            }

            break;
        }

        if !saw_digit {
            return Err(self.err("expected a number"));
        }

        self.text[start..self.idx]
            .parse()
            .map_err(|_| self.err("bad number"))
    }

    fn expect(&mut self, want: char) -> Result<(), String> {
        self.skip();

        if self.peek() == Some(want) {
            self.bump();

            return Ok(());
        }

        Err(self.err(&format!("expected {want}")))
    }

    fn skip(&mut self) {
        loop {
            self.skip_inline();

            if self.starts_with("//") {
                self.skip_line();

                continue;
            }

            if self.peek() == Some('\n') {
                self.bump();

                continue;
            }

            break;
        }
    }

    fn skip_inline(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == ' ' || ch == '\t' || ch == '\r' {
                self.bump();

                continue;
            }

            break;
        }
    }

    fn skip_line(&mut self) {
        while let Some(ch) = self.bump() {
            if ch == '\n' {
                break;
            }
        }
    }

    fn eof(&mut self) -> bool {
        self.skip();

        self.idx >= self.text.len()
    }

    fn peek(&self) -> Option<char> {
        self.text[self.idx..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.idx += ch.len_utf8();

        if ch == '\n' {
            self.line += 1;
        }

        Some(ch)
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.text[self.idx..].starts_with(prefix)
    }

    fn err(&self, message: &str) -> String {
        format!("line {}: {message}", self.line)
    }
}

fn plane_from_points(p0: Vector3, p1: Vector3, p2: Vector3, material: u16) -> Option<Plane> {
    let a = Vector3::new(p1.x - p0.x, p1.y - p0.y, p1.z - p0.z);
    let b = Vector3::new(p2.x - p0.x, p2.y - p0.y, p2.z - p0.z);
    let normal = a.cross(b);
    let distance = normal.x * p0.x + normal.y * p0.y + normal.z * p0.z;

    Plane::new(normal, distance, material)
}

struct FaceAxes {
    axis_u: Vector3,
    axis_v: Vector3,
    shift_u: f64,
    shift_v: f64,
    scale_u: f64,
    scale_v: f64,
}

fn nonzero_scale(scale: f64) -> f64 {
    if scale.abs() < 1e-8 {
        return 1.0;
    }

    scale
}

fn quake_axes(normal: Vector3) -> (Vector3, Vector3) {
    let ax = normal.x.abs();
    let ay = normal.y.abs();
    let az = normal.z.abs();

    if ax >= ay && ax >= az {
        return (Vector3::new(0.0, 1.0, 0.0), Vector3::new(0.0, 0.0, -1.0));
    }

    if ay >= ax && ay >= az {
        return (Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 0.0, -1.0));
    }

    (Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, -1.0, 0.0))
}

fn rotate_axes(axes: (Vector3, Vector3), normal: Vector3, degrees: f64) -> (Vector3, Vector3) {
    if degrees.abs() < 1e-6 {
        return axes;
    }

    let rad = degrees.to_radians();
    let (s, c) = (rad.sin(), rad.cos());
    let rotate = |axis: Vector3| {
        let dot = normal.dot(axis);
        let parallel = Vector3::new(normal.x * dot, normal.y * dot, normal.z * dot);
        let flat = Vector3::new(
            axis.x - parallel.x,
            axis.y - parallel.y,
            axis.z - parallel.z,
        );
        let cross = normal.cross(flat);

        Vector3::new(
            parallel.x + flat.x * c + cross.x * s,
            parallel.y + flat.y * c + cross.y * s,
            parallel.z + flat.z * c + cross.z * s,
        )
    };

    (rotate(axes.0), rotate(axes.1))
}

fn texture_material(name: &str) -> u16 {
    let mut hash = 2166136261u32;

    for byte in name.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16777619);
    }

    let id = (hash & 0xffff) as u16;

    if id == 0 {
        return 1;
    }

    id
}

impl Plane {
    fn new(normal: Vector3, distance: f64, material: u16) -> Option<Self> {
        if !finite(normal) || !distance.is_finite() {
            return None;
        }

        let len = normal.len();

        if len <= LENGTH_EPS {
            return None;
        }

        let inv = 1.0 / len;

        let normal = Vector3::new(normal.x * inv, normal.y * inv, normal.z * inv);
        let (axis_u, axis_v) = basis(normal);

        Some(Self {
            normal,
            distance: distance * inv,
            material,
            tex: MATERIAL_NONE,
            axis_u,
            axis_v,
            shift_u: 0.0,
            shift_v: 0.0,
            scale_u: 1.0,
            scale_v: 1.0,
        })
    }
}

fn closed(planes: &[Plane]) -> bool {
    let points = vertices(planes);

    if points.len() < 4 {
        return false;
    }

    let mut faces = 0usize;

    for plane in planes {
        let mut count = 0usize;

        for point in &points {
            if on_plane(plane, *point) {
                count += 1;
            }
        }

        if count >= 3 {
            faces += 1;
        }
    }

    faces >= 4
}

fn vertices(planes: &[Plane]) -> Vec<Vector3> {
    let mut points = Vec::new();
    let count = planes.len();
    let mut idx = 0;

    while idx < count {
        let mut jdx = idx + 1;

        while jdx < count {
            let mut kdx = jdx + 1;

            while kdx < count {
                if let Some(point) = intersect(&planes[idx], &planes[jdx], &planes[kdx]) {
                    if contains(planes, point) {
                        push_unique(&mut points, point);
                    }
                }

                kdx += 1;
            }

            jdx += 1;
        }

        idx += 1;
    }

    points
}

fn intersect(a: &Plane, b: &Plane, c: &Plane) -> Option<Vector3> {
    let bc = b.normal.cross(c.normal);
    let denom = a.normal.dot(bc);

    if denom.abs() <= LENGTH_EPS {
        return None;
    }

    let ca = c.normal.cross(a.normal);
    let ab = a.normal.cross(b.normal);
    let inv = 1.0 / denom;
    let point = Vector3::new(
        (a.distance * bc.x + b.distance * ca.x + c.distance * ab.x) * inv,
        (a.distance * bc.y + b.distance * ca.y + c.distance * ab.y) * inv,
        (a.distance * bc.z + b.distance * ca.z + c.distance * ab.z) * inv,
    );

    if finite(point) {
        Some(point)
    } else {
        None
    }
}

fn contains(planes: &[Plane], point: Vector3) -> bool {
    for plane in planes {
        if plane.normal.dot(point) > plane.distance + PLANE_EPS {
            return false;
        }
    }

    true
}

fn on_plane(plane: &Plane, point: Vector3) -> bool {
    (plane.normal.dot(point) - plane.distance).abs() <= PLANE_EPS
}

fn push_unique(points: &mut Vec<Vector3>, point: Vector3) {
    for existing in points.iter() {
        let dx = existing.x - point.x;
        let dy = existing.y - point.y;
        let dz = existing.z - point.z;

        if dx * dx + dy * dy + dz * dz <= PLANE_EPS * PLANE_EPS {
            return;
        }
    }

    points.push(point);
}

fn polygons(brush: &Brush) -> Vec<Poly> {
    let points = vertices(&brush.planes);
    let mut polys = Vec::new();

    for plane in &brush.planes {
        let mut face = Vec::new();

        for point in &points {
            if on_plane(plane, *point) {
                face.push(*point);
            }
        }

        if face.len() < 3 {
            continue;
        }

        order_face(plane.normal, &mut face);
        polys.push(Poly {
            normal: plane.normal,
            points: face,
            material: plane.material,
            tex: plane.tex,
            axis_u: plane.axis_u,
            axis_v: plane.axis_v,
            shift_u: plane.shift_u,
            shift_v: plane.shift_v,
            scale_u: plane.scale_u,
            scale_v: plane.scale_v,
            width: 1.0,
            height: 1.0,
        });
    }

    polys
}

fn order_face(normal: Vector3, points: &mut Vec<Vector3>) {
    let mut center = Vector3::new(0.0, 0.0, 0.0);

    for point in points.iter() {
        center.x += point.x;
        center.y += point.y;
        center.z += point.z;
    }

    let scale = 1.0 / points.len() as f64;
    center.x *= scale;
    center.y *= scale;
    center.z *= scale;
    let (tangent, bitangent) = basis(normal);
    points.sort_by(|left, right| {
        let left_angle = angle(*left, center, tangent, bitangent);
        let right_angle = angle(*right, center, tangent, bitangent);
        left_angle
            .partial_cmp(&right_angle)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

fn basis(normal: Vector3) -> (Vector3, Vector3) {
    let axis = if normal.z.abs() < 0.9 {
        Vector3::new(0.0, 0.0, 1.0)
    } else {
        Vector3::new(0.0, 1.0, 0.0)
    };
    let tangent = unit(normal.cross(axis));
    let bitangent = unit(normal.cross(tangent));

    (tangent, bitangent)
}

fn angle(point: Vector3, center: Vector3, tangent: Vector3, bitangent: Vector3) -> f64 {
    let x = point.x - center.x;
    let y = point.y - center.y;
    let z = point.z - center.z;
    let along = x * tangent.x + y * tangent.y + z * tangent.z;
    let across = x * bitangent.x + y * bitangent.y + z * bitangent.z;

    across.atan2(along)
}

fn buried(points: &[Vector3], owner: usize, brushes: &[Brush]) -> bool {
    for (idx, brush) in brushes.iter().enumerate() {
        if idx == owner {
            continue;
        }

        let mut covered = true;

        for point in points {
            if !contains(&brush.planes, *point) {
                covered = false;

                break;
            }
        }

        if covered {
            return true;
        }
    }

    false
}

fn push_poly(vertices: &mut Vec<f32>, poly: &Poly, origin: Vector3, width: f64, height: f64) {
    let [red, green, blue] = material_rgb(poly.material);
    let shade = 0.42 + 0.58 * ((poly.normal.z as f32 + 1.0) * 0.5);

    push_fan(
        vertices,
        poly,
        red * shade,
        green * shade,
        blue * shade,
        origin,
        width,
        height,
    );
}

fn push_poly_color(
    vertices: &mut Vec<f32>,
    poly: &Poly,
    color: [f32; 3],
    origin: Vector3,
    width: f64,
    height: f64,
) {
    push_fan(
        vertices, poly, color[0], color[1], color[2], origin, width, height,
    );
}

fn push_fan(
    vertices: &mut Vec<f32>,
    poly: &Poly,
    red: f32,
    green: f32,
    blue: f32,
    origin: Vector3,
    width: f64,
    height: f64,
) {
    let mut idx = 1;

    while idx + 1 < poly.points.len() {
        let a = poly.points[0];
        let b = poly.points[idx];
        let c = poly.points[idx + 1];

        if tri_area(a, b, c) > AREA_EPS {
            push_tri(
                vertices, poly, a, b, c, red, green, blue, origin, width, height,
            );
        }

        idx += 1;
    }
}

fn tri_area(a: Vector3, b: Vector3, c: Vector3) -> f64 {
    let ab = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
    let ac = Vector3::new(c.x - a.x, c.y - a.y, c.z - a.z);

    ab.cross(ac).len_sq()
}

fn push_tri(
    vertices: &mut Vec<f32>,
    poly: &Poly,
    a: Vector3,
    b: Vector3,
    c: Vector3,
    red: f32,
    green: f32,
    blue: f32,
    origin: Vector3,
    width: f64,
    height: f64,
) {
    let positions = [
        [
            (a.x - origin.x) as f32,
            (a.y - origin.y) as f32,
            (a.z - origin.z) as f32,
        ],
        [
            (b.x - origin.x) as f32,
            (b.y - origin.y) as f32,
            (b.z - origin.z) as f32,
        ],
        [
            (c.x - origin.x) as f32,
            (c.y - origin.y) as f32,
            (c.z - origin.z) as f32,
        ],
    ];
    let normal = tri_normal(positions[0], positions[1], positions[2]);
    let tangent = [
        poly.axis_u.x as f32,
        poly.axis_u.y as f32,
        poly.axis_u.z as f32,
        1.0,
    ];
    let tangent = if tangent[0] == 0.0 && tangent[1] == 0.0 && tangent[2] == 0.0 {
        tri_tangent(positions[0], positions[1], normal)
    } else {
        let unit = super::surface::normalize3([tangent[0], tangent[1], tangent[2]]);
        [unit[0], unit[1], unit[2], tangent[3]]
    };
    let points = [a, b, c];
    let mut idx = 0;

    while idx < 3 {
        push_vertex(
            vertices,
            positions[idx],
            normal,
            tangent,
            surface_uv(poly, points[idx], width, height),
            [0.0, 0.0],
            [red, green, blue],
            0.0,
            poly.tex as f32,
        );
        idx += 1;
    }
}

fn surface_uv(poly: &Poly, point: Vector3, width: f64, height: f64) -> [f32; 2] {
    let scale_u = if poly.scale_u.abs() < 1e-8 {
        1.0
    } else {
        poly.scale_u
    };
    let scale_v = if poly.scale_v.abs() < 1e-8 {
        1.0
    } else {
        poly.scale_v
    };
    let u = (point.dot(poly.axis_u) / scale_u + poly.shift_u) / width.max(1.0);
    let v = (point.dot(poly.axis_v) / scale_v + poly.shift_v) / height.max(1.0);

    [u as f32, v as f32]
}

fn shift_cached(cache: &[f32], origin: Vector3) -> Vec<f32> {
    if origin.x == 0.0 && origin.y == 0.0 && origin.z == 0.0 {
        return cache.to_vec();
    }

    let mut out = cache.to_vec();
    let mut idx = 0;

    while idx + STRIDE <= out.len() {
        out[idx] = (f64::from(out[idx]) - origin.x) as f32;
        out[idx + 1] = (f64::from(out[idx + 1]) - origin.y) as f32;
        out[idx + 2] = (f64::from(out[idx + 2]) - origin.z) as f32;
        idx += STRIDE;
    }

    out
}

fn material_rgb(id: u16) -> [f32; 3] {
    let mut n = (id as u32).wrapping_mul(1664525).wrapping_add(1013904223);
    let red = 0.35 + ((n >> 16) & 255) as f32 / 255.0 * 0.5;
    n = n.wrapping_mul(1664525).wrapping_add(1013904223);
    let green = 0.35 + ((n >> 16) & 255) as f32 / 255.0 * 0.5;
    n = n.wrapping_mul(1664525).wrapping_add(1013904223);
    let blue = 0.35 + ((n >> 16) & 255) as f32 / 255.0 * 0.5;

    [red, green, blue]
}

fn hit_brush(
    brush: &Brush,
    start: Vector3,
    dir: Vector3,
    max_dist: f64,
) -> Option<(f64, Option<Vector3>)> {
    hit_planes(&brush.planes, start, dir, max_dist)
}

fn expand_brush(brush: &Brush, mins: Vector3, maxs: Vector3) -> Vec<Plane> {
    let mut planes = Vec::with_capacity(brush.planes.len());

    for plane in &brush.planes {
        let mut expanded = *plane;
        expanded.distance -= hull_min_dot(plane.normal, mins, maxs);
        planes.push(expanded);
    }

    planes
}

fn hull_min_dot(normal: Vector3, mins: Vector3, maxs: Vector3) -> f64 {
    axis_extent(normal.x, mins.x, maxs.x)
        + axis_extent(normal.y, mins.y, maxs.y)
        + axis_extent(normal.z, mins.z, maxs.z)
}

fn axis_extent(normal: f64, min: f64, max: f64) -> f64 {
    if normal > 0.0 {
        normal * min
    } else {
        normal * max
    }
}

fn parse_origin(text: &str) -> Option<Vector3> {
    let mut parts = text.split_whitespace();
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    let z = parts.next()?.parse().ok()?;
    let origin = Vector3::new(x, y, z);

    if finite(origin) {
        Some(origin)
    } else {
        None
    }
}

fn player_start(entity: &CompiledEntity) -> Option<Vector3> {
    let mut spawn = false;
    let mut origin = None;

    for pair in &entity.keys {
        if pair.key == "classname"
            && (pair.value == "info_player_start" || pair.value == "info_player_teamspawn")
        {
            spawn = true;
        }

        if pair.key == "origin" {
            origin = parse_origin(&pair.value);
        }
    }

    if spawn {
        origin
    } else {
        None
    }
}

fn scale_edits(edits: &mut [BrushEdit], ratio: f64) {
    let mut idx = 0;

    while idx < edits.len() {
        match &mut edits[idx] {
            BrushEdit::Box(brush) => {
                brush.min.x *= ratio;
                brush.min.y *= ratio;
                brush.min.z *= ratio;
                brush.max.x *= ratio;
                brush.max.y *= ratio;
                brush.max.z *= ratio;
            }
            BrushEdit::Convex { planes, .. } => {
                let mut plane = 0;

                while plane < planes.len() {
                    planes[plane].distance *= ratio;
                    plane += 1;
                }
            }
            BrushEdit::Move { delta, .. } => {
                delta.x *= ratio;
                delta.y *= ratio;
                delta.z *= ratio;
            }
            BrushEdit::Remove(_) | BrushEdit::Clear => {}
        }

        idx += 1;
    }
}

fn brush_aabb(brush: &Brush) -> Option<Aabb> {
    let points = vertices(&brush.planes);

    if points.is_empty() {
        return None;
    }

    let mut min = points[0];
    let mut max = points[0];

    for point in &points[1..] {
        min.x = min.x.min(point.x);
        min.y = min.y.min(point.y);
        min.z = min.z.min(point.z);
        max.x = max.x.max(point.x);
        max.y = max.y.max(point.y);
        max.z = max.z.max(point.z);
    }

    if !finite(min) || !finite(max) {
        return None;
    }

    Some(Aabb { min, max })
}

fn segment_aabb(start: Vector3, end: Vector3) -> (Vector3, Vector3) {
    (
        Vector3::new(start.x.min(end.x), start.y.min(end.y), start.z.min(end.z)),
        Vector3::new(start.x.max(end.x), start.y.max(end.y), start.z.max(end.z)),
    )
}

fn hit_planes(
    planes: &[Plane],
    start: Vector3,
    dir: Vector3,
    max_dist: f64,
) -> Option<(f64, Option<Vector3>)> {
    let mut t_enter = 0.0;
    let mut t_exit = max_dist;
    let mut enter_normal = None;

    for plane in planes {
        let denom = plane.normal.dot(dir);
        let offset = plane.distance - plane.normal.dot(start);

        if denom.abs() <= RAY_EPS {
            if offset < -PLANE_EPS {
                return None;
            }

            continue;
        }

        let t_hit = offset / denom;

        if denom < 0.0 {
            if t_hit > t_enter {
                t_enter = t_hit;
                enter_normal = Some(plane.normal);
            }
        } else if t_hit < t_exit {
            t_exit = t_hit;
        }

        if t_enter > t_exit + PLANE_EPS {
            return None;
        }
    }

    if t_exit < 0.0 || t_enter > max_dist {
        return None;
    }

    if t_enter <= PLANE_EPS {
        if contains(planes, start) {
            return Some((0.0, None));
        }

        if t_enter < 0.0 {
            return None;
        }
    }

    Some((t_enter, enter_normal))
}

fn unit(value: Vector3) -> Vector3 {
    let len = value.len();

    if len <= LENGTH_EPS {
        return Vector3::new(0.0, 0.0, 0.0);
    }

    let inv = 1.0 / len;

    Vector3::new(value.x * inv, value.y * inv, value.z * inv)
}

fn positive_scale(scale: f64) -> Option<f64> {
    if scale.is_finite() && scale > 0.0 {
        Some(scale)
    } else {
        None
    }
}

fn finite(point: Vector3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() < 1e-4
    }

    fn box_map() -> BrushMap {
        let mut map = BrushMap::new();
        assert!(map.add_box(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0), 1));

        map
    }

    #[test]
    fn set_scale_grows_the_box() {
        let mut map = box_map();
        assert!(!map.set_scale(0.0));
        assert!(map.set_scale(2.0));
        assert!(near(map.scale(), 2.0));
        let mesh = map.mesh();
        let mut max = 0.0f32;
        let mut idx = 0;

        while idx + 2 < mesh.len() {
            max = max
                .max(mesh[idx].abs())
                .max(mesh[idx + 1].abs())
                .max(mesh[idx + 2].abs());
            idx += STRIDE;
        }

        assert!(near(f64::from(max), 2.0));
    }

    #[test]
    fn set_scale_moves_stored_bounds() {
        let mut map = BrushMap::new();
        assert!(map.add_box(
            Vector3::new(-4.0, -3.0, -2.0),
            Vector3::new(-1.0, -1.0, -1.0),
            1,
        ));
        assert!(map.set_scale(2.0));
        assert!(near(map.bounds[0].min.x, -8.0));
        assert!(near(map.bounds[0].min.y, -6.0));
        assert!(near(map.bounds[0].min.z, -4.0));
        assert!(near(map.bounds[0].max.x, -2.0));
        assert!(near(map.bounds[0].max.y, -2.0));
        assert!(near(map.bounds[0].max.z, -2.0));
        assert_eq!(map.take_scale(), Some(2.0));
        assert!(map.take_scale().is_none());
        assert!(map.set_scale(2.0));
        assert!(near(map.bounds[0].min.x, -8.0));
        assert!(near(map.bounds[0].max.z, -2.0));
        assert!(map.take_scale().is_none());
    }

    #[test]
    fn runtime_boxes_follow_scale_and_place_does_not_record() {
        let mut map = BrushMap::new();
        assert!(map.add_box(Vector3::new(0.0, 0.0, 0.0), Vector3::new(2.0, 1.0, 1.0), 4,));
        assert!(map.place_box(Vector3::new(4.0, 0.0, 0.0), Vector3::new(5.0, 1.0, 1.0), 4,));
        assert_eq!(map.edits().len(), 1);
        assert!(map.set_scale(2.0));
        match &map.edits()[0] {
            BrushEdit::Box(brush) => {
                assert!(near(brush.min.x, 0.0));
                assert!(near(brush.max.x, 4.0));
                assert_eq!(brush.material, 4);
            }
            _ => panic!("expected a box edit"),
        }
        let pending = map.take_edits();
        assert_eq!(pending.len(), 1);
        match &pending[0] {
            BrushEdit::Box(brush) => assert!(near(brush.max.y, 2.0)),
            _ => panic!("expected a box edit"),
        }
        assert!(map.take_edits().is_empty());
        assert_eq!(map.edits().len(), 1);
        let index = map.add_box_index(Vector3::new(8.0, 0.0, 0.0), Vector3::new(10.0, 2.0, 2.0), 4);
        assert_eq!(index, Some(2));
        assert!(map.move_brush(2, Vector3::new(1.0, 0.0, 0.0)));
        assert!(near(map.bounds[2].min.x, 9.0));
        assert!(map.remove_brush(2));
        assert_eq!(map.len(), 2);
        map.clear();
        assert_eq!(map.edits(), &[BrushEdit::Clear]);
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn edits_replay_onto_an_empty_map() {
        let mut map = BrushMap::new();
        let index = map
            .add_box_index(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0), 2)
            .unwrap();
        assert!(map.move_brush(index, Vector3::new(3.0, 0.0, 0.0)));
        assert!(map
            .add_convex_index(
                vec![
                    BrushPlane {
                        normal: Vector3::new(1.0, 0.0, 0.0),
                        distance: 2.0,
                    },
                    BrushPlane {
                        normal: Vector3::new(-1.0, 0.0, 0.0),
                        distance: -1.0,
                    },
                    BrushPlane {
                        normal: Vector3::new(0.0, 1.0, 0.0),
                        distance: 1.0,
                    },
                    BrushPlane {
                        normal: Vector3::new(0.0, -1.0, 0.0),
                        distance: 0.0,
                    },
                    BrushPlane {
                        normal: Vector3::new(0.0, 0.0, 1.0),
                        distance: 1.0,
                    },
                    BrushPlane {
                        normal: Vector3::new(0.0, 0.0, -1.0),
                        distance: 0.0,
                    },
                ],
                5,
            )
            .is_some());
        let edits = map.edits().to_vec();
        let mut client = BrushMap::new();
        let mut idx = 0;

        while idx < edits.len() {
            assert!(client.apply_edit(&edits[idx]));
            idx += 1;
        }

        assert_eq!(client.len(), 2);
        assert!(near(client.bounds[0].min.x, 3.0));
        assert!(near(client.bounds[1].min.x, 1.0));
        assert!(client.apply_edit(&BrushEdit::Clear));
        assert_eq!(client.len(), 0);
    }

    #[test]
    fn box_mesh_faces_outward() {
        let map = box_map();
        let mesh = map.mesh();

        assert_eq!(mesh.len(), 36 * STRIDE);
        assert!(faces_point_outward(&mesh, [0.5, 0.5, 0.5]));
    }

    #[test]
    fn mesh_at_keeps_coordinates_near_the_anchor() {
        let map = box_map();
        let mesh = map.mesh_at(Vector3::new(100_000.0, 0.0, 0.0));
        let mut idx = 0;

        while idx < mesh.len() {
            assert!(mesh[idx] < -99_998.0, "{}", mesh[idx]);
            assert!(mesh[idx] > -100_001.0, "{}", mesh[idx]);
            idx += STRIDE;
        }
    }

    #[test]
    fn adjacent_boxes_drop_the_shared_wall() {
        let mut map = BrushMap::new();
        assert!(map.add_box(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0), 1));
        assert!(map.add_box(Vector3::new(1.0, 0.0, 0.0), Vector3::new(2.0, 1.0, 1.0), 1));
        let mesh = map.mesh();

        assert_eq!(mesh.len(), 60 * STRIDE);
        let hit = map
            .trace(Vector3::new(-1.0, 0.5, 0.5), Vector3::new(3.0, 0.5, 0.5))
            .unwrap();

        assert_eq!(hit.brush, 0);
        assert!(near(hit.distance, 1.0));
        assert!(near(hit.position.x, 0.0));
        assert!(near(hit.normal.unwrap().x, -1.0));
    }

    #[test]
    fn trace_hits_the_entered_face() {
        let map = box_map();
        let hit = map
            .trace(Vector3::new(-1.5, 0.5, 0.5), Vector3::new(1.5, 0.5, 0.5))
            .unwrap();

        assert_eq!(hit.brush, 0);
        assert!(near(hit.distance, 1.5));
        assert!(near(hit.position.x, 0.0));
        assert!(near(hit.normal.unwrap().x, -1.0));
        assert!(near(hit.normal.unwrap().y, 0.0));
        assert!(near(hit.normal.unwrap().z, 0.0));

        let down = map
            .trace(Vector3::new(0.5, 0.5, 2.5), Vector3::new(0.5, 0.5, -1.0))
            .unwrap();

        assert!(near(down.distance, 1.5));
        assert!(near(down.position.z, 1.0));
        assert!(near(down.normal.unwrap().z, 1.0));
    }

    #[test]
    fn sweep_expands_the_box_by_the_hull() {
        let map = box_map();
        let mins = Vector3::new(-0.3, -0.3, 0.0);
        let maxs = Vector3::new(0.3, 0.3, 1.6);
        let hit = map
            .sweep(
                Vector3::new(-1.5, 0.5, 0.5),
                Vector3::new(1.5, 0.5, 0.5),
                mins,
                maxs,
            )
            .unwrap();

        assert!(near(hit.distance, 1.2));
        assert!(near(hit.position.x, -0.3));
        assert!(near(hit.normal.unwrap().x, -1.0));
    }

    #[test]
    fn player_start_is_kept() {
        let mut compiled = CompiledMap::worldspawn();
        compiled.entities.push(CompiledEntity {
            keys: vec![
                CompiledPair {
                    key: "classname".to_string(),
                    value: "info_player_start".to_string(),
                },
                CompiledPair {
                    key: "origin".to_string(),
                    value: "0 28 2".to_string(),
                },
            ],
            brushes: Vec::new(),
        });
        let mut map = BrushMap::new();
        map.load_document(&compiled).unwrap();

        assert_eq!(map.spawns().len(), 1);
        assert!(near(map.spawns()[0].x, 0.0));
        assert!(near(map.spawns()[0].y, 28.0));
        assert!(near(map.spawns()[0].z, 2.0));
    }

    #[test]
    fn trace_from_inside_and_misses() {
        let map = box_map();
        let inside = map
            .trace(Vector3::new(0.5, 0.5, 0.5), Vector3::new(4.0, 0.5, 0.5))
            .unwrap();

        assert_eq!(inside.normal, None);
        assert!(near(inside.distance, 0.0));
        assert!(map
            .trace(Vector3::new(1.5, 0.5, 0.5), Vector3::new(3.5, 0.5, 0.5))
            .is_none());
        assert!(map
            .trace(
                Vector3::new(f64::NAN, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0)
            )
            .is_none());
    }

    #[test]
    fn ramp_mesh_and_trace() {
        let mut map = BrushMap::new();
        assert!(map.add_convex(
            vec![
                BrushPlane {
                    normal: Vector3::new(0.0, 0.0, -1.0),
                    distance: 0.0
                },
                BrushPlane {
                    normal: Vector3::new(0.0, -1.0, 0.0),
                    distance: 0.0
                },
                BrushPlane {
                    normal: Vector3::new(0.0, 1.0, 0.0),
                    distance: 2.0
                },
                BrushPlane {
                    normal: Vector3::new(-1.0, 0.0, 0.0),
                    distance: 0.0
                },
                BrushPlane {
                    normal: Vector3::new(1.0, 0.0, 0.0),
                    distance: 4.0
                },
                BrushPlane {
                    normal: Vector3::new(-1.0, 0.0, 1.0),
                    distance: 0.0
                },
            ],
            8,
        ));
        let mesh = map.mesh();

        assert_eq!(mesh.len(), 24 * STRIDE);
        assert!(faces_point_outward(&mesh, [2.0, 1.0, 0.5]));

        let hit = map
            .trace(Vector3::new(-1.0, 1.0, 0.5), Vector3::new(6.0, 1.0, 0.5))
            .unwrap();

        assert!(near(hit.distance, 1.5));
        assert!(near(hit.position.x, 0.5));
        assert!(near(hit.position.z, 0.5));
        assert!(near(hit.normal.unwrap().x, -1.0 / 2.0_f64.sqrt()));
        assert!(near(hit.normal.unwrap().z, 1.0 / 2.0_f64.sqrt()));
        assert!(map
            .trace(Vector3::new(2.0, 1.0, 5.0), Vector3::new(2.0, 1.0, 6.0))
            .is_none());
    }

    #[test]
    fn rejected_brushes_leave_the_map_alone() {
        assert!(Brush::aabb(Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 1.0), 1).is_none());
        assert!(Brush::aabb(
            Vector3::new(f64::NAN, 0.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1
        )
        .is_none());
        assert!(Brush::convex(vec![], 1).is_none());

        let mut map = box_map();

        assert!(!map.add_convex(
            vec![BrushPlane {
                normal: Vector3::new(0.0, 0.0, 1.0),
                distance: 1.0
            }],
            4
        ));
        assert_eq!(map.len(), 1);
        assert_eq!(map.mesh().len(), 36 * STRIDE);
        assert!(map.load_file("missing_brush_map").is_err());
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn parses_a_quake_map() {
        let text = r#"
// room
{
"classname" "worldspawn"
{
( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) city/floor 0 0 0 1 1
( 0 0 1 ) ( 1 0 1 ) ( 0 1 1 ) city/ceil 0 0 0 1 1
( 0 0 0 ) ( 0 0 1 ) ( 0 1 0 ) city/west 0 0 0 1 1
( 1 0 0 ) ( 1 1 0 ) ( 1 0 1 ) city/east 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 0 1 ) city/south 0 0 0 1 1
( 0 1 0 ) ( 0 1 1 ) ( 1 1 0 ) city/north 0 0 0 1 1
}
}
{
"classname" "info_player_start"
"origin" "0 0 1"
}
"#;
        let map = parse_map(text).unwrap();
        let mesh = map.mesh();

        assert_eq!(map.len(), 1);
        assert_eq!(mesh.len(), 36 * STRIDE);
        assert!(faces_point_outward(&mesh, [0.5, 0.5, 0.5]));

        let hit = map
            .trace(Vector3::new(-1.0, 0.5, 0.5), Vector3::new(2.0, 0.5, 0.5))
            .unwrap();

        assert!(near(hit.position.x, 0.0));
        assert!(near(hit.normal.unwrap().x, -1.0));
        assert!(parse_map("not a map").is_err());
    }

    #[test]
    fn compiled_map_keeps_brushes_and_entity_keys() {
        let text = r#"
{
"classname" "worldspawn"
{
( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) city/floor 0 0 0 1 1
( 0 0 1 ) ( 1 0 1 ) ( 0 1 1 ) city/ceil 0 0 0 1 1
( 0 0 0 ) ( 0 0 1 ) ( 0 1 0 ) city/west 0 0 0 1 1
( 1 0 0 ) ( 1 1 0 ) ( 1 0 1 ) city/east 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 0 1 ) city/south 0 0 0 1 1
( 0 1 0 ) ( 0 1 1 ) ( 1 1 0 ) city/north 0 0 0 1 1
}
}
{
"classname" "info_player_start"
"origin" "0 0 1"
}
"#;
        let source = parse_source(text).unwrap();
        let mut bytes = encode_compiled(&compile_source(&source)).unwrap();
        let compiled = decode_compiled(&bytes).unwrap();

        assert_eq!(&bytes[..4], b"CMAP");
        assert_eq!(compiled.entities.len(), 2);
        assert_eq!(compiled.entities[0].keys[0].key, "classname");
        assert_eq!(compiled.entities[0].keys[0].value, "worldspawn");
        assert_eq!(
            compiled.entities[0].brushes[0].faces[0].texture,
            "city/floor"
        );
        assert_eq!(compiled.entities[1].keys[0].value, "info_player_start");
        assert_eq!(compiled.entities[1].keys[1].value, "0 0 1");

        let mut map = BrushMap::new();
        map.install_compiled(&bytes).unwrap();

        assert_eq!(map.mesh(), parse_map(text).unwrap().mesh());

        bytes[4] = 99;
        assert!(decode_compiled(&bytes).is_err());
        assert!(map.install_compiled(b"nope").is_err());
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn compile_map_writes_beside_the_source() {
        let dir = std::env::temp_dir().join(format!("engine-map-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("box.map");
        std::fs::write(
            &source,
            r#"
{
"classname" "worldspawn"
{
( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) city/floor 0 0 0 1 1
( 0 0 1 ) ( 1 0 1 ) ( 0 1 1 ) city/ceil 0 0 0 1 1
( 0 0 0 ) ( 0 0 1 ) ( 0 1 0 ) city/west 0 0 0 1 1
( 1 0 0 ) ( 1 1 0 ) ( 1 0 1 ) city/east 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 0 1 ) city/south 0 0 0 1 1
( 0 1 0 ) ( 0 1 1 ) ( 1 1 0 ) city/north 0 0 0 1 1
}
}
"#,
        )
        .unwrap();
        let compiled = compile_map(source.to_str().unwrap()).unwrap();

        assert_eq!(compiled, dir.join("box.cmap"));

        let mut map = BrushMap::new();
        map.load_file(compiled.to_str().unwrap()).unwrap();

        assert_eq!(map.len(), 1);
        assert_eq!(map.mesh().len(), 36 * STRIDE);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_file_opens_a_cmap_without_recompiling() {
        let dir = std::env::temp_dir().join(format!("engine-cmap-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("box.map");
        let compiled = dir.join("box.cmap");
        let text = r#"
{
"classname" "worldspawn"
{
( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) city/floor 0 0 0 1 1
( 0 0 1 ) ( 1 0 1 ) ( 0 1 1 ) city/ceil 0 0 0 1 1
( 0 0 0 ) ( 0 0 1 ) ( 0 1 0 ) city/west 0 0 0 1 1
( 1 0 0 ) ( 1 1 0 ) ( 1 0 1 ) city/east 0 0 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 0 1 ) city/south 0 0 0 1 1
( 0 1 0 ) ( 0 1 1 ) ( 1 1 0 ) city/north 0 0 0 1 1
}
}
"#;
        std::fs::write(&source, "not a map").unwrap();
        let bytes = encode_compiled(&compile_source(&parse_source(text).unwrap())).unwrap();
        std::fs::write(&compiled, &bytes).unwrap();

        assert_eq!(resolve_map("box.cmap", &[dir.clone()]).unwrap(), compiled);

        let mut map = BrushMap::new();
        map.load_file(compiled.to_str().unwrap()).unwrap();

        assert_eq!(map.len(), 1);
        assert_eq!(map.mesh().len(), 36 * STRIDE);
        assert_eq!(std::fs::read(&source).unwrap(), b"not a map");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_a_valve_face() {
        let text = r#"
{
"mapversion" "220"
{
( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) DEV/FLOOR [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
( 0 0 1 ) ( 1 0 1 ) ( 0 1 1 ) DEV/CEIL [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
( 0 0 0 ) ( 0 0 1 ) ( 0 1 0 ) DEV/WEST [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1
( 1 0 0 ) ( 1 1 0 ) ( 1 0 1 ) DEV/EAST [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1
( 0 0 0 ) ( 1 0 0 ) ( 0 0 1 ) DEV/SOUTH [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1
( 0 1 0 ) ( 0 1 1 ) ( 1 1 0 ) DEV/NORTH [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1
}
}
"#;
        let map = parse_map(text).unwrap();

        assert_eq!(map.mesh().len(), 36 * STRIDE);
    }

    #[test]
    fn hall_map_file_has_a_floor_and_a_ramp() {
        let mut map = BrushMap::new();
        map.load_file("hall").unwrap();

        assert_eq!(map.len(), 8);

        let floor = map
            .trace(Vector3::new(0.0, 28.0, 4.0), Vector3::new(0.0, 28.0, -1.0))
            .unwrap();

        assert!(near(floor.distance, 3.0));
        assert!(near(floor.position.z, 1.0));
        assert!(near(floor.normal.unwrap().z, 1.0));

        let inside = map
            .trace(Vector3::new(0.0, 30.0, 0.5), Vector3::new(0.0, 30.0, 3.0))
            .unwrap();

        assert!(near(inside.distance, 0.0));
        assert_eq!(inside.normal, None);

        let ramp = map
            .trace(Vector3::new(10.0, 0.0, 1.0), Vector3::new(40.0, 0.0, 1.0))
            .unwrap();

        assert!(near(ramp.distance, 8.0));
        assert!(near(ramp.position.x, 18.0));
        assert!(near(ramp.position.z, 1.0));
        assert!(near(ramp.normal.unwrap().x, -1.0 / 5.0_f64.sqrt()));
        assert!(near(ramp.normal.unwrap().z, 2.0 / 5.0_f64.sqrt()));
    }

    #[test]
    fn source_roundtrip_keeps_hall_planes() {
        let (path, map) = CompiledMap::open_source("hall").unwrap();

        assert!(path.ends_with("hall.map"));
        assert_eq!(map.brush_count(), 8);

        let again = compile_source(&parse_source(&map_text(&map).unwrap()).unwrap());

        assert_eq!(again.entities.len(), map.entities.len());

        for (entity, other) in map.entities.iter().zip(&again.entities) {
            assert_eq!(entity.keys, other.keys);
            assert_eq!(entity.brushes.len(), other.brushes.len());

            for (brush, other_brush) in entity.brushes.iter().zip(&other.brushes) {
                assert_eq!(brush.faces.len(), other_brush.faces.len());

                for (face, other_face) in brush.faces.iter().zip(&other_brush.faces) {
                    assert_eq!(face.texture, other_face.texture);
                    assert_eq!(face.material, other_face.material);
                    assert!(near(face.normal.x, other_face.normal.x));
                    assert!(near(face.normal.y, other_face.normal.y));
                    assert!(near(face.normal.z, other_face.normal.z));
                    assert!(near(face.distance, other_face.distance));
                }
            }
        }
    }

    #[test]
    fn editor_box_saves_and_compiles() {
        let dir = std::env::temp_dir().join(format!("engine-editor-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("box.map");
        let mut map = CompiledMap::worldspawn();

        assert_eq!(
            map.add_box(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(2.0, 3.0, 4.0),
                "crate"
            ),
            Some(0)
        );
        assert!(map.translate_brush(0, Vector3::new(5.0, 0.0, 0.0)));

        let compiled = map.save_source(&source).unwrap();

        assert_eq!(compiled, dir.join("box.cmap"));

        let mut brushes = BrushMap::new();
        brushes.load_file(source.to_str().unwrap()).unwrap();
        let hit = brushes
            .trace(Vector3::new(0.0, 1.0, 1.0), Vector3::new(20.0, 1.0, 1.0))
            .unwrap();

        assert!(near(hit.position.x, 5.0));
        assert!(map.remove_brush(0));
        assert_eq!(map.brush_count(), 0);
        assert!(!map.remove_brush(0));
        assert!(map
            .add_box(
                Vector3::new(1.0, 1.0, 1.0),
                Vector3::new(1.0, 2.0, 2.0),
                "crate"
            )
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accepts_more_than_four_thousand_brushes() {
        let mut map = BrushMap::new();
        let mut idx = 0;

        while idx < 5000 {
            let origin = (idx as f64) * 3.0;
            assert!(map.add_box(
                Vector3::new(origin, 0.0, 0.0),
                Vector3::new(origin + 1.0, 1.0, 1.0),
                1
            ));
            idx += 1;
        }

        assert_eq!(map.len(), 5000);

        let hit = map
            .trace(
                Vector3::new(4999.0 * 3.0 - 1.0, 0.5, 0.5),
                Vector3::new(4999.0 * 3.0 + 0.5, 0.5, 0.5),
            )
            .unwrap();

        assert_eq!(hit.brush, 4999);
        assert!(near(hit.position.x, 4999.0 * 3.0));
    }

    #[test]
    fn spatial_grid_hits_nearby_brushes_only() {
        let mut map = BrushMap::new();

        assert!(map.add_box(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0), 1));
        assert!(map.add_box(
            Vector3::new(2000.0, 0.0, 0.0),
            Vector3::new(2001.0, 1.0, 1.0),
            2
        ));

        let near_hit = map
            .trace(Vector3::new(-1.0, 0.5, 0.5), Vector3::new(0.5, 0.5, 0.5))
            .unwrap();
        let far_hit = map
            .trace(
                Vector3::new(1999.0, 0.5, 0.5),
                Vector3::new(2000.5, 0.5, 0.5),
            )
            .unwrap();

        assert_eq!(near_hit.brush, 0);
        assert_eq!(far_hit.brush, 1);
        assert!(map.set_scale(1.0 / 24.0));
        let scaled = map
            .trace(
                Vector3::new(-1.0, 0.5 / 24.0, 0.5 / 24.0),
                Vector3::new(0.5 / 24.0, 0.5 / 24.0, 0.5 / 24.0),
            )
            .unwrap();
        assert_eq!(scaled.brush, 0);
        assert!(map
            .trace(
                Vector3::new(1000.0 / 24.0, 0.5 / 24.0, 0.5 / 24.0),
                Vector3::new(1001.0 / 24.0, 0.5 / 24.0, 0.5 / 24.0),
            )
            .is_none());
        assert!(map
            .trace(
                Vector3::new(1000.0, 0.5, 0.5),
                Vector3::new(1001.0, 0.5, 0.5),
            )
            .is_none());
    }

    #[test]
    fn brush_size_keeps_the_minimum_corner() {
        let mut map = CompiledMap::worldspawn();

        assert!(map
            .add_box(
                Vector3::new(1.0, 2.0, 3.0),
                Vector3::new(3.0, 6.0, 4.0),
                "solid"
            )
            .is_some());
        assert!(map.set_brush_size(0, Vector3::new(5.0, 4.0, 1.0)));
        let (min, max) = map.brush_box(0).unwrap();

        assert!(near(min.x, 1.0));
        assert!(near(min.y, 2.0));
        assert!(near(min.z, 3.0));
        assert!(near(max.x - min.x, 5.0));
        assert!(near(max.y - min.y, 4.0));
        assert!(near(max.z - min.z, 1.0));
        assert!(!map.set_brush_size(0, Vector3::new(5.0, 4.0, 1.0)));
    }

    #[test]
    fn ramp_size_scales_about_the_minimum_corner() {
        let (_, mut map) = CompiledMap::open_source("hall").unwrap();
        let mut index = 0;
        let mut ramp = None;

        while index < map.brush_count() {
            if map.brush_box(index).is_none() {
                ramp = Some(index);

                break;
            }

            index += 1;
        }

        let index = ramp.expect("hall has a ramp");
        let (min, max) = map.brush_bounds(index).unwrap();
        let size = Vector3::new((max.x - min.x) * 2.0, max.y - min.y, max.z - min.z);

        assert!(map.set_brush_size(index, size));
        let (low, high) = map.brush_bounds(index).unwrap();

        assert!(near(low.x, min.x));
        assert!(near(low.y, min.y));
        assert!(near(low.z, min.z));
        assert!(near(high.x - low.x, size.x));
        assert!(near(high.y - low.y, size.y));
        assert!(near(high.z - low.z, size.z));
    }

    #[test]
    fn snap_moves_the_minimum_corner_onto_the_grid() {
        let mut map = CompiledMap::worldspawn();

        assert!(map
            .add_box(
                Vector3::new(0.4, 0.2, 1.0),
                Vector3::new(2.4, 1.2, 2.0),
                "solid"
            )
            .is_some());
        assert!(map.snap_brush(0, 1.0));
        let (min, max) = map.brush_box(0).unwrap();

        assert!(near(min.x, 0.0));
        assert!(near(min.y, 0.0));
        assert!(near(min.z, 1.0));
        assert!(near(max.x - min.x, 2.0));
        assert!(near(max.y - min.y, 1.0));
        assert!(!map.snap_brush(0, 1.0));
    }

    #[test]
    fn flip_reverses_a_ramp_and_a_second_flip_restores_it() {
        let (_, mut map) = CompiledMap::open_source("hall").unwrap();
        let mut index = 0;
        let mut ramp = None;

        while index < map.brush_count() {
            if map.brush_box(index).is_none() {
                ramp = Some(index);

                break;
            }

            index += 1;
        }

        let index = ramp.expect("hall has a ramp");
        let before = map.brush(index).unwrap().clone();
        let (min, max) = map.brush_bounds(index).unwrap();

        assert!(map.flip_brush(index, 0));
        let (low, high) = map.brush_bounds(index).unwrap();

        assert!(near(low.x, min.x));
        assert!(near(high.x, max.x));
        assert!(map.flip_brush(index, 0));

        let after = map.brush(index).unwrap();
        assert_eq!(before.faces.len(), after.faces.len());
        let mut face = 0;

        while face < before.faces.len() {
            assert!(near(
                before.faces[face].normal.x,
                after.faces[face].normal.x
            ));
            assert!(near(
                before.faces[face].distance,
                after.faces[face].distance
            ));
            face += 1;
        }
    }

    #[test]
    fn hollow_replaces_a_box_with_six_walls() {
        let mut map = CompiledMap::worldspawn();

        assert!(map
            .add_box(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 8.0, 6.0),
                "solid"
            )
            .is_some());
        assert!(map.hollow_brush(0, 5.0).is_none());
        assert_eq!(map.hollow_brush(0, 1.0), Some(0));
        assert_eq!(map.brush_count(), 6);
        let (min, max) = map.brush_box(0).unwrap();

        assert!(near(min.z, 0.0));
        assert!(near(max.z, 1.0));
        assert!(near(max.x - min.x, 10.0));
        assert!(near(max.y - min.y, 8.0));
    }

    #[test]
    fn tie_and_move_to_world_keep_the_brush() {
        let mut map = CompiledMap::worldspawn();

        assert!(map
            .add_box(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 1.0, 1.0),
                "solid"
            )
            .is_some());
        let tied = map.tie_brush(0, "func_detail").unwrap();

        assert_eq!(map.brush_count(), 1);
        assert!(map
            .brush_owner(tied)
            .unwrap()
            .keys
            .iter()
            .any(|pair| { pair.key == "classname" && pair.value == "func_detail" }));
        assert!(map.tie_brush(tied, "func_detail").is_none());
        let world = map.move_brush_to_world(tied).unwrap();

        assert!(map
            .brush_owner(world)
            .unwrap()
            .keys
            .iter()
            .any(|pair| { pair.key == "classname" && pair.value == "worldspawn" }));
        assert!(map.move_brush_to_world(world).is_none());
    }

    #[test]
    fn replace_texture_rewrites_matching_faces() {
        let mut map = CompiledMap::worldspawn();

        assert!(map
            .add_box(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 1.0, 1.0),
                "solid"
            )
            .is_some());
        assert_eq!(map.replace_texture("solid", "floor", None), 6);
        assert_eq!(map.brush(0).unwrap().faces[0].texture, "floor");
        assert_eq!(map.replace_texture("missing", "floor", None), 0);
    }

    #[test]
    fn problems_flag_an_entity_with_no_classname() {
        let mut map = CompiledMap::worldspawn();
        map.entities.push(CompiledEntity {
            keys: Vec::new(),
            brushes: Vec::new(),
        });
        let problems = map.problems();

        assert!(problems
            .iter()
            .any(|(_, text)| text.contains("no classname")));
    }

    fn faces_point_outward(mesh: &[f32], center: [f32; 3]) -> bool {
        let mut idx = 0;

        while idx + STRIDE * 3 <= mesh.len() {
            let ax = mesh[idx];
            let ay = mesh[idx + 1];
            let az = mesh[idx + 2];
            let bx = mesh[idx + STRIDE];
            let by = mesh[idx + STRIDE + 1];
            let bz = mesh[idx + STRIDE + 2];
            let cx = mesh[idx + STRIDE * 2];
            let cy = mesh[idx + STRIDE * 2 + 1];
            let cz = mesh[idx + STRIDE * 2 + 2];
            let nx = (by - ay) * (cz - az) - (bz - az) * (cy - ay);
            let ny = (bz - az) * (cx - ax) - (bx - ax) * (cz - az);
            let nz = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            let toward_x = (ax + bx + cx) / 3.0 - center[0];
            let toward_y = (ay + by + cy) / 3.0 - center[1];
            let toward_z = (az + bz + cz) / 3.0 - center[2];

            if nx * toward_x + ny * toward_y + nz * toward_z <= 0.0 {
                return false;
            }

            idx += STRIDE * 3;
        }

        true
    }
}
