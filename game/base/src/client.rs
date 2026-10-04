use crate::anchor::Anchor;
use crate::demo::{self, CamMode, DemoFrame, DemoPlay, DemoSession, SlotEvent};
use crate::entities::context::FrameInfo;
use crate::entities::{EntityHandle, Player, ScriptedEntity};
use crate::input::Action;
use crate::movement::{self, NetPose, Prediction, UserCommand};
use crate::network::events::{
    EntityBones, EntityModel, EntityNetworked, EntityOwnership, EntitySnapshot, NetVar,
};
use crate::network::packet::{
    bundle_part, owned_payload, pack_bundles, split_unreliable, BundlePart, CONNECTION_TIMEOUT,
    KEEPALIVE_INTERVAL, MAX_DATAGRAM, STREAM_STATE,
};
use crate::network::usermessage::UserMsgReader;
use crate::network::wait_socket;
use crate::network::{
    take_unreliable, ClientToServer, EnqueueStatus, FromServer, NetSend, NetworkClient, PacketType,
    ReliableBody, ReliableChannel, ServerToClient, UnreliableAssembly, UnreliableInbox,
    OUTBOUND_CAP, RECV_BUDGET,
};
use crate::platform::{
    DeviceEvent, ElementState, Event, HostKind, KeyCode, MouseButton, MouseScrollDelta,
    PlatformHost, Touch, TouchPhase, WindowEvent,
};
use crate::r#enum::InputButtons;
use crate::script::engine::DrawCommand;
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
use crate::script::Realm;
use crate::state::GameState;
use crate::ui::backend;
use crate::ui::voxel::FlyCamera;
use crate::ui::window::Window;
use crate::ui::Color;
use core::net::SocketAddr;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::TcpStream;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const LOOK_SPEED: f32 = 0.0025 * (180.0 / std::f32::consts::PI);
const PAD_LOOK: f32 = 2.2;

struct TickIngress {
    tick: u64,
    part_count: u16,
    filled: u16,
    started: bool,
    parts: Vec<Option<Vec<EntitySnapshot>>>,
}

impl TickIngress {
    fn new() -> Self {
        Self {
            tick: 0,
            part_count: 0,
            filled: 0,
            started: false,
            parts: Vec::new(),
        }
    }

    fn push(
        &mut self,
        tick: u64,
        part: u16,
        part_count: u16,
        entities: Vec<EntitySnapshot>,
    ) -> Option<Vec<EntitySnapshot>> {
        if part_count == 0 || part >= part_count || part_count > 1024 {
            return None;
        }

        if self.started && self.part_count == 0 && tick <= self.tick {
            return None;
        }

        if self.started && tick < self.tick {
            return None;
        }

        if !self.started
            || self.tick != tick
            || self.part_count != part_count
            || self.parts.len() != part_count as usize
        {
            self.started = true;
            self.tick = tick;
            self.part_count = part_count;
            self.filled = 0;
            self.parts = vec![None; part_count as usize];
        }

        if self.parts[part as usize].is_none() {
            self.parts[part as usize] = Some(entities);
            self.filled = self.filled.saturating_add(1);
        }

        if self.filled != part_count {
            return None;
        }

        let mut all = Vec::new();
        for slot in self.parts.drain(..) {
            if let Some(batch) = slot {
                all.extend(batch);
            }
        }

        self.filled = 0;
        self.part_count = 0;

        Some(all)
    }
}

struct BuiltSnapshot {
    reset: bool,
    entities: Vec<EntitySnapshot>,
    networked: Vec<EntityNetworked>,
    owners: Vec<EntityOwnership>,
    models: Vec<EntityModel>,
    bones: Vec<EntityBones>,
}

struct SnapshotIngress {
    generation: u32,
    reset: bool,
    part_count: u16,
    filled: u16,
    parts: Vec<
        Option<(
            Vec<EntitySnapshot>,
            Vec<EntityNetworked>,
            Vec<EntityOwnership>,
            Vec<EntityModel>,
            Vec<EntityBones>,
        )>,
    >,
}

impl SnapshotIngress {
    fn new() -> Self {
        Self {
            generation: 0,
            reset: false,
            part_count: 0,
            filled: 0,
            parts: Vec::new(),
        }
    }

    fn push(
        &mut self,
        generation: u32,
        reset: bool,
        part: u16,
        part_count: u16,
        entities: Vec<EntitySnapshot>,
        networked: Vec<EntityNetworked>,
        owners: Vec<EntityOwnership>,
        models: Vec<EntityModel>,
        bones: Vec<EntityBones>,
    ) -> Option<BuiltSnapshot> {
        if part_count == 0 || part >= part_count || part_count > 1024 {
            return None;
        }

        if self.part_count != part_count
            || self.generation != generation
            || self.parts.len() != part_count as usize
        {
            self.generation = generation;
            self.reset = false;
            self.part_count = part_count;
            self.filled = 0;
            self.parts = vec![None; part_count as usize];
        }

        if part == 0 {
            self.reset = reset;
        }

        if self.parts[part as usize].is_none() {
            self.parts[part as usize] = Some((entities, networked, owners, models, bones));
            self.filled = self.filled.saturating_add(1);
        }

        if self.filled != part_count {
            return None;
        }

        let mut built_entities = Vec::new();
        let mut built_networked = Vec::new();
        let mut built_owners = Vec::new();
        let mut built_models = Vec::new();
        let mut built_bones = Vec::new();
        for slot in self.parts.drain(..) {
            if let Some((batch, networked, owners, models, bones)) = slot {
                built_entities.extend(batch);
                built_networked.extend(networked);
                built_owners.extend(owners);
                built_models.extend(models);
                built_bones.extend(bones);
            }
        }

        let built = BuiltSnapshot {
            reset: self.reset,
            entities: built_entities,
            networked: built_networked,
            owners: built_owners,
            models: built_models,
            bones: built_bones,
        };
        self.filled = 0;
        self.part_count = 0;

        Some(built)
    }
}

