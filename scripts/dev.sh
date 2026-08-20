#!/bin/bash
# ============================================
# Myriad Development Script (Linux/macOS)
# ============================================
# Single entry for local development. Default is a local PostgreSQL plus
# cargo / pnpm in this terminal. Pass --docker to use compose postgres.
# Usage: ./dev.sh [command] [service] [options]
#        run without args for the live TUI (start menu + monitor)

set -e

# ==================== Colors & Styles ====================
# Chrome matches shared/logo-ansi.txt (rose / dusk). Status stays semantic.
BOLD='\033[1m'
DIM='\033[38;2;108;90;100m'

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[38;2;232;196;160m'
BLUE='\033[38;2;167;134;139m'
MAGENTA='\033[38;2;223;155;152m'
CYAN='\033[38;2;230;169;168m'
BRIGHT_YELLOW='\033[38;2;246;179;175m'
BRIGHT_CYAN='\033[38;2;246;179;175m'
BRIGHT_WHITE='\033[38;2;242;184;183m'
NC='\033[0m' # Reset

# ==================== Project Config ====================
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Same number as backend/Cargo.toml — not a separate "script version".
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$PROJECT_ROOT/backend/Cargo.toml" 2>/dev/null | head -n 1)"
VERSION="${VERSION:-dev}"
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
DETACH_EXPLICIT=0     # 1 = user passed --detach
CARGO_RELEASE=0
SKIP_INSTALL=0
RUN_BACKEND=1
RUN_FRONTEND=1
DEV_START_BG=0        # 1 = nohup into *.log (TUI start)
DEV_START_NOWAIT=0    # 1 = launch and return (TUI keeps the screen)
WATCH=0               # 1 = live TUI for `status --watch`
NATIVE_PG_FORMULA=""
NATIVE_PG_FORMULA_CHECKED=0
NATIVE_PG_LOG=""
NATIVE_PG_LOG_CHECKED=0

DB_USER=""
DB_PASS=""
DB_HOST="localhost"
DB_PORT="5432"
DB_NAME=""

# ==================== Character Icons ====================
# ASCII / 1-cell only — no emoji (wide cells break the TUI).
ICON_CHECK="+"
ICON_CROSS="x"
ICON_ARROW=">"
ICON_ROCKET="*"
ICON_STOP="-"
ICON_REFRESH="o"
ICON_TRASH="x"
ICON_INFO="i"
ICON_WARN="!"
ICON_DB="#"
ICON_RUST="R"
ICON_NODE="N"
ICON_UPDATER="^"
ICON_HEART="*"

# ==================== Helper Functions ====================

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
    # TUI nowait: 1s then KILL. CLI: full KILL_WAIT_SECS.
    local max_ticks=$((KILL_WAIT_SECS * 4))
    [[ "$DEV_START_NOWAIT" -eq 1 ]] && max_ticks=4
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

# Background a command in $1, append stdio to $2. Sets LAUNCH_PID.
# Never wrap this in $(...) — bash waits for the child's pipe and freezes the TUI.
launch_logged() {
    local dir="$1" log="$2"
    shift 2
    (
        cd "$dir" || exit 1
        exec >>"$log" 2>&1 </dev/null
        exec "$@"
    ) &
    LAUNCH_PID=$!
}

# True if this PID belongs to this checkout (argv path or cwd).
# The compiled binary lives at $PROJECT_ROOT/target/..., not $BACKEND_DIR.
in_project_tree() {
    local pid="$1" cmd="$2" cwd
    [[ -n "$cmd" && "$cmd" == *"$PROJECT_ROOT"* ]] && return 0
    cwd="$(proc_cwd "$pid" 2>/dev/null || true)"
    [[ -n "$cwd" && ( "$cwd" == "$PROJECT_ROOT" || "$cwd" == "$PROJECT_ROOT"/* ) ]]
}

# Match backend via:
#   1) myriad-backend, or `cargo run` / cargo-watch, in this project's tree
#   2) whoever is listening on BACKEND_PORT (must free port to restart)
# rust-analyzer `cargo check` is not a match.
list_backend_pids() {
    local -a found=()
    local pid cmd

    while IFS= read -r line; do
        # pid is first field; remainder is command (may contain spaces)
        pid="${line%% *}"
        cmd="${line#${pid}}"
        cmd="${cmd#"${cmd%%[![:space:]]*}"}" # ltrim
        [[ "$pid" =~ ^[0-9]+$ ]] || continue

        local match=0
        # Only the running server — not `cargo check` / rust-analyzer.
        if [[ "$cmd" == *myriad-backend* ]]; then
            in_project_tree "$pid" "$cmd" && match=1
        elif [[ "$cmd" == *"cargo run"* || "$cmd" == *"cargo-watch"* || "$cmd" == *"cargo watch"* ]]; then
            if [[ "$cmd" != *"cargo check"* && "$cmd" != *"cargo clippy"* ]]; then
                in_project_tree "$pid" "$cmd" && match=1
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

# Match frontend via `pnpm run dev` / astro / vite in this tree, plus FRONTEND_PORT.
list_frontend_pids() {
    local -a found=()
    local pid cmd

    while IFS= read -r line; do
        pid="${line%% *}"
        cmd="${line#${pid}}"
        cmd="${cmd#"${cmd%%[![:space:]]*}"}"
        [[ "$pid" =~ ^[0-9]+$ ]] || continue

        local match=0
        # Dev server only — not eslint / tsc / `astro check` / `vite build`.
        if [[ "$cmd" == *"pnpm run dev"* || "$cmd" == *"pnpm dev"* || \
              ( "$cmd" == *astro* && "$cmd" == *dev* ) || \
              ( "$cmd" == *vite* && "$cmd" != *build* && "$cmd" != *preview* ) ]]; then
            in_project_tree "$pid" "$cmd" && match=1
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

# Repeat a UTF-8 glyph n times. macOS/GNU tr are byte-oriented and
# turn U+2500 / U+2588 into garbage — use awk instead.
repeat_glyph() {
    local n="$1" g="$2"
    [[ "$n" =~ ^[0-9]+$ && -n "$g" ]] || return 0
    [[ "$n" -gt 0 ]] || return 0
    awk -v n="$n" -v g="$g" 'BEGIN { while (n-- > 0) printf "%s", g }'
}

# Draw a box
draw_box() {
    local title="$1"
    local width="${2:-60}"
    local color="${3:-$CYAN}"

    echo -en "${color}╭"
    repeat_glyph $((width-2)) "─"
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
        repeat_glyph $((width-2)) "─"
        echo -e "┤${NC}"
    fi
}

draw_box_bottom() {
    local width="${1:-60}"
    local color="${2:-$CYAN}"
    echo -en "${color}╰"
    repeat_glyph $((width-2)) "─"
    echo -e "╯${NC}"
}

draw_box_line() {
    local text="$1"
    local width="${2:-60}"
    local color="${3:-$CYAN}"

    # Remove ANSI codes for length calculation
    local clean_text
    clean_text="$(printf '%s' "$(echo -e "$text")" | sed $'s/\x1b\\[[0-9;]*m//g')"
    local text_len inner
    if type tui_visible_len >/dev/null 2>&1; then
        text_len="$(tui_visible_len "$clean_text")"
    else
        text_len=${#clean_text}
    fi
    inner=$((width - 3))
    if [[ $text_len -gt $inner ]]; then
        if type tui_trunc >/dev/null 2>&1; then
            text="$(tui_trunc "$text" "$inner")"
            text_len="$(tui_visible_len "$(tui_strip "$text")")"
        else
            clean_text="${clean_text:0:$((inner - 1))}…"
            text="${clean_text}"
            text_len=${#clean_text}
        fi
    fi

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
    [[ $filled -gt 0 ]] && printf '%b%s%b' "$GREEN" "$(repeat_glyph "$filled" "█")" "$NC"
    [[ $empty -gt 0 ]] && printf '%b%s%b' "$DIM" "$(repeat_glyph "$empty" "░")" "$NC"
    printf "] ${BRIGHT_WHITE}%3d%%${NC}" $percent
}

# Print styled messages
print_success() { echo -e "${GREEN}${ICON_CHECK}${NC} $1"; }
print_error() { echo -e "${RED}${ICON_CROSS}${NC} $1"; }
print_info() { echo -e "${CYAN}${ICON_INFO}${NC} $1"; }
print_warning() { echo -e "${YELLOW}${ICON_WARN}${NC} $1"; }
print_step() { echo -e "${MAGENTA}${ICON_ARROW}${NC} $1"; }

have() { command -v "$1" >/dev/null 2>&1; }

# Run a command with cwd set, without moving this shell (TUI stays put).
run_in_dir() {
    local dir="$1"
    shift
    ( cd "$dir" && "$@" )
}

# Fire-and-forget for TUI nowait. Do not wrap in $(...).
run_in_dir_detached() {
    local dir="$1"
    shift
    ( cd "$dir" && exec "$@" ) >/dev/null 2>&1 &
}

# Never print secrets that cargo / compose / rustc may echo.
sanitize_dev_log() {
    sed -E \
        -e 's/((PASSWORD|SECRET|TOKEN)[=:[:space:]]+)[^[:space:]]+/\1***/g' \
        -e 's#(postgres(ql)?://[^:/@]+:)[^@[:space:]]+@#\1***@#g'
}

docker_cli_available() { have docker; }

# `docker ps` blocks for tens of seconds when the CLI exists but the daemon
# is stopped or still coming up. Require a live ping, not just a socket.
docker_usable() {
    docker_cli_available || return 1
    local sock=""
    if [[ -S /var/run/docker.sock ]]; then
        sock=/var/run/docker.sock
    elif [[ -S "${HOME}/.docker/run/docker.sock" ]]; then
        sock="${HOME}/.docker/run/docker.sock"
    fi
    # Socket / DOCKER_HOST can exist while Desktop is starting. Cap the wait.
    if [[ -n "$sock" ]] && have curl; then
        curl -fsS --unix-socket "$sock" --max-time 1 http://localhost/_ping >/dev/null 2>&1
        return
    fi
    if [[ -n "${DOCKER_HOST:-}" ]] && have curl; then
        curl -fsS --max-time 1 "${DOCKER_HOST}/_ping" >/dev/null 2>&1
        return
    fi
    [[ -n "$sock" || -n "${DOCKER_HOST:-}" ]]
}

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

# Prefer PATH, then the keg / Homebrew prefix (Finder-launched Terminal
# often has no /opt/homebrew/bin).
find_pg_tool() {
    local name="$1" formula root
    have "$name" && { command -v "$name"; return 0; }
    native_pg_brew_formula >/dev/null || true
    formula="${NATIVE_PG_FORMULA:-}"
    for root in /opt/homebrew /usr/local; do
        if [[ -n "$formula" && -x "$root/opt/$formula/bin/$name" ]]; then
            printf '%s\n' "$root/opt/$formula/bin/$name"
            return 0
        fi
        if [[ -x "$root/bin/$name" ]]; then
            printf '%s\n' "$root/bin/$name"
            return 0
        fi
    done
    return 1
}

brew_bin() {
    have brew && { command -v brew; return 0; }
    [[ -x /opt/homebrew/bin/brew ]] && { printf '%s\n' /opt/homebrew/bin/brew; return 0; }
    [[ -x /usr/local/bin/brew ]] && { printf '%s\n' /usr/local/bin/brew; return 0; }
    return 1
}

# 0 = reachable, 1 = not, 2 = no client tools to probe.
db_reachable() {
    parse_db_url || return 1
    local tool
    tool="$(find_pg_tool pg_isready || true)"
    if [[ -n "$tool" ]]; then
        # pg_isready uses 2 for "no response" — that is down, not "no tools".
        if "$tool" -q -t 2 -h "$DB_HOST" -p "$DB_PORT" -d "$DB_NAME" -U "$DB_USER" >/dev/null 2>&1; then
            return 0
        fi
        return 1
    fi
    tool="$(find_pg_tool psql || true)"
    if [[ -n "$tool" ]]; then
        if PGCONNECT_TIMEOUT=2 PGPASSWORD="$DB_PASS" "$tool" -w "$(resolve_db_url)" -tAc 'SELECT 1' >/dev/null 2>&1; then
            return 0
        fi
        return 1
    fi
    return 2
}

db_is_local_host() {
    parse_db_url || return 1
    case "$DB_HOST" in
        localhost|127.0.0.1|::1) return 0 ;;
        *) return 1 ;;
    esac
}

# Homebrew formula that owns local PostgreSQL, if any.
native_pg_brew_formula() {
    if [[ -n "$NATIVE_PG_FORMULA" ]]; then
        printf '%s\n' "$NATIVE_PG_FORMULA"
        return 0
    fi
    [[ "$NATIVE_PG_FORMULA_CHECKED" -eq 1 ]] && return 1

    local formula prefix line root
    for formula in postgresql@18 postgresql@17 postgresql@16 postgresql; do
        for root in /opt/homebrew /usr/local; do
            if [[ -d "$root/opt/$formula" ]]; then
                NATIVE_PG_FORMULA="$formula"
                printf '%s\n' "$formula"
                return 0
            fi
        done
    done

    have brew || { NATIVE_PG_FORMULA_CHECKED=1; return 1; }

    for formula in postgresql@18 postgresql@17 postgresql@16 postgresql; do
        prefix="$(brew --prefix "$formula" 2>/dev/null || true)"
        if [[ -n "$prefix" && -d "$prefix" ]]; then
            NATIVE_PG_FORMULA="$formula"
            printf '%s\n' "$formula"
            return 0
        fi
    done

    while IFS= read -r line; do
        formula="${line%% *}"
        case "$formula" in
            postgresql|postgresql@*)
                NATIVE_PG_FORMULA="$formula"
                printf '%s\n' "$formula"
                return 0
                ;;
        esac
    done < <(brew services list 2>/dev/null)

    formula="$(brew list --formula 2>/dev/null | grep -E '^postgresql(@[0-9]+)?$' | tail -n 1 || true)"
    if [[ -n "$formula" ]]; then
        NATIVE_PG_FORMULA="$formula"
        printf '%s\n' "$formula"
        return 0
    fi
    NATIVE_PG_FORMULA_CHECKED=1
    return 1
}

# Homebrew postgres log, if the formula keeps one on disk.
native_pg_log_path() {
    local formula log root
    if [[ -n "${NATIVE_PG_LOG:-}" ]]; then
        printf '%s' "$NATIVE_PG_LOG"
        return 0
    fi
    [[ "${NATIVE_PG_LOG_CHECKED:-0}" -eq 1 ]] && return 1
    native_pg_brew_formula >/dev/null || true
    formula="${NATIVE_PG_FORMULA:-postgresql@18}"
    for root in /opt/homebrew /usr/local; do
        for log in \
            "$root/var/log/${formula}.log" \
            "$root/var/log/postgresql@18.log" \
            "$root/var/log/postgresql.log"
        do
            if [[ -f "$log" ]]; then
                NATIVE_PG_LOG="$log"
                printf '%s' "$log"
                return 0
            fi
        done
    done
    NATIVE_PG_LOG_CHECKED=1
    return 1
}

wait_for_database() {
    local timeout="${1:-20}" elapsed=0 rc
    while [[ $elapsed -lt $timeout ]]; do
        rc=0
        db_reachable || rc=$?
        [[ $rc -eq 0 ]] && return 0
        [[ $rc -eq 2 ]] && return 2
        sleep 1
        elapsed=$((elapsed + 1))
    done
    db_reachable
}

native_pg_datadir() {
    local formula="$1" root
    for root in /opt/homebrew /usr/local; do
        if [[ -d "$root/var/$formula" ]]; then
            printf '%s\n' "$root/var/$formula"
            return 0
        fi
    done
    return 1
}

native_pg_do_start() {
    local formula="$1" brew_cmd pgctl datadir root
    brew_cmd="$(brew_bin || true)"
    if [[ -n "$brew_cmd" ]]; then
        "$brew_cmd" services start "$formula"
        return
    fi
    datadir="$(native_pg_datadir "$formula" || true)"
    for root in /opt/homebrew /usr/local; do
        if [[ -x "$root/opt/$formula/bin/pg_ctl" ]]; then
            pgctl="$root/opt/$formula/bin/pg_ctl"
            break
        fi
    done
    if [[ -z "$pgctl" || -z "$datadir" ]]; then
        print_error "Found $formula but neither brew nor pg_ctl could start it"
        return 1
    fi
    print_info "brew not on PATH — starting with pg_ctl"
    "$pgctl" -D "$datadir" -l "$datadir/server.log" start
}

start_native_database() {
    parse_db_url || return 1
    if ! db_is_local_host; then
        print_error "DATABASE_URL host is $DB_HOST — not started by this script"
        return 1
    fi

    local rc=0
    db_reachable || rc=$?
    if [[ "$rc" -eq 0 ]]; then
        print_success "Local PostgreSQL reachable ($DB_USER@$DB_HOST:$DB_PORT/$DB_NAME)"
        return 0
    fi
    if [[ "$rc" -eq 2 ]]; then
        print_warning "No pg_isready/psql in PATH — will still try to start PostgreSQL"
    fi

    local formula
    native_pg_brew_formula >/dev/null || true
    formula="$NATIVE_PG_FORMULA"
    if [[ -n "$formula" ]]; then
        print_step "Starting Homebrew $formula…"
        native_pg_do_start "$formula" || return 1
        if wait_for_database 25; then
            print_success "Local PostgreSQL reachable ($DB_USER@$DB_HOST:$DB_PORT/$DB_NAME)"
            return 0
        fi
        print_error "Started $formula, but $DB_USER@$DB_HOST:$DB_PORT/$DB_NAME is still unreachable"
        print_info "Create the role + database: $0 db-setup"
        return 1
    fi

    print_error "Local PostgreSQL is not running and no Homebrew postgresql formula was found"
    print_info "Install/start PostgreSQL, then: $0 db-setup"
    return 1
}

native_pg_do_stop() {
    local formula="$1" brew_cmd pgctl="" datadir="" root
    datadir="$(native_pg_datadir "$formula" || true)"
    for root in /opt/homebrew /usr/local; do
        if [[ -x "$root/opt/$formula/bin/pg_ctl" ]]; then
            pgctl="$root/opt/$formula/bin/pg_ctl"
            break
        fi
    done
    # Stop the postmaster first and wait — restart_all starts immediately after.
    if [[ -n "$pgctl" && -n "$datadir" ]]; then
        if db_reachable || [[ -n "$(list_listen_pids "${DB_PORT:-5432}")" ]]; then
            print_info "Stopping $formula with pg_ctl"
            "$pgctl" -D "$datadir" stop -m fast || true
        fi
    fi
    brew_cmd="$(brew_bin || true)"
    if [[ -n "$brew_cmd" ]]; then
        "$brew_cmd" services stop "$formula" >/dev/null 2>&1 || true
    fi
}

stop_native_database() {
    parse_db_url || return 0
    if ! db_is_local_host; then
        print_info "Remote PostgreSQL ($DB_HOST) is not managed by this script"
        return 0
    fi
    local formula
    native_pg_brew_formula >/dev/null || true
    formula="$NATIVE_PG_FORMULA"
    if [[ -z "$formula" ]]; then
        print_info "Local PostgreSQL is not a Homebrew service (left running)"
        return 0
    fi
    print_step "Stopping Homebrew $formula (all databases on :$DB_PORT)…"
    native_pg_do_stop "$formula"

    local i=0 rc
    while [[ $i -lt 15 ]]; do
        rc=0
        db_reachable || rc=$?
        if [[ $rc -ne 0 && -z "$(list_listen_pids "${DB_PORT:-5432}")" ]]; then
            print_success "Database stopped"
            return 0
        fi
        sleep 1
        i=$((i + 1))
    done
    if db_reachable || [[ -n "$(list_listen_pids "${DB_PORT:-5432}")" ]]; then
        print_error "PostgreSQL still listening on :$DB_PORT after stop"
        return 1
    fi
    print_success "Database stopped"
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
# Myriad backend — LOCAL DEVELOPMENT ONLY (generated by scripts/dev.sh)
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
            if docker_usable; then
                print_success "daemon   reachable"
            else
                print_warning "daemon   not running — start Docker Desktop, or use --native"
            fi
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

show_mini_logo() {
    echo -e "${BRIGHT_CYAN}${BOLD}* Myriad${NC} ${DIM}v${VERSION}${NC}"
}

# ==================== Status Functions ====================

get_service_status() {
    local service=$1
    case $service in
        backend)
            backend_port_in_use || [[ -n "$(list_backend_pids)" ]]
            ;;
        frontend)
            frontend_port_in_use || [[ -n "$(list_frontend_pids)" ]]
            ;;
        database)
            if [[ "$USE_NATIVE" -eq 1 ]]; then
                db_reachable
                return
            fi
            if docker_container_running "myriad-postgres-dev"; then
                return 0
            fi
            db_reachable
            ;;
        updater)
            docker_container_running "myriad-updater-dev"
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

