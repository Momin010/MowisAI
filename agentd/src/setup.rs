use crate::config::{AiProvider, MowisConfig};
use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor, Stylize},
    terminal::{self, ClearType},
};
use std::io::{self, Write as _};

// ── Grok models presented to the user ────────────────────────────────────────

struct GrokModel {
    id: &'static str,
    label: &'static str,
    note: &'static str,
}

const GROK_MODELS: &[GrokModel] = &[
    GrokModel { id: "grok-3",           label: "grok-3",           note: "Flagship — best quality"        },
    GrokModel { id: "grok-3-fast",      label: "grok-3-fast",      note: "Faster, lower latency"          },
    GrokModel { id: "grok-3-mini",      label: "grok-3-mini",      note: "Lightweight, cost-efficient"    },
    GrokModel { id: "grok-3-mini-fast", label: "grok-3-mini-fast", note: "Lightweight + fast"             },
    GrokModel { id: "grok-2-1212",      label: "grok-2-1212",      note: "Previous generation"            },
    GrokModel { id: "grok-2-vision-1212", label: "grok-2-vision-1212", note: "Vision capabilities"       },
];

// ── Public API ────────────────────────────────────────────────────────────────

pub struct SetupWizard;

impl SetupWizard {
    pub fn needs_setup() -> bool {
        match MowisConfig::load() {
            Ok(Some(config)) => !config.is_valid(),
            _ => true,
        }
    }

    pub fn run() -> Result<MowisConfig> {
        let mut stdout = io::stdout();
        clear_screen(&mut stdout)?;
        print_banner(&mut stdout)?;
        stdout.flush()?;

        let provider = pick_provider(&mut stdout)?;

        let config = match provider {
            AiProvider::VertexAi => setup_vertex(&mut stdout)?,
            AiProvider::Grok     => setup_grok(&mut stdout)?,
        };

        config.save()?;

        clear_screen(&mut stdout)?;
        println!();
        println!("  \u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2557}");
        println!("  \u{2551}       MowisAI \u{2014} Setup Complete!       \u{2551}");
        println!("  \u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}");
        println!();
        println!("  Provider : {}", config.provider);
        println!("  Model    : {}", config.model);
        println!();
        println!("  \u{2713} Config saved to ~/.mowisai/config.toml (owner-only, 600)");
        println!("  \u{2713} API key encrypted with AES-256-GCM (machine-bound key)");
        println!();
        println!("  Launching MowisAI...");
        println!();
        stdout.flush()?;

        Ok(config)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn clear_screen(stdout: &mut impl Write) -> Result<()>
where
    io::Stdout: Write,
{
    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    Ok(())
}

fn print_banner(stdout: &mut impl Write) -> Result<()>
where
    io::Stdout: Write,
{
    queue!(
        stdout,
        SetForegroundColor(Color::Cyan),
        Print("  \u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2557}\n"),
        Print("  \u{2551}        MowisAI \u{2014} First Run Setup      \u{2551}\n"),
        Print("  \u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}\n"),
        ResetColor,
    )?;
    Ok(())
}

// ── Provider selection ────────────────────────────────────────────────────────

fn pick_provider(stdout: &mut io::Stdout) -> Result<AiProvider> {
    let options = [
        ("Vertex AI", "Google Cloud — requires gcloud CLI + auth"),
        ("Grok AI",   "xAI — requires an API key from console.x.ai"),
    ];
    let mut cursor_idx: usize = 0;

    terminal::enable_raw_mode()?;
    let result = (|| -> Result<AiProvider> {
        loop {
            // Redraw menu
            queue!(stdout, cursor::MoveTo(0, 4))?;
            queue!(
                stdout,
                terminal::Clear(ClearType::FromCursorDown),
                SetForegroundColor(Color::White),
                Print("  Choose your AI provider:\n\n"),
                ResetColor,
            )?;

            for (i, (name, desc)) in options.iter().enumerate() {
                if i == cursor_idx {
                    queue!(
                        stdout,
                        SetForegroundColor(Color::Green),
                        Print(format!("  \u{25ba} {:<18}", name)),
                        SetForegroundColor(Color::DarkGrey),
                        Print(format!("{}\n", desc)),
                        ResetColor,
                    )?;
                } else {
                    queue!(
                        stdout,
                        Print(format!("    {:<18}", name)),
                        SetForegroundColor(Color::DarkGrey),
                        Print(format!("{}\n", desc)),
                        ResetColor,
                    )?;
                }
            }

            queue!(
                stdout,
                Print("\n"),
                SetForegroundColor(Color::DarkGrey),
                Print("  \u{2191}/\u{2193} navigate   Enter select\n"),
                ResetColor,
            )?;
            stdout.flush()?;

            match event::read()? {
                Event::Key(KeyEvent { code: KeyCode::Up, .. }) => {
                    if cursor_idx > 0 { cursor_idx -= 1; }
                }
                Event::Key(KeyEvent { code: KeyCode::Down, .. }) => {
                    if cursor_idx < options.len() - 1 { cursor_idx += 1; }
                }
                Event::Key(KeyEvent { code: KeyCode::Enter, .. }) => break,
                Event::Key(KeyEvent { code: KeyCode::Char('c'), modifiers, .. })
                    if modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    terminal::disable_raw_mode()?;
                    anyhow::bail!("Setup cancelled");
                }
                _ => {}
            }
        }
        Ok(if cursor_idx == 0 { AiProvider::VertexAi } else { AiProvider::Grok })
    })();
    terminal::disable_raw_mode()?;
    result
}

// ── Vertex AI setup (existing gcloud flow) ────────────────────────────────────

fn setup_vertex(stdout: &mut io::Stdout) -> Result<MowisConfig> {
    use std::process::Command;

    println!();
    println!("  \u{25ba} Vertex AI (Google Cloud) setup");
    println!();

    // Check gcloud
    print!("  Checking gcloud CLI... ");
    stdout.flush()?;
    match Command::new("gcloud").arg("--version").output() {
        Ok(o) if o.status.success() => println!("\u{2713} found"),
        _ => {
            println!("\u{2717} not found");
            println!();
            println!("  gcloud CLI is required. Install: https://cloud.google.com/sdk/docs/install");
            anyhow::bail!("gcloud CLI not found");
        }
    }

    // Check auth
    print!("  Checking gcloud auth... ");
    stdout.flush()?;
    match Command::new("gcloud").args(["auth", "print-access-token"]).output() {
        Ok(o) if o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty() => {
            println!("\u{2713} authenticated");
        }
        _ => {
            println!("\u{2717} not authenticated");
            println!();
            println!("  Run: gcloud auth login");
            println!("  Then: gcloud auth application-default login");
            anyhow::bail!("gcloud auth failed");
        }
    }

    // Detect project
    let auto_project = Command::new("gcloud")
        .args(["config", "get-value", "project"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !s.is_empty() && s != "(unset)" { Some(s) } else { None }
            } else {
                None
            }
        });

