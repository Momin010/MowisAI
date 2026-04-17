use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Widget,
};

const SPINNER_FRAMES: &[&str] = &["\u{28fe}", "\u{28f7}", "\u{28ef}", "\u{289f}", "\u{287f}", "\u{28bf}", "\u{28fb}", "\u{28fd}"];

/// Inline loading spinner widget.
pub struct Spinner {
    pub frame: usize,
    pub label: String,
}

impl Spinner {
    pub fn new(frame: usize, label: impl Into<String>) -> Self {
        Self {
            frame: frame % SPINNER_FRAMES.len(),
            label: label.into(),
        }
    }
}

impl Widget for Spinner {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let symbol = SPINNER_FRAMES[self.frame];
        let text = format!("{} {}", symbol, self.label);
        let display: String = text.chars().take(area.width as usize).collect();
        buf.set_string(
            area.x,
            area.y,
            &display,
            Style::default()
                .fg(Color::Rgb(0, 200, 140))
                .add_modifier(Modifier::BOLD),
        );
    }
}

/// A single-line chat message preview (for compact display).
pub struct MessagePreview<'a> {
    pub role: &'a str,
    pub content: &'a str,
    pub selected: bool,
}

impl<'a> Widget for MessagePreview<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let role_color = match self.role {
            "user" => Color::Rgb(100, 200, 150),
            "assistant" => Color::White,
            _ => Color::Rgb(100, 100, 100),
        };
        let prefix = format!("{}: ", self.role);
        let available = area.width as usize;
        let content_width = available.saturating_sub(prefix.len());
        let content: String = self.content.chars().take(content_width).collect();
        let full = format!("{}{}", prefix, content);

        buf.set_string(
            area.x,
            area.y,
            &full,
            Style::default().fg(role_color),
        );
    }
}
