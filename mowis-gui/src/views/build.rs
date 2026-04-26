use crate::theme::Theme;
use crate::types::{Task, TaskStatus};
use egui::{Color32, Frame, ProgressBar, RichText, Stroke, Vec2};

// ── BuildView ─────────────────────────────────────────────────────────────────

pub struct BuildView {
    /// Whether the task list is expanded or collapsed.
    pub expanded: bool,
}

impl Default for BuildView {
    fn default() -> Self {
        Self { expanded: true }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn show(view: &mut BuildView, ui: &mut egui::Ui, tasks: &[Task]) {
    Frame::none()
        .fill(Theme::BG_PANEL)
        .inner_margin(egui::Margin::same(8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            show_inner(view, ui, tasks);
        });
}

// ── Inner layout ──────────────────────────────────────────────────────────────

fn show_inner(view: &mut BuildView, ui: &mut egui::Ui, tasks: &[Task]) {
    // ── Counts ────────────────────────────────────────────────────────────────
    let total = tasks.len();

    if total == 0 {
        show_empty_state(ui);
        return;
    }

    let mut n_pending = 0usize;
    let mut n_running = 0usize;
    let mut n_complete = 0usize;
    let mut n_failed = 0usize;

    for task in tasks {
        match &task.status {
            TaskStatus::Pending => n_pending += 1,
            TaskStatus::Running => n_running += 1,
            TaskStatus::Complete => n_complete += 1,
            TaskStatus::Failed(_) => n_failed += 1,
        }
    }

    let progress_fraction = if total > 0 {
        n_complete as f32 / total as f32
    } else {
        0.0
    };

    // ── Header row (clickable, toggles expanded) ───────────────────────────────
    let header_response = ui.horizontal(|ui| {
        ui.set_width(ui.available_width());

        // Left: "Build Progress" label
        ui.label(
            RichText::new("Build Progress")
                .color(Theme::ACCENT_BLUE)
                .font(Theme::font_label()),
        );

        ui.add_space(4.0);

        // Expand/collapse chevron
        let chevron = if view.expanded { "▾" } else { "▸" };
        ui.label(
            RichText::new(chevron)
                .color(Theme::TEXT_MUTED)
                .font(Theme::font_label()),
        );

        ui.add_space(6.0);

        // Center: progress bar — fill remaining width minus the count label
        let count_text = format!("{}/{} tasks", n_complete, total);
        let count_galley = ui.painter().layout_no_wrap(
            count_text.clone(),
            Theme::font_label(),
            Theme::TEXT_MUTED,
        );
        let count_width = count_galley.size().x + 4.0;

        let bar_width = (ui.available_width() - count_width - 8.0).max(40.0);
        ui.add(
            ProgressBar::new(progress_fraction)
                .desired_width(bar_width)
                .fill(Theme::STATUS_COMPLETE),
        );

        ui.add_space(4.0);

        // Right: "X/Y tasks" count
        ui.label(
            RichText::new(count_text)
                .color(Theme::TEXT_MUTED)
                .font(Theme::font_label()),
        );
    });

    // Make the entire header row clickable for toggling.
    let header_rect = header_response.response.rect;
    let header_sense = ui.allocate_rect(header_rect, egui::Sense::click());
    if header_sense.clicked() {
        view.expanded = !view.expanded;
    }
    if header_sense.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // ── Separator ─────────────────────────────────────────────────────────────
    ui.add_space(2.0);
    ui.painter().hline(
        ui.available_rect_before_wrap().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, Theme::BORDER),
    );
    ui.add_space(4.0);

    // ── Task list (only when expanded) ────────────────────────────────────────
    if view.expanded {
        let time = ui.ctx().input(|i| i.time);

        for task in tasks {
            show_task_row(ui, task, time);
        }

        ui.add_space(4.0);
    }

    // ── Summary line (always visible) ─────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} running · {} complete · {} pending",
                n_running, n_complete, n_pending
            ))
            .color(Theme::TEXT_MUTED)
            .font(Theme::font_label()),
        );

        if n_failed > 0 {
            ui.label(
                RichText::new(format!("· {} failed", n_failed))
                    .color(Theme::STATUS_FAILED)
                    .font(Theme::font_label()),
            );
        }
    });
}

// ── Individual task row ───────────────────────────────────────────────────────

