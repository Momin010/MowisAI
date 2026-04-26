use crate::theme::Theme;
use crate::types::{DiffLine, DiffLineKind, FileDiff};
use egui::{Color32, Frame, RichText, ScrollArea, Stroke, Vec2};

// ── State ─────────────────────────────────────────────────────────────────────

pub struct DiffView {
    /// Path of the currently selected file in the file tree.
    pub selected_file: Option<String>,
    /// Vertical scroll position in the diff content area (carried across frames
    /// by egui's ScrollArea id — we keep this for external reset if needed).
    pub scroll_offset: f32,
}

impl Default for DiffView {
    fn default() -> Self {
        Self {
            selected_file: None,
            scroll_offset: 0.0,
        }
    }
}

impl DiffView {
    pub fn new() -> Self {
        Self::default()
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Render the diff panel (file tree on the left, diff content on the right).
///
/// This function is intended to be called from inside a panel or allocated
/// region — it does **not** open its own `egui::CentralPanel`.
pub fn show(view: &mut DiffView, ui: &mut egui::Ui, diffs: &[FileDiff]) {
    Frame::none()
        .fill(Theme::BG_PANEL)
        .inner_margin(egui::Margin::same(0.0))
        .show(ui, |ui| {
            let total_rect = ui.available_rect_before_wrap();

            // ── Two-column layout ─────────────────────────────────────────────
            // Left  ≈ 220 px  (file tree)
            // Right = remainder (diff content)
            let tree_width = 220.0_f32.min(total_rect.width() * 0.3);
            let divider_width = 1.0_f32;

            // Left column rect
            let tree_rect = egui::Rect::from_min_size(
                total_rect.min,
                Vec2::new(tree_width, total_rect.height()),
            );

            // Vertical divider rect
            let divider_rect = egui::Rect::from_min_size(
                egui::pos2(total_rect.min.x + tree_width, total_rect.min.y),
                Vec2::new(divider_width, total_rect.height()),
            );

            // Right column rect
            let diff_rect = egui::Rect::from_min_max(
                egui::pos2(
                    total_rect.min.x + tree_width + divider_width,
                    total_rect.min.y,
                ),
                total_rect.max,
            );

            // Draw vertical divider
            ui.painter().rect_filled(divider_rect, 0.0, Theme::BORDER);

            // ── Left: file tree ───────────────────────────────────────────────
            let mut child_ui = ui.child_ui(tree_rect, *ui.layout(), None);
            render_file_tree(view, &mut child_ui, diffs);

            // ── Right: diff content ───────────────────────────────────────────
            let mut child_ui = ui.child_ui(diff_rect, *ui.layout(), None);
            let selected_diff = view
                .selected_file
                .as_deref()
                .and_then(|path| diffs.iter().find(|d| d.path == path));
            render_diff_content(view, &mut child_ui, selected_diff);
        });
}

// ── Left panel: file tree ─────────────────────────────────────────────────────

fn render_file_tree(view: &mut DiffView, ui: &mut egui::Ui, diffs: &[FileDiff]) {
    Frame::none()
        .fill(Theme::BG_SIDEBAR)
        .inner_margin(egui::Margin::same(0.0))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());

            // Header
            Frame::none()
                .fill(Theme::BG_SIDEBAR)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Changed Files")
                            .font(Theme::font_label())
                            .color(Theme::ACCENT_BLUE)
                            .strong(),
                    );
                });

            // Separator
            let sep_rect = ui.available_rect_before_wrap();
            let sep_y = sep_rect.min.y;
            ui.painter().hline(
                sep_rect.min.x..=sep_rect.max.x,
                sep_y,
                Stroke::new(1.0, Theme::BORDER),
            );
            ui.add_space(1.0);

            // File list
            if diffs.is_empty() {
                // Empty state
                let avail = ui.available_size();
                ui.allocate_ui_with_layout(
                    avail,
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space(avail.y * 0.35);
                        ui.label(
                            RichText::new("No changes yet")
                                .font(Theme::font_label())
                                .color(Theme::TEXT_MUTED),
                        );
                    },
                );
            } else {
                ScrollArea::vertical()
                    .id_source("diff_file_tree_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        for diff in diffs {
                            render_file_row(view, ui, diff);
                        }
                        ui.add_space(8.0);
                    });
            }
        });
}

