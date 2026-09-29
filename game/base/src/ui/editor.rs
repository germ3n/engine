use crate::script::libs::vector3::Vector3;
use crate::ui::backend;
use crate::ui::voxel::FlyCamera;
use crate::ui::window::Window;
use crate::ui::Color;
use crate::world::{
    find_voxel_file, Block, BlockPos, BrushHit, BrushMap, CompiledMap, Face, TraceHit, VoxelWorld,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;
use winit::event::{DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ControlFlow;
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::CursorGrabMode;

const TEXTURES: [&str; 7] = ["solid", "floor", "ceiling", "side", "crate", "ramp", "end"];
const GRIDS: [f64; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
const REACH: f64 = 8000.0;

const QUADS: [[(i32, i32, i32); 4]; 6] = [
    [(1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1)],
    [(0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)],
    [(0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)],
    [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)],
    [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)],
    [(0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)],
];

const SHADES: [f32; 6] = [0.72, 0.62, 0.58, 0.5, 1.0, 0.4];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Brush,
    Voxel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BrushTool {
    Select,
    Box,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VoxelTool {
    Paint,
    Erase,
}

struct Aim {
    point: Vector3,
    brush: Option<usize>,
    place: Option<BlockPos>,
    erase: Option<BlockPos>,
    surface: bool,
}

struct Editor {
    document: CompiledMap,
    map_path: PathBuf,
    voxel_path: PathBuf,
    brushes: BrushMap,
    voxels: VoxelWorld,
    camera: FlyCamera,
    mode: Mode,
    brush_tool: BrushTool,
    voxel_tool: VoxelTool,
    grid: f64,
    block: u16,
    texture: usize,
    selected: Option<usize>,
    anchor: Option<Vector3>,
    lift: f64,
    dirty: bool,
    message: String,
    cursor_x: f32,
    cursor_y: f32,
    painting: bool,
    last_cell: Option<BlockPos>,
}

pub fn run(map_name: &str) {
    let mut editor = match open_editor(map_name) {
        Ok(editor) => editor,
        Err(err) => {
            println!("[editor] {err}");

            return;
        }
    };

    if editor.map_path.exists() {
        println!("[editor] {}", editor.map_path.display());
    } else {
        println!("[editor] new {}", editor.map_path.display());
    }

    if editor.voxel_path.exists() {
        println!("[editor] {}", editor.voxel_path.display());
    }

    let mut window = backend::create();
    window.set_window_title(&editor.title());
    window.set_size(1600, 900);
    let event_loop = window.take_event_loop();
    let mut keys = HashSet::new();
    let mut modifiers = ModifiersState::default();
    let mut looking = false;
    let mut last_frame = Instant::now();
    let mut shown_title = editor.title();
    let mut solid: Vec<f32> = Vec::new();
    let mut solid_ready = false;
    let mut solid_brush = 0u64;
    let mut solid_voxel = 0u64;
    let mut solid_mark: Option<usize> = None;
    let mut picture: Vec<f32> = Vec::new();
    let mut picture_key = String::new();
    let mut picture_revision = 0u64;

    event_loop
        .run(move |event, window_target| {
            window_target.set_control_flow(ControlFlow::Poll);

            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => {
                        window_target.exit();
                    }
                    WindowEvent::Resized(physical_size) => {
                        window.set_size(physical_size.width, physical_size.height);
                    }
                    WindowEvent::Focused(false) => {
                        keys.clear();
                        looking = false;
                        editor.end_stroke();
                        set_capture(window.winit_window(), false);
                    }
                    WindowEvent::ModifiersChanged(next) => {
                        modifiers = next.state();
                    }
                    WindowEvent::CursorMoved { position, .. } => {
                        editor.cursor_x = position.x as f32;
                        editor.cursor_y = position.y as f32;

                        if editor.painting && !looking {
                            let size = window.winit_window().inner_size();
                            let aim = editor.pointer_aim(size.width, size.height);
                            editor.stroke(&aim);
                        }
                    }
                    WindowEvent::MouseWheel { delta, .. } => {
                        editor.on_wheel(wheel_steps(delta));
                    }
                    WindowEvent::MouseInput { state, button, .. } => {
                        if button == MouseButton::Right {
                            looking = state == ElementState::Pressed;
                            editor.end_stroke();
                            set_capture(window.winit_window(), looking);
                        } else if button == MouseButton::Left && state == ElementState::Released {
                            editor.end_stroke();
                        } else if button == MouseButton::Left
                            && state == ElementState::Pressed
                            && !looking
                        {
                            let size = window.winit_window().inner_size();
                            let aim = editor.pointer_aim(size.width, size.height);

                            if editor.mode == Mode::Voxel {
                                editor.painting = true;
                                editor.stroke(&aim);
                            } else {
                                editor.brush_click(&aim);
                            }
                        }
                    }
                    WindowEvent::KeyboardInput { event, .. } => {
                        if let PhysicalKey::Code(code) = event.physical_key {
                            let pressed = event.state == ElementState::Pressed;

                            if pressed {
                                keys.insert(code);
                            } else {
                                keys.remove(&code);
                            }

                            if pressed && code == KeyCode::Escape {
                                looking = false;
                                editor.end_stroke();
                                set_capture(window.winit_window(), false);
                                editor.cancel();
                            } else if pressed
                                && matches!(code, KeyCode::Delete | KeyCode::Backspace)
                            {
                                if !event.repeat {
                                    if editor.mode == Mode::Voxel {
                                        let size = window.winit_window().inner_size();
                                        let aim = editor.pointer_aim(size.width, size.height);
                                        editor.erase_at(&aim);
                                    } else {
                                        editor.delete_selection();
                                    }
                                }
                            } else if pressed {
                                let control = modifiers.control_key() || modifiers.super_key();
                                editor.on_key(code, event.repeat, control);
                            }
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        let size = window.winit_window().inner_size();
                        let aim = editor.pointer_aim(size.width, size.height);
                        let marked = editor.marked(&aim);

                        if !solid_ready
                            || solid_brush != editor.brushes.revision()
                            || solid_voxel != editor.voxels.revision()
                            || solid_mark != marked
                        {
                            solid = editor.voxels.mesh();

                            match marked {
                                Some(index) => solid.extend(editor.brushes.mesh_highlight(index)),
                                None => solid.extend(editor.brushes.mesh()),
                            }

                            solid_brush = editor.brushes.revision();
                            solid_voxel = editor.voxels.revision();
                            solid_mark = marked;
                            solid_ready = true;
                            picture_key.clear();
                        }

                        let key = overlay_key(&editor, &aim);

                        if picture_key != key {
                            picture = solid.clone();
                            push_overlay(&mut picture, &editor, &aim);
                            picture_key = key;
                            picture_revision = picture_revision.wrapping_add(1);
                        }

                        let aspect = size.width as f32 / size.height.max(1) as f32;
                        let view = editor.camera.scene(aspect, editor.voxels.scale() as f32);
                        let title = editor.title();

                        if title != shown_title {
                            window.set_window_title(&title);
                            shown_title = title;
                        }

                        let lines = hud_lines(&editor, &aim);
                        window.begin_frame(0.46, 0.62, 0.74);
                        window.draw_colored_mesh(&picture, picture_revision, &view);
                        draw_hud(&mut window, &lines);
                        window.render_text();
                        window.present();
                    }
                    _ => {}
                },
                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta },
                    ..
                } => {
                    if looking {
                        editor.camera.look(delta.0 as f32, delta.1 as f32);
                    }
                }
                Event::AboutToWait => {
                    let now = Instant::now();
                    let dt = now.duration_since(last_frame).as_secs_f32().min(0.1);
                    last_frame = now;
                    editor.fly(&keys, modifiers.shift_key(), dt);
                    window.winit_window().request_redraw();
                }
                _ => {}
            }
        })
        .unwrap();
}

