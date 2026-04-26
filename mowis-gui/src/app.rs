use crate::backend::Backend;
use crate::theme::Theme;
use crate::types::{BackendEvent, ChatMessage, FileDiff, FrontendCommand, Task, TaskStatus};
use crate::views::{build::BuildView, chat::ChatView, diff::DiffView, landing::LandingView};
use crate::widgets::{show_status_bar, StatusBarState};
use egui::{CentralPanel, Frame, SidePanel, TopBottomPanel};
use std::time::Instant;

// ── Screen ────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Screen {
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
}

impl MowisApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Theme::apply(&cc.egui_ctx);

        let project_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".into());

        let backend = Backend::spawn(project_dir);

        Self {
            screen: Screen::Landing,
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
        }
    }

    // ── Event pump ────────────────────────────────────────────────────────────

    fn drain_backend_events(&mut self) {
        while let Ok(event) = self.backend.event_rx.try_recv() {
            match event {
                BackendEvent::DaemonStarted => {
                    self.daemon_running = true;
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
            Screen::Landing => self.render_landing(ctx),
            Screen::Main => self.render_main(ctx),
        }
    }
}

// ── Screen renderers ──────────────────────────────────────────────────────────

impl MowisApp {
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
