# ============================================
# Myriad image build + pack (Windows)
# ============================================
# Builds the backend and/or frontend image from the repo root, then exports
# them into a single tar you can scp to a server and `docker load`.
#
# Usage:
#   .\scripts\build-and-pack.ps1 -Tag v0.5.1-custom1
#   .\scripts\build-and-pack.ps1 -Tag dev-abc123 -Component backend
#   .\scripts\build-and-pack.ps1 -Tag v0.5.1 -Platform linux/arm64
#   .\scripts\build-and-pack.ps1 -Tag v0.5.1 -Registry registry.example.com/you -Push
#
# After packing:
#   scp <tar> user@server:/opt/myriad/
#   ssh user@server 'cd /opt/myriad && docker load -i <tar>'

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Tag,

    [ValidateSet("all", "backend", "frontend")]
    [string]$Component = "all",

    [string]$Platform = "linux/amd64",

    # Optional registry/repo prefix, e.g. registry.example.com/you.
    # Empty = local names `myriad-backend:<tag>` / `myriad-frontend:<tag>`.
    [string]$Registry = "",

    # Push after building. Requires -Registry.
    [switch]$Push,

    # Tar output. Relative paths resolve against the repo root (so the default
    # lands in the ignored ./dist/ directory).
    [string]$Output = "dist/myriad-images-$Tag.tar",

    # Full LTO (release, slow) vs thin LTO (ci-release, matches CI).
    [ValidateSet("release", "ci-release")]
    [string]$CargoProfile = "ci-release",

    # Build only; skip docker save.
    [switch]$NoSave,

    # Pass --no-cache to docker build.
    [switch]$NoCache
)

$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Write-Ok {
    param([string]$Message)
    Write-Host "  ok  $Message" -ForegroundColor Green
}

function Write-Note {
    param([string]$Message)
    Write-Host "  ->  $Message" -ForegroundColor Yellow
}

function Invoke-Docker {
    param([string[]]$Arguments)
    Write-Host "> docker $($Arguments -join ' ')" -ForegroundColor DarkGray
    & docker @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "docker $($Arguments[0]) failed with exit code $LASTEXITCODE"
    }
}

if ([string]::IsNullOrWhiteSpace($Tag)) {
    throw "-Tag must not be empty"
}
if ($Push -and [string]::IsNullOrWhiteSpace($Registry)) {
    throw "-Push requires -Registry"
}
if ($Registry.EndsWith("/")) {
    $Registry = $Registry.TrimEnd("/")
}

$projectRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$gitDir = Join-Path $projectRoot ".git"

$resolvedOutput = if ([System.IO.Path]::IsPathRooted($Output)) {
    $Output
}
else {
    Join-Path $projectRoot $Output
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "docker not found on PATH"
}
if (-not (Test-Path $gitDir)) {
    throw "Not a git checkout: $projectRoot"
}

$commitSha = "unknown"
try {
    $sha = & git -C $projectRoot rev-parse HEAD 2>$null
    if ($LASTEXITCODE -eq 0 -and $sha) {
        $commitSha = "$sha".Trim()
    }
}
catch {
    Write-Note "Could not read git HEAD; stamping MYRIAD_COMMIT_SHA=unknown"
}

$dockerfiles = @{
    backend  = "docker/Dockerfile.backend"
    frontend = "docker/Dockerfile.frontend"
}
$prefix = if ([string]::IsNullOrWhiteSpace($Registry)) { "" } else { "$Registry/" }
$images = @{
    backend  = "${prefix}myriad-backend:$Tag"
    frontend = "${prefix}myriad-frontend:$Tag"
}

$selected = if ($Component -eq "all") { @("backend", "frontend") } else { @($Component) }

foreach ($name in $selected) {
    $dockerfile = $dockerfiles[$name]
    if (-not (Test-Path (Join-Path $projectRoot $dockerfile))) {
        throw "Missing $dockerfile under $projectRoot"
    }
}

Write-Step "Building $($selected -join ', ') as :$Tag ($Platform)"
Write-Note "MYRIAD_VERSION=$Tag  MYRIAD_COMMIT_SHA=$commitSha"

Push-Location $projectRoot
try {
    foreach ($name in $selected) {
        $buildArgs = @(
            "build",
            "--platform", $Platform,
            "-f", $dockerfiles[$name],
            "--build-arg", "MYRIAD_VERSION=$Tag",
            "--build-arg", "MYRIAD_COMMIT_SHA=$commitSha"
        )
        if ($name -eq "backend") {
            $buildArgs += @("--build-arg", "CARGO_PROFILE=$CargoProfile")
        }
        if ($NoCache) {
            $buildArgs += "--no-cache"
        }
        $buildArgs += @("-t", $images[$name], ".")

        Write-Step "docker build ($name)"
        Invoke-Docker -Arguments $buildArgs
        Write-Ok $images[$name]
    }

    if ($Push) {
        foreach ($name in $selected) {
            Write-Step "docker push ($name)"
            Invoke-Docker -Arguments @("push", $images[$name])
        }
    }

    if (-not $NoSave) {
        $outputDir = Split-Path -Parent $resolvedOutput
        if ($outputDir -and -not (Test-Path -LiteralPath $outputDir)) {
            New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
        }
        $refs = $selected | ForEach-Object { $images[$_] }
        $saveArgs = @("save", "-o", $resolvedOutput) + $refs

        Write-Step "docker save -> $resolvedOutput"
        Invoke-Docker -Arguments $saveArgs
        $sizeMb = [math]::Round((Get-Item -LiteralPath $resolvedOutput).Length / 1MB, 1)
        Write-Ok "$resolvedOutput ($sizeMb MB)"
    }
}
finally {
    Pop-Location
}

Write-Step "Done"
$imageList = ($selected | ForEach-Object { $images[$_] }) -join ", "
Write-Host "  Images: $imageList" -ForegroundColor Gray
$tarName = Split-Path -Leaf $resolvedOutput
if (-not $NoSave) {
    Write-Host "  Tar:    $resolvedOutput" -ForegroundColor Gray
}

Write-Host @"

Server steps (repo root, e.g. /opt/myriad):
  docker load -i $tarName
  # .env:
  #   MYRIAD_TAG=$Tag
  #   BACKEND_IMAGE=${prefix}myriad-backend
  #   FRONTEND_IMAGE=${prefix}myriad-frontend
  docker compose up -d
"@ -ForegroundColor Gray
