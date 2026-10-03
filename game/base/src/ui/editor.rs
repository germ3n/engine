use crate::anchor::Anchor;
use crate::platform::{
    DeviceEvent, ElementState, Event, HostKind, KeyCode, Modifiers, MouseButton, MouseScrollDelta,
    PlatformHost, WindowEvent,
};
use crate::script::libs::vector3::Vector3;
use crate::ui::backend;
use crate::ui::gui::Gui;
use crate::ui::voxel::FlyCamera;
use crate::ui::window::Window;
use crate::world::{
    block_rgb, find_voxel_file, texture_name_ok, Block, BlockPos, BrushHit, BrushMap,
    CompiledEntity, CompiledMap, Face, TraceHit, VoxelWorld,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

const TEXTURES: [&str; 7] = ["solid", "floor", "ceiling", "side", "crate", "ramp", "end"];
const GRIDS: [f64; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
const BLOCKS: [(u16, &str); 9] = [
    (1, "Stone"),
    (2, "Dirt"),
    (3, "Grass"),
    (4, "Sand"),
    (5, "Sandstone"),
    (6, "Snow"),
    (7, "Water"),
    (8, "Log"),
    (9, "Leaves"),
];
const REACH: f64 = 8000.0;
const HISTORY_LIMIT: usize = 256;
const MERGE_SECONDS: f32 = 0.8;
const NOTICE_SECONDS: f32 = 6.0;
const MAX_VOXEL_SIZE: i32 = 9;
const BOX_COLOR: [f32; 3] = [0.35, 0.9, 1.0];
const PAINT_COLOR: [f32; 3] = [0.4, 0.95, 0.55];
const ERASE_COLOR: [f32; 3] = [1.0, 0.4, 0.35];

const ACCENT: egui::Color32 = egui::Color32::from_rgb(77, 156, 255);
const MUTED: egui::Color32 = egui::Color32::from_rgb(140, 148, 160);
const WARN: egui::Color32 = egui::Color32::from_rgb(255, 196, 92);
const DANGER: egui::Color32 = egui::Color32::from_rgb(255, 110, 100);
const GOOD: egui::Color32 = egui::Color32::from_rgb(110, 220, 140);

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Confirm {
    Quit,
    Revert,
}

struct Aim {
    point: Vector3,
    brush: Option<usize>,
    place: Option<BlockPos>,
    erase: Option<BlockPos>,
    surface: bool,
}

enum Change {
    Document {
        document: CompiledMap,
        selected: Option<usize>,
    },
    Voxels(Vec<(BlockPos, Block, Block)>),
}

struct Step {
    label: String,
    change: Change,
    key: Option<(&'static str, usize)>,
    at: Instant,
}

#[derive(Default)]
struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

struct View {
    grid: bool,
    axes: bool,
    outliner: bool,
    inspector: bool,
    help: bool,
    speed: f32,
    zoom: f32,
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
    texture: String,
    selected: Option<usize>,
    anchor: Option<Vector3>,
    lift: f64,
    dirty: bool,
    message: String,
    message_at: Instant,
    error: bool,
    cursor_x: f32,
    cursor_y: f32,
    painting: bool,
    last_cell: Option<BlockPos>,
    voxel_size: i32,
    history: History,
    stroke: Vec<(BlockPos, Block, Block)>,
    stroke_cells: HashMap<BlockPos, usize>,
    view: View,
    hover: Option<usize>,
    filter: String,
    custom: String,
    palette: (u64, Vec<String>),
    confirm: Option<Confirm>,
    quit: bool,
    fps: f32,
}

pub fn run(map_name: &str) {
    let mut editor = match open_editor(map_name) {
        Ok(editor) => editor,
        Err(err) => {
            log::warn!("[editor] {err}");

            return;
        }
    };

    if editor.map_path.exists() {
        log::info!("[editor] {}", editor.map_path.display());
    } else {
        log::info!("[editor] new {}", editor.map_path.display());
    }

    if editor.voxel_path.exists() {
        log::info!("[editor] {}", editor.voxel_path.display());
    }

    let kind = HostKind::from_env();
    log::info!("[host] {kind:?}");
    let mut host = match PlatformHost::open(kind) {
        Ok(host) => host,
        Err(err) => {
            log::warn!("[host] {err}");

            return;
        }
    };
    host.set_title(&editor.title());
    host.set_size(1600, 900);
    let surface = host.surface().expect("host surface");
    let mut gui = Gui::new(surface.scale_factor as f32);
    let mut window = backend::create_tool(surface);
    style(gui.context());
    let mut keys = HashSet::new();
    let mut modifiers = Modifiers::default();
    let mut looking = false;
    let mut last_frame = Instant::now();
    let mut shown_title = editor.title();
    let mut zoom = 1.0f32;
    let mut solid: Vec<f32> = Vec::new();
    let mut solid_ready = false;
    let mut solid_brush = 0u64;
    let mut solid_voxel = 0u64;
    let mut solid_mark: Option<usize> = None;
    let mut draw_anchor = Anchor::ZERO;
    let mut picture: Vec<f32> = Vec::new();
    let mut picture_key = String::new();
    let mut picture_revision = 0u64;

    host.run(move |event, host, control| {
        control.poll();

        match event {
            Event::Window(event) => {
                let forward = match &event {
                    WindowEvent::CursorMoved { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::TextInput { .. } => !looking,
                    WindowEvent::KeyboardInput(input) => {
                        !looking || input.state == ElementState::Released
                    }
                    _ => true,
                };

                if forward {
                    gui.on_event(&event);
                }

                match event {
                    WindowEvent::CloseRequested => {
                        editor.request(Confirm::Quit);
                    }
                    WindowEvent::Resized { width, height } => {
                        window.set_size(width, height);
                    }
                    WindowEvent::Focused(false) => {
                        keys.clear();
                        looking = false;
                        editor.end_stroke();
                        host.set_cursor_grabbed(false);
                    }
                    WindowEvent::ModifiersChanged(next) => {
                        modifiers = next;
                    }
                    WindowEvent::CursorMoved { x, y } => {
                        editor.cursor_x = x as f32;
                        editor.cursor_y = y as f32;

                        if editor.painting && !looking {
                            let (width, height) = host.size();
                            let aim = editor.pointer_aim(width, height);
                            editor.stroke(&aim);
                        }
                    }
                    WindowEvent::MouseWheel { delta } => {
                        if looking {
                            editor.adjust_speed(wheel_steps(delta));
                        } else if !gui.wants_pointer() {
                            editor.on_wheel(wheel_steps(delta));
                        }
                    }
                    WindowEvent::MouseInput { state, button } => {
                        let pressed = state == ElementState::Pressed;

                        if button == MouseButton::Right {
                            if !pressed || !gui.wants_pointer() {
                                looking = pressed;
                                editor.end_stroke();
                                host.set_cursor_grabbed(looking);
                            }
                        } else if button == MouseButton::Left && !pressed {
                            editor.end_stroke();
                        } else if button == MouseButton::Left
                            && pressed
                            && !looking
                            && !gui.wants_pointer()
                        {
                            let (width, height) = host.size();
                            let aim = editor.pointer_aim(width, height);

                            if editor.mode == Mode::Voxel {
                                editor.painting = true;
                                editor.stroke(&aim);
                            } else {
                                editor.brush_click(&aim);
                            }
                        }
                    }
                    WindowEvent::KeyboardInput(input) => {
                        if let Some(code) = input.key_code {
                            let pressed = input.state == ElementState::Pressed;
                            let typing = gui.wants_keyboard() && !looking;

                            if !pressed {
                                keys.remove(&code);
                            } else if typing {
                                keys.clear();
                            } else {
                                keys.insert(code);
                            }

                            if pressed && !typing {
                                if code == KeyCode::Escape {
                                    looking = false;
                                    editor.end_stroke();
                                    host.set_cursor_grabbed(false);
                                    editor.cancel();
                                } else if matches!(code, KeyCode::Delete | KeyCode::Backspace) {
                                    if !input.repeat {
                                        let (width, height) = host.size();
                                        let aim = editor.pointer_aim(width, height);
                                        editor.delete_at(&aim);
                                    }
                                } else {
                                    let command = modifiers.control_key() || modifiers.super_key();
                                    editor.on_key(code, input.repeat, command, modifiers.shift_key());
                                }
                            }
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        let (width, height) = host.size();

                        if let Some(surface) = host.surface() {
                            gui.set_scale(surface.scale_factor as f32);
                        }

                        let over_ui = gui.wants_pointer() && !looking;
                        let aim = if over_ui {
                            editor.idle_aim()
                        } else {
                            editor.pointer_aim(width, height)
                        };
                        let output = gui.run(width, height, |ctx| draw_ui(ctx, &mut editor, &aim));

                        if (editor.view.zoom - zoom).abs() > f32::EPSILON {
                            zoom = editor.view.zoom;
                            gui.context().set_zoom_factor(zoom);
                        }

                        let aim = if over_ui {
                            editor.idle_aim()
                        } else {
                            editor.pointer_aim(width, height)
                        };
                        let marked = editor.marked(&aim);
                        let camera_moved =
                            draw_anchor.drifted(editor.camera.x, editor.camera.y, editor.camera.z);

                        if camera_moved {
                            draw_anchor =
                                Anchor::new(editor.camera.x, editor.camera.y, editor.camera.z);
                        }

                        if !solid_ready
                            || solid_brush != editor.brushes.revision()
                            || solid_voxel != editor.voxels.revision()
                            || solid_mark != marked
                            || camera_moved
                        {
                            let origin = draw_anchor.to_vec();
                            solid = editor.voxels.mesh_at(origin);

                            match marked {
                                Some(index) => {
                                    solid.extend(editor.brushes.mesh_highlight_at(index, origin))
                                }
                                None => solid.extend(editor.brushes.mesh_at(origin)),
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
                            push_overlay(&mut picture, &editor, &aim, draw_anchor);
                            picture_key = key;
                            picture_revision = picture_revision.wrapping_add(1);
                        }

                        let aspect = width as f32 / height.max(1) as f32;
                        let view =
                            editor
                                .camera
                                .scene_at(aspect, editor.voxels.scale() as f32, draw_anchor);
                        let title = editor.title();

                        if title != shown_title {
                            host.set_title(&title);
                            shown_title = title;
                        }

                        window.begin_frame(0.46, 0.62, 0.74);
                        let count = (picture.len() / crate::world::STRIDE) as u32;
                        let ranges = [crate::world::SurfaceRange {
                            first: 0,
                            count,
                            material: crate::world::MATERIAL_NONE,
                            cubemap: crate::world::CUBEMAP_NONE,
                            pass: crate::world::PASS_OPAQUE,
                        }];
                        let graphics = crate::world::MapGraphics::plain();
                        window.draw_colored_mesh(
                            &picture,
                            &ranges,
                            &graphics,
                            picture_revision,
                            &view,
                        );
                        gui.paint(&mut window, output, width, height);
                        window.present();
                    }
                    _ => {}
                }
            }
            Event::Device(DeviceEvent::MouseMotion { delta }) => {
                if looking {
                    editor.camera.look(delta.0 as f32, delta.1 as f32);
                }
            }
            Event::AboutToWait => {
                if editor.quit {
                    control.exit();

                    return;
                }

                let now = Instant::now();
                let dt = now.duration_since(last_frame).as_secs_f32().min(0.1);
                last_frame = now;

                if dt > 0.0 {
                    editor.fps = editor.fps * 0.92 + (1.0 / dt) * 0.08;
                }

                let command = modifiers.control_key() || modifiers.super_key();

                if !command {
                    editor.fly(&keys, modifiers.shift_key(), dt);
                }

                host.request_redraw();
            }
            _ => {}
        }
    });
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
        texture: TEXTURES[0].to_string(),
        selected: None,
        anchor: None,
        lift: 0.0,
        dirty: false,
        message,
        message_at: Instant::now(),
        error: false,
        cursor_x: 800.0,
        cursor_y: 450.0,
        painting: false,
        last_cell: None,
        voxel_size: 1,
        history: History::default(),
        stroke: Vec::new(),
        stroke_cells: HashMap::new(),
        view: View {
            grid: true,
            axes: true,
            outliner: true,
            inspector: true,
            help: false,
            speed: 1.0,
            zoom: 1.0,
        },
        hover: None,
        filter: String::new(),
        custom: String::new(),
        palette: (u64::MAX, Vec::new()),
        confirm: None,
        quit: false,
        fps: 60.0,
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

    fn notify(&mut self, text: impl Into<String>) {
        self.message = text.into();
        self.message_at = Instant::now();
        self.error = false;
    }

    fn warn(&mut self, text: impl Into<String>) {
        self.message = text.into();
        self.message_at = Instant::now();
        self.error = true;
        log::info!("[editor] {}", self.message);
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

    fn idle_aim(&self) -> Aim {
        Aim {
            point: Vector3::new(self.camera.x, self.camera.y, self.camera.z),
            brush: None,
            place: None,
            erase: None,
            surface: false,
        }
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

        self.hover.or(self.selected).or(aim.brush)
    }

    fn fly(&mut self, keys: &HashSet<KeyCode>, faster: bool, dt: f32) {
        let forward = held_key(keys, KeyCode::KeyW) - held_key(keys, KeyCode::KeyS);
        let right = held_key(keys, KeyCode::KeyD) - held_key(keys, KeyCode::KeyA);
        let up = held_key(keys, KeyCode::Space) - held_key(keys, KeyCode::KeyC);
        let mut speed = 16.0 * self.voxels.scale() as f32 * self.view.speed;

        if faster {
            speed *= 3.0;
        }

        self.camera.fly(forward, right, up, dt, speed);
    }

    fn adjust_speed(&mut self, steps: f64) {
        if steps.abs() < 0.01 {
            return;
        }

        let factor = 1.2f32.powf(steps.signum() as f32);
        self.view.speed = (self.view.speed * factor).clamp(0.1, 10.0);
        self.notify(format!("fly speed {:.2}x", self.view.speed));
    }

    fn on_key(&mut self, code: KeyCode, repeat: bool, command: bool, shift: bool) {
        if command {
            match code {
                KeyCode::KeyS if !repeat => self.save(),
                KeyCode::KeyZ if shift => self.redo(),
                KeyCode::KeyZ => self.undo(),
                KeyCode::KeyY => self.redo(),
                KeyCode::KeyD if !repeat => self.duplicate_selection(),
                _ => {}
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
            KeyCode::KeyF => self.focus_selection(),
            KeyCode::F1 => self.view.help = !self.view.help,
            KeyCode::BracketLeft => self.bump_block(-1),
            KeyCode::BracketRight => self.bump_block(1),
            KeyCode::Minus => self.bump_size(-1),
            KeyCode::Equal => self.bump_size(1),
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
                    self.warn("aim at the grid or a brush");

                    return;
                }

                if let Some(anchor) = self.anchor {
                    let end = self.box_end(aim.point);
                    let (min, max) = box_from_corners(anchor, end, self.grid);
                    let texture = self.texture.clone();
                    let mut created = None;
                    let added = self.edit("add brush", None, |document| {
                        created = document.add_box(min, max, &texture);

                        created.is_some()
                    });

                    if added {
                        self.selected = created;
                        self.anchor = None;
                        self.lift = 0.0;
                        self.notify(format!(
                            "added brush {} ({})",
                            created.unwrap_or(0),
                            size_label(min, max)
                        ));
                    } else {
                        self.warn("that box is not a solid");
                    }
                } else {
                    self.anchor = Some(snap_point(aim.point, self.grid));
                    self.lift = 0.0;
                }
            }
        }
    }

    fn select(&mut self, index: usize) {
        if self.mode != Mode::Brush {
            self.set_mode(Mode::Brush);
        }

        self.selected = Some(index);
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
        self.fill_cells(cell, block);
    }

    fn fill_cells(&mut self, center: BlockPos, block: Block) {
        let (low, high) = brush_cells(center, self.voxel_size);
        let mut x = low.x;

        while x <= high.x {
            let mut y = low.y;

            while y <= high.y {
                let mut z = low.z;

                while z <= high.z {
                    self.write_block(BlockPos::new(x, y, z), block);
                    z += 1;
                }

                y += 1;
            }

            x += 1;
        }
    }

    fn delete_at(&mut self, aim: &Aim) {
        if self.mode == Mode::Voxel {
            if let Some(cell) = aim.erase {
                self.fill_cells(cell, Block::AIR);
                self.end_stroke();
            }

            return;
        }

        self.delete_selection();
    }

    fn write_block(&mut self, cell: BlockPos, block: Block) {
        let old = self.voxels.get(cell);

        if old == block {
            return;
        }

        let revision = self.voxels.revision();
        self.voxels.set(cell, block);

        if self.voxels.revision() == revision {
            return;
        }

        self.dirty = true;

        match self.stroke_cells.get(&cell) {
            Some(&slot) => self.stroke[slot].2 = block,
            None => {
                self.stroke_cells.insert(cell, self.stroke.len());
                self.stroke.push((cell, old, block));
            }
        }
    }

    fn end_stroke(&mut self) {
        self.painting = false;
        self.last_cell = None;
        self.stroke_cells.clear();

        if self.stroke.is_empty() {
            return;
        }

        let cells = std::mem::take(&mut self.stroke);
        let verb = if cells.iter().all(|cell| cell.2.is_air()) {
            "erase"
        } else {
            "paint"
        };
        let label = if cells.len() == 1 {
            format!("{verb} voxel")
        } else {
            format!("{verb} {} voxels", cells.len())
        };
        self.record(label, Change::Voxels(cells), None);
    }

    fn edit(
        &mut self,
        label: &str,
        key: Option<(&'static str, usize)>,
        apply: impl FnOnce(&mut CompiledMap) -> bool,
    ) -> bool {
        let merge = key.is_some()
            && self.history.undo.last().is_some_and(|step| {
                step.key == key && step.at.elapsed().as_secs_f32() < MERGE_SECONDS
            });
        let before = if merge {
            None
        } else {
            Some((self.document.clone(), self.selected))
        };

        if !apply(&mut self.document) {
            return false;
        }

        match before {
            Some((document, selected)) => {
                self.record(
                    label.to_string(),
                    Change::Document { document, selected },
                    key,
                );
            }
            None => {
                if let Some(step) = self.history.undo.last_mut() {
                    step.at = Instant::now();
                }

                self.history.redo.clear();
            }
        }

        self.dirty = true;
        self.sync();

        true
    }

    fn record(&mut self, label: String, change: Change, key: Option<(&'static str, usize)>) {
        self.history.undo.push(Step {
            label,
            change,
            key,
            at: Instant::now(),
        });
        self.history.redo.clear();

        if self.history.undo.len() > HISTORY_LIMIT {
            self.history.undo.remove(0);
        }
    }

    fn undo(&mut self) {
        self.end_stroke();
        let Some(mut step) = self.history.undo.pop() else {
            self.notify("nothing to undo");

            return;
        };

        self.swap_step(&mut step.change, true);
        self.notify(format!("undid {}", step.label));
        step.key = None;
        self.history.redo.push(step);
    }

    fn redo(&mut self) {
        self.end_stroke();
        let Some(mut step) = self.history.redo.pop() else {
            self.notify("nothing to redo");

            return;
        };

        self.swap_step(&mut step.change, false);
        self.notify(format!("redid {}", step.label));
        step.key = None;
        self.history.undo.push(step);
    }

    fn swap_step(&mut self, change: &mut Change, undo: bool) {
        match change {
            Change::Document { document, selected } => {
                std::mem::swap(&mut self.document, document);
                std::mem::swap(&mut self.selected, selected);
                self.sync();
            }
            Change::Voxels(cells) => {
                if undo {
                    for (pos, before, _) in cells.iter().rev() {
                        self.voxels.set(*pos, *before);
                    }
                } else {
                    for (pos, _, after) in cells.iter() {
                        self.voxels.set(*pos, *after);
                    }
                }
            }
        }

        self.anchor = None;
        self.lift = 0.0;
        self.dirty = true;
    }

    fn delete_selection(&mut self) {
        if self.mode != Mode::Brush {
            return;
        }

        let Some(index) = self.selected else {
            return;
        };

        if !self.edit("delete brush", None, |document| document.remove_brush(index)) {
            return;
        }

        self.selected = None;
        self.notify(format!("deleted brush {index}"));
    }

    fn duplicate_selection(&mut self) {
        if self.mode != Mode::Brush {
            return;
        }

        let Some(index) = self.selected else {
            return;
        };
        let offset = Vector3::new(self.grid, self.grid, 0.0);
        let mut created = None;
        let duplicated = self.edit("duplicate brush", None, |document| {
            created = document.duplicate_brush(index, offset);

            created.is_some()
        });

        if duplicated {
            self.selected = created;
            self.notify(format!("duplicated brush {index}"));
        }
    }

    fn retexture_selection(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let texture = self.texture.clone();

        if self.edit("retexture brush", None, |document| {
            document.set_brush_texture(index, &texture)
        }) {
            self.notify(format!("brush {index} now uses {texture}"));
        }
    }

    fn resize_selection(&mut self, min: [f64; 3], max: [f64; 3]) {
        let Some(index) = self.selected else {
            return;
        };
        let min = Vector3::new(min[0], min[1], min[2]);
        let max = Vector3::new(max[0], max[1], max[2]);
        self.edit("resize brush", Some(("resize", index)), |document| {
            document.set_brush_box(index, min, max)
        });
    }

    fn nudge(&mut self, x: f64, y: f64, z: f64) {
        if self.mode != Mode::Brush || (x == 0.0 && y == 0.0 && z == 0.0) {
            return;
        }

        let Some(index) = self.selected else {
            return;
        };

        self.edit("move brush", Some(("move", index)), |document| {
            document.translate_brush(index, Vector3::new(x, y, z))
        });
    }

    fn save(&mut self) {
        self.end_stroke();
        let compiled = match self.document.save_source(&self.map_path) {
            Ok(path) => path,
            Err(err) => {
                self.warn(err);

                return;
            }
        };

        if let Err(err) = self.voxels.save_file(&self.voxel_path) {
            self.warn(err);

            return;
        }

        self.dirty = false;
        self.notify(format!(
            "saved {} and {}",
            file_label(&self.map_path),
            file_label(&self.voxel_path)
        ));
        log::info!(
            "[editor] saved {} and {} ({})",
            self.map_path.display(),
            self.voxel_path.display(),
            compiled.display()
        );
    }

    fn revert(&mut self) {
        self.end_stroke();
        let document = if self.map_path.exists() {
            let name = self.map_path.to_string_lossy().to_string();

            match CompiledMap::open_source(&name) {
                Ok((_, document)) => document,
                Err(err) => {
                    self.warn(err);

                    return;
                }
            }
        } else {
            CompiledMap::worldspawn()
        };
        let mut voxels = VoxelWorld::new();

        if self.voxel_path.exists() {
            if let Err(err) = voxels.load_file(&self.voxel_path) {
                self.warn(err);

                return;
            }
        }

        self.document = document;
        self.voxels = voxels;
        self.history = History::default();
        self.selected = None;
        self.anchor = None;
        self.lift = 0.0;
        self.dirty = false;
        self.sync();
        self.notify("reverted to the saved map");
    }

    fn request(&mut self, confirm: Confirm) {
        if self.dirty {
            self.confirm = Some(confirm);

            return;
        }

        self.resolve(confirm);
    }

    fn resolve(&mut self, confirm: Confirm) {
        self.confirm = None;

        match confirm {
            Confirm::Quit => self.quit = true,
            Confirm::Revert => self.revert(),
        }
    }

    fn sync(&mut self) {
        if let Err(err) = self.brushes.load_document(&self.document) {
            self.warn(err);
        }

        if self
            .selected
            .is_some_and(|index| index >= self.document.brush_count())
        {
            self.selected = None;
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
        let palette = self.palette();

        if palette.is_empty() {
            return;
        }

        let next = match palette.iter().position(|name| name == &self.texture) {
            Some(idx) => (idx + 1) % palette.len(),
            None => 0,
        };
        self.texture = palette[next].clone();
    }

    fn palette(&mut self) -> Vec<String> {
        let revision = self.brushes.revision();

        if self.palette.0 != revision {
            let mut names: Vec<String> = TEXTURES.iter().map(|name| name.to_string()).collect();

            for name in self.document.textures() {
                if !names.contains(&name) {
                    names.push(name);
                }
            }

            self.palette = (revision, names);
        }

        let mut names = self.palette.1.clone();

        if !names.contains(&self.texture) {
            names.push(self.texture.clone());
        }

        names
    }

    fn bump_block(&mut self, dir: i32) {
        let count = BLOCKS.len() as i32;
        let mut next = self.block as i32 + dir;

        if next < 1 {
            next = count;
        }

        if next > count {
            next = 1;
        }

        self.block = next as u16;
    }

    fn bump_size(&mut self, dir: i32) {
        if self.mode != Mode::Voxel {
            return;
        }

        self.voxel_size = (self.voxel_size + dir).clamp(1, MAX_VOXEL_SIZE);
    }

    fn box_end(&self, point: Vector3) -> Vector3 {
        let mut end = snap_point(point, self.grid);
        end.z = snap(end.z + self.lift, self.grid);

        end
    }

    fn focus_selection(&mut self) {
        let bounds = match (self.mode, self.selected) {
            (Mode::Brush, Some(index)) => self.brushes.bounds(index),
            _ => None,
        };

        match bounds {
            Some((min, max)) => focus_box(&mut self.camera, min, max),
            None => self.frame_all(),
        }
    }

    fn frame_all(&mut self) {
        let mut mesh = self.voxels.mesh();
        mesh.extend(self.brushes.mesh());

        if mesh.len() < 6 {
            self.camera = FlyCamera::new();

            return;
        }

        frame_view(&mut self.camera, &mesh);
    }
}

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(27, 29, 34);
    visuals.window_fill = egui::Color32::from_rgb(32, 35, 41);
    visuals.extreme_bg_color = egui::Color32::from_rgb(18, 20, 24);
    visuals.faint_bg_color = egui::Color32::from_rgb(34, 37, 43);
    visuals.selection.bg_fill = egui::Color32::from_rgb(44, 92, 168);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
    visuals.hyperlink_color = ACCENT;
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    visuals.menu_corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.noninteractive.bg_stroke =
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(46, 50, 58));
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 22.0;
    });
}

fn draw_ui(ctx: &egui::Context, editor: &mut Editor, aim: &Aim) {
    editor.hover = None;
    menu_bar(ctx, editor);
    tool_bar(ctx, editor);
    status_bar(ctx, editor, aim);

    if editor.view.outliner {
        outliner(ctx, editor);
    }

    if editor.view.inspector {
        inspector(ctx, editor);
    }

    viewport_overlay(ctx, editor, aim);
    help_window(ctx, editor);
    confirm_modal(ctx, editor);
}

fn menu_bar(ctx: &egui::Context, editor: &mut Editor) {
    let save_key = command_key(ctx, egui::Key::S, false);
    let undo_key = command_key(ctx, egui::Key::Z, false);
    let redo_key = command_key(ctx, egui::Key::Z, true);
    let duplicate_key = command_key(ctx, egui::Key::D, false);

    egui::TopBottomPanel::top("menu").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("Save").shortcut_text(&save_key))
                    .clicked()
                {
                    editor.save();
                    ui.close_menu();
                }

                if ui
                    .add_enabled(editor.dirty, egui::Button::new("Revert to saved"))
                    .clicked()
                {
                    editor.request(Confirm::Revert);
                    ui.close_menu();
                }

                ui.separator();

                if ui.button("Quit").clicked() {
                    editor.request(Confirm::Quit);
                    ui.close_menu();
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo = match editor.history.undo.last() {
                    Some(step) => format!("Undo {}", step.label),
                    None => "Undo".to_string(),
                };
                let redo = match editor.history.redo.last() {
                    Some(step) => format!("Redo {}", step.label),
                    None => "Redo".to_string(),
                };
                let selected = editor.mode == Mode::Brush && editor.selected.is_some();

                if ui
                    .add_enabled(
                        !editor.history.undo.is_empty(),
                        egui::Button::new(undo).shortcut_text(&undo_key),
                    )
                    .clicked()
                {
                    editor.undo();
                    ui.close_menu();
                }

                if ui
                    .add_enabled(
                        !editor.history.redo.is_empty(),
                        egui::Button::new(redo).shortcut_text(&redo_key),
                    )
                    .clicked()
                {
                    editor.redo();
                    ui.close_menu();
                }

                ui.separator();

                if ui
                    .add_enabled(
                        selected,
                        egui::Button::new("Duplicate brush").shortcut_text(&duplicate_key),
                    )
                    .clicked()
                {
                    editor.duplicate_selection();
                    ui.close_menu();
                }

                if ui
                    .add_enabled(selected, egui::Button::new("Delete brush").shortcut_text("Del"))
                    .clicked()
                {
                    editor.delete_selection();
                    ui.close_menu();
                }

                if ui
                    .add_enabled(selected, egui::Button::new("Deselect").shortcut_text("Esc"))
                    .clicked()
                {
                    editor.selected = None;
                    ui.close_menu();
                }
            });
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut editor.view.outliner, "Outliner panel");
                ui.checkbox(&mut editor.view.inspector, "Inspector panel");
                ui.separator();
                ui.checkbox(&mut editor.view.grid, "Grid");
                ui.checkbox(&mut editor.view.axes, "Axes");
                ui.separator();

                if ui
                    .add(egui::Button::new("Focus selection").shortcut_text("F"))
                    .clicked()
                {
                    editor.focus_selection();
                    ui.close_menu();
                }

                if ui.button("Frame everything").clicked() {
                    editor.frame_all();
                    ui.close_menu();
                }

                ui.separator();
                ui.add(
                    egui::Slider::new(&mut editor.view.speed, 0.1..=10.0)
                        .logarithmic(true)
                        .text("Fly speed"),
                );
                ui.add(egui::Slider::new(&mut editor.view.zoom, 0.75..=2.0).text("UI scale"));
            });
            ui.menu_button("Help", |ui| {
                if ui
                    .add(egui::Button::new("Keyboard shortcuts").shortcut_text("F1"))
                    .clicked()
                {
                    editor.view.help = true;
                    ui.close_menu();
                }
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if editor.dirty {
                    ui.label(egui::RichText::new("● unsaved").color(WARN));
                } else {
                    ui.label(egui::RichText::new("saved").color(MUTED));
                }

                ui.label(egui::RichText::new(file_label(&editor.map_path)).strong());
            });
        });
    });
}