fn open_editor(name: &str) -> Result<Editor, String> {
    let voxel_existing = find_voxel_file(name);
    let brush_query = if is_voxel_name(name) {
        map_stem(name).to_string()
    } else {
        name.to_string()
    };
    let (map_path, document, message) = match CompiledMap::open_source(&brush_query) {
        Ok((path, map)) => (path, map, String::new()),
        Err(err) => {
            if err != format!("map {brush_query} was not found") {
                return Err(err);
            }

            let map_path = match &voxel_existing {
                Some(path) => path.with_extension("map"),
                None => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("maps")
                    .join(format!("{}.map", map_stem(&brush_query))),
            };
            let message = format!("new {}", map_path.display());

            (map_path, CompiledMap::worldspawn(), message)
        }
    };
    let voxel_path = voxel_existing.unwrap_or_else(|| map_path.with_extension("vmap"));
    let mut brushes = BrushMap::new();
    brushes.load_document(&document)?;
    let mut voxels = VoxelWorld::new();

    if voxel_path.exists() {
        voxels.load_file(&voxel_path)?;
    }

    let mut camera = FlyCamera::new();
    let mut mesh = voxels.mesh();
    mesh.extend(brushes.mesh());
    frame_view(&mut camera, &mesh);
    let mode = if document.brush_count() == 0 && voxels.chunk_count() > 0 {
        Mode::Voxel
    } else {
        Mode::Brush
    };

    Ok(Editor {
        document,
        map_path,
        voxel_path,
        brushes,
        voxels,
        camera,
        mode,
        brush_tool: BrushTool::Select,
        voxel_tool: VoxelTool::Paint,
        grid: 1.0,
        block: 1,
        texture: 0,
        selected: None,
        anchor: None,
        lift: 0.0,
        dirty: false,
        message,
        cursor_x: 800.0,
        cursor_y: 450.0,
        painting: false,
        last_cell: None,
    })
}

impl Editor {
    fn title(&self) -> String {
        let name = file_label(&self.map_path);

        if self.dirty {
            format!("Map Editor - {name} *")
        } else {
            format!("Map Editor - {name}")
        }
    }

