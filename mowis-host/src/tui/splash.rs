use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

const PURPLE: Color = Color::Rgb(109, 40, 217);

pub fn render_splash(f: &mut Frame, frame: u64) {
    let area = f.size();
    let glow_colors = [
        Color::Rgb(109, 40, 217),
        Color::Rgb(124, 58, 237),
        Color::Rgb(139, 92, 246),
        Color::Rgb(168, 85, 247),
        Color::Rgb(139, 92, 246),
        Color::Rgb(124, 58, 237),
        Color::Rgb(109, 40, 217),
        Color::Rgb(91, 33, 182),
    ];
    let glow = glow_colors[(frame as usize) % glow_colors.len()];
    let dim_glow = Color::Rgb(76, 29, 149);

    let dots_cycle = ["⣾⣽⣻⢿⡿⣟⣯⣷", "⣷⣯⣟⡿⢿⣻⣽⣾", "⣯⣷⣻⣽⣾⣟⡿⢿"];
    let dots = dots_cycle[(frame as usize) % dots_cycle.len()];

    let show_hint = frame > 4;

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "       ███╗   ███╗  ██████╗  ██╗    ██╗ ██╗ ███████╗  █████╗  ██╗",
        Style::default().fg(glow).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "       ████╗ ████║ ██╔═══██╗ ██║    ██║ ██║ ██╔════╝ ██╔══██╗ ██║",
        Style::default().fg(glow).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "       ██╔████╔██║ ██║   ██║ ██║ █╗ ██║ ██║ ███████╗ ███████║ ██║",
        Style::default().fg(Color::Rgb(139, 92, 246)).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "       ██║╚██╔╝██║ ██║   ██║ ██║███╗██║ ██║ ╚════██║ ██╔══██║ ██║",
        Style::default().fg(Color::Rgb(124, 58, 237)).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "       ██║ ╚═╝ ██║ ╚██████╔╝ ╚███╔███╔╝ ██║ ███████║ ██║  ██║ ██║",
        Style::default().fg(glow).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "       ╚═╝     ╚═╝  ╚═════╝   ╚══╝╚══╝  ╚═╝ ╚══════╝ ╚═╝  ╚═╝ ╚═╝",
        Style::default().fg(Color::Rgb(91, 33, 182)).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "                            ╔═══════════════════════════════╗",
        Style::default().fg(dim_glow),
    )));
    lines.push(Line::from(vec![
        Span::styled("                            ║  ", Style::default().fg(dim_glow)),
        Span::styled("multi-agent conductor system", Style::default().fg(Color::Rgb(124, 58, 237)).add_modifier(Modifier::ITALIC)),
        Span::styled(" ║", Style::default().fg(dim_glow)),
    ]));
    lines.push(Line::from(Span::styled(
        "                            ╚═══════════════════════════════╝",
        Style::default().fg(dim_glow),
    )));
    lines.push(Line::raw(""));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("                     {}  Initializing agents...", dots),
        Style::default().fg(glow).add_modifier(Modifier::DIM),
    )));

    if show_hint {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "                          ╭──────────────────────────────╮",
            Style::default().fg(Color::Rgb(42, 42, 74)),
        )));
        lines.push(Line::from(vec![
            Span::styled("                          │", Style::default().fg(Color::Rgb(42, 42, 74))),
            Span::styled("   Press Enter to start   ", Style::default().fg(glow).add_modifier(Modifier::BOLD)),
            Span::styled("│", Style::default().fg(Color::Rgb(42, 42, 74))),
        ]));
        lines.push(Line::from(Span::styled(
            "                          ╰──────────────────────────────╯",
            Style::default().fg(Color::Rgb(42, 42, 74)),
        )));
    }

    let line_count = lines.len();
    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    let vertical_pad = area.height.saturating_sub(line_count as u16) / 2;
    let padded = Rect {
        x: area.x,
        y: area.y + vertical_pad,
        width: area.width,
        height: area.height.saturating_sub(vertical_pad),
    };
    f.render_widget(paragraph, padded);
}
