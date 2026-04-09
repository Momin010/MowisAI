use super::app::{App, MessageRole};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

const SPINNER_FRAMES: &[&str] = &["\u{28fe}", "\u{28f7}", "\u{28ef}", "\u{289f}", "\u{287f}", "\u{28bf}", "\u{28fb}", "\u{28fd}"];

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
    draw_chat(f, app, chunks[1]);
    draw_input(f, app, chunks[2]);
    draw_status(f, app, chunks[3]);
}

fn draw_title(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let title = format!(
        " MowisAI  \u{b7}  {}  \u{b7}  {}",
        app.config.model, app.config.gcp_project_id
    );
    let widget = Paragraph::new(title)
        .style(
            Style::default()
                .bg(Color::Rgb(0, 120, 80))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    f.render_widget(widget, area);
}

fn draw_chat(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let block = Block::default()
        .borders(Borders::NONE);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Build all lines from messages
    let mut all_lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        match msg.role {
            MessageRole::User => {
                all_lines.push(Line::from(vec![
                    Span::styled(
                        "You: ",
                        Style::default()
                            .fg(Color::Rgb(100, 200, 150))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        msg.content.clone(),
                        Style::default().fg(Color::Rgb(100, 200, 150)),
                    ),
                ]));
                all_lines.push(Line::from(""));
            }
            MessageRole::Assistant => {
                all_lines.push(Line::from(vec![
                    Span::styled(
                        "MowisAI: ",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                for line in msg.content.lines() {
                    all_lines.push(Line::from(Span::styled(
                        format!("  {}", line),
                        Style::default().fg(Color::White),
                    )));
                }
                all_lines.push(Line::from(""));
            }
            MessageRole::System => {
                for line in msg.content.lines() {
                    all_lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::Rgb(100, 100, 100)),
                    )));
                }
                all_lines.push(Line::from(""));
            }
        }
    }

    if app.is_loading {
        let spinner = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
        all_lines.push(Line::from(Span::styled(
            format!("{} Thinking...", spinner),
            Style::default()
                .fg(Color::Rgb(0, 200, 140))
                .add_modifier(Modifier::BOLD),
        )));
    }

    let total_lines = all_lines.len();
    let visible_height = inner.height as usize;

    // Calculate scroll: scroll_offset 0 = bottom, higher = scrolled up
    let scroll = if total_lines > visible_height {
        let max_scroll = total_lines - visible_height;
        let desired_scroll = app.scroll_offset;
        max_scroll.saturating_sub(desired_scroll)
    } else {
        0
    };

    let chat = Paragraph::new(all_lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));

    f.render_widget(chat, inner);
}

fn draw_input(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let border_color = if app.is_loading {
        Color::DarkGray
    } else {
        Color::Rgb(0, 180, 120)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Show input with cursor marker
    let display_text = if app.is_loading {
        "(waiting for response...)".to_string()
    } else {
        let before = &app.input_text[..app.input_cursor];
        let after = &app.input_text[app.input_cursor..];
        format!("> {}{}\u{2588}{}", before, after, "")
    };

    let style = if app.is_loading {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };

    let input_widget = Paragraph::new(display_text).style(style);
    f.render_widget(input_widget, inner);
}

fn draw_status(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let status = format!(" {}  \u{b7}  Ctrl+C quit  \u{b7}  /help", app.cwd);
    let widget = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::Rgb(150, 150, 150)))
        .alignment(Alignment::Left);
    f.render_widget(widget, area);
}