fn show_task_row(ui: &mut egui::Ui, task: &Task, time: f64) {
    let is_running = matches!(task.status, TaskStatus::Running);
    let is_failed = matches!(task.status, TaskStatus::Failed(_));

    // Row background on hover
    let row_id = egui::Id::new("task_row").with(&task.id);
    let row_rect = ui.available_rect_before_wrap();

    ui.push_id(row_id, |ui| {
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());

            // ── Status dot ────────────────────────────────────────────────────
            let (dot_rect, _) = ui.allocate_space(Vec2::splat(12.0));
            let dot_center = dot_rect.center();

            let base_color = status_color(&task.status);
            let dot_color = if is_running {
                // Pulse: modulate alpha between ~120 and 255 using a sine wave
                let alpha = ((time * 2.5).sin() * 0.5 + 0.5) * 135.0 + 120.0;
                Color32::from_rgba_unmultiplied(
                    base_color.r(),
                    base_color.g(),
                    base_color.b(),
                    alpha as u8,
                )
            } else {
                base_color
            };

            ui.painter().circle_filled(dot_center, 4.0, dot_color);

            // Request continuous repaint while a task is running so the animation ticks.
            if is_running {
                ui.ctx().request_repaint();
            }

            ui.add_space(4.0);

            // ── Description (truncated) ───────────────────────────────────────
            let available_for_desc = ui.available_width()
                - if task.sandbox.is_some() { 80.0 } else { 0.0 };

            let description = truncate_with_ellipsis(&task.description, available_for_desc, ui);
            let desc_color = if is_failed {
                Theme::TEXT_SECONDARY
            } else {
                Theme::TEXT_PRIMARY
            };
            ui.label(RichText::new(description).color(desc_color).font(Theme::font_label()));

            // ── Sandbox badge ─────────────────────────────────────────────────
            if let Some(sandbox) = &task.sandbox {
                // Push badge to the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let badge_text = sandbox.as_str();
                    let galley = ui.painter().layout_no_wrap(
                        badge_text.to_string(),
                        Theme::font_label(),
                        Theme::TEXT_SECONDARY,
                    );
                    let text_size = galley.size();
                    let padding = egui::vec2(6.0, 2.0);
                    let badge_size = Vec2::new(
                        text_size.x + padding.x * 2.0,
                        text_size.y + padding.y * 2.0,
                    );

                    let (badge_rect, _) = ui.allocate_space(badge_size);

                    ui.painter().rect_filled(
                        badge_rect,
                        Theme::ROUNDING_PILL,
                        Theme::BG_SELECTED,
                    );
                    ui.painter().rect_stroke(
                        badge_rect,
                        Theme::ROUNDING_PILL,
                        Stroke::new(1.0, Theme::BORDER),
                        egui::StrokeKind::Inside,
                    );

                    let text_pos = egui::pos2(
                        badge_rect.left() + padding.x,
                        badge_rect.top() + padding.y,
                    );
                    ui.painter().galley(text_pos, galley, Theme::TEXT_SECONDARY);
                });
            }
        });

        // ── Error line for failed tasks ────────────────────────────────────────
        if let TaskStatus::Failed(ref err) = task.status {
            ui.horizontal(|ui| {
                ui.add_space(16.0); // indent under dot
                let err_display = truncate_str(err, 120);
                ui.label(
                    RichText::new(format!("✗ {}", err_display))
                        .color(Theme::STATUS_FAILED)
                        .font(Theme::font_label()),
                );
            });
        }
    });

    ui.add_space(2.0);
}

// ── Empty state ───────────────────────────────────────────────────────────────

fn show_empty_state(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(12.0);
        ui.label(
            RichText::new("Waiting for orchestration to start…")
                .color(Theme::TEXT_MUTED)
                .font(Theme::font_label()),
        );
        ui.add_space(12.0);
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn status_color(status: &TaskStatus) -> Color32 {
    match status {
        TaskStatus::Pending => Theme::STATUS_PENDING,
        TaskStatus::Running => Theme::STATUS_RUNNING,
        TaskStatus::Complete => Theme::STATUS_COMPLETE,
        TaskStatus::Failed(_) => Theme::STATUS_FAILED,
    }
}

/// Truncate a string to fit within `max_width` pixels, appending "…" if needed.
/// Falls back to a character-count heuristic when the painter layout would be
/// too expensive to call in a tight loop.
fn truncate_with_ellipsis(text: &str, max_width: f32, ui: &egui::Ui) -> String {
    // Fast path: cheap character-count guard (~7px per char at 12pt).
    let char_limit = ((max_width / 7.0).floor() as usize).max(4);
    if text.chars().count() <= char_limit {
        return text.to_string();
    }

    // Measure via the painter and binary-search for the right cut point.
    let mut chars: Vec<char> = text.chars().collect();
    let ellipsis = '…';
    // Trim until it fits.
    while !chars.is_empty() {
        let candidate: String = chars.iter().collect::<String>() + &ellipsis.to_string();
        let galley = ui.painter().layout_no_wrap(
            candidate.clone(),
            Theme::font_label(),
            Theme::TEXT_PRIMARY,
        );
        if galley.size().x <= max_width {
            return candidate;
        }
        chars.pop();
    }
    ellipsis.to_string()
}

/// Simple character-based truncation for error strings.
fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}…", collected)
    } else {
        collected
    }
}
