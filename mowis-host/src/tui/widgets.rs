use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

const PURPLE: Color = Color::Rgb(139, 92, 246);
const CYAN:   Color = Color::Rgb(34, 211, 238);
const GREEN:  Color = Color::Rgb(74, 222, 128);
const YELLOW: Color = Color::Rgb(251, 191, 36);
const RED:    Color = Color::Rgb(248, 113, 113);
const DIM:    Color = Color::Rgb(71, 85, 105);
const INDIGO: Color = Color::Rgb(99, 102, 241);
const BORDER: Color = Color::Rgb(51, 65, 85);

fn markdown_to_lines(text: &str, prefix: &str, prefix_color: Color) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for line_text in text.lines() {
        let trimmed = line_text.trim();
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Add prefix on first line
        if lines.is_empty() && !prefix.is_empty() {
            spans.push(Span::styled(prefix.to_string(), Style::default().fg(prefix_color)));
        } else if !lines.is_empty() {
            // Indent continuation lines to align with text after prefix
            spans.push(Span::raw("  ".to_string()));
        }

        if trimmed.starts_with("# ") {
            // Header
            spans.push(Span::styled(
                trimmed[2..].to_string(),
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ));
        } else if trimmed.starts_with("## ") {
            spans.push(Span::styled(
                trimmed[3..].to_string(),
                Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
            ));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            // Bullet point
            spans.push(Span::styled("  • ".to_string(), Style::default().fg(PURPLE)));
            // Parse inline formatting in the bullet text
            spans.extend(parse_inline_markdown(&trimmed[2..]));
        } else if trimmed.len() > 2 && trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && trimmed.contains(". ") {
            // Numbered list
            if let Some(dot_pos) = trimmed.find(". ") {
                spans.push(Span::styled(
                    format!("  {} ", &trimmed[..dot_pos]),
                    Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
                ));
                spans.extend(parse_inline_markdown(&trimmed[dot_pos + 2..]));
            }
        } else {
            // Regular text with inline formatting
            spans.extend(parse_inline_markdown(trimmed));
        }

        lines.push(Line::from(spans));
    }
    lines
}

fn parse_inline_markdown(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '*' && chars.peek() == Some(&'*') {
            // Bold **text**
            chars.next(); // skip second *
            if !current.is_empty() {
                spans.push(Span::raw(current.clone()));
                current.clear();
            }
            let mut bold_text = String::new();
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'*') {
                    chars.next();
                    break;
                }
                bold_text.push(c);
            }
            spans.push(Span::styled(
                bold_text,
                Style::default().add_modifier(Modifier::BOLD),
            ));
        } else if ch == '*' {
            // Italic *text*
            if !current.is_empty() {
                spans.push(Span::raw(current.clone()));
                current.clear();
            }
            let mut italic_text = String::new();
            while let Some(c) = chars.next() {
                if c == '*' {
                    break;
                }
                italic_text.push(c);
            }
            spans.push(Span::styled(
                italic_text,
                Style::default().add_modifier(Modifier::ITALIC),
            ));
        } else if ch == '`' {
            // Inline code `text`
            if !current.is_empty() {
                spans.push(Span::raw(current.clone()));
                current.clear();
            }
            let mut code_text = String::new();
            while let Some(c) = chars.next() {
                if c == '`' {
                    break;
                }
                code_text.push(c);
            }
            spans.push(Span::styled(
                code_text,
                Style::default().fg(YELLOW).bg(Color::Rgb(30, 30, 40)),
            ));
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        spans.push(Span::raw(current));
    }
    spans
}

// ── Message Log ────────────────────────────────────────────────

pub struct MessageLog {
    lines: Vec<LogLine>,
    /// Scroll position measured in rendered rows from the top of the content.
    scroll_offset: usize,
    pub thinking: bool,
    spinner_frame: usize,
    pub streaming: bool,
    pub had_streaming: bool,
    streaming_text: String,
    /// When true, the view is pinned to the bottom (latest messages).
    pub auto_scroll: bool,
    /// Content height (in rendered rows) and viewport height from the last
    /// render. Cached so the scroll handlers can clamp without re-rendering.
    content_rows: usize,
    viewport_height: usize,
}

struct LogLine {
    prefix: String,
    prefix_color: Color,
    text: String,
    italic: bool,
    dim: bool,
}

impl MessageLog {
    pub fn new() -> Self {
        Self { lines: Vec::new(), scroll_offset: 0, thinking: false, spinner_frame: 0, streaming: false, had_streaming: false, streaming_text: String::new(), auto_scroll: true, content_rows: 0, viewport_height: 0 }
    }

    /// Maximum scroll offset (in rendered rows) — the offset that shows the
    /// last line at the bottom of the viewport.
    fn max_scroll(&self) -> usize {
        self.content_rows.saturating_sub(self.viewport_height)
    }