wait_for_port() {
    local port="$1" tries="${2:-8}" i=0
    while [[ $i -lt $tries ]]; do
        [[ -n "$(list_listen_pids "$port")" ]] && return 0
        sleep 0.25
        i=$((i + 1))
    done
    return 1
}

backend_health_ok() {
    curl -fsS --connect-timeout 0.2 --max-time 0.4 \
        "http://127.0.0.1:${BACKEND_PORT}/health" >/dev/null 2>&1
}

frontend_health_ok() {
    # Astro often binds IPv6 *:port. Probe both stacks; ignore HTTP status.
    local url
    for url in \
        "http://127.0.0.1:${FRONTEND_PORT}/" \
        "http://localhost:${FRONTEND_PORT}/"
    do
        curl -sS -o /dev/null --connect-timeout 0.2 --max-time 0.4 "$url" >/dev/null 2>&1 && return 0
    done
    return 1
}

# psql against the resolved DATABASE_URL. Extra args forwarded (e.g. -tAc 'SELECT 1').
db_psql() {
    parse_db_url || return 1
    have psql || return 1
    PGCONNECT_TIMEOUT=2 PGPASSWORD="$DB_PASS" psql -w -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" "$@"
}

db_scalar() {
    db_psql -tAc "$1" 2>/dev/null | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'
}

