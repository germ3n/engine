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
const COMPILED_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushPlane {
    pub normal: Vector3,
    pub distance: f64,
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
}

#[derive(SchemaWrite, SchemaRead, Clone, Debug, PartialEq)]
pub struct CompiledFace {
    pub texture: String,
    pub normal: Vector3,
    pub distance: f64,
    pub material: u16,
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

    fn rebuild(&mut self, bounds: &[Aabb]) {
        self.cells.clear();

        for (idx, aabb) in bounds.iter().enumerate() {
            let x0 = cell_coord(aabb.min.x);
            let y0 = cell_coord(aabb.min.y);
            let z0 = cell_coord(aabb.min.z);
            let x1 = cell_coord(aabb.max.x);
            let y1 = cell_coord(aabb.max.y);
            let z1 = cell_coord(aabb.max.z);
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

    fn query(&self, min: Vector3, max: Vector3, brush_count: usize) -> Vec<usize> {
        if brush_count == 0 {
            return Vec::new();
        }

        let x0 = cell_coord(min.x);
        let y0 = cell_coord(min.y);
        let z0 = cell_coord(min.z);
        let x1 = cell_coord(max.x);
        let y1 = cell_coord(max.y);
        let z1 = cell_coord(max.z);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
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

fn cell_coord(value: f64) -> i32 {
    (value / GRID_CELL).floor() as i32
}

pub struct BrushMap {
    brushes: Vec<Brush>,
    bounds: Vec<Aabb>,
    spawns: Vec<Vector3>,
    revision: u64,
    grid: BrushGrid,
    mesh_cache: Option<Vec<f32>>,
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
        }
    }

    pub fn len(&self) -> usize {
        self.brushes.len()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn add_box(&mut self, min: Vector3, max: Vector3, material: u16) -> bool {
        let Some(brush) = Brush::aabb(min, max, material) else {
            return false;
        };

        self.push(brush)
    }

    pub fn add_convex(&mut self, planes: Vec<BrushPlane>, material: u16) -> bool {
        let Some(brush) = Brush::convex(planes, material) else {
            return false;
        };

        self.push(brush)
    }

    pub fn load_file(&mut self, name: &str) -> Result<(), String> {
        if let Some(path) = find_map(name) {
            if is_bsp(&path) {
                let bytes = std::fs::read(&path)
                    .map_err(|err| format!("map {}: {err}", path.display()))?;

                return self.install_bsp(&bytes);
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

    fn install_bsp(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut loaded = brush_map_from_bsp(bytes)?;
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

        self.brushes.clear();
        self.bounds.clear();
        self.spawns.clear();
        self.mesh_cache = None;
        self.grid.clear();
        self.touch();
    }

    pub fn spawns(&self) -> &[Vector3] {
        &self.spawns
    }

    pub fn mesh(&self) -> Vec<f32> {
        if let Some(cache) = &self.mesh_cache {
            return cache.clone();
        }

        self.build_mesh(None)
    }

    pub fn mesh_highlight(&self, selected: usize) -> Vec<f32> {
        self.build_mesh(Some(selected))
    }

    fn build_mesh(&self, selected: Option<usize>) -> Vec<f32> {
        let mut vertices = Vec::new();

        for (idx, brush) in self.brushes.iter().enumerate() {
            for poly in polygons(brush) {
                if buried(&poly.points, idx, &self.brushes) {
                    continue;
                }

                if selected == Some(idx) {
                    push_poly_color(&mut vertices, &poly, [1.0, 0.86, 0.28]);
                } else {
                    push_poly(&mut vertices, &poly);
                }
            }
        }

        vertices
    }

    pub fn trace(&self, start: Vector3, end: Vector3) -> Option<BrushHit> {
        if !finite(start) || !finite(end) {
            return None;
        }

        let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let max_dist = delta.len();
        let (min, max) = segment_aabb(start, end);
        let candidates = self.grid.query(min, max, self.brushes.len());

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
        let candidates = self.grid.query(query_min, query_max, self.brushes.len());
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

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.grid.rebuild(&self.bounds);
    }

    fn finalize(&mut self) {
        self.grid.rebuild(&self.bounds);
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

    if version != COMPILED_VERSION {
        return Err(format!("compiled map version {version} is unsupported"));
    }

    wincode::deserialize(&bytes[8..]).map_err(|err| format!("{err}"))
}

fn brush_map_from_compiled(compiled: CompiledMap) -> Result<BrushMap, String> {
    let mut map = BrushMap::new();

    for entity in compiled.entities {
        if let Some(origin) = player_start(&entity) {
            map.spawns.push(origin);
        }

        for brush in entity.brushes {
            let mut planes = Vec::with_capacity(brush.faces.len());

            for face in brush.faces {
                let Some(plane) = Plane::new(face.normal, face.distance, face.material) else {
                    return Err("compiled face is invalid".to_string());
                };

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

    map.finalize();

    Ok(map)
}

fn brush_map_from_bsp(bytes: &[u8]) -> Result<BrushMap, String> {
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

    let mut mesh = Vec::new();

    if let Some(model) = bsp.models().next() {
        for face in model.faces() {
            if !face.is_visible() {
                continue;
            }

            let material = texture_material(face.texture().name());
            let [red, green, blue] = material_rgb(material);
            let normal = face.normal();
            let shade = 0.42 + 0.58 * ((normal.z as f32 + 1.0) * 0.5).clamp(0.0, 1.0);
            let shaded = [red * shade, green * shade, blue * shade];

            for tri in face.triangulate() {
                let a = Vector3::new(tri[0].x as f64, tri[0].y as f64, tri[0].z as f64);
                let b = Vector3::new(tri[1].x as f64, tri[1].y as f64, tri[1].z as f64);
                let c = Vector3::new(tri[2].x as f64, tri[2].y as f64, tri[2].z as f64);

                if tri_area(a, b, c) <= AREA_EPS {
                    continue;
                }

                push_tri(&mut mesh, a, b, c, shaded[0], shaded[1], shaded[2]);
            }
        }
    }

    map.mesh_cache = Some(mesh);
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
            faces.push(CompiledFace {
                texture: texture.clone(),
                normal,
                distance,
                material,
            });
        }

        let entity_index = self.worldspawn_index();
        let mut flat = 0;
        let mut idx = 0;

        while idx < entity_index {
            flat += self.entities[idx].brushes.len();
            idx += 1;
        }

        flat += self.entities[entity_index].brushes.len();
        self.entities[entity_index]
            .brushes
            .push(CompiledBrush { faces });

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
                    "( {} {} {} ) ( {} {} {} ) ( {} {} {} ) {} 0 0 0 1 1\n",
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

fn parse_map(text: &str) -> Result<BrushMap, String> {
    let entities = parse_source(text)?;
    let mut map = BrushMap::new();

    for entity in entities {
        for brush in entity.brushes {
            let planes = brush.faces.into_iter().map(|face| face.plane).collect();
            let Some(solid) = Brush::from_planes(planes) else {
                return Err("brush is not a closed solid".to_string());
            };

            if !map.push_quiet(solid) {
                return Err("brush bounds are invalid".to_string());
            }
        }
    }

    map.finalize();

    Ok(map)
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
            self.skip_line();
            let Some(plane) = plane_from_points(p0, p1, p2, texture_material(&texture)) else {
                return Err(self.err("face points are colinear"));
            };

            faces.push(SourceFace { texture, plane });
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

        Some(Self {
            normal: Vector3::new(normal.x * inv, normal.y * inv, normal.z * inv),
            distance: distance * inv,
            material,
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

fn push_poly(vertices: &mut Vec<f32>, poly: &Poly) {
    let [red, green, blue] = material_rgb(poly.material);
    let shade = 0.42 + 0.58 * ((poly.normal.z as f32 + 1.0) * 0.5);

    push_fan(vertices, poly, red * shade, green * shade, blue * shade);
}

fn push_poly_color(vertices: &mut Vec<f32>, poly: &Poly, color: [f32; 3]) {
    push_fan(vertices, poly, color[0], color[1], color[2]);
}

fn push_fan(vertices: &mut Vec<f32>, poly: &Poly, red: f32, green: f32, blue: f32) {
    let mut idx = 1;

    while idx + 1 < poly.points.len() {
        let a = poly.points[0];
        let b = poly.points[idx];
        let c = poly.points[idx + 1];

        if tri_area(a, b, c) > AREA_EPS {
            push_tri(vertices, a, b, c, red, green, blue);
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
    a: Vector3,
    b: Vector3,
    c: Vector3,
    red: f32,
    green: f32,
    blue: f32,
) {
    push_vert(vertices, a, red, green, blue);
    push_vert(vertices, b, red, green, blue);
    push_vert(vertices, c, red, green, blue);
}

fn push_vert(vertices: &mut Vec<f32>, position: Vector3, red: f32, green: f32, blue: f32) {
    vertices.push(position.x as f32);
    vertices.push(position.y as f32);
    vertices.push(position.z as f32);
    vertices.push(red);
    vertices.push(green);
    vertices.push(blue);
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
        planes.push(Plane {
            normal: plane.normal,
            distance: plane.distance - hull_min_dot(plane.normal, mins, maxs),
            material: plane.material,
        });
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
    fn box_mesh_faces_outward() {
        let map = box_map();
        let mesh = map.mesh();

        assert_eq!(mesh.len(), 216);
        assert!(faces_point_outward(&mesh, [0.5, 0.5, 0.5]));
    }

    #[test]
    fn adjacent_boxes_drop_the_shared_wall() {
        let mut map = BrushMap::new();
        assert!(map.add_box(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0), 1));
        assert!(map.add_box(Vector3::new(1.0, 0.0, 0.0), Vector3::new(2.0, 1.0, 1.0), 1));
        let mesh = map.mesh();

        assert_eq!(mesh.len(), 360);
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

        assert_eq!(mesh.len(), 144);
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
        assert_eq!(map.mesh().len(), 216);
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
        assert_eq!(mesh.len(), 216);
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
        assert_eq!(map.mesh().len(), 216);
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
        assert_eq!(map.mesh().len(), 216);
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

        assert_eq!(map.mesh().len(), 216);
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

        assert!(map.add_box(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
            1
        ));
        assert!(map.add_box(
            Vector3::new(2000.0, 0.0, 0.0),
            Vector3::new(2001.0, 1.0, 1.0),
            2
        ));

        let near_hit = map
            .trace(
                Vector3::new(-1.0, 0.5, 0.5),
                Vector3::new(0.5, 0.5, 0.5),
            )
            .unwrap();
        let far_hit = map
            .trace(
                Vector3::new(1999.0, 0.5, 0.5),
                Vector3::new(2000.5, 0.5, 0.5),
            )
            .unwrap();

        assert_eq!(near_hit.brush, 0);
        assert_eq!(far_hit.brush, 1);
        assert!(map
            .trace(
                Vector3::new(1000.0, 0.5, 0.5),
                Vector3::new(1001.0, 0.5, 0.5),
            )
            .is_none());
    }

    fn faces_point_outward(mesh: &[f32], center: [f32; 3]) -> bool {
        let mut idx = 0;

        while idx + 18 <= mesh.len() {
            let ax = mesh[idx];
            let ay = mesh[idx + 1];
            let az = mesh[idx + 2];
            let bx = mesh[idx + 6];
            let by = mesh[idx + 7];
            let bz = mesh[idx + 8];
            let cx = mesh[idx + 12];
            let cy = mesh[idx + 13];
            let cz = mesh[idx + 14];
            let nx = (by - ay) * (cz - az) - (bz - az) * (cy - ay);
            let ny = (bz - az) * (cx - ax) - (bx - ax) * (cz - az);
            let nz = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            let toward_x = (ax + bx + cx) / 3.0 - center[0];
            let toward_y = (ay + by + cy) / 3.0 - center[1];
            let toward_z = (az + bz + cz) / 3.0 - center[2];

            if nx * toward_x + ny * toward_y + nz * toward_z <= 0.0 {
                return false;
            }

            idx += 18;
        }

        true
    }
}
