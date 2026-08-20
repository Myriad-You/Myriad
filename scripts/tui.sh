# ============================================
# Myriad Dev — live TUI
# ============================================
# Sourced by dev.sh. Do not execute directly.
# macOS /bin/bash 3.2: no assoc arrays, no `wait -n`.
#
# Overview is the start menu (mark + panel at ~90×26, compact below that).
# Start/stop stay on this screen and write backend.log / frontend.log.
# doctor / db-setup / psql / clean leave the alt screen (need a real tty).

TUI_TAB=0
TUI_SEL=0
TUI_SEL0=0
TUI_SEL1=0
TUI_SEL2=0
TUI_SEL3=0
TUI_SEL4=0
TUI_LOG=0
TUI_MSG=""
TUI_MSG_ERR=0
TUI_HELP=0
TUI_CONFIRM=""
TUI_CONFIRM_TEXT=""
TUI_STTY=""
TUI_ACTIVE=0
TUI_STATUS_DIRTY=1
TUI_CACHE_DB_STATE="stopped"
TUI_CACHE_DB_DETAIL="—"
TUI_CACHE_BE_STATE="stopped"
TUI_CACHE_BE_DETAIL="—"
TUI_CACHE_FE_STATE="stopped"
TUI_CACHE_FE_DETAIL="—"
TUI_CACHE_UP_STATE="stopped"
TUI_CACHE_UP_DETAIL="—"
TUI_EXPECT_DB_UNTIL=0
TUI_EXPECT_BE_UNTIL=0
TUI_EXPECT_FE_UNTIL=0
TUI_POLL_FAST=3
TUI_POLL_IDLE=10
TUI_POLL_TIME=3

TUI_TAB_COUNT=5
TUI_LOG_COUNT=4
TUI_MIN_COLS=72
TUI_MIN_ROWS=18
TUI_LOGO_FILE=""
TUI_BODY_FILE=""
TUI_LOGO_H=0
TUI_LOGO_W=0
TUI_IDLE=0
TUI_PREV_COLS=0
TUI_PREV_ROWS=0
TUI_RESIZE=0
TUI_LOG_FP=""
TUI_LOG_SKIP=0
TUI_TAB_NAMES="Overview|Processes|Database|Logs|Tools"

# kind|id|rss|etime|kids|cmd
TUI_PROCS=""
TUI_PROC_COUNT=0

# Overview = original start menu. Updater is Docker-only, so native omits it.
tui_menu_ids() {
    printf '%s\n' start-all stop-all restart-all database backend frontend
    [[ "$USE_NATIVE" -eq 0 ]] && printf '%s\n' updater
    printf '%s\n' clean
}

# Tools = things that need a tty or a browser.
TUI_TOOL_IDS="doctor
db-setup
psql
open-frontend
open-backend
clean"

# ---------- strings ----------

tui_strip() {
    printf '%s' "$1" | sed $'s/\x1b\\[[0-9;]*m//g'
}

tui_visible_len() {
    # Cell count. ASCII is ${#s}. Wide text: codepoints via UTF-32LE,
    # plus one extra cell per 4-byte UTF-8 scalar (emoji).
    local s n extra
    s="$(tui_strip "$1")"
    s="${s%$'\n'}"
    if [[ -z "$(printf '%s' "$s" | LC_ALL=C tr -d '\000-\177')" ]]; then
        printf '%s\n' "${#s}"
        return 0
    fi
    n="$(printf '%s' "$s" | iconv -f UTF-8 -t UTF-32LE 2>/dev/null | wc -c | tr -d '[:space:]')"
    if [[ -n "$n" && "$n" -gt 0 ]]; then
        extra="$(printf '%s' "$s" | LC_ALL=C tr -cd '\360-\364' | wc -c | tr -d '[:space:]')"
        echo $((n / 4 + extra))
    else
        printf '%s' "$s" | wc -c | tr -d '[:space:]'
    fi
}

tui_trunc() {
    local text="$1" max="$2" clean len take
    clean="$(tui_strip "$text")"
    clean="${clean%$'\n'}"
    len="$(tui_visible_len "$clean")"
    if [[ "$len" -le $max ]]; then
        printf '%s' "$text"
        return
    fi
    take=$((max - 1))
    [[ $take -lt 1 ]] && take=1
    printf '%s' "${clean:0:$take}…"
}

tui_pad() {
    local text="$1" width="$2" len pad
    text="$(tui_trunc "$text" "$width")"
    len="$(tui_visible_len "$text")"
    pad=$((width - len))
    printf '%s' "$text"
    [[ $pad -gt 0 ]] && printf '%*s' "$pad" ""
}

tui_hline() {
    repeat_glyph "${1:-$TUI_COLS}" "─"
}

tui_count() {
    printf '%s\n' "$1" | awk 'NF { n++ } END { print n+0 }'
}

tui_nth() {
    printf '%s\n' "$1" | awk -v n="$2" 'NR==n { print; exit }'
}

tui_mode_label() {
    if [[ "$USE_NATIVE" -eq 1 ]]; then
        echo "native"
    else
        echo "docker-pg"
    fi
}

tui_fmt_rss() {
    local kb="${1:-0}"
    kb="${kb%.*}"
    [[ "$kb" =~ ^[0-9]+$ ]] || { echo "?"; return; }
    if [[ $kb -ge 1048576 ]]; then
        awk -v k="$kb" 'BEGIN { printf "%.1fG", k/1048576 }'
    elif [[ $kb -ge 1024 ]]; then
        awk -v k="$kb" 'BEGIN { printf "%.0fM", k/1024 }'
    else
        echo "${kb}K"
    fi
}

tui_dot() {
    case "$1" in
        running|healthy|up) echo -e "${GREEN}*${NC}" ;;
        starting|degraded)  echo -e "${YELLOW}+${NC}" ;;
        *)                  echo -e "${DIM}.${NC}" ;;
    esac
}

tui_menu_label() {
    case "$1" in
        start-all)   echo "${ICON_ROCKET} Start all services" ;;
        stop-all)    echo "${ICON_STOP} Stop all services" ;;
        restart-all) echo "${ICON_REFRESH} Restart all services" ;;
        database)    echo "${ICON_DB} Database only" ;;
        backend)     echo "${ICON_RUST} Backend only" ;;
        frontend)    echo "${ICON_NODE} Frontend only" ;;
        updater)     echo "${ICON_UPDATER} Updater harness" ;;
        clean)       echo "${ICON_TRASH} Clean project" ;;
        *)           echo "$1" ;;
    esac
}

tui_tool_label() {
    case "$1" in
        doctor)        echo "Run doctor" ;;
        db-setup)      echo "Create role + database" ;;
        psql)          echo "Open psql" ;;
        open-frontend) echo "Open frontend" ;;
        open-backend)  echo "Open /health" ;;
        clean)         echo "Clean project" ;;
        *)             echo "$1" ;;
    esac
}

tui_tool_hint() {
    case "$1" in
        doctor)        echo "toolchain, ports, database" ;;
        db-setup)      echo "create local role + database" ;;
        psql)          echo "interactive SQL" ;;
        open-frontend) echo "http://localhost:${FRONTEND_PORT}" ;;
        open-backend)  echo "http://localhost:${BACKEND_PORT}/health" ;;
        clean)         echo "drop tables + build artifacts" ;;
        *)             echo "" ;;
    esac
}

tui_svc_meta() {
    case "$1" in
        database) TUI_M_LABEL="Postgres";  TUI_M_ICON="$ICON_DB";      TUI_M_PORT=":${DB_PORT:-5432}" ;;
        backend)  TUI_M_LABEL="Backend";   TUI_M_ICON="$ICON_RUST";    TUI_M_PORT=":${BACKEND_PORT}" ;;
        frontend) TUI_M_LABEL="Frontend";  TUI_M_ICON="$ICON_NODE";    TUI_M_PORT=":${FRONTEND_PORT}" ;;
        updater)  TUI_M_LABEL="Updater";   TUI_M_ICON="$ICON_UPDATER"; TUI_M_PORT=":1101" ;;
    esac
}

# ---------- terminal ----------

tui_attach_tty() {
    # Drag / Open With often start with stdin = /dev/null.
    if [[ ! -t 0 && -r /dev/tty ]]; then
        exec </dev/tty
    fi
    if [[ ! -t 1 && -w /dev/tty ]]; then
        exec >/dev/tty
    fi
    if [[ ! -t 2 && -w /dev/tty ]]; then
        exec 2>/dev/tty
    fi
}

