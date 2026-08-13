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
#   db-start    Start the local native PostgreSQL server, then exit
#   db-stop     Stop the local native PostgreSQL server, then exit
#   db-status   Show local PostgreSQL reachability / service status
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
  up               Start backend + frontend in the foreground (default).
                   If DATABASE_URL points at localhost and Postgres is down,
                   tries to start the native server first.
  db-start         Start the local native PostgreSQL server
  db-stop          Stop the local native PostgreSQL server
  db-status        Show whether the DB is reachable and how it is managed
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
  MYRIAD_PG_BREW_FORMULA
                       Homebrew formula to start/stop (e.g. postgresql@18).
                       Auto-detected when unset.
  MYRIAD_PGDATA        PostgreSQL data directory for pg_ctl fallback.
  MYRIAD_PG_CTL        Path to pg_ctl when not on PATH.

Docker-based equivalent: scripts/dev/dev.sh
Native deployment guide: docs/deployment/NATIVE_DEPLOYMENT.md
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        up|db-start|db-stop|db-status|db-setup|doctor) COMMAND="$1" ;;
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

# 0 = reachable, 1 = unreachable, 3 = no client tools to probe with.
# NOTE: do not forward pg_isready's raw exit codes — it uses 2 for "no
# response", which must not be confused with "can't tell".
db_reachable() {
    if have pg_isready; then
        if pg_isready -q -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" >/dev/null 2>&1; then
            return 0
        fi
        return 1
    elif have psql; then
        # -w: never prompt. A probe that blocks on a password prompt is worse
        # than one that fails.
        if PGPASSWORD="$DB_PASS" psql -w "$DB_URL" -tAc 'SELECT 1' >/dev/null 2>&1; then
            return 0
        fi
        return 1
    else
        return 3   # can't tell
    fi
}

# True when DATABASE_URL targets this machine — only then may we start/stop the
# OS PostgreSQL service. Remote hosts are never touched.
db_is_local_host() {
    case "$DB_HOST" in
        localhost|127.0.0.1|::1|0.0.0.0|"") return 0 ;;
        *) return 1 ;;
    esac
}

# --- Native PostgreSQL lifecycle --------------------------------------------
# Prefer Homebrew services (macOS), then systemd (Linux packages), then pg_ctl
# with an explicit/auto data directory. Never drive Docker from this script.

PG_BACKEND=""          # brew | systemd | pg_ctl | ""
PG_BREW_FORMULA=""
PG_CTL_BIN=""
PG_DATA_DIR=""
PG_SYSTEMD_UNIT=""