fn render_file_row(view: &mut DiffView, ui: &mut egui::Ui, diff: &FileDiff) {
    let is_selected = view
        .selected_file
        .as_deref()
        .map_or(false, |p| p == diff.path);

    let row_height = 28.0_f32;
    let avail_width = ui.available_width();
    let (row_rect, response) = ui.allocate_exact_size(
        Vec2::new(avail_width, row_height),
        egui::Sense::click(),
    );

    // Determine hover/selected background
    let bg = if is_selected {
        Theme::BG_SELECTED
    } else if response.hovered() {
        Theme::BG_HOVER
    } else {
        Color32::TRANSPARENT
    };

    ui.painter().rect_filled(row_rect, 0.0, bg);

    // Row content (rendered inside the pre-allocated rect)
    let mut row_ui = ui.child_ui(row_rect, egui::Layout::left_to_right(egui::Align::Center), None);
    row_ui.set_clip_rect(row_rect);

    // Left padding
    row_ui.add_space(8.0);

    // Icon: "+" / "-" / "~" coloured by net change
    let (icon, icon_color) = file_icon(diff);
    row_ui.label(
        RichText::new(icon)
            .font(Theme::font_mono())
            .color(icon_color)
            .strong(),
    );

    row_ui.add_space(4.0);

    // File path — basename bold + parent path muted.
    // Truncate from the right when space is tight by splitting the path.
    let (basename, parent) = split_path(&diff.path);
    let stats_approx_width = 70.0_f32; // room for "+N -M" on the right
    let name_max_w = avail_width - 8.0 - 14.0 - 4.0 - stats_approx_width - 8.0;

    row_ui.allocate_ui_with_layout(
        Vec2::new(name_max_w, row_height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_clip_rect(egui::Rect::from_min_size(
                ui.cursor().min,
                Vec2::new(name_max_w, row_height),
            ));

            if let Some(parent_str) = &parent {
                ui.label(
                    RichText::new(format!("{}/", parent_str))
                        .font(Theme::font_label())
                        .color(Theme::TEXT_MUTED),
                );
            }
            ui.label(
                RichText::new(&basename)
                    .font(Theme::font_label())
                    .color(Theme::TEXT_PRIMARY)
                    .strong(),
            );
        },
    );

    // Right side: "+N -M"
    row_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("-{}", diff.deletions))
                .font(Theme::font_label())
                .color(Theme::DIFF_REMOVED_TEXT),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!("+{}", diff.additions))
                .font(Theme::font_label())
                .color(Theme::DIFF_ADDED_TEXT),
        );
    });

    // Handle click — select this file
    if response.clicked() {
        view.selected_file = Some(diff.path.clone());
        view.scroll_offset = 0.0;
    }
}

// ── Right panel: diff content ─────────────────────────────────────────────────

fn render_diff_content(
    view: &mut DiffView,
    ui: &mut egui::Ui,
    diff: Option<&FileDiff>,
) {
    Frame::none()
        .fill(Theme::BG_PANEL)
        .inner_margin(egui::Margin::same(0.0))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());

            match diff {
                None => render_diff_empty(ui),
                Some(d) => render_diff_file(view, ui, d),
            }
        });
}

fn render_diff_empty(ui: &mut egui::Ui) {
    let avail = ui.available_size();
    ui.allocate_ui_with_layout(
        avail,
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(avail.y * 0.40);
            ui.label(
                RichText::new("Select a file to view its diff")
                    .font(Theme::font_body())
                    .color(Theme::TEXT_MUTED),
            );
        },
    );
}