    fn pointer_aim(&self, width: u32, height: u32) -> Aim {
        let (origin, dir) = cursor_ray(
            &self.camera,
            self.cursor_x,
            self.cursor_y,
            width.max(1) as f32,
            height.max(1) as f32,
            self.voxels.scale() as f32,
        );

        self.aim(origin, dir)
    }

    fn aim(&self, origin: Vector3, dir: Vector3) -> Aim {
        let end = Vector3::new(
            origin.x + dir.x * REACH,
            origin.y + dir.y * REACH,
            origin.z + dir.z * REACH,
        );

        if self.mode == Mode::Brush {
            if let Some(hit) = self.brushes.trace(origin, end) {
                return aim_from_brush(&self.voxels, hit);
            }

            let z = self.anchor.map(|anchor| anchor.z).unwrap_or(0.0);

            if let Some(point) = ray_z(origin, dir, z) {
                return Aim {
                    point,
                    brush: None,
                    place: None,
                    erase: None,
                    surface: true,
                };
            }

            return Aim {
                point: origin,
                brush: None,
                place: None,
                erase: None,
                surface: false,
            };
        }

        let brush_hit = self.brushes.trace(origin, end);
        let voxel_hit = self.voxels.trace(origin, end);
        let brush_first = match (&brush_hit, &voxel_hit) {
            (Some(brush), Some(voxel)) => brush.distance <= voxel.distance,
            (Some(_), None) => true,
            _ => false,
        };

        if brush_first {
            return aim_from_brush(&self.voxels, brush_hit.expect("brush hit"));
        }

        if let Some(hit) = voxel_hit {
            return aim_from_voxel(hit);
        }

        if let Some(point) = ray_z(origin, dir, 0.0) {
            let cell = self.voxels.block_at(point);

            return Aim {
                point,
                brush: None,
                place: Some(cell),
                erase: Some(cell),
                surface: true,
            };
        }

        Aim {
            point: origin,
            brush: None,
            place: None,
            erase: None,
            surface: false,
        }
    }

    fn marked(&self, aim: &Aim) -> Option<usize> {
        if self.mode != Mode::Brush {
            return None;
        }

        self.selected.or(aim.brush)
    }

    fn fly(&mut self, keys: &HashSet<KeyCode>, faster: bool, dt: f32) {
        let forward = held_key(keys, KeyCode::KeyW) - held_key(keys, KeyCode::KeyS);
        let right = held_key(keys, KeyCode::KeyD) - held_key(keys, KeyCode::KeyA);
        let up = held_key(keys, KeyCode::Space) - held_key(keys, KeyCode::KeyC);
        let mut speed = 16.0 * self.voxels.scale() as f32;

        if faster {
            speed *= 3.0;
        }

        self.camera.fly(forward, right, up, dt, speed);
    }

    fn on_key(&mut self, code: KeyCode, repeat: bool, control: bool) {
        if control && code == KeyCode::KeyS {
            if !repeat {
                self.save();
            }

            return;
        }

        if repeat && !is_nudge(code) {
            return;
        }

        match code {
            KeyCode::Tab => self.toggle_mode(),
            KeyCode::KeyB => self.set_mode(Mode::Brush),
            KeyCode::KeyV => self.set_mode(Mode::Voxel),
            KeyCode::Digit1 => self.set_tool(1),
            KeyCode::Digit2 => self.set_tool(2),
            KeyCode::KeyG => self.cycle_grid(),
            KeyCode::KeyT => self.cycle_texture(),
            KeyCode::BracketLeft => self.bump_block(-1),
            KeyCode::BracketRight => self.bump_block(1),
            KeyCode::ArrowLeft => self.nudge(-self.grid, 0.0, 0.0),
            KeyCode::ArrowRight => self.nudge(self.grid, 0.0, 0.0),
            KeyCode::ArrowUp => self.nudge(0.0, self.grid, 0.0),
            KeyCode::ArrowDown => self.nudge(0.0, -self.grid, 0.0),
            KeyCode::KeyQ => self.nudge(0.0, 0.0, -self.grid),
            KeyCode::KeyE => self.nudge(0.0, 0.0, self.grid),
            _ => {}
        }
    }

    fn on_wheel(&mut self, steps: f64) {
        if steps.abs() < 0.01 {
            return;
        }

        let dir = if steps > 0.0 { 1.0 } else { -1.0 };

        if self.mode == Mode::Brush && self.brush_tool == BrushTool::Box && self.anchor.is_some() {
            self.lift += dir * self.grid;

            return;
        }

        if self.mode == Mode::Voxel {
            self.bump_block(dir as i32);
        }
    }