# First line of `ps -p` snapshot: pid ppid pcpu pmem rss etime command
proc_ps() {
    local pid="$1"
    [[ "$pid" =~ ^[0-9]+$ ]] || return 1
    ps -p "$pid" -o pid=,ppid=,pcpu=,pmem=,rss=,etime=,command= 2>/dev/null | head -n 1
}

docker_container_running() {
    local name="$1"
    docker_usable || return 1
    docker ps --filter "name=^${name}$" --format "{{.Names}}" 2>/dev/null | grep -q .
}

db_is_docker() {
    [[ "$USE_NATIVE" -eq 1 ]] && return 1
    docker_container_running "myriad-postgres-dev"
}

backend_port_in_use() {
    [[ -n "$(list_listen_pids "$BACKEND_PORT")" ]]
}

# Compiled server process (not `cargo run` still compiling).
backend_binary_up() {
    local p cmd
    while IFS= read -r p; do
        [[ "$p" =~ ^[0-9]+$ ]] || continue
        cmd="$(ps -p "$p" -o command= 2>/dev/null)"
        [[ "$cmd" == *myriad-backend* ]] && return 0
    done < <(list_backend_pids)
    return 1
}

frontend_port_in_use() {
    [[ -n "$(list_listen_pids "$FRONTEND_PORT")" ]]
}