    pub fn scroll_up(&mut self) {
        // If pinned to the bottom, anchor at the current bottom before moving
        // up — otherwise the first keypress would jump to the very top.
        if self.auto_scroll {
            self.scroll_offset = self.max_scroll();
            self.auto_scroll = false;
        }
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        if self.auto_scroll {
            return; // already at the bottom
        }
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + 1).min(max);
        // Reaching the bottom re-pins the view so new messages keep following.
        if self.scroll_offset >= max {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.auto_scroll = true;
    }

    pub fn push_stream_token(&mut self, token: &str) {
        if !self.streaming {
            // Start new streaming line
            self.streaming = true;
            self.had_streaming = true;
            self.thinking = false; // Clear thinking on first token
            self.streaming_text.clear();
            // Add spacing
            if !self.lines.is_empty() {
                self.lines.push(LogLine {
                    prefix: String::new(),
                    prefix_color: DIM,
                    text: String::new(),
                    italic: false,
                    dim: false,
                });
            }
        }
        self.streaming_text.push_str(token);
    }

    pub fn finish_streaming(&mut self) {
        if self.streaming {
            self.streaming = false;
            // Drop the <plan> block — the raw task TOML belongs in the plan
            // panel, not dumped into the chat. Keep any surrounding prose.
            let (visible, _) = Self::strip_plan_block(&self.streaming_text);
            let visible = visible.trim().to_string();
            if !visible.is_empty() {
                self.lines.push(LogLine {
                    prefix: "◈ ".into(),
                    prefix_color: GREEN,
                    text: visible,
                    italic: false,
                    dim: false,
                });
            }
            self.streaming_text.clear();
        }
    }

    /// Hide the `<plan>...</plan>` block from chat display. Returns the visible
    /// text (prose around the block) and whether a plan block is currently open
    /// but not yet closed (i.e. still streaming).
    fn strip_plan_block(text: &str) -> (String, bool) {
        match text.find("<plan>") {
            None => (text.to_string(), false),
            Some(start) => {
                let before = text[..start].trim_end();
                match text[start..].find("</plan>") {
                    Some(end_rel) => {
                        let after = &text[start + end_rel + "</plan>".len()..];
                        (format!("{}{}", before, after), false)
                    }
                    None => (before.to_string(), true),
                }
            }
        }
    }

    pub fn tick_spinner(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % 10;
    }

    pub fn spinner_char(&self) -> &str {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        FRAMES[self.spinner_frame % FRAMES.len()]
    }

    pub fn add_user(&mut self, text: &str) {
        self.thinking = false;
        self.had_streaming = false; // Reset for next exchange
        self.finish_streaming();
        // Add blank line before user message for spacing
        if !self.lines.is_empty() {
            self.lines.push(LogLine {
                prefix: String::new(),
                prefix_color: DIM,
                text: String::new(),
                italic: false,
                dim: false,
            });
        }
        self.lines.push(LogLine {
            prefix: "▸ ".into(),
            prefix_color: INDIGO,
            text: text.to_string(),
            italic: false,
            dim: false,
        });
    }

    pub fn add_conductor(&mut self, text: &str) {
        self.thinking = false;
        // Strip any <plan>…</plan> block — it belongs in the plan panel, not the chat.
        let (visible, _) = Self::strip_plan_block(text);
        let visible = visible.trim();
        if visible.is_empty() {
            return;
        }
        if !self.lines.is_empty() {
            self.lines.push(LogLine {
                prefix: String::new(),
                prefix_color: DIM,
                text: String::new(),
                italic: false,
                dim: false,
            });
        }
        self.lines.push(LogLine {
            prefix: "◈ ".into(),
            prefix_color: GREEN,
            text: visible.to_string(),
            italic: false,
            dim: false,
        });
    }

    pub fn add_system(&mut self, text: &str) {
        self.lines.push(LogLine {
            prefix: String::new(),
            prefix_color: DIM,
            text: text.to_string(),
            italic: false,
            dim: true,
        });
    }

    pub fn add_thinking(&mut self, text: &str) {
        self.thinking = true;
        // Add blank line before thinking for spacing
        if !self.lines.is_empty() {
            self.lines.push(LogLine {
                prefix: String::new(),
                prefix_color: DIM,
                text: String::new(),
                italic: false,
                dim: false,
            });
        }
        self.lines.push(LogLine {
            prefix: "⟳ ".into(),
            prefix_color: YELLOW,
            text: text.to_string(),
            italic: true,
            dim: true,
        });
        self.auto_scroll = true;
    }

    pub fn stop_thinking(&mut self) {
        self.thinking = false;
    }

    pub fn add_plan_link(&mut self, plan_id: &str, version: u32) {
        self.lines.push(LogLine {
            prefix: "◆ ".into(),
            prefix_color: CYAN,
            text: format!("Plan drafted: {} v{} — type 'tab' to view details", plan_id, version),
            italic: false,
            dim: false,
        });
    }

