# ============================================
# Myriad Development Script (Windows)
# ============================================
# Unified script for all development operations
# Usage: .\dev.ps1 <command> [options]

param(
    [Parameter(Position = 0, Mandatory = $false)]
    [ValidateSet("start", "stop", "restart", "clean", "status", "logs", "help")]
    [string]$Command = "help",
    
    [ValidateSet("backend", "frontend", "updater", "all", "all-updater")]
    [string]$Service = "all",
    
    [switch]$Force
)

$ErrorActionPreference = "Stop"

# Get project root
$projectRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$devUpdaterDir = Join-Path $projectRoot ".dev-updater"
$devUpdaterTokenDefault = "9xQ3vN8mP2rT5wY7zA1bC4dF6hJ8kL0n"

# Helper functions
function Write-Header {
    param([string]$Title)
    Write-Host "`n================================" -ForegroundColor Cyan
    Write-Host "  $Title" -ForegroundColor Cyan
    Write-Host "================================`n" -ForegroundColor Cyan
}

function Write-Success {
    param([string]$Message)
    Write-Host "✓ $Message" -ForegroundColor Green
}

function Write-Info {
    param([string]$Message)
    Write-Host "→ $Message" -ForegroundColor Yellow
}

function Write-Error {
    param([string]$Message)
    Write-Host "✗ $Message" -ForegroundColor Red
}

function Test-BackendHealth {
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:1103/health" -TimeoutSec 1 | Out-Null
        return $true
    }
    catch {
        return $false
    }
}

function Wait-Backend {
    param([int]$TimeoutSeconds = 90)

    Write-Info "Waiting for backend health on http://127.0.0.1:1103/health ..."
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-BackendHealth) {
            Write-Success "Backend is ready"
            return $true
        }
        Start-Sleep -Seconds 1
    }
    return $false
}

function Test-CommandLinePath {
    param(
        [AllowNull()][string]$CommandLine,
        [string]$Path
    )

    if (-not $CommandLine) {
        return $false
    }

    $normalizedCommand = $CommandLine.Replace('\', '/')
    $normalizedPath = $Path.Replace('\', '/')
    return $normalizedCommand.Contains($normalizedPath)
}

function Get-ProjectBackendProcesses {
    $backendPath = Join-Path $projectRoot "backend"
    Get-WmiObject Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        (Test-CommandLinePath $_.CommandLine $backendPath) -and
        ($_.CommandLine -match "cargo run|myriad-backend")
    }
}

function Get-ProjectFrontendProcesses {
    $frontendPath = Join-Path $projectRoot "frontend"
    Get-WmiObject Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        (Test-CommandLinePath $_.CommandLine $frontendPath) -and
        ($_.CommandLine -match "pnpm|astro|vite|node")
    }
}

function Stop-ProjectProcesses {
    param(
        [Parameter(Mandatory = $true)]
        [object[]]$Processes
    )

    $count = 0
    foreach ($proc in $Processes) {
        try {
            Stop-Process -Id $proc.ProcessId -Force -ErrorAction Stop
            $count += 1
        }
        catch {
            Write-Host "  Could not stop PID $($proc.ProcessId): $($_.Exception.Message)" -ForegroundColor Yellow
        }
    }
    return $count
}

function Get-DevUpdaterToken {
    if ($env:MYRIAD_DEV_UPDATE_TOKEN) {
        return $env:MYRIAD_DEV_UPDATE_TOKEN
    }
    return $devUpdaterTokenDefault
}

function Test-UpdaterHealth {
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:1101/healthz" -TimeoutSec 1 | Out-Null
        return $true
    }
    catch {
        return $false
    }
}

function Wait-Updater {
    param([int]$TimeoutSeconds = 120)

    Write-Info "Waiting for updater health on http://127.0.0.1:1101/healthz ..."
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-UpdaterHealth) {
            Write-Success "Updater is ready"
            return $true
        }
        Start-Sleep -Seconds 1
    }
    return $false
}