detect_pg_backend() {
    [ -n "$PG_BACKEND" ] && return 0

    # Explicit brew formula wins.
    if [ -n "${MYRIAD_PG_BREW_FORMULA:-}" ] && have brew; then
        PG_BACKEND="brew"
        PG_BREW_FORMULA="$MYRIAD_PG_BREW_FORMULA"
        return 0
    fi

    # Homebrew: pick an installed postgresql@N / postgresql formula. Prefer one
    # already marked started in `brew services list`, else the highest version.
    if have brew; then
        local list formula started="" candidates="" f ver best="" best_ver=-1
        list="$(brew services list 2>/dev/null || true)"
        # shellcheck disable=SC2013
        for formula in $(brew list --formula 2>/dev/null | grep -E '^postgresql(@[0-9]+)?$' || true); do
            candidates="$candidates $formula"
            if printf '%s\n' "$list" | awk -v f="$formula" '$1 == f && $2 == "started" { found=1 } END { exit !found }'; then
                started="$formula"
            fi
            ver="${formula#postgresql@}"
            [ "$ver" = "$formula" ] && ver=0
            if [[ "$ver" =~ ^[0-9]+$ ]] && [ "$ver" -gt "$best_ver" ]; then
                best_ver="$ver"
                best="$formula"
            fi
        done
        if [ -n "$started" ]; then
            PG_BACKEND="brew"
            PG_BREW_FORMULA="$started"
            return 0
        fi
        if [ -n "$best" ]; then
            PG_BACKEND="brew"
            PG_BREW_FORMULA="$best"
            return 0
        fi
        # Formula present via opt path even if brew list is odd.
        for f in postgresql@18 postgresql@17 postgresql@16 postgresql; do
            if [ -d "/opt/homebrew/opt/$f" ] || [ -d "/usr/local/opt/$f" ]; then
                PG_BACKEND="brew"
                PG_BREW_FORMULA="$f"
                return 0
            fi
        done
    fi

    # systemd unit (Debian/Ubuntu/Fedora packages).
    if have systemctl; then
        local unit
        for unit in postgresql postgresql@16-main postgresql@17-main postgresql@18-main \
                    postgresql-16 postgresql-17 postgresql-18; do
            if systemctl cat "$unit" >/dev/null 2>&1; then
                PG_BACKEND="systemd"
                PG_SYSTEMD_UNIT="$unit"
                return 0
            fi
        done
        # cluster alias used by some distros
        if systemctl list-unit-files 'postgresql*' 2>/dev/null | grep -q .; then
            PG_BACKEND="systemd"
            PG_SYSTEMD_UNIT="postgresql"
            return 0
        fi
    fi

    # pg_ctl + data directory.
    if [ -n "${MYRIAD_PG_CTL:-}" ] && [ -x "${MYRIAD_PG_CTL}" ]; then
        PG_CTL_BIN="$MYRIAD_PG_CTL"
    elif have pg_ctl; then
        PG_CTL_BIN="$(command -v pg_ctl)"
    else
        local cand
        for cand in \
            /opt/homebrew/opt/postgresql@18/bin/pg_ctl \
            /opt/homebrew/opt/postgresql@17/bin/pg_ctl \
            /opt/homebrew/opt/postgresql@16/bin/pg_ctl \
            /opt/homebrew/opt/postgresql/bin/pg_ctl \
            /usr/lib/postgresql/18/bin/pg_ctl \
            /usr/lib/postgresql/17/bin/pg_ctl \
            /usr/lib/postgresql/16/bin/pg_ctl \
            /usr/local/opt/postgresql@18/bin/pg_ctl \
            /usr/local/opt/postgresql/bin/pg_ctl
        do
            if [ -x "$cand" ]; then PG_CTL_BIN="$cand"; break; fi
        done
    fi

    if [ -n "${MYRIAD_PGDATA:-}" ]; then
        PG_DATA_DIR="$MYRIAD_PGDATA"
    else
        local d
        for d in \
            /opt/homebrew/var/postgresql@18 \
            /opt/homebrew/var/postgresql@17 \
            /opt/homebrew/var/postgresql@16 \
            /opt/homebrew/var/postgres \
            /usr/local/var/postgresql@18 \
            /usr/local/var/postgres \
            /var/lib/postgresql/data \
            "$HOME/.local/share/postgresql/data"
        do
            if [ -d "$d" ] && [ -f "$d/PG_VERSION" ]; then
                PG_DATA_DIR="$d"
                break
            fi
        done
    fi

    if [ -n "$PG_CTL_BIN" ] && [ -n "$PG_DATA_DIR" ]; then
        PG_BACKEND="pg_ctl"
        return 0
    fi

    PG_BACKEND=""
    return 1
}

pg_backend_label() {
    detect_pg_backend || true
    case "$PG_BACKEND" in
        brew)    echo "brew services ($PG_BREW_FORMULA)" ;;
        systemd) echo "systemd ($PG_SYSTEMD_UNIT)" ;;
        pg_ctl)  echo "pg_ctl -D $PG_DATA_DIR" ;;
        *)       echo "none detected" ;;
    esac
}

wait_db_ready() {
    local timeout_s="${1:-30}" waited=0 rc
    while [ "$waited" -lt "$timeout_s" ]; do
        rc=0
        db_reachable || rc=$?
        # rc 0 = up; rc 3 = no client tools — treat port listen as success below
        if [ "$rc" -eq 0 ]; then return 0; fi
        if [ "$rc" -eq 3 ]; then
            # Fall back: something accepting on the DB port.
            if [ -n "$(port_pids "$DB_PORT" | head -n 1)" ]; then return 0; fi
        fi
        sleep 0.5
        waited=$((waited + 1))
    done
    return 1
}

