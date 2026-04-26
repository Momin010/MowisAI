//! Windows platform backend for MowisAI.
//!
//! Strategy (tried in order):
//!   1. WSL2  — run agentd inside a custom "MowisAI" Alpine distro, bridged to
//!              TCP 9722 via socat inside WSL2.
//!   2. QEMU  — fallback when WSL2 is not available; uses WHPX accelerator when
//!              present, falls back to TCG software emulation.
//!
//! The GUI always connects via TCP 127.0.0.1:9722 regardless of mode.
//!
//! This file is only compiled on Windows (`cfg(target_os = "windows")`).

#![cfg(target_os = "windows")]

use super::{ConnectionTarget, DaemonPlatform};
use crate::types::SetupProgress;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

// ── Constants ─────────────────────────────────────────────────────────────────

const VM_PORT: u16 = 9722;

const WSL_DISTRO_NAME: &str = "MowisAI";

const ALPINE_TAR_URL: &str = "https://releases.mowisai.com/agentd-alpine-v1.0.tar";
const ALPINE_QCOW2_URL: &str = "https://releases.mowisai.com/agentd-alpine-v1.0.qcow2";

/// Timeout for waiting for the daemon to become reachable after launch.
const BOOT_TIMEOUT_SECS: u64 = 45;
const BOOT_POLL_MS: u64 = 500;

// ── Mode ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum WindowsMode {
    Wsl2,
    Qemu,
}

// ── Main struct ───────────────────────────────────────────────────────────────

pub struct WindowsPlatform {
    mode: Option<WindowsMode>,
    wsl_process: Option<tokio::process::Child>,
    qemu_child: Option<tokio::process::Child>,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            mode: None,
            wsl_process: None,
            qemu_child: None,
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Check if the daemon is currently reachable on the bridge port.
    async fn tcp_reachable() -> bool {
        TcpStream::connect(("127.0.0.1", VM_PORT)).await.is_ok()
    }

    /// Resolve %LOCALAPPDATA%: prefer `dirs`, fall back to env var.
    fn local_app_data() -> Result<PathBuf> {
        if let Some(p) = dirs::data_local_dir() {
            return Ok(p);
        }
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .map_err(|_| anyhow!("Cannot determine %%LOCALAPPDATA%% — neither dirs::data_local_dir() nor the LOCALAPPDATA env var is set"))
    }

    /// Path where the Alpine rootfs tarball is stored locally.
    fn alpine_tar_path() -> Result<PathBuf> {
        Ok(Self::local_app_data()?.join("mowisai").join("agentd-alpine.tar"))
    }

    /// Path where the Alpine qcow2 image is stored locally (QEMU fallback).
    fn alpine_qcow2_path() -> Result<PathBuf> {
        Ok(Self::local_app_data()?.join("mowisai").join("agentd-alpine.qcow2"))
    }

    /// Directory where WSL2 imports the distro virtual disk.
    fn wsl_install_dir() -> Result<PathBuf> {
        Ok(Self::local_app_data()?.join("mowisai").join("wsl"))
    }

    // ── WSL2 helpers ──────────────────────────────────────────────────────────

    /// Return `true` if `wsl.exe` is present on this machine.
    fn wsl_available() -> bool {
        // Prefer PATH lookup; if that misses try the canonical location.
        if which::which("wsl").is_ok() {
            return true;
        }
        PathBuf::from(r"C:\Windows\System32\wsl.exe").exists()
    }

    /// Return path to `wsl.exe`, searching PATH and the canonical location.
    fn wsl_exe() -> Result<PathBuf> {
        if let Ok(p) = which::which("wsl") {
            return Ok(p);
        }
        let canonical = PathBuf::from(r"C:\Windows\System32\wsl.exe");
        if canonical.exists() {
            return Ok(canonical);
        }
        Err(anyhow!(
            "wsl.exe not found — WSL2 does not appear to be installed"
        ))
    }