updater_health_ok() {
    curl -fsS --connect-timeout 0.2 --max-time 0.4 \
        "http://127.0.0.1:1101/healthz" >/dev/null 2>&1
}

wait_for_backend() {
    local timeout="${1:-180}"
    local elapsed=0
    print_info "Waiting for backend health on http://127.0.0.1:${BACKEND_PORT}/health ..."
    print_info "First cargo compile can take a few minutes — this is not a crash."
    while [[ $elapsed -lt $timeout ]]; do
        if backend_health_ok; then
            print_success "Backend is ready"
            return 0
        fi
        # Still compiling / booting is success-in-progress if a process exists.
        if [[ $((elapsed % 15)) -eq 0 && $elapsed -gt 0 ]]; then
            if [[ -n "$(list_backend_pids)" ]]; then
                print_info "Still starting… ${elapsed}s / ${timeout}s (cargo may be compiling)"
            else
                print_info "No backend process yet… ${elapsed}s / ${timeout}s"
            fi
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
    mkdir -p "$DEV_UPDATER_DIR/state" "$DEV_UPDATER_DIR/pgdata" "$DEV_UPDATER_DIR/backups" \
        "$DEV_UPDATER_DIR/guard-policy"

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

    cat > "$DEV_UPDATER_DIR/guard-policy/docker-guard.env" <<'EOF'
DOCKER_GUARD_IMAGE=myriad-updater-dev:v0.0.0-dev
GUARD_COMPOSE_PROJECT_NAME=myriad-dev-updater
GUARD_MYRIAD_DOCKER_NETWORK=myriad-dev-updater_default
GUARD_MYRIAD_ADMIN_NETWORK=myriad-dev-admin-net
GUARD_MYRIAD_DOCKER_GUARD_NETWORK=myriad-dev-docker-guard-net
MYRIAD_GUARD_ENV_FILE=guard-policy/docker-guard.env
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
    [[ "$USE_NATIVE" -eq 1 ]] && return 1
    [[ "${MYRIAD_DEV_UPDATER:-}" == "1" ]] && return 0
    get_service_status updater
}