tui_enter() {
    TUI_STTY="$(stty -g 2>/dev/null || true)"
    tput smcup 2>/dev/null || printf '\033[?1049h'
    printf '\033[?25l'
    # Normal cursor keys (CSI), not application mode SS3 after smcup.
    tput rmkx 2>/dev/null || printf '\033[?1l'
    # VMIN=0 VTIME=3: kernel waits up to 0.3s. No bash `read -t` (3.2 is instant).
    stty -echo -icanon time "$TUI_POLL_TIME" min 0 2>/dev/null || stty -echo -icanon 2>/dev/null || true
    TUI_ACTIVE=1
}

tui_leave() {
    [[ "$TUI_ACTIVE" -eq 1 ]] || return 0
    TUI_ACTIVE=0
    [[ -n "$TUI_STTY" ]] && stty "$TUI_STTY" 2>/dev/null || stty sane 2>/dev/null || true
    printf '\033[?25h'
    tput rmcup 2>/dev/null || printf '\033[?1049l'
    if [[ -n "$TUI_LOGO_FILE" && -f "$TUI_LOGO_FILE" ]]; then
        rm -f "$TUI_LOGO_FILE"
        TUI_LOGO_FILE=""
    fi
    if [[ -n "$TUI_BODY_FILE" && -f "$TUI_BODY_FILE" ]]; then
        rm -f "$TUI_BODY_FILE"
        TUI_BODY_FILE=""
    fi
}

tui_size() {
    TUI_COLS="$(tput cols 2>/dev/null || echo 80)"
    TUI_ROWS="$(tput lines 2>/dev/null || echo 24)"
    [[ "$TUI_COLS" -lt 20 ]] && TUI_COLS=20
    [[ "$TUI_ROWS" -lt 10 ]] && TUI_ROWS=10
}

tui_read_byte() {
    # -d '' keeps newline as data. rc=0 + empty still means Enter
    # (bash 3.2 delimiter). rc!=0 + empty is the 1s VTIME idle.
    TUI_BYTE=""
    TUI_READ_RC=0
    IFS= read -r -s -n 1 -d '' TUI_BYTE
    TUI_READ_RC=$?
}

tui_read_key() {
    TUI_KEY=""
    local k="" rest=""
    tui_read_byte
    if [[ -z "$TUI_BYTE" ]]; then
        if [[ "$TUI_READ_RC" -eq 0 ]]; then
            TUI_KEY="enter"
        else
            TUI_KEY="timeout"
        fi
        return 0
    fi
    k="$TUI_BYTE"
    if [[ "$k" == $'\033' ]]; then
        stty time 1 min 0 2>/dev/null || true
        tui_read_byte
        if [[ -z "$TUI_BYTE" ]]; then
            stty time "$TUI_POLL_TIME" min 0 2>/dev/null || true
            TUI_KEY="esc"
            return 0
        fi
        rest="$TUI_BYTE"
        k="${k}${rest}"
        if [[ "$rest" == "O" ]]; then
            tui_read_byte
            k="${k}${TUI_BYTE}"
        elif [[ "$rest" == "[" ]]; then
            tui_read_byte
            k="${k}${TUI_BYTE}"
            # CSI params: ESC [ 1 ; 5 A  or  ESC [ 3 ~
            if [[ "$TUI_BYTE" == [0-9\;] ]]; then
                while true; do
                    tui_read_byte
                    [[ -n "$TUI_BYTE" ]] || break
                    k="${k}${TUI_BYTE}"
                    case "$TUI_BYTE" in
                        [A-Za-z~]) break ;;
                    esac
                done
            fi
        fi
        stty time "$TUI_POLL_TIME" min 0 2>/dev/null || true
    fi
    case "$k" in
        $'\033[A'|$'\033OA'|$'\033[1A') TUI_KEY="up" ;;
        $'\033[B'|$'\033OB'|$'\033[1B') TUI_KEY="down" ;;
        $'\033[C'|$'\033OC'|$'\033[1C') TUI_KEY="right" ;;
        $'\033[D'|$'\033OD'|$'\033[1D') TUI_KEY="left" ;;
        $'\033[H'|$'\033OH'|$'\033[1~'|$'\033[7~') TUI_KEY="g" ;;
        $'\033[F'|$'\033OF'|$'\033[4~'|$'\033[8~') TUI_KEY="G" ;;
        $'\033[5~') TUI_KEY="pgup" ;;
        $'\033[6~') TUI_KEY="pgdn" ;;
        $'\n'|$'\r'|$'\033OM') TUI_KEY="enter" ;;
        $'\t') TUI_KEY="tab" ;;
        $'\x7f'|$'\b') TUI_KEY="bs" ;;
        *) TUI_KEY="$k" ;;
    esac
}

tui_pause() {
    printf '%s' "Press Enter to close..."
    if [[ -t 0 ]]; then
        read -r _ || true
    elif [[ -r /dev/tty ]]; then
        read -r _ </dev/tty || true
    fi
}

# ---------- snapshots ----------

# kind|id|rss|etime|kids|cmd  — roots only; children stay collapsed
tui_append_proc() {
    local kind="$1" id="$2" rss="$3" etime="$4" kids="$5" cmd="$6"
    TUI_PROCS="${TUI_PROCS}${kind}|${id}|${rss}|${etime}|${kids}|${cmd}"$'\n'
    TUI_PROC_COUNT=$((TUI_PROC_COUNT + 1))
}

tui_proc_id() {
    local id="$1"
    case "$id" in
        myriad-*-dev)
            id="${id#myriad-}"
            printf '%s' "${id%-dev}"
            ;;
        *) printf '%s' "$id" ;;
    esac
}

tui_short_cmd() {
    local cmd="$1"
    cmd="$(printf '%s' "$cmd" | tr -s '[:space:]' ' ')"
    case "$cmd" in
        *cargo\ run*)       echo "cargo run" ;;
        *myriad-backend*)   echo "myriad-backend" ;;
        *pnpm\ run\ dev*)   echo "pnpm run dev" ;;
        *astro\ dev*|*astro*) echo "astro" ;;
        *postgres*)         echo "postgres" ;;
        *)
            set -- $cmd
            basename "${1:-?}"
            ;;
    esac
}

tui_pid_listed() {
    local needle="$1" p
    while IFS= read -r p; do
        [[ "$p" == "$needle" ]] && return 0
    done <<< "$2"
    return 1
}

tui_add_pid_tree() {
    local kind="$1"
    shift
    local seeds pid line ppid rss etime cmd kids kid n
    seeds="$(printf '%s\n' "$@" | normalize_pids)"
    [[ -z "$seeds" ]] && return 0
    while IFS= read -r pid; do
        [[ -n "$pid" ]] || continue
        line="$(proc_ps "$pid" || true)"
        [[ -n "$line" ]] || continue
        read -r _ ppid _ _ rss etime cmd <<< "$line"
        if tui_pid_listed "$ppid" "$seeds"; then
            continue
        fi
        n=0
        kids="$(expand_process_tree "$pid")"
        while IFS= read -r kid; do
            [[ -n "$kid" && "$kid" != "$pid" ]] && n=$((n + 1))
        done <<< "$kids"
        tui_append_proc "$kind" "$pid" "$(tui_fmt_rss "$rss")" "${etime:--}" "$n" "$(tui_short_cmd "$cmd")"
    done <<< "$seeds"
}

tui_add_container() {
    local name="$1" kind="$2" status
    docker_usable || return 0
    status="$(docker ps --filter "name=^${name}$" --format '{{.Status}}' 2>/dev/null | head -n 1)"
    [[ -n "$status" ]] || return 0
    tui_append_proc "$kind" "$name" "-" "-" "0" "$status"
}

tui_refresh_procs() {
    TUI_PROCS=""
    TUI_PROC_COUNT=0

    local be fe pg
    be="$(list_backend_pids || true)"
    fe="$(list_frontend_pids || true)"
    [[ -n "$be" ]] && tui_add_pid_tree backend $be
    [[ -n "$fe" ]] && tui_add_pid_tree frontend $fe

    if db_is_docker; then
        tui_add_container "myriad-postgres-dev" postgres
    else
        pg="$(list_listen_pids "${DB_PORT:-5432}" || true)"
        [[ -n "$pg" ]] && tui_add_pid_tree postgres $pg
    fi

    if [[ "$USE_NATIVE" -eq 0 ]]; then
        tui_add_container "myriad-updater-dev" updater
        tui_add_container "myriad-updater-gateway-dev" gateway
        tui_add_container "myriad-docker-guard-dev" guard
    fi

    if [[ $TUI_SEL -ge $TUI_PROC_COUNT && $TUI_PROC_COUNT -gt 0 ]]; then
        TUI_SEL=$((TUI_PROC_COUNT - 1))
    fi
}

