#Requires -Version 5.1
#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Checks whether WSL2 is available and enables it if needed.

.DESCRIPTION
    MowisAI requires WSL2 to run agentd (the overlayfs/cgroup execution engine).
    This script is called automatically on first launch.  It can also be run
    manually from an elevated PowerShell prompt.

.OUTPUTS
    Exit code 0 — WSL2 is ready (or was just enabled; reboot may be needed).
    Exit code 1 — A fatal error occurred.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

function Write-Step    { param([string]$m) Write-Host "  $m" -ForegroundColor Cyan    }
function Write-Success { param([string]$m) Write-Host "  $m" -ForegroundColor Green   }
function Write-Warn    { param([string]$m) Write-Host "  $m" -ForegroundColor Yellow  }
function Write-Fail    { param([string]$m) Write-Host "  ERROR: $m" -ForegroundColor Red }

# ---------------------------------------------------------------------------
# OS version gate — WSL2 needs Windows 10 build 19041+ or Windows 11
# ---------------------------------------------------------------------------

$osVersion = [System.Environment]::OSVersion.Version
$buildNumber = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion").CurrentBuild

Write-Step "Windows build: $buildNumber (version $($osVersion.Major).$($osVersion.Minor))"

if ([int]$buildNumber -lt 19041) {
    Write-Fail "WSL2 requires Windows 10 build 19041 or later. " +
               "Please update Windows and re-run this script."
    exit 1
}

# ---------------------------------------------------------------------------
# Check / enable Windows Subsystem for Linux
# ---------------------------------------------------------------------------

function Get-FeatureState {
    param([string]$FeatureName)
    try {
        $f = Get-WindowsOptionalFeature -Online -FeatureName $FeatureName -ErrorAction Stop
        return $f.State
    }
    catch {
        return "Unknown"
    }
}

$needsReboot = $false

$wslState = Get-FeatureState "Microsoft-Windows-Subsystem-Linux"
Write-Step "WSL feature state: $wslState"

if ($wslState -ne "Enabled") {
    Write-Step "Enabling Windows Subsystem for Linux..."
    Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Windows-Subsystem-Linux -NoRestart | Out-Null
    Write-Success "WSL feature enabled."
    $needsReboot = $true
}
else {
    Write-Success "Windows Subsystem for Linux is already enabled."
}

# ---------------------------------------------------------------------------
# Check / enable Virtual Machine Platform (required for WSL2)
# ---------------------------------------------------------------------------

$vmpState = Get-FeatureState "VirtualMachinePlatform"
Write-Step "VirtualMachinePlatform state: $vmpState"

if ($vmpState -ne "Enabled") {
    Write-Step "Enabling VirtualMachinePlatform..."
    Enable-WindowsOptionalFeature -Online -FeatureName VirtualMachinePlatform -NoRestart | Out-Null
    Write-Success "VirtualMachinePlatform enabled."
    $needsReboot = $true
}
else {
    Write-Success "VirtualMachinePlatform is already enabled."
}

# ---------------------------------------------------------------------------
# Set WSL default version to 2 (best-effort — wsl.exe may not be present yet)
# ---------------------------------------------------------------------------

if (-not $needsReboot) {
    if (Get-Command wsl -ErrorAction SilentlyContinue) {
        Write-Step "Setting WSL default version to 2..."
        wsl --set-default-version 2 2>&1 | Out-Null
        if ($LASTEXITCODE -eq 0) {
            Write-Success "WSL default version set to 2."
        }
        else {
            Write-Warn "Could not set WSL default version — you may need to run: wsl --set-default-version 2"
        }
    }
    else {
        Write-Warn "wsl.exe not found yet. After reboot, run: wsl --set-default-version 2"
    }
}

# ---------------------------------------------------------------------------
# Check whether a Linux distro is registered
# ---------------------------------------------------------------------------

if (-not $needsReboot -and (Get-Command wsl -ErrorAction SilentlyContinue)) {
    $distros = wsl --list --quiet 2>&1
    if ($distros -match '\S') {
        Write-Success "WSL2 is ready.  Registered distributions:"
        $distros | Where-Object { $_ -match '\S' } | ForEach-Object {
            Write-Host "    $_" -ForegroundColor White
        }
    }
    else {
        Write-Warn "No WSL2 Linux distribution installed."
        Write-Warn "MowisAI needs a distro (e.g. Ubuntu).  Install one from the Microsoft Store"
        Write-Warn "or run:  wsl --install -d Ubuntu"
    }
}

# ---------------------------------------------------------------------------
# Result
# ---------------------------------------------------------------------------

Write-Host ""
if ($needsReboot) {
    Write-Warn "A system restart is required to complete WSL2 setup."
    Write-Warn "Please restart your computer, then launch MowisAI again."
    Write-Host ""

    $choice = Read-Host "Restart now? [y/N]"
    if ($choice -match '^[Yy]') {
        Restart-Computer -Force
    }
}
else {
    Write-Success "WSL2 is available and ready."
    Write-Host ""
}

exit 0