fn tool_bar(ctx: &egui::Context, editor: &mut Editor) {
    let frame = egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(10, 6));

    egui::TopBottomPanel::top("tools").frame(frame).show(ctx, |ui| {
        ui.horizontal(|ui| {
            caption(ui, "MODE");

            if ui
                .selectable_label(editor.mode == Mode::Brush, "⬛ Brush")
                .on_hover_text("Build with solid brushes  (B, Tab)")
                .clicked()
            {
                editor.set_mode(Mode::Brush);
            }

            if ui
                .selectable_label(editor.mode == Mode::Voxel, "▦ Voxel")
                .on_hover_text("Paint and erase voxel blocks  (V, Tab)")
                .clicked()
            {
                editor.set_mode(Mode::Voxel);
            }

            ui.separator();
            caption(ui, "TOOL");

            match editor.mode {
                Mode::Brush => {
                    if ui
                        .selectable_label(editor.brush_tool == BrushTool::Select, "Select")
                        .on_hover_text("Click a brush to select it  (1)")
                        .clicked()
                    {
                        editor.set_tool(1);
                    }

                    if ui
                        .selectable_label(editor.brush_tool == BrushTool::Box, "Box")
                        .on_hover_text("Click two corners to draw a box brush  (2)")
                        .clicked()
                    {
                        editor.set_tool(2);
                    }
                }
                Mode::Voxel => {
                    if ui
                        .selectable_label(editor.voxel_tool == VoxelTool::Paint, "Paint")
                        .on_hover_text("Click or drag to place blocks  (1)")
                        .clicked()
                    {
                        editor.set_tool(1);
                    }

                    if ui
                        .selectable_label(editor.voxel_tool == VoxelTool::Erase, "Erase")
                        .on_hover_text("Click or drag to remove blocks  (2)")
                        .clicked()
                    {
                        editor.set_tool(2);
                    }
                }
            }

            ui.separator();
            caption(ui, "GRID");
            egui::ComboBox::from_id_salt("grid")
                .width(56.0)
                .selected_text(number_label(editor.grid))
                .show_ui(ui, |ui| {
                    for grid in GRIDS {
                        ui.selectable_value(&mut editor.grid, grid, number_label(grid));
                    }
                })
                .response
                .on_hover_text("Snap size  (G cycles)");
            ui.separator();

            match editor.mode {
                Mode::Brush => {
                    caption(ui, "TEXTURE");
                    let palette = editor.palette();
                    egui::ComboBox::from_id_salt("texture")
                        .width(110.0)
                        .selected_text(editor.texture.clone())
                        .show_ui(ui, |ui| {
                            for name in &palette {
                                ui.selectable_value(&mut editor.texture, name.clone(), name.as_str());
                            }
                        })
                        .response
                        .on_hover_text("Texture for new brushes  (T cycles)");
                }
                Mode::Voxel => {
                    caption(ui, "BLOCK");
                    swatch(ui, editor.block, 14.0);
                    egui::ComboBox::from_id_salt("block")
                        .width(110.0)
                        .selected_text(block_name(editor.block))
                        .show_ui(ui, |ui| {
                            for (id, name) in BLOCKS {
                                ui.horizontal(|ui| {
                                    swatch(ui, id, 12.0);
                                    ui.selectable_value(&mut editor.block, id, name);
                                });
                            }
                        })
                        .response
                        .on_hover_text("Block to paint  ([ and ] cycle, or mouse wheel)");
                    caption(ui, "SIZE");
                    ui.add(egui::DragValue::new(&mut editor.voxel_size).range(1..=MAX_VOXEL_SIZE))
                        .on_hover_text("Brush size in blocks  (- and =)");
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save = egui::Button::new(if editor.dirty { "💾 Save*" } else { "💾 Save" });
                let save = if editor.dirty {
                    save.fill(egui::Color32::from_rgb(44, 92, 168))
                } else {
                    save
                };

                if ui
                    .add(save)
                    .on_hover_text(format!("Save the map  ({})", command_key(ctx, egui::Key::S, false)))
                    .clicked()
                {
                    editor.save();
                }

                if ui
                    .add_enabled(!editor.history.redo.is_empty(), egui::Button::new("Redo"))
                    .on_hover_text(command_key(ctx, egui::Key::Z, true))
                    .clicked()
                {
                    editor.redo();
                }

                if ui
                    .add_enabled(!editor.history.undo.is_empty(), egui::Button::new("Undo"))
                    .on_hover_text(command_key(ctx, egui::Key::Z, false))
                    .clicked()
                {
                    editor.undo();
                }
            });
        });
    });
}