    pub fn add_critic_verdict(&mut self, verdict: &str, prose: &str) {
        let color = match verdict {
            "approve" => GREEN,
            "revise" => YELLOW,
            _ => RED,
        };
        self.lines.push(LogLine {
            prefix: "◇ ".into(),
            prefix_color: color,
            text: format!("Critic Verdict: {}", verdict.to_uppercase()),
            italic: false,
            dim: false,
        });
        if !prose.is_empty() {
            self.lines.push(LogLine {
                prefix: "  ".into(),
                prefix_color: DIM,
                text: prose.to_string(),
                italic: false,
                dim: true,
            });
        }
    }

    pub fn add_agent_event(&mut self, prefix: &str, prefix_color: Color, text: &str) {
        self.lines.push(LogLine {
            prefix: format!("{} ", prefix),
            prefix_color,
            text: text.to_string(),
            italic: false,
            dim: false,
        });
        self.auto_scroll = true;
    }

    pub fn add_awaiting_approval(&mut self, plan_id: &str) {
        self.lines.push(LogLine {
            prefix: "◆ ".into(),
            prefix_color: YELLOW,
            text: format!("Awaiting approval for {} — type 'approve' or 'cancel'", plan_id),
            italic: false,
            dim: false,
        });
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Estimate how many rendered rows a set of lines occupies once wrapped to
    /// `width`. Used to anchor scrolling to the true bottom of the content.
    fn estimate_rows(spans: &[Line], width: u16) -> usize {
        let w = (width.max(1)) as usize;
        spans
            .iter()
            .map(|line| {
                let lw = line.width();
                if lw == 0 { 1 } else { (lw + w - 1) / w }
            })
            .sum()
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        // No visible border — the layout margins are the boundary.
        // Shrink 1 row from the top so the first message doesn't sit flush against
        // whatever is above (title bar / divider).
        let content_area = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        };

        let mut spans: Vec<Line> = Vec::new();
        for line in &self.lines {
            if line.prefix == "◈ " && line.prefix_color == GREEN {
                for md_line in markdown_to_lines(&line.text, "◈ ", GREEN) {
                    spans.push(md_line);
                }
            } else {
                let prefix_style = Style::default().fg(line.prefix_color);
                let mut text_style = Style::default().fg(Color::White);
                if line.italic { text_style = text_style.add_modifier(Modifier::ITALIC); }
                if line.dim    { text_style = text_style.fg(DIM); }
                spans.push(Line::from(vec![
                    Span::styled(line.prefix.clone(), prefix_style),
                    Span::styled(line.text.clone(), text_style),
                ]));
            }
        }

        if self.thinking {
            spans.push(Line::from(Span::styled(
                format!("  {} thinking…", self.spinner_char()),
                Style::default().fg(YELLOW).add_modifier(Modifier::ITALIC),
            )));
        }

        if self.streaming && !self.streaming_text.is_empty() {
            let (visible, drafting) = Self::strip_plan_block(&self.streaming_text);
            if !visible.trim().is_empty() {
                for line in markdown_to_lines(&visible, "◈ ", GREEN) {
                    spans.push(line);
                }
            }
            if drafting {
                spans.push(Line::from(Span::styled(
                    format!("  {} drafting plan…", self.spinner_char()),
                    Style::default().fg(CYAN).add_modifier(Modifier::ITALIC),
                )));
            }
        }

        // Empty state: centered hint when there is nothing to show
        if spans.is_empty() {
            let h = content_area.height as usize;
            let pad = h.saturating_sub(3) / 2;
            for _ in 0..pad {
                spans.push(Line::raw(""));
            }
            spans.push(
                Line::from(Span::styled("start a conversation", Style::default().fg(BORDER)))
                    .alignment(Alignment::Center),
            );
        }

        let visible_height = content_area.height as usize;
        let content_rows = Self::estimate_rows(&spans, content_area.width);
        self.content_rows = content_rows;
        self.viewport_height = visible_height;

        let max_scroll = content_rows.saturating_sub(visible_height);
        let scroll = if self.auto_scroll { max_scroll } else { self.scroll_offset.min(max_scroll) };

        let log = Paragraph::new(spans)
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0))
            .style(Style::default().bg(Color::Black));
        f.render_widget(log, content_area);
    }
}

// ── Plan Preview ───────────────────────────────────────────────

#[derive(Clone)]
pub struct TaskInfo {
    pub id: String,
    pub title: String,
    pub deps: Vec<String>,
    pub tier: String,
    pub budget: u32,
}

enum PlanState {
    Empty,
    Drafted {
        plan_id: String,
        version: u32,
        overview: String,
        tasks: Vec<TaskInfo>,
        sandbox_image: String,
        sandbox_ram: u32,
    },
    AwaitingApproval {
        plan_id: String,
        version: u32,
        overview: String,
        tasks: Vec<TaskInfo>,
        sandbox_image: String,
        sandbox_ram: u32,
    },
    Approved,
}

