use egui::{Color32, FontId, Rounding, Stroke, Style, Visuals};

// ── Palette ───────────────────────────────────────────────────────────────────
//
// Inspired by Cursor / VS Code dark theme.

pub struct Theme;

impl Theme {
    // Backgrounds
    pub const BG_APP: Color32 = Color32::from_rgb(18, 18, 18);
    pub const BG_PANEL: Color32 = Color32::from_rgb(24, 24, 24);
    pub const BG_SIDEBAR: Color32 = Color32::from_rgb(20, 20, 20);
    pub const BG_INPUT: Color32 = Color32::from_rgb(30, 30, 30);
    pub const BG_HOVER: Color32 = Color32::from_rgb(38, 38, 38);
    pub const BG_SELECTED: Color32 = Color32::from_rgb(42, 42, 42);

    // Text
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(220, 220, 220);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(140, 140, 140);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(90, 90, 90);
    pub const TEXT_ACCENT: Color32 = Color32::from_rgb(100, 181, 246);  // blue

    // Borders
    pub const BORDER: Color32 = Color32::from_rgb(48, 48, 48);
    pub const BORDER_FOCUS: Color32 = Color32::from_rgb(100, 181, 246);

    // Diff colours
    pub const DIFF_ADDED_BG: Color32 = Color32::from_rgba_premultiplied(0, 60, 0, 180);
    pub const DIFF_ADDED_TEXT: Color32 = Color32::from_rgb(130, 210, 130);
    pub const DIFF_REMOVED_BG: Color32 = Color32::from_rgba_premultiplied(80, 0, 0, 180);
    pub const DIFF_REMOVED_TEXT: Color32 = Color32::from_rgb(220, 100, 100);
    pub const DIFF_HEADER_TEXT: Color32 = Color32::from_rgb(120, 160, 220);
    pub const DIFF_CONTEXT_TEXT: Color32 = Color32::from_rgb(170, 170, 170);

    // Accent / status
    pub const ACCENT_BLUE: Color32 = Color32::from_rgb(100, 181, 246);
    pub const STATUS_PENDING: Color32 = Color32::from_rgb(120, 120, 120);
    pub const STATUS_RUNNING: Color32 = Color32::from_rgb(255, 190, 50);
    pub const STATUS_COMPLETE: Color32 = Color32::from_rgb(80, 200, 120);
    pub const STATUS_FAILED: Color32 = Color32::from_rgb(220, 80, 80);

    // Chat bubbles
    pub const BUBBLE_USER_BG: Color32 = Color32::from_rgb(37, 99, 235);
    pub const BUBBLE_USER_TEXT: Color32 = Color32::WHITE;
    pub const BUBBLE_AGENT_BG: Color32 = Color32::from_rgb(34, 34, 34);
    pub const BUBBLE_AGENT_TEXT: Color32 = Color32::from_rgb(220, 220, 220);
    pub const BUBBLE_SYSTEM_BG: Color32 = Color32::from_rgb(28, 28, 28);
    pub const BUBBLE_SYSTEM_TEXT: Color32 = Color32::from_rgb(140, 140, 140);

    // Rounding
    pub const ROUNDING_SM: Rounding = Rounding::same(4.0);
    pub const ROUNDING_MD: Rounding = Rounding::same(8.0);
    pub const ROUNDING_LG: Rounding = Rounding::same(12.0);
    pub const ROUNDING_PILL: Rounding = Rounding::same(999.0);

    // Typography
    pub fn font_body() -> FontId {
        FontId::proportional(14.0)
    }
    pub fn font_mono() -> FontId {
        FontId::monospace(13.0)
    }
    pub fn font_label() -> FontId {
        FontId::proportional(12.0)
    }
    pub fn font_heading() -> FontId {
        FontId::proportional(22.0)
    }
    pub fn font_subheading() -> FontId {
        FontId::proportional(16.0)
    }

    /// Apply the dark theme to an egui Context's style.
    pub fn apply(ctx: &egui::Context) {
        let mut style = Style::default();

        // Visuals
        let mut visuals = Visuals::dark();
        visuals.window_fill = Self::BG_PANEL;
        visuals.panel_fill = Self::BG_PANEL;
        visuals.extreme_bg_color = Self::BG_APP;
        visuals.code_bg_color = Self::BG_INPUT;
        visuals.window_stroke = Stroke::new(1.0, Self::BORDER);
        visuals.widgets.noninteractive.bg_fill = Self::BG_PANEL;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Self::TEXT_SECONDARY);
        visuals.widgets.inactive.bg_fill = Self::BG_INPUT;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Self::TEXT_PRIMARY);
        visuals.widgets.hovered.bg_fill = Self::BG_HOVER;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Self::TEXT_PRIMARY);
        visuals.widgets.active.bg_fill = Self::ACCENT_BLUE;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        visuals.selection.bg_fill = Color32::from_rgba_premultiplied(100, 181, 246, 60);
        visuals.selection.stroke = Stroke::new(1.0, Self::ACCENT_BLUE);
        visuals.window_rounding = Self::ROUNDING_MD;

        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(16.0);

        ctx.set_style(style);
    }
}
