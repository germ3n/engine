mod config;

use config::{hosts, renderers, resolve_base, Settings};
use eframe::egui::{
    self, Color32, CornerRadius, FontId, Frame, Margin, Pos2, Rect, RichText, Sense, Stroke, Ui,
    Vec2,
};
use std::process::Command;

const BG: Color32 = Color32::from_rgb(14, 15, 18);
const SURFACE: Color32 = Color32::from_rgb(24, 26, 32);
const FIELD: Color32 = Color32::from_rgb(34, 37, 46);
const FIELD_HOVER: Color32 = Color32::from_rgb(48, 42, 36);
const LINE: Color32 = Color32::from_rgb(52, 56, 68);
const TEXT: Color32 = Color32::from_rgb(236, 234, 228);
const MUTED: Color32 = Color32::from_rgb(132, 136, 148);
const ACCENT: Color32 = Color32::from_rgb(214, 132, 58);
const ACCENT_SOFT: Color32 = Color32::from_rgb(84, 52, 28);
const ACCENT_INK: Color32 = Color32::from_rgb(28, 18, 10);
const ERROR: Color32 = Color32::from_rgb(216, 88, 78);

const RAIL_COLLAPSED: f32 = 52.0;
const RAIL_EXPANDED: f32 = 248.0;
const DRAWER_H: f32 = 68.0;
const SLIDE_SECS: f32 = 0.2;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([640.0, 440.0])
            .with_min_inner_size([640.0, 440.0])
            .with_max_inner_size([640.0, 440.0])
            .with_resizable(false),
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Engine",
        options,
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}

struct LauncherApp {
    settings: Settings,
    status: Option<String>,
    close: bool,
    open_drawer: Option<&'static str>,
    pinned_drawer: Option<&'static str>,
    appear: f32,
}

impl LauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);

        Self {
            settings: config::load(),
            status: None,
            close: false,
            open_drawer: None,
            pinned_drawer: None,
            appear: 0.0,
        }
    }

    fn launch(&mut self) {
        if let Err(err) = config::save(&self.settings) {
            self.status = Some(format!("Could not save settings: {err}"));

            return;
        }

        let Some(base) = resolve_base() else {
            self.status = Some("Could not find the base binary next to this launcher.".to_string());

            return;
        };

        let mut command = Command::new(&base);

        if self.settings.renderer != "auto" {
            command.env("ENGINE_GFX", &self.settings.renderer);
        }

        if self.settings.host != "winit" {
            command.env("ENGINE_HOST", &self.settings.host);
        }

        let map = self.settings.map.trim();

        if !map.is_empty() {
            command.arg("--map").arg(map);
        }

        command
            .arg("--tickrate")
            .arg(self.settings.tickrate.to_string());

        if self.settings.editor {
            command.arg("--editor");
        }

        match command.spawn() {
            Ok(_) => {
                self.close = true;
            }
            Err(err) => {
                self.status = Some(format!("Failed to start: {err}"));
            }
        }
    }
}

impl eframe::App for LauncherApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);

            return;
        }

        self.appear = (self.appear + ctx.input(|i| i.stable_dt) / 0.45).min(1.0);

        if self.appear < 1.0 {
            ctx.request_repaint();
        }

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(BG))
            .show(ctx, |ui| {
                let full = ui.max_rect();
                paint_atmosphere(ui, full);

                let rail_w = RAIL_EXPANDED + 18.0;
                let brand = Rect::from_min_max(
                    full.min + Vec2::new(36.0, 28.0),
                    Pos2::new(full.max.x - rail_w, full.max.y - 28.0),
                );
                let rail = Rect::from_min_max(
                    Pos2::new(full.max.x - rail_w, full.min.y + 24.0),
                    full.max - Vec2::new(0.0, 24.0),
                );

                let appear = self.appear;
                let status = self.status.clone();
                let mut launch = false;

                ui.scope_builder(egui::UiBuilder::new().max_rect(brand), |ui| {
                    launch = brand_panel(ui, appear, &status);
                });

                ui.scope_builder(egui::UiBuilder::new().max_rect(rail), |ui| {
                    self.draw_rail(ui);
                });

                if launch {
                    self.launch();
                }
            });
    }
}

