use crate::backend::Backend;
use crate::theme::Theme;
use crate::types::{BackendEvent, ChatMessage, FileDiff, FrontendCommand, Task, TaskStatus};
use crate::views::{build::BuildView, chat::ChatView, diff::DiffView, landing::LandingView};
use crate::widgets::{show_status_bar, StatusBarState};
use egui::{CentralPanel, Frame, RichText, SidePanel, TopBottomPanel};
use std::time::{Duration, Instant};

// ── Screen ────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Screen {
    Setup,
    Landing,
    Main,
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct MowisApp {
    screen: Screen,

    // View states
    landing: LandingView,
    chat: ChatView,
    build: BuildView,
    diff: DiffView,

    // Shared data driven by backend events
    messages: Vec<ChatMessage>,
    tasks: Vec<Task>,
    diffs: Vec<FileDiff>,

    // Backend bridge
    backend: Backend,

    // Status bar
    daemon_running: bool,
    started_at: Instant,

    // Setup screen state
    setup_label: String,
    setup_pct: u8,
}

impl MowisApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Theme::apply(&cc.egui_ctx);

        let project_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".into());

        let backend = Backend::spawn(project_dir);

        Self {
            screen: Screen::Setup,
            landing: LandingView::new(),
            chat: ChatView::default(),
            build: BuildView::default(),
            diff: DiffView::new(),
            messages: Vec::new(),
            tasks: Vec::new(),
            diffs: Vec::new(),
            backend,
            daemon_running: false,
            started_at: Instant::now(),
            setup_label: "Starting AI engine…".into(),
            setup_pct: 0,
        }
    }

    // ── Event pump ────────────────────────────────────────────────────────────

    fn drain_backend_events(&mut self) {
        while let Ok(event) = self.backend.event_rx.try_recv() {
            match event {
                BackendEvent::SetupProgress(p) => {
                    use crate::types::SetupProgress;
                    let label = match p {
                        SetupProgress::Checking => {
                            self.setup_pct = 5;
                            "Checking for AI engine…".into()
                        }
                        SetupProgress::Downloading { label, pct } => {
                            self.setup_pct = pct;
                            format!("Downloading {label}… {pct}%")
                        }
                        SetupProgress::Installing { step } => {
                            self.setup_pct = 80;
                            format!("Installing: {step}")
                        }
                        SetupProgress::Starting => {
                            self.setup_pct = 90;
                            "Starting AI engine…".into()
                        }
                        SetupProgress::Ready => {
                            self.setup_pct = 100;
                            self.daemon_running = true;
                            if self.screen == Screen::Setup {
                                self.screen = Screen::Landing;
                            }
                            "AI engine ready.".into()
                        }
                        SetupProgress::Warning(w) => format!("Warning: {w}"),
                        SetupProgress::Failed(e) => format!("Setup failed: {e}"),
                    };
                    self.setup_label = label.clone();
                    self.messages.push(ChatMessage::system(label));
                }
                BackendEvent::DaemonStarted => {
                    self.daemon_running = true;
                    self.setup_pct = 100;
                    if self.screen == Screen::Setup {
                        self.screen = Screen::Landing;
                    }
                    self.messages.push(ChatMessage::system("Daemon connected."));
                }
                BackendEvent::DaemonFailed(e) => {
                    self.daemon_running = false;
                    self.messages
                        .push(ChatMessage::system(format!("Daemon error: {e}")));
                }
                BackendEvent::TaskAdded(task) => {
                    self.tasks.push(task);
                }
                BackendEvent::TaskUpdated { id, status } => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                        t.status = status;
                    }
                }
                BackendEvent::AgentChunk(chunk) => {
                    // Append to the last streaming agent message, or start a new one.
                    if let Some(msg) = self.messages.last_mut().filter(|m| m.streaming) {
                        msg.content.push_str(&chunk);
                    } else {
                        let mut msg = ChatMessage::agent_start();
                        msg.content = chunk;
                        self.messages.push(msg);
                    }
                    self.chat.scroll_to_bottom = true;
                }
                BackendEvent::AgentMessage(content) => {
                    // Finalise any open streaming bubble, then add a complete one.
                    if let Some(msg) = self.messages.last_mut().filter(|m| m.streaming) {
                        msg.streaming = false;
                    }
                    let mut msg = ChatMessage::agent_start();
                    msg.content = content;
                    msg.streaming = false;
                    self.messages.push(msg);
                    self.chat.scroll_to_bottom = true;
                }
                BackendEvent::DiffUpdated(diff) => {
                    if let Some(existing) =
                        self.diffs.iter_mut().find(|d| d.path == diff.path)
                    {
                        *existing = diff;
                    } else {
                        self.diffs.push(diff);
                    }
                }
                BackendEvent::OrchestrationComplete => {
                    // Mark any still-running tasks as complete.
                    for t in &mut self.tasks {
                        if t.status == TaskStatus::Running {
                            t.status = TaskStatus::Complete;
                        }
                    }
                    // Finalise open streaming message.
                    if let Some(msg) = self.messages.last_mut().filter(|m| m.streaming) {
                        msg.streaming = false;
                    }
                    self.messages
                        .push(ChatMessage::system("Orchestration complete."));
                }
                BackendEvent::OrchestrationFailed(e) => {
                    if let Some(msg) = self.messages.last_mut().filter(|m| m.streaming) {
                        msg.streaming = false;
                    }
                    self.messages
                        .push(ChatMessage::system(format!("Orchestration failed: {e}")));
                }
            }
        }
    }

    // ── Status bar data ───────────────────────────────────────────────────────

    fn status_bar_state(&self) -> StatusBarState {
        let active = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Running)
            .count();
        let complete = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Complete)
            .count();
        StatusBarState {
            daemon_running: self.daemon_running,
            active_agents: active,
            tasks_complete: complete,
            tasks_total: self.tasks.len(),
            elapsed_secs: self.started_at.elapsed().as_secs(),
        }
    }
}