pub struct PlanPreview {
    state: PlanState,
}

impl PlanPreview {
    pub fn new() -> Self {
        Self { state: PlanState::Empty }
    }

    pub fn set_plan(&mut self, plan_id: String, version: u32, overview: String, tasks: Vec<TaskInfo>, sandbox_image: String, sandbox_ram: u32) {
        self.state = PlanState::Drafted { plan_id, version, overview, tasks, sandbox_image, sandbox_ram };
    }

    pub fn set_awaiting(&mut self) {
        if let PlanState::Drafted { plan_id, version, overview, tasks, sandbox_image, sandbox_ram } = &self.state {
            self.state = PlanState::AwaitingApproval {
                plan_id: plan_id.clone(), version: *version, overview: overview.clone(),
                tasks: tasks.to_vec(), sandbox_image: sandbox_image.clone(), sandbox_ram: *sandbox_ram,
            };
        }
    }

    pub fn is_awaiting_approval(&self) -> bool {
        matches!(self.state, PlanState::AwaitingApproval { .. })
    }

    pub fn approve(&mut self) {
        self.state = PlanState::Approved;
    }

    pub fn cancel(&mut self) {
        self.state = PlanState::Empty;
    }

    pub fn render(&self, f: &mut Frame, area: Rect, expanded: bool) {
        let (border_color, content) = match &self.state {
            PlanState::Empty => (DIM, vec![
                Line::from(Span::styled("No active plan", Style::default().fg(DIM))),
            ]),
            PlanState::Drafted { plan_id, version, overview, tasks, sandbox_image, sandbox_ram } |
            PlanState::AwaitingApproval { plan_id, version, overview, tasks, sandbox_image, sandbox_ram } => {
                let mut lines = vec![
                    Line::from(vec![
                        Span::raw("Plan: "),
                        Span::styled(format!("{} v{}", plan_id, version), Style::default().fg(CYAN).add_modifier(Modifier::BOLD)),
                    ]),
                ];
                if !expanded {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {} tasks • ", tasks.len()), Style::default().fg(DIM)),
                        Span::styled("[Press P to expand]", Style::default().fg(CYAN).add_modifier(Modifier::DIM)),
                    ]));
                } else {
                    lines.push(Line::from(Span::styled("─".repeat(40), Style::default().fg(DIM))));
                    for line in overview.lines().take(6) {
                        lines.push(Line::from(Span::raw(line.to_string())));
                    }
                    lines.push(Line::raw(""));
                    lines.push(Line::from(Span::styled("Task Graph:", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD))));
                    for task in tasks {
                        let tier_color = match task.tier.as_str() {
                            "fast" => GREEN,
                            "mid" => YELLOW,
                            "flagship" => RED,
                            _ => DIM,
                        };
                        let mut task_line = vec![
                            Span::styled(format!("  [{}]", task.id), Style::default().fg(tier_color).add_modifier(Modifier::BOLD)),
                            Span::raw(format!(" {}", task.title)),
                        ];
                        if !task.deps.is_empty() {
                            task_line.push(Span::styled(
                                format!(" (deps: {})", task.deps.join(", ")),
                                Style::default().fg(DIM),
                            ));
                        }
                        task_line.push(Span::styled(
                            format!(" ⚡{} {}", task.budget, task.tier),
                            Style::default().fg(tier_color).add_modifier(Modifier::DIM),
                        ));
                        lines.push(Line::from(task_line));
                    }
                    lines.push(Line::raw(""));
                    lines.push(Line::from(Span::styled("Configuration:", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD))));
                    lines.push(Line::from(Span::styled(
                        format!("  Sandbox: {} RAM: {}MB", sandbox_image, sandbox_ram),
                        Style::default().fg(DIM),
                    )));
                    lines.push(Line::from(Span::styled(
                        format!("  Conductor: claude-opus-4-7"),
                        Style::default().fg(DIM),
                    )));
                    lines.push(Line::from(Span::styled(
                        format!("  Captain: claude-sonnet-4-6"),
                        Style::default().fg(DIM),
                    )));
                    lines.push(Line::from(Span::styled(
                        format!("  Crew: claude-haiku-4-5"),
                        Style::default().fg(DIM),
                    )));
                }
                (CYAN, lines)
            }
            PlanState::Approved => (GREEN, vec![
                Line::from(Span::styled("✓ Plan approved — execution in progress", Style::default().fg(GREEN))),
            ]),
        };

        let block = Block::default()
            .title(Line::from(vec![
                Span::styled(" Plan Preview", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" (P) ", Style::default().fg(DIM)),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new(content).block(block);
        f.render_widget(paragraph, area);
    }
}

// ── Critic Panel ───────────────────────────────────────────────

pub struct CriticIssue {
    pub severity: String,
    pub section: String,
    pub message: String,
    pub suggested_fix: Option<String>,
}

enum CriticState {
    Idle,
    Reviewing { plan_id: String, version: u32 },
    Done { verdict: String, issues: Vec<CriticIssue>, prose: String },
}

pub struct CriticPanel {
    state: CriticState,
}

impl CriticPanel {
    pub fn new() -> Self {
        Self { state: CriticState::Idle }
    }

    pub fn set_reviewing(&mut self, plan_id: &str, version: u32) {
        self.state = CriticState::Reviewing { plan_id: plan_id.to_string(), version };
    }

    pub fn set_verdict(&mut self, verdict: &str, issues: Vec<CriticIssue>, prose: String) {
        self.state = CriticState::Done { verdict: verdict.to_string(), issues, prose };
    }

    pub fn clear(&mut self) {
        self.state = CriticState::Idle;
    }

    pub fn render(&self, f: &mut Frame, area: Rect, expanded: bool) {
        let (border_color, content) = match &self.state {
            CriticState::Idle => (YELLOW, vec![
                Line::from(Span::styled("Critic standing by", Style::default().fg(DIM))),
            ]),
            CriticState::Reviewing { plan_id, .. } => (YELLOW, vec![
                Line::from(vec![
                    Span::styled("⟳ ", Style::default().fg(YELLOW)),
                    Span::styled(
                        format!("Reviewing {}...", plan_id),
                        Style::default().fg(YELLOW).add_modifier(Modifier::ITALIC),
                    ),
                ]),
            ]),
            CriticState::Done { verdict, issues, prose } => {
                let color = match verdict.as_str() {
                    "approve" => GREEN,
                    "revise" => YELLOW,
                    _ => RED,
                };
                let icon = match verdict.as_str() {
                    "approve" => "✓",
                    "revise" => "⚠",
                    _ => "✗",
                };

                let mut lines = vec![
                    Line::from(vec![
                        Span::styled(format!("{} ", icon), Style::default().fg(color)),
                        Span::raw("Verdict: "),
                        Span::styled(verdict.to_uppercase(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    ]),
                ];

                if !expanded {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {} issues • ", issues.len()), Style::default().fg(DIM)),
                        Span::styled("[Press C to expand]", Style::default().fg(YELLOW).add_modifier(Modifier::DIM)),
                    ]));
                } else {
                    if !prose.is_empty() {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(Span::raw(prose.clone())));
                    }
                    if !issues.is_empty() {
                        lines.push(Line::raw(""));
                        lines.push(Line::from(Span::styled("Issues:", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD))));
                        for issue in issues {
                            let sev_color = match issue.severity.as_str() {
                                "info" => CYAN,
                                "warn" => YELLOW,
                                "block" => RED,
                                _ => DIM,
                            };
                            let sev_icon = match issue.severity.as_str() {
                                "info" => "ℹ",
                                "warn" => "⚠",
                                "block" => "✗",
                                _ => "•",
                            };
                            lines.push(Line::from(vec![
                                Span::styled(format!("  {} ", sev_icon), Style::default().fg(sev_color)),
                                Span::raw(format!("[{}] {}", issue.section, issue.message)),
                            ]));
                            if let Some(fix) = &issue.suggested_fix {
                                lines.push(Line::from(Span::styled(
                                    format!("    → {}", fix),
                                    Style::default().fg(DIM).add_modifier(Modifier::ITALIC),
                                )));
                            }
                        }
                    }
                }
                (color, lines)
            }
        };

        let block = Block::default()
            .title(Line::from(vec![
                Span::styled(" Critic Review", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" (C) ", Style::default().fg(DIM)),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new(content).block(block);
        f.render_widget(paragraph, area);
    }
}

// ── Captain Panel ──────────────────────────────────────────────

enum CaptainEvent {
    CrewStarted { agent_id: String, task_title: String },
    CrewToolSummary { agent_id: String, text: String, success: bool },
    CrewDone { agent_id: String, summary: String },
    CrewFailed { agent_id: String, reason: String },
    MergeStarted { agent_id: String },
    MergeCompleted { agent_id: String, changed_paths: Vec<String> },
    CaptainStarted { sandbox_id: String },
    PlanCompleted,
    PlanFailed { reason: String },
}

pub struct CaptainPanel {
    events: Vec<CaptainEvent>,
    status: String,
}

impl CaptainPanel {
    pub fn new() -> Self {
        Self { events: Vec::new(), status: "idle".into() }
    }

    pub fn set_status(&mut self, status: &str) {
        self.status = status.to_string();
    }

    pub fn clear(&mut self) {
        self.events.clear();
        self.status = "idle".into();
    }

    pub fn add_crew_started(&mut self, agent_id: &str, task_title: &str) {
        self.events.push(CaptainEvent::CrewStarted { agent_id: agent_id.into(), task_title: task_title.into() });
    }

    pub fn add_tool_summary(&mut self, agent_id: &str, text: &str, success: bool) {
        self.events.push(CaptainEvent::CrewToolSummary { agent_id: agent_id.into(), text: text.into(), success });
    }

    pub fn add_crew_done(&mut self, agent_id: &str, summary: &str) {
        self.events.push(CaptainEvent::CrewDone { agent_id: agent_id.into(), summary: summary.into() });
    }

    pub fn add_crew_failed(&mut self, agent_id: &str, reason: &str) {
        self.events.push(CaptainEvent::CrewFailed { agent_id: agent_id.into(), reason: reason.into() });
    }

    pub fn add_merge_completed(&mut self, agent_id: &str, changed_paths: &[&str]) {
        self.events.push(CaptainEvent::MergeCompleted {
            agent_id: agent_id.into(),
            changed_paths: changed_paths.iter().map(|s| s.to_string()).collect(),
        });
    }

    pub fn add_captain_started(&mut self, sandbox_id: &str) {
        self.events.push(CaptainEvent::CaptainStarted { sandbox_id: sandbox_id.into() });
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let status_color = match self.status.as_str() {
            "running" => GREEN,
            "completed" => GREEN,
            "failed" => RED,
            _ => DIM,
        };

        let mut lines = vec![
            Line::from(vec![
                Span::raw("Status: "),
                Span::styled(self.status.to_uppercase(), Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(Span::styled("─".repeat(40), Style::default().fg(DIM))),
        ];

        if self.events.is_empty() {
            lines.push(Line::from(Span::styled("No activity yet", Style::default().fg(DIM))));
        }

        for event in &self.events {
            match event {
                CaptainEvent::CrewStarted { agent_id, task_title } => {
                    lines.push(Line::from(vec![
                        Span::styled("→ ", Style::default().fg(GREEN)),
                        Span::styled(agent_id.clone(), Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
                        Span::raw(format!(" started: {}", task_title)),
                    ]));
                }
                CaptainEvent::CrewToolSummary { text, success, .. } => {
                    let icon = if *success { "✓" } else { "✗" };
                    let color = if *success { GREEN } else { RED };
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {} ", icon), Style::default().fg(color)),
                        Span::raw(text.clone()),
                    ]));
                }
                CaptainEvent::CrewDone { agent_id, summary } => {
                    lines.push(Line::from(vec![
                        Span::styled("✓ ", Style::default().fg(GREEN)),
                        Span::styled(agent_id.clone(), Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
                        Span::raw(format!(" completed: {}", summary)),
                    ]));
                }
                CaptainEvent::CrewFailed { agent_id, reason } => {
                    lines.push(Line::from(vec![
                        Span::styled("✗ ", Style::default().fg(RED)),
                        Span::styled(agent_id.clone(), Style::default().fg(RED).add_modifier(Modifier::BOLD)),
                        Span::raw(format!(" failed: {}", reason)),
                    ]));
                }
                CaptainEvent::MergeStarted { agent_id } => {
                    lines.push(Line::from(vec![
                        Span::styled("⟳ ", Style::default().fg(YELLOW)),
                        Span::styled(format!("Merging {}...", agent_id), Style::default().fg(DIM)),
                    ]));
                }
                CaptainEvent::MergeCompleted { agent_id, changed_paths } => {
                    let mut spans = vec![
                        Span::styled("✓ ", Style::default().fg(GREEN)),
                        Span::styled(format!("Merged {}", agent_id), Style::default().fg(GREEN)),
                    ];
                    if !changed_paths.is_empty() {
                        spans.push(Span::styled(
                            format!(" ({})", changed_paths.join(", ")),
                            Style::default().fg(DIM),
                        ));
                    }
                    lines.push(Line::from(spans));
                }
                CaptainEvent::CaptainStarted { sandbox_id } => {
                    lines.push(Line::from(vec![
                        Span::styled("▶ ", Style::default().fg(CYAN)),
                        Span::styled(
                            format!("Captain started, sandbox: {}", sandbox_id),
                            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                }
                CaptainEvent::PlanCompleted => {
                    lines.push(Line::raw(""));
                    lines.push(Line::from(vec![
                        Span::styled("◆ ", Style::default().fg(GREEN)),
                        Span::styled("PLAN COMPLETED SUCCESSFULLY", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
                    ]));
                }
                CaptainEvent::PlanFailed { reason } => {
                    lines.push(Line::raw(""));
                    lines.push(Line::from(vec![
                        Span::styled("◆ ", Style::default().fg(RED)),
                        Span::styled(format!("PLAN FAILED: {}", reason), Style::default().fg(RED).add_modifier(Modifier::BOLD)),
                    ]));
                }
            }
        }

        // Keep last N events visible
        let visible_height = area.height.saturating_sub(3) as usize;
        if lines.len() > visible_height + 3 {
            let start = lines.len() - visible_height;
            lines = lines[start..].to_vec();
        }

        let block = Block::default()
            .title(Line::from(vec![
                Span::styled(" Captain Panel", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" ", Style::default()),
            ]))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(PURPLE));

        let paragraph = Paragraph::new(lines).block(block);
        f.render_widget(paragraph, area);
    }
}

// ── Slash Menu ─────────────────────────────────────────────────

const COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show commands"),
    ("/clear", "Clear chat"),
    ("/quit", "Exit"),
    ("/about", "About"),
];

pub struct SlashMenu {
    pub visible: bool,
    selected: usize,
    filtered: Vec<(String, String)>,
}

impl SlashMenu {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected: 0,
            filtered: COMMANDS.iter().map(|(c, d)| (c.to_string(), d.to_string())).collect(),
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.selected = 0;
        self.filtered = COMMANDS.iter().map(|(c, d)| (c.to_string(), d.to_string())).collect();
    }

    pub fn hide(&mut self) { self.visible = false; }

    pub fn filter(&mut self, text: &str) {
        self.filtered = COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(text))
            .map(|(c, d)| (c.to_string(), d.to_string()))
            .collect();
        self.selected = 0;
        self.visible = !self.filtered.is_empty();
    }

    pub fn move_up(&mut self) { if self.selected > 0 { self.selected -= 1; } }
    pub fn move_down(&mut self) { if self.selected + 1 < self.filtered.len() { self.selected += 1; } }
    pub fn current(&self) -> Option<&str> {
        self.filtered.get(self.selected).map(|(cmd, _)| cmd.as_str())
    }

    pub fn render(&self, f: &mut Frame, input_area: Rect) {
        if !self.visible || self.filtered.is_empty() {
            return;
        }

        let menu_height = self.filtered.len() as u16 + 2;
        let menu_area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(menu_height),
            width: 40.min(input_area.width),
            height: menu_height,
        };

        f.render_widget(Clear, menu_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(PURPLE))
            .style(Style::default().bg(Color::Black));

        let inner = block.inner(menu_area);
        f.render_widget(block, menu_area);

        let mut lines = Vec::new();
        for (i, (cmd, desc)) in self.filtered.iter().enumerate() {
            let style = if i == self.selected {
                Style::default().bg(PURPLE).fg(Color::White)
            } else {
                Style::default().fg(PURPLE)
            };
            let desc_style = if i == self.selected {
                Style::default().bg(PURPLE).fg(Color::White)
            } else {
                Style::default().fg(DIM)
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<12}", cmd), style),
                Span::styled(format!(" {}", desc), desc_style),
            ]));
        }

        let menu = Paragraph::new(lines).style(Style::default().bg(Color::Black));
        f.render_widget(menu, inner);
    }
}

