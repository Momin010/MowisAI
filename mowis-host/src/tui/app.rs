use anyhow::Result;
use crossterm::{
    event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::tui::splash::render_splash;
use crate::tui::setup::SetupState;
use crate::tui::widgets::*;
use mowis_orchestration::config::OrchConfig;
use mowis_orchestration::conductor::{Conductor, ConductorCommand, ConductorReply};
use mowis_orchestration::critic::Critic;
use mowis_orchestration::events::{Event as OrchEvent, EventBus};

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() { break; }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' { break; }
                        if c == '\x1b' {
                            if chars.peek() == Some(&'\\') { chars.next(); }
                            break;
                        }
                    }
                }
                Some(_) => { chars.next(); }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

const PURPLE: Color = Color::Rgb(139, 92, 246);
const CYAN:   Color = Color::Rgb(34, 211, 238);
const GREEN:  Color = Color::Rgb(74, 222, 128);
const DIM:    Color = Color::Rgb(71, 85, 105);
const BORDER: Color = Color::Rgb(51, 65, 85);

#[derive(Debug, Clone, PartialEq)]
pub enum AppScreen {
    Splash { frame: u64 },
    Setup,
    Main,
}

pub enum TuiEvent {
    Terminal(crossterm::event::KeyEvent),
    Orch(OrchEvent),
    ConductorReply(ConductorReply),
    Tick,
}

pub struct TuiApp {
    pub screen: AppScreen,
    pub setup: SetupState,
    pub message_log: MessageLog,
    pub plan_preview: PlanPreview,
    pub critic_panel: CriticPanel,
    pub captain_panel: CaptainPanel,
    pub overlay_visible: bool,
    pub plan_expanded: bool,
    pub critic_expanded: bool,
    pub input: String,
    pub slash_menu: SlashMenu,
    pub at_menu: AtMenu,
    pub token_meter: TokenMeter,
    pub should_quit: bool,
    pub conductor_tx: Option<mpsc::Sender<ConductorCommand>>,
    pub event_rx: Option<mpsc::UnboundedReceiver<TuiEvent>>,
    pub event_tx: mpsc::UnboundedSender<TuiEvent>,
    pub orchestrator_started: bool,
}

impl TuiApp {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Check if config already exists
        let existing_config = OrchConfig::load().ok().filter(|c| !c.providers.is_empty());
        let has_config = existing_config.is_some();

        let mut app = Self {
            screen: if has_config { AppScreen::Main } else { AppScreen::Splash { frame: 0 } },
            setup: SetupState::new(),
            message_log: MessageLog::new(),
            plan_preview: PlanPreview::new(),
            critic_panel: CriticPanel::new(),
            captain_panel: CaptainPanel::new(),
            overlay_visible: false,
            plan_expanded: false,
            critic_expanded: false,
            input: String::new(),
            slash_menu: SlashMenu::new(),
            at_menu: AtMenu::new(),
            token_meter: TokenMeter::default(),
            should_quit: false,
            conductor_tx: None,
            event_rx: Some(event_rx),
            event_tx,
            orchestrator_started: false,
        };

        // If config exists, start orchestrator immediately
        if let Some(cfg) = existing_config {
            app.start_orchestrator(cfg);
            app.message_log.add_system("MowisAI ready. Type your message.");
        }