    fn brush_click(&mut self, aim: &Aim) {
        match self.brush_tool {
            BrushTool::Select => {
                self.selected = aim.brush;
            }
            BrushTool::Box => {
                if !aim.surface {
                    self.message = "aim at the grid or a brush".to_string();

                    return;
                }

                if let Some(anchor) = self.anchor {
                    let end = self.box_end(aim.point);
                    let (min, max) = box_from_corners(anchor, end, self.grid);

                    match self.document.add_box(min, max, TEXTURES[self.texture]) {
                        Some(index) => {
                            self.selected = Some(index);
                            self.anchor = None;
                            self.lift = 0.0;
                            self.dirty = true;
                            self.message.clear();
                            self.sync();
                        }
                        None => {
                            self.message = "that box is not a solid".to_string();
                        }
                    }
                } else {
                    self.anchor = Some(snap_point(aim.point, self.grid));
                    self.lift = 0.0;
                    self.message.clear();
                }
            }
        }
    }

    fn stroke(&mut self, aim: &Aim) {
        if self.mode != Mode::Voxel {
            return;
        }

        let cell = match self.voxel_tool {
            VoxelTool::Paint => aim.place,
            VoxelTool::Erase => aim.erase,
        };
        let Some(cell) = cell else {
            return;
        };

        if self.last_cell == Some(cell) {
            return;
        }

        self.last_cell = Some(cell);
        let block = match self.voxel_tool {
            VoxelTool::Paint => Block(self.block),
            VoxelTool::Erase => Block::AIR,
        };
        self.write_block(cell, block);
    }

    fn erase_at(&mut self, aim: &Aim) {
        let Some(cell) = aim.erase else {
            return;
        };

        self.write_block(cell, Block::AIR);
    }

    fn write_block(&mut self, cell: BlockPos, block: Block) {
        let revision = self.voxels.revision();
        self.voxels.set(cell, block);

        if self.voxels.revision() != revision {
            self.dirty = true;
        }
    }

    fn end_stroke(&mut self) {
        self.painting = false;
        self.last_cell = None;
    }

    fn delete_selection(&mut self) {
        if self.mode != Mode::Brush {
            return;
        }

        let Some(index) = self.selected else {
            return;
        };

        if !self.document.remove_brush(index) {
            return;
        }

        self.selected = None;
        self.dirty = true;
        self.sync();
    }

    fn nudge(&mut self, x: f64, y: f64, z: f64) {
        if self.mode != Mode::Brush || (x == 0.0 && y == 0.0 && z == 0.0) {
            return;
        }

        let Some(index) = self.selected else {
            return;
        };

        if !self.document.translate_brush(index, Vector3::new(x, y, z)) {
            return;
        }

        self.dirty = true;
        self.sync();
    }

    fn save(&mut self) {
        let compiled = match self.document.save_source(&self.map_path) {
            Ok(path) => path,
            Err(err) => {
                self.message = err;
                println!("[editor] {}", self.message);

                return;
            }
        };

        if let Err(err) = self.voxels.save_file(&self.voxel_path) {
            self.message = err;
            println!("[editor] {}", self.message);

            return;
        }

        self.dirty = false;
        self.message = format!(
            "saved {} and {}",
            self.map_path.display(),
            self.voxel_path.display()
        );
        println!("[editor] {} ({})", self.message, compiled.display());
    }

    fn sync(&mut self) {
        if let Err(err) = self.brushes.load_document(&self.document) {
            self.message = err;
        }
    }

    fn cancel(&mut self) {
        if self.anchor.is_some() || self.lift != 0.0 {
            self.anchor = None;
            self.lift = 0.0;

            return;
        }

        self.selected = None;
    }

    fn toggle_mode(&mut self) {
        let mode = match self.mode {
            Mode::Brush => Mode::Voxel,
            Mode::Voxel => Mode::Brush,
        };
        self.set_mode(mode);
    }

    fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.anchor = None;
        self.lift = 0.0;
        self.end_stroke();
    }

    fn set_tool(&mut self, slot: u8) {
        self.end_stroke();

        match self.mode {
            Mode::Brush => {
                self.brush_tool = if slot == 1 {
                    BrushTool::Select
                } else {
                    BrushTool::Box
                };

                if self.brush_tool != BrushTool::Box {
                    self.anchor = None;
                    self.lift = 0.0;
                }
            }
            Mode::Voxel => {
                self.voxel_tool = if slot == 1 {
                    VoxelTool::Paint
                } else {
                    VoxelTool::Erase
                };
            }
        }
    }

    fn cycle_grid(&mut self) {
        let mut idx = 0;

        while idx < GRIDS.len() {
            if (GRIDS[idx] - self.grid).abs() < 1e-6 {
                self.grid = GRIDS[(idx + 1) % GRIDS.len()];

                return;
            }

            idx += 1;
        }

        self.grid = 1.0;
    }

    fn cycle_texture(&mut self) {
        self.texture = (self.texture + 1) % TEXTURES.len();
    }

    fn bump_block(&mut self, dir: i32) {
        let mut next = self.block as i32 + dir;

        if next < 1 {
            next = 8;
        }

        if next > 8 {
            next = 1;
        }

        self.block = next as u16;
    }

    fn box_end(&self, point: Vector3) -> Vector3 {
        let mut end = snap_point(point, self.grid);
        end.z = snap(end.z + self.lift, self.grid);

        end
    }
}

