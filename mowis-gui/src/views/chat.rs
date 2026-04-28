use crate::theme::Theme;
use crate::types::{ChatMessage, MessageRole};
use egui::{Align, Frame, Key, Layout, RichText, ScrollArea, Stroke, TextEdit};

// ── State ─────────────────────────────────────────────────────────────────────

pub struct ChatView {
    pub input: String,
    pub scroll_to_bottom: bool,
}

impl Default for ChatView {
    fn default() -> Self {
        Self {
            input: String::new(),
            scroll_to_bottom: true,
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Renders the chat panel.
///
/// Returns `Some(text)` when the user submits a follow-up message, `None`
/// otherwise.
pub fn show(
    view: &mut ChatView,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    messages: &[ChatMessage],
) -> Option<String> {
    let mut submitted: Option<String> = None;

    // ── Outer frame (panel background) ────────────────────────────────────────
    Frame::none()
        .fill(Theme::BG_PANEL)
        .inner_margin(egui::Margin::same(0.0))
        .show(ui, |ui| {
            ui.set_min_height(ui.available_height());

            // Reserve space for the input area at the bottom.
            let input_height = 52.0;
            let divider_height = 1.0;
            let list_height = (ui.available_height() - input_height - divider_height).max(0.0);

            // ── Message list ─────────────────────────────────────────────────
            let scroll_area = ScrollArea::vertical()
                .id_source("chat_scroll")
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .max_height(list_height);

            scroll_area.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.add_space(8.0);

                for msg in messages {
                    render_message(ctx, ui, msg);
                    ui.add_space(6.0);
                }

                // If the caller signalled scroll_to_bottom, egui's
                // stick_to_bottom(true) already handles it; we just reset
                // the flag here so callers know it was consumed.
                if view.scroll_to_bottom {
                    view.scroll_to_bottom = false;
                }

                ui.add_space(8.0);
            });

            // ── Divider ───────────────────────────────────────────────────────
            let divider_rect = ui.available_rect_before_wrap();
            let line_y = divider_rect.min.y;
            ui.painter().hline(
                divider_rect.min.x..=divider_rect.max.x,
                line_y,
                Stroke::new(divider_height, Theme::BORDER),
            );
            ui.add_space(divider_height);

            // ── Input area ────────────────────────────────────────────────────
            Frame::none()
                .fill(Theme::BG_PANEL)
                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                .show(ui, |ui| {
                    ui.set_height(input_height - 16.0); // subtract vertical margins
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Send button
                        let can_send = !view.input.trim().is_empty();
                        let btn_label = RichText::new("Send").color(egui::Color32::WHITE);
                        let send_btn = egui::Button::new(btn_label)
                            .fill(if can_send {
                                Theme::BUBBLE_USER_BG
                            } else {
                                Theme::BG_INPUT
                            })
                            .rounding(Theme::ROUNDING_MD);

                        let btn_resp = ui.add_enabled(can_send, send_btn);
                        if btn_resp.clicked() {
                            submitted = submit_input(view);
                        }

                        ui.add_space(8.0);

                        // Text input
                        let text_edit = TextEdit::singleline(&mut view.input)
                            .desired_width(f32::INFINITY)
                            .hint_text("Ask a follow-up…")
                            .frame(true)
                            .text_color(Theme::TEXT_PRIMARY);

                        let te_resp = ui.add(text_edit);

                        // Submit on Enter
                        if te_resp.lost_focus()
                            && ui.input(|i| i.key_pressed(Key::Enter))
                            && can_send
                        {
                            submitted = submit_input(view);
                            te_resp.request_focus();
                        }
                    });
                });
        });

    submitted
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn submit_input(view: &mut ChatView) -> Option<String> {
    let text = view.input.trim().to_string();
    if text.is_empty() {
        return None;
    }
    view.input.clear();
    view.scroll_to_bottom = true;
    Some(text)
}

fn render_message(ctx: &egui::Context, ui: &mut egui::Ui, msg: &ChatMessage) {
    let max_bubble_width = ui.available_width() * 0.75;

    match msg.role {
        // ── System message — full-width, centered, muted italic ──────────────
        MessageRole::System => {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                Frame::none()
                    .fill(Theme::BUBBLE_SYSTEM_BG)
                    .rounding(Theme::ROUNDING_MD)
                    .inner_margin(egui::Margin::symmetric(12.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&msg.content)
                                .italics()
                                .color(Theme::BUBBLE_SYSTEM_TEXT)
                                .font(Theme::font_label()),
                        );
                    });

                render_timestamp(ui, msg);
            });
        }

        // ── User message — right-aligned, blue bubble ─────────────────────────
        MessageRole::User => {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.set_max_width(max_bubble_width);

                ui.vertical(|ui| {
                    Frame::none()
                        .fill(Theme::BUBBLE_USER_BG)
                        .rounding(Theme::ROUNDING_LG)
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(&msg.content)
                                    .color(Theme::BUBBLE_USER_TEXT)
                                    .font(Theme::font_body()),
                            );
                        });

                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        render_timestamp(ui, msg);
                    });
                });
            });
        }

        // ── Agent message — left-aligned, dark bubble, optional cursor ────────
        MessageRole::Agent => {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.set_max_width(max_bubble_width);

                ui.vertical(|ui| {
                    Frame::none()
                        .fill(Theme::BUBBLE_AGENT_BG)
                        .rounding(Theme::ROUNDING_LG)
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                        .show(ui, |ui| {
                            if msg.streaming {
                                // Append blinking block cursor to indicate live
                                // output; request continuous repaints so the
                                // cursor appears to blink as the content grows.
                                ctx.request_repaint();
                                let display = format!("{}▊", msg.content);
                                ui.label(
                                    RichText::new(&display)
                                        .color(Theme::BUBBLE_AGENT_TEXT)
                                        .font(Theme::font_body()),
                                );
                            } else {
                                ui.label(
                                    RichText::new(&msg.content)
                                        .color(Theme::BUBBLE_AGENT_TEXT)
                                        .font(Theme::font_body()),
                                );
                            }
                        });

                    render_timestamp(ui, msg);
                });
            });
        }
    }
}

fn render_timestamp(ui: &mut egui::Ui, msg: &ChatMessage) {
    let ts = msg.timestamp.format("%H:%M").to_string();
    ui.add_space(2.0);
    ui.label(
        RichText::new(ts)
            .color(Theme::TEXT_MUTED)
            .font(Theme::font_label()),
    );
}
