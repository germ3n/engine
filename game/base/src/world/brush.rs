use std::path::PathBuf;
use crate::script::libs::vector3::Vector3;

const MAX_PLANES: usize = 64;
const MAX_BRUSHES: usize = 4096;
const PLANE_EPS: f64 = 1e-4;
const LENGTH_EPS: f64 = 1e-8;
const RAY_EPS: f64 = 1e-8;
const AREA_EPS: f64 = 1e-10;

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

struct Plane {
    normal: Vector3,
    distance: f64,
    material: u16,
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
                BrushPlane { normal: Vector3::new(1.0, 0.0, 0.0), distance: max.x },
                BrushPlane { normal: Vector3::new(-1.0, 0.0, 0.0), distance: -min.x },
                BrushPlane { normal: Vector3::new(0.0, 1.0, 0.0), distance: max.y },
                BrushPlane { normal: Vector3::new(0.0, -1.0, 0.0), distance: -min.y },
                BrushPlane { normal: Vector3::new(0.0, 0.0, 1.0), distance: max.z },
                BrushPlane { normal: Vector3::new(0.0, 0.0, -1.0), distance: -min.z },
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
}

pub struct BrushMap {
    brushes: Vec<Brush>,
    revision: u64,
}

impl BrushMap {
    pub fn new() -> Self {
        Self {
            brushes: Vec::new(),
            revision: 0,
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
        let path = find_map(name).ok_or_else(|| format!("map {name} was not found"))?;
        let text = std::fs::read_to_string(&path).map_err(|err| format!("map {}: {err}", path.display()))?;
        let mut loaded = parse_map(&text)?;
        loaded.revision = self.revision.wrapping_add(loaded.revision).wrapping_add(1);
        *self = loaded;

        Ok(())
    }

    pub fn push(&mut self, brush: Brush) -> bool {
        if self.brushes.len() >= MAX_BRUSHES {
            return false;
        }

        self.brushes.push(brush);
        self.touch();

        true
    }

    pub fn clear(&mut self) {
        if self.brushes.is_empty() {
            return;
        }

        self.brushes.clear();
        self.touch();
    }

    pub fn mesh(&self) -> Vec<f32> {
        let mut vertices = Vec::new();

        for (idx, brush) in self.brushes.iter().enumerate() {
            for poly in polygons(brush) {
                if buried(&poly.points, idx, &self.brushes) {
                    continue;
                }

                push_poly(&mut vertices, &poly);
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

        if max_dist == 0.0 {
            for (idx, brush) in self.brushes.iter().enumerate() {
                if contains(&brush.planes, start) {
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

        for (idx, brush) in self.brushes.iter().enumerate() {
            let Some((distance, normal)) = hit_brush(brush, start, dir, max_dist) else {
                continue;
            };

            if best.as_ref().map(|hit| distance < hit.distance).unwrap_or(true) {
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
    }
}

impl Default for BrushMap {
    fn default() -> Self {
        Self::new()
    }
}

fn find_map(name: &str) -> Option<PathBuf> {
    let given = PathBuf::from(name);

    if given.exists() {
        return Some(given);
    }

    let file = if name.ends_with(".map") {
        name.to_string()
    } else {
        format!("{name}.map")
    };
    let mut candidates = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("maps").join(&file));
        candidates.push(cwd.join("game/base/maps").join(&file));
        candidates.push(cwd.join(&file));
    }

    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("maps").join(&file));

    for path in candidates {
        if path.exists() {
            return Some(path);
        }
    }

    None
}

fn parse_map(text: &str) -> Result<BrushMap, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parser = Parser { text, idx: 0, line: 1 };
    let mut map = BrushMap::new();
    parser.skip();

    while !parser.eof() {
        parser.expect('{')?;
        parser.skip();
        let mut closed = false;

        while !parser.eof() {
            parser.skip();

            if parser.peek() == Some('}') {
                parser.bump();
                closed = true;

                break;
            }

            if parser.peek() == Some('{') {
                let brush = parser.brush()?;

                if !map.push(brush) {
                    return Err(parser.err("too many brushes"));
                }

                continue;
            }

            if parser.peek() == Some('"') {
                parser.string()?;
                parser.skip();
                parser.string()?;

                continue;
            }

            return Err(parser.err("expected a brush or a key"));
        }

        if !closed {
            return Err(parser.err("unclosed entity"));
        }

        parser.skip();
    }

    Ok(map)
}

struct Parser<'a> {
    text: &'a str,
    idx: usize,
    line: usize,
}

impl<'a> Parser<'a> {
    fn brush(&mut self) -> Result<Brush, String> {
        self.expect('{')?;
        let mut planes = Vec::new();

        loop {
            self.skip();

            if self.peek() == Some('}') {
                self.bump();

                break;
            }

            let p0 = self.point()?;
            let p1 = self.point()?;
            let p2 = self.point()?;
            let name = self.texture()?;
            self.skip_line();
            let Some(plane) = plane_from_points(p0, p1, p2, texture_material(&name)) else {
                return Err(self.err("face points are colinear"));
            };

            planes.push(plane);
        }

        Brush::from_planes(planes).ok_or_else(|| self.err("brush is not a closed solid"))
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

        self.text[start..self.idx].parse().map_err(|_| self.err("bad number"))
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
        polys.push(Poly { normal: plane.normal, points: face, material: plane.material });
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
        left_angle.partial_cmp(&right_angle).unwrap_or(std::cmp::Ordering::Equal)
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
    let cr = red * shade;
    let cg = green * shade;
    let cb = blue * shade;
    let mut idx = 1;

    while idx + 1 < poly.points.len() {
        let a = poly.points[0];
        let b = poly.points[idx];
        let c = poly.points[idx + 1];

        if tri_area(a, b, c) > AREA_EPS {
            push_tri(vertices, a, b, c, cr, cg, cb);
        }

        idx += 1;
    }
}

fn tri_area(a: Vector3, b: Vector3, c: Vector3) -> f64 {
    let ab = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
    let ac = Vector3::new(c.x - a.x, c.y - a.y, c.z - a.z);

    ab.cross(ac).len_sq()
}

fn push_tri(vertices: &mut Vec<f32>, a: Vector3, b: Vector3, c: Vector3, red: f32, green: f32, blue: f32) {
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

fn hit_brush(brush: &Brush, start: Vector3, dir: Vector3, max_dist: f64) -> Option<(f64, Option<Vector3>)> {
    let mut t_enter = 0.0;
    let mut t_exit = max_dist;
    let mut enter_normal = None;

    for plane in &brush.planes {
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
        if contains(&brush.planes, start) {
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
        let hit = map.trace(Vector3::new(-1.0, 0.5, 0.5), Vector3::new(3.0, 0.5, 0.5)).unwrap();

        assert_eq!(hit.brush, 0);
        assert!(near(hit.distance, 1.0));
        assert!(near(hit.position.x, 0.0));
        assert!(near(hit.normal.unwrap().x, -1.0));
    }

    #[test]
    fn trace_hits_the_entered_face() {
        let map = box_map();
        let hit = map.trace(Vector3::new(-1.5, 0.5, 0.5), Vector3::new(1.5, 0.5, 0.5)).unwrap();

        assert_eq!(hit.brush, 0);
        assert!(near(hit.distance, 1.5));
        assert!(near(hit.position.x, 0.0));
        assert!(near(hit.normal.unwrap().x, -1.0));
        assert!(near(hit.normal.unwrap().y, 0.0));
        assert!(near(hit.normal.unwrap().z, 0.0));

        let down = map.trace(Vector3::new(0.5, 0.5, 2.5), Vector3::new(0.5, 0.5, -1.0)).unwrap();

        assert!(near(down.distance, 1.5));
        assert!(near(down.position.z, 1.0));
        assert!(near(down.normal.unwrap().z, 1.0));
    }

    #[test]
    fn trace_from_inside_and_misses() {
        let map = box_map();
        let inside = map.trace(Vector3::new(0.5, 0.5, 0.5), Vector3::new(4.0, 0.5, 0.5)).unwrap();

        assert_eq!(inside.normal, None);
        assert!(near(inside.distance, 0.0));
        assert!(map.trace(Vector3::new(1.5, 0.5, 0.5), Vector3::new(3.5, 0.5, 0.5)).is_none());
        assert!(map.trace(Vector3::new(f64::NAN, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).is_none());
    }

    #[test]
    fn ramp_mesh_and_trace() {
        let mut map = BrushMap::new();
        assert!(map.add_convex(
            vec![
                BrushPlane { normal: Vector3::new(0.0, 0.0, -1.0), distance: 0.0 },
                BrushPlane { normal: Vector3::new(0.0, -1.0, 0.0), distance: 0.0 },
                BrushPlane { normal: Vector3::new(0.0, 1.0, 0.0), distance: 2.0 },
                BrushPlane { normal: Vector3::new(-1.0, 0.0, 0.0), distance: 0.0 },
                BrushPlane { normal: Vector3::new(1.0, 0.0, 0.0), distance: 4.0 },
                BrushPlane { normal: Vector3::new(-1.0, 0.0, 1.0), distance: 0.0 },
            ],
            8,
        ));
        let mesh = map.mesh();

        assert_eq!(mesh.len(), 144);
        assert!(faces_point_outward(&mesh, [2.0, 1.0, 0.5]));

        let hit = map.trace(Vector3::new(-1.0, 1.0, 0.5), Vector3::new(6.0, 1.0, 0.5)).unwrap();

        assert!(near(hit.distance, 1.5));
        assert!(near(hit.position.x, 0.5));
        assert!(near(hit.position.z, 0.5));
        assert!(near(hit.normal.unwrap().x, -1.0 / 2.0_f64.sqrt()));
        assert!(near(hit.normal.unwrap().z, 1.0 / 2.0_f64.sqrt()));
        assert!(map.trace(Vector3::new(2.0, 1.0, 5.0), Vector3::new(2.0, 1.0, 6.0)).is_none());
    }

    #[test]
    fn rejected_brushes_leave_the_map_alone() {
        assert!(Brush::aabb(Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 1.0), 1).is_none());
        assert!(Brush::aabb(Vector3::new(f64::NAN, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0), 1).is_none());
        assert!(Brush::convex(vec![], 1).is_none());

        let mut map = box_map();

        assert!(!map.add_convex(vec![BrushPlane { normal: Vector3::new(0.0, 0.0, 1.0), distance: 1.0 }], 4));
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

        let hit = map.trace(Vector3::new(-1.0, 0.5, 0.5), Vector3::new(2.0, 0.5, 0.5)).unwrap();

        assert!(near(hit.position.x, 0.0));
        assert!(near(hit.normal.unwrap().x, -1.0));
        assert!(parse_map("not a map").is_err());
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

        let floor = map.trace(Vector3::new(0.0, 28.0, 4.0), Vector3::new(0.0, 28.0, -1.0)).unwrap();

        assert!(near(floor.distance, 3.0));
        assert!(near(floor.position.z, 1.0));
        assert!(near(floor.normal.unwrap().z, 1.0));

        let inside = map.trace(Vector3::new(0.0, 30.0, 0.5), Vector3::new(0.0, 30.0, 3.0)).unwrap();

        assert!(near(inside.distance, 0.0));
        assert_eq!(inside.normal, None);

        let ramp = map.trace(Vector3::new(10.0, 0.0, 1.0), Vector3::new(40.0, 0.0, 1.0)).unwrap();

        assert!(near(ramp.distance, 8.0));
        assert!(near(ramp.position.x, 18.0));
        assert!(near(ramp.position.z, 1.0));
        assert!(near(ramp.normal.unwrap().x, -1.0 / 5.0_f64.sqrt()));
        assert!(near(ramp.normal.unwrap().z, 2.0 / 5.0_f64.sqrt()));
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