tui_refresh_status() {
    local now line dbp="${DB_PORT:-5432}" have_db=0 have_be=0 have_fe=0 have_up=0
    now="$(date +%s)"
    parse_db_url || true
    dbp="${DB_PORT:-5432}"

    if command -v lsof >/dev/null 2>&1; then
        while IFS= read -r line; do
            case "$line" in
                *":${dbp}"*) have_db=1 ;;
            esac
            case "$line" in
                *":${BACKEND_PORT}"*) have_be=1 ;;
            esac
            case "$line" in
                *":${FRONTEND_PORT}"*) have_fe=1 ;;
            esac
            case "$line" in
                *:1101*) have_up=1 ;;
            esac
        done < <(lsof -nP -iTCP:"$dbp" -iTCP:"$BACKEND_PORT" -iTCP:"$FRONTEND_PORT" -iTCP:1101 -sTCP:LISTEN 2>/dev/null)
    fi

    if [[ $have_db -eq 1 ]]; then
        TUI_CACHE_DB_STATE="running"
        TUI_CACHE_DB_DETAIL=":${dbp}"
        TUI_EXPECT_DB_UNTIL=0
    elif [[ "$now" -lt "${TUI_EXPECT_DB_UNTIL:-0}" ]]; then
        TUI_CACHE_DB_STATE="starting"
        TUI_CACHE_DB_DETAIL="postgres"
    else
        TUI_CACHE_DB_STATE="stopped"
        TUI_CACHE_DB_DETAIL="—"
    fi

    if [[ $have_be -eq 1 ]]; then
        TUI_CACHE_BE_STATE="running"
        TUI_CACHE_BE_DETAIL=":${BACKEND_PORT}"
        TUI_EXPECT_BE_UNTIL=0
    elif [[ "$now" -lt "${TUI_EXPECT_BE_UNTIL:-0}" ]]; then
        TUI_CACHE_BE_STATE="starting"
        TUI_CACHE_BE_DETAIL="cargo"
    else
        TUI_CACHE_BE_STATE="stopped"
        TUI_CACHE_BE_DETAIL="—"
    fi

    if [[ $have_fe -eq 1 ]]; then
        TUI_CACHE_FE_STATE="running"
        TUI_CACHE_FE_DETAIL=":${FRONTEND_PORT}"
        TUI_EXPECT_FE_UNTIL=0
    elif [[ "$now" -lt "${TUI_EXPECT_FE_UNTIL:-0}" ]]; then
        TUI_CACHE_FE_STATE="starting"
        TUI_CACHE_FE_DETAIL="pnpm"
    else
        TUI_CACHE_FE_STATE="stopped"
        TUI_CACHE_FE_DETAIL="—"
    fi

    if [[ "$USE_NATIVE" -eq 1 ]]; then
        TUI_CACHE_UP_STATE="n/a"
        TUI_CACHE_UP_DETAIL="docker only"
    elif [[ $have_up -eq 1 ]]; then
        TUI_CACHE_UP_STATE="running"
        TUI_CACHE_UP_DETAIL=":1101"
    else
        TUI_CACHE_UP_STATE="stopped"
        TUI_CACHE_UP_DETAIL="—"
    fi
    TUI_STATUS_DIRTY=0
}

tui_svc_state() {
    case "$1" in
        database) TUI_S_STATE="$TUI_CACHE_DB_STATE"; TUI_S_DETAIL="$TUI_CACHE_DB_DETAIL" ;;
        backend)  TUI_S_STATE="$TUI_CACHE_BE_STATE"; TUI_S_DETAIL="$TUI_CACHE_BE_DETAIL" ;;
        frontend) TUI_S_STATE="$TUI_CACHE_FE_STATE"; TUI_S_DETAIL="$TUI_CACHE_FE_DETAIL" ;;
        updater)  TUI_S_STATE="$TUI_CACHE_UP_STATE"; TUI_S_DETAIL="$TUI_CACHE_UP_DETAIL" ;;
    esac
}

# ---------- render ----------
tui_draw_list() {
    local ids="$1" labelfn="$2" split_after="${3:-}" rule_w="${4:-}"
    local i=0 id count
    [[ -n "$rule_w" ]] || rule_w=$((TUI_COLS - 4))
    count="$(tui_count "$ids")"
    while [[ $i -lt $count ]]; do
        id="$(tui_nth "$ids" $((i + 1)))"
        if [[ -n "$split_after" && $i -eq $split_after ]]; then
            printf '  %b%s%b\n' "$DIM" "$(tui_hline "$rule_w")" "$NC"
        fi
        if [[ $i -eq $TUI_SEL ]]; then
            printf '  %b> %s%b\n' "$BRIGHT_CYAN" "$($labelfn "$id")" "$NC"
        else
            printf '    %s\n' "$($labelfn "$id")"
        fi
        i=$((i + 1))
    done
}

tui_tick_clock() {
    # Rewrite only the HH:MM:SS so idle seconds move without a full redraw.
    [[ "$TUI_ACTIVE" -eq 1 ]] || return 0
    [[ "$TUI_COLS" -ge $TUI_MIN_COLS && "$TUI_ROWS" -ge $TUI_MIN_ROWS ]] || return 0
    local clock col
    clock="$(date +%H:%M:%S)"
    col=$((TUI_COLS - ${#clock} - 1))
    [[ $col -ge 1 ]] || return 0
    printf '\033[1;%sH%b%s%b' "$col" "$DIM" "$clock" "$NC"
}

tui_draw_header() {
    local clock mode title i name
    local -a tabs
    clock="$(date +%H:%M:%S)"
    mode="$(tui_mode_label)"
    title=" Myriad Dev  v${VERSION} "
    printf '%b%s%b' "${BRIGHT_CYAN}${BOLD}" "$(tui_pad "$title" $((TUI_COLS - ${#clock} - ${#mode} - 4)))" "$NC"
    printf '%b %s  %s %b\n' "${DIM}" "$mode" "$clock" "$NC"

    IFS='|' read -r -a tabs <<< "$TUI_TAB_NAMES"
    printf ' '
    i=0
    for name in "${tabs[@]}"; do
        if [[ $i -eq $TUI_TAB ]]; then
            printf '%b[%d %s]%b  ' "${BOLD}${CYAN}" "$((i + 1))" "$name" "$NC"
        else
            printf '%b%d %s%b  ' "${DIM}" "$((i + 1))" "$name" "$NC"
        fi
        i=$((i + 1))
    done
    echo ""
    printf '%b%s%b\n' "$DIM" "$(tui_hline)" "$NC"
}

tui_set_msg() {
    TUI_MSG="$1"
    TUI_MSG_ERR="${2:-0}"
}

tui_draw_footer() {
    local hint color keys
    keys="1-5 tabs  ?/h help  q quit"
    if [[ "$TUI_HELP" -eq 1 ]]; then
        hint="Esc / ? / h / q  close help"
        keys=""
    elif [[ -n "$TUI_CONFIRM" ]]; then
        hint="y yes    n / Esc cancel"
        keys=""
    else
        case "$TUI_TAB" in
            0) hint="up/dn menu  Enter run  s/x/r  a start-all  K stop-all" ;;
            1) hint="up/dn select  x stop  X kill  r refresh" ;;
            2) hint="s start  x stop  p psql  Tools -> db-setup" ;;
            3) hint="left/right source  up/dn/PgUp scroll  G tail" ;;
            4) hint="up/dn select  Enter run" ;;
        esac
    fi
    printf '%b%s%b\n' "$DIM" "$(tui_hline)" "$NC"
    if [[ -n "$TUI_MSG" && "$TUI_HELP" -eq 0 && -z "$TUI_CONFIRM" ]]; then
        if [[ "$TUI_MSG_ERR" -eq 1 ]]; then
            color="$RED"
            keys="Esc dismiss   1-5 tabs  ?/h help  q quit"
        else
            color="$YELLOW"
        fi
        printf ' %b%s%b\n' "$color" "$(tui_trunc "$TUI_MSG" $((TUI_COLS - 2)))" "$NC"
    else
        printf ' %s\n' "$(tui_trunc "$hint" $((TUI_COLS - 2)))"
    fi
    if [[ -n "$keys" ]]; then
        printf ' %b%s%b\n' "$DIM" "$keys" "$NC"
    else
        printf '\n'
    fi
}

