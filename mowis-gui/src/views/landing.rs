use crate::theme::Theme;
use egui::{Color32, Frame, Key, Modifiers, RichText, Stroke, TextEdit, Vec2};

// ── State ─────────────────────────────────────────────────────────────────────

pub struct LandingView {
    /// Text currently typed in the big input box.
    pub prompt: String,
    /// Whether the text area is focused.
    pub focused: bool,
}

impl Default for LandingView {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            focused: false,
        }
    }
}

impl LandingView {
    pub fn new() -> Self {
        Self::default()
    }
}

// ── View ──────────────────────────────────────────────────────────────────────

/// Render the landing page.
///
/// Returns `Some(prompt)` when the user submits (Ctrl+Enter or "Run →" button).
/// Returns `None` while the user is still composing.
pub fn show(
    view: &mut LandingView,
    ctx: &egui::Context,
    _ui: &mut egui::Ui,
) -> Option<String> {
    let mut submitted: Option<String> = None;

    egui::CentralPanel::default()
        .frame(
            Frame::default()
                .fill(Theme::BG_APP)
                .inner_margin(egui::Margin::same(0.0)),
        )
        .show(ctx, |ui| {
            // Fill the whole panel and centre everything vertically.
            let available = ui.available_size();

            ui.allocate_ui_with_layout(
                available,
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    // Push content down to roughly the vertical centre.
                    // Content height is approximately:
                    //   logo ~40 + tagline ~20 + gap ~32 + input ~140 + hint row ~28 = ~260px
                    let content_h = 260.0;
                    let top_pad = ((available.y - content_h) * 0.5).max(40.0);
                    ui.add_space(top_pad);

                    // ── 1. Logo ───────────────────────────────────────────────
                    ui.label(
                        RichText::new("MowisAI")
                            .font(egui::FontId::proportional(38.0))
                            .color(Theme::TEXT_PRIMARY)
                            .strong(),
                    );

                    ui.add_space(6.0);

                    // ── 2. Tagline ────────────────────────────────────────────
                    ui.label(
                        RichText::new("OS-level AI agent execution — run thousands of agents in parallel")
                            .font(Theme::font_body())
                            .color(Theme::TEXT_MUTED),
                    );

                    ui.add_space(32.0);

                    // ── 3. Input box ──────────────────────────────────────────
                    // Choose border colour based on focus state.
                    let border_color = if view.focused {
                        Theme::BORDER_FOCUS
                    } else {
                        Theme::BORDER
                    };

                    // Outer frame that provides the coloured border + rounded bg.
                    let frame = Frame::default()
                        .fill(Theme::BG_INPUT)
                        .rounding(Theme::ROUNDING_MD)
                        .stroke(Stroke::new(1.5, border_color))
                        .inner_margin(egui::Margin::same(12.0));

                    frame.show(ui, |ui| {
                        let text_edit = TextEdit::multiline(&mut view.prompt)
                            .hint_text(
                                RichText::new("Describe what you want to build…")
                                    .color(Theme::TEXT_MUTED),
                            )
                            .desired_width(576.0) // 600 - 2×12 margin
                            .desired_rows(6)
                            .lock_focus(true)
                            .font(Theme::font_body())
                            .text_color(Theme::TEXT_PRIMARY)
                            .frame(false); // suppress egui's own frame — we drew ours

                        let response = ui.add(text_edit);
                        view.focused = response.has_focus();

                        // Ctrl+Enter inside the text box → submit.
                        if response.has_focus()
                            && ctx.input(|i| {
                                i.key_pressed(Key::Enter)
                                    && i.modifiers.matches_exact(Modifiers::CTRL)
                            })
                            && !view.prompt.trim().is_empty()
                        {
                            submitted = Some(view.prompt.trim().to_string());
                        }
                    });

                    ui.add_space(10.0);

                    // ── 4. Bottom hint row ────────────────────────────────────
                    // Use a horizontal layout that spans the same 600px column.
                    ui.allocate_ui_with_layout(
                        Vec2::new(600.0, 28.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            // Left: keyboard hint
                            ui.label(
                                RichText::new("Ctrl+Enter to run")
                                    .font(Theme::font_label())
                                    .color(Theme::TEXT_MUTED),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let prompt_ready = !view.prompt.trim().is_empty();

                                    // Disable the button (and grey it) when prompt is empty.
                                    ui.add_enabled_ui(prompt_ready, |ui| {
                                        // Style the button to look blue when active.
                                        let (btn_fill, btn_text_color) = if prompt_ready {
                                            (Theme::ACCENT_BLUE, Color32::from_rgb(18, 18, 18))
                                        } else {
                                            (Color32::from_rgb(40, 40, 40), Theme::TEXT_MUTED)
                                        };

                                        let button = egui::Button::new(
                                            RichText::new("Run →")
                                                .font(Theme::font_body())
                                                .color(btn_text_color)
                                                .strong(),
                                        )
                                        .fill(btn_fill)
                                        .rounding(Theme::ROUNDING_MD)
                                        .min_size(Vec2::new(80.0, 28.0));

                                        if ui.add(button).clicked() && prompt_ready {
                                            submitted = Some(view.prompt.trim().to_string());
                                        }
                                    });
                                },
                            );
                        },
                    );
                },
            );
        });

    // If submitted, clear the input field for the next session.
    if submitted.is_some() {
        view.prompt.clear();
        view.focused = false;
    }

    submitted
}
