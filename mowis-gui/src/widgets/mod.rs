use crate::theme::Theme;
use egui::{Color32, Frame, Pos2, RichText, Stroke, Vec2};
use std::f32::consts::TAU;

// ── Status Bar ────────────────────────────────────────────────────────────────

/// Live metrics shown in the thin bar at the very bottom of the window.
pub struct StatusBarState {
    pub daemon_running: bool,
    pub active_agents: usize,
    pub tasks_complete: usize,
    pub tasks_total: usize,
    pub elapsed_secs: u64,
}

/// Renders a full-width ~28 px status bar with daemon state, agent count,
/// task progress, and elapsed time.
pub fn show_status_bar(ui: &mut egui::Ui, state: &StatusBarState) {
    // Outer frame: BG_SIDEBAR background, top border line.
    Frame::none()
        .fill(Theme::BG_SIDEBAR)
        .inner_margin(egui::Margin {
            left: 12.0,
            right: 12.0,
            top: 0.0,
            bottom: 0.0,
        })
        .show(ui, |ui| {
            // Draw the top border manually so we get exactly one pixel.
            let full_rect = ui.available_rect_before_wrap();
            ui.painter().hline(
                full_rect.x_range(),
                full_rect.top(),
                Stroke::new(1.0, Theme::BORDER),
            );

            ui.set_min_height(28.0);
            ui.set_max_height(28.0);

            // Three-section horizontal layout: left | center | right.
            ui.with_layout(
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_height(28.0);

                    // ── Left: daemon status ───────────────────────────────
                    let (dot_color, label) = if state.daemon_running {
                        (Theme::STATUS_COMPLETE, "Daemon running")
                    } else {
                        (Theme::STATUS_FAILED, "Daemon stopped")
                    };

                    // Coloured dot via a tiny colored label character.
                    ui.label(
                        RichText::new("●")
                            .color(dot_color)
                            .font(egui::FontId::proportional(10.0)),
                    );
                    ui.label(
                        RichText::new(label)
                            .color(Theme::TEXT_SECONDARY)
                            .font(egui::FontId::proportional(12.0)),
                    );

                    // ── Center: agent count ───────────────────────────────
                    // Push center label to the middle by taking equal space on
                    // both sides; we approximate this with a fill spacer on
                    // the left and right.
                    ui.with_layout(
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            let agent_text = if state.active_agents == 1 {
                                format!("{} agent active", state.active_agents)
                            } else {
                                format!("{} agents active", state.active_agents)
                            };
                            ui.label(
                                RichText::new(agent_text)
                                    .color(Theme::TEXT_MUTED)
                                    .font(egui::FontId::proportional(12.0)),
                            );
                        },
                    );
                },
            );

            // Right section — we draw it via a right-to-left layout overlaid
            // on the same row. egui doesn't support a true three-column bar
            // natively, so we place the right items using available space.
            // Because `centered_and_justified` consumed remaining width, we
            // use `ui.put` with an absolute rect instead.
            let bar_rect = {
                let r = ui.min_rect();
                egui::Rect::from_min_max(
                    Pos2::new(r.right() - 160.0, r.top()),
                    Pos2::new(r.right(), r.bottom()),
                )
            };

            // Format elapsed time as mm:ss.
            let elapsed = format_elapsed(state.elapsed_secs);
            let right_text = format!(
                "{}/{} tasks  |  {} elapsed",
                state.tasks_complete, state.tasks_total, elapsed
            );

            ui.put(
                bar_rect,
                egui::Label::new(
                    RichText::new(right_text)
                        .color(Theme::TEXT_MUTED)
                        .font(egui::FontId::proportional(12.0)),
                ),
            );
        });
}

/// Formats a duration in seconds as `mm:ss`.
fn format_elapsed(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{:02}:{:02}", m, s)
}

// ── Spinner ───────────────────────────────────────────────────────────────────

/// Draws a 16×16 animated loading spinner composed of 8 dots arranged in a
/// circle. The dots cycle through an alpha fade so the spinner appears to
/// rotate. Calls `request_repaint` automatically to keep the animation alive.
pub fn show_spinner(ui: &mut egui::Ui, color: egui::Color32) {
    const SIZE: f32 = 16.0;
    const DOT_RADIUS: f32 = 1.6;
    const NUM_DOTS: usize = 8;
    const ORBIT_RADIUS: f32 = 5.5;

    // Reserve exactly 16×16 of space.
    let (rect, _response) =
        ui.allocate_exact_size(Vec2::splat(SIZE), egui::Sense::hover());

    // Request continuous repaint so the spinner animates.
    ui.ctx().request_repaint();

    // Current time drives the rotation angle.
    let t = ui.ctx().input(|i| i.time) as f32;
    // One full rotation per second.
    let phase = (t * TAU).rem_euclid(TAU);

    let center = rect.center();
    let painter = ui.painter_at(rect);

    let [r, g, b, _] = color.to_array();

    for i in 0..NUM_DOTS {
        let angle = phase + (i as f32 / NUM_DOTS as f32) * TAU;
        let dot_pos = Pos2::new(
            center.x + ORBIT_RADIUS * angle.cos(),
            center.y + ORBIT_RADIUS * angle.sin(),
        );

        // The dot at angle=phase (i == 0 conceptually) is fully opaque;
        // trailing dots fade towards transparent.
        // "Leading" dot: i == 0 is at angle=phase, but we want the
        // most-recently-passed dot to be brightest.  We reverse the index so
        // that the dot just behind the rotation direction is fully opaque.
        let frac = (NUM_DOTS - i) as f32 / NUM_DOTS as f32;
        // Map frac from [1/N .. 1] to alpha [20 .. 255].
        let alpha = (frac * frac * 240.0 + 15.0).min(255.0) as u8;
        let dot_color = Color32::from_rgba_unmultiplied(r, g, b, alpha);

        painter.circle_filled(dot_pos, DOT_RADIUS, dot_color);
    }
}