pub fn client_loop(
    mut game: GameState<FromServer, ClientToServer>,
    shutdown: Arc<AtomicBool>,
    resync: Arc<AtomicBool>,
) {
    demo::bind_realm(Realm::Client);
    let (host, mut held_window) = client_surface();
    let binds = game.binds.clone();

    crate::script::exec(&game.script_engine.lua, "menu.lua", "lua/menu/menu.luac");
    crate::script::exec(
        &game.script_engine.lua,
        "console.lua",
        "lua/menu/console.luac",
    );

    let mut last_frame = std::time::Instant::now();
    let mut fps_sample = std::time::Instant::now();
    let mut fps_frames = 0u32;
    let mut fps_label = String::from("0 fps");
    let mut accumulated_time = 0.0;
    let mut world_generation = 0u32;
    let mut hold_events = false;
    let mut held: VecDeque<ServerToClient> = VecDeque::new();
    let mut snapshot_ingress = SnapshotIngress::new();
    let mut tick_ingress = TickIngress::new();
    let mut scene_mesh = Vec::new();
    let mut scene_ranges = Vec::new();
    let mut scene_graphics = crate::world::MapGraphics::plain();
    let mut scene_world = u64::MAX;
    let mut scene_brushes = u64::MAX;
    let mut scene_revision = 0u64;
    let mut scene_anchor = Anchor::ZERO;
    let mut shown_mesh = Vec::new();
    let mut shown_ranges = Vec::new();
    let mut shown_revision = 0u64;
    let mut shown_key = (u64::MAX, u64::MAX, true);
    let mut voxel_mesh_time = Instant::now();
    let mut camera = FlyCamera::new();
    let mut prediction = Prediction::new();
    let mut brush_scale: Option<f64> = None;
    let mut remotes: HashMap<EntityHandle, VecDeque<NetPose>> = HashMap::new();
    let session_start = Instant::now();
    let mut captured = false;
    let mut keys = HashSet::new();
    let mut mouse = HashSet::new();
    let mut touches = Vec::new();
    let mut recorder: Option<DemoSession> = None;
    let mut play: Option<DemoPlay> = None;

    host.run(move |event, host, control| {
        control.poll();
        game.script_engine.pointer.lock().unwrap().captured = captured;

        #[cfg(target_os = "android")]
        {
            if let Event::Resumed = &event {
                if held_window.is_none() {
                    if let Some(surface) = host.surface() {
                        held_window = Some(backend::android_window(surface));
                    }
                }
            }

            if let Event::Suspended = &event {
                if let Ok(mut queue) = game.script_engine.render_queue.lock() {
                    queue.book.reset();
                    queue.commands.clear();
                }

                held_window = None;

                return;
            }

            if held_window.is_none() {
                return;
            }
        }

        let client_window = held_window.as_mut().unwrap();

        match event {
            Event::Window(event) => match event {
                WindowEvent::CloseRequested => {
                    shutdown.store(true, Ordering::Relaxed);
                    control.exit();
                }
                WindowEvent::Resized { width, height } => {
                    client_window.set_size(width, height);
                }
                WindowEvent::Focused(focused) => {
                    if !focused {
                        game.script_engine.pointer.lock().unwrap().release_all();
                        keys.clear();
                        mouse.clear();
                    }
                }
                WindowEvent::ModifiersChanged(mods) => {
                    let mut pointer = game.script_engine.pointer.lock().unwrap();
                    pointer.shift = mods.shift;
                    pointer.control = mods.control;
                    pointer.alt = mods.alt;
                    pointer.super_key = mods.super_key;
                }
                WindowEvent::KeyboardInput(input) => {
                    if let Some(code) = input.key_code {
                        let pressed = input.state == ElementState::Pressed;
                        game.script_engine
                            .pointer
                            .lock()
                            .unwrap()
                            .set_key(code, pressed);

                        let mut consumed = false;

                        if pressed {
                            let name = crate::script::libs::input::key_label(code);
                            let handled: Option<bool> =
                                game.run_hook("GuiKeyPressed", (name, input.repeat));
                            consumed = handled == Some(true);
                        }

                        if !consumed {
                            if code == KeyCode::BracketLeft && pressed {
                                game.send_reliable(ClientToServer::ScaleMaps { ratio: 0.5 });
                            } else if code == KeyCode::BracketRight && pressed {
                                game.send_reliable(ClientToServer::ScaleMaps { ratio: 2.0 });
                            } else if code == KeyCode::Escape && pressed {
                                captured = false;
                                mouse.clear();
                                host.set_cursor_grabbed(false);
                            } else if pressed {
                                keys.insert(code);
                            } else {
                                keys.remove(&code);
                            }
                        } else if !pressed {
                            keys.remove(&code);
                        }
                    }
                }
                WindowEvent::TextInput { text } => {
                    game.script_engine.pointer.lock().unwrap().push_text(&text);
                    let _: Option<bool> = game.run_hook("GuiText", text);
                }
                WindowEvent::MouseWheel { delta } => {
                    let (x, y) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => (x as f64, y as f64),
                        MouseScrollDelta::PixelDelta(x, y) => (x, y),
                    };
                    let mut pointer = game.script_engine.pointer.lock().unwrap();
                    pointer.wheel_x += x;
                    pointer.wheel_y += y;

                    if let Some(play) = play.as_mut() {
                        if play.cam == CamMode::Orbit {
                            play.orbit_dist = (play.orbit_dist - y * 0.35).clamp(1.2, 24.0);
                        }
                    }
                }
                WindowEvent::CursorMoved { x, y } => {
                    let mut pointer = game.script_engine.pointer.lock().unwrap();
                    pointer.x = x;
                    pointer.y = y;
                }
                WindowEvent::MouseInput { state, button } => {
                    let down = state == ElementState::Pressed;
                    {
                        let mut pointer = game.script_engine.pointer.lock().unwrap();
                        pointer.set_button(button, down);
                    }

                    if down {
                        let (x, y) = {
                            let pointer = game.script_engine.pointer.lock().unwrap();
                            (pointer.x, pointer.y)
                        };
                        let index = crate::script::libs::input::button_index(button);
                        let handled: Option<bool> =
                            game.run_hook("GuiMousePressed", (index, x, y));

                        if handled != Some(true) {
                            if captured {
                                mouse.insert(button);
                            }

                            if button == MouseButton::Left {
                                let blocked = game.script_engine.pointer.lock().unwrap().block_look;

                                if !blocked {
                                    captured = true;
                                    host.set_cursor_grabbed(true);
                                }
                            }
                        }
                    } else {
                        mouse.remove(&button);
                    }
                }
                WindowEvent::Touch(touch) => {
                    let width = host.size().0.max(1) as f64;
                    apply_touch(
                        &mut touches,
                        &touch,
                        width,
                        &mut camera,
                        &mut prediction.look,
                        !prediction.local.is_null(),
                    );
                }
                WindowEvent::RedrawRequested => {
                    let (width, height) = host.size();
                    let aspect = width as f32 / height.max(1) as f32;
                    let world_revision = game.voxel_world.revision();
                    let brush_revision = game.brush_world.revision();
                    let skin_alpha = if game.tick_interval > 0.0 {
                        (accumulated_time / game.tick_interval).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };

                    if play.is_none()
                        && !prediction.local.is_null()
                        && game.entities.is_valid(prediction.local)
                    {
                        let origin = prediction
                            .view_origin(skin_alpha)
                            .or_else(|| body_origin(&game, prediction.local));

                        if let Some(origin) = origin {
                            let body = player_body(&game, prediction.local);
                            let eye = if prediction.previous().contains(InputButtons::IN_DUCK)
                            {
                                body.view_offset_ducked.z
                            } else {
                                body.view_offset.z
                            };
                            place_camera(&mut camera, origin, prediction.look, eye);
                        }
                    }

                    let camera_moved = scene_anchor.drifted(camera.x, camera.y, camera.z);

                    if camera_moved {
                        scene_anchor = Anchor::new(camera.x, camera.y, camera.z);
                    }

                    if scene_brushes != brush_revision {
                        scene_graphics = game.brush_world.graphics().clone();
                    }

                    let more_meshes = if scene_world != world_revision {
                        game.voxel_world.build_meshes(4)
                    } else {
                        false
                    };
                    let voxels_due = scene_world != world_revision
                        && (!more_meshes
                            || voxel_mesh_time.elapsed() >= Duration::from_millis(80));

                    if camera_moved || scene_brushes != brush_revision || voxels_due
                    {
                        let origin = scene_anchor.to_vec();
                        scene_mesh = game.voxel_world.assembled_mesh(origin);
                        let brush = game.brush_world.draw_at(origin);
                        let base = (scene_mesh.len() / crate::world::STRIDE) as u32;
                        scene_ranges.clear();

                        if base > 0 {
                            scene_ranges.push(crate::world::SurfaceRange {
                                first: 0,
                                count: base,
                                material: crate::world::MATERIAL_NONE,
                                cubemap: crate::world::CUBEMAP_NONE,
                                pass: crate::world::PASS_OPAQUE,
                            });
                        }

                        for range in brush.ranges {
                            scene_ranges.push(crate::world::SurfaceRange {
                                first: range.first + base,
                                count: range.count,
                                material: range.material,
                                cubemap: range.cubemap,
                                pass: range.pass,
                            });
                        }

                        scene_mesh.extend(brush.vertices);

                        if !more_meshes {
                            scene_world = world_revision;
                        }

                        voxel_mesh_time = Instant::now();
                        scene_brushes = brush_revision;
                        scene_revision = scene_revision.wrapping_add(1);
                    }

                    client_window.begin_frame(0.53, 0.71, 0.85);
                    let draw_scale = (game.voxel_world.scale() as f32).max(game.brush_world.scale() as f32);
                    let mut scene = camera.scene_at(aspect, draw_scale, scene_anchor);
                    scene.time = accumulated_time as f32;
                    let nav_key = (scene_revision, game.nav.state.draw_gen, game.nav.state.show);

                    if nav_key != shown_key {
                        shown_mesh.clone_from(&scene_mesh);
                        shown_ranges.clone_from(&scene_ranges);

                        if game.nav.state.show {
                            let nav_verts =
                                crate::world::nav::debug_vertices(&game.nav.state, scene_anchor);

                            if !nav_verts.is_empty() {
                                let first = (shown_mesh.len() / crate::world::STRIDE) as u32;
                                let count = (nav_verts.len() / crate::world::STRIDE) as u32;
                                shown_mesh.extend(nav_verts);
                                shown_ranges.push(crate::world::SurfaceRange {
                                    first,
                                    count,
                                    material: crate::world::MATERIAL_NONE,
                                    cubemap: crate::world::CUBEMAP_NONE,
                                    pass: crate::world::PASS_OPAQUE,
                                });
                            }
                        }

                        shown_key = nav_key;
                        shown_revision = shown_revision.wrapping_add(1);
                    }

                    client_window.draw_colored_mesh(
                        &shown_mesh,
                        &shown_ranges,
                        &scene_graphics,
                        shown_revision,
                        &scene,
                    );
                    let local_time = game.tick_count as f64 + skin_alpha;
                    let (forward, up) = (scene.forward, scene.up);
                    let cull = crate::anim::cull_from(
                        scene.eye,
                        forward,
                        up,
                        scene.fov_y,
                        scene.aspect,
                        scene.far,
                    );
                    let body = player_body(&game, prediction.local);
                    let view_eye = if prediction.previous().contains(InputButtons::IN_DUCK) {
                        body.view_offset_ducked.z
                    } else {
                        body.view_offset.z
                    };
                    let (world_skin, view_skin) = game.skin_batch(
                        prediction.local,
                        prediction.view_origin(skin_alpha),
                        Some(prediction.look),
                        Some(view_eye),
                        local_time,
                        cull,
                        scene_anchor,
                    );
                    client_window.draw_skinned(&world_skin, &scene);

                    if !view_skin.groups.is_empty() {
                        client_window.clear_depth();
                        client_window.draw_skinned(&view_skin, &scene);
                    }

                    {
                        let (width, height) = host.size();
                        let mut queue = game.script_engine.render_queue.lock().unwrap();
                        queue.width = width;
                        queue.height = height;
                    }

                    game.script_engine.flush_webviews();
                    let _: () = game.run_hook("MenuPaint", ());

                    let draw_commands = {
                        let mut q = game.script_engine.render_queue.lock().unwrap();
                        std::mem::take(&mut q.commands)
                    };

                    //todo: optimize
                    for cmd in draw_commands {
                        match cmd {
                            DrawCommand::Rect {
                                x,
                                y,
                                w,
                                h,
                                color,
                                texture,
                                pipeline,
                                sampler,
                            } => {
                                if texture == 0 && pipeline == 0 && !client_window.target_bound() {
                                    client_window.draw_rectangle(x, y, w, h, color);
                                } else {
                                    client_window.draw_sprite(
                                        x, y, w, h, color, texture, pipeline, sampler,
                                    );
                                }
                            }
                            DrawCommand::OutlinedRect {
                                x,
                                y,
                                w,
                                h,
                                thickness,
                                color,
                                texture,
                                pipeline,
                                sampler,
                            } => {
                                if texture == 0 && pipeline == 0 && !client_window.target_bound() {
                                    client_window.draw_outlined_rectangle(
                                        x, y, w, h, thickness, color,
                                    );
                                } else {
                                    client_window.draw_sprite(
                                        x,
                                        y,
                                        w,
                                        thickness,
                                        color,
                                        texture,
                                        pipeline,
                                        sampler,
                                    );
                                    client_window.draw_sprite(
                                        x,
                                        y + h - thickness,
                                        w,
                                        thickness,
                                        color,
                                        texture,
                                        pipeline,
                                        sampler,
                                    );
                                    client_window.draw_sprite(
                                        x,
                                        y + thickness,
                                        thickness,
                                        h - thickness * 2.0,
                                        color,
                                        texture,
                                        pipeline,
                                        sampler,
                                    );
                                    client_window.draw_sprite(
                                        x + w - thickness,
                                        y + thickness,
                                        thickness,
                                        h - thickness * 2.0,
                                        color,
                                        texture,
                                        pipeline,
                                        sampler,
                                    );
                                }
                            }
                            DrawCommand::Text {
                                font,
                                text,
                                x,
                                y,
                                scale,
                                color,
                                texture,
                                pipeline,
                                sampler,
                            } => {
                                let text = text.to_str().unwrap().to_owned();

                                if texture == 0 && pipeline == 0 && !client_window.target_bound() {
                                    client_window.draw_text(
                                        &font.to_str().unwrap().to_owned(),
                                        &text,
                                        x,
                                        y,
                                        scale,
                                        color,
                                    );
                                } else {
                                    client_window.draw_text_user(
                                        &text, x, y, scale, color, texture, pipeline, sampler,
                                    );
                                }
                            }
                            DrawCommand::CreateShader { id, source } => {
                                client_window.create_shader(id, &source);
                            }
                            DrawCommand::CreateTexture { id, path } => {
                                client_window.create_texture(id, &path);
                            }
                            DrawCommand::CreateImage {
                                id,
                                width,
                                height,
                                bytes,
                            } => {
                                client_window.create_rgba(id, width, height, bytes);
                            }
                            DrawCommand::CreateMaterial { id, name } => {
                                client_window.create_material(id, &name);
                            }
                            DrawCommand::CreateTarget { id, width, height } => {
                                client_window.create_target(id, width, height);
                            }
                            DrawCommand::CreateBuffer { id, bytes } => {
                                client_window.create_buffer(id, &bytes);
                            }
                            DrawCommand::CreateSampler { id, linear, repeat } => {
                                client_window.create_sampler(id, linear, repeat);
                            }
                            DrawCommand::CreatePipeline { id, shader, screen } => {
                                client_window.create_pipeline(id, shader, screen);
                            }
                            DrawCommand::CreateMesh { id, verts, screen } => {
                                client_window.create_mesh(id, &verts, screen);
                            }
                            DrawCommand::Free { id } => {
                                client_window.free_gpu(id);
                                game.script_engine
                                    .render_queue
                                    .lock()
                                    .unwrap()
                                    .book
                                    .recycle(id);
                            }
                            DrawCommand::DrawMesh {
                                mesh,
                                pipeline,
                                texture,
                                sampler,
                            } => {
                                client_window.draw_mesh(mesh, pipeline, texture, sampler, &scene);
                            }
                            DrawCommand::SetTarget { id } => {
                                client_window.set_target(id);
                            }
                            DrawCommand::UpdateBuffer { id, bytes } => {
                                client_window.update_buffer(id, &bytes);
                            }
                            DrawCommand::UpdateMesh { id, verts } => {
                                client_window.update_mesh(id, &verts);
                            }
                            DrawCommand::UpdateTexture { id, path } => {
                                client_window.update_texture(id, &path);
                            }
                            DrawCommand::UpdateImage {
                                id,
                                width,
                                height,
                                bytes,
                            } => {
                                client_window.update_rgba(id, width, height, bytes);
                            }
                            DrawCommand::UpdateTarget { id, width, height } => {
                                client_window.update_target(id, width, height);
                            }
                            DrawCommand::SetScissor { rect } => {
                                client_window.set_scissor(rect);
                            }
                        }
                    }

                    fps_frames += 1;
                    let sample = fps_sample.elapsed().as_secs_f64();

                    if sample >= 0.25 {
                        let fps = (fps_frames as f64 / sample).round() as u32;
                        fps_label = format!("{fps} fps");
                        fps_frames = 0;
                        fps_sample = std::time::Instant::now();
                    }

                    client_window.draw_rectangle(
                        8.0,
                        8.0,
                        96.0,
                        24.0,
                        Color::ColorRGBA {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 160,
                        },
                    );
                    client_window.draw_text(
                        "default",
                        &fps_label,
                        14.0,
                        10.0,
                        16.0,
                        Color::ColorRGBA {
                            r: 255,
                            g: 255,
                            b: 255,
                            a: 255,
                        },
                    );
                    client_window.render_text();
                    client_window.present();
                    game.script_engine.pointer.lock().unwrap().end_frame();
                }
            },
            Event::Device(DeviceEvent::MouseMotion { delta }) => {
                let blocked = game.script_engine.pointer.lock().unwrap().block_look;

                if blocked {
                    if captured {
                        captured = false;
                        keys.clear();
                        mouse.clear();
                        host.set_cursor_grabbed(false);
                    }
                } else if let Some(play) = play.as_mut() {
                    if captured && !client_window.vr_input().active {
                        if play.cam == CamMode::Free {
                            camera.look(delta.0 as f32, delta.1 as f32);
                        } else if play.cam == CamMode::Orbit {
                            play.orbit_yaw += delta.0 as f32 * 0.0025;
                            play.orbit_pitch =
                                (play.orbit_pitch - delta.1 as f32 * 0.0025).clamp(-1.2, 1.2);
                        }
                    }
                } else if captured && !client_window.vr_input().active {
                    if prediction.local.is_null() {
                        camera.look(delta.0 as f32, delta.1 as f32);
                    } else {
                        prediction.look.y += delta.0 as f32 * LOOK_SPEED;
                        prediction.look.p -= delta.1 as f32 * LOOK_SPEED;
                        prediction.look.p = prediction.look.p.clamp(-89.0, 89.0);
                    }
                }
            }
            Event::AboutToWait => {
                let blocked = game.script_engine.pointer.lock().unwrap().block_look;

                if blocked && captured {
                    captured = false;
                    keys.clear();
                    mouse.clear();
                    host.set_cursor_grabbed(false);
                }

                let now = std::time::Instant::now();
                let dt = now.duration_since(last_frame).as_secs_f64();
                last_frame = now;
                let frame_dt = (dt as f32).min(0.1);

                crate::console::poll_autocomplete(Realm::Client, &game.script_engine.lua);
                poll_client_noclip(&mut game, &prediction);
                poll_client_demo(
                    &mut game,
                    &mut recorder,
                    &mut play,
                    &mut prediction,
                    &mut remotes,
                    &mut camera,
                    &mut brush_scale,
                    &mut tick_ingress,
                    &mut snapshot_ingress,
                    &mut hold_events,
                    &mut held,
                    &resync,
                    session_start,
                );

                if play.is_some() {
                    while game.script_engine.poll_usermessage().is_some() {}

                    while game.network_receiver.try_recv().is_ok() {}
                } else {
                while let Some((hash, data)) = game.script_engine.poll_usermessage() {
                    game.send_reliable(ClientToServer::UserMessage { hash, data });
                }

                while let Ok(net_event) = game.network_receiver.try_recv() {
                    match net_event {
                        FromServer::Connected { generation } => {
                            log::info!("[cl] link up");
                            if world_generation != generation {
                                world_generation = generation;
                                game.entities.clear();
                                game.sync_entities();
                                prediction.clear();
                                remotes.clear();
                                game.voxel_world.clear();
                                hold_events = true;
                                held.clear();
                                snapshot_ingress = SnapshotIngress::new();
                            }

                            continue;
                        }
                        FromServer::Disconnected => {
                            log::info!("[cl] link lost");

                            continue;
                        }
                        FromServer::Message(message) => {
                            if let ServerToClient::WorldSnapshot {
                                generation,
                                reset,
                                part,
                                parts,
                                entities,
                                networked,
                                owners,
                                models,
                                bones,
                            } = message
                            {
                                log::info!(
                                    "[cl] WorldSnapshot(gen={generation} reset={reset} part={part}/{parts} ents={} networked={} owners={} models={} bones={})",
                                    entities.len(),
                                    networked.len(),
                                    owners.len(),
                                    models.len(),
                                    bones.len()
                                );

                                if generation == world_generation {
                                    if let Some(built) = snapshot_ingress
                                        .push(
                                            generation, reset, part, parts, entities, networked,
                                            owners, models, bones,
                                        )
                                    {
                                        if built.reset {
                                            game.entities.clear();
                                            game.sync_entities();
                                            prediction.clear();
                                            remotes.clear();
                                        }

                                        let now = session_start.elapsed().as_secs_f64();
                                        let interval = game.tick_interval;
                                        let mut states: HashMap<EntityHandle, Vec<NetVar>> = built
                                            .networked
                                            .into_iter()
                                            .map(|entity| (entity.handle, entity.vars))
                                            .collect();
                                        let owners: HashMap<EntityHandle, EntityHandle> = built
                                            .owners
                                            .into_iter()
                                            .map(|ownership| (ownership.handle, ownership.owner))
                                            .collect();

                                        for entity in built.entities {
                                            let vars = states.remove(&entity.handle).unwrap_or_default();
                                            let owner = owners
                                                .get(&entity.handle)
                                                .copied()
                                                .unwrap_or(EntityHandle::NULL);
                                            log::info!(
                                                "[cl] snapshot spawn {:?} class={} hp={} pos=({:.2},{:.2},{:.2})",
                                                entity.handle,
                                                entity.class_hash,
                                                entity.health,
                                                entity.position.x,
                                                entity.position.y,
                                                entity.position.z
                                            );
                                            apply_spawn(
                                                &mut game,
                                                &mut remotes,
                                                now,
                                                interval,
                                                entity,
                                                owner,
                                                prediction.local,
                                                &vars,
                                            );
                                        }

                                        for model in built.models {
                                            apply_anim_model(&mut game, model);
                                        }

                                        for bones in built.bones {
                                            apply_anim_bones(&mut game, bones);
                                        }

                                        hold_events = false;

                                        if let Some(session) = recorder.as_mut() {
                                            demo::remember_players(&mut session.players, &game);
                                            let mut listed = session.players.clone();
                                            let mut player_idx = 0;

                                            while player_idx < listed.len() {
                                                if listed[player_idx].handle == prediction.local {
                                                    listed[player_idx].buttons =
                                                        prediction.previous();
                                                }

                                                player_idx += 1;
                                            }

                                            let tick = game.tick_count;
                                            let local = prediction.local;
                                            let shot = demo::capture_world(
                                                &mut game,
                                                tick,
                                                local,
                                                &listed,
                                            );
                                            session.write_mark(&DemoFrame::Keyframe(shot));
                                        }

                                        while let Some(waiting) = held.pop_front() {
                                            apply_server_event(
                                                &mut game,
                                                &mut recorder,
                                                &mut tick_ingress,
                                                &mut prediction,
                                                &mut remotes,
                                                &mut camera,
                                                &mut brush_scale,
                                                session_start.elapsed().as_secs_f64(),
                                                waiting,
                                            );
                                        }
                                    } else {
                                        hold_events = true;
                                    }
                                }

                                continue;
                            }

                            if hold_events {
                                held.push_back(message);

                                continue;
                            }

                            apply_server_event(
                                &mut game,
                                &mut recorder,
                                &mut tick_ingress,
                                &mut prediction,
                                &mut remotes,
                                &mut camera,
                                &mut brush_scale,
                                session_start.elapsed().as_secs_f64(),
                                message,
                            );

                            continue;
                        }
                    }
                }

                while let Some((hash, data)) = game.script_engine.poll_usermessage() {
                    game.send_reliable(ClientToServer::UserMessage { hash, data });
                }
                }

                let speed = game.voxel_world.scale() as f32 * 14.0;
                let (touch_forward, touch_right) = touch_wish(&touches);
                let pad = {
                    let mut cache = game.pads.lock().unwrap();

                    for idx in 0..crate::platform::PAD_COUNT {
                        let deadzones = crate::console::pad_deadzones(&game.cvars, idx);
                        cache.set(idx, host.gamepad(idx, deadzones));
                    }

                    cache.get(0)
                };
                let (mut forward, mut right, up, buttons) = {
                    let binds = binds.lock().unwrap();
                    let (axis_forward, axis_right) = binds.axis_held(&keys, &mouse, pad.buttons);
                    let buttons = binds.buttons_held(&keys, &mouse, pad.buttons);
                    let up = binds.action_held(Action::Jump, &keys, &mouse, pad.buttons) as i32
                        as f32
                        - binds.action_held(Action::Sprint, &keys, &mouse, pad.buttons) as i32
                            as f32;

                    (
                        axis_forward + touch_forward,
                        axis_right + touch_right,
                        up,
                        buttons,
                    )
                };
                let vr = client_window.vr_input();

                if play.is_some() {
                    if play.as_ref().map(|demo| demo.cam) == Some(CamMode::Free) {
                        let wish_forward = (forward + pad.forward).clamp(-1.0, 1.0);
                        let wish_right = (right + pad.right).clamp(-1.0, 1.0);
                        camera.fly(wish_forward, wish_right, up, frame_dt, speed);
                    }

                    drive_demo(
                        &mut play,
                        &mut game,
                        &mut prediction,
                        &mut remotes,
                        &mut camera,
                        &mut brush_scale,
                        &mut tick_ingress,
                        session_start,
                        dt,
                    );

                    if play.as_ref().map(|demo| demo.cam) != Some(CamMode::Free) {
                        if let Some(demo) = play.as_ref() {
                            place_demo_camera(&mut camera, &game, demo, &prediction);
                        }
                    }
                } else if prediction.arm_look {
                    prediction.look.p = camera.pitch.to_degrees();
                    prediction.look.y = camera.yaw.to_degrees();
                    prediction.look.r = 0.0;
                    prediction.arm_look = false;
                }

                if play.is_none() {
                    if vr.active {
                        let turn = vr.turn * frame_dt * 1.5;

                        if prediction.local.is_null() {
                            camera.yaw -= turn;
                        } else {
                            prediction.look.y -= turn.to_degrees();
                        }

                        forward += vr.move_y;
                        right += vr.move_x;
                    }

                    if !vr.active {
                        let yaw = pad.look_x * PAD_LOOK * frame_dt;
                        let pitch = pad.look_y * PAD_LOOK * frame_dt;

                        if prediction.local.is_null() {
                            camera.yaw += yaw;
                            camera.pitch = (camera.pitch + pitch).clamp(-1.5, 1.5);
                        } else {
                            prediction.look.y += yaw.to_degrees();
                            prediction.look.p =
                                (prediction.look.p + pitch.to_degrees()).clamp(-89.0, 89.0);
                        }
                    }

                    forward += pad.forward;
                    right += pad.right;

                    forward = forward.clamp(-1.0, 1.0);
                    right = right.clamp(-1.0, 1.0);

                    let possessed =
                        !prediction.local.is_null() && game.entities.is_valid(prediction.local);

                    if !possessed && vr.active {
                        camera.fly_facing(camera.yaw + vr.yaw, forward, right, up, frame_dt, speed);
                    } else if !possessed {
                        camera.fly(forward, right, up, frame_dt, speed);
                    }

                    accumulated_time += dt;

                    while accumulated_time >= game.tick_interval {
                        accumulated_time -= game.tick_interval;
                        game.cur_time += game.tick_interval;
                        game.frame_time = game.tick_interval;
                        game.tick_count += 1;
                        game.entities.set_frame(FrameInfo {
                            dt: game.tick_interval,
                            cur_time: game.cur_time,
                            tick_count: game.tick_count,
                        });
                        game.entities.tick_all();
                        game.think_entities();

                        if possessed {
                            predict_tick(
                                &mut game,
                                &mut recorder,
                                &mut prediction,
                                buttons,
                                forward,
                                right,
                                vr.yaw,
                            );
                        }

                        if let Some(session) = recorder.as_mut() {
                            if game.cur_time >= session.next_shot {
                                demo::remember_players(&mut session.players, &game);
                                let mut listed = session.players.clone();
                                let mut player_idx = 0;

                                while player_idx < listed.len() {
                                    if listed[player_idx].handle == prediction.local {
                                        listed[player_idx].buttons = prediction.previous();
                                    }

                                    player_idx += 1;
                                }

                                let tick = game.tick_count;
                                let local = prediction.local;
                                let shot = demo::capture_world(
                                    &mut game,
                                    tick,
                                    local,
                                    &listed,
                                );
                                session.write_mark(&DemoFrame::Keyframe(shot));

                                while session.next_shot <= game.cur_time {
                                    session.next_shot += demo::SHOT_INTERVAL;
                                }
                            }
                        }

                        if recorder.as_ref().is_some_and(|session| session.dead) {
                            log::warn!("[demo] recording stopped");
                            recorder = None;
                        }
                    }

                    if possessed {
                        let alpha = if game.tick_interval > 0.0 {
                            (accumulated_time / game.tick_interval).clamp(0.0, 1.0)
                        } else {
                            1.0
                        };
                        let origin = prediction
                            .view_origin(alpha)
                            .or_else(|| body_origin(&game, prediction.local));

                        if let Some(origin) = origin {
                            let body = player_body(&game, prediction.local);
                            let eye = if buttons.contains(InputButtons::IN_DUCK) {
                                body.view_offset_ducked.z
                            } else {
                                body.view_offset.z
                            };
                            place_camera(&mut camera, origin, prediction.look, eye);
                        }
                    }
                }

                let interval = game.tick_interval;
                present_remotes(
                    &mut game,
                    &mut remotes,
                    prediction.local,
                    session_start.elapsed().as_secs_f64(),
                    interval,
                );
                let sound_dt = if play.as_ref().is_some_and(|demo| demo.paused) {
                    0.0
                } else {
                    frame_dt
                };
                game.update_sound(
                    camera.x,
                    camera.y,
                    camera.z,
                    camera.yaw,
                    camera.pitch,
                    sound_dt,
                );

                host.request_redraw();
            }
            _ => (),
        }
    });
}