    let project_id = if let Some(ref p) = auto_project {
        print!("  GCP Project detected: {} \u{2014} use this? [Y/n] ", p);
        stdout.flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let answer = input.trim().to_lowercase();
        if answer.is_empty() || answer == "y" || answer == "yes" {
            p.clone()
        } else {
            prompt_project_id(stdout)?
        }
    } else {
        prompt_project_id(stdout)?
    };

    Ok(MowisConfig {
        provider: AiProvider::VertexAi,
        gcp_project_id: project_id,
        model: "gemini-2.5-pro".into(),
        ..MowisConfig::default()
    })
}

fn prompt_project_id(stdout: &mut io::Stdout) -> Result<String> {
    print!("  Enter your GCP Project ID: ");
    stdout.flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let project = input.trim().to_string();
    if project.is_empty() {
        anyhow::bail!("GCP Project ID is required");
    }
    Ok(project)
}

// ── Grok AI setup ─────────────────────────────────────────────────────────────

fn setup_grok(stdout: &mut io::Stdout) -> Result<MowisConfig> {
    clear_screen(stdout)?;
    print_banner(stdout)?;

    queue!(
        stdout,
        SetForegroundColor(Color::Yellow),
        Print("  \u{25ba} Grok AI (xAI) setup\n\n"),
        ResetColor,
    )?;
    stdout.flush()?;

    let api_key = read_masked_api_key(stdout)?;

    // Validate key looks plausible (xAI keys start with "xai-")
    if api_key.len() < 8 {
        anyhow::bail!("API key too short — please check your key from console.x.ai");
    }

    let model = pick_grok_model(stdout)?;

    let encrypted = crate::crypto::encrypt(&api_key)?;

    Ok(MowisConfig {
        provider: AiProvider::Grok,
        grok_api_key_enc: Some(encrypted),
        grok_model: model.clone(),
        model,
        ..MowisConfig::default()
    })
}

// ── Masked API key input ──────────────────────────────────────────────────────