fn render_diff_file(view: &mut DiffView, ui: &mut egui::Ui, diff: &FileDiff) {
    // ── Header bar ────────────────────────────────────────────────────────────
    let header_height = 34.0_f32;
    Frame::none()
        .fill(Theme::BG_SIDEBAR)
        .inner_margin(egui::Margin::symmetric(12.0, 0.0))
        .show(ui, |ui| {
            ui.set_height(header_height);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(&diff.path)
                        .font(Theme::font_mono())
                        .color(Theme::TEXT_PRIMARY),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("-{}", diff.deletions))
                            .font(Theme::font_label())
                            .color(Theme::DIFF_REMOVED_TEXT),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("+{}", diff.additions))
                            .font(Theme::font_label())
                            .color(Theme::DIFF_ADDED_TEXT),
                    );
                });
            });
        });

    // Separator
    let sep_rect = ui.available_rect_before_wrap();
    let sep_y = sep_rect.min.y;
    ui.painter().hline(
        sep_rect.min.x..=sep_rect.max.x,
        sep_y,
        Stroke::new(1.0, Theme::BORDER),
    );
    ui.add_space(1.0);

    // ── Scrollable diff lines ─────────────────────────────────────────────────
    let scroll = ScrollArea::vertical()
        .id_source("diff_content_scroll")
        .auto_shrink([false, false]);

    let output = scroll.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.add_space(4.0);

        // Approximate line number gutter width (3-digit line numbers)
        let gutter_width = 42.0_f32;

        let mut add_line_num = 0usize;
        let mut rem_line_num = 0usize;
        let mut ctx_line_num = 0usize;

        // Parse an initial context line number from the first @@ header if any
        // so gutter numbers are meaningful within each hunk.
        // We do a simple pass: track running counts separately per kind.

        for (idx, line) in diff.lines.iter().enumerate() {
            // Derive background and text colour from kind
            let (bg_color, text_color, prefix_char) = diff_line_style(&line.kind);

            // Line number gutter value
            let gutter_num = match line.kind {
                DiffLineKind::Added => {
                    add_line_num += 1;
                    Some(add_line_num)
                }
                DiffLineKind::Removed => {
                    rem_line_num += 1;
                    Some(rem_line_num)
                }
                DiffLineKind::Context => {
                    ctx_line_num += 1;
                    add_line_num = ctx_line_num;
                    rem_line_num = ctx_line_num;
                    Some(ctx_line_num)
                }
                DiffLineKind::Header => {
                    // Reset per-hunk counters when we see a new @@ line.
                    // Try to parse the starting line number from "@@  -A,B +C,D @@"
                    if let Some(new_start) = parse_hunk_start(&line.content) {
                        ctx_line_num = new_start.saturating_sub(1);
                        add_line_num = ctx_line_num;
                        rem_line_num = ctx_line_num;
                    }
                    None
                }
            };

            render_diff_line(ui, idx, line, bg_color, text_color, prefix_char, gutter_num, gutter_width);
        }

        ui.add_space(8.0);
    });

    // Persist scroll offset for external consumers.
    view.scroll_offset = output.state.offset.y;
}

