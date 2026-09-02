use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;

/// Vocalis color system: a violet accent ramp on a near-black background (dark mode
/// is primary). Contrast is carried by *depth in the ramp*, not by saturation — the
/// same rule applies to every multi-segment element (the wave meter below is the
/// canonical example): the center/peak takes the brightest step, flanking segments
/// step down, and the outer edges land on the darkest step. Never fill an icon or a
/// whole control with one flat accent block.
///
/// A light theme is also available (see [`apply`], toggled from the Settings
/// tab/window). Every color used *outside* this module goes through a function
/// (`muted()`, `accent()`, `accent_300()`) rather than a bare constant, backed
/// by [`LIGHT_MODE`] below — so a call site never has to know or care which
/// theme is active, it just asks "what does this color look like right now".
/// `DANGER`/`WARN`/`OK` stay plain constants: they're deliberately outside the
/// accent ramp specifically so they read the same regardless of theme (a
/// saturated status red/amber/green has enough contrast against both a
/// near-black and a near-white background).
static LIGHT_MODE: AtomicBool = AtomicBool::new(false);

/// Whether the light theme is currently active — set by [`apply`], read by
/// every theme-aware color function in this module.
pub fn is_light() -> bool {
    LIGHT_MODE.load(Ordering::Relaxed)
}

pub const BG: egui::Color32 = egui::Color32::from_rgb(20, 22, 28);
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(28, 31, 39);
const LIGHT_BG: egui::Color32 = egui::Color32::from_rgb(246, 245, 250);
const LIGHT_PANEL: egui::Color32 = egui::Color32::from_rgb(255, 255, 255);

/// The ramp, lightest to darkest. `ACCENT` (unnumbered) sits between 300 and 500 —
/// it's the "peak" step, used for the single brightest element in a group.
pub const ACCENT_300: egui::Color32 = egui::Color32::from_rgb(201, 191, 242);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(167, 139, 250);
pub const ACCENT_500: egui::Color32 = egui::Color32::from_rgb(143, 123, 224);
pub const ACCENT_600: egui::Color32 = egui::Color32::from_rgb(122, 102, 199);
pub const ACCENT_700: egui::Color32 = egui::Color32::from_rgb(91, 75, 138);

/// Light-theme accent ramp — same violet hue family as the dark one above, but
/// shifted darker across the board so each step still has real contrast
/// against a near-white background (the dark-theme ramp's lightest steps
/// would wash out there, especially as a *text* color).
const LIGHT_ACCENT_300: egui::Color32 = egui::Color32::from_rgb(124, 92, 219);
const LIGHT_ACCENT: egui::Color32 = egui::Color32::from_rgb(105, 74, 201);
const LIGHT_ACCENT_500: egui::Color32 = egui::Color32::from_rgb(90, 62, 176);
const LIGHT_ACCENT_600: egui::Color32 = egui::Color32::from_rgb(75, 51, 148);
const LIGHT_ACCENT_700: egui::Color32 = egui::Color32::from_rgb(56, 40, 110);

/// The accent color for whichever theme is currently active. Use this (never
/// the bare `ACCENT` constant) from outside this module.
pub fn accent() -> egui::Color32 {
    if is_light() { LIGHT_ACCENT } else { ACCENT }
}

/// See [`accent`].
pub fn accent_300() -> egui::Color32 {
    if is_light() { LIGHT_ACCENT_300 } else { ACCENT_300 }
}

/// The full ramp (lightest to darkest step) for whichever theme is currently
/// active — used by [`wave_meter`], which has no other reason to touch
/// `is_light()` directly.
fn accent_ramp() -> [egui::Color32; 4] {
    if is_light() {
        [LIGHT_ACCENT, LIGHT_ACCENT_500, LIGHT_ACCENT_600, LIGHT_ACCENT_700]
    } else {
        [ACCENT, ACCENT_500, ACCENT_600, ACCENT_700]
    }
}

const CHIP_BG: egui::Color32 = egui::Color32::from_rgb(41, 46, 54);
const LIGHT_CHIP_BG: egui::Color32 = egui::Color32::from_rgb(231, 229, 240);

/// Background for a small inline "chip"-style input (the top bar's class-name
/// and lesson-PIN fields) — a subtle step up from the window/panel
/// background. These fields are drawn with `.frame(false)`, so the text
/// inside them takes its color from the active `Visuals` (light or dark text
/// depending on theme) — the chip's own fill has to track the theme too, or
/// dark text would end up on a dark chip (or light-on-light) once the other
/// one gets set independently by `apply()`.
pub fn chip_bg() -> egui::Color32 {
    if is_light() { LIGHT_CHIP_BG } else { CHIP_BG }
}

/// Status colors: semantically red/amber/green, deliberately outside the accent ramp
/// so they stay legible regardless of accent choice (or, now, theme).
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(224, 82, 82);
pub const WARN: egui::Color32 = egui::Color32::from_rgb(230, 168, 74);
pub const OK: egui::Color32 = egui::Color32::from_rgb(94, 201, 128);

const LIGHT_MUTED: egui::Color32 = egui::Color32::from_rgb(100, 103, 117);