fn replay_command(
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    cmd: UserCommand,
    apply_look: bool,
) {
    if !game.entities.is_valid(prediction.local) {
        return;
    }

    let from = body_origin(game, prediction.local).unwrap_or(Vector3::new(0.0, 0.0, 0.0));
    let prev = prediction.previous();

    if !step_player(game, prediction.local, &cmd, prev) {
        return;
    }

    let to = body_origin(game, prediction.local).unwrap_or(from);
    prediction.note_step(from, to);
    prediction.push(cmd);

    if apply_look {
        prediction.look = cmd.view;
    }

    game.run_predicted(prediction.local, &cmd, true);
}

fn step_player(
    game: &mut GameState<FromServer, ClientToServer>,
    handle: EntityHandle,
    cmd: &UserCommand,
    prev: InputButtons,
) -> bool {
    let (mut position, mut velocity, mut angles, body) = {
        let Some(entity) = game.entities.get(handle) else {
            return false;
        };

        let base = entity.base();
        let body = entity
            .player_body()
            .copied()
            .unwrap_or_default();

        (base.position, base.velocity, base.angles, body)
    };
    let dt = game.tick_interval;
    let gravity = movement::gravity(&game.cvars);
    let root = game
        .entities
        .get(handle)
        .map(|entity| entity.base().anim)
        .and_then(|playback| game.anims.root_motion(&playback, angles.y, cmd.tick, dt));
    movement::step(
        &mut position,
        &mut velocity,
        &mut angles,
        cmd,
        prev,
        dt,
        gravity,
        &game.brush_world,
        &game.voxel_world,
        root,
        &body,
    );

    let Some(entity) = game.entities.get_mut(handle) else {
        return false;
    };

    let base = entity.base_mut();
    base.position = position;
    base.velocity = velocity;
    base.angles = angles;

    true
}

fn poll_client_noclip(
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &Prediction,
) {
    if !movement::take_client_noclip() {
        return;
    }

    let handle = prediction.local;

    if handle.is_null() {
        return;
    }

    let Some(entity) = game.entities.get_mut(handle) else {
        return;
    };

    let Some(body) = entity.player_body_mut() else {
        return;
    };

    body.noclip = !body.noclip;
    log::info!("[cl] noclip={}", body.noclip);
}