fn aim_from_brush(voxels: &VoxelWorld, hit: BrushHit) -> Aim {
    let normal = hit.normal.unwrap_or(Vector3::new(0.0, 0.0, 1.0));

    Aim {
        point: hit.position,
        brush: Some(hit.brush),
        place: Some(cell_offset(voxels, hit.position, normal, 1.0)),
        erase: Some(cell_offset(voxels, hit.position, normal, -1.0)),
        surface: true,
    }
}

fn aim_from_voxel(hit: TraceHit) -> Aim {
    let place = match hit.face {
        Some(face) => Some(neighbor(hit.block, face)),
        None => None,
    };

    Aim {
        point: hit.position,
        brush: None,
        place,
        erase: Some(hit.block),
        surface: true,
    }
}

fn cell_offset(voxels: &VoxelWorld, position: Vector3, normal: Vector3, sign: f64) -> BlockPos {
    let eps = voxels.scale() * 0.05;

    voxels.block_at(Vector3::new(
        position.x + normal.x * eps * sign,
        position.y + normal.y * eps * sign,
        position.z + normal.z * eps * sign,
    ))
}

fn overlay_key(editor: &Editor, aim: &Aim) -> String {
    let mut key = format!(
        "{:?}:{:?}:{:?}:{}",
        editor.mode, editor.brush_tool, editor.voxel_tool, editor.grid
    );

    if editor.mode == Mode::Brush && editor.brush_tool == BrushTool::Box {
        key.push_str(&format!(":{}", aim.surface));

        if aim.surface {
            let point = if editor.anchor.is_some() {
                editor.box_end(aim.point)
            } else {
                snap_point(aim.point, editor.grid)
            };
            key.push_str(&format!(":{}:{}", point_key(point), editor.lift));
        } else if let Some(anchor) = editor.anchor {
            key.push_str(&format!(":anchor:{}", point_key(anchor)));
        }
    }

    if editor.mode == Mode::Voxel {
        let cell = match editor.voxel_tool {
            VoxelTool::Paint => aim.place,
            VoxelTool::Erase => aim.erase,
        };
        key.push_str(&cell_key(cell));
    }

    key
}

fn push_overlay(vertices: &mut Vec<f32>, editor: &Editor, aim: &Aim) {
    push_grid(vertices, editor.grid as f32, 48.0);
    push_axes(vertices);

    if editor.mode == Mode::Brush && editor.brush_tool == BrushTool::Box {
        if let Some(anchor) = editor.anchor {
            if aim.surface {
                let end = editor.box_end(aim.point);
                let (min, max) = box_from_corners(anchor, end, editor.grid);
                push_box(vertices, min, max, [0.35, 0.9, 1.0]);
            } else {
                push_marker(vertices, anchor, editor.grid, [0.35, 0.9, 1.0]);
            }
        } else if aim.surface {
            push_marker(
                vertices,
                snap_point(aim.point, editor.grid),
                editor.grid,
                [0.35, 0.9, 1.0],
            );
        }
    }

    if editor.mode == Mode::Voxel {
        let cell = match editor.voxel_tool {
            VoxelTool::Paint => aim.place,
            VoxelTool::Erase => aim.erase,
        };

        if let Some(cell) = cell {
            let (min, max) = block_bounds(&editor.voxels, cell, editor.voxels.scale() * 0.03);
            let color = match editor.voxel_tool {
                VoxelTool::Paint => [0.4, 0.95, 0.55],
                VoxelTool::Erase => [1.0, 0.4, 0.35],
            };
            push_box(vertices, min, max, color);
        }
    }
}

fn hud_lines(editor: &Editor, aim: &Aim) -> Vec<String> {
    let mode = match editor.mode {
        Mode::Brush => "brush",
        Mode::Voxel => "voxel",
    };
    let tool = match editor.mode {
        Mode::Brush => match editor.brush_tool {
            BrushTool::Select => "select",
            BrushTool::Box => "box",
        },
        Mode::Voxel => match editor.voxel_tool {
            VoxelTool::Paint => "paint",
            VoxelTool::Erase => "erase",
        },
    };
    let dirty = if editor.dirty { " *" } else { "" };
    let corner = if editor.anchor.is_some() {
        format!("  lift {}", editor.lift)
    } else {
        String::new()
    };
    let selected = match editor.selected {
        Some(index) => format!("selected {index}"),
        None => "selected none".to_string(),
    };
    let mut lines = vec![
        format!("{mode}  {tool}{dirty}{corner}"),
        format!(
            "{}   {}   brushes {}   chunks {}",
            file_label(&editor.map_path),
            file_label(&editor.voxel_path),
            editor.document.brush_count(),
            editor.voxels.chunk_count(),
        ),
        format!(
            "grid {}   block {}   texture {}   {}   {}",
            editor.grid,
            editor.block,
            TEXTURES[editor.texture],
            selected,
            aim_label(editor, aim),
        ),
        "RMB look   WASD fly   Space/C vertical   Shift fast   LMB use   Wheel height".to_string(),
        "Tab/B/V mode   1/2 tool   Del   Arrows/QE nudge   G grid   [ ] block   T texture   Ctrl/Cmd+S save".to_string(),
    ];

    if !editor.message.is_empty() {
        lines.push(editor.message.clone());
    }

    lines
}