fn status_bar(ctx: &egui::Context, editor: &mut Editor, aim: &Aim) {
    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(mode_label(editor)).strong().color(ACCENT));
            ui.separator();
            ui.label(aim_label(editor, aim));

            if aim.surface {
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{:.2}  {:.2}  {:.2}",
                        aim.point.x, aim.point.y, aim.point.z
                    ))
                    .monospace()
                    .color(MUTED),
                );
            }

            let age = editor.message_at.elapsed().as_secs_f32();

            if !editor.message.is_empty() && age < NOTICE_SECONDS {
                ui.separator();
                let color = if editor.error { DANGER } else { GOOD };
                let fade = (NOTICE_SECONDS - age).clamp(0.0, 1.0);
                ui.label(egui::RichText::new(&editor.message).color(color.gamma_multiply(fade)));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("{:.0} fps", editor.fps)).color(MUTED));
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "cam {:.1} {:.1} {:.1}",
                        editor.camera.x, editor.camera.y, editor.camera.z
                    ))
                    .monospace()
                    .color(MUTED),
                );
                ui.separator();
                ui.label(format!(
                    "{} brushes · {} chunks",
                    editor.document.brush_count(),
                    editor.voxels.chunk_count()
                ));
            });
        });
    });
}

fn outliner(ctx: &egui::Context, editor: &mut Editor) {
    egui::SidePanel::left("outliner")
        .resizable(true)
        .default_width(250.0)
        .width_range(190.0..=460.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            section(ui, "Map");
            egui::Grid::new("map_info")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Brushes").color(MUTED));
                    ui.label(file_label(&editor.map_path))
                        .on_hover_text(editor.map_path.display().to_string());
                    ui.end_row();
                    ui.label(egui::RichText::new("Voxels").color(MUTED));
                    ui.label(file_label(&editor.voxel_path))
                        .on_hover_text(editor.voxel_path.display().to_string());
                    ui.end_row();
                    ui.label(egui::RichText::new("Entities").color(MUTED));
                    ui.label(editor.document.entities.len().to_string());
                    ui.end_row();
                    ui.label(egui::RichText::new("Chunks").color(MUTED));
                    ui.label(editor.voxels.chunk_count().to_string());
                    ui.end_row();
                });

            section(ui, "Brushes");
            ui.add(
                egui::TextEdit::singleline(&mut editor.filter)
                    .hint_text("Filter by number, texture or entity")
                    .desired_width(f32::INFINITY),
            );
            let total = editor.document.brush_count();
            let filter = editor.filter.trim().to_lowercase();
            let rows: Vec<usize> = (0..total)
                .filter(|index| filter.is_empty() || brush_matches(editor, *index, &filter))
                .collect();
            ui.label(
                egui::RichText::new(if filter.is_empty() {
                    format!("{total} brushes · double-click to focus")
                } else {
                    format!("{} of {total} brushes", rows.len())
                })
                .small()
                .color(MUTED),
            );
            let row_height = ui.spacing().interact_size.y;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, row_height, rows.len(), |ui, range| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        for row in range {
                            let index = rows[row];
                            let text = brush_row(editor, index);
                            let response = ui.selectable_label(editor.selected == Some(index), text);

                            if response.hovered() {
                                editor.hover = Some(index);
                            }

                            if response.clicked() {
                                editor.select(index);
                            }

                            if response.double_clicked() {
                                editor.select(index);
                                editor.focus_selection();
                            }
                        }
                    });
                });
        });
}