fn poll_client_demo(
    game: &mut GameState<FromServer, ClientToServer>,
    recorder: &mut Option<DemoSession>,
    play: &mut Option<DemoPlay>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    camera: &mut FlyCamera,
    brush_scale: &mut Option<f64>,
    tick_ingress: &mut TickIngress,
    snapshot_ingress: &mut SnapshotIngress,
    hold_events: &mut bool,
    held: &mut VecDeque<ServerToClient>,
    resync: &AtomicBool,
    session_start: std::time::Instant,
) {
    let commands = demo::drain(Realm::Client);
    let mut idx = 0;

    while idx < commands.len() {
        match commands[idx].clone() {
            demo::DemoCommand::Record { name } => {
                if recorder.is_some() || play.is_some() {
                    log::warn!("[demo] busy");
                } else {
                    let rate = (1.0 / game.tick_interval).round() as u32;
                    let header = demo::DemoHeader {
                        kind: demo::KIND_CLIENT,
                        map_name: game.map_name.clone(),
                        tickrate: rate,
                        map_scale: game.brush_world.scale(),
                        voxel_scale: game.voxel_world.scale(),
                    };

                    match DemoSession::create(&name, header) {
                        Ok(mut session) => {
                            demo::remember_players(&mut session.players, game);
                            let mut listed = session.players.clone();
                            let mut player_idx = 0;

                            while player_idx < listed.len() {
                                if listed[player_idx].handle == prediction.local {
                                    listed[player_idx].buttons = prediction.previous();
                                }

                                player_idx += 1;
                            }

                            let tick = game.tick_count;
                            let local = prediction.local;
                            let shot = demo::capture_world(game, tick, local, &listed);
                            session.write_mark(&demo::DemoFrame::Keyframe(shot));
                            session.next_shot = game.cur_time + demo::SHOT_INTERVAL;
                            log::info!("[demo] recording {name}");
                            *recorder = Some(session);
                        }
                        Err(err) => log::warn!("[demo] {err}"),
                    }
                }
            }
            demo::DemoCommand::Stop => {
                if recorder.take().is_some() {
                    log::info!("[demo] stopped");
                } else if play.take().is_some() {
                    end_playback(
                        game,
                        prediction,
                        remotes,
                        tick_ingress,
                        snapshot_ingress,
                        hold_events,
                        held,
                        resync,
                    );
                } else {
                    log::warn!("[demo] not recording");
                }
            }
            demo::DemoCommand::Play { name } => {
                if recorder.is_some() || play.is_some() {
                    log::warn!("[demo] busy");
                } else {
                    match DemoPlay::open(&name) {
                        Ok(mut demo_play) => {
                            let header = demo_play.reader.header().clone();
                            let rate = (1.0 / game.tick_interval).round() as u32;
                            demo::warn_header(
                                &header,
                                &game.map_name,
                                rate,
                                game.brush_world.scale(),
                                game.voxel_world.scale(),
                            );

                            if land_demo(
                                &mut demo_play,
                                game,
                                prediction,
                                remotes,
                                camera,
                                brush_scale,
                                tick_ingress,
                                session_start,
                                0,
                                false,
                            ) {
                                log::info!("[demo] playing {name}");
                                *play = Some(demo_play);
                            } else {
                                log::warn!("[demo] {name} has no frames");
                            }
                        }
                        Err(err) => log::warn!("[demo] {err}"),
                    }
                }
            }
            demo::DemoCommand::Pause => {
                if let Some(demo_play) = play.as_mut() {
                    demo_play.paused = !demo_play.paused;
                }
            }
            demo::DemoCommand::Timescale { scale } => {
                if let Some(demo_play) = play.as_mut() {
                    demo_play.timescale = scale;
                }
            }
            demo::DemoCommand::Seek { tick } => {
                if let Some(demo_play) = play.as_mut() {
                    if !land_demo(
                        demo_play,
                        game,
                        prediction,
                        remotes,
                        camera,
                        brush_scale,
                        tick_ingress,
                        session_start,
                        tick,
                        true,
                    ) {
                        log::warn!("[demo] seek failed");
                    }
                }
            }
            demo::DemoCommand::Loop { enabled } => {
                if let Some(demo_play) = play.as_mut() {
                    demo_play.loop_demo = enabled;
                }
            }
            demo::DemoCommand::Cam { mode } => {
                if let Some(demo_play) = play.as_mut() {
                    demo_play.cam = mode;
                }
            }
            demo::DemoCommand::View { index } => {
                if let Some(demo_play) = play.as_mut() {
                    demo_play.view_index = index;
                    demo_play.sync_watched();
                }
            }
        }

        idx += 1;
    }
}

fn end_playback(
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    tick_ingress: &mut TickIngress,
    snapshot_ingress: &mut SnapshotIngress,
    hold_events: &mut bool,
    held: &mut VecDeque<ServerToClient>,
    resync: &AtomicBool,
) {
    game.entities.clear();
    game.sync_entities();
    prediction.clear();
    game.set_local_player(EntityHandle::NULL);
    remotes.clear();
    *tick_ingress = TickIngress::new();
    *snapshot_ingress = SnapshotIngress::new();
    held.clear();
    *hold_events = true;
    game.voxel_world.clear();
    resync.store(true, Ordering::Relaxed);
    log::info!("[demo] playback stopped");
}

fn land_demo(
    play: &mut DemoPlay,
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    camera: &mut FlyCamera,
    brush_scale: &mut Option<f64>,
    tick_ingress: &mut TickIngress,
    session_start: std::time::Instant,
    target: u64,
    catch_up: bool,
) -> bool {
    let Some(mark) = play.reader.mark_for(target) else {
        return false;
    };

    if let Err(err) = play.reader.seek_to(mark.offset) {
        log::warn!("[demo] {err}");

        return false;
    }

    let frame = match play.reader.next_frame() {
        Ok(Some(frame)) => frame,
        Ok(None) => return false,
        Err(err) => {
            log::warn!("[demo] {err}");

            return false;
        }
    };
    let Some(shot) = frame.shot().cloned() else {
        return false;
    };
    let map_name = play.reader.header().map_name.clone();
    let keep_remotes = play.kind() == demo::KIND_CLIENT;
    apply_world_shot(
        game,
        prediction,
        remotes,
        camera,
        brush_scale,
        tick_ingress,
        &map_name,
        &shot,
        session_start.elapsed().as_secs_f64(),
        keep_remotes,
    );
    let prefer = if play.watched.is_null() {
        shot.local
    } else {
        play.watched
    };
    play.adopt_players(&shot.players, prefer);
    play.tick = shot.tick;
    game.tick_count = shot.tick;
    game.cur_time = shot.tick as f64 * game.tick_interval;
    let through = if catch_up { target } else { play.tick };
    let _restarted = pump_frames(
        play,
        game,
        prediction,
        remotes,
        camera,
        brush_scale,
        tick_ingress,
        session_start,
        through,
    );

    if catch_up {
        play.tick = target;
        game.tick_count = target;
        game.cur_time = target as f64 * game.tick_interval;
    }

    play.acc = 0.0;

    true
}

fn drive_demo(
    play: &mut Option<DemoPlay>,
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    camera: &mut FlyCamera,
    brush_scale: &mut Option<f64>,
    tick_ingress: &mut TickIngress,
    session_start: std::time::Instant,
    dt: f64,
) {
    let Some(demo_play) = play.as_mut() else {
        return;
    };

    if demo_play.paused || demo_play.timescale <= 0.0 {
        return;
    }

    let interval = game.tick_interval;
    demo_play.acc += dt * demo_play.timescale;
    let mut steps = 0;

    while demo_play.acc >= interval && steps < 32 {
        match demo_play.reader.peek_tick() {
            Ok(None) => {
                if demo_play.loop_demo {
                    let _ = restart_demo(
                        demo_play,
                        game,
                        prediction,
                        remotes,
                        camera,
                        brush_scale,
                        tick_ingress,
                        session_start,
                    );
                }

                demo_play.acc = 0.0;

                return;
            }
            Err(err) => {
                log::warn!("[demo] {err}");
                demo_play.acc = 0.0;

                return;
            }
            Ok(Some(_)) => {}
        }

        demo_play.acc -= interval;
        demo_play.tick = demo_play.tick.saturating_add(1);
        game.tick_count = demo_play.tick;
        game.cur_time = demo_play.tick as f64 * interval;
        game.frame_time = interval;
        game.entities.set_frame(FrameInfo {
            dt: interval,
            cur_time: game.cur_time,
            tick_count: game.tick_count,
        });
        game.entities.tick_all();
        game.think_entities();
        let through = demo_play.tick;
        let restarted = pump_frames(
            demo_play,
            game,
            prediction,
            remotes,
            camera,
            brush_scale,
            tick_ingress,
            session_start,
            through,
        );
        steps += 1;

        if restarted {
            demo_play.acc = 0.0;

            return;
        }
    }
}

fn restart_demo(
    play: &mut DemoPlay,
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    camera: &mut FlyCamera,
    brush_scale: &mut Option<f64>,
    tick_ingress: &mut TickIngress,
    session_start: std::time::Instant,
) -> bool {
    let Some(mark) = play.reader.index().first().copied() else {
        return false;
    };

    if let Err(err) = play.reader.seek_to(mark.offset) {
        log::warn!("[demo] {err}");

        return false;
    }

    let frame = match play.reader.next_frame() {
        Ok(Some(frame)) => frame,
        Ok(None) => return false,
        Err(err) => {
            log::warn!("[demo] {err}");

            return false;
        }
    };
    let Some(shot) = frame.shot().cloned() else {
        return false;
    };
    let map_name = play.reader.header().map_name.clone();
    let keep_remotes = play.kind() == demo::KIND_CLIENT;
    let watched = play.watched;
    apply_world_shot(
        game,
        prediction,
        remotes,
        camera,
        brush_scale,
        tick_ingress,
        &map_name,
        &shot,
        session_start.elapsed().as_secs_f64(),
        keep_remotes,
    );
    play.adopt_players(&shot.players, watched);
    play.tick = shot.tick;
    game.tick_count = shot.tick;
    game.cur_time = shot.tick as f64 * game.tick_interval;
    play.acc = 0.0;

    true
}

fn pump_frames(
    play: &mut DemoPlay,
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    camera: &mut FlyCamera,
    brush_scale: &mut Option<f64>,
    tick_ingress: &mut TickIngress,
    session_start: std::time::Instant,
    through: u64,
) -> bool {
    loop {
        let frame_tick = match play.reader.peek_tick() {
            Ok(Some(tick)) => tick,
            Ok(None) => {
                if play.loop_demo {
                    return restart_demo(
                        play,
                        game,
                        prediction,
                        remotes,
                        camera,
                        brush_scale,
                        tick_ingress,
                        session_start,
                    );
                }

                return false;
            }
            Err(err) => {
                log::warn!("[demo] {err}");

                return false;
            }
        };

        if frame_tick > through {
            return false;
        }

        let frame = match play.reader.next_frame() {
            Ok(Some(frame)) => frame,
            Ok(None) => return false,
            Err(err) => {
                log::warn!("[demo] {err}");

                return false;
            }
        };
        dispatch_frame(
            play,
            game,
            prediction,
            remotes,
            camera,
            brush_scale,
            tick_ingress,
            session_start,
            frame,
        );
    }
}

fn dispatch_frame(
    play: &mut DemoPlay,
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    camera: &mut FlyCamera,
    brush_scale: &mut Option<f64>,
    tick_ingress: &mut TickIngress,
    session_start: std::time::Instant,
    frame: DemoFrame,
) {
    let now = session_start.elapsed().as_secs_f64();

    match frame {
        DemoFrame::ClientMsg { msg, .. } => {
            let mut quiet = None;
            apply_server_event(
                game,
                &mut quiet,
                tick_ingress,
                prediction,
                remotes,
                camera,
                brush_scale,
                now,
                msg,
            );
        }
        DemoFrame::LocalCmd(cmd) => {
            if prediction.local.is_null() {
                prediction.possess(play.watched);
                game.set_local_player(play.watched);
            }

            replay_command(game, prediction, cmd, true);
        }
        DemoFrame::Keyframe(shot) | DemoFrame::Checkpoint(shot) => {
            let map_name = play.reader.header().map_name.clone();
            let keep_remotes = play.kind() == demo::KIND_CLIENT;
            let watched = play.watched;
            apply_world_shot(
                game,
                prediction,
                remotes,
                camera,
                brush_scale,
                tick_ingress,
                &map_name,
                &shot,
                now,
                keep_remotes,
            );
            play.adopt_players(&shot.players, watched);
        }
        DemoFrame::ServerTick { inputs, events, .. } => {
            let mut event_idx = 0;

            while event_idx < events.len() {
                apply_slot_event(
                    play,
                    game,
                    prediction,
                    remotes,
                    camera,
                    events[event_idx].clone(),
                    now,
                );
                event_idx += 1;
            }

            let mut input_idx = 0;

            while input_idx < inputs.len() {
                apply_slot_input(play, game, prediction, remotes, inputs[input_idx]);
                input_idx += 1;
            }
        }
        DemoFrame::NetVars { entities, .. } => {
            game.apply_networked(&entities, now);
        }
        DemoFrame::Entities { tick, entities } => {
            let mut idx = 0;

            while idx < entities.len() {
                let snapshot = entities[idx].clone();
                idx += 1;

                if snapshot.class_hash == Player::CLASS_HASH {
                    continue;
                }

                if !game.entities.is_valid(snapshot.handle) {
                    apply_spawn(
                        game,
                        remotes,
                        now,
                        game.tick_interval,
                        snapshot.clone(),
                        EntityHandle::NULL,
                        prediction.local,
                        &[],
                    );
                    remotes.remove(&snapshot.handle);
                }

                let Some(entity) = game.entities.get_mut(snapshot.handle) else {
                    continue;
                };
                let base = entity.base_mut();
                base.position = snapshot.position;
                base.angles = snapshot.angles;
                base.velocity = snapshot.velocity;
                base.anim.apply_remote(&snapshot.anim, tick);
                base.anim.draw_tick = tick;
                base.anim.draw_frac = 0.0;
            }
        }
    }
}