fn render_diff_line(
    ui: &mut egui::Ui,
    _idx: usize,
    line: &DiffLine,
    bg_color: Color32,
    text_color: Color32,
    prefix_char: char,
    gutter_num: Option<usize>,
    gutter_width: f32,
) {
    let avail_width = ui.available_width();

    // Each diff line is a horizontal strip: gutter | content
    // We wrap it in a Frame to paint the background across the full width.
    Frame::none()
        .fill(bg_color)
        .inner_margin(egui::Margin::same(0.0))
        .show(ui, |ui| {
            ui.set_min_width(avail_width);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Top), |ui| {
                // ── Gutter ────────────────────────────────────────────────────
                ui.allocate_ui_with_layout(
                    Vec2::new(gutter_width, 18.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.add_space(4.0);
                        let gutter_text = match gutter_num {
                            Some(n) => format!("{}", n),
                            None => String::new(),
                        };
                        ui.label(
                            RichText::new(gutter_text)
                                .font(Theme::font_mono())
                                .color(Theme::TEXT_MUTED),
                        );
                    },
                );

                // Thin separator between gutter and content
                ui.painter().vline(
                    ui.cursor().min.x,
                    ui.cursor().min.y..=ui.cursor().min.y + 18.0,
                    egui::Stroke::new(1.0, Theme::BORDER),
                );
                ui.add_space(1.0);

                // ── Prefix char ("+", "-", " ") ───────────────────────────────
                let prefix_color = match prefix_char {
                    '+' => Theme::DIFF_ADDED_TEXT,
                    '-' => Theme::DIFF_REMOVED_TEXT,
                    _ => Theme::TEXT_MUTED,
                };
                ui.label(
                    RichText::new(format!("{} ", prefix_char))
                        .font(Theme::font_mono())
                        .color(prefix_color)
                        .strong(),
                );

                // ── Line content ──────────────────────────────────────────────
                // Strip the leading "+"/"-"/" " from the raw diff content if
                // present — it is already shown via prefix_char above.
                let display_content = strip_diff_prefix(&line.content);

                let label_rt = RichText::new(display_content)
                    .font(Theme::font_mono())
                    .color(text_color);

                let label_rt = if matches!(line.kind, DiffLineKind::Header) {
                    label_rt.strong()
                } else {
                    label_rt
                };

                ui.label(label_rt);
            });
        });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns `(icon, icon_color)` for the file tree row icon.
fn file_icon(diff: &FileDiff) -> (&'static str, Color32) {
    if diff.additions > 0 && diff.deletions == 0 {
        ("+", Theme::STATUS_COMPLETE)
    } else if diff.deletions > 0 && diff.additions == 0 {
        ("-", Theme::STATUS_FAILED)
    } else {
        // Mixed or empty
        ("~", Color32::from_rgb(220, 180, 60))
    }
}

/// Returns `(bg_color, text_color, prefix_char)` for a diff line kind.
fn diff_line_style(kind: &DiffLineKind) -> (Color32, Color32, char) {
    match kind {
        DiffLineKind::Added => (Theme::DIFF_ADDED_BG, Theme::DIFF_ADDED_TEXT, '+'),
        DiffLineKind::Removed => (Theme::DIFF_REMOVED_BG, Theme::DIFF_REMOVED_TEXT, '-'),
        DiffLineKind::Context => (Color32::TRANSPARENT, Theme::DIFF_CONTEXT_TEXT, ' '),
        DiffLineKind::Header => (Color32::TRANSPARENT, Theme::DIFF_HEADER_TEXT, ' '),
    }
}

/// Split a file path into `(basename, Option<parent>)`.
///
/// e.g. `"src/backend/auth.rs"` → `("auth.rs", Some("src/backend"))`
///      `"main.rs"` → `("main.rs", None)`
fn split_path(path: &str) -> (String, Option<String>) {
    // Normalise to forward slashes for display.
    let normalised = path.replace('\\', "/");
    if let Some(last_slash) = normalised.rfind('/') {
        let basename = normalised[last_slash + 1..].to_string();
        let parent = normalised[..last_slash].to_string();
        (basename, Some(parent))
    } else {
        (normalised, None)
    }
}

/// Strip the leading `+`, `-`, or space character from a raw unified diff line.
fn strip_diff_prefix(content: &str) -> &str {
    let mut chars = content.chars();
    match chars.next() {
        Some('+') | Some('-') | Some(' ') => chars.as_str(),
        _ => content,
    }
}

/// Attempt to parse the starting line number from a unified diff hunk header.
///
/// Format: `@@ -<old_start>[,<old_count>] +<new_start>[,<new_count>] @@`
///
/// Returns `Some(new_start)` on success, `None` otherwise.
fn parse_hunk_start(header: &str) -> Option<usize> {
    // Find "+<digits>" after the first "@@".
    // The hunk header looks like: "@@ -A,B +C,D @@"
    // We want the number after the "+" sign.
    let plus_pos = header.find('+')?;
    let rest = &header[plus_pos + 1..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let num_str = &rest[..end];
    num_str.parse::<usize>().ok()
}
