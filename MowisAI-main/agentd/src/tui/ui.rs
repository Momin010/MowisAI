use super::app::{AgentState, App, AppView, SandboxState};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap,
    },
    Frame,
};

const TITLE_STYLE: Style = Style::new().add_modifier(Modifier::BOLD);
const SELECTED_STYLE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const DIM_STYLE: Style = Style::new().fg(Color::DarkGray);

pub fn draw(f: &mut Frame, app: &App) {
    match &app.view {
        AppView::Overview => draw_overview(f, app),
        AppView::SandboxDetail(name) => draw_sandbox_detail(f, app, name.clone()),
        AppView::AgentDetail(id) => draw_agent_detail(f, app, id.clone()),
        AppView::CommandInput => {
            draw_overview(f, app);
            draw_command_popup(f, app);
        }
        AppView::ErrorLog => draw_error_log(f, app),
    }
}

fn draw_overview(f: &mut Frame, app: &App) {
    let area = f.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Title bar
            Constraint::Length(3),  // Progress bar
            Constraint::Length(3),  // Stats line
            Constraint::Min(8),     // Sandboxes + activity
            Constraint::Length(1),  // Help bar
        ])
        .split(area);

    // Title bar
    let done_indicator = if app.orchestrator_done { " ✓ DONE" } else { "" };
    let title_text = format!(
        " MowisAI Orchestrator — {} tasks, {} agents{}",
        app.stats.total_tasks, app.stats.agents_spawned, done_indicator
    );
    let title = Paragraph::new(title_text)
        .style(TITLE_STYLE)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Progress bar
    let pct = app.progress_percent();
    let progress_label = format!(
        "{}% ({}/{} tasks)",
        pct, app.stats.completed, app.stats.total_tasks
    );
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progress"))
        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
        .percent(pct)
        .label(progress_label);
    f.render_widget(gauge, chunks[1]);

    // Stats line
    let stats_text = format!(
        " Elapsed: {}s | Agents: {} active | Completed: {} | Failed: {} | Pending: {}",
        app.stats.elapsed_secs,
        app.stats.running,
        app.stats.completed,
        app.stats.failed,
        app.stats.pending,
    );
    let stats = Paragraph::new(stats_text)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(stats, chunks[2]);

    // Middle area: sandboxes left, activity right
    let mid_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[3]);

    // Sandbox list
    draw_sandbox_list(f, app, mid_chunks[0]);

    // Activity log
    draw_activity_log(f, app, mid_chunks[1]);

    // Help bar
    let help = if app.orchestrator_done {
        " [q] Quit  [/] Command  [e] Errors"
    } else {
        " [Tab] Switch  [Enter] Drill in  [↑↓] Navigate  [q] Quit  [/] Command  [e] Errors"
    };
    let help_widget = Paragraph::new(help)
        .style(DIM_STYLE)
        .alignment(Alignment::Left);
    f.render_widget(help_widget, chunks[4]);
}

fn draw_sandbox_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .sandboxes
        .iter()
        .enumerate()
        .map(|(i, sb)| {
            let status_sym = match sb.status.as_str() {
                "active" => "●",
                "idle" => "○",
                "done" => "✓",
                _ => "·",
            };
            let status_color = match sb.status.as_str() {
                "active" => Color::Green,
                "idle" => Color::Yellow,
                "done" => Color::Cyan,
                _ => Color::Gray,
            };
            let prefix = if i == app.selected_index { "► " } else { "  " };
            let line = Line::from(vec![
                Span::raw(prefix),
                Span::styled(status_sym, Style::default().fg(status_color)),
                Span::raw(format!(
                    " {} [{} agents] {}/{} tasks",
                    sb.name, sb.active_agents, sb.completed_tasks,
                    sb.completed_tasks + sb.failed_tasks + sb.active_agents
                )),
            ]);
            let style = if i == app.selected_index {
                SELECTED_STYLE
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Sandboxes"),
    );
    f.render_widget(list, area);
}

fn draw_activity_log(f: &mut Frame, app: &App, area: Rect) {
    let max_lines = area.height.saturating_sub(2) as usize;
    let start = app.activity_log.len().saturating_sub(max_lines);
    let items: Vec<ListItem> = app.activity_log[start..]
        .iter()
        .map(|line| ListItem::new(line.as_str()))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Recent Activity"),
    );
    f.render_widget(list, area);
}