impl LauncherApp {
    fn draw_rail(&mut self, ui: &mut Ui) {
        let mut y = ui.max_rect().min.y;
        let right = ui.max_rect().max.x;
        let gap = 8.0;
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let popup_open = ui.memory(|m| m.any_popup_open());

        let drawers: [Drawer; 5] = [
            Drawer {
                id: "renderer",
                label: "GFX",
                title: "Renderer",
                kind: DrawerKind::Combo {
                    value_key: "renderer",
                },
            },
            Drawer {
                id: "host",
                label: "HOST",
                title: "Host",
                kind: DrawerKind::Combo { value_key: "host" },
            },
            Drawer {
                id: "map",
                label: "MAP",
                title: "Map",
                kind: DrawerKind::Map,
            },
            Drawer {
                id: "tick",
                label: "TICK",
                title: "Tickrate",
                kind: DrawerKind::Tick,
            },
            Drawer {
                id: "editor",
                label: "EDIT",
                title: "Editor",
                kind: DrawerKind::Editor,
            },
        ];

        let mut slots = Vec::with_capacity(drawers.len());

        for drawer in &drawers {
            let hit = Rect::from_min_max(
                Pos2::new(right - RAIL_EXPANDED, y),
                Pos2::new(right, y + DRAWER_H + gap),
            );
            slots.push((drawer.id, hit));
            y += DRAWER_H + gap;
        }

        let mut next_open = pointer.and_then(|pos| {
            slots
                .iter()
                .find(|(_, hit)| hit.contains(pos))
                .map(|(id, _)| *id)
        });

        if next_open.is_none() {
            if let Some(id) = self.open_drawer {
                if popup_open || self.pinned_drawer == Some(id) {
                    next_open = Some(id);
                }
            }
        }

        if next_open != self.open_drawer {
            self.open_drawer = next_open;
        }

        let mut animating = false;

        for (drawer, (_, hit)) in drawers.iter().zip(slots.iter()) {
            let open = self.open_drawer == Some(drawer.id);
            let t = ui.ctx().animate_bool_with_time(
                egui::Id::new(("drawer", drawer.id)),
                open,
                SLIDE_SECS,
            );
            let eased = ease_out_cubic(t);

            if t > 0.001 && t < 0.999 {
                animating = true;
            }

            let width = egui::lerp(RAIL_COLLAPSED..=RAIL_EXPANDED, eased);
            let rect = Rect::from_min_size(
                Pos2::new(right - width, hit.min.y),
                Vec2::new(width, DRAWER_H),
            );

            if let Some(pin) = draw_drawer(ui, rect, hit, drawer, &mut self.settings, eased, open) {
                if pin {
                    self.pinned_drawer = Some(drawer.id);
                } else if self.pinned_drawer == Some(drawer.id) {
                    self.pinned_drawer = None;
                }
            }
        }

        if animating {
            ui.ctx().request_repaint();
        }
    }
}

struct Drawer {
    id: &'static str,
    label: &'static str,
    title: &'static str,
    kind: DrawerKind,
}

enum DrawerKind {
    Combo { value_key: &'static str },
    Map,
    Tick,
    Editor,
}

fn draw_drawer(
    ui: &mut Ui,
    rect: Rect,
    hit: &Rect,
    drawer: &Drawer,
    settings: &mut Settings,
    t: f32,
    open: bool,
) -> Option<bool> {
    let _ = ui.interact(*hit, egui::Id::new(("hit", drawer.id)), Sense::hover());
    let fill = lerp_color(SURFACE, FIELD_HOVER, t);
    let edge = lerp_color(LINE, ACCENT, t);

    ui.painter().rect(
        rect,
        CornerRadius {
            nw: 8,
            ne: 0,
            sw: 8,
            se: 0,
        },
        fill,
        Stroke::new(1.0_f32, edge),
        egui::StrokeKind::Inside,
    );

    let accent_bar = Rect::from_min_max(
        Pos2::new(rect.max.x - 3.0, rect.min.y + 8.0),
        Pos2::new(rect.max.x, rect.max.y - 8.0),
    );
    ui.painter().rect_filled(
        accent_bar,
        CornerRadius::ZERO,
        lerp_color(ACCENT_SOFT, ACCENT, t),
    );

    let label_pos = Pos2::new(rect.max.x - RAIL_COLLAPSED * 0.5, rect.center().y);
    ui.painter().text(
        label_pos,
        egui::Align2::CENTER_CENTER,
        drawer.label,
        FontId::proportional(11.0),
        lerp_color(MUTED, TEXT, t.max(0.35)),
    );

    let mut pin = None;

    if t > 0.2 && open {
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_max(
                rect.min + Vec2::new(14.0, 8.0),
                Pos2::new(rect.max.x - RAIL_COLLAPSED - 4.0, rect.max.y - 8.0),
            )),
            |ui| {
                ui.set_opacity(((t - 0.2) / 0.8).clamp(0.0, 1.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(drawer.title).size(11.0).color(MUTED));
                    ui.add_space(2.0);

                    match drawer.kind {
                        DrawerKind::Combo { value_key } => match value_key {
                            "renderer" => {
                                combo(
                                    ui,
                                    value_key,
                                    &mut settings.renderer,
                                    &renderers(),
                                    display_renderer,
                                );
                            }
                            "host" => {
                                combo(
                                    ui,
                                    value_key,
                                    &mut settings.host,
                                    hosts(),
                                    display_host,
                                );
                            }
                            _ => {}
                        },
                        DrawerKind::Map => {
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut settings.map)
                                    .id_salt(("map", drawer.id))
                                    .desired_width(f32::INFINITY)
                                    .margin(Margin::symmetric(8, 4))
                                    .hint_text("map name"),
                            );
                            pin = Some(edit.has_focus());
                        }
                        DrawerKind::Tick => {
                            ui.horizontal(|ui| {
                                let drag = ui.add(
                                    egui::DragValue::new(&mut settings.tickrate)
                                        .range(1..=1000)
                                        .speed(1.0)
                                        .min_decimals(0),
                                );
                                pin = Some(drag.has_focus());
                                ui.label(RichText::new("Hz").size(12.0).color(MUTED));
                            });
                        }
                        DrawerKind::Editor => {
                            ui.checkbox(&mut settings.editor, "Open map editor");
                        }
                    }
                });
            },
        );
    }

    pin
}