        app
    }

    fn tick(&mut self) {
        if let AppScreen::Splash { ref mut frame } = self.screen {
            *frame += 1;
        }
        self.message_log.tick_spinner();
    }

    fn start_orchestrator(&mut self, cfg: OrchConfig) {
        let bus = EventBus::new();
        let bus_for_critic = bus.clone();
        let event_tx = self.event_tx.clone();

        // Subscribe to bus events and forward to TUI
        let bus_sub = bus.subscribe();
        let fwd_tx = event_tx.clone();
        tokio::spawn(async move {
            let mut rx = bus_sub;
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let _ = fwd_tx.send(TuiEvent::Orch(ev));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });

        // Create conductor
        let (mut conductor, _unused_tx) = match Conductor::new(&cfg, bus.clone()) {
            Ok(v) => v,
            Err(e) => {
                self.message_log.add_system(&format!("Failed to create conductor: {}", e));
                return;
            }
        };

        // Give the conductor a per-session sandbox: crews build here (via the
        // start_build tool), and save_to_host copies from here to the user's
        // project. Without this the build/save tools have nowhere to operate.
        let session_id = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ").to_string();
        let workspace = std::path::PathBuf::from(".mowis/sessions")
            .join(&session_id)
            .join("workspace");
        let _ = std::fs::create_dir_all(&workspace);
        let workspace = workspace.canonicalize().unwrap_or(workspace);
        let save_dest =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        conductor.set_workspace(workspace, save_dest);

        // Create our own command channel
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<ConductorCommand>(64);
        self.conductor_tx = Some(cmd_tx);

        // Spawn conductor task that processes commands
        let conductor_event_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    ConductorCommand::UserMessage { text, reply_tx } => {
                        let result = conductor.handle_user_message(text).await;
                        match result {
                            Ok(reply) => {
                                let _ = conductor_event_tx.send(TuiEvent::ConductorReply(reply));
                                let _ = reply_tx.send(ConductorReply::Chat { reply: String::new() });
                            }
                            Err(e) => {
                                let _ = conductor_event_tx.send(TuiEvent::ConductorReply(
                                    ConductorReply::Error { message: e.to_string() }
                                ));
                            }
                        }
                    }
                    ConductorCommand::CriticVerdict { plan_id, version, verdict } => {
                        let _ = conductor.handle_critic_verdict(plan_id, version, verdict).await;
                    }
                    ConductorCommand::EndConversation => {
                        bus.emit(OrchEvent::ConversationEnded);
                        break;
                    }
                }
            }
        });

        // Create and spawn critic
        let mut critic = match Critic::new(&cfg, bus_for_critic) {
            Ok(c) => c,
            Err(e) => {
                self.message_log.add_system(&format!("Failed to create critic: {}", e));
                return;
            }
        };
        tokio::spawn(async move {
            if let Err(e) = critic.run().await {
                tracing::error!(error = %e, "critic exited with error");
            }
        });

        self.orchestrator_started = true;
    }

    pub async fn run_async(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_loop(&mut terminal).await;

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste
        )?;
        terminal.show_cursor()?;

        result
    }

    async fn run_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let tick_rate = Duration::from_millis(500);
        let mut event_rx = self.event_rx.take().unwrap();
        let mut last_tick = std::time::Instant::now();

        loop {
            terminal.draw(|f| self.draw(f))?;

            // Wait for terminal input with short timeout
            if crossterm::event::poll(Duration::from_millis(16))? {
                match event::read()? {
                    Event::Key(key) => {
                        self.handle_key(key.code, key.modifiers).await;
                        if self.should_quit {
                            if let Some(ref tx) = self.conductor_tx {
                                let _ = tx.send(ConductorCommand::EndConversation).await;
                            }
                            return Ok(());
                        }
                    }
                    // Mouse wheel scrolls the message log on the main screen.
                    Event::Mouse(me) if matches!(self.screen, AppScreen::Main) => {
                        match me.kind {
                            MouseEventKind::ScrollUp => self.message_log.scroll_up(),
                            MouseEventKind::ScrollDown => self.message_log.scroll_down(),
                            _ => {}
                        }
                    }
                    // Bracketed paste: the whole clipboard/dictation blob arrives
                    // as ONE event (newlines included), so it never gets split
                    // into multiple messages by the Enter handler.
                    Event::Paste(text) => match self.screen {
                        AppScreen::Main => {
                            self.input.push_str(&strip_ansi(&text));
                            if self.input.starts_with('/') {
                                self.slash_menu.filter(&self.input);
                            }
                        }
                        AppScreen::Setup if self.setup.step == 2 => {
                            self.setup.api_key.push_str(text.trim());
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }

            // Drain all pending orchestration events
            loop {
                match event_rx.try_recv() {
                    Ok(TuiEvent::Orch(ev)) => self.handle_orch_event(ev),
                    Ok(TuiEvent::ConductorReply(reply)) => self.handle_conductor_reply(reply),
                    Ok(_) => {}
                    Err(_) => break,
                }
            }

            // Tick for animations
            if last_tick.elapsed() >= tick_rate {
                self.tick();
                last_tick = std::time::Instant::now();
            }
        }
    }

    fn handle_orch_event(&mut self, event: OrchEvent) {
        match event {
            OrchEvent::TokensUsed { input_tokens, output_tokens, .. } => {
                self.token_meter.record_tokens(input_tokens, output_tokens);
            }
            OrchEvent::CrewToolSummary { agent_id, text, tool_name: _, success } => {
                self.captain_panel.add_tool_summary(&agent_id, &text, success);
                self.token_meter.record_tool_call();
            }
            OrchEvent::CrewStarted { task_id, agent_id, .. } => {
                self.captain_panel.add_crew_started(&agent_id, &task_id.0);
            }
            OrchEvent::CrewDone { agent_id, summary, .. } => {
                self.captain_panel.add_crew_done(&agent_id, &summary);
            }
            OrchEvent::CrewFailed { agent_id, reason, .. } => {
                self.captain_panel.add_crew_failed(&agent_id, &reason);
            }
            OrchEvent::PlanDrafted { plan_id, version } => {
                self.message_log.add_plan_link(&plan_id.0, version);
                // Set plan preview with basic info
                self.plan_preview.set_plan(
                    plan_id.0.clone(),
                    version,
                    "Plan drafted by Conductor".into(),
                    vec![],
                    "N/A".into(),
                    0,
                );
            }
            OrchEvent::PlanRevised { plan_id, version } => {
                self.message_log.add_system(&format!("Plan {} revised to v{}", plan_id.0, version));
            }
            OrchEvent::CriticVerdict { plan_id: _, version: _, verdict } => {
                let verdict_str = match &verdict {
                    mowis_orchestration::critic::Verdict::Approve => "approve",
                    mowis_orchestration::critic::Verdict::Revise { .. } => "revise",
                    mowis_orchestration::critic::Verdict::Block { .. } => "block",
                };
                let issues = match &verdict {
                    mowis_orchestration::critic::Verdict::Revise { issues } => {
                        issues.iter().map(|i| CriticIssue {
                            severity: i.severity.clone(),
                            section: i.section.clone(),
                            message: i.message.clone(),
                            suggested_fix: i.suggested_fix.clone(),
                        }).collect()
                    }
                    mowis_orchestration::critic::Verdict::Block { issues, .. } => {
                        issues.iter().map(|i| CriticIssue {
                            severity: i.severity.clone(),
                            section: i.section.clone(),
                            message: i.message.clone(),
                            suggested_fix: i.suggested_fix.clone(),
                        }).collect()
                    }
                    _ => vec![],
                };
                self.critic_panel.set_verdict(verdict_str, issues, String::new());
                self.message_log.add_critic_verdict(verdict_str, "");
            }
            OrchEvent::CriticReviewing { plan_id, version } => {
                self.critic_panel.set_reviewing(&plan_id.0, version);
            }
            OrchEvent::CaptainStarted { sandbox_id, .. } => {
                self.captain_panel.set_status("running");
                self.captain_panel.add_captain_started(&sandbox_id);
                self.overlay_visible = true;
            }
            OrchEvent::MergeCompleted { agent_id, .. } => {
                self.captain_panel.add_merge_completed(&agent_id, &[]);
            }
            OrchEvent::PlanCompleted { .. } => {
                self.captain_panel.set_status("completed");
                self.message_log.add_system("Plan completed successfully!");
            }
            OrchEvent::PlanFailed { reason, .. } => {
                self.captain_panel.set_status("failed");
                self.message_log.add_system(&format!("Plan failed: {}", reason));
            }
            OrchEvent::ConversationEnded => {
                self.message_log.add_system("Conversation ended.");
            }
            OrchEvent::StreamToken { text } => {
                self.message_log.push_stream_token(&text);
            }
            OrchEvent::StreamDone => {
                self.message_log.finish_streaming();
            }
            _ => {}
        }
    }

    fn handle_conductor_reply(&mut self, reply: ConductorReply) {
        // If we already streamed the text, don't add it again
        let was_streaming = self.message_log.streaming || self.message_log.had_streaming;
        self.message_log.finish_streaming();
        self.message_log.stop_thinking();

        match reply {
            ConductorReply::Chat { reply } => {
                if !reply.is_empty() && !was_streaming {
                    // Only add if we didn't already stream it
                    self.message_log.add_conductor(&reply);
                }
            }
            ConductorReply::PlanDrafted { plan_id, version } => {
                self.message_log.add_plan_link(&plan_id.0, version);
                self.plan_preview.set_awaiting();
                self.message_log.add_awaiting_approval(&plan_id.0);
            }
            ConductorReply::PlanRevised { plan_id, version } => {
                self.message_log.add_system(&format!("Plan revised: {} v{}", plan_id.0, version));
            }
            ConductorReply::Error { message } => {
                self.message_log.add_system(&format!("Error: {}", message));
            }
            _ => {}
        }
    }

    async fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match &self.screen {
            AppScreen::Splash { frame } => {
                if code == KeyCode::Enter || code == KeyCode::Char(' ') {
                    if *frame > 4 {
                        self.screen = AppScreen::Setup;
                    }
                }
                if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                    self.should_quit = true;
                }
            }
            AppScreen::Setup => {
                match code {
                    KeyCode::Up => self.setup.move_up(),
                    KeyCode::Down => self.setup.move_down(),
                    KeyCode::Enter => {
                        if self.setup.step == 1 {
                            self.setup.advance_to_step2();
                        } else if self.setup.step == 2 {
                            // Save config
                            match self.setup.save_config() {
                                Ok(cfg) => {
                                    self.screen = AppScreen::Main;
                                    self.start_orchestrator(cfg);
                                    self.message_log.add_system("MowisAI ready. Type your message.");
                                }
                                Err(e) => {
                                    self.message_log.add_system(&format!("Setup error: {}", e));
                                }
                            }
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
            AppScreen::Main => {
                match code {
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                        self.should_quit = true;
                    }
                    // ── Menu navigation (must come before general Up/Down/Tab) ──
                    KeyCode::Up if self.slash_menu.visible => { self.slash_menu.move_up(); }
                    KeyCode::Down if self.slash_menu.visible => { self.slash_menu.move_down(); }
                    KeyCode::Up if self.at_menu.visible => { self.at_menu.move_up(); }
                    KeyCode::Down if self.at_menu.visible => { self.at_menu.move_down(); }
                    KeyCode::Tab if self.slash_menu.visible => {
                        if let Some(cmd) = self.slash_menu.current().map(|s| s.to_string()) {
                            self.input = cmd;
                            self.slash_menu.hide();
                        }
                    }
                    KeyCode::Tab if self.at_menu.visible => {
                        if let Some(cmd) = self.at_menu.current().map(|s| format!("{} ", s)) {
                            self.input = cmd;
                            self.at_menu.hide();
                        }
                    }
                    // ── General navigation ────────────────────────────────────
                    KeyCode::Tab => {
                        self.overlay_visible = !self.overlay_visible;
                    }
                    KeyCode::Char('p') if self.overlay_visible && self.input.is_empty() => {
                        self.plan_expanded = !self.plan_expanded;
                    }
                    KeyCode::Char('c') if self.overlay_visible && self.input.is_empty() => {
                        self.critic_expanded = !self.critic_expanded;
                    }
                    KeyCode::Up if self.input.is_empty() => { self.message_log.scroll_up(); }
                    KeyCode::Down if self.input.is_empty() => { self.message_log.scroll_down(); }
                    KeyCode::PageUp => { for _ in 0..10 { self.message_log.scroll_up(); } }
                    KeyCode::PageDown => { for _ in 0..10 { self.message_log.scroll_down(); } }
                    // ── Trigger menus on / and @ ──────────────────────────────
                    KeyCode::Char('/') if self.input.is_empty() => {
                        self.input.push('/');
                        self.slash_menu.show();
                    }
                    KeyCode::Char('@') if self.input.is_empty() => {
                        self.input.push('@');
                        self.at_menu.show();
                    }
                    // ── Regular character input ───────────────────────────────
                    KeyCode::Char(c) => {
                        self.input.push(c);
                        if self.input.starts_with('/') {
                            self.slash_menu.filter(&self.input);
                        } else if self.input.starts_with('@') {
                            self.at_menu.filter(&self.input);
                        }
                    }
                    KeyCode::Backspace => {
                        self.input.pop();
                        if self.input.starts_with('/') {
                            self.slash_menu.filter(&self.input);
                        } else if self.input.starts_with('@') {
                            self.at_menu.filter(&self.input);
                        } else if self.input.is_empty() {
                            self.slash_menu.hide();
                            self.at_menu.hide();
                        }
                    }
                    KeyCode::Enter => {
                        // Slash menu: Enter executes the highlighted command
                        if self.slash_menu.visible {
                            if let Some(cmd) = self.slash_menu.current().map(|s| s.to_string()) {
                                self.input = cmd;
                            }
                            self.slash_menu.hide();
                        }
                        // @ menu: Enter inserts the target into input (user still types message)
                        if self.at_menu.visible {
                            if let Some(cmd) = self.at_menu.current().map(|s| format!("{} ", s)) {
                                self.input = cmd;
                            }
                            self.at_menu.hide();
                            return; // don't send yet — let user finish the message
                        }
                        let msg = self.input.trim().to_string();
                        if !msg.is_empty() {
                            if msg == "/help" {
                                self.message_log.add_system("Commands: /help /clear /quit /about");
                            } else if msg == "/clear" {
                                self.message_log.clear();
                            } else if msg == "/quit" {
                                self.should_quit = true;
                            } else if msg == "/about" {
                                self.message_log.add_system("MowisAI v1.0 — multi-agent conductor");
                            } else {
                                self.send_message(msg).await;
                            }
                        }
                        self.input.clear();
                        self.slash_menu.hide();
                        self.at_menu.hide();
                    }
                    KeyCode::Esc => {
                        if self.slash_menu.visible {
                            self.slash_menu.hide();
                        } else if self.at_menu.visible {
                            self.at_menu.hide();
                        } else {
                            self.input.clear();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    async fn send_message(&mut self, msg: String) {
        self.message_log.add_user(&msg);

        if let Some(ref tx) = self.conductor_tx {
            self.message_log.thinking = true; // Show spinner only, no text line
            let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();
            if let Err(e) = tx.send(ConductorCommand::UserMessage {
                text: msg,
                reply_tx,
            }).await {
                self.message_log.add_system(&format!("Failed to send to conductor: {}", e));
            }
        } else {
            self.message_log.add_system("Orchestrator not started. Run setup first.");
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        match &self.screen {
            AppScreen::Splash { frame } => {
                render_splash(f, *frame);
            }
            AppScreen::Setup => {
                self.setup.draw(f);
            }
            AppScreen::Main => {
                self.draw_main(f);
            }
        }
    }

    fn draw_main(&mut self, f: &mut Frame) {
        // Pitch-black background across the whole terminal
        f.render_widget(
            Block::default().style(Style::default().bg(Color::Black)),
            f.size(),
        );

        // 2-cell gutters on each side guarantee no text ever reaches the edge
        let area = f.size().inner(&Margin { horizontal: 2, vertical: 0 });

        if self.overlay_visible {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(10),
                    Constraint::Length(1),
                    Constraint::Length(1),
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

            let sep = Span::styled("  ·  ", Style::default().fg(BORDER));
            let key = |k: &'static str| Span::styled(k, Style::default().fg(PURPLE).add_modifier(Modifier::BOLD));
            let lbl = |l: &'static str| Span::styled(l, Style::default().fg(DIM));
            let footer = Paragraph::new(Line::from(vec![
                key("tab"), lbl(" main"), sep.clone(),
                key("p"), lbl(" plan"), sep.clone(),
                key("c"), lbl(" critic"), sep.clone(),
                key("ctrl+c"), lbl(" quit"),
            ]));
            f.render_widget(footer, chunks[2]);
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),  // title
                    Constraint::Min(1),     // message log — no border, layout is the boundary
                    Constraint::Length(3),  // input block
                    Constraint::Length(1),  // footer
                ])
                .split(area);

            // ── Title bar — brand + live token meter ─────────────────
            let mut title_spans = vec![
                Span::styled("◈  ", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                Span::styled("MowisAI", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ];
            title_spans.extend(self.token_meter.fmt_spans());
            f.render_widget(
                Paragraph::new(Line::from(title_spans)).style(Style::default().bg(Color::Black)),
                chunks[0],
            );

            // ── Message log — no visible border ───────────────────────
            self.message_log.render(f, chunks[1]);

            // ── Input block — invisible border until active ───────────
            let active = !self.input.is_empty();
            let border_col = if active { PURPLE } else { Color::Black };
            let input_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(border_col))
                .style(Style::default().bg(Color::Black));
            let input_inner = input_block.inner(chunks[2]);
            f.render_widget(input_block, chunks[2]);

            let line_count = self.input.split('\n').count();
            let char_count = self.input.chars().count();
            let avail = input_inner.width.saturating_sub(3) as usize;
            let compact = line_count > 1 || char_count > avail;

            let (input_span, text_cols) = if self.input.is_empty() {
                (Span::styled("Message  ·  / commands  ·  @ agents", Style::default().fg(DIM)), 0usize)
            } else if compact {
                let label = if line_count > 1 {
                    format!("[ {} lines · {} chars  ·  Enter ↵  Esc ✕ ]", line_count, char_count)
                } else {
                    format!("[ {} chars  ·  Enter ↵  Esc ✕ ]", char_count)
                };
                let w = label.chars().count();
                (Span::styled(label, Style::default().fg(CYAN)), w)
            } else {
                (Span::raw(self.input.as_str()), char_count)
            };

            let prompt_line = Line::from(vec![
                Span::styled("❯ ", Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
                input_span,
            ]);
            f.render_widget(
                Paragraph::new(prompt_line).style(Style::default().bg(Color::Black)),
                input_inner,
            );

            let max_col = input_inner.width.saturating_sub(1) as usize;
            let cursor_x = input_inner.x + (2 + text_cols).min(max_col) as u16;
            f.set_cursor(cursor_x, input_inner.y);

            // ── Footer hints ──────────────────────────────────────────
            let sep = Span::styled("  ·  ", Style::default().fg(BORDER));
            let key = |k: &'static str| Span::styled(k, Style::default().fg(PURPLE).add_modifier(Modifier::BOLD));
            let lbl = |l: &'static str| Span::styled(l, Style::default().fg(DIM));
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    key("tab"), lbl(" overlay"), sep.clone(),
                    key("p"), lbl(" plan"), sep.clone(),
                    key("c"), lbl(" critic"), sep.clone(),
                    key("ctrl+c"), lbl(" quit"),
                ])).style(Style::default().bg(Color::Black)),
                chunks[3],
            );

            // Popup menus render above the input block
            if self.slash_menu.visible { self.slash_menu.render(f, chunks[2]); }
            if self.at_menu.visible    { self.at_menu.render(f, chunks[2]); }
        }
    }
}