fn aim_label(editor: &Editor, aim: &Aim) -> String {
    match editor.mode {
        Mode::Brush => match aim.brush {
            Some(index) => format!("hover {index}"),
            None => "hover none".to_string(),
        },
        Mode::Voxel => {
            let cell = match editor.voxel_tool {
                VoxelTool::Paint => aim.place,
                VoxelTool::Erase => aim.erase,
            };

            match cell {
                Some(pos) => format!("cell {} {} {}", pos.x, pos.y, pos.z),
                None => "cell none".to_string(),
            }
        }
    }
}

fn draw_hud(window: &mut impl Window, lines: &[String]) {
    let height = 16.0 + lines.len() as f32 * 20.0;
    window.draw_rectangle(
        8.0,
        8.0,
        1180.0,
        height,
        Color::ColorRGBA {
            r: 8,
            g: 10,
            b: 14,
            a: 188,
        },
    );
    let mut y = 14.0;

    for line in lines {
        window.draw_text(
            "default",
            line,
            16.0,
            y,
            16.0,
            Color::ColorRGBA {
                r: 236,
                g: 238,
                b: 242,
                a: 255,
            },
        );
        y += 20.0;
    }
}

fn frame_view(camera: &mut FlyCamera, mesh: &[f32]) {
    if mesh.len() < 6 {
        return;
    }

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut idx = 0;

    while idx + 5 < mesh.len() {
        min[0] = min[0].min(mesh[idx]);
        min[1] = min[1].min(mesh[idx + 1]);
        min[2] = min[2].min(mesh[idx + 2]);
        max[0] = max[0].max(mesh[idx]);
        max[1] = max[1].max(mesh[idx + 1]);
        max[2] = max[2].max(mesh[idx + 2]);
        idx += 6;
    }

    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let dx = max[0] - min[0];
    let dy = max[1] - min[1];
    let dz = max[2] - min[2];
    let radius = (dx * dx + dy * dy + dz * dz).sqrt().max(4.0);
    camera.x = center[0];
    camera.y = center[1] - radius * 0.9 - 6.0;
    camera.z = center[2] + radius * 0.35 + 3.0;
    camera.yaw = std::f32::consts::FRAC_PI_2;
    camera.pitch = -0.4;
}

fn cursor_ray(
    camera: &FlyCamera,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scale: f32,
) -> (Vector3, Vector3) {
    let view = camera.scene(width / height.max(1.0), scale);
    let ndc_x = (x / width.max(1.0)) * 2.0 - 1.0;
    let ndc_y = 1.0 - (y / height.max(1.0)) * 2.0;
    let forward = normalize(view.forward);
    let zaxis = [-forward[0], -forward[1], -forward[2]];
    let xaxis = normalize(cross(view.up, zaxis));
    let yaxis = cross(zaxis, xaxis);
    let tan_half = (view.fov_y * 0.5).tan();
    let dx = ndc_x * view.aspect * tan_half;
    let dy = ndc_y * tan_half;
    let mut dir = [
        xaxis[0] * dx + yaxis[0] * dy + forward[0],
        xaxis[1] * dx + yaxis[1] * dy + forward[1],
        xaxis[2] * dx + yaxis[2] * dy + forward[2],
    ];

    if dot(dir, dir) <= 1e-8 {
        dir = forward;
    } else {
        dir = normalize(dir);
    }

    (
        Vector3::new(view.eye[0] as f64, view.eye[1] as f64, view.eye[2] as f64),
        Vector3::new(dir[0] as f64, dir[1] as f64, dir[2] as f64),
    )
}

fn ray_z(origin: Vector3, dir: Vector3, z: f64) -> Option<Vector3> {
    if dir.z.abs() < 1e-8 {
        return None;
    }

    let t = (z - origin.z) / dir.z;

    if t < 0.0 || t > REACH {
        return None;
    }

    Some(Vector3::new(origin.x + dir.x * t, origin.y + dir.y * t, z))
}

fn box_from_corners(a: Vector3, b: Vector3, grid: f64) -> (Vector3, Vector3) {
    let min = Vector3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
    let mut max = Vector3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
    let span = if grid > 0.0 { grid } else { 1.0 };

    if max.x - min.x < span * 0.5 {
        max.x = min.x + span;
    }

    if max.y - min.y < span * 0.5 {
        max.y = min.y + span;
    }

    if max.z - min.z < span * 0.5 {
        max.z = min.z + span;
    }

    (min, max)
}