fn inspector(ctx: &egui::Context, editor: &mut Editor) {
    egui::SidePanel::right("inspector")
        .resizable(true)
        .default_width(290.0)
        .width_range(230.0..=480.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(4.0);

                    match editor.mode {
                        Mode::Brush => brush_inspector(ui, editor),
                        Mode::Voxel => voxel_inspector(ui, editor),
                    }

                    section(ui, "View");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut editor.view.grid, "Grid");
                        ui.checkbox(&mut editor.view.axes, "Axes");
                    });
                    ui.add(
                        egui::Slider::new(&mut editor.view.speed, 0.1..=10.0)
                            .logarithmic(true)
                            .text("fly speed"),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Focus selection").clicked() {
                            editor.focus_selection();
                        }

                        if ui.button("Frame everything").clicked() {
                            editor.frame_all();
                        }
                    });
                });
        });
}

fn brush_inspector(ui: &mut egui::Ui, editor: &mut Editor) {
    section(ui, "Selection");

    match editor.selected {
        None => {
            ui.label(
                egui::RichText::new(
                    "Nothing selected. Use the Select tool (1) and click a brush, or pick one from the outliner.",
                )
                .color(MUTED),
            );
        }
        Some(index) => {
            let owner = editor
                .document
                .brush_owner(index)
                .map(entity_class)
                .unwrap_or_else(|| "?".to_string());
            let faces = editor
                .document
                .brush(index)
                .map(|brush| brush.faces.len())
                .unwrap_or(0);
            let texture = brush_texture(&editor.document, index);
            egui::Grid::new("selection")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Brush").color(MUTED));
                    ui.label(egui::RichText::new(format!("#{index}")).strong());
                    ui.end_row();
                    ui.label(egui::RichText::new("Entity").color(MUTED));
                    ui.label(owner);
                    ui.end_row();
                    ui.label(egui::RichText::new("Faces").color(MUTED));
                    ui.label(faces.to_string());
                    ui.end_row();
                    ui.label(egui::RichText::new("Texture").color(MUTED));
                    ui.label(texture);
                    ui.end_row();
                });
            ui.add_space(4.0);

            if let Some((min, max)) = editor.document.brush_box(index) {
                let mut low = [min.x, min.y, min.z];
                let mut high = [max.x, max.y, max.z];
                let mut changed = false;
                let speed = (editor.grid * 0.05).max(0.01);
                egui::Grid::new("bounds")
                    .num_columns(4)
                    .spacing([6.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("");
                        ui.label(egui::RichText::new("X").color(egui::Color32::from_rgb(230, 90, 90)));
                        ui.label(egui::RichText::new("Y").color(egui::Color32::from_rgb(110, 210, 120)));
                        ui.label(egui::RichText::new("Z").color(egui::Color32::from_rgb(100, 150, 255)));
                        ui.end_row();
                        ui.label(egui::RichText::new("Min").color(MUTED));

                        for value in low.iter_mut() {
                            changed |= ui
                                .add(egui::DragValue::new(value).speed(speed).max_decimals(3))
                                .changed();
                        }

                        ui.end_row();
                        ui.label(egui::RichText::new("Max").color(MUTED));

                        for value in high.iter_mut() {
                            changed |= ui
                                .add(egui::DragValue::new(value).speed(speed).max_decimals(3))
                                .changed();
                        }

                        ui.end_row();
                        ui.label(egui::RichText::new("Size").color(MUTED));
                        let mut axis = 0;

                        while axis < 3 {
                            ui.label(number_label(high[axis] - low[axis]));
                            axis += 1;
                        }

                        ui.end_row();
                    });

                if changed {
                    editor.resize_selection(low, high);
                }
            } else if let Some((min, max)) = editor.brushes.bounds(index) {
                ui.label(
                    egui::RichText::new(format!(
                        "Bounds {} → {}  ({})",
                        point_label(min),
                        point_label(max),
                        size_label(min, max)
                    ))
                    .color(MUTED),
                );
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                caption(ui, "NUDGE");
                let step = editor.grid;

                if ui.small_button("−X").clicked() {
                    editor.nudge(-step, 0.0, 0.0);
                }

                if ui.small_button("+X").clicked() {
                    editor.nudge(step, 0.0, 0.0);
                }

                if ui.small_button("−Y").clicked() {
                    editor.nudge(0.0, -step, 0.0);
                }

                if ui.small_button("+Y").clicked() {
                    editor.nudge(0.0, step, 0.0);
                }

                if ui.small_button("−Z").clicked() {
                    editor.nudge(0.0, 0.0, -step);
                }

                if ui.small_button("+Z").clicked() {
                    editor.nudge(0.0, 0.0, step);
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Duplicate").clicked() {
                    editor.duplicate_selection();
                }

                if ui.button("Focus").clicked() {
                    editor.focus_selection();
                }

                if ui
                    .button(egui::RichText::new("🗑 Delete").color(DANGER))
                    .clicked()
                {
                    editor.delete_selection();
                }
            });
        }
    }

    section(ui, "Texture");
    let palette = editor.palette();
    ui.horizontal_wrapped(|ui| {
        for name in &palette {
            if ui.selectable_label(&editor.texture == name, name.as_str()).clicked() {
                editor.texture = name.clone();
            }
        }
    });
    ui.horizontal(|ui| {
        let edit = ui.add(
            egui::TextEdit::singleline(&mut editor.custom)
                .hint_text("custom texture")
                .desired_width(140.0),
        );
        let custom = editor.custom.trim().to_string();
        let valid = texture_name_ok(&custom);
        let submit = edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));

        if (ui.add_enabled(valid, egui::Button::new("Use")).clicked() || (submit && valid))
            && valid
        {
            editor.texture = custom;
            editor.custom.clear();
        }
    });

    if editor.selected.is_some()
        && ui
            .button(format!("Apply “{}” to selection", editor.texture))
            .clicked()
    {
        editor.retexture_selection();
    }

    if editor.brush_tool == BrushTool::Box {
        section(ui, "Box tool");
        ui.label(
            egui::RichText::new(
                "Click once to place the first corner, then click again for the opposite corner. Scroll to raise or lower the height. Esc cancels.",
            )
            .color(MUTED),
        );

        if editor.anchor.is_some() {
            ui.label(format!("Height offset {}", number_label(editor.lift)));
        }
    }
}

