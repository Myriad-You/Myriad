#!/bin/bash
# =============================================================================
# Myriad Native Assemble Script (Docker-free)
# =============================================================================
# Builds the backend + frontend and assembles a self-contained deploy bundle
# into ./Upload, matching the runtime layout documented in
# docs/deployment/NATIVE_DEPLOYMENT.md.
#
# Bundle contents (./Upload):
#   myriad-backend     Release binary (serves /api/* + the static SPA)
#   frontend/dist/     Built static SPA
#   data/              Git-tracked agent seed files (SOUL.md / USER.md / ...)
#   .env.example       Production env template — fill in and rename to .env
#   myriad.service     systemd unit template (edit paths/user if needed)
#   DEPLOY.md          Copy of the native deployment guide
#   VERSION            Stamped build version (also baked into /health)
#
# Usage:
#   scripts/extra/assemble.sh [--skip-build] [--tar] [--version <v>] [-h]
#
#   --skip-build   Reuse existing target/release binary + frontend/dist
#   --tar          Also produce ./myriad-native-<version>.tar.gz
#   --version <v>  Override the version stamp (default: git describe)
# =============================================================================

set -euo pipefail

# Keep the release artifact path while avoiding fat-LTO's long serial tail.
# Callers can override either value for final benchmarking/release builds.
: "${CARGO_PROFILE_RELEASE_LTO:=thin}"
: "${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:=16}"
export CARGO_PROFILE_RELEASE_LTO CARGO_PROFILE_RELEASE_CODEGEN_UNITS

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
ok()   { echo -e "${GREEN}$1${NC}"; }
info() { echo -e "${CYAN}$1${NC}"; }
warn() { echo -e "${YELLOW}$1${NC}"; }
err()  { echo -e "${RED}$1${NC}"; }

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

UPLOAD="$ROOT/Upload"
SKIP_BUILD=0
MAKE_TAR=0
VERSION=""

