// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2025-2026 Z1xus
// Copyright (C) 2026 Alpha-Leader

use crate::window_manager::{Anchor, DisplayInfo, Placement, PlacementMode, WindowInfo};
use egui::{
    Align, Align2, Color32, ColorImage, FontId, Layout, RichText, Sense, Stroke, Style, Visuals,
};

pub trait IconCacheInterface
{
    fn get(&mut self, key: &str) -> Option<&egui::TextureHandle>;
    fn insert(&mut self, key: String, texture: egui::TextureHandle);
    fn contains_key(&self, key: &str) -> bool;
}

// Neutral dark surfaces with a blue accent. Values are chosen so that adjacent
// elements stay visually separated: each surface step is a visible lift from the
// one behind it, and every text color except TEXT_DISABLED clears the 4.5:1
// contrast floor against BG_PANEL. Disabled text is deliberately below it.
const BG_PANEL: Color32 = Color32::from_rgb(24, 24, 28);
const BG_POPUP: Color32 = Color32::from_rgb(32, 32, 38);
const BG_INSET: Color32 = Color32::from_rgb(30, 30, 36);
const SURFACE: Color32 = Color32::from_rgb(48, 48, 57);
const SURFACE_HOVER: Color32 = Color32::from_rgb(62, 62, 73);
const SURFACE_ACTIVE: Color32 = Color32::from_rgb(76, 76, 89);
const BORDER: Color32 = Color32::from_rgb(88, 88, 99);
const BORDER_STRONG: Color32 = Color32::from_rgb(122, 122, 136);

const TEXT: Color32 = Color32::from_rgb(243, 243, 246);
const TEXT_DIM: Color32 = Color32::from_rgb(190, 190, 202);
const TEXT_FAINT: Color32 = Color32::from_rgb(150, 150, 165);
const TEXT_DISABLED: Color32 = Color32::from_rgb(112, 112, 126);

const ACCENT: Color32 = Color32::from_rgb(59, 130, 246);
const ACCENT_DEEP: Color32 = Color32::from_rgb(37, 99, 235);
const SUCCESS: Color32 = Color32::from_rgb(74, 222, 128);
const WARNING: Color32 = Color32::from_rgb(250, 190, 88);
const ERROR: Color32 = Color32::from_rgb(248, 113, 113);

fn widget(bg: Color32, border: Color32, text: Color32, expansion: f32)
-> egui::style::WidgetVisuals
{
    egui::style::WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: Stroke::new(1.0_f32, border),
        fg_stroke: Stroke::new(1.0_f32, text),
        corner_radius: egui::CornerRadius::same(4),
        expansion,
    }
}

pub fn setup_dark_theme(ctx: &egui::Context)
{
    let mut style = Style::default();

    style.visuals = Visuals {
        dark_mode: true,
        // Left unset so disabled widgets can dim via their own fg_stroke; an
        // override would force every widget to the same text color.
        override_text_color: None,
        widgets: egui::style::Widgets {
            noninteractive: widget(BG_PANEL, BORDER, TEXT, 0.0),
            inactive: widget(SURFACE, BORDER, TEXT, 0.0),
            hovered: widget(SURFACE_HOVER, BORDER_STRONG, TEXT, 1.0),
            active: widget(SURFACE_ACTIVE, ACCENT, TEXT, 1.0),
            open: widget(SURFACE, BORDER_STRONG, TEXT, 0.0),
        },
        selection: egui::style::Selection {
            bg_fill: ACCENT_DEEP,
            stroke: Stroke::new(1.0_f32, TEXT),
        },
        // Lighter than the panel and outlined, so the window dropdown reads as a
        // layer above the UI instead of blending into it.
        window_fill: BG_POPUP,
        window_stroke: Stroke::new(1.0_f32, BORDER),
        panel_fill: BG_PANEL,
        extreme_bg_color: BG_INSET,
        faint_bg_color: BG_POPUP,
        ..Default::default()
    };

    // egui keeps a separate dark and light Style and chooses between them from
    // the system theme. `set_style` only writes whichever one is resolved at the
    // time, so on a machine set to light mode the palette landed in one slot
    // while egui rendered from the other — the app came out in stock light
    // visuals. Pin the preference to dark and install the palette into both
    // slots so neither the system setting nor a mid-session theme change can
    // swap it out.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style);
}

pub fn render_header(ui: &mut egui::Ui, window_count: usize)
{
    ui.add_space(10.0);

    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(
            RichText::new(env!("CARGO_PKG_NAME"))
                .font(FontId::proportional(20.0))
                .color(TEXT),
        );

        ui.add_space(15.0);

        let counter_text = if window_count == 1 {
            format!("{} window found", window_count)
        } else {
            format!("{} windows found", window_count)
        };

        ui.label(
            RichText::new(counter_text)
                .font(FontId::proportional(12.0))
                .color(TEXT_DIM),
        );
    });

    ui.add_space(10.0);
}