// ── eframe::App impl ──────────────────────────────────────────────────────────

impl eframe::App for MowisApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain any events the background thread produced.
        self.drain_backend_events();

        match self.screen {
            Screen::Setup => self.render_setup(ctx),
            Screen::Landing => self.render_landing(ctx),
            Screen::Main => self.render_main(ctx),
        }
    }
}

// ── Screen renderers ──────────────────────────────────────────────────────────

impl MowisApp {
    fn render_setup(&mut self, ctx: &egui::Context) {
        // Keep the screen animating while we wait for the daemon.
        ctx.request_repaint_after(Duration::from_millis(100));

        CentralPanel::default()
            .frame(Frame::none().fill(Theme::BG_APP))
            .show(ctx, |ui| {
                let available = ui.available_size();

                // Content height estimate: title ~50 + gap ~12 + label ~20 +
                // gap ~16 + progress ~12 + gap ~16 + spinner ~20 = ~146px
                let content_h = 146.0_f32;
                let top_pad = ((available.y - content_h) * 0.5).max(40.0);

                ui.allocate_ui_with_layout(
                    available,
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space(top_pad);

                        // ── Title ─────────────────────────────────────────────
                        ui.label(
                            RichText::new("MowisAI")
                                .font(egui::FontId::proportional(42.0))
                                .color(Theme::TEXT_PRIMARY)
                                .strong(),
                        );

                        ui.add_space(12.0);

                        // ── Current setup label ───────────────────────────────
                        ui.label(
                            RichText::new(&self.setup_label)
                                .font(Theme::font_body())
                                .color(Theme::TEXT_SECONDARY),
                        );

                        ui.add_space(16.0);

                        // ── Progress bar ──────────────────────────────────────
                        let fraction = self.setup_pct as f32 / 100.0;
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(400.0)
                                .fill(Theme::ACCENT_BLUE),
                        );

                        ui.add_space(16.0);

                        // ── Spinner (animated dots) ───────────────────────────
                        // Derive a tick from wall-clock time so it animates
                        // independently of frame rate.
                        let tick = (self.started_at.elapsed().as_millis() / 400) as usize;
                        let dots = match tick % 4 {
                            0 => "   ",
                            1 => ".  ",
                            2 => ".. ",
                            _ => "...",
                        };
                        ui.label(
                            RichText::new(dots)
                                .font(egui::FontId::proportional(18.0))
                                .color(Theme::ACCENT_BLUE),
                        );
                    },
                );
            });
    }

    fn render_landing(&mut self, ctx: &egui::Context) {
        CentralPanel::default()
            .frame(Frame::none().fill(Theme::BG_APP))
            .show(ctx, |ui| {
                if let Some(prompt) =
                    crate::views::landing::show(&mut self.landing, ctx, ui)
                {
                    // User submitted — send to backend and switch screens.
                    self.messages.push(ChatMessage::user(&prompt));
                    self.chat.scroll_to_bottom = true;

                    let _ = self
                        .backend
                        .command_tx
                        .try_send(FrontendCommand::StartOrchestration { prompt });

                    self.screen = Screen::Main;
                }
            });
    }

    fn render_main(&mut self, ctx: &egui::Context) {
        // ── Status bar at the very bottom ─────────────────────────────────────
        TopBottomPanel::bottom("status_bar")
            .frame(Frame::none())
            .show(ctx, |ui| {
                show_status_bar(ui, &self.status_bar_state());
            });

        // ── Right panel: diff view ────────────────────────────────────────────
        SidePanel::right("diff_panel")
            .default_width(480.0)
            .min_width(280.0)
            .frame(Frame::none().fill(Theme::BG_PANEL))
            .show(ctx, |ui| {
                crate::views::diff::show(&mut self.diff, ui, &self.diffs);
            });

        // ── Left/center: build progress + chat ───────────────────────────────
        CentralPanel::default()
            .frame(Frame::none().fill(Theme::BG_PANEL))
            .show(ctx, |ui| {
                // Build progress takes a fixed slice at the top when tasks exist.
                if !self.tasks.is_empty() {
                    let build_height = if self.build.expanded { 220.0 } else { 52.0 };
                    let (build_rect, chat_rect) = {
                        let full = ui.available_rect_before_wrap();
                        let split = full.top() + build_height;
                        let build = egui::Rect::from_min_max(full.min, egui::pos2(full.right(), split));
                        let chat = egui::Rect::from_min_max(egui::pos2(full.left(), split), full.max);
                        (build, chat)
                    };

                    ui.allocate_ui_at_rect(build_rect, |ui| {
                        crate::views::build::show(&mut self.build, ui, &self.tasks);
                    });

                    ui.allocate_ui_at_rect(chat_rect, |ui| {
                        if let Some(text) =
                            crate::views::chat::show(&mut self.chat, ctx, ui, &self.messages)
                        {
                            self.messages.push(ChatMessage::user(&text));
                            let _ = self
                                .backend
                                .command_tx
                                .try_send(FrontendCommand::SendFollowUp { content: text });
                        }
                    });
                } else {
                    // No tasks yet — full height for chat.
                    if let Some(text) =
                        crate::views::chat::show(&mut self.chat, ctx, ui, &self.messages)
                    {
                        self.messages.push(ChatMessage::user(&text));
                        let _ = self
                            .backend
                            .command_tx
                            .try_send(FrontendCommand::SendFollowUp { content: text });
                    }
                }
            });
    }
}