// ── @ Menu ─────────────────────────────────────────────────────

const AT_TARGETS: &[(&str, &str)] = &[
    ("@conductor", "Send to Conductor"),
    ("@captain",   "Send to Captain"),
    ("@critic",    "Request a review"),
    ("@crew",      "Broadcast to Crew"),
];

pub struct AtMenu {
    pub visible: bool,
    selected: usize,
    filtered: Vec<(String, String)>,
}

impl AtMenu {
    pub fn new() -> Self {
        Self {
            visible: false,
            selected: 0,
            filtered: AT_TARGETS.iter().map(|(c, d)| (c.to_string(), d.to_string())).collect(),
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.selected = 0;
        self.filtered = AT_TARGETS.iter().map(|(c, d)| (c.to_string(), d.to_string())).collect();
    }

    pub fn hide(&mut self) { self.visible = false; }

    pub fn filter(&mut self, text: &str) {
        self.filtered = AT_TARGETS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(text))
            .map(|(c, d)| (c.to_string(), d.to_string()))
            .collect();
        self.selected = 0;
        self.visible = !self.filtered.is_empty();
    }

    pub fn move_up(&mut self) { if self.selected > 0 { self.selected -= 1; } }
    pub fn move_down(&mut self) { if self.selected + 1 < self.filtered.len() { self.selected += 1; } }
    pub fn current(&self) -> Option<&str> {
        self.filtered.get(self.selected).map(|(cmd, _)| cmd.as_str())
    }

