#!/bin/bash
# ============================================
# Myriad Development Script (Linux/macOS)
# ============================================
# Interactive development environment manager
# Usage: ./dev.sh [command] or run without args for menu

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
    printf "\033[2J\033[H"
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

# ==================== Logo & Banner ====================

show_logo() {
    echo -e "${BRIGHT_CYAN}"
    cat << 'EOF'
    __  ___           _           __
   /  |/  /_  _______(_)___ _____/ /
  / /|_/ / / / / ___/ / __ `/ __  / 
 / /  / / /_/ / /  / / /_/ / /_/ /  
/_/  /_/\__, /_/  /_/\__,_/\__,_/   
       /____/                        
EOF
    echo -e "${NC}"
    echo -e "${DIM}${ICON_SPARKLE} Multi-platform Personal Information Aggregation ${ICON_SPARKLE}${NC}"
    echo ""
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
            docker ps --filter "name=myriad-postgres" --format "{{.Names}}" 2>/dev/null | grep -q "myriad-postgres"
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
    print_step "Stopping PostgreSQL database..."
    cd "$PROJECT_ROOT"
    if [[ -f "docker-compose.dev.yml" ]]; then
        docker compose -f docker-compose.dev.yml stop postgres 2>/dev/null || true
        docker compose -f docker-compose.dev.yml rm -f postgres 2>/dev/null || true
        print_success "Database stopped"
    fi
}

start_updater() {
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

    local cargo_cmd="cargo run"
    # Prefer gateway (token injection) when harness is up; no UPDATE_TOKEN in backend env.
    local updater_url="http://127.0.0.1:1104"
    if dev_updater_enabled; then
        ensure_dev_updater_files
        cargo_cmd="MYRIAD_UPDATER_URL=$updater_url cargo run"
        print_info "Backend updater proxy via gateway: $updater_url (no UPDATE_TOKEN in backend)"
    fi

    if [[ "$OSTYPE" == "darwin"* ]]; then
        osascript -e 'tell application "Terminal" to do script "cd '"$BACKEND_DIR"' && echo \"🦀 Myriad Backend\" && source ~/.cargo/env 2>/dev/null; '"$cargo_cmd"'"' 2>/dev/null
    else
        if command -v gnome-terminal &> /dev/null; then
            gnome-terminal -- bash -c "cd '$BACKEND_DIR' && echo '🦀 Myriad Backend' && $cargo_cmd; exec bash" 2>/dev/null
        else
            if dev_updater_enabled; then
                MYRIAD_UPDATER_URL="$updater_url" nohup cargo run > "$PROJECT_ROOT/backend.log" 2>&1 &
            else
                nohup cargo run > "$PROJECT_ROOT/backend.log" 2>&1 &
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

# ==================== Combined Actions ====================

start_all() {
    echo ""
    echo -e "${BRIGHT_CYAN}${BOLD}${ICON_ROCKET} Starting All Services${NC}"
    echo ""

    if [[ "${MYRIAD_DEV_UPDATER:-}" == "1" ]]; then
        start_updater
        if get_service_status backend; then
            print_info "Restarting backend so it picks up updater dev environment..."
            stop_backend
            sleep 1
        fi
        echo ""
    fi
    
    start_database
    echo ""
    sleep 2
    start_backend
    if ! backend_health_ok; then
        echo ""
        print_error "Backend is not ready; frontend was not started to avoid 127.0.0.1:${BACKEND_PORT} ECONNREFUSED."
        print_info "Fix the backend error first, then run: ./scripts/dev/dev.sh start frontend"
        return 1
    fi
    echo ""
    start_frontend
    
    echo ""
    echo -e "${GREEN}${BOLD}${ICON_CHECK} All services started!${NC}"
    echo ""
    echo -e "${DIM}URLs:${NC}"
    echo -e "  ${CYAN}Frontend:${NC} http://localhost:${FRONTEND_PORT}"
    echo -e "  ${CYAN}Backend:${NC}  http://localhost:${BACKEND_PORT}"
    echo -e "  ${CYAN}Health:${NC}   http://localhost:${BACKEND_PORT}/health"
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
    if get_service_status database; then
        docker exec myriad-postgres-dev psql -U myriad -d myriad -c "
DO \$\$ DECLARE r RECORD;
BEGIN
    FOR r IN (SELECT tablename FROM pg_tables WHERE schemaname = 'public') LOOP
        EXECUTE 'DROP TABLE IF EXISTS ' || quote_ident(r.tablename) || ' CASCADE';
    END LOOP;
END \$\$;" > /dev/null 2>&1 || true
    fi
    sleep 0.3
    
    ((current++)); progress_bar $current $total 40 "Cleaning"
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
            1) start_all; echo -e "${DIM}Press Enter...${NC}"; read -r ;;
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
    echo -e "${BOLD}Usage:${NC} ./dev.sh [command] [service]"
    echo ""
    echo -e "${BOLD}Commands:${NC}"
    echo -e "  ${GREEN}start${NC}   [service]  Start services (default: all)"
    echo -e "  ${RED}stop${NC}    [service]  Stop services (default: all)"
    echo -e "  ${YELLOW}restart${NC} [service]  Restart services (default: all)"
    echo -e "  ${CYAN}status${NC}             Show service status"
    echo -e "  ${MAGENTA}clean${NC}              Clean build files and database"
    echo -e "  ${BLUE}menu${NC}               Open interactive menu"
    echo -e "  ${DIM}help${NC}               Show this help"
    echo ""
    echo -e "${BOLD}Services:${NC} database (db), backend, frontend, updater, all, all-updater"
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo -e "  ${DIM}./dev.sh${NC}                 # Open interactive menu"
    echo -e "  ${DIM}./dev.sh start${NC}           # Start all services"
    echo -e "  ${DIM}./dev.sh start all-updater${NC} # Start dev stack with updater harness"
    echo -e "  ${DIM}./dev.sh start updater${NC}   # Start updater harness only"
    echo -e "  ${DIM}./dev.sh start backend${NC}   # Start backend only"
    echo -e "  ${DIM}./dev.sh stop${NC}            # Stop all services"
    echo -e "  ${DIM}./dev.sh status${NC}          # Show status"
    echo ""
}

# ==================== Main ====================

main() {
    local command="${1:-}"
    local service="${2:-all}"
    
    [[ -z "$command" ]] && { run_interactive; exit 0; }
    
    case "$command" in
        start)
            case "$service" in
                all) start_all ;; database|db) start_database ;;
                all-updater|with-updater) MYRIAD_DEV_UPDATER=1 start_all ;;
                backend) start_backend ;; frontend) start_frontend ;; updater) start_updater ;;
                *) print_error "Unknown service: $service" ;;
            esac ;;
        stop)
            case "$service" in
                all) stop_all ;; database|db) stop_database ;;
                all-updater|with-updater) stop_all ;;
                backend) stop_backend ;; frontend) stop_frontend ;; updater) stop_updater ;;
                *) print_error "Unknown service: $service" ;;
            esac ;;
        restart)
            case "$service" in
                all) restart_all ;;
                all-updater|with-updater) MYRIAD_DEV_UPDATER=1 restart_all ;;
                database|db) stop_database; sleep 1; start_database ;;
                backend) stop_backend; sleep 1; start_backend ;;
                frontend) stop_frontend; sleep 1; start_frontend ;;
                updater) stop_updater; sleep 1; start_updater ;;
                *) print_error "Unknown service: $service" ;;
            esac ;;
        status) show_mini_logo; show_status_dashboard ;;
        clean) clean_project ;;
        menu) run_interactive ;;
        help|--help|-h) show_help ;;
        *) print_error "Unknown command: $command"; echo ""; show_help; exit 1 ;;
    esac
}

main "$@"
