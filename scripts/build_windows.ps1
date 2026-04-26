#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$OutDir = "dist"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "  $Message" -ForegroundColor Cyan
}

function Write-Success {
    param([string]$Message)
    Write-Host "  $Message" -ForegroundColor Green
}

function Write-Fail {
    param([string]$Message)
    Write-Host "  ERROR: $Message" -ForegroundColor Red
}

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------

Write-Host ""
Write-Host "MowisAI Windows build" -ForegroundColor White
Write-Host ("=" * 40) -ForegroundColor DarkGray
Write-Host ""

Write-Step "Checking prerequisites..."

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Fail "cargo not found. Install Rust from https://rustup.rs"
    exit 1
}

$cargoVersion = cargo --version 2>&1
Write-Success "Found $cargoVersion"

# Ensure the requested target is installed
$installedTargets = rustup target list --installed 2>&1
if ($installedTargets -notmatch [regex]::Escape($Target)) {
    Write-Step "Installing Rust target $Target..."
    rustup target add $Target
    if ($LASTEXITCODE -ne 0) {
        Write-Fail "Failed to install target $Target"
        exit 1
    }
    Write-Success "Target installed."
}
else {
    Write-Success "Target $Target already installed."
}

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

Write-Host ""
Write-Step "Building mowis-gui (release, $Target)..."
Write-Host ""

cargo build --release --target $Target -p mowis-gui
if ($LASTEXITCODE -ne 0) {
    Write-Fail "cargo build failed."
    exit 1
}

# ---------------------------------------------------------------------------
# Collect artefacts
# ---------------------------------------------------------------------------

Write-Host ""
Write-Step "Collecting artefacts into '$OutDir\'..."

$BinSrc = "target\$Target\release\mowisai.exe"

if (-not (Test-Path $BinSrc)) {
    # Fallback: cargo may name the binary after the package
    $BinSrc = "target\$Target\release\mowis-gui.exe"
    if (-not (Test-Path $BinSrc)) {
        Write-Fail "Built binary not found. Expected target\$Target\release\mowisai.exe"
        exit 1
    }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$Dest = Join-Path $OutDir "mowisai.exe"
Copy-Item $BinSrc $Dest -Force

$SizeMB = [math]::Round((Get-Item $Dest).Length / 1MB, 1)

Write-Host ""
Write-Host ("=" * 40) -ForegroundColor DarkGray
Write-Success "Build complete."
Write-Host "  Binary : $Dest ($SizeMB MB)" -ForegroundColor White
Write-Host "  Run    : .\$Dest" -ForegroundColor White
Write-Host ""