fn apply_slot_event(
    play: &mut DemoPlay,
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    camera: &mut FlyCamera,
    event: SlotEvent,
    now: f64,
) {
    match event {
        SlotEvent::Join { slot, handle, name } => {
            if !game.entities.is_valid(handle) {
                apply_spawn(
                    game,
                    remotes,
                    now,
                    game.tick_interval,
                    EntitySnapshot {
                        handle,
                        class_hash: Player::CLASS_HASH,
                        health: 100,
                        position: Vector3::new(0.0, 0.0, 1.0),
                        angles: Angle3::new(0.0, 0.0, 0.0),
                        velocity: Vector3::new(0.0, 0.0, 0.0),
                        ack: 0,
                        anim: Default::default(),
                        noclip: false,
                    },
                    EntityHandle::NULL,
                    prediction.local,
                    &[],
                );
            }

            if play.slot_link(slot).is_none() {
                play.slots.push(demo::SlotLink {
                    slot,
                    handle,
                    buttons: InputButtons::NONE,
                });
            }

            if !play.players.iter().any(|player| player.handle == handle) {
                play.players.push(demo::DemoPlayer {
                    slot,
                    handle,
                    name,
                    buttons: InputButtons::NONE,
                });
            }
        }
        SlotEvent::Leave { slot } => {
            let handle = play.slot_link(slot).map(|link| link.handle);

            if let Some(handle) = handle {
                game.sound.forget_entity(handle);
                game.entities.remove(handle);
                game.sync_entities();
                remotes.remove(&handle);
                play.slots.retain(|link| link.slot != slot);
                play.players.retain(|player| player.handle != handle);

                if prediction.local == handle {
                    prediction.clear();
                    game.set_local_player(EntityHandle::NULL);
                }

                play.sync_watched();
            }
        }
        SlotEvent::UserMessage { hash, data, .. } => {
            game.run_usermessage(hash, UserMsgReader::new(data));
        }
        SlotEvent::ScaleMaps { ratio } => {
            let brush = game.brush_world.scale() * ratio;
            let voxel = game.voxel_world.scale() * ratio;
            let _ = game.brush_world.set_scale(brush);
            let _ = game.voxel_world.apply_scale(voxel);
            scale_view(game, camera, prediction, remotes, ratio);
        }
    }
}

fn apply_slot_input(
    play: &mut DemoPlay,
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    input: demo::SlotInput,
) {
    let Some(link_idx) = play.slots.iter().position(|link| link.slot == input.slot) else {
        return;
    };
    let handle = play.slots[link_idx].handle;
    let prev = play.slots[link_idx].buttons;

    if !step_player(game, handle, &input.command, prev) {
        return;
    }

    play.slots[link_idx].buttons = input.command.buttons;
    remotes.remove(&handle);

    if play.watched == handle {
        prediction.look = input.command.view;
    }
}

fn apply_world_shot(
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    _camera: &mut FlyCamera,
    brush_scale: &mut Option<f64>,
    tick_ingress: &mut TickIngress,
    map_name: &str,
    shot: &demo::WorldShot,
    now: f64,
    keep_remotes: bool,
) {
    game.entities.clear();
    game.sync_entities();
    prediction.clear();
    remotes.clear();
    *tick_ingress = TickIngress::new();
    game.voxel_world.clear();

    if !game.voxel_world.apply_scale(shot.voxel_scale) {
        log::warn!("[demo] bad voxel scale {}", shot.voxel_scale);
    }

    let mut idx = 0;

    while idx < shot.voxels.len() {
        if !game.voxel_world.apply(&shot.voxels[idx]) {
            log::warn!("[demo] bad chunk");
        }

        idx += 1;
    }

    if let Err(err) = game.brush_world.load_file(map_name) {
        log::warn!("[demo] {err}");
    }

    if !game.brush_world.set_scale(shot.brush_scale) {
        log::warn!("[demo] bad brush scale {}", shot.brush_scale);
    }

    *brush_scale = Some(shot.brush_scale);
    idx = 0;

    while idx < shot.brush_edits.len() {
        if !game.brush_world.apply_edit(&shot.brush_edits[idx]) {
            log::warn!("[demo] bad brush edit");
        }

        idx += 1;
    }

    let states: HashMap<EntityHandle, Vec<NetVar>> = shot
        .networked
        .iter()
        .map(|entity| (entity.handle, entity.vars.clone()))
        .collect();
    let owners: HashMap<EntityHandle, EntityHandle> = shot
        .owners
        .iter()
        .map(|ownership| (ownership.handle, ownership.owner))
        .collect();
    idx = 0;

    while idx < shot.entities.len() {
        let entity = &shot.entities[idx];
        let vars = states
            .get(&entity.handle)
            .map(|vars| vars.as_slice())
            .unwrap_or(&[]);
        let owner = owners
            .get(&entity.handle)
            .copied()
            .unwrap_or(EntityHandle::NULL);
        apply_spawn(
            game,
            remotes,
            now,
            game.tick_interval,
            entity.clone(),
            owner,
            shot.local,
            vars,
        );
        idx += 1;
    }

    idx = 0;

    while idx < shot.models.len() {
        apply_anim_model(game, shot.models[idx].clone());
        idx += 1;
    }

    idx = 0;

    while idx < shot.bones.len() {
        apply_anim_bones(game, shot.bones[idx].clone());
        idx += 1;
    }

    let playing = game.sound.baseline();
    idx = 0;

    while idx < playing.len() {
        let sound = &playing[idx];
        game.sound
            .hear_stop(sound.entity, sound.def_hash, sound.sound_hash);
        idx += 1;
    }

    idx = 0;

    while idx < shot.sounds.len() {
        let sound = &shot.sounds[idx];
        game.sound.hear_play(
            sound.sound_hash,
            sound.def_hash,
            sound.entity_handle,
            sound.position,
            sound.volume,
            sound.pitch,
            shot.tick,
            true,
            sound.positional,
        );
        idx += 1;
    }

    if !keep_remotes {
        remotes.clear();
    }

    if shot.local.is_null() {
        game.set_local_player(EntityHandle::NULL);
    } else {
        prediction.possess(shot.local);
        game.set_local_player(shot.local);
    }

    game.sync_entities();
}

fn place_demo_camera(
    camera: &mut FlyCamera,
    game: &GameState<FromServer, ClientToServer>,
    play: &DemoPlay,
    prediction: &Prediction,
) {
    let handle = play.watched;
    let alpha = if game.tick_interval > 0.0 {
        (play.acc / game.tick_interval).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let client_local = play.kind() == demo::KIND_CLIENT && handle == prediction.local;
    let body = player_body(game, handle);
    let (origin, look, eye) = if client_local {
        if let Some(origin) = prediction.view_origin(alpha) {
            let eye = if prediction.previous().contains(InputButtons::IN_DUCK) {
                body.view_offset_ducked.z
            } else {
                body.view_offset.z
            };

            (origin, prediction.look, eye)
        } else if let Some(origin) = body_origin(game, handle) {
            (origin, prediction.look, body.view_offset.z)
        } else {
            return;
        }
    } else if let Some(entity) = game.entities.get(handle) {
        let base = entity.base();

        (base.position, base.angles, body.view_offset.z)
    } else {
        return;
    };

    match play.cam {
        CamMode::First => place_camera(camera, origin, look, eye),
        CamMode::Chase => {
            place_camera(camera, origin, look, eye);
            let yaw = look.y.to_radians();
            let pitch = look.p.to_radians();
            let fx = pitch.cos() * yaw.cos();
            let fy = pitch.cos() * yaw.sin();
            let fz = pitch.sin();
            let dist = 2.8;
            camera.x -= f64::from(fx) * dist;
            camera.y -= f64::from(fy) * dist;
            camera.z -= f64::from(fz) * dist;
            camera.z += 0.55;
        }
        CamMode::Orbit => {
            let yaw = play.orbit_yaw;
            let pitch = play.orbit_pitch;
            let dist = play.orbit_dist;
            let fx = pitch.cos() * yaw.cos();
            let fy = pitch.cos() * yaw.sin();
            let fz = pitch.sin();
            camera.x = origin.x - f64::from(fx) * dist;
            camera.y = origin.y - f64::from(fy) * dist;
            camera.z = origin.z + eye - f64::from(fz) * dist;
            camera.yaw = yaw;
            camera.pitch = pitch;
        }
        CamMode::Free => {}
    }
}

fn client_surface() -> (PlatformHost, Option<backend::GfxWindow>) {
    let kind = HostKind::from_env();
    log::info!("[host] {kind:?}");
    let mut host = PlatformHost::open(kind).unwrap_or_else(|err| {
        log::warn!("[host] {err}");
        std::process::exit(1);
    });

    #[cfg(target_os = "android")]
    {
        return (host, None);
    }

    #[cfg(not(target_os = "android"))]
    {
        host.set_title("Rust Engine - Rendering");
        #[cfg(not(target_os = "ios"))]
        host.set_size(1920, 1080);
        let surface = host.surface().expect("host surface");
        let mut window = backend::create(surface);
        window.enable_vr();

        (host, Some(window))
    }
}

struct TouchPoint {
    id: u64,
    origin_x: f64,
    origin_y: f64,
    x: f64,
    y: f64,
    last_x: f64,
    last_y: f64,
    look: bool,
}

fn apply_touch(
    points: &mut Vec<TouchPoint>,
    touch: &Touch,
    width: f64,
    camera: &mut FlyCamera,
    look: &mut Angle3,
    possessed: bool,
) {
    match touch.phase {
        TouchPhase::Started => {
            points.push(TouchPoint {
                id: touch.id,
                origin_x: touch.location.0,
                origin_y: touch.location.1,
                x: touch.location.0,
                y: touch.location.1,
                last_x: touch.location.0,
                last_y: touch.location.1,
                look: touch.location.0 >= width * 0.5,
            });
        }
        TouchPhase::Moved => {
            let mut idx = 0;

            while idx < points.len() {
                if points[idx].id == touch.id {
                    let dx = touch.location.0 - points[idx].last_x;
                    let dy = touch.location.1 - points[idx].last_y;
                    points[idx].x = touch.location.0;
                    points[idx].y = touch.location.1;
                    points[idx].last_x = touch.location.0;
                    points[idx].last_y = touch.location.1;

                    if points[idx].look {
                        if possessed {
                            look.y += dx as f32 * LOOK_SPEED;
                            look.p -= dy as f32 * LOOK_SPEED;
                            look.p = look.p.clamp(-89.0, 89.0);
                        } else {
                            camera.look(dx as f32, dy as f32);
                        }
                    }

                    break;
                }

                idx += 1;
            }
        }
        TouchPhase::Ended | TouchPhase::Cancelled => {
            points.retain(|point| point.id != touch.id);
        }
    }
}

fn touch_wish(points: &[TouchPoint]) -> (f32, f32) {
    let mut forward = 0.0;
    let mut right = 0.0;
    let mut idx = 0;

    while idx < points.len() {
        let point = &points[idx];

        if !point.look {
            right += ((point.x - point.origin_x) / 90.0).clamp(-1.0, 1.0) as f32;
            forward -= ((point.y - point.origin_y) / 90.0).clamp(-1.0, 1.0) as f32;
        }

        idx += 1;
    }

    (forward.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0))
}

fn command_view(look: Angle3, vr_yaw: f32) -> Angle3 {
    let mut view = look;
    view.y += vr_yaw.to_degrees();
    view.p = view.p.clamp(-89.0, 89.0);

    view.normalize()
}

fn body_origin(
    game: &GameState<FromServer, ClientToServer>,
    handle: EntityHandle,
) -> Option<Vector3> {
    game.entities
        .get(handle)
        .map(|entity| entity.base().position)
}

fn player_body(
    game: &GameState<FromServer, ClientToServer>,
    handle: EntityHandle,
) -> movement::PlayerBody {
    game.entities
        .get(handle)
        .and_then(|entity| entity.player_body().copied())
        .unwrap_or_default()
}

fn scale_view(
    game: &mut GameState<FromServer, ClientToServer>,
    camera: &mut FlyCamera,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    ratio: f64,
) {
    if !ratio.is_finite() || (ratio - 1.0).abs() <= 1e-12 {
        return;
    }

    crate::scale::shift_entities(&mut game.entities, None, ratio);
    camera.x *= ratio;
    camera.y *= ratio;
    camera.z *= ratio;
    prediction.scale_span(ratio);

    for samples in remotes.values_mut() {
        let mut idx = 0;

        while idx < samples.len() {
            let pose = &mut samples[idx];
            pose.position.x *= ratio;
            pose.position.y *= ratio;
            pose.position.z *= ratio;
            pose.velocity.x *= ratio;
            pose.velocity.y *= ratio;
            pose.velocity.z *= ratio;
            idx += 1;
        }
    }
}

fn place_camera(camera: &mut FlyCamera, origin: Vector3, look: Angle3, eye: f64) {
    camera.x = origin.x;
    camera.y = origin.y;
    camera.z = origin.z + eye;
    camera.yaw = look.y.to_radians();
    camera.pitch = look.p.to_radians();
}

fn predict_tick(
    game: &mut GameState<FromServer, ClientToServer>,
    recorder: &mut Option<DemoSession>,
    prediction: &mut Prediction,
    buttons: InputButtons,
    forward: f32,
    right: f32,
    vr_yaw: f32,
) {
    if !game.entities.is_valid(prediction.local) {
        return;
    }

    let cmd = UserCommand {
        tick: game.tick_count,
        buttons,
        wish: Vector3::new(forward as f64, right as f64, 0.0),
        view: command_view(prediction.look, vr_yaw),
    };

    if let Some(session) = recorder.as_mut() {
        session.write_frame(&DemoFrame::LocalCmd(cmd));
    }

    if recorder.as_ref().is_some_and(|session| session.dead) {
        log::warn!("[demo] recording stopped");
        *recorder = None;
    }

    replay_command(game, prediction, cmd, false);
    game.send_unreliable(ClientToServer::PlayerInput {
        tick: cmd.tick,
        buttons: cmd.buttons,
        movement: cmd.wish,
        viewangles: cmd.view,
    });
}