tui_logo_init() {
    local src="$PROJECT_ROOT/shared/logo-ansi.txt"
    [[ -f "$src" ]] || { TUI_LOGO_H=0; return 1; }
    [[ -n "$TUI_LOGO_FILE" && -f "$TUI_LOGO_FILE" ]] && return 0

    local line vis pad i
    local -a lines=() widths=()
    TUI_LOGO_W=0
    TUI_LOGO_H=0
    while IFS= read -r line || [[ -n "$line" ]]; do
        lines+=("$line")
        vis="$(tui_visible_len "$line")"
        widths+=("$vis")
        [[ "$vis" -gt "$TUI_LOGO_W" ]] && TUI_LOGO_W="$vis"
        TUI_LOGO_H=$((TUI_LOGO_H + 1))
    done < "$src"

    TUI_LOGO_FILE="$(mktemp "${TMPDIR:-/tmp}/myriad-logo.XXXXXX")" || return 1
    : > "$TUI_LOGO_FILE"
    i=0
    for line in "${lines[@]}"; do
        vis="${widths[$i]}"
        i=$((i + 1))
        pad=$((TUI_LOGO_W - vis))
        [[ $pad -lt 0 ]] && pad=0
        # Keep the file's own cells; no forced black canvas behind the mark.
        printf '%s%*s\033[0m\n' "$line" "$pad" "" >> "$TUI_LOGO_FILE"
    done
}

# Gap between mark and panel. Panel shrinks so 80×24 still fits left/right.
tui_overview_panel_w() {
    local w=$((TUI_COLS - TUI_LOGO_W - 2))
    [[ $w -gt 42 ]] && w=42
    [[ $w -lt 28 ]] && w=28
    echo "$w"
}

tui_overview_fits_mark() {
    [[ "$TUI_LOGO_H" -gt 0 ]] || return 1
    [[ "$TUI_COLS" -ge $((TUI_LOGO_W + 30)) ]] || return 1
    [[ "$(tui_body_height)" -ge 12 ]]
}

tui_status_names() {
    printf '%s\n' database backend frontend
    [[ "$USE_NATIVE" -eq 0 ]] && printf '%s\n' updater
}

tui_draw_overview_panel() {
    local name pw label_w
    pw="${1:-$(tui_overview_panel_w)}"
    label_w=$((pw - 16))
    [[ $label_w -gt 14 ]] && label_w=14
    [[ $label_w -lt 10 ]] && label_w=10
    printf '  %bStatus%b\n' "$BOLD" "$NC"
    printf '  %b%s%b\n' "$DIM" "$(tui_hline "$pw")" "$NC"
    while IFS= read -r name; do
        [[ -n "$name" ]] || continue
        tui_svc_meta "$name"
        tui_svc_state "$name"
        printf '  %s %s %s\n' \
            "$(tui_dot "$TUI_S_STATE")" \
            "$(tui_pad "$TUI_M_ICON $TUI_M_LABEL" "$label_w")" \
            "$(tui_pad "$TUI_S_STATE" 10)"
    done <<< "$(tui_status_names)"
    printf '  %b%s%b\n' "$DIM" "$(tui_trunc "http://localhost:${FRONTEND_PORT}" "$pw")" "$NC"
    printf '  %b%s%b\n' "$DIM" "$(tui_trunc "http://localhost:${BACKEND_PORT}/health" "$pw")" "$NC"
    printf '  %bMenu%b\n' "$BOLD" "$NC"
    printf '  %b%s%b\n' "$DIM" "$(tui_hline "$pw")" "$NC"
    tui_draw_list "$(tui_menu_ids)" tui_menu_label 3 "$pw"
}

tui_body_height() {
    local n=$((TUI_ROWS - 6))
    [[ $n -lt 4 ]] && n=4
    echo "$n"
}

tui_ensure_scratch() {
    [[ -n "$TUI_BODY_FILE" && -f "$TUI_BODY_FILE" ]] && return 0
    TUI_BODY_FILE="$(mktemp "${TMPDIR:-/tmp}/myriad-body.XXXXXX")" || return 1
}

# Draw in this shell (not a pipe) so caches / logo files stick, then clip.
tui_paint_body() {
    tui_ensure_scratch || { "$@"; return; }
    "$@" > "$TUI_BODY_FILE"
    awk -v n="$(tui_body_height)" 'NR <= n { print }' "$TUI_BODY_FILE"
}

tui_draw_overview_compact() {
    local name
    printf '  '
    while IFS= read -r name; do
        [[ -n "$name" ]] || continue
        tui_svc_meta "$name"
        tui_svc_state "$name"
        printf '%s %s %s  ' "$(tui_dot "$TUI_S_STATE")" "$TUI_M_ICON" "$TUI_M_LABEL"
    done <<< "$(tui_status_names)"
    printf '\n'
    printf '  %bMenu%b\n' "$BOLD" "$NC"
    printf '  %b%s%b\n' "$DIM" "$(tui_hline 42)" "$NC"
    tui_draw_list "$(tui_menu_ids)" tui_menu_label 3 42
}

tui_draw_overview() {
    parse_db_url || true
    tui_refresh_status
    tui_logo_init || true

    if tui_overview_fits_mark; then
        awk 'NR==FNR { a[FNR]=$0; n=FNR; next }
             {
                 if (FNR <= n) printf "%s  %s\n", a[FNR], $0
                 else printf "%s  %s\n", pad, $0
                 m=FNR
             }
             END {
                 if (m < n) for (i=m+1; i<=n; i++) print a[i]
             }' pad="$(printf '%*s' "$TUI_LOGO_W" "")" \
            "$TUI_LOGO_FILE" <(tui_draw_overview_panel "$(tui_overview_panel_w)")
        return 0
    fi

    if [[ "$(tui_body_height)" -ge 21 ]]; then
        printf '  %bWiden to %sx%s for the mark beside the menu.%b\n\n' \
            "$DIM" "$((TUI_LOGO_W + 30))" "18" "$NC"
        tui_draw_overview_panel 42
    else
        tui_draw_overview_compact
    fi
}

tui_draw_processes() {
    if [[ $TUI_PROC_COUNT -eq 0 ]]; then
        printf '\n  %bNo Myriad processes.%b  Overview -> Start all.\n' "$DIM" "$NC"
        return
    fi
    printf '  %s\n' "$(tui_pad "SVC" 9)$(tui_pad "ID" 16)$(tui_pad "RSS" 8)$(tui_pad "TIME" 10)CMD"
    printf '  %b%s%b\n' "$DIM" "$(tui_hline $((TUI_COLS - 4)))" "$NC"

    local i=0 line kind id rss etime kids cmd shown row
    local body=$(( $(tui_body_height) - 4 ))
    [[ $body -lt 1 ]] && body=1
    local start=0
    [[ $TUI_SEL -ge $body ]] && start=$((TUI_SEL - body + 1))
    local end=$((start + body))

    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        if [[ $i -lt $start ]]; then
            i=$((i + 1))
            continue
        fi
        [[ $i -ge $end ]] && break
        IFS='|' read -r kind id rss etime kids cmd <<< "$line"
        shown="$cmd"
        [[ "$kids" =~ ^[1-9] ]] && shown="$cmd  +$kids"
        row="$(tui_pad "$kind" 9)$(tui_pad "$(tui_proc_id "$id")" 16)$(tui_pad "$rss" 8)$(tui_pad "$etime" 10)$(tui_trunc "$shown" $((TUI_COLS - 47)))"
        if [[ $i -eq $TUI_SEL ]]; then
            printf ' %b>%b %s\n' "$BRIGHT_CYAN" "$NC" "$row"
        else
            printf '   %s\n' "$row"
        fi
        i=$((i + 1))
    done <<< "$TUI_PROCS"
    printf '\n  %b%d root(s)  x stop   X kill   postgres -> Database stop%b\n' "$DIM" "$TUI_PROC_COUNT" "$NC"
}