    /// Return `true` if the "MowisAI" WSL2 distro already exists.
    async fn wsl_distro_exists() -> bool {
        let wsl = match Self::wsl_exe() {
            Ok(p) => p,
            Err(_) => return false,
        };
        // `wsl -l -q` outputs one distro name per line (UTF-16LE on real Windows,
        // but tokio's output() returns raw bytes; we convert leniently).
        let output = tokio::process::Command::new(wsl)
            .args(["-l", "-q"])
            .output()
            .await;

        match output {
            Ok(out) => {
                // Windows encodes this output in UTF-16LE; decode it properly,
                // then fall back to a raw UTF-8 scan so tests work in either mode.
                let text = decode_utf16le_or_utf8(&out.stdout);
                text.lines()
                    .any(|l| l.trim().eq_ignore_ascii_case(WSL_DISTRO_NAME))
            }
            Err(_) => false,
        }
    }

    /// Download `url` to `dest` using `curl.exe` (ships with Windows 10 1803+).
    async fn download_with_curl(
        url: &str,
        dest: &PathBuf,
        label: &str,
        tx: &mpsc::Sender<SetupProgress>,
    ) -> Result<()> {
        // Ensure parent directory exists.
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create_dir_all({parent:?})"))?;
        }

        let _ = tx
            .send(SetupProgress::Downloading {
                label: label.into(),
                pct: 0,
            })
            .await;

        let dest_str = dest
            .to_str()
            .ok_or_else(|| anyhow!("Download path contains non-UTF-8 characters: {dest:?}"))?;

        // curl.exe is built into Windows 10 1803+ at C:\Windows\System32\curl.exe.
        // We prefer PATH so the user can substitute a newer version.
        let curl_bin = which::which("curl")
            .unwrap_or_else(|_| PathBuf::from(r"C:\Windows\System32\curl.exe"));

        let status = tokio::process::Command::new(&curl_bin)
            .args(["-L", "-o", dest_str, url])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .with_context(|| format!("Failed to execute {curl_bin:?}"))?;