# One-line detail for status: "pid 1234  cpu 1.2%  mem 3.4%  up 00:12"
status_proc_detail() {
    local pid="$1"
    local line cpu mem etime
    line="$(proc_ps "$pid" || true)"
    [[ -n "$line" ]] || { echo "pid $pid"; return 0; }
    # pid ppid pcpu pmem rss etime command — fields 1,3,4,6
    set -- $line
    cpu="${3:-?}"
    mem="${4:-?}"
    etime="${6:-?}"
    echo "pid $pid  cpu ${cpu}%  mem ${mem}%  up $etime"
}

show_status_dashboard() {
    local width=70

    echo ""
    draw_box "Service Status" $width "$BRIGHT_CYAN"

    # Database
    local db_status=false
    get_service_status database && db_status=true
    local db_kind="local"
    db_is_docker && db_kind="docker"
    draw_box_line "${ICON_DB} Database (PostgreSQL)    $(get_status_text $db_status)" $width "$BRIGHT_CYAN"
    if $db_status && parse_db_url; then
        draw_box_line "    ${DIM}${db_kind}  ${DB_USER}@${DB_HOST}:${DB_PORT}/${DB_NAME}${NC}" $width "$BRIGHT_CYAN"
        if have psql; then
            local db_size db_conns db_tables db_ver
            db_size="$(db_scalar "SELECT pg_size_pretty(pg_database_size(current_database()))" || true)"
            db_conns="$(db_scalar "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database()" || true)"
            db_tables="$(db_scalar "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_type = 'BASE TABLE'" || true)"
            db_ver="$(db_scalar "SELECT split_part(current_setting('server_version'), ' ', 1)" || true)"
            [[ -n "$db_size" ]] && draw_box_line "    ${DIM}v${db_ver}  ${db_size}  ${db_conns} conn  ${db_tables} tables${NC}" $width "$BRIGHT_CYAN"
        fi
    fi

    # Backend
    local backend_status=false
    get_service_status backend && backend_status=true
    local be_health="${DIM}—${NC}"
    if $backend_status; then
        if backend_health_ok; then
            be_health="${GREEN}healthy${NC}"
        else
            be_health="${YELLOW}starting / unhealthy${NC}"
        fi
    fi
    draw_box_line "${ICON_RUST} Backend (Rust/Axum)      $(get_status_text $backend_status)  ${be_health}" $width "$BRIGHT_CYAN"
    if $backend_status; then
        local be_pid
        be_pid="$(list_backend_pids | head -n 1)"
        [[ -n "$be_pid" ]] && draw_box_line "    ${DIM}$(status_proc_detail "$be_pid")${NC}" $width "$BRIGHT_CYAN"
    fi

    # Frontend
    local frontend_status=false
    get_service_status frontend && frontend_status=true
    local fe_health="${DIM}—${NC}"
    if $frontend_status; then
        if frontend_health_ok; then
            fe_health="${GREEN}up${NC}"
        else
            fe_health="${YELLOW}starting${NC}"
        fi
    fi
    draw_box_line "${ICON_NODE} Frontend (Astro/React)   $(get_status_text $frontend_status)  ${fe_health}" $width "$BRIGHT_CYAN"
    if $frontend_status; then
        local fe_pid
        fe_pid="$(list_frontend_pids | head -n 1)"
        [[ -n "$fe_pid" ]] && draw_box_line "    ${DIM}$(status_proc_detail "$fe_pid")${NC}" $width "$BRIGHT_CYAN"
    fi

    # Updater harness
    local updater_status=false
    get_service_status updater && updater_status=true
    local up_health="${DIM}—${NC}"
    if $updater_status; then
        if updater_health_ok; then
            up_health="${GREEN}healthy${NC}"
        else
            up_health="${YELLOW}starting / unhealthy${NC}"
        fi
    fi
    draw_box_line "${ICON_UPDATER} Updater Harness          $(get_status_text $updater_status)  ${up_health}" $width "$BRIGHT_CYAN"

    draw_box_line "" $width "$BRIGHT_CYAN"

    if $backend_status || $frontend_status || $updater_status; then
        $backend_status && draw_box_line "${DIM}API:      http://localhost:${BACKEND_PORT}   /health${NC}" $width "$BRIGHT_CYAN"
        $frontend_status && draw_box_line "${DIM}Frontend: http://localhost:${FRONTEND_PORT}${NC}" $width "$BRIGHT_CYAN"
        $updater_status && draw_box_line "${DIM}Updater:  http://127.0.0.1:1101   gateway :1104${NC}" $width "$BRIGHT_CYAN"
    fi

    draw_box_bottom $width "$BRIGHT_CYAN"
    echo ""
}