/// De-emphasized text (empty-seat placeholders, hints, captions) that holds up
/// against the current theme's background — dark near-black or light
/// near-white. Prefer this over `ui.weak()` anywhere the text must stay
/// readable in *both* themes; egui's built-in "weak" only really works for
/// light backgrounds. Use this function (never the old bare `MUTED` constant)
/// from outside this module.
pub fn muted() -> egui::Color32 {
    if is_light() { LIGHT_MUTED } else { MUTED }
}

/// Dark-theme value backing [`muted`] — kept as a `const` (not inlined into
/// the function) only because the wave meter and a couple of other spots in
/// this module still reason about the dark ramp directly.
const MUTED: egui::Color32 = egui::Color32::from_rgb(150, 154, 170);

/// Applies the given theme (dark by default) to `ctx` — call once at startup
/// with the value loaded from `Settings`, and again any time the user
/// switches it in the Settings tab/window (egui applies visuals changes on
/// the very next frame, so this takes effect immediately, no restart).
pub fn apply(ctx: &egui::Context, light: bool) {
    LIGHT_MODE.store(light, Ordering::Relaxed);
    let mut visuals = if light { egui::Visuals::light() } else { egui::Visuals::dark() };

    if light {
        visuals.window_fill = LIGHT_BG;
        visuals.panel_fill = LIGHT_PANEL;
        visuals.extreme_bg_color = egui::Color32::from_rgb(255, 255, 255);
        visuals.faint_bg_color = egui::Color32::from_rgb(236, 233, 245);
        visuals.override_text_color = None;
        visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(28, 28, 32);

        visuals.widgets.noninteractive.bg_fill = LIGHT_PANEL;
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(231, 229, 240);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(220, 210, 245);
        visuals.widgets.active.bg_fill = LIGHT_ACCENT;
        visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
        visuals.selection.bg_fill = LIGHT_ACCENT.linear_multiply(0.35);
        visuals.selection.stroke.color = LIGHT_ACCENT;
        visuals.hyperlink_color = LIGHT_ACCENT;
    } else {
        visuals.window_fill = BG;
        visuals.panel_fill = PANEL;
        visuals.extreme_bg_color = egui::Color32::from_rgb(14, 15, 20);
        visuals.faint_bg_color = egui::Color32::from_rgb(34, 37, 46);
        visuals.override_text_color = None;
        visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(226, 228, 235);

        visuals.widgets.noninteractive.bg_fill = PANEL;
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(38, 41, 51);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 45, 66);
        visuals.widgets.active.bg_fill = ACCENT;
        visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(14, 14, 18);
        visuals.selection.bg_fill = ACCENT.linear_multiply(0.5);
        visuals.selection.stroke.color = ACCENT;
        visuals.hyperlink_color = ACCENT;
    }

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.rounding = egui::Rounding::same(8.0);
    }
    visuals.window_rounding = egui::Rounding::same(10.0);
    visuals.menu_rounding = egui::Rounding::same(8.0);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.spacing.window_margin = egui::Margin::same(14.0);
    style.spacing.interact_size.y = 32.0;

    // The default egui sizes read fine on a developer's monitor up close, but this
    // app is meant to be read across a classroom (and often projected) — bump
    // everything up a step so it stays crisp at a distance.
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Small, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(17.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(17.0, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(28.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(15.0, FontFamily::Monospace)),
    ]
    .into();

    ctx.set_style(style);
}

/// A small "wave" VU meter: bars follow a center-peak silhouette (quiet audio only
/// lights the middle), colored by ramp depth — center brightest, edges darkest —
/// rather than one flat accent fill. Shared by the teacher console (own mic /
/// listen-in level) and the student client (own mic level).
pub fn wave_meter(ui: &mut egui::Ui, level_millis: i32) {
    const BAR_COUNT: usize = 7;
    const BAR_W: f32 = 6.0;
    const GAP: f32 = 3.0;
    const MAX_H: f32 = 18.0;
    // Silhouette: how tall each bar can get at full level, center tallest.
    const SHAPE: [f32; BAR_COUNT] = [0.35, 0.55, 0.8, 1.0, 0.8, 0.55, 0.35];

    let level = (level_millis as f32 / 1000.0).clamp(0.0, 1.0);
    let total_w = BAR_COUNT as f32 * BAR_W + (BAR_COUNT as f32 - 1.0) * GAP;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, MAX_H), egui::Sense::hover());
    let painter = ui.painter();
    let [peak, mid, edge, far_edge] = accent_ramp();

    for (i, &shape) in SHAPE.iter().enumerate() {
        let color = match i {
            3 => peak,
            2 | 4 => mid,
            1 | 5 => edge,
            _ => far_edge,
        };
        let x = rect.left() + i as f32 * (BAR_W + GAP);
        let silhouette_h = MAX_H * shape;
        let track_rect = egui::Rect::from_min_size(
            egui::pos2(x, rect.bottom() - silhouette_h),
            egui::vec2(BAR_W, silhouette_h),
        );
        painter.rect_filled(track_rect, 2.0, color.linear_multiply(0.2));

        let fill_h = (silhouette_h * level).max(if level > 0.02 { 2.0 } else { 0.0 });
        if fill_h > 0.0 {
            let fill_rect = egui::Rect::from_min_size(
                egui::pos2(x, rect.bottom() - fill_h),
                egui::vec2(BAR_W, fill_h),
            );
            painter.rect_filled(fill_rect, 2.0, color);
        }
    }
}