        if !status.success() {
            return Err(anyhow!(
                "curl.exe exited with {} while downloading {label}",
                status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into())
            ));
        }

        let _ = tx
            .send(SetupProgress::Downloading {
                label: label.into(),
                pct: 100,
            })
            .await;

        Ok(())
    }

    /// Ensure the Alpine rootfs tarball is present, downloading it if needed.
    async fn ensure_alpine_tar(tx: &mpsc::Sender<SetupProgress>) -> Result<PathBuf> {
        let path = Self::alpine_tar_path()?;
        if !path.exists() {
            Self::download_with_curl(
                ALPINE_TAR_URL,
                &path,
                "MowisAI Alpine rootfs",
                tx,
            )
            .await?;
        }
        Ok(path)
    }

    /// Ensure the Alpine qcow2 image is present, downloading it if needed.
    async fn ensure_alpine_qcow2(tx: &mpsc::Sender<SetupProgress>) -> Result<PathBuf> {
        let path = Self::alpine_qcow2_path()?;
        if !path.exists() {
            Self::download_with_curl(
                ALPINE_QCOW2_URL,
                &path,
                "MowisAI Alpine VM image",
                tx,
            )
            .await?;
        }
        Ok(path)
    }

    /// Import the Alpine tarball as the "MowisAI" WSL2 distro.
    async fn wsl_import_distro(tar_path: &PathBuf, tx: &mpsc::Sender<SetupProgress>) -> Result<()> {
        let _ = tx
            .send(SetupProgress::Installing {
                step: "Creating MowisAI WSL2 environment".into(),
            })
            .await;

        let install_dir = Self::wsl_install_dir()?;
        tokio::fs::create_dir_all(&install_dir)
            .await
            .with_context(|| format!("create_dir_all({install_dir:?})"))?;

        let wsl = Self::wsl_exe()?;

        let tar_str = tar_path
            .to_str()
            .ok_or_else(|| anyhow!("Alpine tar path contains non-UTF-8 characters: {tar_path:?}"))?;
        let install_str = install_dir
            .to_str()
            .ok_or_else(|| anyhow!("WSL install dir contains non-UTF-8 characters: {install_dir:?}"))?;

        let status = tokio::process::Command::new(&wsl)
            .args([
                "--import",
                WSL_DISTRO_NAME,
                install_str,
                tar_str,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .with_context(|| "Failed to run `wsl.exe --import`")?;

        if !status.success() {
            return Err(anyhow!(
                "`wsl --import {WSL_DISTRO_NAME}` failed with exit code {}",
                status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into())
            ));
        }

        Ok(())
    }

    /// Spawn agentd + socat inside the MowisAI WSL2 distro.
    async fn wsl_start_agentd(&mut self) -> Result<()> {
        let wsl = Self::wsl_exe()?;

        let child = tokio::process::Command::new(&wsl)
            .args([
                "-d",
                WSL_DISTRO_NAME,
                "--",
                "/usr/local/bin/start-agentd.sh",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| {
                format!("Failed to spawn agentd inside WSL2 distro '{WSL_DISTRO_NAME}'")
            })?;

        self.wsl_process = Some(child);
        Ok(())
    }

    // ── QEMU helpers ──────────────────────────────────────────────────────────

    /// Locate `qemu-system-x86_64.exe`.
    fn find_qemu() -> Result<PathBuf> {
        if let Ok(p) = which::which("qemu-system-x86_64") {
            return Ok(p);
        }
        // Common manual install location.
        let candidate = PathBuf::from(r"C:\Program Files\qemu\qemu-system-x86_64.exe");
        if candidate.exists() {
            return Ok(candidate);
        }
        Err(anyhow!(
            "qemu-system-x86_64.exe not found. \
             Install WSL2 (Windows feature) or QEMU to run MowisAI."
        ))
    }

    /// Spawn QEMU with WHPX acceleration, retrying with TCG if WHPX is unavailable.
    async fn qemu_start(&mut self, image_path: &PathBuf) -> Result<()> {
        let qemu_bin = Self::find_qemu()?;

        let image_str = image_path
            .to_str()
            .ok_or_else(|| anyhow!("QEMU image path contains non-UTF-8 characters: {image_path:?}"))?;

        // Try WHPX first (Windows Hypervisor Platform — requires Hyper-V feature).
        // If WHPX fails (feature not enabled), fall back to TCG software emulation.
        let child = match self.qemu_spawn(&qemu_bin, image_str, "whpx").await {
            Ok(child) => child,
            Err(whpx_err) => {
                log::warn!(
                    "WHPX accelerator unavailable ({whpx_err}); falling back to TCG \
                     (software emulation — performance will be degraded)"
                );
                self.qemu_spawn(&qemu_bin, image_str, "tcg").await?
            }
        };

        self.qemu_child = Some(child);
        Ok(())
    }

    /// Spawn a single QEMU process with the given `-accel` value.
    async fn qemu_spawn(
        &self,
        qemu_bin: &PathBuf,
        image_str: &str,
        accel: &str,
    ) -> Result<tokio::process::Child> {
        tokio::process::Command::new(qemu_bin)
            .args([
                "-nographic",
                "-m",
                "512",
                "-smp",
                "2",
                "-accel",
                accel,
                "-drive",
                &format!("file={image_str},format=qcow2,if=virtio"),
                "-netdev",
                &format!("user,id=net0,hostfwd=tcp::{VM_PORT}-:{VM_PORT}"),
                "-device",
                "virtio-net-pci,netdev=net0",
                "-serial",
                "none",
                "-monitor",
                "none",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to spawn QEMU ({qemu_bin:?}) with -accel {accel}"))
    }

    // ── Boot-wait helper ─────────────────────────────────────────────────────

    /// Poll TCP 9722 until the daemon responds or the timeout expires.
    async fn wait_for_boot() -> Result<()> {
        let deadline = tokio::time::Instant::now()
            + std::time::Duration::from_secs(BOOT_TIMEOUT_SECS);

        loop {
            if Self::tcp_reachable().await {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "MowisAI daemon did not become reachable on port {VM_PORT} \
                     within {BOOT_TIMEOUT_SECS}s"
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(BOOT_POLL_MS)).await;
        }
    }
}

// ── UTF-16LE decode helper ────────────────────────────────────────────────────

/// Decode bytes as UTF-16LE (Windows default for WSL output); fall back to
/// treating them as UTF-8 so the function is useful in tests / Wine too.
fn decode_utf16le_or_utf8(bytes: &[u8]) -> String {
    // Heuristic: if every other byte is 0x00 and the slice has an even length,
    // it is very likely UTF-16LE.
    if bytes.len() >= 2 && bytes.len() % 2 == 0 && bytes[1] == 0x00 {
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        if let Ok(s) = String::from_utf16(&u16s) {
            return s;
        }
    }
    // Fallback: interpret as UTF-8 (lossy).
    String::from_utf8_lossy(bytes).into_owned()
}

// ── DaemonPlatform impl ───────────────────────────────────────────────────────

#[async_trait]
impl DaemonPlatform for WindowsPlatform {
    async fn ensure_running(&mut self, tx: mpsc::Sender<SetupProgress>) -> Result<()> {
        // ── Step 1: already up? ──────────────────────────────────────────────
        let _ = tx.send(SetupProgress::Checking).await;

        if Self::tcp_reachable().await {
            let _ = tx.send(SetupProgress::Ready).await;
            return Ok(());
        }

        // ── Step 2: try WSL2 ────────────────────────────────────────────────
        if Self::wsl_available() {
            // Ensure the distro is imported.
            if !Self::wsl_distro_exists().await {
                let tar = Self::ensure_alpine_tar(&tx).await?;
                Self::wsl_import_distro(&tar, &tx).await?;
            }

            let _ = tx.send(SetupProgress::Starting).await;

            self.wsl_start_agentd().await?;
            self.mode = Some(WindowsMode::Wsl2);

            Self::wait_for_boot().await?;

            let _ = tx.send(SetupProgress::Ready).await;
            return Ok(());
        }

        // ── Step 3: QEMU fallback ────────────────────────────────────────────
        // Verify QEMU is available before downloading the (large) image.
        if Self::find_qemu().is_err() {
            return Err(anyhow!(
                "Install WSL2 (Windows feature) or QEMU to run MowisAI"
            ));
        }

        let image_path = Self::ensure_alpine_qcow2(&tx).await?;

        let _ = tx.send(SetupProgress::Starting).await;

        self.qemu_start(&image_path).await?;
        self.mode = Some(WindowsMode::Qemu);

        Self::wait_for_boot().await?;

        let _ = tx.send(SetupProgress::Ready).await;
        Ok(())
    }

    fn connection_target(&self) -> ConnectionTarget {
        ConnectionTarget::Tcp { port: VM_PORT }
    }

    async fn is_reachable(&self) -> bool {
        Self::tcp_reachable().await
    }

    async fn stop(&mut self) -> Result<()> {
        match self.mode {
            Some(WindowsMode::Wsl2) => {
                if let Some(mut child) = self.wsl_process.take() {
                    child.kill().await.ok();
                    child.wait().await.ok();
                }
                // Optionally terminate the distro via `wsl --terminate` so its
                // background processes do not linger after the GUI exits.
                if let Ok(wsl) = Self::wsl_exe() {
                    tokio::process::Command::new(&wsl)
                        .args(["--terminate", WSL_DISTRO_NAME])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .await
                        .ok();
                }
            }
            Some(WindowsMode::Qemu) => {
                if let Some(mut child) = self.qemu_child.take() {
                    child.kill().await.ok();
                    child.wait().await.ok();
                }
            }
            None => {}
        }
        Ok(())
    }
}