tui_draw_database() {
    parse_db_url || { printf '\n  %bCould not parse DATABASE_URL%b\n' "$RED" "$NC"; return; }
    local reachable="no" src="native"
    db_reachable && reachable="yes"
    db_is_docker && src="docker"
    TUI_DB_REACH="$reachable"

    printf '\n  %s@%s:%s/%s   %s   %s\n' "$DB_USER" "$DB_HOST" "$DB_PORT" "$DB_NAME" "$src" "$reachable"

    if [[ "$reachable" != "yes" ]]; then
        printf '\n  %bNot reachable.%b  Press s to start, x to stop, or Tools -> Create role + database.\n' "$YELLOW" "$NC"
        return
    fi
    if ! have psql; then
        printf '\n  %bpsql not found%b — install the PostgreSQL client tools.\n' "$YELLOW" "$NC"
        return
    fi

    local ver size conns maxc tables mig avail nt nm na
    ver="$(db_scalar "SELECT current_setting('server_version')" || echo '?')"
    size="$(db_scalar "SELECT pg_size_pretty(pg_database_size(current_database()))" || echo '?')"
    conns="$(db_scalar "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database()" || echo '?')"
    maxc="$(db_scalar "SELECT current_setting('max_connections')" || echo '?')"
    tables="$(db_scalar "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_type = 'BASE TABLE'" || echo '?')"
    mig="$(db_scalar "SELECT count(*) FROM seaql_migrations" || echo '—')"

    printf '  postgres %s   %s   %s/%s sess   %s tables   %s mig\n' \
        "$ver" "$size" "$conns" "$maxc" "$tables" "$mig"

    avail=$(( $(tui_body_height) - 6 ))
    [[ $avail -lt 3 ]] && avail=3
    nt=$(( (avail * 2) / 5 ))
    nm=$(( avail / 5 ))
    na=$((avail - nt - nm))
    [[ $nt -lt 1 ]] && nt=1
    [[ $nm -lt 1 ]] && nm=1
    [[ $na -lt 1 ]] && na=1

    echo ""
    printf '  %bLargest%b\n' "$BOLD" "$NC"
    db_psql -P pager=off -F $'\t' -A -c \
        "SELECT relname, pg_size_pretty(pg_total_relation_size(relid))
         FROM pg_catalog.pg_statio_user_tables
         ORDER BY pg_total_relation_size(relid) DESC
         LIMIT $nt" 2>/dev/null | awk 'NR==1{next} NF{printf "    %-32s %s\n", $1, $2}'

    echo ""
    printf '  %bMigrations%b\n' "$BOLD" "$NC"
    db_psql -P pager=off -tAc \
        "SELECT version FROM seaql_migrations ORDER BY version DESC LIMIT $nm" 2>/dev/null \
        | sed 's/^/    /' || printf '    %bno seaql_migrations (backend has not applied schema)%b\n' "$DIM" "$NC"

    echo ""
    printf '  %bActivity%b\n' "$BOLD" "$NC"
    db_psql -P pager=off -F $'\t' -A -c \
        "SELECT pid, COALESCE(state,''), left(regexp_replace(COALESCE(query,''), E'[\\n\\r]+', ' ', 'g'), 40)
         FROM pg_stat_activity
         WHERE datname = current_database() AND pid <> pg_backend_pid()
         ORDER BY backend_start
         LIMIT $na" 2>/dev/null | awk -F '\t' 'NR==1{next} NF{printf "    %-7s %-12s %s\n", $1, $2, $3}'
}

tui_file_fp() {
    local f="$1"
    [[ -f "$f" ]] || { echo "missing"; return; }
    if stat -f %m:%z "$f" >/dev/null 2>&1; then
        stat -f %m:%z "$f"
    else
        stat -c %Y:%s "$f" 2>/dev/null || echo "missing"
    fi
}

tui_log_fp() {
    case "$TUI_LOG" in
        0) tui_file_fp "$PROJECT_ROOT/backend.log" ;;
        1) tui_file_fp "$PROJECT_ROOT/frontend.log" ;;
        2)
            if docker_container_running "myriad-postgres-dev"; then
                docker logs --tail 1 --timestamps myriad-postgres-dev 2>&1 | tail -n 1
            else
                tui_file_fp "$(native_pg_log_path 2>/dev/null || true)"
            fi
            ;;
        3)
            if docker_container_running "myriad-updater-dev"; then
                docker logs --tail 1 --timestamps myriad-updater-dev 2>&1 | tail -n 1
            else
                echo "updater-down"
            fi
            ;;
    esac
}

tui_emit_log_lines() {
    local width=$((TUI_COLS - 4))
    [[ $width -lt 20 ]] && width=20
    while IFS= read -r line || [[ -n "$line" ]]; do
        line="${line//$'\r'/ }"
        line="${line//$'\t'/ }"
        # Drop leftover C0 (keep ESC) and cursor/erase CSI; keep SGR colors.
        line="$(printf '%s' "$line" | tr -d '\000-\010\013\014\016-\032\034-\037' | sed -E $'s/\x1b\\[[0-9;?]*[A-ln-z]//g')"
        printf '  %s\033[0m\n' "$(tui_trunc "$line" "$width")"
    done
}

tui_log_body() {
    local n=$(( $(tui_body_height) - 2 ))
    [[ $n -lt 1 ]] && n=1
    echo "$n"
}

# Keep skip inside the file (or a hard cap for docker streams) before the header prints it.
tui_clamp_log_skip() {
    [[ "$TUI_LOG_SKIP" -lt 0 ]] && TUI_LOG_SKIP=0
    [[ "$TUI_LOG_SKIP" -eq 0 ]] && return 0
    local body total maxskip file=""
    body="$(tui_log_body)"
    case "$TUI_LOG" in
        0) file="$PROJECT_ROOT/backend.log" ;;
        1) file="$PROJECT_ROOT/frontend.log" ;;
        2)
            if docker_container_running "myriad-postgres-dev"; then
                [[ "$TUI_LOG_SKIP" -gt 2000 ]] && TUI_LOG_SKIP=2000
                return 0
            fi
            file="$(native_pg_log_path 2>/dev/null || true)"
            ;;
        3)
            [[ "$TUI_LOG_SKIP" -gt 2000 ]] && TUI_LOG_SKIP=2000
            return 0
            ;;
    esac
    if [[ -n "$file" && -f "$file" ]]; then
        total="$(wc -l < "$file" | tr -d '[:space:]')"
        [[ "$total" =~ ^[0-9]+$ ]] || total=0
        maxskip=$(( total > body ? total - body : 0 ))
        [[ "$TUI_LOG_SKIP" -gt $maxskip ]] && TUI_LOG_SKIP=$maxskip
    else
        TUI_LOG_SKIP=0
    fi
}

# Window over a log file. skip=0 is a live tail (no wc); skip>0 is older lines.
tui_emit_file_window() {
    local file="$1" body="$2"
    [[ -f "$file" ]] || return 1
    if [[ "$TUI_LOG_SKIP" -le 0 ]]; then
        tail -n "$body" "$file"
    else
        tail -n $((body + TUI_LOG_SKIP)) "$file" | head -n "$body"
    fi | sanitize_dev_log | tui_emit_log_lines
}

tui_emit_stream_window() {
    local body="$1"
    # Caller already fetched tail -(body+skip) lines.
    if [[ "$TUI_LOG_SKIP" -le 0 ]]; then
        cat
    else
        head -n "$body"
    fi | sanitize_dev_log | tui_emit_log_lines
}

tui_set_log() {
    local next="$1"
    [[ "$next" -eq "$TUI_LOG" ]] && return 1
    TUI_LOG="$next"
    TUI_LOG_SKIP=0
    TUI_LOG_FP=""
    return 0
}

tui_draw_logs() {
    local src i=0 body file
    tui_clamp_log_skip
    printf '  '
    for src in backend.log frontend.log postgres updater; do
        if [[ $i -eq $TUI_LOG ]]; then
            printf '%b[%s]%b  ' "$BOLD$CYAN" "$src" "$NC"
        else
            printf '%b %s %b  ' "$DIM" "$src" "$NC"
        fi
        i=$((i + 1))
    done
    if [[ "$TUI_LOG_SKIP" -gt 0 ]]; then
        printf '  %b^%s%b' "$DIM" "$TUI_LOG_SKIP" "$NC"
    fi
    echo ""
    printf '  %b%s%b\n' "$DIM" "$(tui_hline $((TUI_COLS - 4)))" "$NC"

    body="$(tui_log_body)"
    case "$TUI_LOG" in
        0)
            file="$PROJECT_ROOT/backend.log"
            if [[ -f "$file" ]]; then
                tui_emit_file_window "$file" "$body"
            else
                printf '\n  %bNo backend.log%b — start from Overview so logs land here.\n' "$YELLOW" "$NC"
            fi
            ;;
        1)
            file="$PROJECT_ROOT/frontend.log"
            if [[ -f "$file" ]]; then
                tui_emit_file_window "$file" "$body"
            else
                printf '\n  %bNo frontend.log%b — start from Overview so logs land here.\n' "$YELLOW" "$NC"
            fi
            ;;
        2)
            if docker_container_running "myriad-postgres-dev"; then
                docker logs --tail $((body + TUI_LOG_SKIP)) myriad-postgres-dev 2>&1 | tui_emit_stream_window "$body"
            else
                tui_draw_native_pg_log "$body"
            fi
            ;;
        3)
            if docker_container_running "myriad-updater-dev"; then
                docker logs --tail $((body + TUI_LOG_SKIP)) myriad-updater-dev 2>&1 | tui_emit_stream_window "$body"
            else
                printf '\n  Updater harness is not running.\n'
            fi
            ;;
    esac
}

