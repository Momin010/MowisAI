use egui::{Color32, FontId, Rounding, Stroke, Style, Visuals};

// ── Palette ───────────────────────────────────────────────────────────────────
//
// Brand colors: Black, Cream White, Glass, Turquoise

pub struct Theme;

impl Theme {
    // Backgrounds - Black & Glass
    pub const BG_APP: Color32 = Color32::from_rgb(10, 10, 10);           // Pure black
    pub const BG_PANEL: Color32 = Color32::from_rgb(15, 15, 15);         // Slightly lighter black
    pub const BG_SIDEBAR: Color32 = Color32::from_rgb(12, 12, 12);       // Dark black
    pub const BG_INPUT: Color32 = Color32::from_rgb(20, 20, 20);         // Input black
    pub const BG_HOVER: Color32 = Color32::from_rgb(25, 25, 25);         // Hover state
    pub const BG_SELECTED: Color32 = Color32::from_rgb(30, 30, 30);      // Selected state
    
    // Glass effect (semi-transparent)
    pub const GLASS_LIGHT: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 10);
    pub const GLASS_MEDIUM: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 20);
    pub const GLASS_STRONG: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 30);

    // Text - Cream White
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(250, 248, 240);   // Cream white
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(200, 198, 190); // Muted cream
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(120, 118, 110);     // Very muted
    pub const TEXT_ACCENT: Color32 = Color32::from_rgb(64, 224, 208);     // Turquoise

    // Borders - Subtle glass
    pub const BORDER: Color32 = Color32::from_rgb(30, 30, 30);
    pub const BORDER_FOCUS: Color32 = Color32::from_rgb(64, 224, 208);    // Turquoise

    // Diff colours
    pub const DIFF_ADDED_BG: Color32 = Color32::from_rgba_premultiplied(0, 60, 0, 180);
    pub const DIFF_ADDED_TEXT: Color32 = Color32::from_rgb(130, 210, 130);
    pub const DIFF_REMOVED_BG: Color32 = Color32::from_rgba_premultiplied(80, 0, 0, 180);
    pub const DIFF_REMOVED_TEXT: Color32 = Color32::from_rgb(220, 100, 100);
    pub const DIFF_HEADER_TEXT: Color32 = Color32::from_rgb(64, 224, 208);  // Turquoise
    pub const DIFF_CONTEXT_TEXT: Color32 = Color32::from_rgb(200, 198, 190);

    // Accent / status - Turquoise
    pub const ACCENT_TURQUOISE: Color32 = Color32::from_rgb(64, 224, 208);  // Primary turquoise
    pub const ACCENT_TURQUOISE_DARK: Color32 = Color32::from_rgb(32, 178, 170); // Darker turquoise
    pub const STATUS_PENDING: Color32 = Color32::from_rgb(120, 118, 110);
    pub const STATUS_RUNNING: Color32 = Color32::from_rgb(64, 224, 208);    // Turquoise
    pub const STATUS_COMPLETE: Color32 = Color32::from_rgb(80, 200, 120);
    pub const STATUS_FAILED: Color32 = Color32::from_rgb(220, 80, 80);

    // Chat bubbles
    pub const BUBBLE_USER_BG: Color32 = Color32::from_rgb(64, 224, 208);    // Turquoise
    pub const BUBBLE_USER_TEXT: Color32 = Color32::from_rgb(10, 10, 10);    // Black text on turquoise
    pub const BUBBLE_AGENT_BG: Color32 = Color32::from_rgb(20, 20, 20);     // Dark glass
    pub const BUBBLE_AGENT_TEXT: Color32 = Color32::from_rgb(250, 248, 240); // Cream
    pub const BUBBLE_SYSTEM_BG: Color32 = Color32::from_rgb(15, 15, 15);
    pub const BUBBLE_SYSTEM_TEXT: Color32 = Color32::from_rgb(120, 118, 110);

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
        visuals.widgets.active.bg_fill = Self::ACCENT_TURQUOISE;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::from_rgb(10, 10, 10));
        visuals.selection.bg_fill = Color32::from_rgba_premultiplied(64, 224, 208, 60);
        visuals.selection.stroke = Stroke::new(1.0, Self::ACCENT_TURQUOISE);
        visuals.window_rounding = Self::ROUNDING_MD;

        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(16.0);

        ctx.set_style(style);
    }
}
