use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::time::{Duration, Instant};

use crate::tui::splash::render_splash;
use crate::tui::setup::SetupState;
use crate::tui::widgets::*;

const PURPLE: Color = Color::Rgb(109, 40, 217);
const CYAN: Color = Color::Rgb(34, 211, 238);
const GREEN: Color = Color::Rgb(34, 197, 94);
const YELLOW: Color = Color::Rgb(234, 179, 8);
const RED: Color = Color::Rgb(239, 68, 68);
const DIM: Color = Color::Rgb(102, 102, 102);
const BG: Color = Color::Rgb(0, 0, 0);
const BG_PANEL: Color = Color::Rgb(13, 13, 26);

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Splash { frame: u64, started: Instant },
    Setup,
    Main,
}

pub struct TuiApp {
    pub screen: Screen,
    pub setup: SetupState,
    pub message_log: MessageLog,
    pub plan_preview: PlanPreview,
    pub critic_panel: CriticPanel,
    pub captain_panel: CaptainPanel,
    pub overlay_visible: bool,
    pub plan_expanded: bool,
    pub critic_expanded: bool,
    pub input: String,
    pub input_cursor: usize,
    pub slash_menu: SlashMenu,
    pub scroll_offset: usize,
    pub should_quit: bool,
}

impl TuiApp {
    pub fn new() -> Self {
        Self {
            screen: Screen::Splash { frame: 0, started: Instant::now() },
            setup: SetupState::new(),
            message_log: MessageLog::new(),
            plan_preview: PlanPreview::new(),
            critic_panel: CriticPanel::new(),
            captain_panel: CaptainPanel::new(),
            overlay_visible: false,
            plan_expanded: false,
            critic_expanded: false,
            input: String::new(),
            input_cursor: 0,
            slash_menu: SlashMenu::new(),
            scroll_offset: 0,
            should_quit: false,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_loop(&mut terminal);

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        result
    }

    fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let tick_rate = Duration::from_millis(600);
        let mut last_tick = Instant::now();

        loop {
            // Draw
            terminal.draw(|f| self.draw(f))?;

            // Handle input
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if crossterm::event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key.code, key.modifiers);
                }
            }

            if last_tick.elapsed() >= tick_rate {
                self.tick();
                last_tick = Instant::now();
            }

            if self.should_quit {
                return Ok(());
            }
        }
    }

    fn tick(&mut self) {
        match &self.screen {
            Screen::Splash { frame, started } => {
                let elapsed = started.elapsed();
                if elapsed > Duration::from_secs(3) {
                    self.screen = Screen::Splash { frame: frame + 1, started: *started };
                } else {
                    self.screen = Screen::Splash { frame: frame + 1, started: *started };
                }
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match &self.screen {
            Screen::Splash { started, .. } => {
                if code == KeyCode::Enter || code == KeyCode::Char(' ') {
                    if started.elapsed() > Duration::from_secs(2) {
                        self.screen = Screen::Setup;
                    }
                }
                if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                    self.should_quit = true;
                }
            }
            Screen::Setup => {
                match code {
                    KeyCode::Up => self.setup.move_up(),
                    KeyCode::Down => self.setup.move_down(),
                    KeyCode::Enter => {
                        if self.setup.step == 1 {
                            self.setup.advance_to_step2();
                        } else if self.setup.step == 2 && !self.setup.api_key.is_empty() {
                            self.screen = Screen::Main;
                            self.message_log.add_system("MowisAI ready. Type your message.");
                        }
                    }
                    KeyCode::Char(c) if self.setup.step == 2 => {
                        self.setup.api_key.push(c);
                    }
                    KeyCode::Backspace if self.setup.step == 2 => {
                        self.setup.api_key.pop();
                    }
                    KeyCode::Esc => self.should_quit = true,
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                        self.should_quit = true;
                    }
                    _ => {}
                }
            }
            Screen::Main => {
                match code {
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                        self.should_quit = true;
                    }
                    KeyCode::Tab => {
                        self.overlay_visible = !self.overlay_visible;
                    }
                    KeyCode::Char('p') if self.overlay_visible => {
                        self.plan_expanded = !self.plan_expanded;
                    }
                    KeyCode::Char('c') if self.overlay_visible && !self.input.is_empty() => {
                        // Don't toggle critic when typing
                    }
                    KeyCode::Char('c') if self.overlay_visible => {
                        self.critic_expanded = !self.critic_expanded;
                    }
                    KeyCode::Char('/') if self.input.is_empty() => {
                        self.input.push('/');
                        self.slash_menu.show();
                    }
                    KeyCode::Char(c) => {
                        self.input.push(c);
                        if self.input.starts_with('/') {
                            self.slash_menu.filter(&self.input);
                        }
                    }
                    KeyCode::Backspace => {
                        self.input.pop();
                        if self.input.starts_with('/') {
                            self.slash_menu.filter(&self.input);
                        } else if self.input.is_empty() {
                            self.slash_menu.hide();
                        }
                    }
                    KeyCode::Enter => {
                        let msg = self.input.trim().to_string();
                        if !msg.is_empty() {
                            if msg == "/help" {
                                self.message_log.add_system("Commands: /help /clear /quit /about");
                            } else if msg == "/clear" {
                                self.message_log.clear();
                            } else if msg == "/quit" {
                                self.should_quit = true;
                            } else if msg == "/about" {
                                self.message_log.add_system("MowisAI v1.0 — multi-agent conductor system");
                            } else {
                                self.message_log.add_user(&msg);
                                self.handle_conversation(&msg);
                            }
                        }
                        self.input.clear();
                        self.slash_menu.hide();
                    }
                    KeyCode::Esc => {
                        if self.slash_menu.visible {
                            self.slash_menu.hide();
                        } else {
                            self.input.clear();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_conversation(&mut self, msg: &str) {
        let lower = msg.to_lowercase();

        if self.plan_preview.is_awaiting_approval() {
            if lower.contains("approve") || lower.contains("yes") {
                self.plan_preview.approve();
                self.message_log.add_conductor("Plan approved! Starting execution...");
                self.captain_panel.set_status("running");
                self.overlay_visible = true;
                self.message_log.add_system("Captain started execution.");
                // In real app: send UserApproved to conductor
            } else if lower.contains("cancel") {
                self.plan_preview.cancel();
                self.captain_panel.clear();
                self.critic_panel.clear();
                self.message_log.add_conductor("Plan cancelled.");
            } else {
                self.message_log.add_conductor("Type 'approve' or 'cancel'.");
            }
            return;
        }

        // Simulate conductor response
        if lower.contains("hello") || lower.contains("hi") || lower.contains("hey") {
            self.message_log.add_conductor(
                "Hey! I'm MowisAI, your multi-agent coding assistant. What would you like to work on?",
            );
        } else if lower.contains("build") || lower.contains("create") || lower.contains("make")
            || lower.contains("implement") || lower.contains("fix")
        {
            self.draft_plan(msg);
        } else {
            self.message_log.add_conductor(&format!(
                "I understand you said: '{}'. Could you tell me what you'd like to work on?",
                msg
            ));
        }
    }

    fn draft_plan(&mut self, goal: &str) {
        self.message_log.add_thinking("Analyzing your request and drafting a plan...");

        let tasks = if goal.to_lowercase().contains("api") {
            vec![
                TaskInfo { id: "t1".into(), title: "Set up project structure".into(), deps: vec![], tier: "fast".into(), budget: 10 },
                TaskInfo { id: "t2".into(), title: "Implement API routes".into(), deps: vec!["t1".into()], tier: "mid".into(), budget: 20 },
                TaskInfo { id: "t3".into(), title: "Add data models".into(), deps: vec!["t1".into()], tier: "mid".into(), budget: 15 },
                TaskInfo { id: "t4".into(), title: "Write tests".into(), deps: vec!["t2".into(), "t3".into()], tier: "fast".into(), budget: 20 },
            ]
        } else {
            vec![
                TaskInfo { id: "t1".into(), title: "Analyze requirements".into(), deps: vec![], tier: "mid".into(), budget: 10 },
                TaskInfo { id: "t2".into(), title: "Set up project".into(), deps: vec!["t1".into()], tier: "fast".into(), budget: 10 },
                TaskInfo { id: "t3".into(), title: "Implement core".into(), deps: vec!["t2".into()], tier: "mid".into(), budget: 25 },
                TaskInfo { id: "t4".into(), title: "Add tests".into(), deps: vec!["t3".into()], tier: "fast".into(), budget: 15 },
            ]
        };

        let plan_id = format!("plan-{}", chrono::Utc::now().format("%H%M%S"));
        let overview = format!("## Plan: {}\n\nThis plan breaks down your request into {} tasks.\nEach task runs in an isolated sandbox.", goal, tasks.len());

        self.plan_preview.set_plan(
            plan_id.clone(),
            1,
            overview,
            tasks,
            "ubuntu-24.04".into(),
            8192,
        );

        self.message_log.add_plan_link(&plan_id, 1);

        // Critic reviews
        self.critic_panel.set_reviewing(&plan_id, 1);
        self.message_log.add_system("Critic reviewing plan...");

        // Simulate critic verdict
        self.critic_panel.set_verdict(
            "approve",
            vec![
                CriticIssue { severity: "info".into(), section: "tasks.toml".into(), message: "Task dependencies are valid".into(), suggested_fix: None },
                CriticIssue { severity: "warn".into(), section: "sandbox.toml".into(), message: "Consider more RAM for large builds".into(), suggested_fix: Some("Set ram_mb = 16384".into()) },
            ],
            "Plan is well-structured with clear task boundaries.".into(),
        );

        self.message_log.add_critic_verdict("approve", "Plan is well-structured with clear task boundaries.");
        self.plan_preview.set_awaiting();
        self.message_log.add_awaiting_approval(&plan_id);
    }

    fn draw(&mut self, f: &mut Frame) {
        match &self.screen {
            Screen::Splash { frame, .. } => {
                render_splash(f, *frame);
            }
            Screen::Setup => {
                self.setup.draw(f);
            }
            Screen::Main => {
                self.draw_main(f);
            }
        }
    }

    fn draw_main(&mut self, f: &mut Frame) {
        let area = f.size();

        if self.overlay_visible {
            // Overlay view: PlanPreview + CriticPanel + CaptainPanel
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(10),  // panels
                    Constraint::Length(1), // divider
                    Constraint::Length(1), // footer
                ])
                .split(area);

            let panel_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Percentage(34),
                ])
                .split(chunks[0]);

            self.plan_preview.render(f, panel_chunks[0], self.plan_expanded);
            self.critic_panel.render(f, panel_chunks[1], self.critic_expanded);
            self.captain_panel.render(f, panel_chunks[2]);

            // Footer
            let footer = Paragraph::new(Line::from(vec![
                Span::styled("Tab", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::raw(" Main • "),
                Span::styled("P", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::raw(" Plan • "),
                Span::styled("C", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::raw(" Critic • "),
                Span::styled("Ctrl+c", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::raw(" Exit"),
            ]));
            f.render_widget(footer, chunks[2]);
        } else {
            // Main chat view
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),  // header
                    Constraint::Length(3),  // status
                    Constraint::Min(1),    // message log
                    Constraint::Length(1),  // divider
                    Constraint::Length(1),  // input
                    Constraint::Length(1),  // divider
                    Constraint::Length(1),  // footer
                ])
                .split(area);

            // Header
            let header = Paragraph::new(vec![
                Line::from(vec![
                    Span::raw("Welcome to "),
                    Span::styled("MowisAI", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(Span::styled("multi-agent conductor system", Style::default().fg(DIM))),
            ])
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(PURPLE)));
            f.render_widget(header, chunks[0]);

            // Status
            let status = Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("● ", Style::default().fg(PURPLE)),
                    Span::raw("Connected to MowisAI"),
                ]),
                Line::from(vec![
                    Span::styled("● ", Style::default().fg(PURPLE)),
                    Span::raw("Session active"),
                ]),
            ]);
            f.render_widget(status, chunks[1]);

            // Message log
            self.message_log.render(f, chunks[2]);

            // Divider
            let divider = Paragraph::new("─".repeat(chunks[3].width as usize))
                .style(Style::default().fg(PURPLE));
            f.render_widget(divider, chunks[3]);

            // Input
            let input_text = if self.input.is_empty() {
                Span::styled("Type a message, / for commands", Style::default().fg(DIM))
            } else {
                Span::raw(&self.input)
            };
            let input = Paragraph::new(input_text);
            f.render_widget(input, chunks[4]);

            // Divider
            let divider2 = Paragraph::new("─".repeat(chunks[5].width as usize))
                .style(Style::default().fg(PURPLE));
            f.render_widget(divider2, chunks[5]);

            // Footer
            let footer = Paragraph::new(Line::from(vec![
                Span::styled("Tab", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::raw(" Overlay • "),
                Span::styled("P", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::raw(" Plan • "),
                Span::styled("C", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::raw(" Critic • "),
                Span::styled("Ctrl+c", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::raw(" Exit"),
            ]));
            f.render_widget(footer, chunks[6]);

            // Slash menu overlay
            if self.slash_menu.visible {
                self.slash_menu.render(f, chunks[4]);
            }
        }
    }
}