tui_draw_native_pg_log() {
    local body="$1" log
    log="$(native_pg_log_path 2>/dev/null || true)"
    if [[ -n "$log" && -f "$log" ]]; then
        tui_emit_file_window "$log" "$body"
        return 0
    fi
    printf '\n  Native PostgreSQL is not a compose service.\n'
    printf '  %bNo local postgres log found.%b  Try: brew services info postgresql@18\n' "$DIM" "$NC"
}

tui_draw_tools() {
    local i=0 id count hint
    printf '\n  %bTools%b\n\n' "$BOLD" "$NC"
    count="$(tui_count "$TUI_TOOL_IDS")"
    while [[ $i -lt $count ]]; do
        id="$(tui_nth "$TUI_TOOL_IDS" $((i + 1)))"
        hint="$(tui_tool_hint "$id")"
        if [[ $i -eq $TUI_SEL ]]; then
            printf '  %b> %s%b  %b%s%b\n' \
                "$BRIGHT_CYAN" "$(tui_pad "$(tui_tool_label "$id")" 24)" "$NC" \
                "$DIM" "$hint" "$NC"
        else
            printf '    %s  %b%s%b\n' \
                "$(tui_pad "$(tui_tool_label "$id")" 24)" "$DIM" "$hint" "$NC"
        fi
        i=$((i + 1))
    done
}

tui_draw_help() {
    printf '\n  %bKeys%b\n' "$BOLD" "$NC"
    printf '    1-5 / tab / left right   switch tabs\n'
    printf '    up/dn j k  g/G           move / first / last\n'
    printf '    s / x / r                start / stop / restart\n'
    printf '    a / K                    start-all / stop-all\n'
    printf '    p                        psql     ?/h help   q quit\n'
    printf '    Logs                     up/dn / PgUp PgDn scroll   G tail\n'
    printf '    Esc               dismiss message / close help\n'
    printf '\n  Mark is on the left from ~70×18. Starts write backend.log / frontend.log.\n'
}

tui_draw_confirm() {
    printf '\n\n'
    printf '  %bConfirm%b  %s\n\n' "$BOLD$YELLOW" "$NC" "$TUI_CONFIRM_TEXT"
    printf '    y  yes      n  cancel\n'
}

tui_status_fp() {
    printf '%s' "$TUI_CACHE_DB_STATE|$TUI_CACHE_BE_STATE|$TUI_CACHE_FE_STATE|$TUI_CACHE_UP_STATE|$TUI_CACHE_DB_DETAIL|$TUI_CACHE_BE_DETAIL|$TUI_CACHE_FE_DETAIL|$TUI_CACHE_UP_DETAIL"
}

tui_stack_quiet() {
    [[ "$TUI_CACHE_DB_STATE" == "running" && \
       "$TUI_CACHE_BE_STATE" == "running" && \
       "$TUI_CACHE_FE_STATE" == "running" ]]
}

tui_apply_poll() {
    local want="$TUI_POLL_FAST"
    tui_stack_quiet && want="$TUI_POLL_IDLE"
    [[ "$want" -eq "$TUI_POLL_TIME" ]] && return 0
    TUI_POLL_TIME="$want"
    stty time "$TUI_POLL_TIME" min 0 2>/dev/null || true
}

tui_idle_tick() {
    TUI_IDLE=$((TUI_IDLE + 1))
    tui_size
    if [[ "$TUI_RESIZE" -eq 1 || "$TUI_COLS" != "$TUI_PREV_COLS" || "$TUI_ROWS" != "$TUI_PREV_ROWS" ]]; then
        TUI_RESIZE=0
        tui_render || true
        return 0
    fi
    tui_tick_clock
    tui_apply_poll
    local quiet=0 msgttl=16
    tui_stack_quiet && { quiet=1; msgttl=8; }
    if [[ -n "$TUI_MSG" && "$TUI_MSG_ERR" -eq 0 && $TUI_IDLE -ge $msgttl ]]; then
        tui_set_msg ""
        TUI_STATUS_DIRTY=1
        tui_render || true
        return 0
    fi
    # Booting: probe every 0.3s. All running: clock only, probe ~30s.
    if [[ $quiet -eq 1 && $((TUI_IDLE % 30)) -ne 0 ]]; then
        return 0
    fi
    case "$TUI_TAB" in
        0)
            local before after
            before="$(tui_status_fp)"
            tui_refresh_status || true
            after="$(tui_status_fp)"
            if [[ "$before" != "$after" ]]; then
                [[ "$TUI_MSG" == Postgres* ]] && tui_set_msg "$(tui_live_msg)"
                tui_render || true
            fi
            ;;
        1)
            local before="$TUI_PROCS"
            tui_refresh_procs || true
            [[ "$before" != "$TUI_PROCS" ]] && tui_render || true
            ;;
        2)
            local now="no"
            db_reachable && now="yes"
            if [[ "$now" != "${TUI_DB_REACH:-}" ]]; then
                TUI_DB_REACH="$now"
                tui_render || true
            fi
            ;;
        3)
            local fp
            fp="$(tui_log_fp)"
            if [[ "$fp" != "$TUI_LOG_FP" ]]; then
                TUI_LOG_FP="$fp"
                tui_render || true
            fi
            ;;
    esac
}

tui_render() {
    tui_size
    TUI_PREV_COLS="$TUI_COLS"
    TUI_PREV_ROWS="$TUI_ROWS"
    printf '\033[H'
    if [[ "$TUI_COLS" -lt $TUI_MIN_COLS || "$TUI_ROWS" -lt $TUI_MIN_ROWS ]]; then
        printf 'Myriad TUI needs at least %s×%s (now %s×%s). Widen the terminal.\n' \
            "$TUI_MIN_COLS" "$TUI_MIN_ROWS" "$TUI_COLS" "$TUI_ROWS"
        return
    fi
    tui_draw_header
    printf '\033[J'
    if [[ "$TUI_HELP" -eq 1 ]]; then
        tui_paint_body tui_draw_help
    elif [[ -n "$TUI_CONFIRM" ]]; then
        tui_paint_body tui_draw_confirm
    else
        case "$TUI_TAB" in
            0) tui_paint_body tui_draw_overview ;;
            1) tui_paint_body tui_draw_processes ;;
            2) tui_paint_body tui_draw_database ;;
            3) tui_paint_body tui_draw_logs ;;
            4) tui_paint_body tui_draw_tools ;;
        esac
    fi
    printf '\033[0m'
    printf '\033[%s;1H' "$((TUI_ROWS - 2))"
    tui_draw_footer
    printf '\033[J'
    [[ "$TUI_TAB" -eq 3 ]] && TUI_LOG_FP="$(tui_log_fp)"
}

# ---------- actions ----------

tui_store_sel() {
    case "$TUI_TAB" in
        0) TUI_SEL0="$TUI_SEL" ;;
        1) TUI_SEL1="$TUI_SEL" ;;
        2) TUI_SEL2="$TUI_SEL" ;;
        3) TUI_SEL3="$TUI_SEL" ;;
        4) TUI_SEL4="$TUI_SEL" ;;
    esac
}

tui_restore_sel() {
    case "$TUI_TAB" in
        0) TUI_SEL="$TUI_SEL0" ;;
        1) TUI_SEL="$TUI_SEL1" ;;
        2) TUI_SEL="$TUI_SEL2" ;;
        3) TUI_SEL="$TUI_SEL3" ;;
        4) TUI_SEL="$TUI_SEL4" ;;
        *) TUI_SEL=0 ;;
    esac
    local max
    max="$(tui_sel_max)"
    [[ $TUI_SEL -gt $max ]] && TUI_SEL=$max
    [[ $TUI_SEL -lt 0 ]] && TUI_SEL=0
}

tui_set_tab() {
    local next="$1"
    [[ "$next" -eq "$TUI_TAB" ]] && return 1
    tui_store_sel
    TUI_TAB="$next"
    TUI_HELP=0
    TUI_CONFIRM=""
    TUI_CONFIRM_TEXT=""
    # Keep errors visible when jumping to Logs after a failed start.
    [[ "$TUI_MSG_ERR" -eq 1 ]] || tui_set_msg ""
    [[ "$TUI_TAB" -eq 0 ]] && TUI_STATUS_DIRTY=1
    [[ "$TUI_TAB" -eq 1 ]] && tui_refresh_procs
    tui_restore_sel
    return 0
}

tui_sel_max() {
    case "$TUI_TAB" in
        0) echo $(( $(tui_count "$(tui_menu_ids)") - 1 )) ;;
        1)
            if [[ $TUI_PROC_COUNT -gt 0 ]]; then
                echo $((TUI_PROC_COUNT - 1))
            else
                echo 0
            fi
            ;;
        4) echo $(( $(tui_count "$TUI_TOOL_IDS") - 1 )) ;;
        *) echo 0 ;;
    esac
}

