#!/usr/bin/env pwsh
# rustdesk++ Self-Hosted Server Starter
# Starts hbbs (ID/rendezvous server) and hbbr (relay server) locally.

param(
    [string]$ServerDir = "$PSScriptRoot",
    [string]$DataDir = "$PSScriptRoot\data",
    [switch]$NoRelay
)

$ErrorActionPreference = "Continue"
$ServerDir = Resolve-Path $ServerDir

# Ensure data directory
New-Item -ItemType Directory -Force -Path $DataDir | Out-Null

# Kill any existing hbbs/hbbr
Get-Process -Name hbbs, hbbr -ErrorAction SilentlyContinue | Stop-Process -Force

# Generate server key pair if missing
$keyFile = "$DataDir\id_ed25519"
$pubFile = "$DataDir\id_ed25519.pub"
if (-not (Test-Path $keyFile)) {
    Write-Host "Generating server key pair..." -ForegroundColor Yellow
    # hbbs generates the key on first run automatically
}

# Pre-populate the db if using persistent storage
$dbFile = "$DataDir\db_v2.sqlite3"
if (-not (Test-Path $dbFile)) {
    Copy-Item "$PSScriptRoot\db_v2.sqlite3" $dbFile -ErrorAction SilentlyContinue
}

Write-Host "=== Starting rustdesk++ Server ===" -ForegroundColor Cyan

# Start hbbs (ID server + rendezvous)
$hbbs = Join-Path $ServerDir "target\release\hbbs.exe"
if (Test-Path $hbbs) {
    $hbbsArgs = @()
    if (Test-Path $keyFile) { $hbbsArgs += "-k", (Get-Content $keyFile -Raw).Trim() }
    Write-Host "  hbbs (ID server) on :21116 (TCP/UDP), :21115 (NAT), :21118 (WS)" -ForegroundColor Green
    Start-Process -NoNewWindow -FilePath $hbbs -ArgumentList $hbbsArgs
} else {
    Write-Host "  hbbs.exe not found. Run 'cargo build --release --bin hbbs' first." -ForegroundColor Red
}

# Start hbbr (relay server)
if (-not $NoRelay) {
    $hbbr = Join-Path $ServerDir "target\release\hbbr.exe"
    if (Test-Path $hbbr) {
        Write-Host "  hbbr (Relay server) on :21117, :21119 (WS)" -ForegroundColor Green
        Start-Process -NoNewWindow -FilePath $hbbr
    } else {
        Write-Host "  hbbr.exe not found. Run 'cargo build --release --bin hbbr' first." -ForegroundColor Red
    }
}

Start-Sleep 2
Write-Host "=== Server status ===" -ForegroundColor Cyan
Get-Process -Name hbbs, hbbr -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "  $($_.ProcessName) PID $($_.Id) running" -ForegroundColor Green
}

Write-Host ""
Write-Host "Configure your RustDesk clients with:" -ForegroundColor Yellow
Write-Host '  --config "{\"host\":\"127.0.0.1\",\"key\":\"<your-key>\"}"' -ForegroundColor White
Write-Host ""
Write-Host "Or set in the GUI: ID Server = 127.0.0.1:21116, Relay Server = 127.0.0.1:21117" -ForegroundColor White