function Test-UpdaterRunning {
    $running = docker ps --filter "name=myriad-updater-dev" --format "{{.Names}}" 2>$null
    return $running -match "myriad-updater-dev"
}

function Ensure-DevUpdaterFiles {
    New-Item -ItemType Directory -Force -Path $devUpdaterDir | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $devUpdaterDir "state") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $devUpdaterDir "pgdata") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $devUpdaterDir "backups") | Out-Null

    $envText = @"
MYRIAD_TAG=v0.0.0-dev
PROXY_TAG=v0.0.0-dev
UPDATER_TAG=v0.0.0-dev
COMPOSE_PROJECT_NAME=myriad-dev-updater
POSTGRES_PASSWORD=devupdaterpostgres12345678901234567890
JWT_SECRET=devupdaterjwtsecret12345678901234567890
CORS_ORIGINS=http://localhost:1102,http://localhost:1103
UPDATE_TOKEN=$(Get-DevUpdaterToken)
CHANNEL=stable
CHECK_INTERVAL_SECS=0
"@
    Set-Content -Path (Join-Path $devUpdaterDir ".env") -Value $envText -Encoding UTF8

    $composeText = @"
services:
  postgres:
    image: postgres:18-alpine
  backend:
    image: example/myriad-backend:`${MYRIAD_TAG}
  frontend:
    image: example/myriad-frontend:`${MYRIAD_TAG}
  proxy:
    image: example/myriad-proxy:`${PROXY_TAG}
  updater:
    image: example/myriad-updater:`${UPDATER_TAG}
"@
    Set-Content -Path (Join-Path $devUpdaterDir "docker-compose.yml") -Value $composeText -Encoding UTF8
}

function Start-Updater {
    Write-Info "Starting updater dev harness..."
    if (Test-UpdaterRunning) {
        Write-Host "Updater harness is already running" -ForegroundColor Yellow
        return $true
    }

    Ensure-DevUpdaterFiles
    Push-Location $projectRoot
    $prevUpdateToken = $env:UPDATE_TOKEN
    try {
        $env:UPDATE_TOKEN = Get-DevUpdaterToken
        docker compose -f docker-compose.dev.yml --profile updater up -d docker-guard updater updater-gateway
    }
    finally {
        $env:UPDATE_TOKEN = $prevUpdateToken
        Pop-Location
    }

    if (-not (Wait-Updater -TimeoutSeconds 120)) {
        Write-Error "Updater did not become ready within 120s"
        Write-Info "Check logs with: docker compose -f docker-compose.dev.yml --profile updater logs -f docker-guard updater updater-gateway"
        return $false
    }
    Write-Success "Updater harness started (gateway on 127.0.0.1:1104)"
    Write-Info "Backend will use it when started/restarted while the harness is running."
    return $true
}

function Stop-Updater {
    Write-Info "Stopping updater dev harness..."
    Push-Location $projectRoot
    try {
        docker compose -f docker-compose.dev.yml --profile updater stop updater-gateway updater docker-guard 2>$null | Out-Null
        docker compose -f docker-compose.dev.yml --profile updater rm -f updater-gateway updater docker-guard 2>$null | Out-Null
    }
    finally {
        Pop-Location
    }
    Write-Success "Updater harness stopped"
}