show_logs() {
    local which="${1:-all}"
    local lines="${2:-80}"
    case "$which" in
        backend|be)
            if [[ -f "$PROJECT_ROOT/backend.log" ]]; then
                echo -e "${BOLD}backend.log${NC} (last $lines)"
                tail -n "$lines" "$PROJECT_ROOT/backend.log" | sanitize_dev_log
            else
                print_warning "No backend.log — backend may have been started in another terminal"
            fi
            ;;
        frontend|fe)
            if [[ -f "$PROJECT_ROOT/frontend.log" ]]; then
                echo -e "${BOLD}frontend.log${NC} (last $lines)"
                tail -n "$lines" "$PROJECT_ROOT/frontend.log" | sanitize_dev_log
            else
                print_warning "No frontend.log — frontend may have been started in another terminal"
            fi
            ;;
        database|db|postgres)
            if docker_container_running "myriad-postgres-dev"; then
                echo -e "${BOLD}docker logs myriad-postgres-dev${NC} (last $lines)"
                docker logs --tail "$lines" myriad-postgres-dev 2>&1 | sanitize_dev_log
            else
                local pglog
                pglog="$(native_pg_log_path 2>/dev/null || true)"
                if [[ -n "$pglog" && -f "$pglog" ]]; then
                    echo -e "${BOLD}$pglog${NC} (last $lines)"
                    tail -n "$lines" "$pglog" | sanitize_dev_log
                else
                    print_info "Native PostgreSQL has no compose log — no local postgres log found"
                fi
            fi
            ;;
        updater)
            if docker_container_running "myriad-updater-dev"; then
                echo -e "${BOLD}docker logs myriad-updater-dev${NC} (last $lines)"
                docker logs --tail "$lines" myriad-updater-dev 2>&1 | sanitize_dev_log
            else
                print_warning "Updater harness is not running"
            fi
            ;;
        all|*)
            show_logs backend "$lines"
            echo ""
            show_logs frontend "$lines"
            echo ""
            show_logs database "$lines"
            ;;
    esac
}

# ==================== Service Control ====================

start_database() {
    maybe_autodetect_native
    parse_db_url || true

    if [[ "$USE_NATIVE" -eq 1 ]]; then
        print_step "Starting local PostgreSQL..."
        start_native_database
        return
    fi

    print_step "Starting PostgreSQL database..."

    if get_service_status database; then
        print_warning "Database is already running"
        return 0
    fi

    if ! docker_usable; then
        print_error "Docker daemon is not reachable"
        return 1
    fi
    if [[ ! -f "$PROJECT_ROOT/docker-compose.dev.yml" ]]; then
        print_error "docker-compose.dev.yml not found"
        return 1
    fi
    run_in_dir "$PROJECT_ROOT" docker compose -f docker-compose.dev.yml up -d
    if wait_for_database 25; then
        print_success "Database started"
        return 0
    fi
    print_error "Failed to start database"
    return 1
}

stop_database() {
    local stopped_docker=0
    if docker_container_running "myriad-postgres-dev"; then
        print_step "Stopping PostgreSQL database..."
        run_in_dir "$PROJECT_ROOT" docker compose -f docker-compose.dev.yml stop postgres || true
        run_in_dir "$PROJECT_ROOT" docker compose -f docker-compose.dev.yml rm -f postgres || true
        print_success "Database stopped"
        stopped_docker=1
    fi
    if [[ "$USE_NATIVE" -eq 1 && "$stopped_docker" -eq 0 ]]; then
        stop_native_database
    fi
}

start_updater() {
    if [[ "$USE_NATIVE" -eq 1 ]]; then
        print_error "Updater harness is Docker-only (not available with --native)"
        return 1
    fi
    if ! docker_usable; then
        print_error "Docker daemon is not reachable"
        return 1
    fi

    print_step "Starting updater dev harness..."

    if get_service_status updater; then
        print_warning "Updater harness is already running"
        return 0
    fi

    ensure_dev_updater_files
    if [[ "$DEV_START_NOWAIT" -eq 1 ]]; then
        UPDATE_TOKEN="$(dev_updater_token)" \
            run_in_dir_detached "$PROJECT_ROOT" docker compose -f docker-compose.dev.yml --profile updater \
            up -d docker-guard updater updater-gateway
        print_info "Updater harness launching (gateway :1104)"
        return 0
    fi
    UPDATE_TOKEN="$(dev_updater_token)" \
        run_in_dir "$PROJECT_ROOT" docker compose -f docker-compose.dev.yml --profile updater up -d docker-guard updater updater-gateway
    if ! wait_for_updater 120; then
        print_warning "Updater did not become ready within 120s"
        print_info "Check logs with: docker compose -f docker-compose.dev.yml --profile updater logs -f docker-guard updater updater-gateway"
        return 1
    fi

    print_success "Updater harness started (gateway on 127.0.0.1:1104)"
    print_info "Backend will use it when started/restarted while the harness is running."
}

stop_updater() {
    if ! docker_usable; then
        print_info "Docker is not running — updater left as-is"
        return 0
    fi
    print_step "Stopping updater dev harness..."
    if [[ "$DEV_START_NOWAIT" -eq 1 ]]; then
        run_in_dir_detached "$PROJECT_ROOT" docker compose -f docker-compose.dev.yml --profile updater \
            stop updater-gateway updater docker-guard
        print_info "Updater harness stopping"
        return 0
    fi
    run_in_dir "$PROJECT_ROOT" docker compose -f docker-compose.dev.yml --profile updater stop updater-gateway updater docker-guard || true
    run_in_dir "$PROJECT_ROOT" docker compose -f docker-compose.dev.yml --profile updater rm -f updater-gateway updater docker-guard || true
    print_success "Updater harness stopped"
}