tui_move() {
    local max old="$TUI_SEL"
    max="$(tui_sel_max)"
    TUI_SEL=$((TUI_SEL + $1))
    [[ $TUI_SEL -lt 0 ]] && TUI_SEL=0
    [[ $TUI_SEL -gt $max ]] && TUI_SEL=$max
    [[ $TUI_SEL -eq $old ]] && return 1
    return 0
}

tui_bg_on() {
    DEV_START_BG=1
    USE_FG=0
    DETACH_EXPLICIT=1
}

tui_run_quiet() {
    local work="$1" done="$2" tmp rc last old_e=0
    shift 2
    if [[ -n "$work" ]]; then
        tui_set_msg "$work"
        tui_render || true
    fi
    tmp="$(mktemp "${TMPDIR:-/tmp}/myriad-dev.XXXXXX")" || {
        tui_set_msg "mktemp failed" 1
        return 0
    }
    [[ $- == *e* ]] && old_e=1
    tui_bg_on
    DEV_START_NOWAIT=1
    set +e
    "$@" >"$tmp" 2>&1
    rc=$?
    DEV_START_NOWAIT=0
    [[ $old_e -eq 1 ]] && set -e
    last="$(tui_strip "$(awk 'NF { line=$0 } END { print line }' "$tmp" | sanitize_dev_log)")"
    rm -f "$tmp"
    if [[ $rc -ne 0 ]]; then
        tui_set_msg "${last:-failed ($rc)}" 1
    elif [[ "$last" == *"already running"* ]]; then
        tui_set_msg "$last"
    else
        tui_set_msg "${done:-${work:-${last:-ok}}}"
    fi
    return 0
}

tui_suspend() {
    tui_leave
    echo ""
    set +e
    "$@"
    local rc=$?
    echo ""
    [[ $rc -ne 0 ]] && print_warning "Command exited $rc (TUI will resume)" && echo ""
    echo -e "${DIM}Press Enter to return to TUI...${NC}"
    read -r _ || true
    tui_enter
    tui_note_change
    return 0
}

tui_open_url() {
    local url="$1"
    if [[ "$OSTYPE" == "darwin"* ]]; then
        open "$url" >/dev/null 2>&1 || true
    elif have xdg-open; then
        xdg-open "$url" >/dev/null 2>&1 || true
    else
        tui_set_msg "Open $url"
        return
    fi
    tui_set_msg "Opened $url"
}

tui_note_change() {
    TUI_STATUS_DIRTY=1
    [[ "$TUI_TAB" -eq 1 ]] && tui_refresh_procs
}

tui_live_msg() {
    printf 'Postgres %s   Backend %s   Frontend %s' \
        "${TUI_CACHE_DB_STATE:-?}" "${TUI_CACHE_BE_STATE:-?}" "${TUI_CACHE_FE_STATE:-?}"
}

tui_mark_launch() {
    local until=$(( $(date +%s) + 45 ))
    case "$1" in
        database|all) TUI_EXPECT_DB_UNTIL=$until ;;
    esac
    case "$1" in
        backend|all) TUI_EXPECT_BE_UNTIL=$until ;;
    esac
    case "$1" in
        frontend|all) TUI_EXPECT_FE_UNTIL=$until ;;
    esac
}

tui_clear_launch() {
    case "$1" in
        database|all) TUI_EXPECT_DB_UNTIL=0 ;;
    esac
    case "$1" in
        backend|all) TUI_EXPECT_BE_UNTIL=0 ;;
    esac
    case "$1" in
        frontend|all) TUI_EXPECT_FE_UNTIL=0 ;;
    esac
}

tui_set_live_msg() {
    tui_refresh_status || true
    tui_apply_poll
    tui_set_msg "$(tui_live_msg)"
}

tui_start_svc() {
    tui_mark_launch "$1"
    case "$1" in
        database) tui_run_quiet "Starting database…" "" start_database ;;
        backend)  tui_run_quiet "Starting backend…" "" start_backend ;;
        frontend) tui_run_quiet "Starting frontend…" "" start_frontend ;;
        updater)  tui_run_quiet "Starting updater…" "" start_updater ;;
        all)      tui_run_quiet "Starting database + stack…" "" start_all ;;
        *)        tui_set_msg "cannot start $1" 1; return 0 ;;
    esac
    tui_note_change
    [[ "$TUI_MSG_ERR" -eq 1 ]] && return 0
    tui_set_live_msg
}

tui_stop_svc() {
    tui_clear_launch "$1"
    case "$1" in
        database) tui_run_quiet "Stopping database…" "" stop_database ;;
        backend)  tui_run_quiet "Stopping backend…" "" stop_backend ;;
        frontend) tui_run_quiet "Stopping frontend…" "" stop_frontend ;;
        updater)  tui_run_quiet "Stopping updater…" "" stop_updater ;;
        all)      tui_run_quiet "Stopping all services…" "" stop_all ;;
        *)        tui_set_msg "cannot stop $1" 1; return 0 ;;
    esac
    tui_note_change
    [[ "$TUI_MSG_ERR" -eq 1 ]] && return 0
    tui_set_live_msg
}

tui_restart_svc() {
    local svc="$1"
    [[ -n "$svc" ]] || return 0
    if [[ "$svc" == "all" ]]; then
        tui_mark_launch all
        tui_run_quiet "Restarting database + stack…" "" restart_all
        tui_note_change
        [[ "$TUI_MSG_ERR" -eq 1 ]] || tui_set_live_msg
        return 0
    fi
    tui_stop_svc "$svc"
    tui_start_svc "$svc"
}

tui_overview_target() {
    case "$(tui_nth "$(tui_menu_ids)" $((TUI_SEL + 1)))" in
        start-all|stop-all|restart-all) echo all ;;
        database|backend|frontend|updater) tui_nth "$(tui_menu_ids)" $((TUI_SEL + 1)) ;;
        *) echo "" ;;
    esac
}

tui_run_menu() {
    local id
    id="$(tui_nth "$(tui_menu_ids)" $((TUI_SEL + 1)))"
    case "$id" in
        start-all)   tui_start_svc all ;;
        stop-all)    tui_stop_svc all ;;
        restart-all) tui_restart_svc all ;;
        database|backend|frontend|updater) tui_start_svc "$id" ;;
        clean) tui_ask_confirm clean "Drop public tables + build artifacts?" ;;
        *) tui_set_msg "unknown menu $id" 1 ;;
    esac
}

tui_proc_at() {
    printf '%s' "$TUI_PROCS" | awk -v n="$(( $1 + 1 ))" 'NF && NR==n { print; exit }'
}

tui_kill_selected() {
    local sig="$1" line kind id
    line="$(tui_proc_at "$TUI_SEL")"
    [[ -n "$line" ]] || return 0
    IFS='|' read -r kind id _ <<< "$line"
    # Never SIGTERM the postmaster — stop via brew / compose.
    if [[ "$kind" == "postgres" ]]; then
        tui_stop_svc database
        return 0
    fi
    if [[ "$id" =~ ^[0-9]+$ ]]; then
        if [[ "$sig" == "KILL" ]]; then
            kill -KILL "$id" 2>/dev/null || true
        else
            terminate_pids "selected" "$id" >/dev/null 2>&1 || true
        fi
    elif [[ "$sig" == "KILL" ]]; then
        docker kill "$id" >/dev/null 2>&1 || true
    else
        docker stop "$id" >/dev/null 2>&1 || true
    fi
    tui_note_change
    tui_set_msg "signaled $id ($sig)"
}

tui_feed_clean() {
    printf 'yes\n' | clean_project
}

tui_run_tool() {
    local id="$1"
    case "$id" in
        doctor)        tui_suspend run_doctor ;;
        db-setup)      tui_suspend run_db_setup ;;
        psql)
            parse_db_url || { tui_set_msg "bad DATABASE_URL" 1; return; }
            have psql || { tui_set_msg "psql not found" 1; return; }
            if ! db_reachable; then
                tui_set_msg "Database is not reachable -- s to start, or Tools -> Create role + database" 1
                return
            fi
            tui_suspend env PGPASSWORD="$DB_PASS" psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME"
            ;;
        open-frontend)
            if ! frontend_health_ok && ! frontend_port_in_use; then
                tui_set_msg "Frontend is not running — start it from Overview" 1
                return
            fi
            tui_open_url "http://localhost:${FRONTEND_PORT}"
            ;;
        open-backend)
            if ! backend_health_ok && ! backend_port_in_use; then
                tui_set_msg "Backend is not running — start it from Overview" 1
                return
            fi
            tui_open_url "http://localhost:${BACKEND_PORT}/health"
            ;;
        clean)         tui_ask_confirm clean "Drop public tables + build artifacts?" ;;
        *)             tui_set_msg "unknown tool $id" 1 ;;
    esac
}