fn reconcile_player(
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    snapshot: &EntitySnapshot,
    predicted: &[EntityNetworked],
) {
    if snapshot.handle != prediction.local {
        return;
    }

    if !prediction.take_ack(snapshot.ack) {
        log::debug!(
            "[pred] skip stale ack={} have={}",
            snapshot.ack,
            prediction.acked(),
        );

        return;
    }

    if let Some(entity) = game.entities.get_mut(snapshot.handle) {
        entity
            .base_mut()
            .anim
            .apply_remote(&snapshot.anim, snapshot.ack);

        if let Some(body) = entity.player_body_mut() {
            body.noclip = snapshot.noclip;
        }
    }

    let (predicted_pos, predicted_vel) = match game.entities.get(snapshot.handle) {
        Some(entity) => {
            let base = entity.base();

            (base.position, base.velocity)
        }
        None => return,
    };

    let mut position = snapshot.position;
    let mut velocity = snapshot.velocity;
    let mut angles = snapshot.angles;
    let dt = game.tick_interval;
    let gravity = movement::gravity(&game.cvars);
    let pending = prediction.pending();
    let handle = snapshot.handle;
    let mut prev = prediction.base_buttons();
    game.begin_reconcile(predicted);

    for cmd in prediction.commands() {
        let (root, body) = {
            let Some(entity) = game.entities.get(handle) else {
                break;
            };
            let body = entity
                .player_body()
                .copied()
                .unwrap_or_default();
            let root = game
                .anims
                .root_motion(&entity.base().anim, angles.y, cmd.tick, dt);

            (root, body)
        };
        movement::step(
            &mut position,
            &mut velocity,
            &mut angles,
            &cmd,
            prev,
            dt,
            gravity,
            &game.brush_world,
            &game.voxel_world,
            root,
            &body,
        );
        prev = cmd.buttons;

        if let Some(entity) = game.entities.get_mut(handle) {
            let base = entity.base_mut();
            base.position = position;
            base.velocity = velocity;
            base.angles = angles;
        }

        game.run_predicted(handle, &cmd, false);
    }

    let dx = predicted_pos.x - position.x;
    let dy = predicted_pos.y - position.y;
    let dz = predicted_pos.z - position.z;
    let err = (dx * dx + dy * dy + dz * dz).sqrt();
    let dvx = predicted_vel.x - velocity.x;
    let dvy = predicted_vel.y - velocity.y;
    let dvz = predicted_vel.z - velocity.z;
    let dvel = (dvx * dvx + dvy * dvy + dvz * dvz).sqrt();
    let base_dx = predicted_pos.x - snapshot.position.x;
    let base_dy = predicted_pos.y - snapshot.position.y;
    let base_dz = predicted_pos.z - snapshot.position.z;
    let base_err = (base_dx * base_dx + base_dy * base_dy + base_dz * base_dz).sqrt();
    let noisy = err > 0.01 || dvel > 0.05;

    if noisy || should_log_pred() {
        let _ = (pending, base_err);
        log::info!(
            "[pred] {} ack={} err={:.4} dvel={:.4} sv=({:.2},{:.2},{:.2}) cl=({:.2},{:.2},{:.2})",
            if noisy { "ERR" } else { "ok" },
            snapshot.ack,
            err,
            dvel,
            snapshot.position.x,
            snapshot.position.y,
            snapshot.position.z,
            predicted_pos.x,
            predicted_pos.y,
            predicted_pos.z,
        );
    }

    if let Some(entity) = game.entities.get_mut(snapshot.handle) {
        let base = entity.base_mut();
        base.position = position;
        base.velocity = velocity;
        base.angles = angles;
    }

    game.end_reconcile();
    prediction.correct_view(position);

    let _: () = game.run_hook(
        "TransformUpdated",
        (
            snapshot.handle,
            Some(position),
            Some(angles),
            Some(velocity),
        ),
    );
}

fn should_log_pred() -> bool {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static LAST_MS: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let prev = LAST_MS.load(Ordering::Relaxed);

    if now.saturating_sub(prev) < 500 {
        return false;
    }

    LAST_MS.store(now, Ordering::Relaxed);

    true
}

fn note_remote(
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    handle: EntityHandle,
    pose: NetPose,
    interval: f64,
) {
    let samples = remotes.entry(handle).or_default();
    movement::remember_pose(samples, pose, interval);
}

fn predicted_owned(
    game: &GameState<FromServer, ClientToServer>,
    handle: EntityHandle,
    local: EntityHandle,
) -> bool {
    if local.is_null() || handle == local {
        return false;
    }

    game.entities
        .get(handle)
        .map(|entity| entity.base().owner == local)
        .unwrap_or(false)
}

fn present_remotes(
    game: &mut GameState<FromServer, ClientToServer>,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    local: EntityHandle,
    now: f64,
    interval: f64,
) {
    let delay = interval.max(0.0) * 2.0;
    let render_time = now - delay;
    let mut visual = Vec::new();

    for (handle, samples) in remotes.iter_mut() {
        if *handle == local || predicted_owned(game, *handle, local) {
            continue;
        }

        if let Some(pose) = movement::blend_poses(samples, render_time, interval) {
            let clock = movement::sample_clock(samples, render_time).unwrap_or((pose.tick, 0.0));
            visual.push((*handle, pose, clock.0, clock.1));
        }

        movement::forget_old_poses(samples, render_time);
    }

    game.present_networked(render_time);
    let mut anim_events = Vec::new();
    let mut idx = 0;

    while idx < visual.len() {
        let (handle, pose, tick, frac) = visual[idx];
        let sample = tick as f64 + f64::from(frac);
        let crossed = sample.floor() as u64;

        let playback = game.entities.get(handle).map(|entity| entity.base().anim);

        if let Some(entity) = game.entities.get_mut(handle) {
            let base = entity.base_mut();
            base.position = pose.position;
            base.angles = pose.angles;
            base.velocity = pose.velocity;
            base.anim.draw_tick = tick;
            base.anim.draw_frac = frac;

            if let Some(playback) = playback {
                if playback.event_tick == 0 {
                    base.anim.event_tick = crossed.max(1);
                } else if crossed > playback.event_tick {
                    base.anim.event_tick = crossed;
                }
            }
        }

        if let Some(playback) = playback {
            if playback.event_tick > 0 && crossed > playback.event_tick {
                let names = game.anims.events(
                    &playback,
                    playback.event_tick as f64,
                    crossed as f64,
                    interval,
                );
                let mut name_idx = 0;

                while name_idx < names.len() {
                    anim_events.push((handle, names[name_idx].clone()));
                    name_idx += 1;
                }
            }
        }

        idx += 1;
    }

    game.fire_anim_events(anim_events);
}

fn apply_server_event(
    game: &mut GameState<FromServer, ClientToServer>,
    recorder: &mut Option<DemoSession>,
    tick_ingress: &mut TickIngress,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    camera: &mut FlyCamera,
    brush_scale: &mut Option<f64>,
    now: f64,
    message: ServerToClient,
) {
    if let Some(session) = recorder.as_mut() {
        session.note_client(game.tick_count, &message);
    }

    if recorder.as_ref().is_some_and(|session| session.dead) {
        log::warn!("[demo] recording stopped");
        *recorder = None;
    }

    match &message {
        ServerToClient::TickState { .. }
        | ServerToClient::PredictedState { .. }
        | ServerToClient::NetworkedUpdate { .. }
        | ServerToClient::Pong { .. }
        | ServerToClient::ServerTick { .. }
        | ServerToClient::VoxelChunk(_)
        | ServerToClient::NavMesh { .. } => {
            log::debug!("[cl] {}", message.summary());
        }
        _ => {
            log::info!("[cl] {}", message.summary());
        }
    }

    match message {
        ServerToClient::PlayerConnected { handle, name } => {
            let _: () = game.run_hook("PlayerConnected", (handle, name));
        }
        ServerToClient::PlayerDisconnected { handle } => {
            let _: () = game.run_hook("PlayerDisconnected", handle);
        }
        ServerToClient::PlayerSpawned { handle } => {
            prediction.possess(handle);
            game.set_local_player(handle);
            remotes.remove(&handle);
            let _: () = game.run_hook("PlayerSpawned", handle);
        }
        ServerToClient::PlayerDamaged {
            handle,
            attacker,
            inflictor,
            damage,
            new_health,
        } => {
            let _: () = game.run_hook(
                "PlayerDamaged",
                (handle, attacker, inflictor, damage, new_health),
            );
        }
        ServerToClient::PlayerDied {
            handle,
            killer,
            inflictor,
        } => {
            let _: () = game.run_hook("PlayerDied", (handle, killer, inflictor));
        }
        ServerToClient::ModelChanged { handle, model } => {
            let _: () = game.run_hook("ModelChanged", (handle, model));
        }
        ServerToClient::TransformUpdated {
            handle,
            position,
            angles,
            velocity,
        } => {
            if handle != prediction.local && !predicted_owned(game, handle, prediction.local) {
                if let Some(entity) = game.entities.get(handle) {
                    let base = entity.base();
                    let tick = remotes
                        .get(&handle)
                        .and_then(|samples| samples.back())
                        .map(|pose| pose.tick.saturating_add(1))
                        .unwrap_or(1);
                    note_remote(
                        remotes,
                        handle,
                        NetPose {
                            tick,
                            time: now,
                            position: position.unwrap_or(base.position),
                            angles: angles.unwrap_or(base.angles),
                            velocity: velocity.unwrap_or(base.velocity),
                        },
                        game.tick_interval,
                    );
                }
            }

            let _: () = game.run_hook("TransformUpdated", (handle, position, angles, velocity));
        }

        ServerToClient::UserMessage { hash, data } => {
            game.run_usermessage(hash, UserMsgReader::new(data));
        }
        ServerToClient::ChatMessage {
            sender_handle,
            team_only,
            text,
        } => {
            let _: () = game.run_hook("ChatMessage", (sender_handle, team_only, text));
        }
        ServerToClient::VoiceChunk {
            sender_handle,
            data,
        } => {
            let _: () = game.run_hook("VoiceChunk", (sender_handle, data));
        }
        ServerToClient::WorldSnapshot { .. } => {}
        ServerToClient::VoxelScale { scale } => {
            if !game.voxel_world.apply_scale(scale) {
                log::warn!("[cl] bad voxel scale {scale}");
            }
        }
        ServerToClient::BrushScale { scale } => {
            *brush_scale = Some(scale);

            if !game.brush_world.set_scale(scale) {
                log::warn!("[cl] bad brush scale {scale}");
            }
        }
        ServerToClient::WorldMotion { ratio } => {
            scale_view(game, camera, prediction, remotes, ratio);
        }
        ServerToClient::VoxelChunk(update) => {
            if !game.voxel_world.apply(&update) {
                log::warn!("[cl] bad chunk {} {} {}", update.x, update.y, update.z);
            }
        }
        ServerToClient::BrushEdit(edit) => {
            if !game.brush_world.apply_edit(&edit) {
                log::warn!("[cl] bad brush edit");
            }
        }
        ServerToClient::MapChange { map_name } => {
            if let Err(err) = game.brush_world.load_file(&map_name) {
                log::warn!("[map] {err}");
            }

            if let Some(scale) = *brush_scale {
                if !game.brush_world.set_scale(scale) {
                    log::warn!("[cl] bad brush scale {scale}");
                }
            }

            game.nav.state.clear_mesh();
        }
        ServerToClient::NavMesh { part, parts, bytes } => {
            game.nav.state.push_part(part, parts, bytes);
        }
        ServerToClient::NavShow { enabled } => {
            game.nav.state.set_show(enabled);
        }
        ServerToClient::NavPath { follow, points } => {
            if follow {
                game.nav.state.set_follow(points);
            } else {
                game.nav.state.set_debug_path(points);
            }
        }
        ServerToClient::EntitySpawned {
            handle,
            class_hash,
            position,
            angles,
            owner,
            networked,
        } => {
            if game.entities.is_valid(handle) {
                return;
            }

            if class_hash == Player::CLASS_HASH {
                let mut player = Player::new();
                player.base.position = position;
                player.base.angles = angles;
                player.base.owner = owner;
                game.entities.insert_at(handle, Box::new(player));
            } else if !spawn_scripted(
                game,
                now,
                handle,
                class_hash,
                position,
                angles,
                Vector3::new(0.0, 0.0, 0.0),
                owner,
                &networked,
            ) {
                return;
            }

            if owner != prediction.local {
                note_remote(
                    remotes,
                    handle,
                    NetPose {
                        tick: 0,
                        time: now,
                        position,
                        angles,
                        velocity: Vector3::new(0.0, 0.0, 0.0),
                    },
                    game.tick_interval,
                );
            }
        }
        ServerToClient::EntityDespawned { handle } => {
            game.sound.forget_entity(handle);
            game.entities.remove(handle);
            game.sync_entities();
            remotes.remove(&handle);

            if prediction.local == handle {
                prediction.clear();
                game.set_local_player(EntityHandle::NULL);
            }
        }
        ServerToClient::NetworkedUpdate { entities } => {
            game.apply_networked(&entities, now);
        }
        ServerToClient::PredictedState {
            tick: _,
            player,
            entities,
            anims,
        } => {
            if player.handle == prediction.local {
                for anim in &anims {
                    if let Some(entity) = game.entities.get_mut(anim.handle) {
                        let cur = entity.base().anim;
                        let snap = &anim.anim;

                        if cur.sequence != snap.sequence || cur.gesture != snap.gesture {
                            entity.base_mut().anim.apply_remote(snap, player.ack);
                        }
                    }

                    if !predicted_owned(game, anim.handle, prediction.local) {
                        game.anims.apply_bone_net(anim.handle.0, &anim.bones);
                    }
                }

                reconcile_player(game, prediction, &player, &entities);
            }
        }
        ServerToClient::AnimModel {
            handle,
            mesh,
            clips,
        } => {
            apply_anim_model(
                game,
                EntityModel {
                    handle,
                    mesh,
                    clips,
                },
            );
        }
        ServerToClient::AnimBones { handle, bones } => {
            if predicted_owned(game, handle, prediction.local) {
                return;
            }

            apply_anim_bones(
                game,
                EntityBones {
                    handle,
                    bones,
                },
            );
        }
        ServerToClient::EntityOwner { handle, owner } => {
            let Some(entity) = game.entities.get_mut(handle) else {
                return;
            };

            entity.base_mut().owner = owner;
            game.owner_changed(handle, owner);

            if owner == prediction.local {
                remotes.remove(&handle);
            }
        }
        ServerToClient::TickState {
            tick,
            part,
            parts,
            entities,
        } => {
            if let Some(entities) = tick_ingress.push(tick, part, parts, entities) {
                log::debug!(
                    "[cl] tick {} assembled {} entity updates",
                    tick,
                    entities.len()
                );

                for snapshot in entities {
                    if snapshot.handle == prediction.local
                        || predicted_owned(game, snapshot.handle, prediction.local)
                    {
                        continue;
                    }

                    if let Some(entity) = game.entities.get_mut(snapshot.handle) {
                        entity.base_mut().anim.apply_remote(&snapshot.anim, tick);
                        entity.base_mut().anim.draw_tick = tick;
                        entity.base_mut().anim.draw_frac = 0.0;

                        if let Some(body) = entity.player_body_mut() {
                            body.noclip = snapshot.noclip;
                        }
                    }

                    note_remote(
                        remotes,
                        snapshot.handle,
                        NetPose {
                            tick,
                            time: now,
                            position: snapshot.position,
                            angles: snapshot.angles,
                            velocity: snapshot.velocity,
                        },
                        game.tick_interval,
                    );

                    let _: () = game.run_hook(
                        "TransformUpdated",
                        (
                            snapshot.handle,
                            Some(snapshot.position),
                            Some(snapshot.angles),
                            Some(snapshot.velocity),
                        ),
                    );
                }
            }
        }

        ServerToClient::PlaySound {
            sound_hash,
            entity_handle,
            position,
            volume,
            pitch,
            def_hash,
            tick,
            positional,
        } => {
            game.sound.hear_play(
                sound_hash,
                def_hash,
                entity_handle.unwrap_or(EntityHandle::NULL),
                position,
                volume,
                pitch,
                tick,
                false,
                positional,
            );
        }
        ServerToClient::StopSound {
            def_hash,
            sound_hash,
            entity_handle,
        } => {
            game.sound.hear_stop(entity_handle, def_hash, sound_hash);
        }
        ServerToClient::SoundBaseline { sounds } => {
            let mut idx = 0;

            while idx < sounds.len() {
                let sound = &sounds[idx];
                game.sound.hear_play(
                    sound.sound_hash,
                    sound.def_hash,
                    sound.entity_handle,
                    sound.position,
                    sound.volume,
                    sound.pitch,
                    0,
                    true,
                    sound.positional,
                );
                idx += 1;
            }
        }
        other => {
            log::warn!("[cl] unhandled {}", other.summary());
        }
    }
}