fn brand_panel(ui: &mut Ui, appear: f32, status: &Option<String>) -> bool {
    ui.set_opacity(0.35 + 0.65 * appear);
    let shift = (1.0 - appear) * 14.0;
    ui.add_space(shift);

    ui.label(
        RichText::new("ENGINE")
            .font(FontId::proportional(42.0))
            .color(TEXT)
            .strong(),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new("Hover the rail to tune the session.")
            .size(13.0)
            .color(MUTED),
    );

    ui.add_space(28.0);

    let launch = egui::Button::new(
        RichText::new("Launch")
            .size(15.0)
            .color(ACCENT_INK)
            .strong(),
    )
    .fill(ACCENT)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(8))
    .min_size(Vec2::new(168.0, 42.0));

    let clicked = ui.add(launch).clicked();

    if let Some(status) = status {
        ui.add_space(12.0);
        ui.label(RichText::new(status).size(12.0).color(ERROR));
    }

    clicked
}

fn paint_atmosphere(ui: &mut Ui, full: Rect) {
    let painter = ui.painter();
    painter.rect_filled(full, CornerRadius::ZERO, BG);

    let wash = Rect::from_min_size(
        Pos2::new(full.max.x - 280.0, full.min.y),
        Vec2::new(280.0, full.height()),
    );
    painter.rect_filled(
        wash,
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(214, 132, 58, 18),
    );

    painter.line_segment(
        [
            Pos2::new(full.max.x - RAIL_EXPANDED - 28.0, full.min.y + 40.0),
            Pos2::new(full.max.x - RAIL_EXPANDED - 28.0, full.max.y - 40.0),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(38, 40, 48)),
    );
}

fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = BG;
    visuals.panel_fill = BG;
    visuals.extreme_bg_color = FIELD;
    visuals.faint_bg_color = SURFACE;
    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);
    visuals.widgets.inactive.bg_fill = FIELD;
    visuals.widgets.inactive.weak_bg_fill = FIELD;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
    visuals.widgets.hovered.bg_fill = FIELD_HOVER;
    visuals.widgets.hovered.weak_bg_fill = FIELD_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(120, 84, 52));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
    visuals.widgets.active.bg_fill = FIELD_HOVER;
    visuals.widgets.active.weak_bg_fill = FIELD_HOVER;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.corner_radius = CornerRadius::same(6);
    visuals.widgets.open.bg_fill = FIELD;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.open.corner_radius = CornerRadius::same(6);
    visuals.selection.bg_fill = Color32::from_rgb(120, 72, 32);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.override_text_color = Some(TEXT);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(6);
    visuals.window_stroke = Stroke::new(1.0_f32, LINE);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.interact_size = Vec2::new(40.0, 28.0);
    style.visuals = ctx.style().visuals.clone();
    ctx.set_style(style);
}

fn combo(
    ui: &mut Ui,
    id: &str,
    value: &mut String,
    options: &[&str],
    display: fn(&str) -> &str,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(display(value))
        .width(ui.available_width().max(140.0))
        .show_ui(ui, |ui| {
            for name in options {
                ui.selectable_value(value, name.to_string(), display(name));
            }
        });
}

fn display_renderer(name: &str) -> &str {
    match name {
        "auto" => "Auto",
        "metal" => "Metal",
        "vulkan" => "Vulkan",
        "opengl" => "OpenGL",
        "d3d12" => "Direct3D 12",
        "d3d11" => "Direct3D 11",
        other => other,
    }
}

fn display_host(name: &str) -> &str {
    match name {
        "winit" => "winit",
        "sdl2" => "SDL2",
        "xbox" => "Xbox",
        other => other,
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| -> u8 { ((x as f32) + (y as f32 - x as f32) * t).round() as u8 };

    Color32::from_rgba_unmultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}