fn draw_sandbox_detail(f: &mut Frame, app: &App, sandbox_name: String) {
    let area = f.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(area);

    // Title
    let active = app.agents_for_sandbox(&sandbox_name).len();
    let title_text = format!(" {} Sandbox — {} Active Agents", sandbox_name, active);
    let title = Paragraph::new(title_text)
        .style(TITLE_STYLE)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // Agent list
    let agents = app.agents_for_sandbox(&sandbox_name);
    let items: Vec<ListItem> = agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let status_sym = match agent.status.as_str() {
                "thinking" => "⏵",
                "executing_tool" => "⚙",
                "completed" => "✓",
                "failed" => "✗",
                _ => "·",
            };
            let status_color = match agent.status.as_str() {
                "thinking" => Color::Blue,
                "executing_tool" => Color::Yellow,
                "completed" => Color::Green,
                "failed" => Color::Red,
                _ => Color::Gray,
            };
            let prefix = if i == app.selected_index { "► " } else { "  " };
            let tool_line = if let Some(ref tool) = agent.current_tool {
                format!("  └─ {} (running)", tool)
            } else if let Some(last_tool) = agent.tool_history.last() {
                format!("  └─ {} ({})", last_tool.tool_name, if last_tool.success { "done" } else { "failed" })
            } else {
                String::new()
            };

            let mut lines = vec![Line::from(vec![
                Span::raw(prefix),
                Span::styled(status_sym, Style::default().fg(status_color)),
                Span::raw(format!(
                    " Agent {}  {} [{}s]",
                    agent.id, agent.task_description.chars().take(30).collect::<String>(),
                    agent.elapsed_secs
                )),
            ])];
            if !tool_line.is_empty() {
                lines.push(Line::from(Span::styled(
                    tool_line,
                    Style::default().fg(Color::DarkGray),
                )));
            }

            let style = if i == app.selected_index {
                SELECTED_STYLE
            } else {
                Style::default()
            };
            ListItem::new(Text::from(lines)).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Agents in {}", sandbox_name)),
    );
    f.render_widget(list, chunks[1]);

    // Help
    let help = Paragraph::new(" [Enter] View agent  [Esc] Back  [↑↓] Navigate  [q] Quit")
        .style(DIM_STYLE);
    f.render_widget(help, chunks[2]);
}

fn draw_agent_detail(f: &mut Frame, app: &App, agent_id: String) {
    let area = f.size();

    let agent = app.agents.iter().find(|a| a.id == agent_id);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Min(6),
            Constraint::Length(1),
        ])
        .split(area);

    if let Some(agent) = agent {
        // Title
        let title_text = format!(" Agent {} — {}", agent.id, agent.task_description);
        let title = Paragraph::new(title_text)
            .style(TITLE_STYLE)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(title, chunks[0]);

        // Status line
        let status_text = format!(
            " Status: {} | Elapsed: {}s | Diff: {} bytes",
            agent.status, agent.elapsed_secs, agent.diff_size
        );
        let status = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(status, chunks[1]);

        // Tool history
        let tool_items: Vec<ListItem> = agent
            .tool_history
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let sym = if entry.success { "✓" } else { "✗" };
                let color = if entry.success { Color::Green } else { Color::Red };
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{}. ", i + 1)),
                    Span::styled(sym, Style::default().fg(color)),
                    Span::raw(format!(" {}  {}", entry.tool_name, entry.preview)),
                ]))
            })
            .collect();

        let tools_list = List::new(tool_items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Tool History"),
        );
        f.render_widget(tools_list, chunks[2]);

        // Last tool preview / diff info
        let diff_text = if agent.diff_size > 0 {
            format!("Diff size: {} bytes\n\nTask: {}", agent.diff_size, agent.task_description)
        } else if let Some(last) = agent.tool_history.last() {
            format!("Last: {}\n{}", last.tool_name, last.preview)
        } else {
            format!("Task: {}\nStatus: {}", agent.task_description, agent.status)
        };
        let diff = Paragraph::new(diff_text)
            .block(Block::default().borders(Borders::ALL).title("Current Diff"))
            .wrap(Wrap { trim: true });
        f.render_widget(diff, chunks[3]);
    } else {
        let msg = Paragraph::new("Agent not found")
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(msg, chunks[0]);
    }

    // Help
    let help = Paragraph::new(" [Esc] Back  [↑↓] Scroll  [q] Quit").style(DIM_STYLE);
    f.render_widget(help, chunks[4]);
}

fn draw_command_popup(f: &mut Frame, app: &App) {
    let area = f.size();
    // Center a small popup
    let popup_area = centered_rect(60, 20, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Command Input (Enter: submit, Esc: cancel) ");
    f.render_widget(Clear, popup_area);

    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);

    let input_text = format!("/ {}_", app.current_command);
    let input = Paragraph::new(input_text).style(Style::default().fg(Color::Yellow));
    f.render_widget(input, inner);
}

fn draw_error_log(f: &mut Frame, app: &App) {
    let area = f.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(1)])
        .split(area);

    let title = Paragraph::new(format!(" Error Log ({} errors)", app.errors.len()))
        .style(TITLE_STYLE)
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let start = app.scroll_offset.min(app.errors.len().saturating_sub(1));
    let items: Vec<ListItem> = app.errors[start..]
        .iter()
        .map(|e| ListItem::new(e.as_str()).style(Style::default().fg(Color::Red)))
        .collect();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Errors"));
    f.render_widget(list, chunks[1]);

    let help = Paragraph::new(" [Esc/q] Back  [↑↓] Scroll").style(DIM_STYLE);
    f.render_widget(help, chunks[2]);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
