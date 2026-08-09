# =============================================================================
# Myriad Docker Unified Deployment Script (PowerShell)
# =============================================================================
# Brings up the full stack (proxy + frontend + backend + postgres + updater)
# defined in docker-compose.yml.
#
# After bootstrap, normal day-to-day updates run through the admin UI:
#   Settings -> About -> Update Management
# See docs/UPDATER_QUICKSTART.md.
# =============================================================================

param(
    [Parameter(Position = 0)]
    [string]$Command = "up",

    # Remaining args (e.g. doctor --host). Avoids clash with automatic $Host.
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Rest = @()
)

$ErrorActionPreference = "Stop"

function Write-Color($c) { $f = $host.UI.RawUI.ForegroundColor; $host.UI.RawUI.ForegroundColor = $c; if ($args) { Write-Output $args }; $host.UI.RawUI.ForegroundColor = $f }
function Write-Ok    { Write-Color Green $args }
function Write-Info  { Write-Color Cyan $args }
function Write-Warn  { Write-Color Yellow $args }
function Write-Err   { Write-Color Red $args }

# Change to repo root.
Set-Location (Resolve-Path (Join-Path $PSScriptRoot "..\.."))

$GuardEnvFile = if ($env:MYRIAD_GUARD_ENV_FILE) {
    $env:MYRIAD_GUARD_ENV_FILE
} else {
    Join-Path $env:ProgramData "Myriad\docker-guard.env"
}

function Show-Usage {
    @"
Usage: deploy.ps1 [command]

Commands:
  up        (default) Initialise .env / pgdata if needed, then docker compose up -d
  down      Stop and remove containers (volumes preserved)
  restart   docker compose restart
  pull      Pull images pinned by .env tags
  logs      docker compose logs -f
  status    docker compose ps + image versions
  doctor    Read-only topology / security checks (docker-guard, sock mounts, cosign)
            Optional: doctor --host    (non-fatal privileged / docker.sock scan)
                      doctor --events  (stream container create/start; Ctrl-C)
  upgrade   Pull images pinned by .env tags + recreate
  help      Show this help

Examples:
  .\deploy.ps1                 # Bootstrap + start
  .\deploy.ps1 down            # Stop
  .\deploy.ps1 status          # See running versions
  .\deploy.ps1 doctor          # Topology security checks
  .\deploy.ps1 doctor --host   # + non-fatal host privilege scan
  .\deploy.ps1 doctor --events # Watch container create/start
"@ | Write-Host
}

function Get-ComposeCmd {
    docker compose version 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { return "docker compose" }
    if (Get-Command docker-compose -ErrorAction SilentlyContinue) { return "docker-compose" }
    Write-Err "X Neither 'docker compose' nor 'docker-compose' is available"
    exit 2
}

function Invoke-Compose {
    Assert-GuardPolicy
    $env:MYRIAD_GUARD_ENV_FILE = (Resolve-Path -LiteralPath $GuardEnvFile).Path
    $cmd = (Get-ComposeCmd) -split " "
    $prefix = @()
    if ($cmd.Count -gt 1) { $prefix = $cmd[1..($cmd.Count - 1)] }
    & $cmd[0] @prefix --env-file .env --env-file $GuardEnvFile @args
}