#[cfg(feature = "client")]
const HANDSHAKE_INTERVAL: Duration = Duration::from_millis(200);

#[cfg(feature = "client")]
pub fn client_network_loop(
    server_addr: SocketAddr,
    tx: Sender<FromServer>,
    rx: Receiver<NetSend<ClientToServer>>,
    shutdown: Arc<AtomicBool>,
    mut wake: TcpStream,
    resync: Arc<AtomicBool>,
) {
    let local_addr = if server_addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let mut join_gen = crate::network::steam::join_generation();
    let mut client = NetworkClient::new(SocketAddr::from_str(local_addr).unwrap());
    if join_gen == 0 {
        client
            .connect(server_addr)
            .expect("Failed to connect to server");
    } else {
        client.set_steam(true);
        log::info!("[cl] joining friend");
    }
    let mut reliable_chan = ReliableChannel::new();
    let mut state_chan = ReliableChannel::with_stream(STREAM_STATE);
    let mut connected = false;
    let mut local_reliable: VecDeque<Vec<u8>> = VecDeque::new();
    let mut unreliable_out: u32 = 0;
    let mut unreliable_in = UnreliableInbox::new();
    let mut unreliable_assembly = UnreliableAssembly::new();
    let mut replace_session: Option<u64> = None;
    let mut generation: Option<u32> = None;

    let _ = client.send_message(&connect_packet(None));
    let mut challenge_response_bytes: Option<Vec<u8>> = None;
    let mut pending_auth: Option<(u64, u64)> = None;
    let mut auth_stopped = false;
    let mut session: Option<u64> = None;
    let mut last_sent = Instant::now();
    let mut last_server_seen = Instant::now();
    let mut unreliable_parts: Vec<BundlePart> = Vec::new();

    loop {
        let gen = crate::network::steam::join_generation();
        if gen != join_gen {
            join_gen = gen;
            if let Some(current) = session {
                let bytes =
                    wincode::serialize(&PacketType::Disconnect { session: current }).unwrap();
                let _ = client.send_message(&bytes);
            }

            if connected {
                let _ = tx.send(FromServer::Disconnected);
            }

            client.set_steam(true);
            connected = false;
            session = None;
            generation = None;
            replace_session = None;
            challenge_response_bytes = None;
            pending_auth = None;
            auth_stopped = false;
            crate::network::steam::cancel_ticket();
            unreliable_out = 0;
            unreliable_in = UnreliableInbox::new();
            unreliable_assembly = UnreliableAssembly::new();
            reliable_chan = ReliableChannel::new();
            state_chan = ReliableChannel::with_stream(STREAM_STATE);
            local_reliable.clear();
            unreliable_parts.clear();
            let _ = client.send_message(&connect_packet(None));
            last_sent = Instant::now();
            last_server_seen = Instant::now();
            log::info!("[cl] joining friend");
        }

        if shutdown.load(Ordering::Relaxed) {
            if let Some(current) = session {
                let bytes =
                    wincode::serialize(&PacketType::Disconnect { session: current }).unwrap();
                let _ = client.send_message(&bytes);
            }

            let _ = tx.send(FromServer::Disconnected);

            return;
        }

        if resync.swap(false, Ordering::Relaxed) {
            if let Some(current) = session {
                let bytes =
                    wincode::serialize(&PacketType::Disconnect { session: current }).unwrap();
                let _ = client.send_message(&bytes);
            }

            let was_connected = connected;
            begin_reconnect(
                &client,
                &mut reliable_chan,
                &mut state_chan,
                &mut local_reliable,
                &mut connected,
                &mut session,
                &mut generation,
                &mut replace_session,
                &mut challenge_response_bytes,
                &mut unreliable_out,
                &mut unreliable_in,
                &mut unreliable_assembly,
                &mut last_sent,
            );
            unreliable_parts.clear();
            pending_auth = None;
            auth_stopped = false;

            if was_connected {
                let _ = tx.send(FromServer::Disconnected);
            }

            log::info!("[cl] demo resync");
        }

        while let Ok(outgoing) = rx.try_recv() {
            queue_client_send(
                outgoing,
                &reliable_chan,
                &mut local_reliable,
                &mut unreliable_out,
                connected,
                session,
                &mut unreliable_parts,
            );
        }

        let mut got_packet = false;
        let mut need_ack = false;
        for _idx in 0..RECV_BUDGET {
            let Some(parsed) = client.poll_packet() else {
                break;
            };

            got_packet = true;
            let Ok(packet) = parsed else {
                continue;
            };

            match packet {
                PacketType::Connect { .. } => {}
                PacketType::Challenge {
                    token,
                    secure,
                    host_steam_id,
                } => {
                    if !connected && !auth_stopped {
                        if !secure {
                            crate::network::steam::cancel_ticket();
                            pending_auth = None;
                            if let Some(bytes) = auth_response(token, 0, Vec::new(), String::new())
                            {
                                challenge_response_bytes = Some(bytes.clone());
                                let _ = client.send_message(&bytes);
                                last_sent = Instant::now();
                                log::info!("[cl] challenge {token}");
                            }
                        } else if host_steam_id == 0 {
                            challenge_response_bytes = None;
                        } else {
                            crate::network::steam::request_ticket(host_steam_id);
                            pending_auth = Some((token, host_steam_id));
                            challenge_response_bytes = None;
                        }
                    }
                }
                PacketType::ChallengeResponse { .. } => {
                    log::info!("[cl] challenge response");
                }
                PacketType::Connected {
                    session: new_session,
                    generation: new_generation,
                } => {
                    if !connected && replace_session != Some(new_session) {
                        let session_changed = session.is_some() && session != Some(new_session);
                        let generation_changed = generation != Some(new_generation);
                        if session_changed || generation_changed {
                            reclaim_reliable(&mut reliable_chan, generation, &mut local_reliable);
                            reliable_chan = ReliableChannel::new();
                            state_chan = ReliableChannel::with_stream(STREAM_STATE);
                            unreliable_out = 0;
                            unreliable_in = UnreliableInbox::new();
                            unreliable_assembly = UnreliableAssembly::new();
                            unreliable_parts.clear();
                        }

                        connected = true;
                        session = Some(new_session);
                        generation = Some(new_generation);
                        replace_session = None;
                        reliable_chan.set_session(new_session);
                        state_chan.set_session(new_session);
                        challenge_response_bytes = None;
                        last_server_seen = Instant::now();
                        log::info!("[cl] connected");
                        let _ = tx.send(FromServer::Connected {
                            generation: new_generation,
                        });
                    } else if session == Some(new_session) {
                        last_server_seen = Instant::now();
                    }
                }
                PacketType::Disconnect { session: incoming } => {
                    if connected && session == Some(incoming) {
                        unreliable_parts.clear();
                        begin_reconnect(
                            &client,
                            &mut reliable_chan,
                            &mut state_chan,
                            &mut local_reliable,
                            &mut connected,
                            &mut session,
                            &mut generation,
                            &mut replace_session,
                            &mut challenge_response_bytes,
                            &mut unreliable_out,
                            &mut unreliable_in,
                            &mut unreliable_assembly,
                            &mut last_sent,
                        );
                        pending_auth = None;
                        auth_stopped = false;
                        log::info!("[cl] disconnect");
                        let _ = tx.send(FromServer::Disconnected);
                    }
                }
                PacketType::Bundle {
                    session: incoming,
                    ack,
                    cumulative,
                    selective,
                    state_cumulative,
                    state_selective,
                    parts,
                } => {
                    if connected && session == Some(incoming) {
                        last_server_seen = Instant::now();
                        if ack {
                            reliable_chan.handle_ack(cumulative, selective);
                            state_chan.handle_ack(state_cumulative, state_selective);
                        }

                        for part in parts {
                            if apply_client_part(
                                &mut reliable_chan,
                                &mut state_chan,
                                &tx,
                                &mut unreliable_in,
                                &mut unreliable_assembly,
                                connected,
                                session,
                                generation,
                                incoming,
                                part,
                                &mut last_server_seen,
                            ) {
                                need_ack = true;
                            }
                        }
                    }
                }
                PacketType::KeepAlive { session: incoming } => {
                    if connected && session == Some(incoming) {
                        last_server_seen = Instant::now();
                    }
                }
                PacketType::Reliable {
                    session: incoming,
                    stream,
                    sequence,
                    generation: packet_generation,
                    payload,
                } => {
                    let channel = if stream == STREAM_STATE {
                        &mut state_chan
                    } else {
                        &mut reliable_chan
                    };

                    if push_client_reliable(
                        channel,
                        &tx,
                        connected,
                        session,
                        generation,
                        incoming,
                        packet_generation,
                        sequence,
                        ReliableBody::Complete(owned_payload(payload)),
                        &mut last_server_seen,
                    ) {
                        need_ack = true;
                    }
                }
                PacketType::Ack {
                    session: incoming,
                    cumulative,
                    selective,
                    state_cumulative,
                    state_selective,
                } => {
                    if connected && session == Some(incoming) {
                        last_server_seen = Instant::now();
                        reliable_chan.handle_ack(cumulative, selective);
                        state_chan.handle_ack(state_cumulative, state_selective);
                    }
                }
                PacketType::Unreliable {
                    session: incoming,
                    sequence,
                    payload,
                } => {
                    if connected && session == Some(incoming) {
                        if let Some(payload) = take_unreliable(
                            &mut unreliable_in,
                            &mut unreliable_assembly,
                            sequence,
                            owned_payload(payload),
                        ) {
                            last_server_seen = Instant::now();
                            if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                                let _ = tx.send(FromServer::Message(event));
                            }
                        }
                    }
                }
                PacketType::Fragment {
                    session: incoming,
                    stream,
                    sequence,
                    generation: packet_generation,
                    packet_id,
                    fragment_idx,
                    total_fragments,
                    data,
                } => {
                    let channel = if stream == STREAM_STATE {
                        &mut state_chan
                    } else {
                        &mut reliable_chan
                    };

                    if push_client_reliable(
                        channel,
                        &tx,
                        connected,
                        session,
                        generation,
                        incoming,
                        packet_generation,
                        sequence,
                        ReliableBody::Fragment {
                            packet_id,
                            fragment_idx,
                            total_fragments,
                            data: owned_payload(data),
                        },
                        &mut last_server_seen,
                    ) {
                        need_ack = true;
                    }
                }
            }
        }

        if let Some((token, host)) = pending_auth {
            match crate::network::steam::take_ticket() {
                Some(Ok(ticket)) if ticket.host == host => {
                    if let Some(bytes) =
                        auth_response(token, ticket.steam_id, ticket.ticket, ticket.name)
                    {
                        challenge_response_bytes = Some(bytes.clone());
                        let _ = client.send_message(&bytes);
                        last_sent = Instant::now();
                        pending_auth = None;
                        log::info!("[cl] challenge {token}");
                    } else {
                        log::warn!("[cl] auth ticket does not fit");
                        pending_auth = None;
                        auth_stopped = true;
                        crate::network::steam::cancel_ticket();
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(())) => {
                    log::warn!("[cl] steam auth unavailable");
                    pending_auth = None;
                    auth_stopped = true;
                }
                None => {}
            }
        }

        if connected {
            if let Some(current_generation) = generation {
                reliable_chan.set_generation(current_generation);
                state_chan.set_generation(current_generation);
            }
            flush_local_reliable(&mut reliable_chan, &mut local_reliable);
            let mut parts = Vec::new();
            reliable_chan.pump(|packet| {
                if let Some(part) = bundle_part(&packet) {
                    parts.push(part);
                }
            });
            state_chan.pump(|packet| {
                if let Some(part) = bundle_part(&packet) {
                    parts.push(part);
                }
            });
            parts.extend(std::mem::take(&mut unreliable_parts));

            if let Some(current) = session {
                if !parts.is_empty() || need_ack {
                    let ack = reliable_chan.selective_ack();
                    let state_ack = state_chan.selective_ack();
                    let datagrams = pack_bundles(
                        current,
                        ack.cumulative,
                        ack.selective,
                        state_ack.cumulative,
                        state_ack.selective,
                        true,
                        parts,
                    );
                    for datagram in datagrams {
                        let _ = client.send_message(&datagram);
                        last_sent = Instant::now();
                    }
                }
            }
        }

        if connected && last_server_seen.elapsed() >= CONNECTION_TIMEOUT {
            connected = false;
            replace_session = None;
            challenge_response_bytes = None;
            pending_auth = None;
            auth_stopped = false;
            crate::network::steam::cancel_ticket();
            log::warn!("[cl] server timeout");
            let _ = tx.send(FromServer::Disconnected);
            let _ = client.send_message(&connect_packet(None));
            last_sent = Instant::now();
        } else if connected && last_sent.elapsed() >= KEEPALIVE_INTERVAL {
            if let Some(current) = session {
                let bytes =
                    wincode::serialize(&PacketType::KeepAlive { session: current }).unwrap();
                let _ = client.send_message(&bytes);
                last_sent = Instant::now();
            }
        } else if !connected && last_sent.elapsed() >= HANDSHAKE_INTERVAL {
            let _ = client.send_message(&connect_packet(replace_session));
            last_sent = Instant::now();
            if let Some(ref bytes) = challenge_response_bytes {
                let _ = client.send_message(bytes);
                last_sent = Instant::now();
            }
        }

        if !got_packet {
            wait_socket(client.socket(), &mut wake);
        }

        client.flush_sim();
    }
}