    pub fn render(&self, f: &mut Frame, input_area: Rect) {
        if !self.visible || self.filtered.is_empty() { return; }

        let menu_height = self.filtered.len() as u16 + 2;
        let menu_area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(menu_height),
            width: 44.min(input_area.width),
            height: menu_height,
        };

        f.render_widget(Clear, menu_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CYAN))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(menu_area);
        f.render_widget(block, menu_area);

        let lines: Vec<Line> = self.filtered.iter().enumerate().map(|(i, (cmd, desc))| {
            let (cmd_style, desc_style) = if i == self.selected {
                (
                    Style::default().bg(CYAN).fg(Color::Black).add_modifier(Modifier::BOLD),
                    Style::default().bg(CYAN).fg(Color::Black),
                )
            } else {
                (Style::default().fg(CYAN), Style::default().fg(DIM))
            };
            Line::from(vec![
                Span::styled(format!("  {:<16}", cmd), cmd_style),
                Span::styled(format!(" {}", desc), desc_style),
            ])
        }).collect();

        let menu = Paragraph::new(lines).style(Style::default().bg(Color::Black));
        f.render_widget(menu, inner);
    }
}

// ── Token Meter ────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct TokenMeter {
    pub total_input: u32,
    pub total_output: u32,
    pub api_calls: u32,
    pub tool_calls: u32,
}