fn voxel_inspector(ui: &mut egui::Ui, editor: &mut Editor) {
    section(ui, "Block");
    egui::Grid::new("blocks")
        .num_columns(2)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            let mut idx = 0;

            while idx < BLOCKS.len() {
                let (id, name) = BLOCKS[idx];
                ui.horizontal(|ui| {
                    swatch(ui, id, 14.0);

                    if ui.selectable_label(editor.block == id, name).clicked() {
                        editor.block = id;
                    }
                });

                if idx % 2 == 1 {
                    ui.end_row();
                }

                idx += 1;
            }
        });

    section(ui, "Brush");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut editor.voxel_tool, VoxelTool::Paint, "Paint");
        ui.selectable_value(&mut editor.voxel_tool, VoxelTool::Erase, "Erase");
    });
    ui.add(egui::Slider::new(&mut editor.voxel_size, 1..=MAX_VOXEL_SIZE).text("size"));
    ui.label(
        egui::RichText::new(
            "Click or drag to edit. The outline shows the affected blocks. Delete erases under the cursor.",
        )
        .color(MUTED),
    );

    section(ui, "World");
    egui::Grid::new("voxel_info")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Chunks").color(MUTED));
            ui.label(editor.voxels.chunk_count().to_string());
            ui.end_row();
            ui.label(egui::RichText::new("Block scale").color(MUTED));
            ui.label(number_label(editor.voxels.scale()));
            ui.end_row();
        });
}