pub fn render_window_selector(
    ui: &mut egui::Ui,
    windows: &[WindowInfo],
    selected_hwnd: &mut Option<isize>,
    icon_cache: &mut dyn IconCacheInterface,
)
{
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Select Window:")
                .font(FontId::proportional(13.0))
                .color(TEXT_DIM),
        );
    });

    ui.add_space(5.0);

    let selected_text = selected_hwnd
        .and_then(|hwnd| windows.iter().find(|window| window.hwnd == hwnd))
        .map_or_else(|| "Select a window...".to_string(), |window| window.display_text());

    egui::ComboBox::from_id_salt("window_selector")
        .selected_text(selected_text)
        .width(ui.available_width())
        .height(150.0)
        .show_ui(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

            for window in windows.iter() {
                ui.horizontal(|ui| {
                    ui.set_min_width(ui.available_width());
                    if let Some(icon_data) = &window.icon_data {
                        let cache_key = format!("icon_{}", window.hwnd);

                        if !icon_cache.contains_key(&cache_key) {
                            let color_image =
                                ColorImage::from_rgba_unmultiplied([16, 16], icon_data);
                            let texture = ui.ctx().load_texture(
                                &cache_key,
                                color_image,
                                egui::TextureOptions::LINEAR,
                            );
                            icon_cache.insert(cache_key.clone(), texture);
                        }

                        if let Some(texture) = icon_cache.get(&cache_key) {
                            ui.image((texture.id(), egui::vec2(16.0, 16.0)));
                        }
                    } else {
                        let (rect, _response) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());

                        let status_icon = "○";
                        let status_color = if window.is_borderless {
                            SUCCESS
                        } else {
                            TEXT_DIM
                        };

                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            status_icon,
                            FontId::proportional(12.0),
                            status_color,
                        );
                    }

                    let status_text = if window.is_borderless { "[B]" } else { "[W]" };
                    let status_color = if window.is_borderless {
                        SUCCESS
                    } else {
                        TEXT_FAINT
                    };

                    ui.label(
                        RichText::new(status_text)
                            .color(status_color)
                            .font(FontId::proportional(10.0)),
                    );

                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            let response = ui.selectable_label(
                                *selected_hwnd == Some(window.hwnd),
                                window.display_text(),
                            );
                            if response.clicked() {
                                *selected_hwnd = Some(window.hwnd);
                            }
                        },
                    );
                });
            }
        });
}

const SIZE_PRESETS: [(&str, i32, i32); 3] =
    [("4K", 3840, 2160), ("1440p", 2560, 1440), ("1080p", 1920, 1080)];

pub fn render_placement_controls(
    ui: &mut egui::Ui,
    placement: &mut Placement,
    display: Option<&DisplayInfo>,
)
{
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.add_space(5.0);
        ui.label(
            RichText::new("Placement:")
                .font(FontId::proportional(12.0))
                .color(TEXT_DIM),
        );

        ui.selectable_value(&mut placement.mode, PlacementMode::FullDisplay, "Full display");
        ui.selectable_value(&mut placement.mode, PlacementMode::Region, "Region");
    });

    ui.horizontal(|ui| {
        ui.add_space(5.0);
        ui.selectable_value(&mut placement.mode, PlacementMode::LeaveInPlace, "Leave in place");
    });

    if placement.mode == PlacementMode::Region {
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.add_space(5.0);
            ui.label(
                RichText::new("Size:")
                    .font(FontId::proportional(12.0))
                    .color(TEXT_DIM),
            );
            ui.add(egui::DragValue::new(&mut placement.width).range(1..=32768).speed(8.0));
            ui.label("x");
            ui.add(egui::DragValue::new(&mut placement.height).range(1..=32768).speed(8.0));
        });

        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.add_space(5.0);
            for (label, width, height) in SIZE_PRESETS {
                if ui.small_button(label).clicked() {
                    placement.width = width;
                    placement.height = height;
                }
            }
        });

        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.add_space(5.0);
            ui.label(
                RichText::new("Anchor:")
                    .font(FontId::proportional(12.0))
                    .color(TEXT_DIM),
            );
            ui.selectable_value(&mut placement.anchor, Anchor::Centered, "Centered");
            ui.selectable_value(&mut placement.anchor, Anchor::Left, "Left");
            ui.selectable_value(&mut placement.anchor, Anchor::Right, "Right");
            ui.selectable_value(&mut placement.anchor, Anchor::Custom, "Custom");
        });

        if placement.anchor == Anchor::Custom {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(5.0);
                ui.label(
                    RichText::new("Offset:")
                        .font(FontId::proportional(12.0))
                        .color(TEXT_DIM),
                );
                ui.add(egui::DragValue::new(&mut placement.custom_x).speed(8.0).prefix("x "));
                ui.add(egui::DragValue::new(&mut placement.custom_y).speed(8.0).prefix("y "));
            });
        }
    }

    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.add_space(5.0);
        ui.add(egui::Checkbox::new(&mut placement.always_on_top, ""));
        ui.label(
            RichText::new("Always on top")
                .font(FontId::proportional(12.0))
                .color(TEXT_DIM),
        )
        .on_hover_text(
            "Required to cover the taskbar. Windows only hides the taskbar for a window \
             that fills an entire monitor.",
        );
    });

    render_placement_preview(ui, placement, display);
}

