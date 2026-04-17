use super::app::{App, MainView, MessageRole};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};

const SPINNER_FRAMES: &[&str] = &["\u{28fe}", "\u{28f7}", "\u{28ef}", "\u{289f}", "\u{287f}", "\u{28bf}", "\u{28fb}", "\u{28fd}"];

// Colour palette
const GREEN: Color = Color::Rgb(0, 180, 120);
const BRIGHT_GREEN: Color = Color::Rgb(0, 220, 140);
const DARK_GREEN: Color = Color::Rgb(0, 120, 80);
const DIM: Color = Color::Rgb(100, 100, 100);
const WHITE: Color = Color::White;
const AMBER: Color = Color::Rgb(255, 180, 0);
const RED: Color = Color::Rgb(220, 60, 60);
const BLUE: Color = Color::Rgb(80, 160, 255);

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    draw_title(f, app, chunks[0]);

    match app.view_mode {
        MainView::Development => draw_development_view(f, app, chunks[1]),
        MainView::Orchestration if app.orchestrating => draw_orchestration_dashboard(f, app, chunks[1]),
        _ => draw_chat(f, app, chunks[1]),
    }

    draw_input(f, app, chunks[2]);
    draw_status(f, app, chunks[3]);
}

fn draw_title(f: &mut Frame, app: &App, area: Rect) {
    let title = if app.orchestrating {
        let active = app
            .agents
            .iter()
            .filter(|a| a.status == "thinking" || a.status == "executing_tool")
            .count();
        let view_hint = match app.view_mode {
            MainView::Chat => "[Tab: dashboard]",
            MainView::Orchestration => "[Tab: dev logs]",
            MainView::Development => "[Tab: chat]",
        };
        format!(
            " MowisAI  \u{b7}  {}  \u{b7}  {}  \u{b7}  \u{1f528} Orchestrating \u{2014} {} agent{} active  {}",
            app.config.model,
            app.config.gcp_project_id,
            active,
            if active == 1 { "" } else { "s" },
            view_hint,
        )
    } else if app.dev_mode_active {
        format!(
            " MowisAI  \u{b7}  {}  \u{b7}  {}  \u{b7}  [DEV MODE] Tab: cycle views | /development to toggle",
            app.config.model, app.config.gcp_project_id
        )
    } else {
        format!(
            " MowisAI  \u{b7}  {}  \u{b7}  {}",
            app.config.model, app.config.gcp_project_id
        )
    };
    let widget = Paragraph::new(title)
        .style(
            Style::default()
                .bg(DARK_GREEN)
                .fg(WHITE)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    f.render_widget(widget, area);
}

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default().borders(Borders::NONE);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let avail_width = inner.width as usize;

    let mut all_lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        match msg.role {
            MessageRole::User => {
                let prefix = "You: ";
                let content_width = avail_width.saturating_sub(prefix.len());
                let wrapped = textwrap::wrap(&msg.content, content_width.max(10));
                for (i, line) in wrapped.iter().enumerate() {
                    if i == 0 {
                        all_lines.push(Line::from(vec![
                            Span::styled(
                                prefix,
                                Style::default()
                                    .fg(BRIGHT_GREEN)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(line.to_string(), Style::default().fg(BRIGHT_GREEN)),
                        ]));
                    } else {
                        all_lines.push(Line::from(vec![
                            Span::raw(" ".repeat(prefix.len())),
                            Span::styled(line.to_string(), Style::default().fg(BRIGHT_GREEN)),
                        ]));
                    }
                }
                all_lines.push(Line::from(""));
            }
            MessageRole::Assistant => {
                all_lines.push(Line::from(Span::styled(
                    "MowisAI:",
                    Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
                )));
                let content_width = avail_width.saturating_sub(2);
                for para in msg.content.lines() {
                    if para.is_empty() {
                        all_lines.push(Line::from(""));
                        continue;
                    }
                    let wrapped = textwrap::wrap(para, content_width.max(10));
                    for line in wrapped {
                        all_lines.push(Line::from(Span::styled(
                            format!("  {}", line),
                            Style::default().fg(WHITE),
                        )));
                    }
                }
                all_lines.push(Line::from(""));
            }
            MessageRole::System => {
                let content_width = avail_width.saturating_sub(2);
                for para in msg.content.lines() {
                    if para.is_empty() {
                        all_lines.push(Line::from(""));
                        continue;
                    }
                    let wrapped = textwrap::wrap(para, content_width.max(10));
                    for line in wrapped {
                        all_lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(DIM),
                        )));
                    }
                }
                all_lines.push(Line::from(""));
            }
        }
    }

    if app.is_loading && app.view_mode == MainView::Chat {
        let spinner = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
        let label = if app.orchestrating {
            "Orchestrating... (Tab to see dashboard)"
        } else {
            "Thinking..."
        };
        all_lines.push(Line::from(Span::styled(
            format!("{} {}", spinner, label),
            Style::default().fg(BRIGHT_GREEN).add_modifier(Modifier::BOLD),
        )));
    }

    let total_lines = all_lines.len();
    let visible_height = inner.height as usize;

    let scroll = if total_lines > visible_height {
        let max_scroll = total_lines - visible_height;
        max_scroll.saturating_sub(app.scroll_offset)
    } else {
        0
    };

    let chat = Paragraph::new(all_lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));

    f.render_widget(chat, inner);
}