fn viewport_overlay(ctx: &egui::Context, editor: &Editor, aim: &Aim) {
    let rect = ctx.available_rect();

    if rect.width() < 40.0 || rect.height() < 40.0 {
        return;
    }

    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("viewport_overlay"),
        ))
        .with_clip_rect(rect);
    let font = egui::FontId::proportional(13.0);
    pill(
        &painter,
        rect.left_top() + egui::vec2(12.0, 10.0),
        egui::Align2::LEFT_TOP,
        tool_hint(editor),
        font.clone(),
        egui::Color32::from_gray(235),
    );
    pill(
        &painter,
        rect.left_bottom() + egui::vec2(12.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        "Hold RMB to look · WASD fly · Space/C up/down · Shift faster · RMB+wheel speed · F1 help"
            .to_string(),
        egui::FontId::proportional(12.0),
        egui::Color32::from_gray(200),
    );

    let ppp = ctx.pixels_per_point();
    let cursor = egui::pos2(editor.cursor_x / ppp, editor.cursor_y / ppp);

    if !rect.contains(cursor) {
        return;
    }

    let label = match editor.mode {
        Mode::Brush => match (editor.brush_tool, editor.anchor) {
            (BrushTool::Box, Some(anchor)) if aim.surface => {
                let end = editor.box_end(aim.point);
                let (min, max) = box_from_corners(anchor, end, editor.grid);

                Some(size_label(min, max))
            }
            (BrushTool::Box, None) if aim.surface => {
                Some(point_label(snap_point(aim.point, editor.grid)))
            }
            (BrushTool::Select, _) => aim.brush.map(|index| format!("brush #{index}")),
            _ => None,
        },
        Mode::Voxel => {
            let cell = match editor.voxel_tool {
                VoxelTool::Paint => aim.place,
                VoxelTool::Erase => aim.erase,
            };

            cell.map(|pos| format!("{} {} {}", pos.x, pos.y, pos.z))
        }
    };

    if let Some(label) = label {
        pill(
            &painter,
            cursor + egui::vec2(16.0, 14.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::monospace(12.0),
            egui::Color32::WHITE,
        );
    }
}

fn help_window(ctx: &egui::Context, editor: &mut Editor) {
    let command = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };
    let groups: [(&str, Vec<(String, &str)>); 4] = [
        (
            "Camera",
            vec![
                ("Hold right mouse".to_string(), "look around"),
                ("W A S D".to_string(), "fly"),
                ("Space / C".to_string(), "up / down"),
                ("Shift".to_string(), "fly faster"),
                ("Right mouse + wheel".to_string(), "change fly speed"),
                ("F".to_string(), "focus selection / frame map"),
            ],
        ),
        (
            "Modes & tools",
            vec![
                ("Tab, B, V".to_string(), "switch mode"),
                ("1 / 2".to_string(), "select or box, paint or erase"),
                ("G".to_string(), "cycle grid size"),
                ("T".to_string(), "cycle texture"),
                ("[ / ] or wheel".to_string(), "cycle voxel block"),
                ("- / =".to_string(), "voxel brush size"),
            ],
        ),
        (
            "Editing",
            vec![
                ("Left mouse".to_string(), "use tool"),
                ("Arrows, Q / E".to_string(), "nudge selection"),
                ("Delete".to_string(), "delete brush / erase voxels"),
                (format!("{command}+D"), "duplicate brush"),
                (format!("{command}+Z"), "undo"),
                (format!("{command}+Shift+Z"), "redo"),
                ("Esc".to_string(), "cancel / deselect"),
            ],
        ),
        ("File", vec![(format!("{command}+S"), "save map and voxels")]),
    ];

    egui::Window::new("Keyboard shortcuts")
        .open(&mut editor.view.help)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                for (title, rows) in &groups {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(*title).strong().color(ACCENT));
                        egui::Grid::new(*title)
                            .num_columns(2)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                for (keys, action) in rows {
                                    ui.label(egui::RichText::new(keys).monospace());
                                    ui.label(egui::RichText::new(*action).color(MUTED));
                                    ui.end_row();
                                }
                            });
                    });
                    ui.add_space(12.0);
                }
            });
        });
}

fn confirm_modal(ctx: &egui::Context, editor: &mut Editor) {
    let Some(confirm) = editor.confirm else {
        return;
    };
    let mut choice: Option<&str> = None;
    let response = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.heading("Unsaved changes");
        ui.add_space(4.0);
        ui.label(match confirm {
            Confirm::Quit => "You have unsaved changes. Save them before quitting?",
            Confirm::Revert => "Discard every unsaved change and reload the map from disk?",
        });
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match confirm {
                Confirm::Quit => {
                    if ui
                        .add(egui::Button::new("Save and quit").fill(egui::Color32::from_rgb(44, 92, 168)))
                        .clicked()
                    {
                        choice = Some("save");
                    }

                    if ui
                        .button(egui::RichText::new("Quit without saving").color(DANGER))
                        .clicked()
                    {
                        choice = Some("discard");
                    }
                }
                Confirm::Revert => {
                    if ui
                        .button(egui::RichText::new("Discard and reload").color(DANGER))
                        .clicked()
                    {
                        choice = Some("discard");
                    }
                }
            }

            if ui.button("Cancel").clicked() {
                choice = Some("cancel");
            }
        });
    });

    match choice {
        Some("save") => {
            editor.save();

            if !editor.dirty {
                editor.resolve(confirm);
            } else {
                editor.confirm = None;
            }
        }
        Some("discard") => editor.resolve(confirm),
        Some(_) => editor.confirm = None,
        None => {
            if response.should_close() {
                editor.confirm = None;
            }
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(title.to_uppercase()).small().strong().color(MUTED));
    ui.separator();
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().color(MUTED));
}