start_backend() {
    print_step "Starting Rust backend..."
    have cargo || { print_error "cargo not found — install Rust: https://rustup.rs"; return 1; }

    # Start means start — already up is a no-op. Restart is a separate action.
    if backend_port_in_use || backend_binary_up || [[ -n "$(list_backend_pids)" ]]; then
        print_warning "Backend is already running"
        return 0
    fi

    ensure_backend_env || return 1

    local -a run_cmd=(cargo run)
    [[ "$CARGO_RELEASE" -eq 1 ]] && run_cmd=(cargo run --release)
    local updater_url="http://127.0.0.1:1104"
    local gw_secret=""
    local inject_updater=0
    if dev_updater_enabled; then
        ensure_dev_updater_files
        gw_secret="$(dev_updater_gateway_secret)"
        inject_updater=1
        print_info "Backend updater proxy via gateway: $updater_url (gateway secret; no UPDATE_TOKEN)"
    fi
    if [[ "$CARGO_RELEASE" -eq 1 ]]; then
        print_info "Backend will run in release mode"
    fi

    if [[ "$DEV_START_BG" -eq 1 ]]; then
        if [[ "$inject_updater" -eq 1 ]]; then
            launch_logged "$BACKEND_DIR" "$PROJECT_ROOT/backend.log" \
                env MYRIAD_UPDATER_URL="$updater_url" UPDATER_GATEWAY_SECRET="$gw_secret" \
                "${run_cmd[@]}"
        else
            launch_logged "$BACKEND_DIR" "$PROJECT_ROOT/backend.log" "${run_cmd[@]}"
        fi
        print_info "Backend running in background (pid $LAUNCH_PID, logs: backend.log)"
        return 0
    fi

    local cargo_cmd="${run_cmd[*]}"
    if [[ "$inject_updater" -eq 1 ]]; then
        cargo_cmd="MYRIAD_UPDATER_URL=$updater_url UPDATER_GATEWAY_SECRET=$gw_secret $cargo_cmd"
    fi
    if [[ "$OSTYPE" == "darwin"* ]]; then
        osascript -e 'tell application "Terminal" to do script "cd '"$BACKEND_DIR"' && echo \"* Myriad Backend\" && source ~/.cargo/env 2>/dev/null; '"$cargo_cmd"'"' 2>/dev/null
    elif command -v gnome-terminal &> /dev/null; then
        gnome-terminal -- bash -c "cd '$BACKEND_DIR' && echo '* Myriad Backend' && $cargo_cmd; exec bash" 2>/dev/null
    else
        if [[ "$inject_updater" -eq 1 ]]; then
            launch_logged "$BACKEND_DIR" "$PROJECT_ROOT/backend.log" \
                env MYRIAD_UPDATER_URL="$updater_url" UPDATER_GATEWAY_SECRET="$gw_secret" \
                "${run_cmd[@]}"
        else
            launch_logged "$BACKEND_DIR" "$PROJECT_ROOT/backend.log" "${run_cmd[@]}"
        fi
        print_info "Backend running in background (pid $LAUNCH_PID, logs: backend.log)"
        return 0
    fi

    if [[ "$DEV_START_NOWAIT" -eq 1 ]]; then
        return 0
    fi
    if ! wait_for_backend 180; then
        if [[ -n "$(list_backend_pids)" ]] || backend_port_in_use; then
            print_warning "Backend is still compiling or booting after 180s — leaving it running"
            print_info "Watch: $0 logs backend   or   tail -f $PROJECT_ROOT/backend.log"
        else
            print_warning "Backend did not become ready within 180s and no process is visible"
            print_warning "Check the backend terminal, or backend.log if it was started in background."
        fi
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

ensure_frontend_deps() {
    [[ "$SKIP_INSTALL" -eq 1 ]] && return 0
    [[ -d "$FRONTEND_DIR/node_modules" ]] && return 0
    if [[ "$DEV_START_NOWAIT" -eq 1 ]]; then
        print_error "frontend/node_modules missing — run: (cd frontend && pnpm install)"
        return 1
    fi
    print_step "Installing frontend dependencies…"
    ( cd "$FRONTEND_DIR" && pnpm install ) || return 1
}

start_frontend() {
    print_step "Starting Astro frontend..."
    have pnpm || { print_error "pnpm not found — run 'corepack enable'"; return 1; }

    if frontend_port_in_use || [[ -n "$(list_frontend_pids)" ]]; then
        print_warning "Frontend is already running"
        return 0
    fi

    ensure_frontend_deps || return 1

    if [[ "$DEV_START_BG" -eq 1 ]]; then
        launch_logged "$FRONTEND_DIR" "$PROJECT_ROOT/frontend.log" pnpm run dev
        print_info "Frontend running in background (pid $LAUNCH_PID, logs: frontend.log)"
        return 0
    fi

    if [[ "$OSTYPE" == "darwin"* ]]; then
        osascript -e 'tell application "Terminal" to do script "cd '"$FRONTEND_DIR"' && echo \"* Myriad Frontend\" && pnpm run dev"' 2>/dev/null
    elif command -v gnome-terminal &> /dev/null; then
        gnome-terminal -- bash -c "cd '$FRONTEND_DIR' && echo '* Myriad Frontend' && pnpm run dev; exec bash" 2>/dev/null
    else
        launch_logged "$FRONTEND_DIR" "$PROJECT_ROOT/frontend.log" pnpm run dev
        print_info "Frontend running in background (pid $LAUNCH_PID, logs: frontend.log)"
        return 0
    fi

    if [[ "$DEV_START_NOWAIT" -eq 1 ]]; then
        return 0
    fi
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

    start_database || return 1
    echo ""

    print_step "Checking prerequisites…"
    check_tools || return 1

    if [[ "${MYRIAD_DEV_UPDATER:-}" == "1" ]]; then
        start_updater || return 1
        echo ""
    fi

    if [[ "$RUN_BACKEND" -eq 1 ]]; then
        if backend_port_in_use; then
            if backend_health_ok && get_service_status backend; then
                print_error "Backend is already running on :$BACKEND_PORT"
                print_info "Foreground start cannot attach to an existing process (Ctrl-C would not stop it)."
            else
                print_error "Port $BACKEND_PORT (backend) is already in use"
            fi
            print_info "Stop it first: $0 stop backend    Restart: $0 restart    Watch: $0 monitor"
            return 1
        fi
        parse_db_url || { print_error "Could not parse DATABASE_URL: $(resolve_db_url)"; return 1; }
        ensure_backend_env || return 1
    fi
    if [[ "$RUN_FRONTEND" -eq 1 ]] && frontend_port_in_use; then
        if get_service_status frontend; then
            print_error "Frontend is already running on :$FRONTEND_PORT"
            print_info "Foreground start cannot attach to an existing process (Ctrl-C would not stop it)."
        else
            print_error "Port $FRONTEND_PORT (frontend) is already in use"
        fi
        print_info "Stop it first: $0 stop frontend    Restart: $0 restart    Watch: $0 monitor"
        return 1
    fi

    if [[ "$RUN_FRONTEND" -eq 1 ]]; then
        ensure_frontend_deps || return 1
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
    if [[ "$DEV_START_NOWAIT" -eq 1 ]]; then
        USE_FG=0
        DETACH_EXPLICIT=1
        DEV_START_BG=1
    elif [[ "$USE_NATIVE" -eq 1 && "$USE_FG" -eq 0 && "$DETACH_EXPLICIT" -eq 0 ]]; then
        USE_FG=1
    fi

    if [[ "$USE_FG" -eq 1 ]]; then
        start_foreground_stack
        return
    fi

    echo ""
    echo -e "${BRIGHT_CYAN}${BOLD}${ICON_ROCKET} Starting All Services${NC}"
    echo ""

    # Database first and actually up — backend must not race a down postgres.
    start_database || return 1
    if ! get_service_status database; then
        print_error "Database is not reachable; backend was not started"
        return 1
    fi
    echo ""

    check_tools || return 1

    if [[ "${MYRIAD_DEV_UPDATER:-}" == "1" ]]; then
        start_updater || return 1
        if get_service_status backend; then
            print_info "Restarting backend so it picks up updater dev environment..."
            stop_backend
            sleep 1
        fi
        echo ""
    fi

    if [[ "$DEV_START_NOWAIT" -eq 0 ]]; then
        sleep 2
    fi
    if [[ "$RUN_BACKEND" -eq 1 ]]; then
        start_backend || return 1
        if [[ "$DEV_START_NOWAIT" -eq 1 ]] || backend_health_ok; then
            :
        elif [[ -n "$(list_backend_pids)" ]] || backend_port_in_use; then
            echo ""
            print_warning "Backend is still starting (often cargo compile). Frontend will start anyway."
            print_info "If /health stays down: $0 logs backend"
        else
            echo ""
            print_error "Backend process never appeared; frontend was not started."
            print_info "Fix the backend error first, then run: $0 start frontend"
            return 1
        fi
        echo ""
    fi
    if [[ "$RUN_FRONTEND" -eq 1 ]]; then
        start_frontend || return 1
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
    if docker_container_running "myriad-postgres-dev"; then
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

# ==================== Interactive (TUI) ====================

run_interactive() {
    run_tui || true
    return 0
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
    echo -e "  ${CYAN}status${NC}             Snapshot of services, PIDs, health, database"
    echo -e "  ${CYAN}monitor${NC} | ${CYAN}tui${NC} | ${CYAN}menu${NC}  Live TUI (start menu + processes / database / logs)"
    echo -e "  ${CYAN}logs${NC}    [service]  Tail backend.log / frontend.log / postgres"
    echo -e "  ${CYAN}doctor${NC}             Check toolchain, ports and database"
    echo -e "  ${CYAN}db-setup${NC}           Create local PostgreSQL role + database"
    echo -e "  ${MAGENTA}clean${NC}              Clean build files and database"
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
    echo -e "  ${DIM}--skip-install${NC}     Skip pnpm install even if node_modules is missing"
    echo -e "  ${DIM}--watch${NC}            With status: open the live TUI"
    echo ""
    echo -e "${BOLD}Environment:${NC}"
    echo -e "  ${DIM}DATABASE_URL${NC}         Override the dev database URL"
    echo -e "  ${DIM}MYRIAD_PSQL_ADMIN${NC}    Superuser psql command used by db-setup"
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo -e "  ${DIM}./dev.sh${NC}                      # Live TUI (start menu + monitor)"
    echo -e "  ${DIM}./dev.sh monitor${NC}              # Same as no-args TUI"
    echo -e "  ${DIM}./dev.sh start${NC}                # Local postgres, logs in this terminal"
    echo -e "  ${DIM}./dev.sh start --docker${NC}       # Docker postgres + new terminals"
    echo -e "  ${DIM}./dev.sh start --docker --fg${NC}  # Docker postgres, logs in this terminal"
    echo -e "  ${DIM}./dev.sh start all-updater${NC}    # Dev stack + updater harness (Docker)"
    echo -e "  ${DIM}./dev.sh start backend${NC}        # Start backend only"
    echo -e "  ${DIM}./dev.sh doctor${NC}               # Check prerequisites"
    echo -e "  ${DIM}./dev.sh db-setup${NC}             # Create local role + database"
    echo -e "  ${DIM}./dev.sh logs backend${NC}         # Tail backend.log"
    echo -e "  ${DIM}./dev.sh stop${NC}                 # Stop all services"
    echo -e "  ${DIM}./dev.sh status${NC}               # One-shot status"
    echo -e "  ${DIM}./dev.sh status --watch${NC}       # Live TUI"
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
            --watch) WATCH=1; shift ;;
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

    # Drag / Open With may pass this script's own path as $1.
    if [[ ${#positional[@]} -gt 0 ]]; then
        case "${positional[0]}" in
            "$0"|*/scripts/dev.sh|*/dev.sh)
                positional=("${positional[@]:1}")
                ;;
        esac
    fi

    local command="${positional[0]:-}"
    local service="${positional[1]:-all}"

    if [[ -z "$command" ]]; then
        run_interactive
        exit 0
    fi

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
        status)
            if [[ "$WATCH" -eq 1 ]]; then
                run_tui
            else
                show_mini_logo
                show_status_dashboard
            fi
            ;;
        doctor) run_doctor ;;
        db-setup) run_db_setup ;;
        logs) show_logs "$service" ;;
        clean) clean_project ;;
        menu|monitor|tui) run_tui ;;
        help) show_help ;;
        *) print_error "Unknown command: $command"; echo ""; show_help; exit 1 ;;
    esac
}

# Live TUI (sourced so it can call start_*/stop_* defined above).
# shellcheck source=tui.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/tui.sh"

main "$@"