fn draw_development_view(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(100, 100, 255)))
        .title(Span::styled(
            " Development Log (all internal logs) ",
            Style::default()
                .fg(Color::Rgb(100, 100, 255))
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let avail_width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    for (level, message, timestamp) in &app.dev_log {
        let color = match level.as_str() {
            "ERROR" => Color::Rgb(220, 60, 60),
            "WARN" => Color::Rgb(255, 180, 0),
            "INFO" => Color::Rgb(0, 220, 140),
            "DEBUG" => Color::Rgb(150, 150, 150),
            _ => Color::White,
        };

        let time_str = format!(
            "[{:02}:{:02}:{:02}] ",
            (timestamp % 86400) / 3600,
            (timestamp % 3600) / 60,
            timestamp % 60,
        );

        let full_line = format!("{}{}: {}", time_str, level, message);
        let wrapped = textwrap::wrap(&full_line, avail_width.max(10));
        for line in wrapped {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(color),
            )));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No log entries yet. Internal activity will appear here.",
            Style::default().fg(Color::Rgb(80, 80, 80)),
        )));
    }

    let total = lines.len();
    let visible = inner.height as usize;
    let scroll = if total > visible { (total - visible) as u16 } else { 0 };

    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(widget, inner);
}

fn draw_orchestration_dashboard(f: &mut Frame, app: &App, area: Rect) {
    // Split horizontally: left = activity log, right = agent list + progress
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    draw_orch_log(f, app, cols[0]);
    draw_orch_agents(f, app, cols[1]);
}

fn draw_orch_log(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN))
        .title(Span::styled(
            " Activity Log ",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let avail_width = inner.width as usize;
    let mut lines: Vec<Line> = Vec::new();

    for entry in &app.orch_log {
        let color = if entry.starts_with('\u{2713}') || entry.starts_with('\u{25b8}') {
            BRIGHT_GREEN
        } else if entry.starts_with('\u{2717}') {
            RED
        } else if entry.starts_with("[Layer") {
            AMBER
        } else {
            DIM
        };
        let wrapped = textwrap::wrap(entry, avail_width.max(10));
        for line in wrapped {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(color),
            )));
        }
    }

    // Show spinner at bottom when active
    if app.is_loading {
        let spinner = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
        let layer_info = if app.orch_layer > 0 {
            format!("{} Layer {} running...", spinner, app.orch_layer)
        } else {
            format!("{} Initializing...", spinner)
        };
        lines.push(Line::from(Span::styled(
            layer_info,
            Style::default().fg(BRIGHT_GREEN).add_modifier(Modifier::BOLD),
        )));
    }

    // Scroll to bottom
    let total = lines.len();
    let visible = inner.height as usize;
    let scroll = if total > visible { (total - visible) as u16 } else { 0 };

    let widget = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(widget, inner);
}

fn draw_orch_agents(f: &mut Frame, app: &App, area: Rect) {
    // Split vertically: top = progress, middle = agent list
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(3),
        ])
        .split(area);

    draw_orch_progress(f, app, rows[0]);
    draw_orch_agent_list(f, app, rows[1]);
}

fn draw_orch_progress(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN))
        .title(Span::styled(
            " Progress ",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (ratio, label) = if app.orch_total > 0 {
        let pct = app.orch_completed as f64 / app.orch_total as f64;
        (
            pct,
            format!(
                " {}/{} tasks  |  Layer {}/7",
                app.orch_completed, app.orch_total, app.orch_layer
            ),
        )
    } else {
        (
            0.0,
            format!(" Layer {}/7  |  Initializing...", app.orch_layer),
        )
    };

    let gauge = Gauge::default()
        .gauge_style(
            Style::default()
                .fg(BRIGHT_GREEN)
                .bg(Color::Rgb(20, 40, 30)),
        )
        .ratio(ratio.clamp(0.0, 1.0))
        .label(label);

    f.render_widget(gauge, inner);
}

fn draw_orch_agent_list(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN))
        .title(Span::styled(
            " Agents ",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let avail = inner.width as usize;

    if app.agents.is_empty() {
        let waiting = Line::from(Span::styled(
            " Waiting for agents...",
            Style::default().fg(DIM),
        ));
        f.render_widget(Paragraph::new(vec![waiting]), inner);
        return;
    }

    let items: Vec<ListItem> = app
        .agents
        .iter()
        .map(|agent| {
            let (symbol, color) = match agent.status.as_str() {
                "thinking" => ("\u{25cf}", AMBER),
                "executing_tool" => ("\u{25b6}", BLUE),
                "completed" => ("\u{2713}", BRIGHT_GREEN),
                "failed" => ("\u{2717}", RED),
                _ => ("\u{b7}", DIM),
            };

            let tool_info = agent
                .current_tool
                .as_deref()
                .map(|t| format!(" [{t}]"))
                .unwrap_or_default();

            let id_short = &agent.agent_id[..agent.agent_id.len().min(10)];
            let desc_max = avail.saturating_sub(id_short.len() + tool_info.len() + 5);
            let desc: String = agent.description.chars().take(desc_max).collect();

            let text = format!("{} {}  {}{}", symbol, id_short, desc, tool_info);
            ListItem::new(Line::from(Span::styled(
                text,
                Style::default().fg(color),
            )))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let border_color = if app.is_loading { Color::DarkGray } else { GREEN };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let display_text = if app.is_loading {
        "(waiting for response...)".to_string()
    } else {
        let before = &app.input_text[..app.input_cursor];
        let after = &app.input_text[app.input_cursor..];
        format!("> {}\u{2588}{}", before, after)
    };

    let style = if app.is_loading {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(WHITE)
    };

    f.render_widget(Paragraph::new(display_text).style(style), inner);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let status = if app.orchestrating {
        format!(
            " {}  \u{b7}  Tab: switch view  \u{b7}  Ctrl+C quit  \u{b7}  /help",
            app.cwd
        )
    } else {
        format!(" {}  \u{b7}  Ctrl+C quit  \u{b7}  /help", app.cwd)
    };
    let widget = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::Rgb(150, 150, 150)))
        .alignment(Alignment::Left);
    f.render_widget(widget, area);
}