fn snap(value: f64, grid: f64) -> f64 {
    if grid <= 0.0 {
        return value;
    }

    (value / grid).round() * grid
}

fn snap_point(point: Vector3, grid: f64) -> Vector3 {
    Vector3::new(
        snap(point.x, grid),
        snap(point.y, grid),
        snap(point.z, grid),
    )
}

fn neighbor(pos: BlockPos, face: Face) -> BlockPos {
    match face {
        Face::NegX => BlockPos::new(pos.x - 1, pos.y, pos.z),
        Face::PosX => BlockPos::new(pos.x + 1, pos.y, pos.z),
        Face::NegY => BlockPos::new(pos.x, pos.y - 1, pos.z),
        Face::PosY => BlockPos::new(pos.x, pos.y + 1, pos.z),
        Face::NegZ => BlockPos::new(pos.x, pos.y, pos.z - 1),
        Face::PosZ => BlockPos::new(pos.x, pos.y, pos.z + 1),
    }
}

fn block_bounds(world: &VoxelWorld, pos: BlockPos, pad: f64) -> (Vector3, Vector3) {
    let scale = world.scale();
    let min = Vector3::new(
        pos.x as f64 * scale - pad,
        pos.y as f64 * scale - pad,
        pos.z as f64 * scale - pad,
    );
    let max = Vector3::new(
        (pos.x as f64 + 1.0) * scale + pad,
        (pos.y as f64 + 1.0) * scale + pad,
        (pos.z as f64 + 1.0) * scale + pad,
    );

    (min, max)
}

fn push_marker(vertices: &mut Vec<f32>, point: Vector3, grid: f64, color: [f32; 3]) {
    let s = (grid * 0.12).max(0.04);
    push_box(
        vertices,
        Vector3::new(point.x - s, point.y - s, point.z - s),
        Vector3::new(point.x + s, point.y + s, point.z + s),
        color,
    );
}

fn push_axes(vertices: &mut Vec<f32>) {
    push_box(
        vertices,
        Vector3::new(0.0, -0.04, -0.04),
        Vector3::new(4.0, 0.04, 0.04),
        [0.9, 0.25, 0.25],
    );
    push_box(
        vertices,
        Vector3::new(-0.04, 0.0, -0.04),
        Vector3::new(0.04, 4.0, 0.04),
        [0.25, 0.85, 0.35],
    );
    push_box(
        vertices,
        Vector3::new(-0.04, -0.04, 0.0),
        Vector3::new(0.04, 0.04, 4.0),
        [0.3, 0.55, 1.0],
    );
}

fn push_grid(vertices: &mut Vec<f32>, step: f32, extent: f32) {
    if step <= 0.0 {
        return;
    }

    let color = [0.58, 0.62, 0.68];
    let mut cursor = -extent;

    while cursor <= extent + step * 0.25 {
        push_ribbon(
            vertices,
            [-extent, cursor, -0.03],
            [extent, cursor, -0.03],
            0.02,
            color,
        );
        push_ribbon(
            vertices,
            [cursor, -extent, -0.03],
            [cursor, extent, -0.03],
            0.02,
            color,
        );
        cursor += step;
    }
}

fn push_ribbon(
    vertices: &mut Vec<f32>,
    a: [f32; 3],
    b: [f32; 3],
    half_width: f32,
    color: [f32; 3],
) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();

    if len < 1e-6 {
        return;
    }

    let sx = -dy / len * half_width;
    let sy = dx / len * half_width;
    let p0 = [a[0] - sx, a[1] - sy, a[2]];
    let p1 = [b[0] - sx, b[1] - sy, b[2]];
    let p2 = [b[0] + sx, b[1] + sy, b[2]];
    let p3 = [a[0] + sx, a[1] + sy, a[2]];
    push_tri(vertices, p0, p1, p2, color);
    push_tri(vertices, p0, p2, p3, color);
}

fn push_box(vertices: &mut Vec<f32>, min: Vector3, max: Vector3, color: [f32; 3]) {
    let span = [max.x - min.x, max.y - min.y, max.z - min.z];

    for face in 0..6 {
        let quad = QUADS[face];
        let shade = SHADES[face];
        let tint = [color[0] * shade, color[1] * shade, color[2] * shade];
        let mut corners = [[0.0f32; 3]; 4];

        for corner in 0..4 {
            corners[corner] = [
                (min.x + quad[corner].0 as f64 * span[0]) as f32,
                (min.y + quad[corner].1 as f64 * span[1]) as f32,
                (min.z + quad[corner].2 as f64 * span[2]) as f32,
            ];
        }

        push_tri(vertices, corners[0], corners[1], corners[2], tint);
        push_tri(vertices, corners[0], corners[2], corners[3], tint);
    }
}

fn push_tri(vertices: &mut Vec<f32>, a: [f32; 3], b: [f32; 3], c: [f32; 3], color: [f32; 3]) {
    push_vert(vertices, a, color);
    push_vert(vertices, b, color);
    push_vert(vertices, c, color);
}