db_start() {
    parse_db_url || { err "✗ could not parse DATABASE_URL: $DB_URL"; return 1; }

    if ! db_is_local_host; then
        err "✗ refusing to start PostgreSQL: DATABASE_URL host is '$DB_HOST' (not local)"
        err "  db-start/db-stop only manage a native server on this machine"
        return 1
    fi

    local rc=0
    db_reachable || rc=$?
    if [ "$rc" -eq 0 ]; then
        ok "✓ PostgreSQL already reachable ($DB_USER@$DB_HOST:$DB_PORT/$DB_NAME)"
        return 0
    fi

    if ! detect_pg_backend; then
        err "✗ no native PostgreSQL manager found (brew / systemd / pg_ctl)"
        err "  install Postgres, or start it yourself, then re-run"
        err "  override: MYRIAD_PG_BREW_FORMULA / MYRIAD_PGDATA / MYRIAD_PG_CTL"
        return 1
    fi

    info "Starting PostgreSQL via $(pg_backend_label)…"
    case "$PG_BACKEND" in
        brew)
            brew services start "$PG_BREW_FORMULA" || {
                err "✗ brew services start $PG_BREW_FORMULA failed"
                return 1
            }
            ;;
        systemd)
            if have sudo; then
                sudo systemctl start "$PG_SYSTEMD_UNIT" || {
                    err "✗ systemctl start $PG_SYSTEMD_UNIT failed"
                    return 1
                }
            else
                systemctl --user start "$PG_SYSTEMD_UNIT" 2>/dev/null \
                    || systemctl start "$PG_SYSTEMD_UNIT" || {
                    err "✗ systemctl start $PG_SYSTEMD_UNIT failed (try with sudo)"
                    return 1
                }
            fi
            ;;
        pg_ctl)
            "$PG_CTL_BIN" -D "$PG_DATA_DIR" -l "$PG_DATA_DIR/myriad-pg.log" start || {
                err "✗ pg_ctl start failed (log: $PG_DATA_DIR/myriad-pg.log)"
                return 1
            }
            ;;
    esac

    if wait_db_ready 40; then
        ok "✓ PostgreSQL started — $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"
        return 0
    fi

    err "✗ PostgreSQL was launched but is not accepting connections on $DB_HOST:$DB_PORT"
    err "  check: $(pg_backend_label)"
    return 1
}

db_stop() {
    parse_db_url || { err "✗ could not parse DATABASE_URL: $DB_URL"; return 1; }

    if ! db_is_local_host; then
        err "✗ refusing to stop PostgreSQL: DATABASE_URL host is '$DB_HOST' (not local)"
        return 1
    fi

    if ! detect_pg_backend; then
        # Still try to report whether something is listening.
        if [ -n "$(port_pids "$DB_PORT" | head -n 1)" ]; then
            err "✗ PostgreSQL appears to be listening on :$DB_PORT but no manager was detected"
            err "  stop it manually, or set MYRIAD_PG_BREW_FORMULA / MYRIAD_PGDATA"
            return 1
        fi
        warn "! no native PostgreSQL manager found and nothing is listening on :$DB_PORT"
        return 0
    fi

    info "Stopping PostgreSQL via $(pg_backend_label)…"
    case "$PG_BACKEND" in
        brew)
            brew services stop "$PG_BREW_FORMULA" || {
                err "✗ brew services stop $PG_BREW_FORMULA failed"
                return 1
            }
            ;;
        systemd)
            if have sudo; then
                sudo systemctl stop "$PG_SYSTEMD_UNIT" || {
                    err "✗ systemctl stop $PG_SYSTEMD_UNIT failed"
                    return 1
                }
            else
                systemctl --user stop "$PG_SYSTEMD_UNIT" 2>/dev/null \
                    || systemctl stop "$PG_SYSTEMD_UNIT" || {
                    err "✗ systemctl stop $PG_SYSTEMD_UNIT failed (try with sudo)"
                    return 1
                }
            fi
            ;;
        pg_ctl)
            # Only stop if this data dir is the one running — avoid killing a
            # foreign cluster that happens to share the port.
            if ! "$PG_CTL_BIN" -D "$PG_DATA_DIR" status >/dev/null 2>&1; then
                warn "! pg_ctl reports cluster not running at $PG_DATA_DIR"
                return 0
            fi
            "$PG_CTL_BIN" -D "$PG_DATA_DIR" -m fast stop || {
                err "✗ pg_ctl stop failed"
                return 1
            }
            ;;
    esac

    # Wait until the port is free / not accepting (best-effort).
    local waited=0
    while [ "$waited" -lt 30 ]; do
        if ! db_reachable 2>/dev/null; then
            # db_reachable returns non-zero when down — good. But rc=3 means
            # unknown; also require the listen port to be gone when we can tell.
            if [ -z "$(port_pids "$DB_PORT" | head -n 1)" ]; then
                ok "✓ PostgreSQL stopped"
                return 0
            fi
        fi
        sleep 0.5
        waited=$((waited + 1))
    done

    # Some installs keep the process registered but closed connections slowly.
    if db_reachable 2>/dev/null; then
        err "✗ PostgreSQL still reachable on $DB_HOST:$DB_PORT after stop"
        return 1
    fi
    ok "✓ PostgreSQL stop issued ($(pg_backend_label))"
}