#[cfg(feature = "client")]
fn queue_client_send(
    outgoing: NetSend<ClientToServer>,
    reliable_chan: &ReliableChannel,
    local_reliable: &mut VecDeque<Vec<u8>>,
    unreliable_out: &mut u32,
    connected: bool,
    session: Option<u64>,
    unreliable_parts: &mut Vec<BundlePart>,
) {
    match outgoing {
        NetSend::Reliable(event) | NetSend::ReliableTo(_, event) | NetSend::StateTo(_, event) => {
            let payload = wincode::serialize(&event).unwrap();
            let backlog = reliable_backlog(local_reliable, reliable_chan);
            if backlog >= OUTBOUND_CAP {
                log::warn!("[cl] reliable outbound full");
            } else {
                local_reliable.push_back(payload);
            }
        }
        NetSend::Unreliable(event) | NetSend::UnreliableTo(_, event) => {
            queue_client_unreliable(unreliable_out, connected, session, &event, unreliable_parts);
        }
    }
}

#[cfg(feature = "client")]
fn flush_local_reliable(
    reliable_chan: &mut ReliableChannel,
    local_reliable: &mut VecDeque<Vec<u8>>,
) {
    loop {
        let status = match local_reliable.front() {
            Some(payload) => reliable_chan.enqueue_bytes(payload.clone()),
            None => {
                break;
            }
        };

        match status {
            EnqueueStatus::Queued => {
                local_reliable.pop_front();
            }
            EnqueueStatus::Full => {
                break;
            }
            EnqueueStatus::TooLarge => {
                log::warn!("[cl] reliable payload too large");
                local_reliable.pop_front();
            }
        }
    }
}

#[cfg(feature = "client")]
fn reliable_backlog(local_reliable: &VecDeque<Vec<u8>>, reliable_chan: &ReliableChannel) -> usize {
    local_reliable.len() + reliable_chan.queued_messages()
}

#[cfg(feature = "client")]
fn auth_response(token: u64, steam_id: u64, ticket: Vec<u8>, name: String) -> Option<Vec<u8>> {
    let bytes = wincode::serialize(&PacketType::ChallengeResponse {
        token,
        steam_id,
        ticket,
        name,
    })
    .unwrap();
    if bytes.len() > MAX_DATAGRAM {
        return None;
    }

    Some(bytes)
}

#[cfg(feature = "client")]
fn connect_packet(replace: Option<u64>) -> Vec<u8> {
    wincode::serialize(&PacketType::Connect { replace }).unwrap()
}

#[cfg(feature = "client")]
fn begin_reconnect(
    client: &NetworkClient,
    reliable_chan: &mut ReliableChannel,
    state_chan: &mut ReliableChannel,
    local_reliable: &mut VecDeque<Vec<u8>>,
    connected: &mut bool,
    session: &mut Option<u64>,
    generation: &mut Option<u32>,
    replace_session: &mut Option<u64>,
    challenge_response_bytes: &mut Option<Vec<u8>>,
    unreliable_out: &mut u32,
    unreliable_in: &mut UnreliableInbox,
    unreliable_assembly: &mut UnreliableAssembly,
    last_sent: &mut Instant,
) {
    reclaim_reliable(reliable_chan, *generation, local_reliable);
    *replace_session = *session;
    *connected = false;
    *session = None;
    *generation = None;
    *challenge_response_bytes = None;
    crate::network::steam::cancel_ticket();
    *unreliable_out = 0;
    *unreliable_in = UnreliableInbox::new();
    *unreliable_assembly = UnreliableAssembly::new();
    *reliable_chan = ReliableChannel::new();
    *state_chan = ReliableChannel::with_stream(STREAM_STATE);
    let _ = client.send_message(&connect_packet(*replace_session));
    *last_sent = Instant::now();
}

#[cfg(feature = "client")]
fn reclaim_reliable(
    channel: &mut ReliableChannel,
    generation: Option<u32>,
    local_reliable: &mut VecDeque<Vec<u8>>,
) {
    let Some(_generation) = generation else {
        return;
    };

    let pending = channel.take_unacked();
    let mut restored = VecDeque::new();
    for payload in pending {
        restored.push_back(payload);
    }

    while restored.len() + local_reliable.len() > OUTBOUND_CAP && !local_reliable.is_empty() {
        local_reliable.pop_back();
    }

    while restored.len() > OUTBOUND_CAP {
        restored.pop_back();
    }

    restored.append(local_reliable);
    *local_reliable = restored;
}

#[cfg(feature = "client")]
fn queue_client_unreliable(
    unreliable_out: &mut u32,
    connected: bool,
    session: Option<u64>,
    event: &ClientToServer,
    parts: &mut Vec<BundlePart>,
) {
    if !connected || session.is_none() {
        return;
    }

    let payload = Arc::new(wincode::serialize(event).unwrap());
    let sequence = *unreliable_out;
    let split = split_unreliable(sequence, payload);
    if split.is_empty() {
        return;
    }

    *unreliable_out = unreliable_out.wrapping_add(1);
    parts.extend(split);
}

#[cfg(feature = "client")]
fn push_client_reliable(
    reliable_chan: &mut ReliableChannel,
    tx: &Sender<FromServer>,
    connected: bool,
    session: Option<u64>,
    generation: Option<u32>,
    packet_session: u64,
    packet_generation: u32,
    sequence: u32,
    body: ReliableBody,
    last_server_seen: &mut Instant,
) -> bool {
    if !connected || session != Some(packet_session) {
        return false;
    }

    *last_server_seen = Instant::now();

    let result = reliable_chan.receive(sequence, body);
    if !result.ack {
        return false;
    }

    let Some(generation) = generation else {
        return true;
    };

    if packet_generation != generation {
        return true;
    }

    for payload in result.messages {
        // Forward event
        if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
            let _ = tx.send(FromServer::Message(event));
        }
    }

    true
}

#[cfg(feature = "client")]
fn apply_client_part(
    reliable_chan: &mut ReliableChannel,
    state_chan: &mut ReliableChannel,
    tx: &Sender<FromServer>,
    unreliable_in: &mut UnreliableInbox,
    unreliable_assembly: &mut UnreliableAssembly,
    connected: bool,
    session: Option<u64>,
    generation: Option<u32>,
    incoming: u64,
    part: BundlePart,
    last_server_seen: &mut Instant,
) -> bool {
    match part {
        BundlePart::Reliable {
            stream,
            sequence,
            generation: packet_generation,
            payload,
        } => {
            let channel = if stream == STREAM_STATE {
                state_chan
            } else {
                reliable_chan
            };

            push_client_reliable(
                channel,
                tx,
                connected,
                session,
                generation,
                incoming,
                packet_generation,
                sequence,
                ReliableBody::Complete(owned_payload(payload)),
                last_server_seen,
            )
        }
        BundlePart::Fragment {
            stream,
            sequence,
            generation: packet_generation,
            packet_id,
            fragment_idx,
            total_fragments,
            data,
        } => {
            let channel = if stream == STREAM_STATE {
                state_chan
            } else {
                reliable_chan
            };

            push_client_reliable(
                channel,
                tx,
                connected,
                session,
                generation,
                incoming,
                packet_generation,
                sequence,
                ReliableBody::Fragment {
                    packet_id,
                    fragment_idx,
                    total_fragments,
                    data: owned_payload(data),
                },
                last_server_seen,
            )
        }
        BundlePart::Unreliable { sequence, payload } => {
            if connected && session == Some(incoming) {
                if let Some(payload) = take_unreliable(
                    unreliable_in,
                    unreliable_assembly,
                    sequence,
                    owned_payload(payload),
                ) {
                    *last_server_seen = Instant::now();
                    if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                        let _ = tx.send(FromServer::Message(event));
                    }
                }
            }

            false
        }
        BundlePart::UnreliableFragment {
            sequence,
            fragment_idx,
            total_fragments,
            data,
        } => {
            if connected && session == Some(incoming) {
                if let Some(payload) = unreliable_assembly.push(
                    unreliable_in,
                    sequence,
                    fragment_idx,
                    total_fragments,
                    owned_payload(data),
                ) {
                    *last_server_seen = Instant::now();
                    if let Ok(event) = wincode::deserialize::<ServerToClient>(&payload) {
                        let _ = tx.send(FromServer::Message(event));
                    }
                }
            }

            false
        }
    }
}

fn apply_anim_model(game: &mut GameState<FromServer, ClientToServer>, model: EntityModel) {
    let mesh = game.anims.load_mesh(&model.mesh);
    let clips = game.anims.load_clips(&model.clips);
    let Some(entity) = game.entities.get_mut(model.handle) else {
        return;
    };

    if let (Ok(mesh), Ok(clips)) = (mesh, clips) {
        entity.base_mut().anim.mesh = mesh;
        entity.base_mut().anim.clips = clips;
    }
}

fn apply_anim_bones(game: &mut GameState<FromServer, ClientToServer>, bones: EntityBones) {
    game.anims.apply_bone_net(bones.handle.0, &bones.bones);
}

fn apply_spawn(
    game: &mut GameState<FromServer, ClientToServer>,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    now: f64,
    interval: f64,
    entity: EntitySnapshot,
    owner: EntityHandle,
    local: EntityHandle,
    vars: &[NetVar],
) {
    if entity.class_hash == Player::CLASS_HASH {
        let mut player = Player::new();
        player.health = entity.health;
        player.base.position = entity.position;
        player.base.angles = entity.angles;
        player.base.velocity = entity.velocity;
        player.base.owner = owner;
        player.body.noclip = entity.noclip;
        game.entities.insert_at(entity.handle, Box::new(player));

        if !vars.is_empty() {
            game.apply_networked(
                &[EntityNetworked {
                    handle: entity.handle,
                    vars: vars.to_vec(),
                }],
                now,
            );
        }
    } else if !spawn_scripted(
        game,
        now,
        entity.handle,
        entity.class_hash,
        entity.position,
        entity.angles,
        entity.velocity,
        owner,
        vars,
    ) {
        return;
    }

    if let Some(spawned) = game.entities.get_mut(entity.handle) {
        spawned.base_mut().anim.apply_remote(&entity.anim, 0);
    }

    if entity.handle == local || (!local.is_null() && owner == local) {
        return;
    }

    note_remote(
        remotes,
        entity.handle,
        NetPose {
            tick: 0,
            time: now,
            position: entity.position,
            angles: entity.angles,
            velocity: entity.velocity,
        },
        interval,
    );
}

fn spawn_scripted(
    game: &mut GameState<FromServer, ClientToServer>,
    now: f64,
    handle: EntityHandle,
    class_hash: u32,
    position: Vector3,
    angles: Angle3,
    velocity: Vector3,
    owner: EntityHandle,
    vars: &[NetVar],
) -> bool {
    let mut entity = ScriptedEntity::new(class_hash);
    entity.spawned = true;
    entity.base.position = position;
    entity.base.angles = angles;
    entity.base.velocity = velocity;
    entity.base.owner = owner;

    if !game.entities.insert_at(handle, Box::new(entity)) {
        return false;
    }

    if game.net_spawn(handle, vars, now) {
        return true;
    }

    log::warn!("[cl] unknown class {}", class_hash);
    game.entities.remove(handle);
    game.sync_entities();

    false
}