/// Live readout of exactly where the window will land, so the numbers can be
/// checked before anything is moved.
fn render_placement_preview(
    ui: &mut egui::Ui,
    placement: &Placement,
    display: Option<&DisplayInfo>,
)
{
    let Some(display) = display else {
        return;
    };

    ui.add_space(6.0);

    let Some(rect) = placement.resolve(display) else {
        ui.horizontal(|ui| {
            ui.add_space(5.0);
            ui.label(
                RichText::new("→ window is left where it is")
                    .font(FontId::proportional(11.0))
                    .color(TEXT_FAINT),
            );
        });
        return;
    };

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let overflows = placement.overflows(display);

    let summary = format!("→ {}x{} at ({}, {})", width, height, rect.left, rect.top);

    let color =
        if overflows { WARNING } else { SUCCESS };

    ui.horizontal(|ui| {
        ui.add_space(5.0);
        ui.label(RichText::new(summary).font(FontId::proportional(11.0)).color(color));
    });

    let detail = if overflows {
        "extends past the display edge".to_string()
    } else {
        let left_gap = rect.left - display.x;
        let right_gap = (display.x + display.width) - rect.right;
        format!("sides free: {}px | {}px", left_gap, right_gap)
    };

    ui.horizontal(|ui| {
        ui.add_space(5.0);
        ui.label(
            RichText::new(detail).font(FontId::proportional(11.0)).color(TEXT_FAINT),
        );
    });
}

pub fn render_display_selector(
    ui: &mut egui::Ui,
    displays: &[DisplayInfo],
    selected_display: &mut Option<usize>,
)
{
    ui.add_space(5.0);

    ui.horizontal(|ui| {
        ui.add_space(5.0);

        ui.label(
            RichText::new("Display:")
                .font(FontId::proportional(12.0))
                .color(TEXT_DIM),
        );
    });

    ui.add_space(3.0);

    let selected_text = if let Some(index) = selected_display {
        if let Some(display) = displays.get(*index) {
            display.display_text()
        } else {
            "Select a display...".to_string()
        }
    } else {
        "Select a display...".to_string()
    };

    ui.horizontal(|ui| {
        ui.add_space(5.0);

        egui::ComboBox::from_id_salt("display_selector")
            .selected_text(selected_text)
            .width(ui.available_width() - 10.0)
            .show_ui(ui, |ui| {
                for (index, display) in displays.iter().enumerate() {
                    let response = ui
                        .selectable_label(*selected_display == Some(index), display.display_text());
                    if response.clicked() {
                        *selected_display = Some(index);
                    }
                }
            });
    });
}

/// `can_restore` means this app saved the window's original frame and can put it
/// back exactly. Without it a borderless window is left alone rather than being
/// given a frame it never had.
pub fn render_action_button(
    ui: &mut egui::Ui,
    windows: &[WindowInfo],
    selected_hwnd: Option<isize>,
    can_restore: bool,
) -> Option<isize>
{
    ui.add_space(15.0);

    let mut clicked_window = None;

    let selected =
        selected_hwnd.and_then(|hwnd| windows.iter().find(|window| window.hwnd == hwnd));

    let (button_text, button_enabled) = match selected {
        Some(_) if can_restore => ("Restore Borders", true),
        Some(window) if !window.is_borderless => ("Make Borderless", true),
        Some(_) => ("Restore Borders", false),
        None => ("Make Borderless", false),
    };

    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_enabled_ui(button_enabled, |ui| {
            let button = egui::Button::new(
                RichText::new(button_text).font(FontId::proportional(14.0)).color(
                    if button_enabled { TEXT } else { TEXT_DISABLED },
                ),
            )
            .min_size(egui::vec2(180.0, 35.0));

            if ui.add(button).clicked() && button_enabled {
                clicked_window = selected_hwnd;
            }
        });
    });

    clicked_window
}

/// Attribution to the upstream project this is derived from, alongside the
/// version and license. Kept visible in the app rather than only in the repo.
pub fn render_footer(ui: &mut egui::Ui)
{
    ui.add_space(6.0);

    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(
            RichText::new(format!(
                "{} v{}  ·  based on ihateborders by Z1xus  ·  GPL-3.0",
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .font(FontId::proportional(9.0))
            .color(TEXT_FAINT),
        );
    });
}

/// Shows the outcome of the last action. The app has no console, so failures
/// would otherwise be invisible.
pub fn render_status(ui: &mut egui::Ui, message: &str, is_error: bool)
{
    ui.add_space(8.0);

    let color = if is_error {
        ERROR
    } else {
        SUCCESS
    };

    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.label(RichText::new(message).font(FontId::proportional(11.0)).color(color));
    });
}
