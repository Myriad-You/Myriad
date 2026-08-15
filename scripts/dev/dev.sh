#!/bin/bash
# ============================================
# Myriad Development Script (Linux/macOS)
# ============================================
# Single entry for local development. Default is a local PostgreSQL plus
# cargo / pnpm in this terminal. Pass --docker to use compose postgres.
# Usage: ./dev.sh [command] [service] [options]
#        run without args for the interactive menu

set -e

# ==================== Colors & Styles ====================
BOLD='\033[1m'
DIM='\033[2m'

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
WHITE='\033[0;37m'

# Bright Colors
BRIGHT_RED='\033[1;31m'
BRIGHT_GREEN='\033[1;32m'
BRIGHT_YELLOW='\033[1;33m'
BRIGHT_CYAN='\033[1;36m'
BRIGHT_WHITE='\033[1;37m'

NC='\033[0m' # Reset

# ==================== Project Config ====================
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="1.0.0"
DEV_UPDATER_DIR="$PROJECT_ROOT/.dev-updater"
DEV_UPDATER_TOKEN_DEFAULT="9xQ3vN8mP2rT5wY7zA1bC4dF6hJ8kL0n"
BACKEND_PORT=1103
FRONTEND_PORT=1102
BACKEND_DIR="$PROJECT_ROOT/backend"
FRONTEND_DIR="$PROJECT_ROOT/frontend"
KILL_WAIT_SECS=5

# Same credentials as docker-compose.dev.yml, so Docker and --native share a URL.
DEFAULT_DB_URL="postgres://myriad:myriad_dev_password@localhost:5432/myriad"

USE_NATIVE=1          # default: local PostgreSQL. --docker flips this off.
USE_FG=1              # default: backend + frontend in this terminal
FG_EXPLICIT=0         # 1 = user passed --fg
DETACH_EXPLICIT=0     # 1 = user passed --detach (keep the menu usable)
CARGO_RELEASE=0
SKIP_INSTALL=0
RUN_BACKEND=1
RUN_FRONTEND=1

DB_USER=""
DB_PASS=""
DB_HOST="localhost"
DB_PORT="5432"
DB_NAME=""

# ==================== Unicode Icons ====================
ICON_CHECK="✔"
ICON_CROSS="✖"
ICON_ARROW="➜"
ICON_ROCKET="🚀"
ICON_STOP="⏹"
ICON_REFRESH="↻"
ICON_TRASH="🗑"
ICON_INFO="ℹ"
ICON_WARN="⚠"
ICON_DB="🗄"
ICON_RUST="🦀"
ICON_NODE="⬢"
ICON_UPDATER="⇧"
ICON_HEART="❤"
ICON_SPARKLE="✨"

# ==================== Helper Functions ====================

clear_screen() {
    # Erase display + scrollback, then home. Avoids a tall blank region
    # above the logo when scrolling up after clear (some terminals keep
    # cleared rows in scrollback when only CSI 2J is used).
    printf "\033[H\033[2J\033[3J"
}

hide_cursor() { printf "\033[?25l"; }
show_cursor() { printf "\033[?25h"; }

get_terminal_size() {
    TERM_COLS=$(tput cols 2>/dev/null || echo 80)
}

# Unique, sorted PIDs from multi-line input (ignores empty / non-numeric).
normalize_pids() {
    awk '
        {
            for (i = 1; i <= NF; i++) {
                if ($i ~ /^[0-9]+$/) print $i
            }
        }
    ' | sort -n | uniq
}

# Current working directory of a PID (macOS/Linux via lsof).
proc_cwd() {
    local pid="$1"
    if ! command -v lsof >/dev/null 2>&1; then
        return 0
    fi
    lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | awk '/^n/ { print substr($0, 2); exit }'
}

# PIDs listening on a TCP port.
list_listen_pids() {
    local port="$1"
    if ! command -v lsof >/dev/null 2>&1; then
        return 0
    fi
    # -nP avoids DNS/service-name resolution delays
    lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null | normalize_pids || true
}

# Direct children of given PIDs (one level; call repeatedly or use expand_process_tree).
list_child_pids() {
    local parent
    for parent in "$@"; do
        [[ "$parent" =~ ^[0-9]+$ ]] || continue
        ps -axo pid=,ppid= 2>/dev/null | awk -v p="$parent" '$2 == p { print $1 }'
    done | normalize_pids
}