fn push_vert(vertices: &mut Vec<f32>, position: [f32; 3], color: [f32; 3]) {
    vertices.push(position[0]);
    vertices.push(position[1]);
    vertices.push(position[2]);
    vertices.push(color[0]);
    vertices.push(color[1]);
    vertices.push(color[2]);
}

fn point_key(point: Vector3) -> String {
    format!("{:.4},{:.4},{:.4}", point.x, point.y, point.z)
}

fn cell_key(cell: Option<BlockPos>) -> String {
    match cell {
        Some(pos) => format!(":{}:{}:{}", pos.x, pos.y, pos.z),
        None => ":none".to_string(),
    }
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|text| text.to_str())
        .unwrap_or("map")
        .to_string()
}

fn map_stem(name: &str) -> &str {
    let file = Path::new(name)
        .file_name()
        .and_then(|file| file.to_str())
        .unwrap_or(name);
    let stem = file
        .strip_suffix(".vmap")
        .or_else(|| file.strip_suffix(".map"))
        .or_else(|| file.strip_suffix(".cmap"))
        .unwrap_or(file);

    if stem.is_empty() {
        "untitled"
    } else {
        stem
    }
}

fn is_voxel_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("vmap"))
}

fn is_nudge(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::ArrowLeft
            | KeyCode::ArrowRight
            | KeyCode::ArrowUp
            | KeyCode::ArrowDown
            | KeyCode::KeyQ
            | KeyCode::KeyE
    )
}

fn wheel_steps(delta: MouseScrollDelta) -> f64 {
    match delta {
        MouseScrollDelta::LineDelta(_, y) => y as f64,
        MouseScrollDelta::PixelDelta(offset) => offset.y / 48.0,
    }
}

fn held_key(keys: &HashSet<KeyCode>, code: KeyCode) -> f32 {
    if keys.contains(&code) {
        1.0
    } else {
        0.0
    }
}

fn set_capture(window: &winit::window::Window, captured: bool) {
    if captured {
        if window.set_cursor_grab(CursorGrabMode::Locked).is_err() {
            let _ = window.set_cursor_grab(CursorGrabMode::Confined);
        }

        window.set_cursor_visible(false);

        return;
    }

    let _ = window.set_cursor_grab(CursorGrabMode::None);
    window.set_cursor_visible(true);
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();

    if len <= 0.0 {
        return [0.0, 0.0, 0.0];
    }

    [v[0] / len, v[1] / len, v[2] / len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hall_opens_without_a_window() {
        let editor = open_editor("hall").unwrap();

        assert_eq!(editor.document.brush_count(), 8);
        assert!(editor.map_path.ends_with("hall.map"));
        assert!(editor.voxel_path.ends_with("hall.vmap"));
        assert_eq!(editor.mode, Mode::Brush);
    }

    #[test]
    fn flat_corners_gain_one_grid_of_thickness() {
        let (min, max) = box_from_corners(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 4.0, 0.0),
            1.0,
        );

        assert_eq!(min.x, 0.0);
        assert_eq!(max.x, 1.0);
        assert_eq!(min.y, 0.0);
        assert_eq!(max.y, 4.0);
        assert_eq!(min.z, 0.0);
        assert_eq!(max.z, 1.0);
    }

    #[test]
    fn neighbor_steps_out_of_the_hit_face() {
        let pos = BlockPos::new(1, 2, 3);

        assert_eq!(neighbor(pos, Face::NegX), BlockPos::new(0, 2, 3));
        assert_eq!(neighbor(pos, Face::PosZ), BlockPos::new(1, 2, 4));
    }

    #[test]
    fn cursor_ray_matches_the_scene_projection() {
        let camera = FlyCamera::new();
        let width = 800.0;
        let height = 600.0;
        let x = 620.0;
        let y = 180.0;
        let (origin, dir) = cursor_ray(&camera, x, y, width, height, 1.0);
        let view = camera.scene(width / height, 1.0);
        let view_proj = crate::ui::d3d::math::view_proj(&view);
        let point = [
            origin.x as f32 + dir.x as f32 * 10.0,
            origin.y as f32 + dir.y as f32 * 10.0,
            origin.z as f32 + dir.z as f32 * 10.0,
        ];
        let clip = project(&view_proj, point);
        let ndc_x = clip[0] / clip[3];
        let ndc_y = clip[1] / clip[3];
        let want_x = (x / width) * 2.0 - 1.0;
        let want_y = 1.0 - (y / height) * 2.0;

        assert!((ndc_x - want_x).abs() < 1e-3);
        assert!((ndc_y - want_y).abs() < 1e-3);
    }

    fn project(view_proj: &[f32; 16], point: [f32; 3]) -> [f32; 4] {
        let mut out = [0.0; 4];
        let mut row = 0;

        while row < 4 {
            out[row] = view_proj[row] * point[0]
                + view_proj[4 + row] * point[1]
                + view_proj[8 + row] * point[2]
                + view_proj[12 + row];
            row += 1;
        }

        out
    }
}
