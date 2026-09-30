use crate::entities::context::FrameInfo;
use crate::entities::{EntityHandle, Player};
use crate::input::Action;
use crate::movement::{self, NetPose, Prediction, UserCommand};
use crate::network::events::EntitySnapshot;
use crate::network::packet::{
    bundle_part, owned_payload, pack_bundles, split_unreliable, BundlePart, CONNECTION_TIMEOUT,
    KEEPALIVE_INTERVAL, STREAM_STATE,
};
use crate::network::usermessage::UserMsgReader;
use crate::network::wait_socket;
use crate::network::{
    take_unreliable, ClientToServer, EnqueueStatus, FromServer, NetSend, NetworkClient, PacketType,
    ReliableBody, ReliableChannel, ServerToClient, UnreliableAssembly, UnreliableInbox,
    OUTBOUND_CAP, RECV_BUDGET,
};
use crate::platform::{
    DeviceEvent, ElementState, Event, HostKind, KeyCode, MouseButton, PlatformHost, Touch,
    TouchPhase, WindowEvent,
};
use crate::r#enum::InputButtons;
use crate::script::engine::DrawCommand;
use crate::script::libs::angle3::Angle3;
use crate::script::libs::vector3::Vector3;
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
}

struct SnapshotIngress {
    generation: u32,
    reset: bool,
    part_count: u16,
    filled: u16,
    parts: Vec<Option<Vec<EntitySnapshot>>>,
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
            self.parts[part as usize] = Some(entities);
            self.filled = self.filled.saturating_add(1);
        }

        if self.filled != part_count {
            return None;
        }

        let mut built_entities = Vec::new();
        for slot in self.parts.drain(..) {
            if let Some(batch) = slot {
                built_entities.extend(batch);
            }
        }

        let built = BuiltSnapshot {
            reset: self.reset,
            entities: built_entities,
        };
        self.filled = 0;
        self.part_count = 0;

        Some(built)
    }
}