db_status() {
    parse_db_url || { err "✗ could not parse DATABASE_URL: $DB_URL"; return 1; }

    echo "================================================"
    echo "  Myriad Native Dev — database"
    echo "================================================"
    echo -e "  ${DIM}url host${NC}  ${DB_USER}@${DB_HOST}:${DB_PORT}/${DB_NAME}"
    if db_is_local_host; then
        ok "  ✓ host is local — start/stop are allowed"
    else
        warn "  ! host is remote — db-start/db-stop will refuse"
    fi

    detect_pg_backend || true
    echo -e "  ${DIM}manager${NC}  $(pg_backend_label)"

    local rc=0
    db_reachable || rc=$?
    case "$rc" in
        0) ok   "  ✓ reachable" ;;
        3) warn "  ? no pg_isready/psql — cannot verify connectivity" ;;
        *) err  "  ✗ not reachable" ;;
    esac

    local pids
    pids="$(port_pids "$DB_PORT" | tr '\n' ' ')"
    if [ -n "${pids// /}" ]; then
        echo -e "  ${DIM}listen${NC}   :$DB_PORT PID(s) ${pids% }"
    else
        echo -e "  ${DIM}listen${NC}   :$DB_PORT free"
    fi

    if [ "$PG_BACKEND" = "brew" ] && have brew; then
        local line
        line="$(brew services list 2>/dev/null | awk -v f="$PG_BREW_FORMULA" '$1 == f { print; exit }')"
        [ -n "$line" ] && echo -e "  ${DIM}brew${NC}     $line"
    fi
    if [ "$PG_BACKEND" = "systemd" ] && have systemctl; then
        systemctl is-active --quiet "$PG_SYSTEMD_UNIT" 2>/dev/null \
            && ok "  ✓ systemd unit active" \
            || warn "  ! systemd unit inactive"
    fi
    if [ "$PG_BACKEND" = "pg_ctl" ] && [ -n "$PG_CTL_BIN" ] && [ -n "$PG_DATA_DIR" ]; then
        if "$PG_CTL_BIN" -D "$PG_DATA_DIR" status >/dev/null 2>&1; then
            ok "  ✓ pg_ctl cluster running"
        else
            warn "  ! pg_ctl cluster not running"
        fi
    fi
    echo ""
    [ "$rc" -eq 0 ]
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
            detect_pg_backend || true
            if db_is_local_host; then
                echo -e "  ${DIM}manager  $(pg_backend_label)${NC}"
            else
                echo -e "  ${DIM}manager  skipped (remote host)${NC}"
            fi
            local rc=0
            db_reachable || rc=$?
            case "$rc" in
                0) ok   "  ✓ reachable" ;;
                3) warn "  ? no pg_isready/psql — cannot verify from here" ;;
                *)
                    err  "  ✗ not reachable"
                    if db_is_local_host; then
                        err  "    try: $0 db-start   then   $0 db-setup"
                    else
                        err  "    start the remote PostgreSQL or fix DATABASE_URL"
                    fi
                    ;;
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

    # Bring a local server up first when we can manage it.
    local rc=0
    db_reachable || rc=$?
    if [ "$rc" -ne 0 ] && [ "$rc" -ne 3 ] && db_is_local_host; then
        info "PostgreSQL not reachable — attempting db-start…"
        db_start || exit 1
    elif [ "$rc" -ne 0 ] && [ "$rc" -ne 3 ]; then
        err "✗ database unreachable: $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"
        exit 1
    fi

    info "Creating role '$DB_USER' and database '$DB_NAME' on $DB_HOST:$DB_PORT…"

    # Pick a superuser connection, in order:
    #   1. explicit MYRIAD_PSQL_ADMIN override
    #   2. peer/trust as the current OS user (Homebrew Postgres on macOS —
    #      the installing user is the cluster superuser; there is often no
    #      'postgres' OS account)
    #   3. a local 'postgres' role over TCP (common on Arch / some brew setups)
    #   4. sudo -u postgres (Debian/Ubuntu packages)
    local -a admin
    if [ -n "${MYRIAD_PSQL_ADMIN:-}" ]; then
        read -r -a admin <<< "$MYRIAD_PSQL_ADMIN"
    elif psql -w -d postgres -tAc 'SELECT 1' >/dev/null 2>&1; then
        admin=(psql -d postgres)
    elif psql -w -U postgres -h "$DB_HOST" -p "$DB_PORT" -d postgres -tAc 'SELECT 1' >/dev/null 2>&1; then
        # Probed with -w so a password prompt can't hang the detection; the
        # chosen command omits it so a real run may still prompt if needed.
        admin=(psql -U postgres -h "$DB_HOST" -p "$DB_PORT" -d postgres)
    elif have sudo && sudo -n -u postgres true >/dev/null 2>&1 \
         && sudo -u postgres psql -d postgres -tAc 'SELECT 1' >/dev/null 2>&1; then
        admin=(sudo -u postgres psql -d postgres)
    elif have sudo; then
        # May prompt for a password; last resort on Debian/Ubuntu.
        admin=(sudo -u postgres psql -d postgres)
    else
        err "✗ no superuser psql route found"
        err "  set MYRIAD_PSQL_ADMIN, e.g. MYRIAD_PSQL_ADMIN='psql -d postgres'"
        err "  or on Debian/Ubuntu: MYRIAD_PSQL_ADMIN='sudo -u postgres psql'"
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
FRONTEND_LOG_PID=""
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