function Assert-GuardPolicy {
    if (-not (Test-Path -LiteralPath $GuardEnvFile -PathType Leaf)) {
        throw "Missing host-owned Guard policy: $GuardEnvFile. Copy docker-guard.env.example outside the deployment root and set an independently verified repo@sha256 digest."
    }
    $policyPath = (Resolve-Path -LiteralPath $GuardEnvFile).Path
    $rootPath = (Resolve-Path -LiteralPath '.').Path.TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ($policyPath.Equals($rootPath, [StringComparison]::OrdinalIgnoreCase) -or
        $policyPath.StartsWith("$rootPath$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::OrdinalIgnoreCase)) {
        throw "Guard policy must be outside the deployment root: $policyPath"
    }
    $lines = @(Get-Content -LiteralPath $GuardEnvFile)
    $required = @('DOCKER_GUARD_IMAGE', 'GUARD_COMPOSE_PROJECT_NAME', 'GUARD_MYRIAD_DOCKER_NETWORK', 'GUARD_MYRIAD_ADMIN_NETWORK', 'GUARD_MYRIAD_DOCKER_GUARD_NETWORK', 'MYRIAD_GUARD_ENV_FILE')
    foreach ($key in $required) {
        $entries = @($lines | Where-Object { $_ -match "^$([regex]::Escape($key))=\S.+$" })
        if ($entries.Count -ne 1) { throw "$GuardEnvFile must contain exactly one non-empty $key" }
    }
    $matches = @($lines | Where-Object { $_ -match '^DOCKER_GUARD_IMAGE=' })
    if ($matches.Count -ne 1) {
        throw "$GuardEnvFile must contain exactly one DOCKER_GUARD_IMAGE"
    }
    $image = ($matches[0] -split '=', 2)[1]
    if ($image -notmatch '^docker\.io/somekawahitomi/myriad-updater@sha256:[0-9a-fA-F]{64}$') {
        throw "DOCKER_GUARD_IMAGE must be the trusted repository pinned by an exact sha256 digest"
    }
    $configuredPath = (($lines | Where-Object { $_ -match '^MYRIAD_GUARD_ENV_FILE=' }) -split '=', 2)[1]
    if (-not $configuredPath.Equals($policyPath, [StringComparison]::OrdinalIgnoreCase)) {
        throw "MYRIAD_GUARD_ENV_FILE must equal the resolved policy path: $policyPath"
    }
}

function New-Secret {
    $bytes = New-Object byte[] 36
    [System.Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    return [Convert]::ToBase64String($bytes).TrimEnd("=").Replace("+", "-").Replace("/", "_")
}

function Ensure-Key($key, $default) {
    if (-not (Select-String -Path .env -Pattern "^$key=" -Quiet -ErrorAction SilentlyContinue)) {
        Add-Content -Path .env -Value "$key=$default"
        Write-Info "  + appended $key"
    }
}

function Ensure-SecretKey([string]$Key) {
    if (Select-String -Path .env -Pattern "^$Key=.+" -Quiet -ErrorAction SilentlyContinue) {
        return
    }

    $token = New-Secret
    if (Select-String -Path .env -Pattern "^$Key=" -Quiet -ErrorAction SilentlyContinue) {
        $lines = Get-Content .env
        $replaced = $false
        $lines = $lines | ForEach-Object {
            if (-not $replaced -and $_ -match "^$Key=") {
                $replaced = $true
                "$Key=$token"
            } else {
                $_
            }
        }
        Set-Content -Path .env -Value $lines
        Write-Info "  + filled empty $Key"
    } else {
        Add-Content -Path .env -Value "$Key=$token"
        Write-Info "  + appended $Key"
    }
}

function Ensure-UpdateToken {
    Ensure-SecretKey "UPDATE_TOKEN"
}

function Ensure-UpdaterGatewaySecret {
    Ensure-SecretKey "UPDATER_GATEWAY_SECRET"
}

function Ensure-Env {
    if (-not (Test-Path ".env")) {
        if (-not (Test-Path ".env.production.example")) {
            Write-Err "X Missing both .env and .env.production.example"
            exit 2
        }
        Write-Warn ".env not found - copying from .env.production.example"
        Copy-Item ".env.production.example" ".env"
        Write-Warn ""
        Write-Warn "Edit .env now and set at minimum:"
        Write-Warn "  - POSTGRES_PASSWORD"
        Write-Warn "  - JWT_SECRET"
        Write-Warn "  - CORS_ORIGINS"
        Write-Warn ""
        Write-Warn "This script will create pgdata/state/backups and fill empty UPDATE_TOKEN / UPDATER_GATEWAY_SECRET."
        Write-Warn ""
        $r = Read-Host "Open .env in notepad? (y/N)"
        if ($r -match "^[Yy]$") {
            notepad .env
        }
    }
}

function Ensure-CurrentLayout {
    Write-Info "==> Ensuring current proxy + updater layout"
    New-Item -ItemType Directory -Force -Path pgdata, state, state/snapshots, state/cache, backups | Out-Null
    Ensure-Key "MYRIAD_TAG" "v0.3.28"
    Ensure-Key "PROXY_TAG" "v0.3.28"
    Ensure-Key "UPDATER_TAG" "v0.3.28"
    Ensure-Key "BACKEND_IMAGE" "docker.io/somekawahitomi/myriad-backend"
    Ensure-Key "FRONTEND_IMAGE" "docker.io/somekawahitomi/myriad-frontend"
    Ensure-Key "COMPOSE_PROJECT_NAME" "myriad"
    Ensure-Key "CHANNEL" "stable"
    Ensure-Key "UPDATE_MODE" "release"
    Ensure-Key "MYRIAD_GITHUB_REPO" "Myriad-You/Myriad"
    Ensure-Key "CHECK_INTERVAL_SECS" "3600"
    Ensure-Key "PROXY_ALLOW_DIRECT_UPDATER" "false"
    Ensure-UpdateToken
    Ensure-UpdaterGatewaySecret
}

# Backend runs as uid 1000 (USER myriad). Named volumes are root-owned on first
# create, and older deployments may also leave owner-write/search bits unset.
# Repair both ownership and owner permissions without broadening group/world access.
function Ensure-BackendVolumePerms {
    $project = $env:COMPOSE_PROJECT_NAME
    if ([string]::IsNullOrWhiteSpace($project)) {
        $match = Select-String -Path .env -Pattern "^COMPOSE_PROJECT_NAME=(.+)$" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($match) {
            $project = $match.Matches[0].Groups[1].Value.Trim().Trim('"').Trim("'")
        }
    }
    if ([string]::IsNullOrWhiteSpace($project)) { $project = "myriad" }
    if ($project -notmatch '^[a-z0-9][a-z0-9_-]*$') {
        throw "Invalid COMPOSE_PROJECT_NAME for backend volume repair: $project"
    }

    $cacheVol = "${project}_backend_cache"
    $dataVol = "${project}_backend_data"

    Write-Info "==> Ensuring backend named volumes writable by uid 1000 (myriad)"
    docker volume create $cacheVol | Out-Null
    docker volume create $dataVol | Out-Null
    docker run --rm `
        -v "${cacheVol}:/app/cache" `
        -v "${dataVol}:/app/data" `
        alpine:3.20 `
        sh -c "chown -R 1000:1000 /app/cache /app/data && chmod -R u+rwX /app/cache /app/data"
    if ($LASTEXITCODE -ne 0) {
        Write-Err "Backend volume ownership/permission repair failed; refusing to start a broken backend."
        Write-Err "Run as host admin:"
        Write-Err "  docker run --rm -v ${cacheVol}:/app/cache -v ${dataVol}:/app/data alpine:3.20 sh -c 'chown -R 1000:1000 /app/cache /app/data && chmod -R u+rwX /app/cache /app/data'"
        throw "Backend volume repair failed"
    }

    docker run --rm --user 1000:1000 `
        -v "${cacheVol}:/app/cache" `
        -v "${dataVol}:/app/data" `
        alpine:3.20 `
        sh -eu -c 'umask 077; probe_dir() { dir="$1"; [ -L "$dir" ] && exit 1; [ -d "$dir" ] || return 0; probe="$dir/.myriad-volume-write-probe-$$"; (set -C; : > "$probe") || exit 1; rm -f -- "$probe"; }; probe_dir /app/cache; probe_dir /app/data; probe_dir /app/data/tapps; for dir in /app/data/tapps/*; do [ -e "$dir" ] || [ -L "$dir" ] || continue; [ -L "$dir" ] && exit 1; owner_id="${dir##*/}"; case "$owner_id" in ""|*[!0-9]*) continue ;; esac; probe_dir "$dir"; done'
    if ($LASTEXITCODE -ne 0) {
        Write-Err "Backend volume remains unwritable by uid 1000 after repair; refusing to continue."
        Write-Err "Check for a read-only mount, NFS/CIFS root_squash, ACLs, or immutable attributes."
        throw "Backend volume write verification failed"
    }
    Write-Ok "Backend volumes are writable by uid 1000"
}

# Soft (warn-only) topology check after successful up/upgrade. Never fails deploy.
function Cmd-SoftDoctor {
    Write-Info "==> Post-deploy topology soft-check (warn-only)"
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try {
        if (-not (Cmd-Doctor -Soft)) {
            Write-Warn "Topology soft-check reported issues; run: .\deploy.ps1 doctor  for details"
        }
    } finally {
        $ErrorActionPreference = $prevEap
    }
}

function Cmd-Up {
    Ensure-Env
    Ensure-CurrentLayout
    Ensure-BackendVolumePerms
    Write-Info "==> docker compose up -d"
    Invoke-Compose up -d
    Write-Host ""
    Write-Ok "Stack started. Admin UI: http://localhost/ -> Settings -> About -> Update Management"
    Cmd-SoftDoctor
}

function Cmd-Down     { Write-Info "==> docker compose down"; Invoke-Compose down }
function Cmd-Restart  { Write-Info "==> docker compose restart"; Invoke-Compose restart }
function Cmd-Pull     { Write-Info "==> docker compose pull"; Invoke-Compose pull }
function Cmd-Logs     { Invoke-Compose logs -f --tail=200 }
function Cmd-Status {
    Invoke-Compose ps
    Write-Host ""
    Write-Info "Image versions in use:"
    Invoke-Compose images 2>$null
    if ($LASTEXITCODE -ne 0) { Invoke-Compose ps --format "table {{.Service}}`t{{.Image}}" }
}

function Test-ContainerExists([string]$Name) {
    docker inspect $Name 2>$null | Out-Null
    return ($LASTEXITCODE -eq 0)
}

function Test-ContainerMountsSock([string]$Name) {
    $mounts = docker inspect -f '{{range .Mounts}}{{.Source}}|{{.Destination}}{{"\n"}}{{end}}' $Name 2>$null
    if ($LASTEXITCODE -ne 0) { return $false }
    return ($mounts -match 'docker\.sock')
}

function Get-ContainerHealth([string]$Name) {
    $h = docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' $Name 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($h)) { return "unknown" }
    return $h.Trim()
}

function Get-ContainerNetworks([string]$Name) {
    $nets = docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}' $Name 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($nets)) { return @() }
    return @($nets.Trim() -split '\s+' | Where-Object { $_ })
}

function Test-ContainerOnNetwork([string]$Name, [string]$Network) {
    return (Get-ContainerNetworks $Name) -contains $Network
}

function Test-ContainerEnvHas([string]$Name, [string]$Prefix) {
    $envLines = docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' $Name 2>$null
    if ($LASTEXITCODE -ne 0) { return $false }
    return ($envLines -split "`n" | Where-Object { $_ -like "$Prefix*" }).Count -gt 0
}

function Get-ContainerEnvValue([string]$Name, [string]$Key) {
    $envLines = docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' $Name 2>$null
    if ($LASTEXITCODE -ne 0) { return $null }
    $line = $envLines -split "`n" | Where-Object { $_ -like "$Key=*" } | Select-Object -First 1
    if (-not $line) { return $null }
    return $line.Substring($Key.Length + 1)
}

function Get-EnvValue([string]$Key) {
    if (-not (Test-Path ".env")) { return $null }
    $line = Select-String -Path .env -Pattern "^$Key=(.*)$" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $line) { return $null }
    return $line.Matches[0].Groups[1].Value.Trim().Trim('"').Trim("'")
}

