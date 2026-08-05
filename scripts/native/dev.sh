#!/bin/bash
# =============================================================================
# Myriad Native Dev Script (Docker-free)
# =============================================================================
# Runs the development loop without Docker: a locally installed PostgreSQL plus
# `cargo run` (backend, :1103) and `pnpm dev` (frontend, :1102), with prefixed
# interleaved logs and a single Ctrl-C that takes both down gracefully.
#
# The Docker-based counterpart is scripts/dev/dev.sh, which drives Postgres and
# the updater harness through `docker compose`. Use this script when Docker is
# unavailable, or to develop against the same topology you deploy natively
# (docs/deployment/NATIVE_DEPLOYMENT.md).
#
# Not available here — same gap as a native deploy: the updater service and the
# in-app Update Management page are Docker-only.
#
# Usage:
#   scripts/native/dev.sh [command] [options]
#
# Commands:
#   up          Start backend + frontend in the foreground (default)
#   db-setup    Create the local role + database, then exit
#   doctor      Check toolchain, ports and database, then exit
#
# Options:
#   --backend-only    Run only the backend
#   --frontend-only   Run only the frontend
#   --release         Build/run the backend in release mode
#   --skip-install    Skip `pnpm install` before starting the frontend
#   -h, --help        Show this help
# =============================================================================

set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'
BLUE='\033[0;34m'; MAGENTA='\033[0;35m'; DIM='\033[2m'; NC='\033[0m'
ok()   { echo -e "${GREEN}$1${NC}"; }
info() { echo -e "${CYAN}$1${NC}"; }
warn() { echo -e "${YELLOW}$1${NC}"; }
err()  { echo -e "${RED}$1${NC}" >&2; }

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

BACKEND_DIR="$ROOT/backend"
FRONTEND_DIR="$ROOT/frontend"
BACKEND_PORT=1103
FRONTEND_PORT=1102

# Same credentials as docker-compose.dev.yml, so switching between the Docker
# and native dev paths needs no DATABASE_URL edit.
DEFAULT_DB_URL="postgres://myriad:myriad_dev_password@localhost:5432/myriad"

COMMAND="up"
RUN_BACKEND=1
RUN_FRONTEND=1
CARGO_RELEASE=0
SKIP_INSTALL=0