show_usage() {
    cat <<EOF
Usage: $0 [options]

Options:
  --skip-build     Skip cargo/pnpm builds; assemble from existing artifacts
  --tar            Also create ./myriad-native-<version>.tar.gz
  --version <v>    Version stamp (default: \`git describe --tags --always --dirty\`)
  -h, --help       Show this help

Rust release profile overrides:
  CARGO_PROFILE_RELEASE_LTO            LTO mode (default: thin)
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS  Parallel codegen units (default: 16)

Output: ./Upload  (see docs/deployment/NATIVE_DEPLOYMENT.md)
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --skip-build|--no-build) SKIP_BUILD=1 ;;
        --tar) MAKE_TAR=1 ;;
        --version) shift; VERSION="${1:-}"; [ -n "$VERSION" ] || { err "--version needs a value"; exit 2; } ;;
        -h|--help) show_usage; exit 0 ;;
        *) err "unknown option: $1"; show_usage; exit 2 ;;
    esac
    shift
done

# --- Resolve version -------------------------------------------------------
if [ -z "$VERSION" ]; then
    if git rev-parse --git-dir >/dev/null 2>&1; then
        VERSION="$(git describe --tags --always --dirty 2>/dev/null || true)"
    fi
    if [ -z "$VERSION" ]; then
        VERSION="v$(grep -m1 '^version' backend/Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
    fi
fi

echo "================================================"
echo "  Myriad Native Assemble"
echo "  version: $VERSION"
echo "  output:  $UPLOAD"
echo "================================================"

BIN="$ROOT/target/release/myriad-backend"
DIST="$ROOT/frontend/dist"

# --- Build -----------------------------------------------------------------
if [ "$SKIP_BUILD" -eq 0 ]; then
    info "[1/4] Building backend (release, LTO=$CARGO_PROFILE_RELEASE_LTO, codegen-units=$CARGO_PROFILE_RELEASE_CODEGEN_UNITS)…"
    command -v cargo >/dev/null 2>&1 || { err "✗ cargo not found — install Rust (https://rustup.rs)"; exit 1; }
    MYRIAD_VERSION="$VERSION" cargo build -p myriad-backend --release
    ok "✓ backend built"

    info "[2/4] Building frontend (pnpm build)…"
    command -v pnpm >/dev/null 2>&1 || { err "✗ pnpm not found — run 'corepack enable'"; exit 1; }
    # PUBLIC_API_URL empty  -> SPA calls the API same-origin via relative /api/*
    # HUSKY=0               -> don't trip git hooks during CI-style installs
    # --trust-lockfile      -> the lockfile is committed and reviewed, so don't
    #                          re-apply minimumReleaseAge / trustPolicy to every
    #                          entry at assemble time. Without it a routine dep
    #                          bump blocks the build for 24h (pnpm quarantines
    #                          freshly published versions), and no-downgrade
    #                          misfires on maintenance branches. Vet the lockfile
    #                          on the way in — `pnpm install` in frontend/ still
    #                          enforces both policies for day-to-day work.
    ( cd frontend && HUSKY=0 pnpm install --frozen-lockfile --trust-lockfile \
        && HUSKY=0 PUBLIC_API_URL="" PUBLIC_MYRIAD_VERSION="$VERSION" pnpm run build )
    ok "✓ frontend built"
else
    warn "[1-2/4] --skip-build: reusing existing artifacts"
    warn "         NOTE: frontend/dist is shipped as-is. Ensure it was built with"
    warn "         PUBLIC_API_URL empty and no other PUBLIC_* secrets — those are"
    warn "         inlined into the client bundle at build time and cannot be undone here."
fi

[ -f "$BIN" ]  || { err "✗ missing $BIN (build first, or drop --skip-build)"; exit 1; }
[ -d "$DIST" ] || { err "✗ missing $DIST (build first, or drop --skip-build)"; exit 1; }

# Refuse to assemble a mixed-version bundle. This also catches --skip-build
# accidentally reusing artifacts from an older checkout.
if ! LC_ALL=C grep -aFq -- "$VERSION" "$BIN"; then
    err "✗ backend artifact does not contain version stamp $VERSION: $BIN"
    err "  rebuild without --skip-build, or pass --version matching the artifact"
    exit 1
fi

if ! grep -Fq -- "name=\"myriad-version\" content=\"$VERSION\"" "$DIST/index.html"; then
    err "✗ frontend artifact version does not match $VERSION: $DIST/index.html"
    err "  rebuild without --skip-build, or pass --version matching the artifact"
    exit 1
fi

# --- Assemble ./Upload -----------------------------------------------------
info "[3/4] Assembling bundle…"
rm -rf "$UPLOAD"
mkdir -p "$UPLOAD/frontend"

# Binary
install -m 0755 "$BIN" "$UPLOAD/myriad-backend"

# Static SPA
cp -r "$DIST" "$UPLOAD/frontend/dist"

# Agent seed data: ship exactly the git-tracked files so we never bundle a
# developer's local runtime state (downloaded icons, agent memory, tapps, …).
if git rev-parse --git-dir >/dev/null 2>&1; then
    while IFS= read -r -d '' f; do
        dest="$UPLOAD/${f#backend/}"      # backend/data/agent/SOUL.md -> data/agent/SOUL.md
        mkdir -p "$(dirname "$dest")"
        cp "$f" "$dest"
    done < <(git ls-files -z backend/data)
else
    # Without git we can't tell tracked seed files from local runtime state, so
    # copy only the known seed dirs and skip everything else (tapps, downloaded
    # icons, agent memory) to avoid bundling a developer's private runtime data.
    warn "not a git repo — copying known seed dirs only (agent, brew); skipping runtime state"
    mkdir -p "$UPLOAD/data"
    for seed in agent brew; do
        [ -e "backend/data/$seed" ] && cp -r "backend/data/$seed" "$UPLOAD/data/$seed"
    done
fi

# Version marker
printf '%s\n' "$VERSION" > "$UPLOAD/VERSION"

# Deployment guide (single source of truth, just copied in for offline use)
cp docs/deployment/NATIVE_DEPLOYMENT.md "$UPLOAD/DEPLOY.md"

# Production .env template (mirrors DEPLOY.md §8)
cat > "$UPLOAD/.env.example" <<'ENV'
# Myriad native deployment — copy to .env and fill in.  chmod 600 .env
# Full reference: DEPLOY.md §8

# ---- Database (REQUIRED) ----
DATABASE_URL=postgres://myriad:CHANGE_ME_STRONG_PASSWORD@localhost:5432/myriad

# ---- Runtime mode (REQUIRED for production security) ----
# Turns on the strict security posture: CSP + HSTS headers, CORS_ORIGINS is
# enforced, and a weak JWT_SECRET hard-fails startup. Leave this unset only for
# local development — a public install without it runs with CSP/HSTS disabled.
ENVIRONMENT=production

# ---- Server ----
SERVER_HOST=127.0.0.1
SERVER_PORT=1103
# Absolute path to the SPA shipped in this bundle:
FRONTEND_DIST_PATH=/opt/myriad/frontend/dist

# ---- Security (REQUIRED, >= 32 chars) ----
# Generate: openssl rand -base64 48
# The placeholder below is REJECTED at startup on purpose — replace it or the
# backend refuses to boot (fail-closed, so a known key never ships to prod).
JWT_SECRET=CHANGEME_generate_via_openssl_rand_base64_48

# ---- CORS / public URLs ----
CORS_ORIGINS=https://yourdomain.com
BASE_URL=https://yourdomain.com
FRONTEND_URL=https://yourdomain.com

# ---- Logging ----
RUST_LOG=info
ENV

# systemd unit template (mirrors DEPLOY.md §9)
cat > "$UPLOAD/myriad.service" <<'UNIT'
[Unit]
Description=Myriad backend (API + static frontend)
After=network-online.target postgresql.service
Wants=network-online.target
Requires=postgresql.service

[Service]
Type=exec
User=myriad
Group=myriad
WorkingDirectory=/opt/myriad
ExecStart=/opt/myriad/myriad-backend
Restart=on-failure
RestartSec=5s

NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
# .env is listed explicitly: the setup wizard and in-app config saves rewrite it,
# and ProtectSystem=strict makes everything outside these paths read-only.
ReadWritePaths=/opt/myriad/.env /opt/myriad/data /opt/myriad/cache

[Install]
WantedBy=multi-user.target
UNIT

ok "✓ bundle assembled"

# --- Optional tarball ------------------------------------------------------
if [ "$MAKE_TAR" -eq 1 ]; then
    info "[4/4] Creating tarball…"
    TAR="$ROOT/myriad-native-${VERSION}.tar.gz"
    tar -C "$UPLOAD" -czf "$TAR" .
    ok "✓ $(basename "$TAR")  ($(du -h "$TAR" | cut -f1))"
else
    info "[4/4] Skipping tarball (pass --tar to enable)"
fi

# --- Summary ---------------------------------------------------------------
echo ""
echo "================================================"
ok   "  Assemble complete — $VERSION"
echo "================================================"
echo -e "Bundle:  ${CYAN}${UPLOAD}${NC}  ($(du -sh "$UPLOAD" | cut -f1))"
echo ""
echo "Next steps:"
echo -e "  1. Upload ./Upload to the server, e.g.  ${CYAN}rsync -a ./Upload/ user@host:/opt/myriad/${NC}"
echo -e "  2. On the server: cp .env.example .env  and fill it in (chmod 600 .env)"
echo -e "  3. Install myriad.service, then  ${CYAN}systemctl enable --now myriad${NC}"
echo -e "  Full guide: ${CYAN}DEPLOY.md${NC} (docs/deployment/NATIVE_DEPLOYMENT.md)"