fn read_masked_api_key(stdout: &mut io::Stdout) -> Result<String> {
    queue!(
        stdout,
        SetForegroundColor(Color::White),
        Print("  Paste your xAI API key (from console.x.ai):\n"),
        ResetColor,
        SetForegroundColor(Color::DarkGrey),
        Print("  The key will be encrypted with AES-256-GCM and stored locally.\n\n"),
        ResetColor,
        Print("  > "),
    )?;
    stdout.flush()?;

    let mut key_buf = String::new();

    terminal::enable_raw_mode()?;
    let result = (|| -> Result<String> {
        loop {
            match event::read()? {
                Event::Key(KeyEvent { code: KeyCode::Enter, .. }) => {
                    if !key_buf.is_empty() {
                        break;
                    }
                }
                Event::Key(KeyEvent { code: KeyCode::Backspace, .. }) => {
                    if key_buf.pop().is_some() {
                        // Erase one asterisk
                        queue!(
                            stdout,
                            cursor::MoveLeft(1),
                            Print(" "),
                            cursor::MoveLeft(1),
                        )?;
                        stdout.flush()?;
                    }
                }
                Event::Key(KeyEvent { code: KeyCode::Char(c), modifiers, .. }) => {
                    if modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                        terminal::disable_raw_mode()?;
                        anyhow::bail!("Setup cancelled");
                    }
                    key_buf.push(c);
                    queue!(stdout, Print("\u{2022}"))?; // bullet instead of asterisk
                    stdout.flush()?;
                }
                _ => {}
            }
        }
        Ok(key_buf)
    })();
    terminal::disable_raw_mode()?;

    println!(); // newline after the masked input line
    println!();

    match result {
        Ok(k) => Ok(k.trim().to_string()),
        Err(e) => Err(e),
    }
}

// ── Grok model picker (spacebar multi-select, at least one required) ──────────

fn pick_grok_model(stdout: &mut io::Stdout) -> Result<String> {
    let mut selected: Vec<bool> = vec![true, false, false, false, false, false]; // grok-3 pre-selected
    let mut cursor_idx: usize = 0;

    queue!(
        stdout,
        SetForegroundColor(Color::White),
        Print("  Choose a Grok model:\n"),
        ResetColor,
        SetForegroundColor(Color::DarkGrey),
        Print("  Space to toggle, \u{2191}/\u{2193} navigate, Enter to confirm (select at least one).\n\n"),
        ResetColor,
    )?;
    stdout.flush()?;

    let menu_top_row = {
        let pos = cursor::position()?;
        pos.1
    };

    terminal::enable_raw_mode()?;
    let result = (|| -> Result<String> {
        loop {
            // Redraw model list
            execute!(stdout, cursor::MoveTo(0, menu_top_row))?;
            for (i, model) in GROK_MODELS.iter().enumerate() {
                let check = if selected[i] { "\u{25c9}" } else { "\u{25cb}" }; // ◉ / ○
                let row_color = if i == cursor_idx { Color::Green } else { Color::Reset };
                let cursor_arrow = if i == cursor_idx { "\u{25ba}" } else { " " };

                queue!(
                    stdout,
                    terminal::Clear(ClearType::CurrentLine),
                    SetForegroundColor(Color::DarkGrey),
                    Print(format!("  {} ", cursor_arrow)),
                    SetForegroundColor(if selected[i] { Color::Green } else { Color::DarkGrey }),
                    Print(format!("{} ", check)),
                    SetForegroundColor(row_color),
                    Print(format!("{:<22}", model.label)),
                    SetForegroundColor(Color::DarkGrey),
                    Print(format!("  {}\n", model.note)),
                    ResetColor,
                )?;
            }

            let any_selected = selected.iter().any(|&s| s);
            let hint = if any_selected {
                "  Press Enter to confirm"
            } else {
                "  Select at least one model to continue"
            };
            queue!(
                stdout,
                terminal::Clear(ClearType::CurrentLine),
                Print("\n"),
                SetForegroundColor(if any_selected { Color::DarkGrey } else { Color::Red }),
                Print(hint),
                Print("\n"),
                ResetColor,
            )?;
            stdout.flush()?;

            match event::read()? {
                Event::Key(KeyEvent { code: KeyCode::Up, .. }) => {
                    if cursor_idx > 0 { cursor_idx -= 1; }
                }
                Event::Key(KeyEvent { code: KeyCode::Down, .. }) => {
                    if cursor_idx < GROK_MODELS.len() - 1 { cursor_idx += 1; }
                }
                Event::Key(KeyEvent { code: KeyCode::Char(' '), .. }) => {
                    selected[cursor_idx] = !selected[cursor_idx];
                }
                Event::Key(KeyEvent { code: KeyCode::Enter, .. }) => {
                    if any_selected {
                        break;
                    }
                }
                Event::Key(KeyEvent { code: KeyCode::Char('c'), modifiers, .. })
                    if modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    terminal::disable_raw_mode()?;
                    anyhow::bail!("Setup cancelled");
                }
                _ => {}
            }
        }

        // Return the first selected model as the active model.
        let primary = GROK_MODELS
            .iter()
            .zip(selected.iter())
            .find(|(_, &s)| s)
            .map(|(m, _)| m.id.to_string())
            .unwrap_or_else(|| "grok-3".to_string());

        Ok(primary)
    })();
    terminal::disable_raw_mode()?;

    println!();
    result
}

// ── Trait alias so helpers accept &mut io::Stdout without generic bounds ──────

trait Write: io::Write + crossterm::QueueableCommand {}
impl Write for io::Stdout {}