function Test-EnvTruthy([string]$Key) {
    $v = Get-EnvValue $Key
    if ($null -eq $v) { return $false }
    return @("true", "1", "yes", "on") -contains $v.ToLowerInvariant()
}

# Read-only topology checks. Does not migrate or restart services.
# Returns $true on pass, $false on fail. Use -Soft to avoid exit (for post-up check).
# Use -HostScan / --host for non-fatal privileged/sock scan.
# Use -Events / --events to stream container create/start (skips topology).
function Cmd-Doctor {
    param(
        [switch]$Soft,
        [switch]$HostScan,
        [switch]$Events
    )
    if ($Events) {
        Write-Info "==> Streaming docker events (container create/start) — Ctrl-C to stop"
        Write-Info "    Follow up with inspect if you see unexpected Privileged=true or docker.sock binds"
        Write-Info "    Expected sock holder: myriad-docker-guard only"
        docker events `
            --filter 'type=container' `
            --filter 'event=create' `
            --filter 'event=start' `
            --format '{{.Time}} {{.Action}} {{.Actor.Attributes.name}} image={{.Actor.Attributes.image}}'
        return $true
    }
    $fail = 0
    $skip = 0
    $adminNet = Get-EnvValue "MYRIAD_ADMIN_NETWORK"
    if ([string]::IsNullOrWhiteSpace($adminNet)) { $adminNet = "myriad-admin-net" }
    $guardNet = Get-EnvValue "MYRIAD_DOCKER_GUARD_NETWORK"
    if ([string]::IsNullOrWhiteSpace($guardNet)) { $guardNet = "myriad-docker-guard-net" }
    $businessNet = Get-EnvValue "MYRIAD_DOCKER_NETWORK"
    if ([string]::IsNullOrWhiteSpace($businessNet)) { $businessNet = "myriad-net" }

    Write-Info "==> Deploy topology doctor (read-only)"

    if (Test-ContainerExists "myriad-docker-guard") {
        Write-Ok "PASS  myriad-docker-guard container exists"
        $gh = Get-ContainerHealth "myriad-docker-guard"
        if ($gh -eq "healthy" -or $gh -eq "running") {
            Write-Ok "PASS  docker-guard status=$gh"
        } else {
            Write-Err "FAIL  docker-guard status=$gh (expected healthy or running)"
            $fail++
        }
        if (Test-ContainerMountsSock "myriad-docker-guard") {
            Write-Ok "PASS  docker-guard mounts docker.sock"
        } else {
            Write-Err "FAIL  docker-guard does not mount docker.sock"
            $fail++
        }
        if (Test-ContainerOnNetwork "myriad-docker-guard" $guardNet) {
            Write-Ok "PASS  docker-guard is on $guardNet"
        } else {
            Write-Err "FAIL  docker-guard is not on $guardNet"
            $fail++
        }
        if (Test-ContainerOnNetwork "myriad-docker-guard" $businessNet) {
            Write-Err "FAIL  docker-guard must not be on business net $businessNet"
            $fail++
        } else {
            Write-Ok "PASS  docker-guard is not on business net $businessNet"
        }
        $expectedGuardImage = Get-ContainerEnvValue "myriad-docker-guard" "DOCKER_GUARD_EXPECTED_IMAGE"
        $runningGuardImage = docker inspect -f '{{.Config.Image}}' myriad-docker-guard 2>$null
        if ($expectedGuardImage -match '^docker\.io/somekawahitomi/myriad-updater@sha256:[0-9a-fA-F]{64}$' -and
            $runningGuardImage -eq $expectedGuardImage) {
            Write-Ok "PASS  docker-guard runs the host-pinned trusted image digest"
        } else {
            Write-Err "FAIL  docker-guard identity mismatch (running=$runningGuardImage expected=$expectedGuardImage)"
            $fail++
        }
        $legacyAllowlist = Get-ContainerEnvValue "myriad-docker-guard" "DOCKER_GUARD_ALLOWED_IMAGES"
        $legacyToken = Get-ContainerEnvValue "myriad-docker-guard" "UPDATE_TOKEN"
        if ($legacyAllowlist -or $legacyToken) {
            Write-Err "FAIL  docker-guard still trusts updater-controlled runtime policy/token (legacy topology)"
            $fail++
        } else {
            Write-Ok "PASS  docker-guard has no mutable image allowlist or updater credential"
        }
    } else {
        Write-Err "FAIL  myriad-docker-guard not found (stack down or legacy pre-guard topology)"
        $fail++
    }

    if (Test-ContainerExists "myriad-updater") {
        Write-Ok "PASS  myriad-updater container exists"
        if (Test-ContainerMountsSock "myriad-updater") {
            Write-Err "FAIL  myriad-updater mounts docker.sock (legacy layout — sock should only be on docker-guard)"
            $fail++
        } else {
            Write-Ok "PASS  myriad-updater does not mount docker.sock"
        }
        $dhost = Get-ContainerEnvValue "myriad-updater" "DOCKER_HOST"
        if ($dhost -and $dhost -match "docker-guard") {
            Write-Ok "PASS  updater DOCKER_HOST points at docker-guard ($dhost)"
        } elseif ([string]::IsNullOrWhiteSpace($dhost)) {
            Write-Err "FAIL  updater DOCKER_HOST is unset (expected tcp://docker-guard:2375)"
            $fail++
        } else {
            Write-Err "FAIL  updater DOCKER_HOST=$dhost (expected to contain docker-guard)"
            $fail++
        }
        if (Test-ContainerOnNetwork "myriad-updater" $adminNet) {
            Write-Ok "PASS  updater is on admin-net $adminNet"
        } else {
            Write-Err "FAIL  updater is not on admin-net $adminNet"
            $fail++
        }
        if (Test-ContainerOnNetwork "myriad-updater" $guardNet) {
            Write-Ok "PASS  updater is on guard-net $guardNet"
        } else {
            Write-Err "FAIL  updater is not on guard-net $guardNet"
            $fail++
        }
        if (Test-ContainerOnNetwork "myriad-updater" $businessNet) {
            Write-Err "FAIL  updater must not be on business net $businessNet (frontend/postgres isolation)"
            $fail++
        } else {
            Write-Ok "PASS  updater is not on business net $businessNet"
        }
    } else {
        Write-Warn "SKIP  myriad-updater not running"
        $skip++
    }

    if (Test-ContainerExists "myriad-updater-gateway") {
        Write-Ok "PASS  myriad-updater-gateway container exists"
        if (Test-ContainerMountsSock "myriad-updater-gateway") {
            Write-Err "FAIL  updater-gateway mounts docker.sock (must not)"
            $fail++
        } else {
            Write-Ok "PASS  updater-gateway does not mount docker.sock"
        }
        if (Test-ContainerOnNetwork "myriad-updater-gateway" $adminNet) {
            Write-Ok "PASS  updater-gateway is on admin-net $adminNet"
        } else {
            Write-Err "FAIL  updater-gateway is not on admin-net $adminNet"
            $fail++
        }
        if (Test-ContainerOnNetwork "myriad-updater-gateway" $guardNet) {
            Write-Err "FAIL  updater-gateway must not be on guard-net $guardNet"
            $fail++
        } else {
            Write-Ok "PASS  updater-gateway is not on guard-net"
        }
        if (Test-ContainerOnNetwork "myriad-updater-gateway" $businessNet) {
            Write-Err "FAIL  updater-gateway must not be on business net $businessNet"
            $fail++
        } else {
            Write-Ok "PASS  updater-gateway is not on business net"
        }
    } else {
        Write-Err "FAIL  myriad-updater-gateway not found (P0 topology requires gateway; token off backend)"
        $fail++
    }

    if (Test-ContainerExists "myriad-backend") {
        Write-Ok "PASS  myriad-backend container exists"
        $buser = docker inspect -f '{{.Config.User}}' myriad-backend 2>$null
        if ($LASTEXITCODE -ne 0) { $buser = "" }
        $buser = if ($null -eq $buser) { "" } else { $buser.Trim() }
        if ($buser -in @("", "0", "0:0", "root")) {
            $uid = docker exec myriad-backend id -u 2>$null
            if ($LASTEXITCODE -eq 0 -and $uid.Trim() -eq "0") {
                Write-Warn "WARN  backend appears to run as uid 0 (prefer non-root / USER myriad)"
            } elseif ($LASTEXITCODE -eq 0) {
                Write-Ok "PASS  backend runtime uid=$($uid.Trim()) (Config.User=$buser)"
            } else {
                Write-Warn "SKIP  backend user not inspectable (Config.User=$buser)"
                $skip++
            }
        } else {
            Write-Ok "PASS  backend Config.User=$buser"
        }
        if (Test-ContainerEnvHas "myriad-backend" "UPDATE_TOKEN=") {
            Write-Err "FAIL  backend Config.Env contains UPDATE_TOKEN (should use updater-gateway only)"
            $fail++
        } else {
            Write-Ok "PASS  backend Config.Env has no UPDATE_TOKEN"
        }
        if (Test-ContainerEnvHas "myriad-backend" "UPDATER_GATEWAY_SECRET=") {
            $bsec = Get-ContainerEnvValue "myriad-backend" "UPDATER_GATEWAY_SECRET"
            if ($bsec -and $bsec.Length -ge 32) {
                Write-Ok "PASS  backend has UPDATER_GATEWAY_SECRET (≥32 chars)"
            } else {
                Write-Err "FAIL  backend UPDATER_GATEWAY_SECRET is set but shorter than 32 chars"
                $fail++
            }
        } else {
            Write-Err "FAIL  backend Config.Env missing UPDATER_GATEWAY_SECRET (required for gateway hop)"
            $fail++
        }
        $upUrl = Get-ContainerEnvValue "myriad-backend" "MYRIAD_UPDATER_URL"
        if ($upUrl -and ($upUrl -match "updater-gateway|updater")) {
            Write-Ok "PASS  backend MYRIAD_UPDATER_URL=$upUrl"
        } elseif ([string]::IsNullOrWhiteSpace($upUrl)) {
            Write-Warn "WARN  backend MYRIAD_UPDATER_URL unset (defaults may still apply)"
        } else {
            Write-Warn "WARN  backend MYRIAD_UPDATER_URL=$upUrl (expected updater-gateway)"
        }
        # Soft reachability: gateway /healthz when exec works
        $probeUrl = if ($upUrl) { "$($upUrl.TrimEnd('/'))/healthz" } else { "http://updater-gateway:1104/healthz" }
        docker exec myriad-backend wget --spider -q "http://updater-gateway:1104/healthz" 2>$null | Out-Null
        $probeOk = ($LASTEXITCODE -eq 0)
        if (-not $probeOk) {
            docker exec myriad-backend wget --spider -q $probeUrl 2>$null | Out-Null
            $probeOk = ($LASTEXITCODE -eq 0)
        }
        if ($probeOk) {
            Write-Ok "PASS  backend can reach updater-gateway /healthz (soft)"
        } else {
            Write-Warn "WARN  backend cannot probe updater-gateway /healthz (soft; stack may still be starting)"
        }
    } else {
        Write-Warn "SKIP  myriad-backend not running"
        $skip++
    }

    if (Test-ContainerExists "myriad-updater-gateway") {
        if (Test-ContainerEnvHas "myriad-updater-gateway" "UPDATER_GATEWAY_SECRET=") {
            $gsec = Get-ContainerEnvValue "myriad-updater-gateway" "UPDATER_GATEWAY_SECRET"
            if ($gsec -and $gsec.Length -ge 32) {
                Write-Ok "PASS  updater-gateway has UPDATER_GATEWAY_SECRET (≥32 chars)"
            } else {
                Write-Err "FAIL  updater-gateway UPDATER_GATEWAY_SECRET shorter than 32 chars"
                $fail++
            }
        } else {
            Write-Err "FAIL  updater-gateway missing UPDATER_GATEWAY_SECRET"
            $fail++
        }
    }

    if (Test-Path ".env") {
        $cosign = Get-EnvValue "COSIGN_VERIFY"
        if ([string]::IsNullOrWhiteSpace($cosign)) { $cosign = "strict" }
        switch ($cosign.ToLowerInvariant()) {
            { $_ -in @("off", "false", "0") } {
                if ((Test-EnvTruthy "UPDATER_ALLOW_INSECURE_COSIGN") -or (Test-EnvTruthy "COSIGN_INSECURE_OK")) {
                    Write-Warn "WARN  COSIGN_VERIFY=$cosign with insecure allow key set (supply-chain risk)"
                } else {
                    Write-Err "FAIL  COSIGN_VERIFY=$cosign without UPDATER_ALLOW_INSECURE_COSIGN=true (updater refuses to start)"
                    $fail++
                }
            }
            { $_ -in @("soft", "warn") } {
                Write-Warn "WARN  COSIGN_VERIFY=$cosign (prefer strict for production)"
            }
            default {
                Write-Ok "PASS  COSIGN_VERIFY=$cosign"
            }
        }

        $direct = Get-EnvValue "PROXY_ALLOW_DIRECT_UPDATER"
        if ([string]::IsNullOrWhiteSpace($direct)) { $direct = "false" }
        if (@("true", "1", "yes", "on") -contains $direct.ToLowerInvariant()) {
            Write-Warn "WARN  PROXY_ALLOW_DIRECT_UPDATER=$direct (rescue path; keep false for normal ops)"
        } else {
            Write-Ok "PASS  PROXY_ALLOW_DIRECT_UPDATER=$direct"
        }
    } else {
        Write-Warn "SKIP  .env not found (cosign / direct-updater checks)"
        $skip++
    }

    Write-Host ""
    Write-Info "Optional host checks:"
    Write-Info "  .\deploy.ps1 doctor --host     # non-fatal privileged / docker.sock scan"
    Write-Info "  .\deploy.ps1 doctor --events   # stream container create/start (Ctrl-C)"

    if ($HostScan) {
        Write-Host ""
        Write-Info "==> Optional host privilege scan (non-fatal; warn only)"
        $foundPriv = 0
        $foundSock = 0
        $ids = docker ps -aq 2>$null
        if ($LASTEXITCODE -eq 0 -and $ids) {
            foreach ($id in @($ids -split "`n" | Where-Object { $_ })) {
                $priv = (docker inspect -f '{{.HostConfig.Privileged}}' $id 2>$null)
                $name = (docker inspect -f '{{.Name}}' $id 2>$null)
                if ($name) { $name = $name.TrimStart('/') }
                if ($priv -eq "true") {
                    Write-Warn "WARN  privileged container: $(if ($name) { $name } else { $id })"
                    $foundPriv++
                }
                if (Test-ContainerMountsSock $id) {
                    switch ($name) {
                        { $_ -in @("myriad-docker-guard", "myriad-docker-guard-dev") } {
                            Write-Ok "PASS  docker.sock bind expected on $name"
                        }
                        default {
                            Write-Warn "WARN  docker.sock bind on unexpected container: $(if ($name) { $name } else { $id })"
                            $foundSock++
                        }
                    }
                }
            }
            if ($foundPriv -eq 0) {
                Write-Ok "PASS  no Privileged=true containers found (scan)"
            }
            if ($foundSock -eq 0) {
                Write-Ok "PASS  no unexpected docker.sock binds (scan)"
            }
        } else {
            Write-Warn "SKIP  docker CLI unavailable for host scan"
            $skip++
        }
    }

    Write-Host ""
    if ($fail -gt 0) {
        Write-Err "Doctor: $fail check(s) failed (skip=$skip). Fix topology; this command does not auto-migrate."
        return $false
    }
    Write-Ok "Doctor: all checks passed (skip=$skip)."
    # Soft operator red lines (never fail doctor). Full list:
    # docs/deployment/UPDATER_SECURITY_BASELINE.md
    Write-Host ""
    Write-Info "Operator red lines (reminders — see docs/deployment/UPDATER_SECURITY_BASELINE.md):"
    Write-Info "  1. Existing installs: host deploy upgrade / compose up -d so topology matches"
    Write-Info "  2. Protect UPDATE_TOKEN and UPDATER_GATEWAY_SECRET (not frontend/tickets)"
    Write-Info "  3. Keep PROXY_ALLOW_DIRECT_UPDATER=false except temporary rescue"
    Write-Info "  4. Keep COSIGN_VERIFY=strict unless intentional dual-key off"
    Write-Info "  5. Do not publish updater/gateway/guard ports on the host"
    return $true
}

