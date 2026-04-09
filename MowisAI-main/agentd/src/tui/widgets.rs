use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Widget,
};

/// Simple progress bar widget: shows filled/empty chars with percentage label.
pub struct ProgressBar {
    pub percent: u16,
    pub label: String,
}

impl ProgressBar {
    pub fn new(percent: u16, label: impl Into<String>) -> Self {
        Self {
            percent: percent.min(100),
            label: label.into(),
        }
    }
}

impl Widget for ProgressBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let width = area.width as usize;
        let filled = (width * self.percent as usize) / 100;
        let bar: String = std::iter::repeat('█')
            .take(filled)
            .chain(std::iter::repeat('░').take(width - filled))
            .collect();
        let label = format!(" {} {}%", self.label, self.percent);
        let display = if label.len() <= width {
            format!("{}{}", bar[..width - label.len()].to_string(), label)
        } else {
            bar
        };
        buf.set_string(
            area.x,
            area.y,
            &display,
            Style::default().fg(Color::Green).bg(Color::DarkGray),
        );
    }
}

/// A single-line status row for an agent.
pub struct AgentRow<'a> {
    pub id: &'a str,
    pub status: &'a str,
    pub task: &'a str,
    pub elapsed: u64,
    pub selected: bool,
}

impl<'a> Widget for AgentRow<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let (status_sym, color) = match self.status {
            "thinking" => ("⏵", Color::Blue),
            "executing_tool" => ("⚙", Color::Yellow),
            "completed" => ("✓", Color::Green),
            "failed" => ("✗", Color::Red),
            _ => ("·", Color::Gray),
        };
        let prefix = if self.selected { "► " } else { "  " };
        let row = format!(
            "{}{} {}  {} [{}s]",
            prefix,
            status_sym,
            self.id,
            self.task.chars().take(30).collect::<String>(),
            self.elapsed
        );
        let style = if self.selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default()
        };
        buf.set_string(area.x, area.y, &row, style);
    }
}

/// Scrollable activity log widget.
pub struct ActivityLog<'a> {
    pub lines: &'a [String],
    pub max_visible: usize,
}

impl<'a> Widget for ActivityLog<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let visible = self.max_visible.min(area.height as usize);
        let start = self.lines.len().saturating_sub(visible);
        for (i, line) in self.lines[start..].iter().enumerate() {
            if i >= area.height as usize {
                break;
            }
            let truncated: String = line.chars().take(area.width as usize).collect();
            buf.set_string(area.x, area.y + i as u16, &truncated, Style::default());
        }
    }
}
