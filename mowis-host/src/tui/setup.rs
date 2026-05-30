use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use mowis_orchestration::config::{ModelRef, OrchConfig, ProviderCreds};
use mowis_orchestration::plan::Tier;
use mowis_orchestration::providers::Provider;

const PURPLE: Color = Color::Rgb(109, 40, 217);
const DIM: Color = Color::Rgb(102, 102, 102);
const BG_PANEL: Color = Color::Rgb(13, 13, 26);

const PROVIDERS: &[(&str, &str, Provider)] = &[
    ("anthropic", "Anthropic (Claude)", Provider::Anthropic),
    ("openai", "OpenAI (GPT)", Provider::OpenAi),
    ("gemini", "Google Gemini", Provider::Gemini),
    ("vertex_ai", "Vertex AI (GCP)", Provider::VertexAi),
    ("grok", "Grok (xAI)", Provider::Grok),
    ("groq", "Groq", Provider::Groq),
    ("mimo", "Mimo (Xiaomi)", Provider::Mimo),
];

pub struct SetupState {
    pub step: u8,
    pub selected: usize,
    pub provider_id: String,
    pub provider_name: String,
    pub provider: Provider,
    pub api_key: String,
}

impl SetupState {
    pub fn new() -> Self {
        Self {
            step: 1,
            selected: 0,
            provider_id: String::new(),
            provider_name: String::new(),
            provider: Provider::Anthropic,
            api_key: String::new(),
        }
    }

    pub fn move_up(&mut self) {
        if self.step == 1 {
            self.selected = self.selected.wrapping_sub(1) % PROVIDERS.len();
        }
    }

    pub fn move_down(&mut self) {
        if self.step == 1 {
            self.selected = (self.selected + 1) % PROVIDERS.len();
        }
    }

    pub fn advance_to_step2(&mut self) {
        let (id, name, provider) = &PROVIDERS[self.selected];
        self.provider_id = id.to_string();
        self.provider_name = name.to_string();
        self.provider = provider.clone();
        self.step = 2;
    }

    pub fn save_config(&self) -> anyhow::Result<OrchConfig> {
        use std::collections::HashMap;

        let encrypted = if self.provider == Provider::VertexAi {
            None
        } else {
            Some(mowis_orchestration::crypto::encrypt(&self.api_key)?)
        };

        let project_id = if self.provider == Provider::VertexAi {
            Some(self.api_key.clone()) // For Vertex, api_key field holds project_id
        } else {
            None
        };

        let mut providers = HashMap::new();
        providers.insert(
            self.provider.clone(),
            ProviderCreds {
                api_key_enc: encrypted,
                project_id,
            },
        );

        let default_model = match self.provider {
            Provider::Anthropic => "claude-sonnet-4-20250514",
            Provider::OpenAi => "gpt-4o",
            Provider::Gemini => "gemini-2.5-pro",
            Provider::VertexAi => "gemini-2.5-pro",
            Provider::Grok => "grok-3",
            Provider::Groq => "llama-3.3-70b-versatile",
            Provider::Mimo => "mimo-v2.5-pro",
        };

        let model_ref = ModelRef {
            provider: self.provider.clone(),
            model: default_model.to_string(),
        };

        let mut tiers = HashMap::new();
        tiers.insert(Tier::Conductor, model_ref.clone());
        tiers.insert(Tier::Critic, model_ref.clone());
        tiers.insert(Tier::Captain, model_ref.clone());
        tiers.insert(Tier::Crew, model_ref);

        let cfg = OrchConfig {
            providers,
            tiers,
            sandbox: mowis_orchestration::plan::SandboxConfig::default(),
            plans_dir: std::path::PathBuf::from(".mowis/plans"),
        };

        cfg.save()?;
        Ok(cfg)
    }