fn swatch(ui: &mut egui::Ui, block: u16, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let [r, g, b] = block_rgb(block);
    ui.painter().rect_filled(
        rect,
        3.0,
        egui::Color32::from_rgb(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
        ),
    );
    ui.painter().rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(140)),
        egui::StrokeKind::Inside,
    );
}

fn pill(
    painter: &egui::Painter,
    pos: egui::Pos2,
    align: egui::Align2,
    text: String,
    font: egui::FontId,
    color: egui::Color32,
) {
    let galley = painter.layout_no_wrap(text, font, color);
    let size = galley.size() + egui::vec2(14.0, 8.0);
    let rect = align.anchor_size(pos, size);
    painter.rect_filled(rect, 5.0, egui::Color32::from_black_alpha(165));
    painter.galley(rect.min + egui::vec2(7.0, 4.0), galley, color);
}

fn command_key(ctx: &egui::Context, key: egui::Key, shift: bool) -> String {
    let modifiers = if shift {
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::COMMAND
    };

    ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, key))
}

fn mode_label(editor: &Editor) -> String {
    let tool = match editor.mode {
        Mode::Brush => match editor.brush_tool {
            BrushTool::Select => "Select",
            BrushTool::Box => "Box",
        },
        Mode::Voxel => match editor.voxel_tool {
            VoxelTool::Paint => "Paint",
            VoxelTool::Erase => "Erase",
        },
    };

    match editor.mode {
        Mode::Brush => format!("BRUSH · {tool}"),
        Mode::Voxel => format!("VOXEL · {tool}"),
    }
}

fn tool_hint(editor: &Editor) -> String {
    match editor.mode {
        Mode::Brush => match editor.brush_tool {
            BrushTool::Select => match editor.selected {
                Some(index) => format!(
                    "Brush #{index} selected · arrows/Q/E nudge · {} duplicate · Del delete",
                    if cfg!(target_os = "macos") { "Cmd+D" } else { "Ctrl+D" }
                ),
                None => "Click a brush to select it".to_string(),
            },
            BrushTool::Box => match editor.anchor {
                Some(_) => format!(
                    "Click the opposite corner · wheel height ({}) · Esc cancel",
                    number_label(editor.lift)
                ),
                None => format!("Click to place the first corner · texture “{}”", editor.texture),
            },
        },
        Mode::Voxel => {
            let verb = match editor.voxel_tool {
                VoxelTool::Paint => format!("Painting {}", block_name(editor.block)),
                VoxelTool::Erase => "Erasing".to_string(),
            };
            let size = editor.voxel_size;

            format!("{verb} · {size}×{size}×{size} · click or drag")
        }
    }
}

fn block_name(block: u16) -> String {
    for (id, name) in BLOCKS {
        if id == block {
            return name.to_string();
        }
    }

    format!("block {block}")
}

fn entity_class(entity: &CompiledEntity) -> String {
    for pair in &entity.keys {
        if pair.key == "classname" {
            return pair.value.clone();
        }
    }

    "entity".to_string()
}

fn brush_texture(document: &CompiledMap, index: usize) -> String {
    let Some(brush) = document.brush(index) else {
        return "?".to_string();
    };
    let Some(first) = brush.faces.first() else {
        return "?".to_string();
    };

    if brush.faces.iter().all(|face| face.texture == first.texture) {
        first.texture.clone()
    } else {
        format!("{} (mixed)", first.texture)
    }
}

fn brush_matches(editor: &Editor, index: usize, filter: &str) -> bool {
    if index.to_string().contains(filter) {
        return true;
    }

    if let Some(brush) = editor.document.brush(index) {
        if brush
            .faces
            .iter()
            .any(|face| face.texture.to_lowercase().contains(filter))
        {
            return true;
        }
    }

    editor
        .document
        .brush_owner(index)
        .is_some_and(|entity| entity_class(entity).to_lowercase().contains(filter))
}

fn brush_row(editor: &Editor, index: usize) -> egui::RichText {
    let texture = brush_texture(&editor.document, index);
    let size = editor
        .brushes
        .bounds(index)
        .map(|(min, max)| size_label(min, max))
        .unwrap_or_default();
    let owner = editor
        .document
        .brush_owner(index)
        .map(entity_class)
        .unwrap_or_default();
    let owner = if owner == "worldspawn" {
        String::new()
    } else {
        format!("  [{owner}]")
    };

    egui::RichText::new(format!("#{index:<4} {texture:<10} {size}{owner}")).monospace()
}