pub fn client_loop(mut game: GameState<FromServer, ClientToServer>, shutdown: Arc<AtomicBool>) {
    let (host, mut held_window) = client_surface();
    let binds = game.binds.clone();

    crate::script::exec(&game.script_engine.lua, "menu.lua", "lua/menu/menu.luac");

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
    let mut scene_world = u64::MAX;
    let mut scene_brushes = u64::MAX;
    let mut scene_revision = 0u64;
    let mut camera = FlyCamera::new();
    let mut prediction = Prediction::new();
    let mut remotes: HashMap<EntityHandle, VecDeque<NetPose>> = HashMap::new();
    let session_start = Instant::now();
    let mut captured = false;
    let mut keys = HashSet::new();
    let mut mouse = HashSet::new();
    let mut touches = Vec::new();

    host.run(move |event, host, control| {
        control.poll();

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
                WindowEvent::KeyboardInput(input) => {
                    if let Some(code) = input.key_code {
                        if code == KeyCode::Escape && input.state == ElementState::Pressed {
                            captured = false;
                            mouse.clear();
                            host.set_cursor_grabbed(false);
                        } else if input.state == ElementState::Pressed {
                            keys.insert(code);
                        } else {
                            keys.remove(&code);
                        }
                    }
                }
                WindowEvent::MouseInput { state, button } => {
                    if state == ElementState::Pressed {
                        if captured {
                            mouse.insert(button);
                        }

                        if button == MouseButton::Left {
                            captured = true;
                            host.set_cursor_grabbed(true);
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

                    if scene_world != world_revision || scene_brushes != brush_revision {
                        scene_mesh = game.voxel_world.mesh();
                        scene_mesh.extend(game.brush_world.mesh());
                        scene_world = world_revision;
                        scene_brushes = brush_revision;
                        scene_revision = scene_revision.wrapping_add(1);
                    }

                    client_window.begin_frame(0.53, 0.71, 0.85);
                    client_window.draw_colored_mesh(
                        &scene_mesh,
                        scene_revision,
                        &camera.scene(aspect, game.voxel_world.scale() as f32),
                    );

                    game.run_hook("MenuPaint", ());

                    let draw_commands = {
                        let mut q = game.script_engine.render_queue.lock().unwrap();
                        std::mem::take(&mut *q)
                    };

                    //todo: optimize
                    for cmd in draw_commands {
                        match cmd {
                            DrawCommand::Rect { x, y, w, h, color } => {
                                client_window.draw_rectangle(x, y, w, h, color);
                            }
                            DrawCommand::OutlinedRect {
                                x,
                                y,
                                w,
                                h,
                                thickness,
                                color,
                            } => {
                                client_window.draw_outlined_rectangle(x, y, w, h, thickness, color);
                            }
                            DrawCommand::Text {
                                font,
                                text,
                                x,
                                y,
                                scale,
                                color,
                            } => {
                                client_window.draw_text(
                                    &font.to_str().unwrap().to_owned(),
                                    &text.to_str().unwrap().to_owned(),
                                    x,
                                    y,
                                    scale,
                                    color,
                                );
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
                }
                _ => (),
            },
            Event::Device(DeviceEvent::MouseMotion { delta }) => {
                if captured && !client_window.vr_input().active {
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
                let now = std::time::Instant::now();
                let dt = now.duration_since(last_frame).as_secs_f64();
                last_frame = now;
                let frame_dt = (dt as f32).min(0.1);

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
                            } = message
                            {
                                log::info!(
                                    "[cl] WorldSnapshot(gen={generation} reset={reset} part={part}/{parts} ents={})",
                                    entities.len()
                                );

                                if generation == world_generation {
                                    if let Some(built) = snapshot_ingress
                                        .push(generation, reset, part, parts, entities)
                                    {
                                        if built.reset {
                                            game.entities.clear();
                                            prediction.clear();
                                            remotes.clear();
                                        }

                                        let now = session_start.elapsed().as_secs_f64();
                                        let interval = game.tick_interval;

                                        for entity in built.entities {
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
                                            );
                                        }

                                        hold_events = false;
                                        while let Some(waiting) = held.pop_front() {
                                            apply_server_event(
                                                &mut game,
                                                &mut tick_ingress,
                                                &mut prediction,
                                                &mut remotes,
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
                                &mut tick_ingress,
                                &mut prediction,
                                &mut remotes,
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

                if prediction.arm_look {
                    prediction.look.p = camera.pitch.to_degrees();
                    prediction.look.y = camera.yaw.to_degrees();
                    prediction.look.r = 0.0;
                    prediction.arm_look = false;
                }

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

                    if possessed {
                        predict_tick(&mut game, &mut prediction, buttons, forward, right, vr.yaw);
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
                        place_camera(
                            &mut camera,
                            origin,
                            prediction.look,
                            movement::eye_height(buttons),
                        );
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

                host.request_redraw();
            }
            _ => (),
        }
    });
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

fn place_camera(camera: &mut FlyCamera, origin: Vector3, look: Angle3, eye: f64) {
    camera.x = origin.x as f32;
    camera.y = origin.y as f32;
    camera.z = (origin.z + eye) as f32;
    camera.yaw = look.y.to_radians();
    camera.pitch = look.p.to_radians();
}

fn predict_tick(
    game: &mut GameState<FromServer, ClientToServer>,
    prediction: &mut Prediction,
    buttons: InputButtons,
    forward: f32,
    right: f32,
    vr_yaw: f32,
) {
    if !game.entities.is_valid(prediction.local) {
        return;
    }

    let (mut position, mut velocity, mut angles) = {
        let Some(entity) = game.entities.get(prediction.local) else {
            return;
        };

        let base = entity.base();

        (base.position, base.velocity, base.angles)
    };
    let cmd = UserCommand {
        tick: game.tick_count,
        buttons,
        wish: Vector3::new(forward as f64, right as f64, 0.0),
        view: command_view(prediction.look, vr_yaw),
    };
    let prev = prediction.previous();
    let from = position;
    let dt = game.tick_interval;
    let gravity = movement::gravity(&game.cvars);
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
    );
    prediction.note_step(from, position);
    prediction.push(cmd.clone());

    if let Some(entity) = game.entities.get_mut(prediction.local) {
        let base = entity.base_mut();
        base.position = position;
        base.velocity = velocity;
        base.angles = angles;
    }

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
    prediction.replay(
        &mut position,
        &mut velocity,
        &mut angles,
        dt,
        gravity,
        &game.brush_world,
        &game.voxel_world,
    );

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

    prediction.correct_view(position);

    game.run_hook(
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
        if *handle == local {
            continue;
        }

        if let Some(pose) = movement::blend_poses(samples, render_time, interval) {
            visual.push((*handle, pose));
        }

        movement::forget_old_poses(samples, render_time);
    }

    let mut idx = 0;

    while idx < visual.len() {
        let (handle, pose) = visual[idx];

        if let Some(entity) = game.entities.get_mut(handle) {
            let base = entity.base_mut();
            base.position = pose.position;
            base.angles = pose.angles;
            base.velocity = pose.velocity;
        }

        idx += 1;
    }
}

fn apply_server_event(
    game: &mut GameState<FromServer, ClientToServer>,
    tick_ingress: &mut TickIngress,
    prediction: &mut Prediction,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    now: f64,
    message: ServerToClient,
) {
    match &message {
        ServerToClient::TickState { .. }
        | ServerToClient::Pong { .. }
        | ServerToClient::ServerTick { .. }
        | ServerToClient::VoxelChunk(_) => {
            log::debug!("[cl] {}", message.summary());
        }
        _ => {
            log::info!("[cl] {}", message.summary());
        }
    }

    match message {
        ServerToClient::PlayerConnected { handle, name } => {
            game.run_hook("PlayerConnected", (handle, name));
        }
        ServerToClient::PlayerDisconnected { handle } => {
            game.run_hook("PlayerDisconnected", handle);
        }
        ServerToClient::PlayerSpawned { handle } => {
            prediction.possess(handle);
            remotes.remove(&handle);
            game.run_hook("PlayerSpawned", handle);
        }
        ServerToClient::PlayerDamaged {
            handle,
            attacker,
            inflictor,
            damage,
            new_health,
        } => {
            game.run_hook(
                "PlayerDamaged",
                (handle, attacker, inflictor, damage, new_health),
            );
        }
        ServerToClient::PlayerDied {
            handle,
            killer,
            inflictor,
        } => {
            game.run_hook("PlayerDied", (handle, killer, inflictor));
        }
        ServerToClient::ModelChanged { handle, model } => {
            game.run_hook("ModelChanged", (handle, model));
        }
        ServerToClient::TransformUpdated {
            handle,
            position,
            angles,
            velocity,
        } => {
            if handle != prediction.local {
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

            game.run_hook("TransformUpdated", (handle, position, angles, velocity));
        }

        ServerToClient::UserMessage { hash, data } => {
            game.run_usermessage(hash, UserMsgReader::new(data));
        }
        ServerToClient::WorldSnapshot { .. } => {}
        ServerToClient::VoxelScale { scale } => {
            if !game.voxel_world.apply_scale(scale) {
                log::warn!("[cl] bad voxel scale {scale}");
            }
        }
        ServerToClient::VoxelChunk(update) => {
            if !game.voxel_world.apply(&update) {
                log::warn!("[cl] bad chunk {} {} {}", update.x, update.y, update.z);
            }
        }
        ServerToClient::MapChange { map_name } => {
            if let Err(err) = game.brush_world.load_file(&map_name) {
                log::warn!("[map] {err}");
            }
        }
        ServerToClient::EntitySpawned {
            handle,
            class_hash,
            position,
        } => {
            if class_hash == Player::CLASS_HASH && !game.entities.is_valid(handle) {
                let mut player = Player::new();
                player.base.position = position;
                game.entities.insert_at(handle, Box::new(player));
                note_remote(
                    remotes,
                    handle,
                    NetPose {
                        tick: 0,
                        time: now,
                        position,
                        angles: Angle3::new(0.0, 0.0, 0.0),
                        velocity: Vector3::new(0.0, 0.0, 0.0),
                    },
                    game.tick_interval,
                );
            }
        }
        ServerToClient::EntityDespawned { handle } => {
            game.entities.remove(handle);
            remotes.remove(&handle);

            if prediction.local == handle {
                prediction.clear();
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
                    if snapshot.handle == prediction.local {
                        reconcile_player(game, prediction, &snapshot);

                        continue;
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

                    game.run_hook(
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
                PacketType::Challenge { token } => {
                    if !connected {
                        let bytes =
                            wincode::serialize(&PacketType::ChallengeResponse { token }).unwrap();
                        challenge_response_bytes = Some(bytes.clone());
                        let _ = client.send_message(&bytes);
                        last_sent = Instant::now();
                        log::info!("[cl] challenge {token}");
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

fn apply_spawn(
    game: &mut GameState<FromServer, ClientToServer>,
    remotes: &mut HashMap<EntityHandle, VecDeque<NetPose>>,
    now: f64,
    interval: f64,
    entity: EntitySnapshot,
) {
    if entity.class_hash != Player::CLASS_HASH {
        log::warn!("[cl] unknown class {}", entity.class_hash);

        return;
    }

    let mut player = Player::new();
    player.health = entity.health;
    player.base.position = entity.position;
    player.base.angles = entity.angles;
    player.base.velocity = entity.velocity;
    game.entities.insert_at(entity.handle, Box::new(player));
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