# Astro 7 backgrounds the real dev server (non-TTY or after spawn) and the
# launcher (`pnpm`/`astro`) exits 0 immediately. Track the process that is
# actually listening on FRONTEND_PORT instead of the short-lived launcher.
wait_for_listen_pid() {
    local port="$1" timeout_s="${2:-30}" waited=0 pid
    while [ "$waited" -lt "$timeout_s" ]; do
        pid="$(port_pids "$port" | head -n 1 | tr -d '[:space:]')"
        if [ -n "$pid" ]; then
            echo "$pid"
            return 0
        fi
        sleep 0.2
        waited=$((waited + 1))
    done
    return 1
}

stop_frontend() {
    # Prefer Astro's own stop so it clears its lock/pid file cleanly.
    if [ -x "$FRONTEND_DIR/node_modules/.bin/astro" ]; then
        ( cd "$FRONTEND_DIR" && ./node_modules/.bin/astro dev stop ) >/dev/null 2>&1 || true
    fi
    kill_tree TERM "$FRONTEND_LOG_PID"
    kill_tree TERM "$FRONTEND_PID"
    # Free the port in case a detached node lingered past astro stop.
    local p
    for p in $(port_pids "$FRONTEND_PORT"); do
        kill_tree TERM "$p"
    done
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
    stop_frontend

    local waited=0
    while alive "$BACKEND_PID" || alive "$FRONTEND_PID" || alive "$FRONTEND_LOG_PID"; do
        sleep 0.2
        waited=$((waited + 1))
        if [ "$waited" -ge 50 ]; then      # 10s grace, then force
            warn "  services did not exit in 10s — sending SIGKILL"
            kill_tree KILL "$BACKEND_PID"
            kill_tree KILL "$FRONTEND_LOG_PID"
            kill_tree KILL "$FRONTEND_PID"
            local p
            for p in $(port_pids "$FRONTEND_PORT"); do
                kill_tree KILL "$p"
            done
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
            3) warn "  ! no pg_isready/psql to verify the database — starting anyway" ;;
            *)
                if db_is_local_host; then
                    info "  database down — starting native PostgreSQL…"
                    if db_start; then
                        ok "  ✓ database reachable ($DB_USER@$DB_HOST:$DB_PORT/$DB_NAME)"
                    else
                        err "  ✗ database unreachable: $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"
                        err "    fix Postgres, then:  $0 db-start && $0 db-setup"
                        exit 1
                    fi
                else
                    err "  ✗ database unreachable: $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"
                    err "    remote host — start that PostgreSQL yourself (db-start only manages localhost)"
                    exit 1
                fi
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
        # Astro 7 detaches the real server; start it, then re-attach to the
        # listener PID and stream logs with `astro dev logs --follow`.
        start_service "[frontend]" "$BLUE" "$FRONTEND_DIR" pnpm run dev
        local launcher_pid="$STARTED_PID"
        local listen_pid=""
        if listen_pid="$(wait_for_listen_pid "$FRONTEND_PORT" 60)"; then
            FRONTEND_PID="$listen_pid"
            # Drain the short-lived launcher so it doesn't sit as a zombie.
            wait "$launcher_pid" 2>/dev/null || true
            # Follow Astro's background log stream (blocks until stop).
            start_service "[frontend]" "$BLUE" "$FRONTEND_DIR" \
                ./node_modules/.bin/astro dev logs --follow
            FRONTEND_LOG_PID="$STARTED_PID"
            echo -e "  ${BLUE}frontend${NC}  http://localhost:$FRONTEND_PORT  (pid $FRONTEND_PID)"
        else
            err "✗ frontend did not bind :$FRONTEND_PORT within 60s"
            wait "$launcher_pid" 2>/dev/null || true
            shutdown
            exit 1
        fi
    fi
    echo ""

    # Wake when the first supervised process exits, then take the others down.
    # Prefer bash 4.3+ `wait -n` when available; fall back to a poll loop for
    # macOS /bin/bash 3.2 which rejects `wait -n` as an invalid option.
    # FRONTEND_PID is often not our direct child (Astro-detached node), so the
    # poll path also checks it with kill -0; wait -n only covers direct children.
    local status=0
    set +e
    if [ -n "${BASH_VERSINFO:-}" ] && {
           [ "${BASH_VERSINFO[0]}" -gt 4 ] ||
           { [ "${BASH_VERSINFO[0]}" -eq 4 ] && [ "${BASH_VERSINFO[1]}" -ge 3 ]; }
       }; then
        local -a running=()
        [ -n "$BACKEND_PID" ]      && running+=("$BACKEND_PID")
        [ -n "$FRONTEND_LOG_PID" ] && running+=("$FRONTEND_LOG_PID")
        # Poll loop still needed for detached FRONTEND_PID; race wait -n with poll.
        (
            while [ "$SHUTTING_DOWN" -eq 0 ]; do
                if [ -n "$FRONTEND_PID" ] && ! alive "$FRONTEND_PID"; then
                    exit 91
                fi
                sleep 0.5
            done
        ) &
        local poll_pid=$!
        wait -n "${running[@]}" "$poll_pid"
        status=$?
        kill "$poll_pid" 2>/dev/null || true
        wait "$poll_pid" 2>/dev/null || true
    else
        while [ "$SHUTTING_DOWN" -eq 0 ]; do
            if [ -n "$BACKEND_PID" ] && ! alive "$BACKEND_PID"; then
                wait "$BACKEND_PID" 2>/dev/null
                status=$?
                break
            fi
            if [ -n "$FRONTEND_PID" ] && ! alive "$FRONTEND_PID"; then
                status=0
                break
            fi
            if [ -n "$FRONTEND_LOG_PID" ] && ! alive "$FRONTEND_LOG_PID"; then
                wait "$FRONTEND_LOG_PID" 2>/dev/null
                status=$?
                break
            fi
            sleep 0.5
        done
    fi
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
    doctor)    doctor ;;
    db-start)  db_start ;;
    db-stop)   db_stop ;;
    db-status) db_status ;;
    db-setup)  db_setup ;;
    up)        up ;;
esac
