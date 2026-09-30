mod config;

use config::{hosts, renderers, resolve_base, Settings};
use eframe::egui::{self, Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, Vec2};
use std::process::Command;

const BG: Color32 = Color32::from_rgb(16, 17, 20);
const SURFACE: Color32 = Color32::from_rgb(26, 28, 34);
const FIELD: Color32 = Color32::from_rgb(36, 39, 48);
const FIELD_HOVER: Color32 = Color32::from_rgb(46, 50, 60);
const LINE: Color32 = Color32::from_rgb(52, 56, 68);
const TEXT: Color32 = Color32::from_rgb(236, 234, 228);
const MUTED: Color32 = Color32::from_rgb(132, 136, 148);
const ACCENT: Color32 = Color32::from_rgb(214, 132, 58);
const ACCENT_INK: Color32 = Color32::from_rgb(28, 18, 10);
const ERROR: Color32 = Color32::from_rgb(216, 88, 78);

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 460.0])
            .with_min_inner_size([400.0, 460.0])
            .with_max_inner_size([400.0, 520.0])
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
}

impl LauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx);

        Self {
            settings: config::load(),
            status: None,
            close: false,
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

        egui::TopBottomPanel::bottom("launch")
            .frame(
                Frame::NONE
                    .fill(BG)
                    .inner_margin(Margin {
                        left: 24,
                        right: 24,
                        top: 0,
                        bottom: 24,
                    })
                    .stroke(Stroke::NONE),
            )
            .exact_height(if self.status.is_some() { 86.0 } else { 64.0 })
            .show_separator_line(false)
            .show(ctx, |ui| {
                let launch = egui::Button::new(
                    RichText::new("Launch")
                        .size(15.0)
                        .color(ACCENT_INK)
                        .strong(),
                )
                .fill(ACCENT)
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(7))
                .min_size(Vec2::new(ui.available_width(), 40.0));

                if ui.add(launch).clicked() {
                    self.launch();
                }

                if let Some(status) = &self.status {
                    ui.add_space(8.0);
                    ui.label(RichText::new(status).size(12.0).color(ERROR));
                }
            });

        egui::CentralPanel::default()
            .frame(
                Frame::NONE.fill(BG).inner_margin(Margin {
                    left: 24,
                    right: 24,
                    top: 22,
                    bottom: 12,
                }),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("ENGINE")
                        .font(FontId::proportional(26.0))
                        .color(TEXT)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Graphics and session settings")
                        .size(12.5)
                        .color(MUTED),
                );
                ui.add_space(18.0);

                Frame::new()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0_f32, LINE))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        egui::Grid::new("settings")
                            .num_columns(2)
                            .spacing([16.0, 12.0])
                            .min_col_width(72.0)
                            .show(ui, |ui| {
                                row_label(ui, "Renderer");
                                combo(
                                    ui,
                                    "renderer",
                                    &mut self.settings.renderer,
                                    &renderers(),
                                    display_renderer,
                                );
                                ui.end_row();

                                row_label(ui, "Host");
                                combo(
                                    ui,
                                    "host",
                                    &mut self.settings.host,
                                    hosts(),
                                    display_host,
                                );
                                ui.end_row();

                                ui.add_space(4.0);
                                ui.end_row();

                                row_label(ui, "Map");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.settings.map)
                                        .desired_width(f32::INFINITY)
                                        .margin(Margin::symmetric(10, 6)),
                                );
                                ui.end_row();

                                row_label(ui, "Tickrate");
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut self.settings.tickrate)
                                            .range(1..=1000)
                                            .speed(1.0)
                                            .min_decimals(0),
                                    );
                                    ui.label(RichText::new("Hz").size(12.0).color(MUTED));
                                });
                                ui.end_row();

                                ui.label("");
                                ui.checkbox(&mut self.settings.editor, "Open map editor");
                                ui.end_row();
                            });
                    });
            });
    }
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
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(78, 84, 100));
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

fn row_label(ui: &mut egui::Ui, text: &str) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.label(RichText::new(text).size(13.0).color(MUTED));
    });
}

fn combo(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut String,
    options: &[&str],
    display: fn(&str) -> &str,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(display(value))
        .width(ui.available_width().max(180.0))
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