function Cmd-Upgrade {
    Ensure-Env
    Ensure-BackendVolumePerms
    Write-Info "==> docker compose pull"
    Invoke-Compose pull
    Write-Info "==> docker compose up -d (recreate with new tags)"
    Invoke-Compose up -d
    Write-Ok "Upgrade complete."
    Cmd-SoftDoctor
}

$hostScan = $false
$eventsOnly = $false
foreach ($a in @($Rest)) {
    if ($a -eq "--host" -or $a -eq "-Host" -or $a -eq "-host") {
        $hostScan = $true
    }
    if ($a -eq "--events" -or $a -eq "-Events" -or $a -eq "-events") {
        $eventsOnly = $true
    }
}

switch ($Command.ToLower()) {
    "up"      { Cmd-Up }
    "down"    { Cmd-Down }
    "restart" { Cmd-Restart }
    "pull"    { Cmd-Pull }
    "logs"    { Cmd-Logs }
    "status"  { Cmd-Status }
    "doctor"  {
        if (-not (Cmd-Doctor -HostScan:$hostScan -Events:$eventsOnly)) { exit 1 }
    }
    "upgrade" { Cmd-Upgrade }
    "help"    { Show-Usage }
    "-h"      { Show-Usage }
    "--help"  { Show-Usage }
    default {
        Write-Err "Unknown command: $Command"
        Show-Usage
        exit 1
    }
}