# Expand seed PIDs to include all descendants (BFS).
expand_process_tree() {
    local seeds
    seeds="$(printf '%s\n' "$@" | normalize_pids)"
    [[ -z "$seeds" ]] && return 0

    local -a queue=()
    local -a all=()
    local pid child
    local seen=""

    while IFS= read -r pid; do
        [[ -n "$pid" ]] || continue
        queue+=("$pid")
        all+=("$pid")
        seen="$seen $pid "
    done <<< "$seeds"

    local i=0
    while [[ $i -lt ${#queue[@]} ]]; do
        pid="${queue[$i]}"
        i=$((i + 1))
        while IFS= read -r child; do
            [[ -n "$child" ]] || continue
            case "$seen" in
                *" $child "*) continue ;;
            esac
            seen="$seen $child "
            queue+=("$child")
            all+=("$child")
        done < <(list_child_pids "$pid")
    done

    printf '%s\n' "${all[@]}" | normalize_pids
}

# Soft kill → wait → hard kill. Expands each PID to its descendant tree first.
terminate_pids() {
    local label="${1:-process}"
    shift || true
    local seeds
    seeds="$(printf '%s\n' "$@" | normalize_pids)"
    [[ -z "$seeds" ]] && return 0

    local pids
    # shellcheck disable=SC2086
    pids="$(expand_process_tree $seeds)"
    [[ -z "$pids" ]] && return 0

    local -a pid_arr=()
    while IFS= read -r pid; do
        [[ -n "$pid" ]] && pid_arr+=("$pid")
    done <<< "$pids"

    print_info "Sending SIGTERM to ${#pid_arr[@]} $label PID(s): ${pid_arr[*]}"
    kill -TERM "${pid_arr[@]}" 2>/dev/null || true

    local remaining
    local ticks=0
    local max_ticks=$((KILL_WAIT_SECS * 4))
    while [[ $ticks -lt $max_ticks ]]; do
        remaining=()
        for pid in "${pid_arr[@]}"; do
            if kill -0 "$pid" 2>/dev/null; then
                remaining+=("$pid")
            fi
        done
        [[ ${#remaining[@]} -eq 0 ]] && return 0
        sleep 0.25
        ticks=$((ticks + 1))
    done

    remaining=()
    for pid in "${pid_arr[@]}"; do
        if kill -0 "$pid" 2>/dev/null; then
            remaining+=("$pid")
        fi
    done
    if [[ ${#remaining[@]} -gt 0 ]]; then
        print_warning "Force killing stubborn $label PID(s): ${remaining[*]}"
        kill -KILL "${remaining[@]}" 2>/dev/null || true
        sleep 0.2
    fi
}

# Free a TCP listen port (kill listeners + their trees).
free_port() {
    local port="$1"
    local label="${2:-port $port}"
    local pids
    pids="$(list_listen_pids "$port")"
    if [[ -z "$pids" ]]; then
        return 0
    fi
    print_info "Freeing $label (listeners: $(echo "$pids" | tr '\n' ' '))"
    # shellcheck disable=SC2086
    terminate_pids "$label" $pids
    # Final sweep if something re-bound or ignored signals
    pids="$(list_listen_pids "$port")"
    if [[ -n "$pids" ]]; then
        # shellcheck disable=SC2086
        kill -KILL $pids 2>/dev/null || true
        sleep 0.2
    fi
}

# Match backend via:
#   1) cmdline contains this project's backend path + cargo/myriad-backend
#   2) process cwd under this project's backend + cargo/myriad-backend
#   3) whoever is listening on BACKEND_PORT (must free port to restart)
list_backend_pids() {
    local -a found=()
    local pid cmd cwd

    while IFS= read -r line; do
        # pid is first field; remainder is command (may contain spaces)
        pid="${line%% *}"
        cmd="${line#${pid}}"
        cmd="${cmd#"${cmd%%[![:space:]]*}"}" # ltrim
        [[ "$pid" =~ ^[0-9]+$ ]] || continue

        local match=0
        if [[ "$cmd" == *"$BACKEND_DIR"* && ( "$cmd" == *cargo* || "$cmd" == *myriad-backend* ) ]]; then
            match=1
        elif [[ "$cmd" == *myriad-backend* || "$cmd" == *"cargo run"* || "$cmd" == *"cargo-watch"* || "$cmd" == *"cargo watch"* ]]; then
            cwd="$(proc_cwd "$pid" 2>/dev/null || true)"
            if [[ -n "$cwd" && ( "$cwd" == "$BACKEND_DIR" || "$cwd" == "$BACKEND_DIR"/* ) ]]; then
                match=1
            fi
        fi
        [[ $match -eq 1 ]] && found+=("$pid")
    done < <(ps -axo pid=,command= 2>/dev/null || true)

    while IFS= read -r pid; do
        [[ -n "$pid" ]] && found+=("$pid")
    done < <(list_listen_pids "$BACKEND_PORT")

    if [[ ${#found[@]} -eq 0 ]]; then
        return 0
    fi
    printf '%s\n' "${found[@]}" | normalize_pids
}

# Match frontend via cmdline path, cwd under this project's frontend, and FRONTEND_PORT.
list_frontend_pids() {
    local -a found=()
    local pid cmd cwd

    while IFS= read -r line; do
        pid="${line%% *}"
        cmd="${line#${pid}}"
        cmd="${cmd#"${cmd%%[![:space:]]*}"}"
        [[ "$pid" =~ ^[0-9]+$ ]] || continue

        local match=0
        if [[ "$cmd" == *"$FRONTEND_DIR"* && ( "$cmd" == *pnpm* || "$cmd" == *astro* || "$cmd" == *vite* || "$cmd" == *node* ) ]]; then
            match=1
        elif [[ "$cmd" == *pnpm* || "$cmd" == *astro* || "$cmd" == *vite* || "$cmd" == *node* ]]; then
            cwd="$(proc_cwd "$pid" 2>/dev/null || true)"
            if [[ -n "$cwd" && ( "$cwd" == "$FRONTEND_DIR" || "$cwd" == "$FRONTEND_DIR"/* ) ]]; then
                match=1
            fi
        fi
        [[ $match -eq 1 ]] && found+=("$pid")
    done < <(ps -axo pid=,command= 2>/dev/null || true)

    while IFS= read -r pid; do
        [[ -n "$pid" ]] && found+=("$pid")
    done < <(list_listen_pids "$FRONTEND_PORT")

    if [[ ${#found[@]} -eq 0 ]]; then
        return 0
    fi
    printf '%s\n' "${found[@]}" | normalize_pids
}

# Draw a box
draw_box() {
    local title="$1"
    local width="${2:-60}"
    local color="${3:-$CYAN}"
    
    echo -en "${color}╭"
    printf '%*s' "$((width-2))" | tr ' ' "─"
    echo -e "╮${NC}"
    
    if [[ -n "$title" ]]; then
        local title_len=${#title}
        local padding=$(( (width - title_len - 4) / 2 ))
        echo -en "${color}│${NC}"
        printf '%*s' "$padding" ""
        echo -en "${BOLD}${BRIGHT_WHITE} ${title} ${NC}"
        printf '%*s' "$((width - padding - title_len - 4))" ""
        echo -e "${color}│${NC}"
        
        echo -en "${color}├"
        printf '%*s' "$((width-2))" | tr ' ' "─"
        echo -e "┤${NC}"
    fi
}

draw_box_bottom() {
    local width="${1:-60}"
    local color="${2:-$CYAN}"
    echo -en "${color}╰"
    printf '%*s' "$((width-2))" | tr ' ' "─"
    echo -e "╯${NC}"
}

draw_box_line() {
    local text="$1"
    local width="${2:-60}"
    local color="${3:-$CYAN}"
    
    # Remove ANSI codes for length calculation
    local clean_text=$(echo -e "$text" | sed 's/\x1b\[[0-9;]*m//g')
    local text_len=${#clean_text}
    
    echo -en "${color}│${NC} ${text}"
    local padding=$((width - text_len - 3))
    [[ $padding -gt 0 ]] && printf '%*s' "$padding" ""
    echo -e "${color}│${NC}"
}

# Progress bar
progress_bar() {
    local current=$1
    local total=$2
    local width="${3:-40}"
    local title="${4:-Progress}"
    
    local percent=$((current * 100 / total))
    local filled=$((current * width / total))
    local empty=$((width - filled))
    
    printf "\r${CYAN}${title}${NC} ["
    [[ $filled -gt 0 ]] && printf "${GREEN}%*s${NC}" $filled | tr ' ' '█'
    [[ $empty -gt 0 ]] && printf "${DIM}%*s${NC}" $empty | tr ' ' '░'
    printf "] ${BRIGHT_WHITE}%3d%%${NC}" $percent
}

# Print styled messages
print_success() { echo -e "${GREEN}${ICON_CHECK}${NC} $1"; }
print_error() { echo -e "${RED}${ICON_CROSS}${NC} $1"; }
print_info() { echo -e "${CYAN}${ICON_INFO}${NC} $1"; }
print_warning() { echo -e "${YELLOW}${ICON_WARN}${NC} $1"; }
print_step() { echo -e "${MAGENTA}${ICON_ARROW}${NC} $1"; }

have() { command -v "$1" >/dev/null 2>&1; }
docker_cli_available() { have docker; }

# ==================== Native / local PostgreSQL ====================
# Used by --native, doctor, and db-setup. Credentials match
# docker-compose.dev.yml so the two database paths are interchangeable.

read_env_db_url() {
    [[ -f "$BACKEND_DIR/.env" ]] || return 0
    sed -n 's/^[[:space:]]*DATABASE_URL[[:space:]]*=[[:space:]]*//p' "$BACKEND_DIR/.env" \
        | tail -n 1 | sed -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//"
}

resolve_db_url() {
    if [[ -n "${DATABASE_URL:-}" ]]; then
        printf '%s' "$DATABASE_URL"
        return
    fi
    local from_env
    from_env="$(read_env_db_url)"
    if [[ -n "$from_env" ]]; then
        printf '%s' "$from_env"
        return
    fi
    printf '%s' "$DEFAULT_DB_URL"
}

parse_db_url() {
    local url="${1:-}"
    [[ -n "$url" ]] || url="$(resolve_db_url)"
    local re='^postgres(ql)?://([^:@/]+)(:([^@/]*))?@([^:/?]+)(:([0-9]+))?/([^?]+)'
    if [[ "$url" =~ $re ]]; then
        DB_USER="${BASH_REMATCH[2]}"
        DB_PASS="${BASH_REMATCH[4]}"
        DB_HOST="${BASH_REMATCH[5]}"
        DB_PORT="${BASH_REMATCH[7]:-5432}"
        DB_NAME="${BASH_REMATCH[8]}"
        return 0
    fi
    return 1
}

# 0 = reachable, 1 = not, 2 = no client tools to probe.
db_reachable() {
    parse_db_url || return 1
    if have pg_isready; then
        pg_isready -q -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" >/dev/null 2>&1
    elif have psql; then
        PGPASSWORD="$DB_PASS" psql -w "$(resolve_db_url)" -tAc 'SELECT 1' >/dev/null 2>&1
    else
        return 2
    fi
}

maybe_autodetect_native() {
    [[ "$USE_NATIVE" -eq 1 ]] && return 0
    if ! docker_cli_available; then
        print_warning "Docker not found — using local PostgreSQL"
        USE_NATIVE=1
        if [[ "$DETACH_EXPLICIT" -eq 0 ]]; then
            USE_FG=1
        fi
    fi
}

ensure_backend_env() {
    local env_file="$BACKEND_DIR/.env"
    [[ -f "$env_file" ]] && return 0

    print_info "backend/.env missing — writing a development default"

    local secret=""
    if have openssl; then
        secret="$(openssl rand -base64 48 | tr -d '\n')"
    elif [[ -r /dev/urandom ]]; then
        secret="$(LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 64)"
    fi
    if [[ -z "$secret" ]]; then
        print_error "Cannot generate JWT_SECRET (no openssl, no /dev/urandom)"
        print_info "Create $env_file by hand from backend/.env.example"
        return 1
    fi

    local db_url
    db_url="$(resolve_db_url)"

    ( umask 077; cat > "$env_file" <<ENV
# Myriad backend — LOCAL DEVELOPMENT ONLY (generated by scripts/dev/dev.sh)
# Not a production config: no ENVIRONMENT=production, so CSP/HSTS stay off.
# Full reference: backend/.env.example

DATABASE_URL=$db_url

SERVER_HOST=127.0.0.1
SERVER_PORT=$BACKEND_PORT
RUST_LOG=info,myriad_backend=debug

FRONTEND_DIST_PATH=../frontend/dist

JWT_SECRET=$secret

CORS_ORIGINS=http://localhost:$FRONTEND_PORT,http://localhost:$BACKEND_PORT

BASE_URL=http://127.0.0.1:$BACKEND_PORT
FRONTEND_URL=http://localhost:$FRONTEND_PORT
TAPP_STORE_STATS_ENABLED=false
ENV
    ) || { print_error "Failed to write $env_file"; return 1; }
    print_success "Wrote $env_file (chmod 600, JWT_SECRET generated)"
}

check_tools() {
    local missing=0
    if [[ "$RUN_BACKEND" -eq 1 ]]; then
        if have cargo; then
            print_success "cargo    $(cargo --version 2>/dev/null | awk '{print $2}')"
        else
            print_error "cargo not found — install Rust: https://rustup.rs"
            missing=1
        fi
    fi
    if [[ "$RUN_FRONTEND" -eq 1 ]]; then
        if have pnpm; then
            print_success "pnpm     $(pnpm --version 2>/dev/null)"
        else
            print_error "pnpm not found — run 'corepack enable'"
            missing=1
        fi
        if have node; then
            print_success "node     $(node --version 2>/dev/null)"
        else
            print_error "node not found — Node 20+ required"
            missing=1
        fi
    fi
    return "$missing"
}

run_doctor() {
    echo ""
    echo -e "${BOLD}Myriad Dev — doctor${NC}"
    echo ""

    print_step "Toolchain"
    local tools_ok=0
    check_tools || tools_ok=1

    if [[ "$USE_NATIVE" -eq 0 ]]; then
        echo ""
        print_step "Docker"
        if docker_cli_available; then
            print_success "docker   $(docker --version 2>/dev/null | awk '{print $3}' | tr -d ',')"
        else
            print_warning "docker not found — start will fall back to --native"
        fi
    fi

    if [[ "$RUN_BACKEND" -eq 1 ]]; then
        echo ""
        print_step "Database"
        if ! parse_db_url; then
            print_error "Could not parse DATABASE_URL: $(resolve_db_url)"
        else
            echo -e "  ${DIM}${DB_USER}@${DB_HOST}:${DB_PORT}/${DB_NAME}${NC}"
            local rc=0
            db_reachable || rc=$?
            case "$rc" in
                0) print_success "reachable" ;;
                2) print_warning "no pg_isready/psql — cannot verify from here" ;;
                *)
                    if [[ "$USE_NATIVE" -eq 1 ]]; then
                        print_error "not reachable — start PostgreSQL, then: $0 db-setup"
                    else
                        print_warning "not reachable — start will launch docker postgres (or use --native + db-setup)"
                    fi
                    ;;
            esac
        fi
        echo ""
        print_step "Config"
        if [[ -f "$BACKEND_DIR/.env" ]]; then
            print_success "backend/.env present"
        else
            print_warning "backend/.env missing — it will be generated on start"
        fi
    fi

    echo ""
    print_step "Ports"
    local port pids
    for port in $([ "$RUN_BACKEND" -eq 1 ] && echo "$BACKEND_PORT"; \
                  [ "$RUN_FRONTEND" -eq 1 ] && echo "$FRONTEND_PORT"); do
        pids="$(list_listen_pids "$port" | tr '\n' ' ')"
        if [[ -n "${pids// /}" ]]; then
            print_warning ":$port in use by PID(s) ${pids% }"
        else
            print_success ":$port free"
        fi
    done

    echo ""
    if [[ "$tools_ok" -eq 0 ]]; then
        print_success "Ready."
        return 0
    fi
    print_error "Missing prerequisites (see above)."
    return 1
}

run_db_setup() {
    parse_db_url || { print_error "Could not parse DATABASE_URL: $(resolve_db_url)"; return 1; }
    have psql || { print_error "psql not found — install the PostgreSQL client tools"; return 1; }

    print_step "Creating role '$DB_USER' and database '$DB_NAME' on $DB_HOST:$DB_PORT…"

    local -a admin
    if [[ -n "${MYRIAD_PSQL_ADMIN:-}" ]]; then
        # shellcheck disable=SC2206
        admin=($MYRIAD_PSQL_ADMIN)
    elif psql -w -U postgres -h "$DB_HOST" -p "$DB_PORT" -tAc 'SELECT 1' >/dev/null 2>&1; then
        admin=(psql -U postgres -h "$DB_HOST" -p "$DB_PORT")
    elif have sudo; then
        admin=(sudo -u postgres psql)
    else
        print_error "No superuser psql route found"
        print_info "Set MYRIAD_PSQL_ADMIN, e.g. MYRIAD_PSQL_ADMIN='sudo -u postgres psql'"
        return 1
    fi

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

    print_success "Database ready — $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"
    print_info "Schema migrations run automatically on backend startup."
}

# ==================== Logo & Banner ====================

show_logo() {
    local logo="$PROJECT_ROOT/shared/logo-ansi.txt"
    if [[ -f "$logo" ]]; then
        # Truecolor half-block art (UTF-8). No wordmark — character only.
        cat "$logo"
        echo ""
    fi
}

show_mini_logo() {
    echo -e "${BRIGHT_CYAN}${BOLD}◆ Myriad${NC} ${DIM}v${VERSION}${NC}"
}

# ==================== Status Functions ====================

get_service_status() {
    local service=$1
    case $service in
        backend)
            [[ -n "$(list_backend_pids)" ]]
            ;;
        frontend)
            [[ -n "$(list_frontend_pids)" ]]
            ;;
        database)
            if docker ps --filter "name=myriad-postgres" --format "{{.Names}}" 2>/dev/null | grep -q "myriad-postgres"; then
                return 0
            fi
            db_reachable
            ;;
        updater)
            docker ps --filter "name=myriad-updater-dev" --format "{{.Names}}" 2>/dev/null | grep -q "myriad-updater-dev"
            ;;
    esac
}

get_status_text() {
    if $1; then
        echo -e "${GREEN}${ICON_CHECK} Running${NC}"
    else
        echo -e "${RED}${ICON_CROSS} Stopped${NC}"
    fi
}

backend_health_ok() {
    curl -fsS --max-time 1 "http://127.0.0.1:${BACKEND_PORT}/health" >/dev/null 2>&1
}

backend_port_in_use() {
    [[ -n "$(list_listen_pids "$BACKEND_PORT")" ]]
}

frontend_port_in_use() {
    [[ -n "$(list_listen_pids "$FRONTEND_PORT")" ]]
}

updater_health_ok() {
    curl -fsS --max-time 1 "http://127.0.0.1:1101/healthz" >/dev/null 2>&1
}

wait_for_backend() {
    local timeout="${1:-90}"
    local elapsed=0
    print_info "Waiting for backend health on http://127.0.0.1:${BACKEND_PORT}/health ..."
    while [[ $elapsed -lt $timeout ]]; do
        if backend_health_ok; then
            print_success "Backend is ready"
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    return 1
}

wait_for_updater() {
    local timeout="${1:-90}"
    local elapsed=0
    print_info "Waiting for updater health on http://127.0.0.1:1101/healthz ..."
    while [[ $elapsed -lt $timeout ]]; do
        if updater_health_ok; then
            print_success "Updater is ready"
            return 0
        fi
        sleep 1
        elapsed=$((elapsed + 1))
    done
    return 1
}

dev_updater_token() {
    echo "${MYRIAD_DEV_UPDATE_TOKEN:-$DEV_UPDATER_TOKEN_DEFAULT}"
}

dev_updater_gateway_secret() {
    echo "${MYRIAD_DEV_UPDATER_GATEWAY_SECRET:-dev-updater-gateway-secret-32chars!!}"
}

ensure_dev_updater_files() {
    mkdir -p "$DEV_UPDATER_DIR/state" "$DEV_UPDATER_DIR/pgdata" "$DEV_UPDATER_DIR/backups"

    cat > "$DEV_UPDATER_DIR/.env" <<EOF
MYRIAD_TAG=v0.0.0-dev
PROXY_TAG=v0.0.0-dev
UPDATER_TAG=v0.0.0-dev
COMPOSE_PROJECT_NAME=myriad-dev-updater
POSTGRES_PASSWORD=devupdaterpostgres12345678901234567890
JWT_SECRET=devupdaterjwtsecret12345678901234567890
CORS_ORIGINS=http://localhost:1102,http://localhost:1103
UPDATE_TOKEN=$(dev_updater_token)
CHANNEL=stable
CHECK_INTERVAL_SECS=0
EOF

    cat > "$DEV_UPDATER_DIR/docker-guard.env" <<'EOF'
DOCKER_GUARD_IMAGE=myriad-updater-dev:v0.0.0-dev
GUARD_COMPOSE_PROJECT_NAME=myriad-dev-updater
GUARD_MYRIAD_DOCKER_NETWORK=myriad-dev-updater_default
GUARD_MYRIAD_ADMIN_NETWORK=myriad-dev-admin-net
GUARD_MYRIAD_DOCKER_GUARD_NETWORK=myriad-dev-docker-guard-net
MYRIAD_GUARD_ENV_FILE=/dev/docker-guard.env
EOF

    cat > "$DEV_UPDATER_DIR/docker-compose.yml" <<'EOF'
services:
  postgres:
    image: postgres:18-alpine
  backend:
    image: example/myriad-backend:${MYRIAD_TAG}
  frontend:
    image: example/myriad-frontend:${MYRIAD_TAG}
  proxy:
    image: example/myriad-proxy:${PROXY_TAG}
  updater:
    image: example/myriad-updater:${UPDATER_TAG}
EOF
}

dev_updater_enabled() {
    [[ "${MYRIAD_DEV_UPDATER:-}" == "1" ]] || get_service_status updater
}

show_status_dashboard() {
    local width=50
    
    echo ""
    draw_box "Service Status" $width "$BRIGHT_CYAN"
    
    # Database
    local db_status=false
    get_service_status database && db_status=true
    draw_box_line "${ICON_DB} Database (PostgreSQL)    $(get_status_text $db_status)" $width "$BRIGHT_CYAN"
    
    # Backend
    local backend_status=false
    get_service_status backend && backend_status=true
    draw_box_line "${ICON_RUST} Backend (Rust/Axum)      $(get_status_text $backend_status)" $width "$BRIGHT_CYAN"
    
    # Frontend
    local frontend_status=false
    get_service_status frontend && frontend_status=true
    draw_box_line "${ICON_NODE} Frontend (Astro/React)   $(get_status_text $frontend_status)" $width "$BRIGHT_CYAN"

    # Updater harness
    local updater_status=false
    get_service_status updater && updater_status=true
    draw_box_line "${ICON_UPDATER} Updater Harness        $(get_status_text $updater_status)" $width "$BRIGHT_CYAN"
    
    draw_box_line "" $width "$BRIGHT_CYAN"
    
    if $backend_status || $frontend_status || $updater_status; then
        $backend_status && draw_box_line "${DIM}API:      http://localhost:${BACKEND_PORT}${NC}" $width "$BRIGHT_CYAN"
        $frontend_status && draw_box_line "${DIM}Frontend: http://localhost:${FRONTEND_PORT}${NC}" $width "$BRIGHT_CYAN"
        $updater_status && draw_box_line "${DIM}Updater:  http://127.0.0.1:1101${NC}" $width "$BRIGHT_CYAN"
    fi
    
    draw_box_bottom $width "$BRIGHT_CYAN"
    echo ""
}

# ==================== Service Control ====================

start_database() {
    maybe_autodetect_native

    if [[ "$USE_NATIVE" -eq 1 ]]; then
        print_step "Checking local PostgreSQL..."
        local rc=0
        db_reachable || rc=$?
        case "$rc" in
            0)
                print_success "Local PostgreSQL reachable ($DB_USER@$DB_HOST:$DB_PORT/$DB_NAME)"
                return 0
                ;;
            2)
                print_warning "No pg_isready/psql — cannot verify the database, starting anyway"
                return 0
                ;;
            *)
                print_error "Local PostgreSQL not reachable: $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME"
                print_info "Start PostgreSQL, then run: $0 db-setup"
                return 1
                ;;
        esac
    fi

    print_step "Starting PostgreSQL database..."

    if get_service_status database; then
        print_warning "Database is already running"
        return 0
    fi

    cd "$PROJECT_ROOT"
    if [[ -f "docker-compose.dev.yml" ]]; then
        docker compose -f docker-compose.dev.yml up -d 2>/dev/null
        sleep 3
        if get_service_status database; then
            print_success "Database started"
        else
            print_error "Failed to start database"
            return 1
        fi
    else
        print_error "docker-compose.dev.yml not found"
        return 1
    fi
}

stop_database() {
    local stopped_docker=0
    if docker_cli_available && [[ -f "$PROJECT_ROOT/docker-compose.dev.yml" ]]; then
        if docker ps --filter "name=myriad-postgres" --format "{{.Names}}" 2>/dev/null | grep -q "myriad-postgres"; then
            print_step "Stopping PostgreSQL database..."
            cd "$PROJECT_ROOT"
            docker compose -f docker-compose.dev.yml stop postgres 2>/dev/null || true
            docker compose -f docker-compose.dev.yml rm -f postgres 2>/dev/null || true
            print_success "Database stopped"
            stopped_docker=1
        fi
    fi
    if [[ "$USE_NATIVE" -eq 1 && "$stopped_docker" -eq 0 ]]; then
        print_info "Local PostgreSQL is not managed by this script (left running)"
    fi
}

start_updater() {
    if [[ "$USE_NATIVE" -eq 1 ]]; then
        print_error "Updater harness is Docker-only (not available with --native)"
        return 1
    fi

    print_step "Starting updater dev harness..."

    if get_service_status updater; then
        print_warning "Updater harness is already running"
        return 0
    fi

    ensure_dev_updater_files
    cd "$PROJECT_ROOT"
    UPDATE_TOKEN="$(dev_updater_token)" docker compose -f docker-compose.dev.yml --profile updater up -d docker-guard updater updater-gateway

    if ! wait_for_updater 120; then
        print_warning "Updater did not become ready within 120s"
        print_info "Check logs with: docker compose -f docker-compose.dev.yml --profile updater logs -f docker-guard updater updater-gateway"
        return 1
    fi

    print_success "Updater harness started (gateway on 127.0.0.1:1104)"
    print_info "Backend will use it when started/restarted while the harness is running."
}

stop_updater() {
    print_step "Stopping updater dev harness..."
    cd "$PROJECT_ROOT"
    docker compose -f docker-compose.dev.yml --profile updater stop updater-gateway updater docker-guard 2>/dev/null || true
    docker compose -f docker-compose.dev.yml --profile updater rm -f updater-gateway updater docker-guard 2>/dev/null || true
    print_success "Updater harness stopped"
}

start_backend() {
    print_step "Starting Rust backend..."

    # Healthy and process visible → nothing to do.
    if backend_health_ok && get_service_status backend; then
        print_warning "Backend is already running and healthy"
        return 0
    fi

    # Stale process, wrong cwd, or port occupied by an orphan binary → hard reset.
    if get_service_status backend || backend_port_in_use || backend_health_ok; then
        print_warning "Clearing existing backend / port ${BACKEND_PORT} before start..."
        stop_backend || true
        sleep 0.5
        if backend_port_in_use; then
            print_error "Port ${BACKEND_PORT} still in use; cannot start backend"
            return 1
        fi
    fi

    cd "$BACKEND_DIR"
    ensure_backend_env || return 1

    local rust_cmd="cargo run"
    [[ "$CARGO_RELEASE" -eq 1 ]] && rust_cmd="cargo run --release"
    # Prefer gateway when harness is up; backend holds UPDATER_GATEWAY_SECRET, not UPDATE_TOKEN.
    local updater_url="http://127.0.0.1:1104"
    local gw_secret
    gw_secret="$(dev_updater_gateway_secret)"
    local cargo_cmd="$rust_cmd"
    local inject_updater=0
    if dev_updater_enabled && [[ "$USE_NATIVE" -eq 0 ]]; then
        ensure_dev_updater_files
        cargo_cmd="MYRIAD_UPDATER_URL=$updater_url UPDATER_GATEWAY_SECRET=$gw_secret $rust_cmd"
        inject_updater=1
        print_info "Backend updater proxy via gateway: $updater_url (gateway secret; no UPDATE_TOKEN)"
    elif dev_updater_enabled && [[ "$USE_NATIVE" -eq 1 ]]; then
        print_warning "Ignoring updater harness in --native mode"
    fi
    if [[ "$CARGO_RELEASE" -eq 1 ]]; then
        print_info "Backend will run in release mode"
    fi

    if [[ "$OSTYPE" == "darwin"* ]]; then
        osascript -e 'tell application "Terminal" to do script "cd '"$BACKEND_DIR"' && echo \"🦀 Myriad Backend\" && source ~/.cargo/env 2>/dev/null; '"$cargo_cmd"'"' 2>/dev/null
    else
        if command -v gnome-terminal &> /dev/null; then
            gnome-terminal -- bash -c "cd '$BACKEND_DIR' && echo '🦀 Myriad Backend' && $cargo_cmd; exec bash" 2>/dev/null
        else
            if [[ "$inject_updater" -eq 1 ]]; then
                MYRIAD_UPDATER_URL="$updater_url" UPDATER_GATEWAY_SECRET="$gw_secret" nohup $rust_cmd > "$PROJECT_ROOT/backend.log" 2>&1 &
            else
                nohup $rust_cmd > "$PROJECT_ROOT/backend.log" 2>&1 &
            fi
            print_info "Backend running in background (logs: backend.log)"
        fi
    fi

    if ! wait_for_backend 90; then
        print_warning "Backend did not become ready within 90s"
        print_warning "Check the backend terminal, or backend.log if it was started in background."
        if backend_port_in_use; then
            print_info "Port ${BACKEND_PORT} listeners: $(list_listen_pids "$BACKEND_PORT" | tr '\n' ' ')"
        fi
    fi
}

stop_backend() {
    print_step "Stopping backend..."
    local pids
    pids="$(list_backend_pids)"

    if [[ -z "$pids" ]] && ! backend_port_in_use; then
        print_info "No Myriad backend process found"
        return 0
    fi

    if [[ -n "$pids" ]]; then
        # shellcheck disable=SC2086
        terminate_pids "backend" $pids
    fi

    # Always free the listen port — handles relative-path binaries and foreign clones.
    free_port "$BACKEND_PORT" "backend port ${BACKEND_PORT}"

    if backend_port_in_use || [[ -n "$(list_backend_pids)" ]]; then
        print_error "Backend may still be running (port ${BACKEND_PORT} or matching PIDs remain)"
        local leftover
        leftover="$(list_backend_pids)"
        [[ -n "$leftover" ]] && print_info "Remaining PIDs: $(echo "$leftover" | tr '\n' ' ')"
        leftover="$(list_listen_pids "$BACKEND_PORT")"
        [[ -n "$leftover" ]] && print_info "Port listeners: $(echo "$leftover" | tr '\n' ' ')"
        return 1
    fi

    print_success "Backend stopped"
}

start_frontend() {
    print_step "Starting Astro frontend..."

    if get_service_status frontend && frontend_port_in_use; then
        print_warning "Frontend is already running"
        return 0
    fi

    if get_service_status frontend || frontend_port_in_use; then
        print_warning "Clearing existing frontend / port ${FRONTEND_PORT} before start..."
        stop_frontend || true
        sleep 0.5
        if frontend_port_in_use; then
            print_error "Port ${FRONTEND_PORT} still in use; cannot start frontend"
            return 1
        fi
    fi

    cd "$FRONTEND_DIR"

    if [[ "$OSTYPE" == "darwin"* ]]; then
        osascript -e 'tell application "Terminal" to do script "cd '"$FRONTEND_DIR"' && echo \"⚡ Myriad Frontend\" && pnpm run dev"' 2>/dev/null
    else
        if command -v gnome-terminal &> /dev/null; then
            gnome-terminal -- bash -c "cd '$FRONTEND_DIR' && echo '⚡ Myriad Frontend' && pnpm run dev; exec bash" 2>/dev/null
        else
            nohup pnpm run dev > "$PROJECT_ROOT/frontend.log" 2>&1 &
            print_info "Frontend running in background (logs: frontend.log)"
        fi
    fi

    sleep 2
    print_success "Frontend starting on http://localhost:${FRONTEND_PORT}"
}

stop_frontend() {
    print_step "Stopping frontend..."
    local pids
    pids="$(list_frontend_pids)"

    if [[ -z "$pids" ]] && ! frontend_port_in_use; then
        print_info "No Myriad frontend process found"
        return 0
    fi

    if [[ -n "$pids" ]]; then
        # shellcheck disable=SC2086
        terminate_pids "frontend" $pids
    fi

    free_port "$FRONTEND_PORT" "frontend port ${FRONTEND_PORT}"

    if frontend_port_in_use || [[ -n "$(list_frontend_pids)" ]]; then
        print_error "Frontend may still be running (port ${FRONTEND_PORT} or matching PIDs remain)"
        return 1
    fi

    print_success "Frontend stopped"
}

# ==================== Foreground supervisor (--fg / up) ====================
# Prefixes interleaved logs and takes both services down on a single Ctrl-C.
# Portable: macOS /bin/bash 3.2 has no `wait -n`, so we poll.

FG_BACKEND_PID=""
FG_FRONTEND_PID=""
FG_SHUTTING_DOWN=0

fg_prefix() {
    local tag="$1" color="$2"
    trap '' INT TERM
    while IFS= read -r line; do
        printf '%b%s%b %s\n' "$color" "$tag" "$NC" "$line"
    done
}

FG_STARTED_PID=""
fg_start_service() {
    local tag="$1" color="$2" dir="$3"
    shift 3
    ( cd "$dir" && exec "$@" ) > >(fg_prefix "$tag" "$color") 2>&1 &
    FG_STARTED_PID=$!
}

fg_alive() { [[ -n "${1:-}" ]] && kill -0 "$1" 2>/dev/null; }

fg_child_pids() {
    if have pgrep; then
        pgrep -P "$1" 2>/dev/null || true
    else
        ps -axo pid=,ppid= 2>/dev/null | awk -v p="$1" '$2 == p { print $1 }'
    fi
}

fg_kill_tree() {
    local sig="$1" pid="$2" child
    fg_alive "$pid" || return 0
    for child in $(fg_child_pids "$pid"); do
        fg_kill_tree "$sig" "$child"
    done
    kill -"$sig" "$pid" 2>/dev/null || true
}

fg_shutdown() {
    [[ "$FG_SHUTTING_DOWN" -eq 1 ]] && return 0
    FG_SHUTTING_DOWN=1
    trap '' INT TERM
    echo ""
    print_info "Stopping services…"
    fg_kill_tree TERM "$FG_BACKEND_PID"
    fg_kill_tree TERM "$FG_FRONTEND_PID"

    local waited=0
    while fg_alive "$FG_BACKEND_PID" || fg_alive "$FG_FRONTEND_PID"; do
        sleep 0.2
        waited=$((waited + 1))
        if [[ "$waited" -ge 50 ]]; then
            print_warning "Services did not exit in 10s — sending SIGKILL"
            fg_kill_tree KILL "$FG_BACKEND_PID"
            fg_kill_tree KILL "$FG_FRONTEND_PID"
            break
        fi
    done
    print_success "Stopped"
}

start_foreground_stack() {
    echo ""
    echo -e "${BRIGHT_CYAN}${BOLD}${ICON_ROCKET} Starting services in foreground${NC}"
    if [[ "$USE_NATIVE" -eq 1 ]]; then
        echo -e "${DIM}  mode: native (local PostgreSQL, no Docker)${NC}"
    fi
    echo ""

    print_step "Checking prerequisites…"
    check_tools || return 1

    if [[ "${MYRIAD_DEV_UPDATER:-}" == "1" ]]; then
        start_updater || return 1
        echo ""
    fi

    start_database || return 1
    echo ""

    if [[ "$RUN_BACKEND" -eq 1 ]]; then
        if backend_port_in_use; then
            print_error "Port $BACKEND_PORT (backend) is already in use"
            print_info "Stop it first: $0 stop backend"
            return 1
        fi
        parse_db_url || { print_error "Could not parse DATABASE_URL: $(resolve_db_url)"; return 1; }
        ensure_backend_env || return 1
    fi
    if [[ "$RUN_FRONTEND" -eq 1 ]] && frontend_port_in_use; then
        print_error "Port $FRONTEND_PORT (frontend) is already in use"
        print_info "Stop it first: $0 stop frontend"
        return 1
    fi

    if [[ "$RUN_FRONTEND" -eq 1 && "$SKIP_INSTALL" -eq 0 ]]; then
        print_step "Installing frontend dependencies…"
        ( cd "$FRONTEND_DIR" && pnpm install ) || return 1
        echo ""
    fi

    print_step "Starting services (Ctrl-C to stop)…"
    trap fg_shutdown INT TERM

    if [[ "$RUN_BACKEND" -eq 1 ]]; then
        local -a cargo_cmd=(cargo run)
        [[ "$CARGO_RELEASE" -eq 1 ]] && cargo_cmd=(cargo run --release)
        if dev_updater_enabled && [[ "$USE_NATIVE" -eq 0 ]]; then
            ensure_dev_updater_files
            export MYRIAD_UPDATER_URL="http://127.0.0.1:1104"
            UPDATER_GATEWAY_SECRET="$(dev_updater_gateway_secret)"
            export UPDATER_GATEWAY_SECRET
            print_info "Backend updater proxy via gateway: $MYRIAD_UPDATER_URL"
        fi
        fg_start_service "[backend ]" "$MAGENTA" "$BACKEND_DIR" "${cargo_cmd[@]}"
        FG_BACKEND_PID="$FG_STARTED_PID"
        echo -e "  ${MAGENTA}backend${NC}   http://localhost:$BACKEND_PORT  (pid $FG_BACKEND_PID)"
    fi
    if [[ "$RUN_FRONTEND" -eq 1 ]]; then
        fg_start_service "[frontend]" "$BLUE" "$FRONTEND_DIR" pnpm run dev
        FG_FRONTEND_PID="$FG_STARTED_PID"
        echo -e "  ${BLUE}frontend${NC}  http://localhost:$FRONTEND_PORT  (pid $FG_FRONTEND_PID)"
    fi
    echo ""

    while [[ "$FG_SHUTTING_DOWN" -eq 0 ]]; do
        if [[ "$RUN_BACKEND" -eq 1 ]] && ! fg_alive "$FG_BACKEND_PID"; then
            print_error "Backend exited"
            break
        fi
        if [[ "$RUN_FRONTEND" -eq 1 ]] && ! fg_alive "$FG_FRONTEND_PID"; then
            print_error "Frontend exited"
            break
        fi
        sleep 0.3
    done

    fg_shutdown
}

# ==================== Combined Actions ====================

start_all() {
    maybe_autodetect_native
    if [[ "$USE_NATIVE" -eq 1 && "$USE_FG" -eq 0 && "$DETACH_EXPLICIT" -eq 0 ]]; then
        USE_FG=1
    fi

    if [[ "$USE_FG" -eq 1 ]]; then
        start_foreground_stack
        return
    fi

    echo ""
    echo -e "${BRIGHT_CYAN}${BOLD}${ICON_ROCKET} Starting All Services${NC}"
    echo ""

    if [[ "${MYRIAD_DEV_UPDATER:-}" == "1" ]]; then
        start_updater || return 1
        if get_service_status backend; then
            print_info "Restarting backend so it picks up updater dev environment..."
            stop_backend
            sleep 1
        fi
        echo ""
    fi

    start_database || return 1
    echo ""
    sleep 2
    if [[ "$RUN_BACKEND" -eq 1 ]]; then
        start_backend
        if ! backend_health_ok; then
            echo ""
            print_error "Backend is not ready; frontend was not started to avoid 127.0.0.1:${BACKEND_PORT} ECONNREFUSED."
            print_info "Fix the backend error first, then run: $0 start frontend"
            return 1
        fi
        echo ""
    fi
    if [[ "$RUN_FRONTEND" -eq 1 ]]; then
        start_frontend
    fi

    echo ""
    echo -e "${GREEN}${BOLD}${ICON_CHECK} All services started!${NC}"
    echo ""
    echo -e "${DIM}URLs:${NC}"
    [[ "$RUN_FRONTEND" -eq 1 ]] && echo -e "  ${CYAN}Frontend:${NC} http://localhost:${FRONTEND_PORT}"
    [[ "$RUN_BACKEND" -eq 1 ]] && echo -e "  ${CYAN}Backend:${NC}  http://localhost:${BACKEND_PORT}"
    [[ "$RUN_BACKEND" -eq 1 ]] && echo -e "  ${CYAN}Health:${NC}   http://localhost:${BACKEND_PORT}/health"
    if get_service_status updater; then
        echo -e "  ${CYAN}Updater:${NC}  http://127.0.0.1:1101"
    fi
    echo ""
}

stop_all() {
    echo ""
    echo -e "${BRIGHT_YELLOW}${BOLD}${ICON_STOP} Stopping All Services${NC}"
    echo ""

    # Best-effort: individual stoppers may return non-zero if a process resists;
    # continue so database/updater still get cleaned up under `set -e`.
    stop_frontend || true
    stop_backend || true
    stop_updater || true
    stop_database || true

    echo ""
    print_success "All services stopped"
    echo ""
}

restart_all() {
    stop_all
    sleep 2
    start_all
}

# ==================== Clean Functions ====================

clean_project() {
    echo ""
    draw_box "Clean Project" 50 "$YELLOW"
    draw_box_line "${ICON_WARN} This will delete:" 50 "$YELLOW"
    draw_box_line "  • Database tables" 50 "$YELLOW"
    draw_box_line "  • Backend build (target/)" 50 "$YELLOW"
    draw_box_line "  • Frontend build (dist/)" 50 "$YELLOW"
    draw_box_line "  • Cache files" 50 "$YELLOW"
    draw_box_bottom 50 "$YELLOW"
    echo ""
    
    echo -en "${YELLOW}Type 'yes' to confirm: ${NC}"
    read -r confirm
    
    if [[ "$confirm" != "yes" ]]; then
        print_info "Cancelled"
        return
    fi
    
    echo ""
    local total=5
    local current=0
    
    ((current++)); progress_bar $current $total 40 "Cleaning"
    if docker ps --filter "name=myriad-postgres-dev" --format "{{.Names}}" 2>/dev/null | grep -q "myriad-postgres-dev"; then
        docker exec myriad-postgres-dev psql -U myriad -d myriad -c "
DO \$\$ DECLARE r RECORD;
BEGIN
    FOR r IN (SELECT tablename FROM pg_tables WHERE schemaname = 'public') LOOP
        EXECUTE 'DROP TABLE IF EXISTS ' || quote_ident(r.tablename) || ' CASCADE';
    END LOOP;
END \$\$;" > /dev/null 2>&1 || true
    elif have psql && parse_db_url && db_reachable; then
        PGPASSWORD="$DB_PASS" psql -w "$(resolve_db_url)" -c "
DO \$\$ DECLARE r RECORD;
BEGIN
    FOR r IN (SELECT tablename FROM pg_tables WHERE schemaname = 'public') LOOP
        EXECUTE 'DROP TABLE IF EXISTS ' || quote_ident(r.tablename) || ' CASCADE';
    END LOOP;
END \$\$;" > /dev/null 2>&1 || true
    fi
    sleep 0.3
    
    ((current++)); progress_bar $current $total 40 "Cleaning"
    # Workspace target is at repo root; also drop legacy nested backend/target
    rm -rf "$PROJECT_ROOT/target" 2>/dev/null || true
    rm -rf "$PROJECT_ROOT/backend/target" 2>/dev/null || true
    sleep 0.3
    
    ((current++)); progress_bar $current $total 40 "Cleaning"
    rm -rf "$PROJECT_ROOT/frontend/dist" "$PROJECT_ROOT/frontend/.astro" 2>/dev/null || true
    sleep 0.3
    
    ((current++)); progress_bar $current $total 40 "Cleaning"
    rm -rf "$PROJECT_ROOT/backend/cache"/*.json 2>/dev/null || true
    sleep 0.3
    
    ((current++)); progress_bar $current $total 40 "Cleaning"
    rm -f "$PROJECT_ROOT/backend.log" "$PROJECT_ROOT/frontend.log" 2>/dev/null || true
    
    echo ""
    echo ""
    print_success "Cleanup complete!"
    echo ""
}

# ==================== Interactive Menu ====================

show_menu() {
    clear_screen
    show_logo
    show_status_dashboard
    
    local width=50
    draw_box "Main Menu" $width "$MAGENTA"
    draw_box_line "" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_GREEN}1${NC})  ${ICON_ROCKET} Start All Services" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_GREEN}2${NC})  ${ICON_STOP} Stop All Services" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_GREEN}3${NC})  ${ICON_REFRESH} Restart All Services" $width "$MAGENTA"
    draw_box_line "" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_YELLOW}4${NC})  ${ICON_DB} Database Only" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_YELLOW}5${NC})  ${ICON_RUST} Backend Only" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_YELLOW}6${NC})  ${ICON_NODE} Frontend Only" $width "$MAGENTA"
    draw_box_line "" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_YELLOW}7${NC})  ${ICON_UPDATER} Updater Harness" $width "$MAGENTA"
    draw_box_line "" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_CYAN}8${NC})  ${ICON_INFO} Show Status" $width "$MAGENTA"
    draw_box_line "  ${BRIGHT_RED}9${NC})  ${ICON_TRASH} Clean Project" $width "$MAGENTA"
    draw_box_line "" $width "$MAGENTA"
    draw_box_line "  ${DIM}q${NC})  Exit" $width "$MAGENTA"
    draw_box_line "" $width "$MAGENTA"
    draw_box_bottom $width "$MAGENTA"
    
    echo ""
    echo -en "${CYAN}Select option: ${NC}"
}

show_service_menu() {
    local service=$1
    local service_name="" icon=""
    
    case $service in
        database) service_name="Database"; icon="$ICON_DB" ;;
        backend) service_name="Backend"; icon="$ICON_RUST" ;;
        frontend) service_name="Frontend"; icon="$ICON_NODE" ;;
        updater) service_name="Updater Harness"; icon="$ICON_UPDATER" ;;
    esac
    
    echo ""
    draw_box "$icon $service_name" 40 "$CYAN"
    draw_box_line "  1) Start" 40 "$CYAN"
    draw_box_line "  2) Stop" 40 "$CYAN"
    draw_box_line "  3) Restart" 40 "$CYAN"
    draw_box_line "  b) Back" 40 "$CYAN"
    draw_box_bottom 40 "$CYAN"
    
    echo ""
    echo -en "${CYAN}Select: ${NC}"
    read -r choice
    
    case $choice in
        1) case $service in
            database) start_database ;; backend) start_backend ;; frontend) start_frontend ;; updater) start_updater ;;
           esac ;;
        2) case $service in
            database) stop_database ;; backend) stop_backend ;; frontend) stop_frontend ;; updater) stop_updater ;;
           esac ;;
        3) case $service in
            database) stop_database; sleep 1; start_database ;;
            backend) stop_backend; sleep 1; start_backend ;;
            frontend) stop_frontend; sleep 1; start_frontend ;;
            updater) stop_updater; sleep 1; start_updater ;;
           esac ;;
    esac
    
    echo ""
    echo -e "${DIM}Press Enter to continue...${NC}"
    read -r
}

run_interactive() {
    trap 'show_cursor; echo ""; exit 0' INT TERM
    
    while true; do
        show_menu
        read -r choice
        
        case $choice in
            1) USE_FG=0; DETACH_EXPLICIT=1; start_all; echo -e "${DIM}Press Enter...${NC}"; read -r ;;
            2) stop_all; echo -e "${DIM}Press Enter...${NC}"; read -r ;;
            3) restart_all; echo -e "${DIM}Press Enter...${NC}"; read -r ;;
            4) show_service_menu "database" ;;
            5) show_service_menu "backend" ;;
            6) show_service_menu "frontend" ;;
            7) show_service_menu "updater" ;;
            8) clear_screen; show_mini_logo; show_status_dashboard; echo -e "${DIM}Press Enter...${NC}"; read -r ;;
            9) clean_project; echo -e "${DIM}Press Enter...${NC}"; read -r ;;
            q|Q) clear_screen; echo -e "${CYAN}${ICON_HEART} Thanks for using Myriad! ${ICON_HEART}${NC}"; echo ""; exit 0 ;;
            *) print_warning "Invalid option"; sleep 1 ;;
        esac
    done
}

# ==================== CLI Help ====================

show_help() {
    show_mini_logo
    echo ""
    echo -e "${BOLD}Usage:${NC} ./dev.sh [command] [service] [options]"
    echo ""
    echo -e "${BOLD}Commands:${NC}"
    echo -e "  ${GREEN}start${NC}   [service]  Start services (default: all)"
    echo -e "  ${GREEN}up${NC}                 Alias for start --fg (this terminal)"
    echo -e "  ${RED}stop${NC}    [service]  Stop services (default: all)"
    echo -e "  ${YELLOW}restart${NC} [service]  Restart services (default: all)"
    echo -e "  ${CYAN}status${NC}             Show service status"
    echo -e "  ${CYAN}doctor${NC}             Check toolchain, ports and database"
    echo -e "  ${CYAN}db-setup${NC}           Create local PostgreSQL role + database"
    echo -e "  ${MAGENTA}clean${NC}              Clean build files and database"
    echo -e "  ${BLUE}menu${NC}               Open interactive menu"
    echo -e "  ${DIM}help${NC}               Show this help"
    echo ""
    echo -e "${BOLD}Services:${NC} database (db), backend, frontend, updater, all, all-updater"
    echo ""
    echo -e "${BOLD}Options:${NC}"
    echo -e "  ${DIM}--native${NC}           Local PostgreSQL (default). Implies --fg unless --detach"
    echo -e "  ${DIM}--docker${NC}           Start PostgreSQL via docker compose"
    echo -e "  ${DIM}--fg, --foreground${NC} Run backend + frontend in this terminal (default)"
    echo -e "  ${DIM}--detach${NC}           Open separate terminals (default with --docker)"
    echo -e "  ${DIM}--backend-only${NC}     Start only the backend"
    echo -e "  ${DIM}--frontend-only${NC}    Start only the frontend"
    echo -e "  ${DIM}--release${NC}          Build/run the backend in release mode"
    echo -e "  ${DIM}--skip-install${NC}     Skip pnpm install (foreground starts only)"
    echo ""
    echo -e "${BOLD}Environment:${NC}"
    echo -e "  ${DIM}DATABASE_URL${NC}         Override the dev database URL"
    echo -e "  ${DIM}MYRIAD_PSQL_ADMIN${NC}    Superuser psql command used by db-setup"
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo -e "  ${DIM}./dev.sh${NC}                      # Open interactive menu"
    echo -e "  ${DIM}./dev.sh start${NC}                # Local postgres, logs in this terminal"
    echo -e "  ${DIM}./dev.sh start --docker${NC}       # Docker postgres + new terminals"
    echo -e "  ${DIM}./dev.sh start --docker --fg${NC}  # Docker postgres, logs in this terminal"
    echo -e "  ${DIM}./dev.sh start all-updater${NC}    # Dev stack + updater harness (Docker)"
    echo -e "  ${DIM}./dev.sh start backend${NC}        # Start backend only"
    echo -e "  ${DIM}./dev.sh doctor${NC}               # Check prerequisites"
    echo -e "  ${DIM}./dev.sh db-setup${NC}             # Create local role + database"
    echo -e "  ${DIM}./dev.sh stop${NC}                 # Stop all services"
    echo -e "  ${DIM}./dev.sh status${NC}               # Show status"
    echo ""
}

# ==================== Main ====================

main() {
    local show_help_flag=0
    local -a positional=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --native) USE_NATIVE=1; shift ;;
            --docker) USE_NATIVE=0; shift ;;
            --fg|--foreground) USE_FG=1; FG_EXPLICIT=1; shift ;;
            --detach) USE_FG=0; DETACH_EXPLICIT=1; shift ;;
            --backend-only) RUN_FRONTEND=0; shift ;;
            --frontend-only) RUN_BACKEND=0; shift ;;
            --release) CARGO_RELEASE=1; shift ;;
            --skip-install) SKIP_INSTALL=1; shift ;;
            -h|--help) show_help_flag=1; shift ;;
            --*) print_error "Unknown option: $1"; echo ""; show_help; exit 2 ;;
            *) positional+=("$1"); shift ;;
        esac
    done

    if [[ "$show_help_flag" -eq 1 ]]; then
        show_help
        exit 0
    fi

    if [[ "$RUN_BACKEND" -eq 0 && "$RUN_FRONTEND" -eq 0 ]]; then
        print_error "--backend-only and --frontend-only are mutually exclusive"
        exit 2
    fi

    # --docker without an explicit --fg keeps the old detach (new terminal) behavior.
    if [[ "$USE_NATIVE" -eq 0 && "${FG_EXPLICIT:-0}" -eq 0 && "$DETACH_EXPLICIT" -eq 0 ]]; then
        USE_FG=0
    fi

    if [[ "$USE_NATIVE" -eq 1 && "$USE_FG" -eq 0 && "$DETACH_EXPLICIT" -eq 0 ]]; then
        USE_FG=1
    fi

    local command="${positional[0]:-}"
    local service="${positional[1]:-all}"

    [[ -z "$command" ]] && { run_interactive; exit 0; }

    case "$command" in
        start|up)
            [[ "$command" == "up" ]] && USE_FG=1
            case "$service" in
                all) start_all ;;
                database|db) start_database ;;
                all-updater|with-updater)
                    if [[ "$USE_NATIVE" -eq 1 ]]; then
                        print_info "all-updater requires Docker — switching off --native"
                        USE_NATIVE=0
                        if [[ "${FG_EXPLICIT:-0}" -eq 0 && "$DETACH_EXPLICIT" -eq 0 ]]; then
                            USE_FG=0
                        fi
                    fi
                    MYRIAD_DEV_UPDATER=1 start_all
                    ;;
                backend) start_backend ;;
                frontend) start_frontend ;;
                updater)
                    USE_NATIVE=0
                    start_updater
                    ;;
                *) print_error "Unknown service: $service"; exit 1 ;;
            esac ;;
        stop)
            case "$service" in
                all) stop_all ;; database|db) stop_database ;;
                all-updater|with-updater) stop_all ;;
                backend) stop_backend ;; frontend) stop_frontend ;; updater) stop_updater ;;
                *) print_error "Unknown service: $service"; exit 1 ;;
            esac ;;
        restart)
            case "$service" in
                all) restart_all ;;
                all-updater|with-updater)
                    USE_NATIVE=0
                    if [[ "${FG_EXPLICIT:-0}" -eq 0 && "$DETACH_EXPLICIT" -eq 0 ]]; then
                        USE_FG=0
                    fi
                    MYRIAD_DEV_UPDATER=1 restart_all
                    ;;
                database|db) stop_database; sleep 1; start_database ;;
                backend) stop_backend; sleep 1; start_backend ;;
                frontend) stop_frontend; sleep 1; start_frontend ;;
                updater) USE_NATIVE=0; stop_updater; sleep 1; start_updater ;;
                *) print_error "Unknown service: $service"; exit 1 ;;
            esac ;;
        status) show_mini_logo; show_status_dashboard ;;
        doctor) run_doctor ;;
        db-setup) run_db_setup ;;
        clean) clean_project ;;
        menu) run_interactive ;;
        help) show_help ;;
        *) print_error "Unknown command: $command"; echo ""; show_help; exit 1 ;;
    esac
}

main "$@"