# ====================
# START Command
# ====================
function Start-Services {
    Write-Header "Starting Myriad Services"

    $startUpdater = $Service -eq "updater" -or $Service -eq "all-updater"
    $startBackend = $Service -eq "backend" -or $Service -eq "all" -or $Service -eq "all-updater"
    $startFrontend = $Service -eq "frontend" -or $Service -eq "all" -or $Service -eq "all-updater"

    if ($startUpdater) {
        $ok = Start-Updater
        if (-not $ok -or $Service -eq "updater") {
            return
        }
    }
    
    # Check if services are already running
    $existingBackend = @(Get-ProjectBackendProcesses)
    $existingFrontend = @(Get-ProjectFrontendProcesses)

    if ($Service -eq "all-updater" -and $existingBackend) {
        Write-Info "Restarting backend so it picks up updater dev environment..."
        Stop-ProjectProcesses -Processes $existingBackend | Out-Null
        Start-Sleep -Seconds 1
        $existingBackend = @()
    }

    if (($existingBackend -and $startBackend) -or
        ($existingFrontend -and $startFrontend -and $Service -ne "all-updater")) {
        
        if (-not $Force) {
            Write-Host "⚠️  Warning: Some services are already running" -ForegroundColor Yellow
            Write-Host "Use -Force to stop and restart them" -ForegroundColor Yellow
            return
        }
        
        Write-Info "Stopping existing services..."
        Stop-Services
        Start-Sleep -Seconds 2
    }

    # Start Backend
    $backendReady = Test-BackendHealth
    if ($startBackend) {
        if ($backendReady) {
            Write-Info "Backend is already healthy"
        }
        else {
            Write-Info "Starting Backend..."
            $backendPath = Join-Path $projectRoot "backend"
            $backendCommand = "Write-Host '🦀 Myriad Backend' -ForegroundColor Cyan; Write-Host ''; Set-Location '$backendPath'; "
            if (Test-UpdaterRunning) {
                Ensure-DevUpdaterFiles
                $backendCommand += "`$env:MYRIAD_UPDATER_URL='http://127.0.0.1:1104'; "
                Write-Info "Backend updater proxy via gateway: http://127.0.0.1:1104 (no UPDATE_TOKEN in backend)"
            }
            $backendCommand += "cargo run"

            Start-Process powershell -ArgumentList `
                "-NoExit", "-NoProfile", "-Command", `
                $backendCommand `
                -WindowStyle Normal -WorkingDirectory $backendPath

            Write-Success "Backend starting in new window"
            $backendReady = Wait-Backend -TimeoutSeconds 90
            if (-not $backendReady) {
                Write-Error "Backend did not become ready within 90s"
            }
        }
    }

    if (($Service -eq "all" -or $Service -eq "all-updater") -and -not $backendReady) {
        Write-Error "Frontend was not started to avoid 127.0.0.1:1103 ECONNREFUSED."
        Write-Info "Fix the backend error first, then run: .\dev.ps1 start -Service frontend"
        return
    }

    # Start Frontend
    if ($startFrontend) {
        Write-Info "Starting Frontend..."
        $frontendPath = Join-Path $projectRoot "frontend"
        
        Start-Process powershell -ArgumentList `
            "-NoExit", "-NoProfile", "-Command", `
            "Write-Host '⚡ Myriad Frontend' -ForegroundColor Cyan; Write-Host ''; Set-Location '$frontendPath'; pnpm run dev" `
            -WindowStyle Normal -WorkingDirectory $frontendPath
        
        Write-Success "Frontend starting in new window"
    }

    Write-Host "`n" -NoNewline
    Write-Success "Services Started!"
    Write-Host "`nURLs (wait ~10 seconds for startup):"
    Write-Host "  Frontend: http://localhost:1102" -ForegroundColor White
    Write-Host "  Backend:  http://localhost:1103" -ForegroundColor White
    Write-Host "  Health:   http://localhost:1103/health" -ForegroundColor White
    if (Test-UpdaterRunning) {
        Write-Host "  Updater:  http://127.0.0.1:1101" -ForegroundColor White
    }
    Write-Host ""
}

# ====================
# STOP Command
# ====================
function Stop-Services {
    Write-Header "Stopping Myriad Services"
    
    $stoppedCount = 0
    $stopUpdater = $Service -eq "updater" -or $Service -eq "all" -or $Service -eq "all-updater"
    $stopBackend = $Service -eq "backend" -or $Service -eq "all" -or $Service -eq "all-updater"
    $stopFrontend = $Service -eq "frontend" -or $Service -eq "all" -or $Service -eq "all-updater"

    # Stop Backend
    if ($stopBackend) {
        Write-Info "Stopping backend services..."
        
        $backendProcesses = @(Get-ProjectBackendProcesses)
        $stoppedCount += Stop-ProjectProcesses -Processes $backendProcesses
        
        Write-Success "Backend stopped"
    }

    # Stop Frontend
    if ($stopFrontend) {
        Write-Info "Stopping frontend services..."
        
        $nodeProcesses = @(Get-ProjectFrontendProcesses)
        $stoppedCount += Stop-ProjectProcesses -Processes $nodeProcesses
        
        Write-Success "Frontend stopped"
    }

    if ($stopUpdater) {
        Stop-Updater
    }

    Write-Host ""
    if ($stoppedCount -gt 0) {
        Write-Success "Stopped $stoppedCount process(es)"
    }
    else {
        Write-Host "No services were running" -ForegroundColor Gray
    }
    Write-Host ""
}

# ====================
# RESTART Command
# ====================
function Restart-Services {
    Write-Header "Restarting Myriad Services"
    Stop-Services
    Start-Sleep -Seconds 2
    Start-Services
}

# ====================
# CLEAN Command
# ====================
function Clear-Project {
    [System.Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseApprovedVerbs', '')]
    param()
    
    Write-Header "Myriad Clean Tool"
    
    Write-Host "WARNING: This will delete:" -ForegroundColor Yellow
    Write-Host "  - Database data (drop all tables)" -ForegroundColor Yellow
    Write-Host "  - Backend build files (target/)" -ForegroundColor Yellow
    Write-Host "  - Frontend build files (frontend/dist/)" -ForegroundColor Yellow
    Write-Host "  - Cache files (backend/cache/)" -ForegroundColor Yellow
    Write-Host "  - Environment config (backend/.env)" -ForegroundColor Yellow
    Write-Host ""
    
    if (-not $Force) {
        $confirmation = Read-Host "Type 'yes' to continue"
        if ($confirmation -ne "yes") {
            Write-Info "Operation cancelled"
            return
        }
    }
    
    Write-Host "`nStarting cleanup..." -ForegroundColor Cyan

    # 1. Clear database
    Write-Info "[1/5] Clearing database..."
    $dbRunning = docker ps --filter "name=myriad-postgres" --format "{{.Names}}" 2>$null
    if ($dbRunning -eq "myriad-postgres") {
        $dropSQL = @"
DO `$`$ DECLARE r RECORD;
BEGIN
    FOR r IN (SELECT tablename FROM pg_tables WHERE schemaname = 'public') LOOP
        EXECUTE 'DROP TABLE IF EXISTS ' || quote_ident(r.tablename) || ' CASCADE';
    END LOOP;
END `$`$;
"@
        docker exec myriad-postgres psql -U myriad -d myriad -c $dropSQL 2>&1 | Out-Null
        Write-Success "Database tables dropped"
    }
    else {
        Write-Host "  Database not running" -ForegroundColor Gray
    }

    # 2. Delete backend build files
    Write-Info "[2/5] Deleting backend build files..."
    $backendTarget = Join-Path $projectRoot "backend\target"
    if (Test-Path $backendTarget) {
        Remove-Item -Path $backendTarget -Recurse -Force -ErrorAction SilentlyContinue
        Write-Success "Backend build files deleted"
    }
    else {
        Write-Host "  No backend build files" -ForegroundColor Gray
    }

    # 3. Delete frontend build files
    Write-Info "[3/5] Deleting frontend build files..."
    $frontendDist = Join-Path $projectRoot "frontend\dist"
    if (Test-Path $frontendDist) {
        Remove-Item -Path $frontendDist -Recurse -Force
        Write-Success "Frontend build files deleted"
    }
    else {
        Write-Host "  No frontend build files" -ForegroundColor Gray
    }

    # 4. Delete cache files
    Write-Info "[4/5] Deleting cache files..."
    $cacheDir = Join-Path $projectRoot "backend\cache"
    if (Test-Path $cacheDir) {
        Get-ChildItem -Path $cacheDir -Filter "*.json" -ErrorAction SilentlyContinue | Remove-Item -Force
        Write-Success "Cache files deleted"
    }
    else {
        Write-Host "  No cache files" -ForegroundColor Gray
    }

    # 5. Delete environment config
    Write-Info "[5/5] Deleting environment config..."
    $envFile = Join-Path $projectRoot "backend\.env"
    if (Test-Path $envFile) {
        Remove-Item -Path $envFile -Force
        Write-Success ".env file deleted"
    }
    else {
        Write-Host "  No .env file" -ForegroundColor Gray
    }

    Write-Host ""
    Write-Success "Cleanup Complete!"
    Write-Host "`nNext steps:"
    Write-Host "  1. Run '.\dev.ps1 start' to start services" -ForegroundColor White
    Write-Host "  2. Complete setup wizard at http://localhost:1102/setup" -ForegroundColor White
    Write-Host ""
}

# ====================
# STATUS Command
# ====================
function Show-Status {
    Write-Header "Myriad Services Status"
    
    # Backend status
    $backendProcesses = @(Get-ProjectBackendProcesses)
    if ($backendProcesses) {
        Write-Host "Backend:  " -NoNewline
        Write-Host "RUNNING" -ForegroundColor Green
        Write-Host "  Processes: $($backendProcesses.Count)" -ForegroundColor Gray
    }
    else {
        Write-Host "Backend:  " -NoNewline
        Write-Host "STOPPED" -ForegroundColor Red
    }
    
    # Frontend status
    $frontendProcesses = @(Get-ProjectFrontendProcesses)
    if ($frontendProcesses) {
        Write-Host "Frontend: " -NoNewline
        Write-Host "RUNNING" -ForegroundColor Green
        Write-Host "  Processes: $($frontendProcesses.Count)" -ForegroundColor Gray
    }
    else {
        Write-Host "Frontend: " -NoNewline
        Write-Host "STOPPED" -ForegroundColor Red
    }

    if (Test-UpdaterRunning) {
        Write-Host "Updater:  " -NoNewline
        Write-Host "RUNNING" -ForegroundColor Green
        Write-Host "  URL: http://127.0.0.1:1101" -ForegroundColor Gray
    }
    else {
        Write-Host "Updater:  " -NoNewline
        Write-Host "STOPPED" -ForegroundColor Red
    }
    
    Write-Host ""
}

# ====================
# LOGS Command
# ====================
function Show-Logs {
    Write-Header "Myriad Service Logs"
    Write-Host "Logs are displayed in service windows" -ForegroundColor Yellow
    Write-Host "Check the PowerShell windows opened by the start command" -ForegroundColor Gray
    Write-Host ""
}

# ====================
# HELP Command
# ====================
function Show-Help {
    Write-Host "Myriad Development Script" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Usage: .\dev.ps1 <command> [options]" -ForegroundColor White
    Write-Host ""
    Write-Host "Commands:" -ForegroundColor Yellow
    Write-Host "  start [-Service <service>]   - Start services (default: all)" -ForegroundColor White
    Write-Host "  stop [-Service <service>]    - Stop services (default: all)" -ForegroundColor White
    Write-Host "  restart [-Service <service>] - Restart services (default: all)" -ForegroundColor White
    Write-Host "  clean [-Force]               - Clean build files and database" -ForegroundColor White
    Write-Host "  status                       - Show service status" -ForegroundColor White
    Write-Host "  logs                         - Show logs info" -ForegroundColor White
    Write-Host "  help                         - Show this help" -ForegroundColor White
    Write-Host ""
    Write-Host "Services: backend, frontend, updater, all, all-updater (default: all)" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Examples:" -ForegroundColor Yellow
    Write-Host "  .\dev.ps1 start                    # Start all services" -ForegroundColor Gray
    Write-Host "  .\dev.ps1 start -Service all-updater # Start dev stack with updater harness" -ForegroundColor Gray
    Write-Host "  .\dev.ps1 start -Service updater   # Start updater harness only" -ForegroundColor Gray
    Write-Host "  .\dev.ps1 start -Service backend   # Start backend only" -ForegroundColor Gray
    Write-Host "  .\dev.ps1 stop                     # Stop all services" -ForegroundColor Gray
    Write-Host "  .\dev.ps1 restart -Service frontend # Restart frontend" -ForegroundColor Gray
    Write-Host "  .\dev.ps1 clean -Force             # Clean without prompt" -ForegroundColor Gray
    Write-Host "  .\dev.ps1 status                   # Show status" -ForegroundColor Gray
    Write-Host ""
}

# ====================
# Interactive Menu
# ====================
function Show-InteractiveMenu {
    while ($true) {
        Write-Host ""
        Write-Host "================================" -ForegroundColor Cyan
        Write-Host "  Myriad Development Menu" -ForegroundColor Cyan
        Write-Host "================================" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "1. Start all services" -ForegroundColor White
        Write-Host "2. Start all services + updater harness" -ForegroundColor White
        Write-Host "3. Start backend only" -ForegroundColor White
        Write-Host "4. Start frontend only" -ForegroundColor White
        Write-Host "5. Start updater harness only" -ForegroundColor White
        Write-Host "6. Stop all services" -ForegroundColor White
        Write-Host "7. Restart all services" -ForegroundColor White
        Write-Host "8. Show status" -ForegroundColor White
        Write-Host "9. Clean project" -ForegroundColor White
        Write-Host "10. Show logs info" -ForegroundColor White
        Write-Host "0. Exit" -ForegroundColor Gray
        Write-Host ""
        
        $choice = Read-Host "Select an option (0-10)"
        
        switch ($choice) {
            "1" {
                $script:Service = "all"
                Start-Services
            }
            "2" {
                $script:Service = "all-updater"
                Start-Services
            }
            "3" {
                $script:Service = "backend"
                Start-Services
            }
            "4" {
                $script:Service = "frontend"
                Start-Services
            }
            "5" {
                $script:Service = "updater"
                Start-Services
            }
            "6" {
                $script:Service = "all"
                Stop-Services
            }
            "7" {
                $script:Service = "all"
                Restart-Services
            }
            "8" {
                Show-Status
            }
            "9" {
                Clear-Project
            }
            "10" {
                Show-Logs
            }
            "0" {
                Write-Host ""
                Write-Host "Goodbye! 👋" -ForegroundColor Cyan
                return
            }
            default {
                Write-Host ""
                Write-Host "Invalid option. Please select 0-10" -ForegroundColor Red
            }
        }
        
        Write-Host ""
        Write-Host "Press any key to continue..." -ForegroundColor Gray
        try {
            $null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
        }
        catch {
            Start-Sleep -Seconds 2
        }
    }
}

# ====================
# Main Execution
# ====================

# If no command provided or help requested, show interactive menu
if ($Command -eq "help" -and $PSBoundParameters.Count -eq 0) {
    Show-InteractiveMenu
}
else {
    switch ($Command) {
        "start" { Start-Services }
        "stop" { Stop-Services }
        "restart" { Restart-Services }
        "clean" { Clear-Project }
        "status" { Show-Status }
        "logs" { Show-Logs }
        "help" { Show-Help }
    }
    
    # Only wait for key press if running interactively and not from VS Code terminal
    if ($Host.Name -eq "ConsoleHost" -and -not $env:TERM_PROGRAM) {
        Write-Host ""
        Write-Host "Press any key to exit..." -ForegroundColor Gray
        try {
            $null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
        }
        catch {
            # Ignore errors if not in interactive mode
        }
    }
}