impl TokenMeter {
    pub fn record_tokens(&mut self, input: u32, output: u32) {
        self.total_input += input;
        self.total_output += output;
        self.api_calls += 1;
    }

    pub fn record_tool_call(&mut self) {
        self.tool_calls += 1;
    }

    pub fn is_empty(&self) -> bool {
        self.api_calls == 0
    }

    pub fn fmt_spans(&self) -> Vec<Span<'static>> {
        if self.is_empty() {
            return vec![];
        }
        vec![
            Span::styled("  ↑".to_string(), Style::default().fg(DIM)),
            Span::styled(fmt_k(self.total_input), Style::default().fg(Color::White)),
            Span::styled(" ↓".to_string(), Style::default().fg(DIM)),
            Span::styled(fmt_k(self.total_output), Style::default().fg(Color::White)),
            Span::styled(format!("  ⚡{}", self.api_calls), Style::default().fg(DIM)),
            if self.tool_calls > 0 {
                Span::styled(format!("  🔧{}", self.tool_calls), Style::default().fg(DIM))
            } else {
                Span::raw("".to_string())
            },
        ]
    }
}

fn fmt_k(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f32 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f32 / 1000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    // Build a log with a known content height and viewport, as render() would cache.
    fn log_with(content_rows: usize, viewport: usize) -> MessageLog {
        let mut log = MessageLog::new();
        log.content_rows = content_rows;
        log.viewport_height = viewport;
        log
    }

    #[test]
    fn first_scroll_up_anchors_at_bottom_not_top() {
        // 100 rows of content, 20 visible => max scroll is 80.
        let mut log = log_with(100, 20);
        assert!(log.auto_scroll);
        log.scroll_up();
        // Must land just above the bottom (79), NOT jump to the top (0).
        assert!(!log.auto_scroll);
        assert_eq!(log.scroll_offset, 79);
    }

    #[test]
    fn scroll_up_then_down_returns_and_repins() {
        let mut log = log_with(100, 20); // max scroll = 80
        log.scroll_up(); // 79
        log.scroll_up(); // 78
        assert_eq!(log.scroll_offset, 78);
        log.scroll_down(); // 79
        assert_eq!(log.scroll_offset, 79);
        log.scroll_down(); // 80 -> re-pins to bottom
        assert!(log.auto_scroll);
    }

    #[test]
    fn scroll_up_clamps_at_top() {
        let mut log = log_with(25, 20); // max scroll = 5
        for _ in 0..20 {
            log.scroll_up();
        }
        assert_eq!(log.scroll_offset, 0);
        assert!(!log.auto_scroll);
    }

    #[test]
    fn content_shorter_than_viewport_has_no_scroll() {
        let mut log = log_with(5, 20); // everything fits, max scroll = 0
        log.scroll_up();
        assert_eq!(log.scroll_offset, 0);
    }
}

#[cfg(test)]
mod plan_strip_tests {
    use super::*;

    #[test]
    fn no_plan_passes_through() {
        let (v, drafting) = MessageLog::strip_plan_block("Just a normal reply.");
        assert_eq!(v, "Just a normal reply.");
        assert!(!drafting);
    }

    #[test]
    fn closed_plan_block_is_removed_keeping_prose() {
        let text = "Here is the plan.\n<plan>\n[[task]]\nid=\"t1\"\n</plan>\nDone!";
        let (v, drafting) = MessageLog::strip_plan_block(text);
        assert!(!v.contains("[[task]]"));
        assert!(!v.contains("<plan>"));
        assert!(v.contains("Here is the plan."));
        assert!(v.contains("Done!"));
        assert!(!drafting);
    }

    #[test]
    fn open_plan_block_reports_drafting_and_hides_toml() {
        let text = "Spinning this up.\n<plan>\n[[task]]\nid=\"t1\"\ndescription=\"lots of code";
        let (v, drafting) = MessageLog::strip_plan_block(text);
        assert_eq!(v, "Spinning this up.");
        assert!(!v.contains("[[task]]"));
        assert!(drafting);
    }
}