    pub fn draw(&self, f: &mut Frame) {
        let area = f.size();

        // Center box
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Max(4),
                Constraint::Length(20),
                Constraint::Max(4),
            ])
            .split(area);

        let inner = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Max(5),
                Constraint::Length(60),
                Constraint::Max(5),
            ])
            .split(outer[1]);

        // Clear background
        f.render_widget(Clear, inner[1]);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PURPLE))
            .style(Style::default().bg(BG_PANEL));

        let inner_area = block.inner(inner[1]);
        f.render_widget(block, inner[1]);

        if self.step == 1 {
            self.render_step1(f, inner_area);
        } else {
            self.render_step2(f, inner_area);
        }
    }

    fn render_step1(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // title
                Constraint::Length(1), // step indicator
                Constraint::Length(1), // label
                Constraint::Length(1), // spacer
                Constraint::Length(PROVIDERS.len() as u16 + 2), // provider list
                Constraint::Length(1), // spacer
                Constraint::Length(1), // hint
            ])
            .split(area);

        // Title
        let title = Paragraph::new(Line::from(Span::styled(
            "MowisAI Setup",
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        )));
        f.render_widget(title, chunks[0]);

        // Step indicator
        let step = Paragraph::new(Line::from(Span::styled(
            "Step 1 of 2",
            Style::default().fg(PURPLE),
        )));
        f.render_widget(step, chunks[1]);

        // Label
        let label = Paragraph::new(Line::from("Select your AI provider:"));
        f.render_widget(label, chunks[2]);

        // Provider list
        let list_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(42, 42, 74)))
            .style(Style::default().bg(Color::Rgb(8, 8, 12)));

        let list_area = list_block.inner(chunks[4]);
        f.render_widget(list_block, chunks[4]);

        let mut list_lines = Vec::new();
        for (i, (_id, name, _provider)) in PROVIDERS.iter().enumerate() {
            let style = if i == self.selected {
                Style::default().bg(PURPLE).fg(Color::White)
            } else {
                Style::default()
            };
            list_lines.push(Line::from(Span::styled(format!("  {}", name), style)));
        }
        let list = Paragraph::new(list_lines);
        f.render_widget(list, list_area);

        // Hint
        let hint = Paragraph::new(Line::from(Span::styled(
            "Press Enter to select",
            Style::default().fg(DIM),
        )));
        f.render_widget(hint, chunks[6]);
    }

    fn render_step2(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // title
                Constraint::Length(1), // step indicator
                Constraint::Length(1), // provider
                Constraint::Length(1), // spacer
                Constraint::Length(1), // label
                Constraint::Length(3), // input field with border
                Constraint::Length(1), // spacer
                Constraint::Length(1), // hint
            ])
            .split(area);

        let title = Paragraph::new(Line::from(Span::styled(
            "MowisAI Setup",
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        )));
        f.render_widget(title, chunks[0]);

        let step = Paragraph::new(Line::from(Span::styled(
            "Step 2 of 2",
            Style::default().fg(PURPLE),
        )));
        f.render_widget(step, chunks[1]);

        let provider = Paragraph::new(Line::from(vec![
            Span::raw("Provider: "),
            Span::styled(&self.provider_name, Style::default().fg(PURPLE).add_modifier(Modifier::BOLD)),
        ]));
        f.render_widget(provider, chunks[2]);

        let label = Paragraph::new(Line::from("Enter your API key:"));
        f.render_widget(label, chunks[4]);

        // API key input with border, masked characters, and cursor
        let masked = "*".repeat(self.api_key.len());
        let input_text = format!("{}▌", masked); // cursor character
        let input_block = Block::default()
            .title(" API Key ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(PURPLE))
            .style(Style::default().bg(Color::Rgb(8, 8, 12)));
        let input = Paragraph::new(Line::from(Span::styled(
            input_text,
            Style::default().fg(Color::White),
        )))
        .block(input_block);
        f.render_widget(input, chunks[5]);

        let hint = Paragraph::new(Line::from(Span::styled(
            "Press Enter to continue",
            Style::default().fg(DIM),
        )));
        f.render_widget(hint, chunks[7]);
    }
}