fn number_label(value: f64) -> String {
    if (value - value.round()).abs() < 1e-6 {
        let rounded = value.round();

        if rounded == 0.0 {
            return "0".to_string();
        }

        return format!("{rounded}");
    }

    let text = format!("{value:.3}");

    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn point_label(point: Vector3) -> String {
    format!(
        "{}, {}, {}",
        number_label(point.x),
        number_label(point.y),
        number_label(point.z)
    )
}

fn size_label(min: Vector3, max: Vector3) -> String {
    format!(
        "{} × {} × {}",
        number_label(max.x - min.x),
        number_label(max.y - min.y),
        number_label(max.z - min.z)
    )
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
        "{:?}:{:?}:{:?}:{}:{}:{}:{}",
        editor.mode,
        editor.brush_tool,
        editor.voxel_tool,
        editor.grid,
        editor.view.grid,
        editor.view.axes,
        editor.voxel_size
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

fn push_overlay(vertices: &mut Vec<f32>, editor: &Editor, aim: &Aim, draw: Anchor) {
    if editor.view.grid {
        push_grid(vertices, editor.grid as f32, 48.0, draw);
    }

    if editor.view.axes {
        push_axes(vertices, draw);
    }

    if editor.mode == Mode::Brush && editor.brush_tool == BrushTool::Box {
        if let Some(anchor) = editor.anchor {
            if aim.surface {
                let end = editor.box_end(aim.point);
                let (min, max) = box_from_corners(anchor, end, editor.grid);
                push_box(vertices, min, max, BOX_COLOR, draw);
            } else {
                push_marker(vertices, anchor, editor.grid, BOX_COLOR, draw);
            }
        } else if aim.surface {
            push_marker(
                vertices,
                snap_point(aim.point, editor.grid),
                editor.grid,
                BOX_COLOR,
                draw,
            );
        }
    }

    if editor.mode == Mode::Voxel {
        let cell = match editor.voxel_tool {
            VoxelTool::Paint => aim.place,
            VoxelTool::Erase => aim.erase,
        };

        if let Some(cell) = cell {
            let scale = editor.voxels.scale();
            let (low, high) = brush_cells(cell, editor.voxel_size);
            let (min, _) = block_bounds(&editor.voxels, low, scale * 0.03);
            let (_, max) = block_bounds(&editor.voxels, high, scale * 0.03);
            let color = match editor.voxel_tool {
                VoxelTool::Paint => PAINT_COLOR,
                VoxelTool::Erase => ERASE_COLOR,
            };
            push_wire_box(vertices, min, max, scale * 0.04, color, draw);
        }
    }
}

fn aim_label(editor: &Editor, aim: &Aim) -> String {
    match editor.mode {
        Mode::Brush => match aim.brush {
            Some(index) => format!("hover brush #{index}"),
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

fn brush_cells(center: BlockPos, size: i32) -> (BlockPos, BlockPos) {
    let size = size.clamp(1, MAX_VOXEL_SIZE);
    let low = -(size - 1) / 2;
    let high = size / 2;

    (
        BlockPos::new(center.x + low, center.y + low, center.z + low),
        BlockPos::new(center.x + high, center.y + high, center.z + high),
    )
}

fn camera_forward(camera: &FlyCamera) -> Vector3 {
    let cp = camera.pitch.cos() as f64;
    let sp = camera.pitch.sin() as f64;
    let cy = camera.yaw.cos() as f64;
    let sy = camera.yaw.sin() as f64;

    Vector3::new(cp * cy, cp * sy, sp)
}

fn focus_box(camera: &mut FlyCamera, min: Vector3, max: Vector3) {
    let center = Vector3::new(
        (min.x + max.x) * 0.5,
        (min.y + max.y) * 0.5,
        (min.z + max.z) * 0.5,
    );
    let dx = max.x - min.x;
    let dy = max.y - min.y;
    let dz = max.z - min.z;
    let radius = ((dx * dx + dy * dy + dz * dz).sqrt() * 0.5).max(0.5);
    let distance = radius * 2.4 + 1.0;
    let forward = camera_forward(camera);
    camera.x = center.x - forward.x * distance;
    camera.y = center.y - forward.y * distance;
    camera.z = center.z - forward.z * distance;
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
    camera.x = f64::from(center[0]);
    camera.y = f64::from(center[1] - radius * 0.9 - 6.0);
    camera.z = f64::from(center[2] + radius * 0.35 + 3.0);
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
        Vector3::new(camera.x, camera.y, camera.z),
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

fn push_marker(
    vertices: &mut Vec<f32>,
    point: Vector3,
    grid: f64,
    color: [f32; 3],
    draw: Anchor,
) {
    let s = (grid * 0.12).max(0.04);
    push_box(
        vertices,
        Vector3::new(point.x - s, point.y - s, point.z - s),
        Vector3::new(point.x + s, point.y + s, point.z + s),
        color,
        draw,
    );
}

fn push_axes(vertices: &mut Vec<f32>, draw: Anchor) {
    push_box(
        vertices,
        Vector3::new(0.0, -0.04, -0.04),
        Vector3::new(4.0, 0.04, 0.04),
        [0.9, 0.25, 0.25],
        draw,
    );
    push_box(
        vertices,
        Vector3::new(-0.04, 0.0, -0.04),
        Vector3::new(0.04, 4.0, 0.04),
        [0.25, 0.85, 0.35],
        draw,
    );
    push_box(
        vertices,
        Vector3::new(-0.04, -0.04, 0.0),
        Vector3::new(0.04, 0.04, 4.0),
        [0.3, 0.55, 1.0],
        draw,
    );
}

fn push_grid(vertices: &mut Vec<f32>, step: f32, extent: f32, draw: Anchor) {
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
            draw,
        );
        push_ribbon(
            vertices,
            [cursor, -extent, -0.03],
            [cursor, extent, -0.03],
            0.02,
            color,
            draw,
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
    draw: Anchor,
) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();

    if len < 1e-6 {
        return;
    }

    let sx = -dy / len * half_width;
    let sy = dx / len * half_width;
    let p0 = shift_point(draw, [a[0] - sx, a[1] - sy, a[2]]);
    let p1 = shift_point(draw, [b[0] - sx, b[1] - sy, b[2]]);
    let p2 = shift_point(draw, [b[0] + sx, b[1] + sy, b[2]]);
    let p3 = shift_point(draw, [a[0] + sx, a[1] + sy, a[2]]);
        crate::world::push_shaded_tri(vertices, p0, p1, p2, color);
        crate::world::push_shaded_tri(vertices, p0, p2, p3, color);
}

fn shift_point(draw: Anchor, point: [f32; 3]) -> [f32; 3] {
    draw.relative(
        f64::from(point[0]),
        f64::from(point[1]),
        f64::from(point[2]),
    )
}

fn push_wire_box(
    vertices: &mut Vec<f32>,
    min: Vector3,
    max: Vector3,
    thickness: f64,
    color: [f32; 3],
    draw: Anchor,
) {
    let t = thickness.max(0.005);
    let low = [min.x, min.y, min.z];
    let high = [max.x, max.y, max.z];
    let mut axis = 0;

    while axis < 3 {
        let a = (axis + 1) % 3;
        let b = (axis + 2) % 3;
        let mut corner = 0;

        while corner < 4 {
            let mut start = [0.0; 3];
            let mut end = [0.0; 3];
            let along_a = if corner & 1 == 0 { low[a] } else { high[a] };
            let along_b = if corner & 2 == 0 { low[b] } else { high[b] };
            start[axis] = low[axis] - t;
            end[axis] = high[axis] + t;
            start[a] = along_a - t;
            end[a] = along_a + t;
            start[b] = along_b - t;
            end[b] = along_b + t;
            push_box(
                vertices,
                Vector3::new(start[0], start[1], start[2]),
                Vector3::new(end[0], end[1], end[2]),
                color,
                draw,
            );
            corner += 1;
        }

        axis += 1;
    }
}

fn push_box(vertices: &mut Vec<f32>, min: Vector3, max: Vector3, color: [f32; 3], draw: Anchor) {
    let span = [max.x - min.x, max.y - min.y, max.z - min.z];

    for face in 0..6 {
        let quad = QUADS[face];
        let shade = SHADES[face];
        let tint = [color[0] * shade, color[1] * shade, color[2] * shade];
        let mut corners = [[0.0f32; 3]; 4];

        for corner in 0..4 {
            corners[corner] = draw.relative(
                min.x + quad[corner].0 as f64 * span[0],
                min.y + quad[corner].1 as f64 * span[1],
                min.z + quad[corner].2 as f64 * span[2],
            );
        }

        crate::world::push_shaded_tri(vertices, corners[0], corners[1], corners[2], tint);
        crate::world::push_shaded_tri(vertices, corners[0], corners[2], corners[3], tint);
    }
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
        MouseScrollDelta::PixelDelta(_, y) => y / 48.0,
    }
}

fn held_key(keys: &HashSet<KeyCode>, code: KeyCode) -> f32 {
    if keys.contains(&code) {
        1.0
    } else {
        0.0
    }
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

    #[test]
    fn brush_edits_undo_and_redo() {
        let mut editor = open_editor("hall").unwrap();
        let count = editor.document.brush_count();
        let texture = editor.texture.clone();

        assert!(editor.edit("add brush", None, |document| {
            document
                .add_box(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0), &texture)
                .is_some()
        }));
        assert_eq!(editor.document.brush_count(), count + 1);
        assert!(editor.dirty);

        editor.undo();

        assert_eq!(editor.document.brush_count(), count);
        assert_eq!(editor.brushes.len(), count);

        editor.redo();

        assert_eq!(editor.document.brush_count(), count + 1);
        assert_eq!(editor.brushes.len(), count + 1);
    }

    #[test]
    fn repeated_nudges_merge_into_one_step() {
        let mut editor = open_editor("hall").unwrap();
        editor.selected = Some(0);
        let before = editor.document.brush_box(0);
        editor.nudge(1.0, 0.0, 0.0);
        editor.nudge(1.0, 0.0, 0.0);
        editor.nudge(1.0, 0.0, 0.0);

        assert_eq!(editor.history.undo.len(), 1);

        editor.undo();

        assert_eq!(editor.document.brush_box(0), before);
    }

    #[test]
    fn voxel_strokes_undo_as_one_step() {
        let mut editor = open_editor("hall").unwrap();
        editor.mode = Mode::Voxel;
        editor.voxel_size = 3;
        let center = BlockPos::new(40, 40, 40);
        editor.fill_cells(center, Block(2));
        editor.end_stroke();

        assert_eq!(editor.history.undo.len(), 1);
        assert_eq!(editor.voxels.get(center), Block(2));
        assert_eq!(editor.voxels.get(BlockPos::new(41, 41, 41)), Block(2));

        editor.undo();

        assert_eq!(editor.voxels.get(center), Block::AIR);
        assert_eq!(editor.voxels.get(BlockPos::new(41, 41, 41)), Block::AIR);

        editor.redo();

        assert_eq!(editor.voxels.get(center), Block(2));
    }

    #[test]
    fn duplicates_and_resizes_keep_the_selection_valid() {
        let mut editor = open_editor("hall").unwrap();
        let count = editor.document.brush_count();
        editor.selected = Some(0);
        editor.duplicate_selection();

        assert_eq!(editor.document.brush_count(), count + 1);
        let copy = editor.selected.unwrap();
        let (min, max) = editor.document.brush_box(copy).unwrap();
        editor.resize_selection([min.x, min.y, min.z], [max.x + 2.0, max.y, max.z]);
        let (_, grown) = editor.document.brush_box(copy).unwrap();

        assert!((grown.x - (max.x + 2.0)).abs() < 1e-9);
    }

    #[test]
    fn brush_cells_cover_the_requested_size() {
        let (low, high) = brush_cells(BlockPos::new(0, 0, 0), 1);

        assert_eq!(low, high);

        let (low, high) = brush_cells(BlockPos::new(0, 0, 0), 4);

        assert_eq!(high.x - low.x + 1, 4);
    }

    #[test]
    fn number_labels_trim_noise() {
        assert_eq!(number_label(2.0), "2");
        assert_eq!(number_label(-0.0), "0");
        assert_eq!(number_label(0.5), "0.5");
        assert_eq!(number_label(1.25), "1.25");
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