show_usage() {
    cat <<EOF
Usage: $0 [command] [options]

Commands:
  up               Start backend + frontend in the foreground (default)
  db-setup         Create the local PostgreSQL role + database, then exit
  doctor           Check toolchain, ports and database, then exit

Options:
  --backend-only   Run only the backend  (:$BACKEND_PORT)
  --frontend-only  Run only the frontend (:$FRONTEND_PORT)
  --release        Build/run the backend in release mode
  --skip-install   Skip \`pnpm install\` before starting the frontend
  -h, --help       Show this help

Environment:
  DATABASE_URL         Override the dev database URL
                       (default: backend/.env, else $DEFAULT_DB_URL)
  MYRIAD_PSQL_ADMIN    Superuser psql command used by db-setup
                       (default: auto-detected)

Docker-based equivalent: scripts/dev/dev.sh
Native deployment guide: docs/deployment/NATIVE_DEPLOYMENT.md
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        up|db-setup|doctor) COMMAND="$1" ;;
        --backend-only)  RUN_FRONTEND=0 ;;
        --frontend-only) RUN_BACKEND=0 ;;
        --release)       CARGO_RELEASE=1 ;;
        --skip-install)  SKIP_INSTALL=1 ;;
        -h|--help)       show_usage; exit 0 ;;
        *) err "unknown option: $1"; show_usage; exit 2 ;;
    esac
    shift
done

if [ "$RUN_BACKEND" -eq 0 ] && [ "$RUN_FRONTEND" -eq 0 ]; then
    err "✗ --backend-only and --frontend-only are mutually exclusive"
    exit 2
fi

# --- Database URL ----------------------------------------------------------
# Precedence: explicit env > backend/.env > dev default.
read_env_db_url() {
    [ -f "$BACKEND_DIR/.env" ] || return 0
    # Take the last uncommented DATABASE_URL assignment, strip optional quotes.
    sed -n 's/^[[:space:]]*DATABASE_URL[[:space:]]*=[[:space:]]*//p' "$BACKEND_DIR/.env" \
        | tail -n 1 | sed -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//"
}

DB_URL="${DATABASE_URL:-$(read_env_db_url)}"
[ -n "$DB_URL" ] || DB_URL="$DEFAULT_DB_URL"

DB_USER=""; DB_PASS=""; DB_HOST="localhost"; DB_PORT="5432"; DB_NAME=""
parse_db_url() {
    local re='^postgres(ql)?://([^:@/]+)(:([^@/]*))?@([^:/?]+)(:([0-9]+))?/([^?]+)'
    if [[ "$DB_URL" =~ $re ]]; then
        DB_USER="${BASH_REMATCH[2]}"
        DB_PASS="${BASH_REMATCH[4]}"
        DB_HOST="${BASH_REMATCH[5]}"
        DB_PORT="${BASH_REMATCH[7]:-5432}"
        DB_NAME="${BASH_REMATCH[8]}"
        return 0
    fi
    return 1
}

# --- Checks ----------------------------------------------------------------
have() { command -v "$1" >/dev/null 2>&1; }

port_pids() {
    local port="$1"
    if have lsof; then
        lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null || true
    elif have ss; then
        ss -lptnH "sport = :$port" 2>/dev/null \
            | grep -o 'pid=[0-9]*' | cut -d= -f2 | sort -u || true
    fi
}

# 0 = reachable. Prefers pg_isready; falls back to an actual connect.
db_reachable() {
    if have pg_isready; then
        pg_isready -q -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" >/dev/null 2>&1
    elif have psql; then
        # -w: never prompt. A probe that blocks on a password prompt is worse
        # than one that fails.
        PGPASSWORD="$DB_PASS" psql -w "$DB_URL" -tAc 'SELECT 1' >/dev/null 2>&1
    else
        return 2   # can't tell
    fi
}

check_tools() {
    local missing=0
    if [ "$RUN_BACKEND" -eq 1 ]; then
        if have cargo; then ok "  ✓ cargo    $(cargo --version 2>/dev/null | awk '{print $2}')"
        else err "  ✗ cargo not found — install Rust: https://rustup.rs"; missing=1; fi
    fi
    if [ "$RUN_FRONTEND" -eq 1 ]; then
        if have pnpm; then ok "  ✓ pnpm     $(pnpm --version 2>/dev/null)"
        else err "  ✗ pnpm not found — run 'corepack enable'"; missing=1; fi
        if have node; then ok "  ✓ node     $(node --version 2>/dev/null)"
        else err "  ✗ node not found — Node 20+ required"; missing=1; fi
    fi
    return "$missing"
}

# --- backend/.env ----------------------------------------------------------
# The backend refuses to boot without DATABASE_URL and a >=32 char JWT_SECRET,
# so generate a dev-only .env rather than letting the first run fail.
ensure_backend_env() {
    local env_file="$BACKEND_DIR/.env"
    [ -f "$env_file" ] && return 0

    info "  backend/.env missing — writing a development default"

    local secret=""
    if have openssl; then
        secret="$(openssl rand -base64 48 | tr -d '\n')"
    elif [ -r /dev/urandom ]; then
        secret="$(LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 64)"
    fi
    if [ -z "$secret" ]; then
        err "  ✗ cannot generate JWT_SECRET (no openssl, no /dev/urandom)"
        err "    create $env_file by hand from backend/.env.example"
        return 1
    fi

    # Subshell so the restrictive umask (0600 on the secret) doesn't leak.
    ( umask 077; cat > "$env_file" <<ENV
# Myriad backend — LOCAL DEVELOPMENT ONLY (generated by scripts/native/dev.sh)
# Not a production config: no ENVIRONMENT=production, so CSP/HSTS stay off.
# Full reference: backend/.env.example

DATABASE_URL=$DB_URL

SERVER_HOST=127.0.0.1
SERVER_PORT=$BACKEND_PORT
RUST_LOG=info,myriad_backend=debug

# Dev only: the frontend runs its own server on :$FRONTEND_PORT, so the backend
# serves the API alone until you build the SPA into this path.
FRONTEND_DIST_PATH=../frontend/dist

# Generated locally, never committed (backend/.gitignore covers .env).
JWT_SECRET=$secret

CORS_ORIGINS=http://localhost:$FRONTEND_PORT,http://localhost:$BACKEND_PORT

# Local site URLs (API + SPA). Store install stats are OFF in local dev by default
# so we do not pollute production counters (set TAPP_STORE_STATS_ENABLED=true only
# when deliberately testing the stats edge).
BASE_URL=http://127.0.0.1:$BACKEND_PORT
FRONTEND_URL=http://localhost:$FRONTEND_PORT
TAPP_STORE_STATS_ENABLED=false
ENV
    ) || { err "  ✗ failed to write $env_file"; return 1; }
    ok "  ✓ wrote $env_file (chmod 600, JWT_SECRET generated)"
}

# --- Commands --------------------------------------------------------------
doctor() {
    echo "================================================"
    echo "  Myriad Native Dev — doctor"
    echo "================================================"

    info "Toolchain:"
    local tools_ok=0
    check_tools || tools_ok=1

    if [ "$RUN_BACKEND" -eq 1 ]; then
        echo ""
        info "Database:"
        if ! parse_db_url; then
            err "  ✗ could not parse DATABASE_URL: $DB_URL"
        else
            echo -e "  ${DIM}${DB_USER}@${DB_HOST}:${DB_PORT}/${DB_NAME}${NC}"
            local rc=0
            db_reachable || rc=$?
            case "$rc" in
                0) ok   "  ✓ reachable" ;;
                2) warn "  ? no pg_isready/psql — cannot verify from here" ;;
                *) err  "  ✗ not reachable — start PostgreSQL, or run: $0 db-setup" ;;
            esac
        fi
        echo ""
        info "Config:"
        if [ -f "$BACKEND_DIR/.env" ]; then ok "  ✓ backend/.env present"
        else warn "  ! backend/.env missing — it will be generated on 'up'"; fi
    fi

    echo ""
    info "Ports:"
    local port pids
    for port in $([ "$RUN_BACKEND" -eq 1 ] && echo "$BACKEND_PORT"; \
                  [ "$RUN_FRONTEND" -eq 1 ] && echo "$FRONTEND_PORT"); do
        pids="$(port_pids "$port" | tr '\n' ' ')"
        if [ -n "${pids// /}" ]; then warn "  ! :$port in use by PID(s) ${pids% }"
        else ok "  ✓ :$port free"; fi
    done

    echo ""
    [ "$tools_ok" -eq 0 ] && ok "Ready." || { err "Missing prerequisites (see above)."; return 1; }
}

db_setup() {
    parse_db_url || { err "✗ could not parse DATABASE_URL: $DB_URL"; exit 1; }
    have psql || { err "✗ psql not found — install the PostgreSQL client tools"; exit 1; }

    info "Creating role '$DB_USER' and database '$DB_NAME' on $DB_HOST:$DB_PORT…"

    # Pick a superuser connection: an explicit override, a local 'postgres'
    # role (common on Homebrew/Arch), then sudo -u postgres (Debian/Ubuntu).
    local -a admin
    if [ -n "${MYRIAD_PSQL_ADMIN:-}" ]; then
        read -r -a admin <<< "$MYRIAD_PSQL_ADMIN"
    elif psql -w -U postgres -h "$DB_HOST" -p "$DB_PORT" -tAc 'SELECT 1' >/dev/null 2>&1; then
        # Probed with -w so a password prompt can't hang the detection; the
        # chosen command omits it so a real run may still prompt if needed.
        admin=(psql -U postgres -h "$DB_HOST" -p "$DB_PORT")
    elif have sudo; then
        admin=(sudo -u postgres psql)
    else
        err "✗ no superuser psql route found"
        err "  set MYRIAD_PSQL_ADMIN, e.g. MYRIAD_PSQL_ADMIN='sudo -u postgres psql'"
        exit 1
    fi

    # Idempotent: only creates what's missing. The password is passed as a psql
    # variable and quoted with :'…' so it is never interpolated into SQL text.
    # Locale/encoding mirror docker-compose.dev.yml and NATIVE_DEPLOYMENT.md §7.
    "${admin[@]}" -v ON_ERROR_STOP=1 -v role="$DB_USER" -v pw="$DB_PASS" -v db="$DB_NAME" <<'SQL'
SELECT format('CREATE ROLE %I WITH LOGIN PASSWORD %L', :'role', :'pw')
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = :'role')
\gexec

SELECT format(
    'CREATE DATABASE %I WITH OWNER = %I ENCODING = ''UTF8''
     LC_COLLATE = ''C'' LC_CTYPE = ''C'' TEMPLATE = template0',
    :'db', :'role')
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = :'db')
\gexec
SQL

    ok "✓ database ready — $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"
    info "  Schema migrations run automatically on backend startup."
}

# --- Service supervision ---------------------------------------------------
BACKEND_PID=""
FRONTEND_PID=""
SHUTTING_DOWN=0

# Tag each line so two services can share one terminal.
prefix() {
    local tag="$1" color="$2"
    trap '' INT TERM          # drain the pipe instead of dying mid-shutdown
    while IFS= read -r line; do
        printf '%b%s%b %s\n' "$color" "$tag" "$NC" "$line"
    done
}

# Starts a command in $dir and reports its PID in STARTED_PID.
#
# Deliberately not `PID=$(start_service …)`: inside a command substitution the
# prefixer would inherit the capture pipe as stdout, so the service's output
# would be swallowed and $( ) would block forever waiting for that pipe to
# close. Assigning a global keeps the job a direct child of the script, which
# is also what makes `wait -n` and the shutdown tree walk work.
STARTED_PID=""
start_service() {
    local tag="$1" color="$2" dir="$3"; shift 3
    # exec => the recorded PID is the service itself, not a wrapper subshell
    ( cd "$dir" && exec "$@" ) > >(prefix "$tag" "$color") 2>&1 &
    STARTED_PID=$!
}

alive() { [ -n "$1" ] && kill -0 "$1" 2>/dev/null; }

child_pids() {
    if have pgrep; then
        pgrep -P "$1" 2>/dev/null || true
    else
        ps -axo pid=,ppid= 2>/dev/null | awk -v p="$1" '$2 == p { print $1 }'
    fi
}

# Signal a process and everything below it, leaves first. `cargo run` and
# `pnpm dev` both exec the thing you actually want stopped as a child, and
# neither forwards signals reliably — so walk the tree instead of trusting the
# parent. Explicit PIDs (not `kill -- -0`) keep this safe when the script is
# invoked non-interactively and shares its parent's process group.
kill_tree() {
    local sig="$1" pid="$2" child
    alive "$pid" || return 0
    for child in $(child_pids "$pid"); do
        kill_tree "$sig" "$child"
    done
    kill -"$sig" "$pid" 2>/dev/null || true
}

# One Ctrl-C stops everything. Interactive SIGINT already reaches the whole
# foreground group; re-signalling here is harmless and makes `kill <script>`
# behave identically.
shutdown() {
    [ "$SHUTTING_DOWN" -eq 1 ] && return 0
    SHUTTING_DOWN=1
    trap '' INT TERM
    echo ""
    info "Stopping services…"
    kill_tree TERM "$BACKEND_PID"
    kill_tree TERM "$FRONTEND_PID"

    local waited=0
    while alive "$BACKEND_PID" || alive "$FRONTEND_PID"; do
        sleep 0.2
        waited=$((waited + 1))
        if [ "$waited" -ge 50 ]; then      # 10s grace, then force
            warn "  services did not exit in 10s — sending SIGKILL"
            kill_tree KILL "$BACKEND_PID"
            kill_tree KILL "$FRONTEND_PID"
            break
        fi
    done
    ok "✓ stopped"
}

require_free_port() {
    local port="$1" label="$2" pids
    pids="$(port_pids "$port" | tr '\n' ' ')"
    [ -z "${pids// /}" ] && return 0
    err "✗ port $port ($label) is already in use by PID(s) ${pids% }"
    err "  stop it first:  kill ${pids% }"
    err "  or, if it is a previous Myriad dev run:  scripts/dev/dev.sh stop all"
    return 1
}

up() {
    echo "================================================"
    echo "  Myriad Native Dev"
    echo "================================================"

    info "[1/3] Checking prerequisites…"
    check_tools || exit 1

    if [ "$RUN_BACKEND" -eq 1 ]; then
        require_free_port "$BACKEND_PORT" backend || exit 1
        parse_db_url || { err "✗ could not parse DATABASE_URL: $DB_URL"; exit 1; }
        ensure_backend_env || exit 1

        local rc=0
        db_reachable || rc=$?
        case "$rc" in
            0) ok "  ✓ database reachable ($DB_USER@$DB_HOST:$DB_PORT/$DB_NAME)" ;;
            2) warn "  ! no pg_isready/psql to verify the database — starting anyway" ;;
            *)
                err "  ✗ database unreachable: $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"
                err "    start PostgreSQL, then create the role/db:  $0 db-setup"
                exit 1
                ;;
        esac
    fi
    [ "$RUN_FRONTEND" -eq 1 ] && { require_free_port "$FRONTEND_PORT" frontend || exit 1; }

    if [ "$RUN_FRONTEND" -eq 1 ] && [ "$SKIP_INSTALL" -eq 0 ]; then
        info "[2/3] Installing frontend dependencies…"
        ( cd "$FRONTEND_DIR" && pnpm install ) || exit 1
    else
        info "[2/3] Skipping pnpm install"
    fi

    info "[3/3] Starting services (Ctrl-C to stop)…"
    trap shutdown INT TERM

    if [ "$RUN_BACKEND" -eq 1 ]; then
        local -a cargo_cmd=(cargo run)
        [ "$CARGO_RELEASE" -eq 1 ] && cargo_cmd=(cargo run --release)
        start_service "[backend ]" "$MAGENTA" "$BACKEND_DIR" "${cargo_cmd[@]}"
        BACKEND_PID="$STARTED_PID"
        echo -e "  ${MAGENTA}backend${NC}   http://localhost:$BACKEND_PORT  (pid $BACKEND_PID)"
    fi
    if [ "$RUN_FRONTEND" -eq 1 ]; then
        start_service "[frontend]" "$BLUE" "$FRONTEND_DIR" pnpm run dev
        FRONTEND_PID="$STARTED_PID"
        echo -e "  ${BLUE}frontend${NC}  http://localhost:$FRONTEND_PORT  (pid $FRONTEND_PID)"
    fi
    echo ""

    # Wake on the first child to exit, then take the other one down with it.
    local -a running=()
    [ -n "$BACKEND_PID" ]  && running+=("$BACKEND_PID")
    [ -n "$FRONTEND_PID" ] && running+=("$FRONTEND_PID")

    local status=0
    set +e
    wait -n "${running[@]}"
    status=$?
    set -e

    if [ "$SHUTTING_DOWN" -eq 0 ]; then
        # Name only the service that actually went away, not the one we never started.
        if [ "$RUN_BACKEND" -eq 1 ] && ! alive "$BACKEND_PID"; then
            err "✗ backend exited (status $status)"
        fi
        if [ "$RUN_FRONTEND" -eq 1 ] && ! alive "$FRONTEND_PID"; then
            err "✗ frontend exited (status $status)"
        fi
        shutdown
        exit "$status"
    fi

    shutdown
}

case "$COMMAND" in
    doctor)   doctor ;;
    db-setup) db_setup ;;
    up)       up ;;
esac