tui_ask_confirm() {
    TUI_CONFIRM="$1"
    TUI_CONFIRM_TEXT="$2"
}

tui_do_confirm() {
    case "$TUI_CONFIRM" in
        clean)
            tui_suspend tui_feed_clean
            tui_set_msg "clean finished"
            ;;
        kill) tui_kill_selected KILL ;;
    esac
    TUI_CONFIRM=""
    TUI_CONFIRM_TEXT=""
}

tui_handle_key() {
    local key="$1" target
    [[ "$key" == "timeout" ]] && return 0

    if [[ "$TUI_HELP" -eq 1 ]]; then
        case "$key" in
            \?|esc|q|h) TUI_HELP=0 ;;
            *) return 2 ;;
        esac
        return 0
    fi

    if [[ -n "$TUI_CONFIRM" ]]; then
        case "$key" in
            y|Y) tui_do_confirm ;;
            n|N|esc|q) TUI_CONFIRM=""; TUI_CONFIRM_TEXT="" ;;
            *) return 2 ;;
        esac
        return 0
    fi

    case "$key" in
        q|Q) return 1 ;;
        \?|h) TUI_HELP=1 ;;
        esc)
            [[ -n "$TUI_MSG" ]] || return 2
            tui_set_msg ""
            ;;
        1) tui_set_tab 0 || return 2 ;;
        2) tui_set_tab 1 || return 2 ;;
        3) tui_set_tab 2 || return 2 ;;
        4) tui_set_tab 3 || return 2 ;;
        5) tui_set_tab 4 || return 2 ;;
        tab) tui_set_tab $(( (TUI_TAB + 1) % TUI_TAB_COUNT )) || return 2 ;;
        right)
            if [[ "$TUI_TAB" -eq 3 ]]; then
                tui_set_log $(( (TUI_LOG + 1) % TUI_LOG_COUNT )) || true
            else
                tui_set_tab $(( (TUI_TAB + 1) % TUI_TAB_COUNT ))
            fi
            ;;
        left)
            if [[ "$TUI_TAB" -eq 3 ]]; then
                tui_set_log $(( (TUI_LOG + TUI_LOG_COUNT - 1) % TUI_LOG_COUNT )) || true
            else
                tui_set_tab $(( (TUI_TAB + TUI_TAB_COUNT - 1) % TUI_TAB_COUNT ))
            fi
            ;;
        up|k)
            if [[ "$TUI_TAB" -eq 3 ]]; then
                local oldskip="$TUI_LOG_SKIP"
                TUI_LOG_SKIP=$((TUI_LOG_SKIP + 1))
                tui_clamp_log_skip
                [[ "$TUI_LOG_SKIP" -eq "$oldskip" ]] && return 2
                TUI_LOG_FP=""
            else
                tui_move -1 || return 2
            fi
            ;;
        down|j)
            if [[ "$TUI_TAB" -eq 3 ]]; then
                [[ "$TUI_LOG_SKIP" -eq 0 ]] && return 2
                TUI_LOG_SKIP=$((TUI_LOG_SKIP - 1))
                TUI_LOG_FP=""
            else
                tui_move 1 || return 2
            fi
            ;;
        pgup)
            [[ "$TUI_TAB" -eq 3 ]] || return 2
            local oldskip="$TUI_LOG_SKIP"
            TUI_LOG_SKIP=$((TUI_LOG_SKIP + $(tui_log_body) ))
            tui_clamp_log_skip
            [[ "$TUI_LOG_SKIP" -eq "$oldskip" ]] && return 2
            TUI_LOG_FP=""
            ;;
        pgdn)
            [[ "$TUI_TAB" -eq 3 ]] || return 2
            [[ "$TUI_LOG_SKIP" -eq 0 ]] && return 2
            TUI_LOG_SKIP=$((TUI_LOG_SKIP - $(tui_log_body) ))
            [[ $TUI_LOG_SKIP -lt 0 ]] && TUI_LOG_SKIP=0
            TUI_LOG_FP=""
            ;;
        g)
            if [[ "$TUI_TAB" -eq 3 ]]; then
                local oldskip="$TUI_LOG_SKIP"
                TUI_LOG_SKIP=999999
                tui_clamp_log_skip
                [[ "$TUI_LOG_SKIP" -eq "$oldskip" ]] && return 2
                TUI_LOG_FP=""
            else
                [[ "$TUI_SEL" -eq 0 ]] && return 2
                TUI_SEL=0
            fi
            ;;
        G)
            if [[ "$TUI_TAB" -eq 3 ]]; then
                [[ "$TUI_LOG_SKIP" -eq 0 ]] && return 2
                TUI_LOG_SKIP=0
                TUI_LOG_FP=""
            else
                local max
                max="$(tui_sel_max)"
                [[ "$TUI_SEL" -eq "$max" ]] && return 2
                TUI_SEL=$max
            fi
            ;;
        a) tui_start_svc all ;;
        K) tui_stop_svc all ;;
        p) tui_run_tool psql ;;
        b)
            [[ "$TUI_TAB" -eq 3 ]] || return 2
            tui_set_log 0 || true
            ;;
        f)
            [[ "$TUI_TAB" -eq 3 ]] || return 2
            tui_set_log 1 || true
            ;;
        d)
            [[ "$TUI_TAB" -eq 3 ]] || return 2
            tui_set_log 2 || true
            ;;
        u)
            [[ "$TUI_TAB" -eq 3 ]] || return 2
            tui_set_log 3 || true
            ;;
        s)
            case "$TUI_TAB" in
                0)
                    target="$(tui_overview_target)"
                    [[ -n "$target" ]] || return 2
                    tui_start_svc "$target"
                    ;;
                2) tui_start_svc database ;;
                *) return 2 ;;
            esac
            ;;
        x)
            case "$TUI_TAB" in
                0)
                    target="$(tui_overview_target)"
                    [[ -n "$target" ]] || return 2
                    tui_stop_svc "$target"
                    ;;
                1) tui_kill_selected TERM ;;
                2) tui_stop_svc database ;;
                *) return 2 ;;
            esac
            ;;
        X)
            [[ "$TUI_TAB" -eq 1 ]] || return 2
            local xkind
            xkind="$(tui_proc_at "$TUI_SEL" | awk -F'|' '{print $1}')"
            if [[ "$xkind" == "postgres" ]]; then
                tui_stop_svc database
            else
                tui_ask_confirm kill "SIGKILL selected process?"
            fi
            ;;
        r)
            case "$TUI_TAB" in
                0)
                    target="$(tui_overview_target)"
                    [[ -n "$target" ]] || return 2
                    tui_restart_svc "$target"
                    ;;
                1) tui_refresh_procs ;;
                2) TUI_STATUS_DIRTY=1 ;;
                3) TUI_LOG_FP="" ;;
                *) return 2 ;;
            esac
            ;;
        enter)
            case "$TUI_TAB" in
                0) tui_run_menu ;;
                2) tui_start_svc database ;;
                4) tui_run_tool "$(tui_nth "$TUI_TOOL_IDS" $((TUI_SEL + 1)))" ;;
                *) return 2 ;;
            esac
            ;;
        *) return 2 ;;
    esac
    return 0
}

# ---------- entry ----------

run_tui() {
    set +e
    tui_attach_tty
    if [[ ! -t 0 || ! -t 1 ]]; then
        print_error "TUI needs a real Terminal window"
        print_info "Open Terminal.app, then: $0"
        print_info "Or a one-shot snapshot: $0 status"
        show_mini_logo
        show_status_dashboard || true
        tui_pause
        set -e
        return 0
    fi

    maybe_autodetect_native
    parse_db_url || true
    tui_refresh_status || true
    tui_logo_init || true

    TUI_RESIZE=0
    trap 'TUI_RESIZE=1' WINCH
    trap 'tui_leave; exit 0' INT TERM EXIT
    tui_enter
    tui_render || true

    while true; do
        tui_read_key
        if [[ "$TUI_KEY" == "timeout" ]]; then
            tui_idle_tick
            continue
        fi
        TUI_IDLE=0
        tui_handle_key "$TUI_KEY"
        case $? in
            1) break ;;
            2) continue ;;
        esac
        tui_render || true
    done

    trap - INT TERM EXIT WINCH
    tui_leave
    echo -e "${CYAN}${ICON_HEART} Services keep running. ${DIM}$0 status${NC} to check, ${DIM}$0 stop${NC} to shut down."
    set -e
    return 0
}
