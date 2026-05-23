use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

const PURPLE: Color = Color::Rgb(109, 40, 217);
const CYAN: Color = Color::Rgb(34, 211, 238);
const GREEN: Color = Color::Rgb(34, 197, 94);
const YELLOW: Color = Color::Rgb(234, 179, 8);
const RED: Color = Color::Rgb(239, 68, 68);
const DIM: Color = Color::Rgb(102, 102, 102);

// ── Message Log ────────────────────────────────────────────────

pub struct MessageLog {
    lines: Vec<LogLine>,
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
        Self { lines: Vec::new() }
    }

    pub fn add_user(&mut self, text: &str) {
        self.lines.push(LogLine {
            prefix: "• ".into(),
            prefix_color: PURPLE,
            text: text.to_string(),
            italic: false,
            dim: false,
        });
    }

    pub fn add_conductor(&mut self, text: &str) {
        self.lines.push(LogLine {
            prefix: "◈ ".into(),
            prefix_color: GREEN,
            text: text.to_string(),
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
        self.lines.push(LogLine {
            prefix: "⟳ ".into(),
            prefix_color: YELLOW,
            text: text.to_string(),
            italic: true,
            dim: true,
        });
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

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let mut spans: Vec<Line> = Vec::new();
        for line in &self.lines {
            let prefix_style = Style::default().fg(line.prefix_color);
            let mut text_style = Style::default().fg(Color::White);
            if line.italic {
                text_style = text_style.add_modifier(Modifier::ITALIC);
            }
            if line.dim {
                text_style = text_style.fg(DIM);
            }
            spans.push(Line::from(vec![
                Span::styled(&line.prefix, prefix_style),
                Span::styled(&line.text, text_style),
            ]));
        }

        // Show last N lines that fit the area
        let visible_height = area.height as usize;
        let start = if spans.len() > visible_height {
            spans.len() - visible_height
        } else {
            0
        };

        let visible: Vec<Line> = spans[start..].to_vec();
        let log = Paragraph::new(visible);
        f.render_widget(log, area);
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
                Span::styled("Plan Preview", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" (P)", Style::default().fg(DIM)),
            ]))
            .borders(Borders::ALL)
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
                Span::styled("Critic Review", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(" (C)", Style::default().fg(DIM)),
            ]))
            .borders(Borders::ALL)
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
            .title(Span::styled("Captain Panel", Style::default().add_modifier(Modifier::BOLD)))
            .borders(Borders::ALL)
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

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn filter(&mut self, text: &str) {
        if text.is_empty() {
            self.filtered = COMMANDS.iter().map(|(c, d)| (c.to_string(), d.to_string())).collect();
        } else {
            self.filtered = COMMANDS
                .iter()
                .filter(|(cmd, _)| cmd.starts_with(text))
                .map(|(c, d)| (c.to_string(), d.to_string()))
                .collect();
        }
        self.selected = 0;
        self.visible = !self.filtered.is_empty();
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
            .border_style(Style::default().fg(PURPLE))
            .style(Style::default().bg(Color::Rgb(13, 13, 26)));

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

        let menu = Paragraph::new(lines);
        f.render_widget(menu, inner);
    }
}
