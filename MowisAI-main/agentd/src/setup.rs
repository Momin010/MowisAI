use crate::config::MowisConfig;
use anyhow::Result;
use std::io::{self, Write};
use std::process::Command;

pub struct SetupWizard;

impl SetupWizard {
    pub fn needs_setup() -> bool {
        match MowisConfig::load() {
            Ok(Some(config)) => !config.is_valid(),
            _ => true,
        }
    }

    pub fn run() -> Result<MowisConfig> {
        println!();
        println!("  \u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2557}");
        println!("  \u{2551}        MowisAI \u{2014} First Run Setup     \u{2551}");
        println!("  \u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}");
        println!();

        Self::check_gcloud()?;
        Self::check_gcloud_auth()?;
        let project_id = Self::get_project_id()?;

        let config = MowisConfig {
            gcp_project_id: project_id,
            ..MowisConfig::default()
        };

        config.save()?;
        println!();
        println!("  \u{2713} Configuration saved to ~/.mowisai/config.toml");
        println!();

        Ok(config)
    }

    fn check_gcloud() -> Result<()> {
        print!("  Checking gcloud CLI... ");
        io::stdout().flush()?;
        match Command::new("gcloud").arg("--version").output() {
            Ok(output) if output.status.success() => {
                println!("\u{2713} found");
                Ok(())
            }
            _ => {
                println!("\u{2717} not found");
                println!();
                println!("  gcloud CLI is required. Install it:");
                println!("  \u{2192} https://cloud.google.com/sdk/docs/install");
                println!();
                anyhow::bail!("gcloud CLI not found. Install it and try again.");
            }
        }
    }

    fn check_gcloud_auth() -> Result<()> {
        print!("  Checking gcloud auth... ");
        io::stdout().flush()?;
        match Command::new("gcloud")
            .args(["auth", "print-access-token"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let token = String::from_utf8_lossy(&output.stdout);
                if token.trim().is_empty() {
                    println!("\u{2717} no token");
                    println!();
                    println!("  Run: gcloud auth login");
                    anyhow::bail!("Not authenticated with gcloud");
                }
                println!("\u{2713} authenticated");
                Ok(())
            }
            _ => {
                println!("\u{2717} failed");
                println!();
                println!("  Run: gcloud auth login");
                println!("  Then: gcloud auth application-default login");
                anyhow::bail!("gcloud auth failed");
            }
        }
    }

    fn get_project_id() -> Result<String> {
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

        if let Some(ref project) = auto_project {
            print!("  GCP Project detected: {} \u{2014} use this? [Y/n] ", project);
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let answer = input.trim().to_lowercase();
            if answer.is_empty() || answer == "y" || answer == "yes" {
                return Ok(project.clone());
            }
        }

        print!("  Enter your GCP Project ID: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let project = input.trim().to_string();
        if project.is_empty() {
            anyhow::bail!("GCP Project ID is required");
        }
        Ok(project)
    }
}
