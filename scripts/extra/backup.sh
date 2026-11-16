#!/usr/bin/env bash
# Site disaster backup / restore: PostgreSQL + backend_data + .env.
# Does not include regenerable cache volumes. Updater ./pgdata snapshots are
# not a substitute for this backup.
#
# A backup is two parts kept apart:
# - the data part (one directory per run, keep as many as you like): the
#   database without her log's key rows, and backend_data without the data
#   key. It holds no key and no secret;
# - the keys part (one directory, overwritten every run): .env, the data key
#   and her log's key rows. Keep only the newest copy: forgetting deletes an
#   object's key, and that reaches the backups only once no older keys part
#   is left anywhere.
#
# Two deploys are supported. The default is the official Docker Compose one.
# BACKUP_MODE=local is a backend run as a plain process on this host: the
# database from DATABASE_URL in backend/.env, the data in backend/data. It
# never stops anything; pg_dump's snapshot keeps the database consistent.
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
if [ ! -t 1 ]; then RED=''; GREEN=''; YELLOW=''; CYAN=''; NC=''; fi
ok()   { echo -e "${GREEN}$1${NC}"; }
info() { echo -e "${CYAN}$1${NC}"; }
warn() { echo -e "${YELLOW}$1${NC}"; }
err()  { echo -e "${RED}$1${NC}"; }

ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
cd "$ROOT"

COMMAND="${1:-}"
shift || true

DOCKER="${DOCKER:-docker}"

env_file_value() {
    env_file_key .env "$1"
}

# Read KEY= from a dotenv file. Never prints the value.
env_file_key() {
    local file="$1" key="$2"
    [ -f "$file" ] || return 0
    grep -E "^${key}=" "$file" 2>/dev/null | head -1 | cut -d= -f2-
}

is_local() {
    [ "${BACKUP_MODE:-compose}" = local ]
}

case "${BACKUP_MODE:-compose}" in
    compose|local) ;;
    *) err "BACKUP_MODE must be compose or local"; exit 1 ;;
esac

local_backend_dir() {
    printf '%s' "${LOCAL_BACKEND_DIR:-$ROOT/backend}"
}

local_data_dir() {
    printf '%s/data' "$(local_backend_dir)"
}

# The deploy's own .env: the one the backend reads.
deploy_env_file() {
    if is_local; then
        printf '%s/.env' "$(local_backend_dir)"
    else
        printf '.env'
    fi
}

# What the manifests call the deploy, so a keys part cannot pair with a data
# part of another deploy.
backup_project() {
    if is_local; then
        printf 'local'
    else
        project_name
    fi
}

compose_bin() {
    if $DOCKER compose version >/dev/null 2>&1; then
        echo "$DOCKER compose"
    elif command -v docker-compose >/dev/null 2>&1; then
        echo "docker-compose"
    else
        err "neither docker compose nor docker-compose is available"
        exit 1
    fi
}

project_name() {
    local project="${COMPOSE_PROJECT_NAME:-}"
    [ -z "$project" ] && project="$(env_file_value COMPOSE_PROJECT_NAME)"
    [ -z "$project" ] && project="myriad"
    printf '%s' "$project"
}

# Restore pins RESTORE_PROJECT before overwriting .env so compose cannot retarget.
active_project() {
    printf '%s' "${RESTORE_PROJECT:-$(project_name)}"
}

compose() {
    local kind
    kind="$(compose_bin)"
    # shellcheck disable=SC2086
    $kind --env-file .env -p "$(active_project)" "$@"
}

# Official writers that share backend_data / the same database.
# Stop workers before web so persona/federation cannot keep writing.
WRITER_STOP_ORDER="federation-worker persona-worker backend"
WRITER_START_ORDER="backend federation-worker persona-worker"

# True only when the named compose service has a running container.
# A failed inspect/ps is an error, not “not running”.
# Missing service / no container is “not running”.
service_is_running() {
    local service="$1"
    local id running
    id="$(compose ps -q "$service")" || {
        err "cannot determine $service status"
        return 2
    }
    [ -n "$id" ] || return 1
    running="$($DOCKER inspect -f '{{.State.Running}}' "$id")" || {
        err "cannot inspect $service container"
        return 2
    }
    [ "$running" = "true" ]
}

# Stop running writers. Detection failure or stop failure aborts.
# Sets WRITERS_STOPPED_BY_US=1 as soon as any unit is actually stopped so a
# later quiesce failure still restarts writers already down.
quiesce_writers() {
    local svc state
    STOPPED_WRITERS=""
    for svc in $WRITER_STOP_ORDER; do
        if service_is_running "$svc"; then
            info "==> stopping $svc to quiesce writes"
            if ! compose stop "$svc"; then
                err "failed to stop $svc after writers already quiesced: $STOPPED_WRITERS"
                return 1
            fi
            STOPPED_WRITERS="${STOPPED_WRITERS:+$STOPPED_WRITERS }$svc"
            WRITERS_STOPPED_BY_US=1
        else
            state=$?
            if [ "$state" -eq 2 ]; then
                return 1
            fi
            info "$svc is not running"
        fi
    done
}

load_backup_storage() {
    # shellcheck source=backend-storage.sh
    source "$ROOT/scripts/extra/backend-storage.sh"
    load_backend_storage "$DOCKER" "$(active_project)" "$ROOT" existing
}

preflight_restore_storage() {
    load_backup_storage || return 1
    # Prove mount/write access before stopping writers or replacing .env/database.
    "$DOCKER" run --rm --network none \
        --mount "$BACKEND_DATA_MOUNT,dst=/data" \
        alpine:3.20 sh -eu -c '
            probe="$(mktemp /data/.myriad-restore-probe.XXXXXX)"
            rm -f -- "$probe"
        '
}

# POSIX payload used by the alpine restore container and by host tests.
# DEST and ARCHIVE are paths inside the shell that runs this text.
volume_restore_posix() {
    cat <<'EOF'
set -eu
dest="${DEST:?}"
archive="${ARCHIVE:?}"
if [ ! -d "$dest" ]; then
    echo "restore dest is not a directory" >&2
    exit 1
fi
if [ ! -f "$archive" ]; then
    echo "restore archive missing" >&2
    exit 1
fi
# Archive metadata may restore a historical 0755 root. Keep private data private
# even when extraction fails after applying that metadata.
chmod 700 "$dest"
trap 'status=$?; chmod 700 "$dest" || exit 1; exit "$status"' 0
find "$dest" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
leftover=""
for p in "$dest"/* "$dest"/.[!.]* "$dest"/..?*; do
    if [ -e "$p" ] || [ -L "$p" ]; then
        leftover="$p"
        break
    fi
done
if [ -n "$leftover" ]; then
    echo "volume cleanup incomplete; refuse unpack ($leftover)" >&2
    exit 1
fi
tar xzf "$archive" -C "$dest"
if [ -n "${RESTORE_OWNER:-}" ]; then
    # Writers are recreated with --no-deps, so volume-init will not repair an
    # old archive's nested root-owned/private files. Do not follow symlinks.
    chown -Rh "$RESTORE_OWNER" "$dest"
    find "$dest" -type d -exec chmod u+rwx {} +
    find "$dest" -type f -exec chmod u+rw {} +
fi
EOF
}

restore_data_tree() {
    local dest="$1" archive="$2"
    DEST="$dest" ARCHIVE="$archive" sh -c "$(volume_restore_posix)"
}

# Official compose inits Postgres with POSTGRES_PASSWORD from the host .env.
# This restore only targets a cluster that already uses the backup's password.
# A different role password is unsupported: refuse before stop / copy / restore.
require_same_postgres_password() {
    local backup_env="$1"
    local current backup
    if [ ! -f .env ]; then
        err "restore requires the current .env so it can match the running Postgres password"
        exit 1
    fi
    current="$(env_file_key .env POSTGRES_PASSWORD)"
    backup="$(env_file_key "$backup_env" POSTGRES_PASSWORD)"
    if [ -z "$current" ] || [ -z "$backup" ]; then
        err "POSTGRES_PASSWORD missing in current or backup .env; refuse restore"
        exit 1
    fi
    if [ "$current" != "$backup" ]; then
        err "restore refuses a different PostgreSQL password than the running cluster"
        err "supported scope: same compose project, same POSTGRES_PASSWORD as the live role"
        err "re-init the cluster to the backup password first, or restore onto a matching host"
        exit 1
    fi
}

# Refuse before stop if the backup names a different compose project.
# Pins RESTORE_PROJECT so a later .env overwrite cannot switch A → B.
require_same_compose_project() {
    local manifest="$1" backup_env="$2"
    local backup live
    live="$(project_name)"
    backup="$(manifest_value "$manifest" project)"
    if [ -z "$backup" ]; then
        backup="$(env_file_key "$backup_env" COMPOSE_PROJECT_NAME)"
    fi
    [ -z "$backup" ] && backup="myriad"
    if [ "$live" != "$backup" ]; then
        err "restore refuses compose project '$backup'; live project is '$live'"
        err "stop and restore only target the project that is currently selected"
        exit 1
    fi
    export RESTORE_PROJECT="$live"
}

# KEY= from a MANIFEST.txt; empty when the file or line is missing.
manifest_value() {
    local file="$1" key="$2"
    [ -f "$file" ] || return 0
    grep -E "^${key}=" "$file" 2>/dev/null | head -1 | cut -d= -f2-
}

keys_dir() {
    if is_local; then
        printf '%s' "${BACKUP_KEYS_DIR:-$HOME/Backups/myriad/keys}"
    else
        printf '%s' "${BACKUP_KEYS_DIR:-$ROOT/backup-keys}"
    fi
}

absolute_path() {
    case "$1" in
        /*) printf '%s' "$1" ;;
        *) printf '%s/%s' "$PWD" "$1" ;;
    esac
}

# The keys part must not land inside a data part: that is the copy kept many times.
require_keys_outside() {
    local keys data
    keys="$(absolute_path "$1")"
    data="$(absolute_path "$2")"
    case "$keys/" in
        "$data/"*)
            err "the keys part must not be inside the data part ($data); set BACKUP_KEYS_DIR elsewhere"
            return 1
            ;;
    esac
}

sha256_stdin() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum | cut -d' ' -f1
    else
        shasum -a 256 | cut -d' ' -f1
    fi
}

# A short name for the data key in a file, so the two parts of a backup can be
# matched without either holding the other's secret. Never the key itself.
data_key_fingerprint() {
    local file="$1"
    if [ ! -s "$file" ]; then
        printf 'none'
        return 0
    fi
    { printf 'myriad-data-key-fingerprint\n'; tr -d '[:space:]' < "$file"; } \
        | sha256_stdin | cut -c1-16
}

# Her log's key tables: rows go in the keys part, never in the data part's dump.
HER_LOG_KEY_TABLES="agent_log_audience_keys agent_log_object_keys"

her_log_key_columns() {
    case "$1" in
        agent_log_audience_keys) printf 'user_id, audience, sealed, created_at' ;;
        agent_log_object_keys) printf 'user_id, audience, object, subject, sealed, created_at' ;;
    esac
}

# psql on the deploy's database: the compose service, or libpq set up by
# local_db_setup with LOCAL_DB naming the database to work on.
db_psql() {
    if is_local; then
        psql -X -w -v ON_ERROR_STOP=1 -d "${LOCAL_DB:?local_db_setup first}" "$@"
    else
        compose exec -T postgres psql -U myriad -d myriad -v ON_ERROR_STOP=1 "$@"
    fi
}

table_exists() {
    [ "$(db_psql -At -c "SELECT to_regclass('public.$1') IS NOT NULL")" = "t" ]
}

# A directory lock other runs wait on (a daily backup and an hourly keys
# rewrite may meet). A lock whose holder is gone is taken over.
LOCKS_HELD=""

take_lock() {
    local lock="$1" waited=0 pid
    while ! mkdir "$lock" 2>/dev/null; do
        pid="$(cat "$lock/pid" 2>/dev/null || true)"
        if [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null; then
            rm -rf -- "$lock"
            continue
        fi
        if [ "$waited" -ge "${BACKUP_LOCK_WAIT:-900}" ]; then
            err "another backup run still holds $lock"
            return 1
        fi
        sleep 5
        waited=$((waited + 5))
    done
    printf '%s\n' "$$" > "$lock/pid"
    LOCKS_HELD="${LOCKS_HELD:+$LOCKS_HELD
}$lock"
}

release_lock() {
    local lock="$1" held="" rest=""
    if [ "$(cat "$lock/pid" 2>/dev/null || true)" = "$$" ]; then
        rm -rf -- "$lock"
    fi
    while IFS= read -r held; do
        if [ -n "$held" ] && [ "$held" != "$lock" ]; then
            rest="${rest:+$rest
}$held"
        fi
    done <<EOF
${LOCKS_HELD:-}
EOF
    LOCKS_HELD="$rest"
}

release_locks() {
    local lock
    [ -n "${LOCKS_HELD:-}" ] || return 0
    while IFS= read -r lock; do
        if [ -n "$lock" ] && [ "$(cat "$lock/pid" 2>/dev/null || true)" = "$$" ]; then
            rm -rf -- "$lock"
        fi
    done <<EOF
$LOCKS_HELD
EOF
    LOCKS_HELD=""
}

# Write the keys part into a fresh sibling directory, then swap it in, so a
# failure leaves the previous copy whole. Every older copy left by an earlier
# crash is removed: it may hold keys forgotten since.
write_keys() {
    local dir="$1" next old table stale key_from rows="" env_src
    env_src="$(deploy_env_file)"
    dir="$(absolute_path "$dir")"
    mkdir -p "$(dirname "$dir")"
    take_lock "$dir.lock" || return 1
    for stale in "$dir".next.*; do
        [ -e "$stale" ] && rm -rf -- "$stale"
    done
    next="$dir.next.$$"
    umask 077
    mkdir -m 700 "$next"
    # Time Machine would keep every older keys part, and an older keys part
    # still opens what was forgotten since. The mark sits on the directory,
    # so each new copy gets it again.
    if is_local && command -v tmutil >/dev/null 2>&1; then
        tmutil addexclusion "$next" >/dev/null 2>&1 || warn "could not keep the keys part out of Time Machine"
    fi

    cp "$env_src" "$next/env"
    chmod 600 "$next/env"

    # The official compose reads the data key from backend_data/.secret-key.
    # MYRIAD_DATA_KEY reaches the backend only through a compose override; .env
    # goes into the keys part whole, so its value is kept either way. It names
    # the data key here only when there is no file.
    if is_local; then
        : > "$next/data-key"
        if [ -f "$(local_data_dir)/.secret-key" ]; then
            cat "$(local_data_dir)/.secret-key" > "$next/data-key"
        fi
    else
        "$DOCKER" run --rm --network none \
            --mount "$BACKEND_DATA_MOUNT,dst=/data,readonly" \
            alpine:3.20 \
            sh -c 'if [ -f /data/.secret-key ]; then cat /data/.secret-key; fi' > "$next/data-key"
    fi
    if [ -s "$next/data-key" ]; then
        key_from=file
    elif grep -Eq '^MYRIAD_DATA_KEY=.+' "$env_src"; then
        key_from=env
        env_file_key "$env_src" MYRIAD_DATA_KEY > "$next/data-key"
    else
        key_from=none
        warn "no data key yet (backend never started?); the keys part has none"
    fi
    chmod 600 "$next/data-key"

    for table in $HER_LOG_KEY_TABLES; do
        : > "$next/$table.copy"
        if table_exists "$table"; then
            db_psql -c "COPY $table ($(her_log_key_columns "$table")) TO STDOUT" > "$next/$table.copy"
        fi
        chmod 600 "$next/$table.copy"
        rows="${rows:+$rows }$table=$(wc -l < "$next/$table.copy" | tr -d ' ')"
    done

    cat > "$next/MANIFEST.txt" <<EOF
format=2
part=keys
created=$(date -u +%Y-%m-%dT%H:%M:%SZ)
project=$(backup_project)
data_key_source=$key_from
data_key_fingerprint=$(data_key_fingerprint "$next/data-key")
rows=$rows
EOF
    chmod 600 "$next/MANIFEST.txt"

    old="$dir.old.$$"
    if [ -e "$dir" ]; then
        mv -- "$dir" "$old"
    fi
    mv -- "$next" "$dir"
    for stale in "$dir".old.*; do
        [ -e "$stale" ] && rm -rf -- "$stale"
    done
    release_lock "$dir.lock"
    ok "keys part written: $dir (keep only this copy)"
}

# Refuse a keys part that does not belong with the data part, before anything stops.
require_matching_keys() {
    local from="$1" keys="$2" file data_print keys_print data_made keys_made
    [ -f "$keys/MANIFEST.txt" ] || { err "missing $keys/MANIFEST.txt"; exit 1; }
    [ "$(manifest_value "$keys/MANIFEST.txt" part)" = "keys" ] \
        || { err "$keys is not a keys part"; exit 1; }
    for file in env data-key $(for t in $HER_LOG_KEY_TABLES; do printf '%s.copy ' "$t"; done); do
        [ -f "$keys/$file" ] || { err "missing $keys/$file"; exit 1; }
    done
    if [ "$(manifest_value "$keys/MANIFEST.txt" project)" != "$(manifest_value "$from/MANIFEST.txt" project)" ]; then
        err "the keys part and the data part name different compose projects"
        exit 1
    fi
    keys_print="$(data_key_fingerprint "$keys/data-key")"
    if [ "$keys_print" != "$(manifest_value "$keys/MANIFEST.txt" data_key_fingerprint)" ]; then
        err "the keys part's data key does not match its own manifest"
        exit 1
    fi
    data_print="$(manifest_value "$from/MANIFEST.txt" data_key_fingerprint)"
    # A data part made before the backend ever started has no data key to match.
    if [ "$data_print" != "none" ] && [ "$data_print" != "$keys_print" ]; then
        err "the keys part holds a different data key than the data part was made with"
        exit 1
    fi
    data_made="$(manifest_value "$from/MANIFEST.txt" created)"
    keys_made="$(manifest_value "$keys/MANIFEST.txt" created)"
    if [[ "$keys_made" < "$data_made" ]]; then
        warn "the keys part is older than the data part; her log rows newer than it will not open"
    fi
}

# Her log's key rows into the restored database. The keys part can be newer
# than the data part: rows of accounts the dump does not have are left out.
restore_her_log_keys() {
    local keys="$1" table columns
    for table in $HER_LOG_KEY_TABLES; do
        [ -s "$keys/$table.copy" ] || continue
        if ! table_exists "$table"; then
            warn "$table is not in the restored database; its key rows are left out"
            continue
        fi
        columns="$(her_log_key_columns "$table")"
        {
            printf 'BEGIN;\n'
            printf 'CREATE TEMP TABLE restore_keys (LIKE %s) ON COMMIT DROP;\n' "$table"
            printf 'COPY restore_keys (%s) FROM STDIN;\n' "$columns"
            cat "$keys/$table.copy"
            printf '\\.\n'
            printf 'INSERT INTO %s (%s) SELECT %s FROM restore_keys r\n' "$table" "$columns" "$columns"
            printf '  WHERE EXISTS (SELECT 1 FROM users u WHERE u.id = r.user_id) ON CONFLICT DO NOTHING;\n'
            printf 'COMMIT;\n'
        } | db_psql -q >/dev/null
    done
}

# The data key file back into backend_data. Read from stdin so it never sits
# in an argument or the environment.
restore_data_key_file() {
    local keys="$1"
    [ "$(manifest_value "$keys/MANIFEST.txt" data_key_source)" = "file" ] || return 0
    "$DOCKER" run --rm -i --network none \
        --mount "$BACKEND_DATA_MOUNT,dst=/data" \
        alpine:3.20 \
        sh -eu -c 'umask 077; cat > /data/.secret-key.restore; chown 1000:1000 /data/.secret-key.restore; mv /data/.secret-key.restore /data/.secret-key' \
        < "$keys/data-key"
}

wait_backend_ready() {
    local i
    info "==> waiting for backend /ready"
    for i in $(seq 1 30); do
        if compose exec -T backend wget -qO- http://127.0.0.1:1103/ready >/dev/null 2>&1 \
            || compose exec -T backend curl -fsS http://127.0.0.1:1103/ready >/dev/null 2>&1; then
            ok "backend /ready"
            return 0
        fi
        sleep 2
    done
    err "backend /ready did not succeed after restore; do not treat frontend HTML 200 as ready"
    return 1
}

# ---- Local deploy (BACKUP_MODE=local) ----------------------------------------

# %XX in a URL part back to bytes. A backslash stays a backslash.
url_decode() {
    local s="${1//\\/\\\\}"
    printf '%b' "${s//%/\\x}"
}

# A field of a libpq password file: backslash and colon are escaped.
pgpass_field() {
    local s="${1//\\/\\\\}"
    printf '%s' "${s//:/\\:}"
}

# Point libpq at DATABASE_URL from a deploy .env. The password goes into a
# private password file, never into an argument, the environment, or a message.
# Sets LOCAL_LIVE_DB (the database the URL names) and LOCAL_DB (where to work;
# the same unless the caller changes it).
local_db_setup() {
    local env_file="$1" url rest hostport auth="" user="" pass="" host port db
    url="$(env_file_key "$env_file" DATABASE_URL)"
    url="${url%\"}"; url="${url#\"}"; url="${url%\'}"; url="${url#\'}"
    [ -n "$url" ] || { err "DATABASE_URL is missing in $env_file"; return 1; }
    case "$url" in
        postgres://*|postgresql://*) rest="${url#*://}" ;;
        *) err "DATABASE_URL in $env_file is not a postgres:// URL"; return 1 ;;
    esac
    rest="${rest%%\?*}"
    hostport="${rest%%/*}"
    db=""
    case "$rest" in */*) db="${rest#*/}" ;; esac
    case "$hostport" in
        *@*) auth="${hostport%@*}"; hostport="${hostport##*@}" ;;
    esac
    user="${auth%%:*}"
    case "$auth" in *:*) pass="${auth#*:}" ;; esac
    case "$hostport" in
        \[*) err "DATABASE_URL in $env_file uses an IPv6 host; give it as a name"; return 1 ;;
        *:*) host="${hostport%:*}"; port="${hostport##*:}" ;;
        *) host="$hostport"; port=5432 ;;
    esac
    [ -n "$host" ] || host=localhost
    [ -n "$db" ] || { err "DATABASE_URL in $env_file names no database"; return 1; }
    user="$(url_decode "$user")"
    db="$(url_decode "$db")"

    unset PGPASSWORD PGSERVICE PGSERVICEFILE PGPASSFILE
    export PGHOST="$host" PGPORT="$port" PGDATABASE="$db"
    if [ -n "$user" ]; then export PGUSER="$user"; else unset PGUSER; fi
    if [ -n "$pass" ]; then
        PGPASS_DIR="$(mktemp -d "${TMPDIR:-/tmp}/myriad-backup-pg.XXXXXX")"
        chmod 700 "$PGPASS_DIR"
        (
            umask 077
            printf '%s:%s:*:%s:%s\n' "$(pgpass_field "$host")" "$(pgpass_field "$port")" \
                "$(pgpass_field "${user:-*}")" "$(pgpass_field "$(url_decode "$pass")")" \
                > "$PGPASS_DIR/pgpass"
        )
        export PGPASSFILE="$PGPASS_DIR/pgpass"
    fi
    LOCAL_LIVE_DB="$db"
    LOCAL_DB="$db"
}

local_cleanup() {
    if [ -n "${PGPASS_DIR:-}" ]; then
        rm -rf -- "$PGPASS_DIR"
        PGPASS_DIR=""
    fi
    release_locks
}

# Refuse a run that would leave less than BACKUP_MIN_FREE_GB (default 8) free.
# The estimate is the data directory plus the database, both uncompressed.
require_free_space() {
    local parent="$1" data="$2" min_kb data_kb db_kb free_kb need_kb
    min_kb=$(( ${BACKUP_MIN_FREE_GB:-8} * 1024 * 1024 ))
    data_kb="$(du -sk "$data" | cut -f1)"
    db_kb="$(db_psql -At -c "SELECT pg_database_size(current_database()) / 1024")"
    free_kb="$(df -Pk "$parent" | awk 'NR == 2 { print $4 }')"
    need_kb=$((data_kb + db_kb))
    if [ $((free_kb - need_kb)) -lt "$min_kb" ]; then
        err "not enough disk for a data part: $((free_kb / 1048576))G free, about $((need_kb / 1048576 + 1))G needed,"
        err "and ${BACKUP_MIN_FREE_GB:-8}G must stay free (BACKUP_MIN_FREE_GB). Free space or lower BACKUP_KEEP."
        return 1
    fi
}

# Keep the newest KEEP finished data parts in PARENT; older ones go.
# Only directories this script made (myriad-*, part=data) are counted.
prune_data_parts() {
    local parent="$1" keep="$2" d count=0 drop
    case "$keep" in
        ''|*[!0-9]*|0) err "BACKUP_KEEP must be a whole number of at least 1"; return 1 ;;
    esac
    for d in "$parent"/myriad-*; do
        [ -d "$d" ] || continue
        case "$d" in *.partial) continue ;; esac
        [ "$(manifest_value "$d/MANIFEST.txt" part)" = data ] || continue
        count=$((count + 1))
    done
    drop=$((count - keep))
    for d in "$parent"/myriad-*; do
        [ "$drop" -gt 0 ] || break
        [ -d "$d" ] || continue
        case "$d" in *.partial) continue ;; esac
        [ "$(manifest_value "$d/MANIFEST.txt" part)" = data ] || continue
        info "==> removing old data part $d"
        rm -rf -- "$d"
        drop=$((drop - 1))
    done
}

# backend/data into a gzip tar, without the data key. Top-level names are
# listed one by one: bsdtar's --exclude would also drop a nested .secret-key.
# `..secret-key.tmp-*` is the data key half-written by a starting backend.
archive_local_data() {
    local data="$1" out="$2" p name status
    local names=()
    for p in "$data"/* "$data"/.[!.]* "$data"/..?*; do
        [ -e "$p" ] || [ -L "$p" ] || continue
        name="${p##*/}"
        case "$name" in
            .secret-key|..secret-key.tmp-*) continue ;;
        esac
        names+=("./$name")
    done
    local flags=()
    if tar --version 2>/dev/null | grep -q bsdtar; then
        flags=(--no-mac-metadata --no-xattrs)
    fi
    umask 077
    set +e
    if [ "${#names[@]}" -eq 0 ]; then
        tar -czf "$out" -T /dev/null
    elif [ "${#flags[@]}" -eq 0 ]; then
        COPYFILE_DISABLE=1 tar -czf "$out" -C "$data" "${names[@]}"
    else
        COPYFILE_DISABLE=1 tar "${flags[@]}" -czf "$out" -C "$data" "${names[@]}"
    fi
    status=$?
    set -e
    chmod 600 "$out"
    if [ "$status" -ne 0 ]; then
        # The site keeps running: a file removed or rewritten while it was read
        # makes tar complain. The archive still counts if it is whole.
        if gzip -t "$out" 2>/dev/null && tar -tzf "$out" >/dev/null 2>&1; then
            warn "some files changed while they were archived; the archive is whole"
        else
            err "archiving $data failed"
            return 1
        fi
    fi
}

local_live_port() {
    local port
    port="$(env_file_key "$(deploy_env_file)" SERVER_PORT)"
    printf '%s' "${port:-1103}"
}

# True when something listens on the deploy's port. Without lsof nobody can
# tell, which counts as listening.
port_listening() {
    command -v lsof >/dev/null 2>&1 || return 0
    lsof -nP -iTCP:"$1" -sTCP:LISTEN -t >/dev/null 2>&1
}

do_local_backup() {
    local out="" final parent keys env_file data work stamped=0
    while [ $# -gt 0 ]; do
        case "$1" in
            --out) out="$2"; shift 2 ;;
            --no-stop) shift ;;
            *) err "unknown option: $1"; exit 1 ;;
        esac
    done
    env_file="$(deploy_env_file)"
    data="$(local_data_dir)"
    [ -f "$env_file" ] || { err "$env_file is missing; refuse to backup without deploy secrets"; exit 1; }
    [ -d "$data" ] || { err "$data is missing"; exit 1; }
    if [ -n "$out" ]; then
        final="$(absolute_path "$out")"
    else
        final="$(stamp_dir)"
        stamped=1
    fi
    keys="$(keys_dir)"
    require_keys_outside "$keys" "$final" || exit 1
    parent="$(dirname "$final")"
    mkdir -p "$parent"
    [ "$stamped" -eq 0 ] || chmod 700 "$parent"
    [ ! -e "$final" ] || { err "$final already exists"; exit 1; }

    trap 'code=$?; trap - EXIT; local_cleanup; exit "$code"' EXIT
    local_db_setup "$env_file"
    take_lock "$parent/.backup.lock"
    for work in "$parent"/myriad-*.partial; do
        [ -e "$work" ] && rm -rf -- "$work"
    done
    require_free_space "$parent" "$data"

    work="$final.partial"
    mkdir -m 700 "$work"
    info "==> data part $final (the site keeps running)"

    info "==> PostgreSQL dump (her log's key rows go to the keys part)"
    local exclude=() table
    for table in $HER_LOG_KEY_TABLES; do
        exclude+=("--exclude-table-data=public.$table")
    done
    umask 077
    pg_dump -w -Fc "${exclude[@]}" -d "$LOCAL_DB" -f "$work/postgres.dump"
    chmod 600 "$work/postgres.dump"

    info "==> backend data (the data key goes to the keys part)"
    archive_local_data "$data" "$work/backend_data.tar.gz"

    info "==> keys part"
    write_keys "$keys"

    cat > "$work/MANIFEST.txt" <<EOF
format=2
part=data
deploy=local
created=$(date -u +%Y-%m-%dT%H:%M:%SZ)
project=$(backup_project)
data_key_fingerprint=$(manifest_value "$keys/MANIFEST.txt" data_key_fingerprint)
includes=postgres.dump backend_data.tar.gz
excludes=.env .secret-key $HER_LOG_KEY_TABLES (rows) -> keys part
EOF
    chmod 600 "$work/MANIFEST.txt"
    mv -- "$work" "$final"

    if [ "$stamped" -eq 1 ]; then
        prune_data_parts "$parent" "${BACKUP_KEEP:-3}"
    fi
    ok "backup complete: data part $final, keys part $keys"
}

do_local_keys() {
    local env_file
    env_file="$(deploy_env_file)"
    [ -f "$env_file" ] || { err "$env_file is missing; refuse to write a keys part without deploy secrets"; exit 1; }
    trap 'code=$?; trap - EXIT; local_cleanup; exit "$code"' EXIT
    local_db_setup "$env_file"
    write_keys "$(keys_dir)"
}

# Restoring the live site keeps its database: the keys part's .env must name
# the same DATABASE_URL as the live one. Neither value is ever printed.
require_same_database_url() {
    local live="$1" backup="$2"
    if [ "$(env_file_key "$live" DATABASE_URL)" != "$(env_file_key "$backup" DATABASE_URL)" ]; then
        err "the keys part's .env names another DATABASE_URL than $live; refuse restore"
        err "restore beside the live site with --db and --data-dir, or fix DATABASE_URL first"
        exit 1
    fi
}

dir_identity() {
    if [ -d "$1" ]; then
        (cd "$1" && pwd -P)
    else
        absolute_path "$1"
    fi
}

do_local_restore() {
    local from="" keys="" db="" data_dir="" no_stop=0 beside=0 env_file live_data conn_env target port tables
    while [ $# -gt 0 ]; do
        case "$1" in
            --from) from="$2"; shift 2 ;;
            --keys) keys="$2"; shift 2 ;;
            --db) db="$2"; shift 2 ;;
            --data-dir) data_dir="$2"; shift 2 ;;
            --no-stop) no_stop=1; shift ;;
            *) err "unknown option: $1"; exit 1 ;;
        esac
    done
    [ -n "$from" ] || { err "--from DIR is required"; exit 1; }
    [ -f "$from/postgres.dump" ] || { err "missing $from/postgres.dump"; exit 1; }
    [ -f "$from/backend_data.tar.gz" ] || { err "missing $from/backend_data.tar.gz"; exit 1; }
    if [ "$(manifest_value "$from/MANIFEST.txt" format)" != "2" ] \
        || [ "$(manifest_value "$from/MANIFEST.txt" deploy)" != "local" ]; then
        err "$from is not a data part of a local deploy (BACKUP_MODE=local backup)"
        exit 1
    fi
    if [ -n "$db" ] || [ -n "$data_dir" ]; then
        [ -n "$db" ] && [ -n "$data_dir" ] \
            || { err "a restore beside the live site needs both --db and --data-dir"; exit 1; }
        beside=1
    fi
    if [ -z "$keys" ]; then
        keys="$(keys_dir)"
        if [ ! -f "$keys/MANIFEST.txt" ]; then
            err "no keys part at $keys: the data part holds no data key, .env or her log keys"
            err "give the keys part with --keys DIR (or set BACKUP_KEYS_DIR); without it the"
            err "backend would make a new data key and nothing sealed would open"
            exit 1
        fi
        info "==> keys part: $keys"
    fi
    require_matching_keys "$from" "$keys"

    env_file="$(deploy_env_file)"
    live_data="$(local_data_dir)"
    conn_env="$env_file"
    [ -f "$conn_env" ] || conn_env="$keys/env"
    if [ "$beside" -eq 0 ] && [ -f "$env_file" ]; then
        require_same_database_url "$env_file" "$keys/env"
    fi

    trap 'code=$?; trap - EXIT; local_cleanup; exit "$code"' EXIT
    local_db_setup "$conn_env"
    if [ "$beside" -eq 1 ]; then
        if [ "$db" = "$LOCAL_LIVE_DB" ]; then
            err "--db names the live database; a restore beside the live site needs another one"
            exit 1
        fi
        if [ "$(dir_identity "$data_dir")" = "$(dir_identity "$live_data")" ]; then
            err "--data-dir is the live data directory; a restore beside the live site needs another one"
            exit 1
        fi
        LOCAL_DB="$db"
        tables="$(db_psql -At -c "SELECT count(*) FROM pg_tables WHERE schemaname = 'public'")" \
            || { err "cannot open database $db; create it first, owned by the deploy's role"; exit 1; }
        [ "$tables" = 0 ] || { err "database $db already has tables; restore beside the live site only into an empty one"; exit 1; }
        target="$data_dir"
        mkdir -p "$target"
    else
        port="$(local_live_port)"
        if port_listening "$port"; then
            if [ "$no_stop" -eq 0 ]; then
                err "something listens on port $port: stop the backend and its workers first"
                err "(this script never stops a local deploy; --no-stop restores anyway)"
                exit 1
            fi
            warn "port $port is in use; restore may race with live writes"
        fi
        target="$live_data"
    fi

    info "==> restoring PostgreSQL into $LOCAL_DB"
    pg_restore -w --single-transaction --exit-on-error --clean --if-exists -d "$LOCAL_DB" "$from/postgres.dump"
    info "==> restoring her log's key rows"
    restore_her_log_keys "$keys"

    info "==> restoring backend data into $target"
    restore_data_tree "$target" "$from/backend_data.tar.gz"
    if [ "$(manifest_value "$keys/MANIFEST.txt" data_key_source)" = "file" ]; then
        info "==> restoring the data key"
        (
            umask 077
            cat "$keys/data-key" > "$target/.secret-key.restore"
            mv -- "$target/.secret-key.restore" "$target/.secret-key"
        )
    fi

    if [ "$beside" -eq 0 ]; then
        info "==> restoring .env (previous file kept as .env.bak.restore)"
        umask 077
        if [ -f "$env_file" ]; then
            cp "$env_file" "$env_file.bak.restore"
            chmod 600 "$env_file.bak.restore"
        fi
        cp "$keys/env" "$env_file"
        chmod 600 "$env_file"
        ok "restore complete from $from; start the backend again"
    else
        ok "restore complete from $from into database $db and $target; the live .env was left alone"
    fi
}

# launchd jobs for a local deploy on macOS: the data part daily, the keys part
# hourly. Only these two plists are written; unschedule removes exactly them.
SCHEDULE_DATA_LABEL="com.myriad-you.backup-data"
SCHEDULE_KEYS_LABEL="com.myriad-you.backup-keys"

launch_agents_dir() {
    printf '%s' "${LAUNCH_AGENTS_DIR:-$HOME/Library/LaunchAgents}"
}

xml_text() {
    local s="${1//&/&amp;}"
    s="${s//</&lt;}"
    printf '%s' "${s//>/&gt;}"
}

# One job's plist on stdout. $1 label, $2 command (backup|keys), $3 timing XML.
schedule_plist() {
    local label="$1" command="$2" timing="$3" logs pgbin
    logs="${BACKUP_LOG_DIR:-$HOME/Backups/myriad/logs}"
    pgbin="$(dirname "$(type -P pg_dump)")"
    cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>$(xml_text "$label")</string>
    <key>ProgramArguments</key>
    <array>
        <string>/bin/bash</string>
        <string>$(xml_text "$ROOT/scripts/extra/backup.sh")</string>
        <string>$(xml_text "$command")</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>BACKUP_MODE</key><string>local</string>
        <key>PATH</key><string>$(xml_text "$pgbin"):/usr/bin:/bin:/usr/sbin:/sbin</string>
        <key>LOCAL_BACKEND_DIR</key><string>$(xml_text "$(local_backend_dir)")</string>
        <key>BACKUP_OUT</key><string>$(xml_text "${BACKUP_OUT:-$HOME/Backups/myriad/data}")</string>
        <key>BACKUP_KEYS_DIR</key><string>$(xml_text "$(keys_dir)")</string>
        <key>BACKUP_KEEP</key><string>$(xml_text "${BACKUP_KEEP:-3}")</string>
    </dict>
$timing
    <key>ProcessType</key><string>Background</string>
    <key>LowPriorityIO</key><true/>
    <key>Nice</key><integer>10</integer>
    <key>StandardOutPath</key><string>$(xml_text "$logs/$command.log")</string>
    <key>StandardErrorPath</key><string>$(xml_text "$logs/$command.log")</string>
</dict>
</plist>
EOF
}

do_local_schedule() {
    local agents logs uid label file
    command -v launchctl >/dev/null 2>&1 || { err "schedule needs macOS launchd"; exit 1; }
    type -P pg_dump >/dev/null 2>&1 || { err "pg_dump is not on PATH"; exit 1; }
    case "${BACKUP_KEEP:-3}" in ''|*[!0-9]*|0) err "BACKUP_KEEP must be a whole number of at least 1"; exit 1 ;; esac
    agents="$(launch_agents_dir)"
    logs="${BACKUP_LOG_DIR:-$HOME/Backups/myriad/logs}"
    uid="$(id -u)"
    mkdir -p "$agents" "$logs"
    chmod 700 "$logs"
    schedule_plist "$SCHEDULE_DATA_LABEL" backup '    <key>StartCalendarInterval</key>
    <dict><key>Hour</key><integer>4</integer><key>Minute</key><integer>30</integer></dict>' \
        > "$agents/$SCHEDULE_DATA_LABEL.plist"
    schedule_plist "$SCHEDULE_KEYS_LABEL" keys '    <key>StartInterval</key><integer>3600</integer>' \
        > "$agents/$SCHEDULE_KEYS_LABEL.plist"
    for label in "$SCHEDULE_DATA_LABEL" "$SCHEDULE_KEYS_LABEL"; do
        file="$agents/$label.plist"
        plutil -lint "$file" >/dev/null || { err "$file is not a valid plist"; exit 1; }
        launchctl bootout "gui/$uid/$label" >/dev/null 2>&1 || true
        launchctl bootstrap "gui/$uid" "$file"
        ok "scheduled $label ($file)"
    done
    info "data part daily at 04:30, keys part hourly; logs in $logs"
}

do_local_unschedule() {
    local agents uid label
    agents="$(launch_agents_dir)"
    uid="$(id -u)"
    for label in "$SCHEDULE_DATA_LABEL" "$SCHEDULE_KEYS_LABEL"; do
        launchctl bootout "gui/$uid/$label" >/dev/null 2>&1 || true
        rm -f -- "$agents/$label.plist"
        ok "unscheduled $label"
    done
    info "backups already made are left where they are"
}

show_usage() {
    cat <<EOF
Usage: $0 <backup|keys|restore|schedule|unschedule> [options]

  backup [--out DIR] [--no-stop]
      Data part into DIR: Postgres without her log's key rows, and backend
      data selected by Compose without the data key. Keys part into
      BACKUP_KEYS_DIR (default ./backup-keys), replacing the copy there:
      .env, the data key, her log's key rows.
      Stops backend / federation-worker / persona-worker briefly unless
      --no-stop is set. Excludes cache.
      If this script stopped a running writer, EXIT restarts it and
      keeps the original exit code.

  keys
      Rewrite only the keys part, without stopping anything. Optional:
      every backup rewrites it anyway. Running it in between (after
      forgetting, or from cron) only makes forgetting reach the backups
      sooner: an object forgotten since the last rewrite still has its key
      in the keys part until then.

  restore --from DIR [--keys KEYS] [--no-stop]
      Restore Postgres, backend_data, and .env from a data part and the
      keys part. Destructive. Stops writers unless --no-stop is set.
      Without --keys the keys part is taken from BACKUP_KEYS_DIR (default
      ./backup-keys), where backup put it. Refused when there is no
      matching keys part: without it the backend would make a new data key
      and nothing sealed would open again.
      A backup made before the parts were split restores without --keys.
      Supported scope: same compose project and the same
      POSTGRES_PASSWORD as the running role. A different password or
      compose project is refused before any stop, copy, or restore.
      Order: match password, project and keys → pin project → probe data
      volume → stop writers → restore .env → restore Postgres → her log's
      key rows → restore volume → data key → recreate backend → wait /ready
      → recreate workers.
      A failed restore leaves stopped writers down.

Local deploy (BACKUP_MODE=local): the backend runs as a plain process on
this host, reading LOCAL_BACKEND_DIR/.env and LOCAL_BACKEND_DIR/data
(default ./backend). The database is the one DATABASE_URL names; pg_dump,
pg_restore and psql must be on PATH. Nothing is ever stopped or started.

  backup [--out DIR]
      Data part into BACKUP_OUT/myriad-<time> (default
      ~/Backups/myriad/data), written as <dir>.partial and renamed when
      whole. Keys part into BACKUP_KEYS_DIR (default ~/Backups/myriad/keys).
      Without --out, only the newest BACKUP_KEEP data parts (default 3) are
      kept. Refused when less than BACKUP_MIN_FREE_GB (default 8) would be
      left free. The database is consistent through pg_dump's snapshot;
      files changed while they are archived are taken as they are.

  keys
      Rewrite only the keys part.

  restore --from DIR [--keys KEYS] [--db NAME --data-dir DIR] [--no-stop]
      Without --db/--data-dir: restore the live database, backend/data
      and backend/.env. Refused while something listens on SERVER_PORT
      (stop the backend and its workers first) unless --no-stop is set.
      With --db and --data-dir: restore beside the live site, into an
      existing empty database on the same server and a directory, to
      check a backup. The live .env, database and data are left alone.
      The keys part is found as above when --keys is not given.

  schedule / unschedule   (macOS)
      Install or remove two launchd jobs in ~/Library/LaunchAgents:
      com.myriad-you.backup-data (backup, daily at 04:30) and
      com.myriad-you.backup-keys (keys, hourly), with the paths, PATH and
      BACKUP_KEEP of the shell that runs it. Logs go to BACKUP_LOG_DIR
      (default ~/Backups/myriad/logs). unschedule leaves backups in place.

Notes:
  - Keep many data parts if you like; keep only the newest keys part. An
    older keys part still opens what was forgotten since.
  - Updater file-level ./pgdata snapshots are rollback material, not this backup.
  - Secrets are copied as files; this script never prints them.
  - cache / backend_cache is omitted on purpose (regenerable).
EOF
}

stamp_dir() {
    local dest
    if is_local; then
        dest="${BACKUP_OUT:-$HOME/Backups/myriad/data}"
    else
        dest="${BACKUP_OUT:-$ROOT/backups}"
    fi
    mkdir -p "$dest"
    printf '%s/myriad-%s' "$dest" "$(date +%Y%m%d_%H%M%S)"
}

restart_writers_keep_status() {
    local code="$1"
    local svc
    if [ "${WRITERS_STOPPED_BY_US:-0}" -eq 1 ]; then
        for svc in $WRITER_START_ORDER; do
            case " $STOPPED_WRITERS " in
                *" $svc "*)
                    info "==> starting $svc"
                    if ! compose start "$svc"; then
                        err "could not start $svc; start it with scripts/extra/deploy.sh"
                        [ "$code" -ne 0 ] || code=1
                    fi
                    ;;
            esac
        done
    fi
    return "$code"
}

do_backup() {
    local out="" no_stop=0 keys
    if is_local; then do_local_backup "$@"; return; fi
    while [ $# -gt 0 ]; do
        case "$1" in
            --out) out="$2"; shift 2 ;;
            --no-stop) no_stop=1; shift ;;
            *) err "unknown option: $1"; exit 1 ;;
        esac
    done
    [ -f .env ] || { err ".env is missing; refuse to backup without deploy secrets"; exit 1; }
    [ -n "$out" ] || out="$(stamp_dir)"
    keys="$(keys_dir)"
    require_keys_outside "$keys" "$out" || exit 1
    mkdir -p "$out"
    chmod 700 "$out"

    WRITERS_STOPPED_BY_US=0
    STOPPED_WRITERS=""
    trap 'code=$?; trap - EXIT; release_locks; restart_writers_keep_status "$code"; exit $?' EXIT

    load_backup_storage || return 1
    info "==> backup directory $out"
    if [ "$no_stop" -eq 0 ]; then
        quiesce_writers
    else
        warn "writers left running; dump may be crash-consistent only"
    fi

    info "==> PostgreSQL dump (her log's key rows go to the keys part)"
    local exclude=() table
    for table in $HER_LOG_KEY_TABLES; do
        exclude+=("--exclude-table-data=public.$table")
    done
    compose exec -T postgres pg_dump -U myriad -d myriad -Fc "${exclude[@]}" > "$out/postgres.dump"
    chmod 600 "$out/postgres.dump"

    info "==> backend data (the data key goes to the keys part)"
    # Let the host operator create the archive. A root container writing through
    # /out would leave a root-owned file that non-root Linux operators cannot chmod.
    # `..secret-key.tmp-*` is the data key half-written by a starting backend.
    umask 077
    "$DOCKER" run --rm \
        --mount "$BACKEND_DATA_MOUNT,dst=/data,readonly" \
        alpine:3.20 \
        tar czf - -C /data --exclude=./.secret-key --exclude='./..secret-key.tmp-*' . \
        > "$out/backend_data.tar.gz"
    chmod 600 "$out/backend_data.tar.gz"

    info "==> keys part"
    write_keys "$keys"

    cat > "$out/MANIFEST.txt" <<EOF
format=2
part=data
created=$(date -u +%Y-%m-%dT%H:%M:%SZ)
project=$(project_name)
data_key_fingerprint=$(manifest_value "$keys/MANIFEST.txt" data_key_fingerprint)
includes=postgres.dump backend_data.tar.gz
excludes=.env .secret-key $HER_LOG_KEY_TABLES (rows) -> keys part; backend_cache; ./pgdata (updater rollback only)
EOF
    chmod 600 "$out/MANIFEST.txt"

    ok "backup complete: data part $out, keys part $keys"
}

do_keys() {
    if is_local; then do_local_keys "$@"; return; fi
    [ $# -eq 0 ] || { err "keys takes no options; set BACKUP_KEYS_DIR to move it"; exit 1; }
    [ -f .env ] || { err ".env is missing; refuse to write a keys part without deploy secrets"; exit 1; }
    load_backup_storage || return 1
    trap 'code=$?; trap - EXIT; release_locks; exit "$code"' EXIT
    write_keys "$(keys_dir)"
}

do_restore() {
    local from="" keys="" no_stop=0 split=0
    if is_local; then do_local_restore "$@"; return; fi
    while [ $# -gt 0 ]; do
        case "$1" in
            --from) from="$2"; shift 2 ;;
            --keys) keys="$2"; shift 2 ;;
            --no-stop) no_stop=1; shift ;;
            *) err "unknown option: $1"; exit 1 ;;
        esac
    done
    [ -n "$from" ] || { err "--from DIR is required"; exit 1; }
    [ -f "$from/postgres.dump" ] || { err "missing $from/postgres.dump"; exit 1; }
    [ -f "$from/backend_data.tar.gz" ] || { err "missing $from/backend_data.tar.gz"; exit 1; }
    if [ "$(manifest_value "$from/MANIFEST.txt" format)" = "2" ]; then
        split=1
        if [ -z "$keys" ]; then
            keys="$(keys_dir)"
            if [ ! -f "$keys/MANIFEST.txt" ]; then
                err "no keys part at $keys: the data part holds no data key, .env or her log keys"
                err "give the keys part with --keys DIR (or set BACKUP_KEYS_DIR); without it the"
                err "backend would make a new data key and nothing sealed would open"
                exit 1
            fi
            info "==> keys part: $keys"
        fi
        require_matching_keys "$from" "$keys"
        require_same_postgres_password "$keys/env"
        require_same_compose_project "$from/MANIFEST.txt" "$keys/env"
    else
        [ -z "$keys" ] || { err "$from was made before backups were split and carries its own keys; drop --keys"; exit 1; }
        [ -f "$from/env" ] || { err "missing $from/env"; exit 1; }
        warn "$from was made before backups were split: it carries the data key and her log keys"
        require_same_postgres_password "$from/env"
        require_same_compose_project "$from/MANIFEST.txt" "$from/env"
    fi

    preflight_restore_storage || return 1

    WRITERS_STOPPED_BY_US=0
    STOPPED_WRITERS=""
    trap 'code=$?; trap - EXIT; if [ "$code" -ne 0 ] && [ "${WRITERS_STOPPED_BY_US:-0}" -eq 1 ]; then err "restore failed; writers left stopped so writes stay quiesced"; fi; exit "$code"' EXIT

    if [ "$no_stop" -eq 0 ]; then
        quiesce_writers
    else
        warn "writers left running; restore may race with live writes"
    fi

    info "==> restoring .env (previous file kept as .env.bak.restore)"
    if [ -f .env ]; then
        cp .env ".env.bak.restore"
        chmod 600 ".env.bak.restore"
    fi
    umask 077
    if [ "$split" -eq 1 ]; then
        cp "$keys/env" .env
    else
        cp "$from/env" .env
    fi
    chmod 600 .env

    info "==> restoring PostgreSQL"
    compose exec -T postgres pg_restore -U myriad -d myriad --clean --if-exists < "$from/postgres.dump"
    if [ "$split" -eq 1 ]; then
        info "==> restoring her log's key rows"
        restore_her_log_keys "$keys"
    fi

    info "==> restoring backend data"
    "$DOCKER" run --rm \
        -e DEST=/data \
        -e ARCHIVE=/in/backend_data.tar.gz \
        -e RESTORE_OWNER=1000:1000 \
        --mount "$BACKEND_DATA_MOUNT,dst=/data" \
        -v "$from:/in:ro" \
        alpine:3.20 \
        sh -c "$(volume_restore_posix)"
    if [ "$split" -eq 1 ]; then
        info "==> restoring the data key"
        restore_data_key_file "$keys"
    fi

    # Recreate writers after .env is in place so JWT / DATABASE_URL match the files.
    # Postgres stays up: the dump was applied to the running cluster.
    if [ "$no_stop" -eq 0 ]; then
        info "==> recreating backend with restored environment"
        compose up -d --force-recreate --no-deps backend
        wait_backend_ready
        info "==> recreating federation-worker and persona-worker"
        compose up -d --force-recreate --no-deps federation-worker persona-worker
        WRITERS_STOPPED_BY_US=0
    fi
    ok "restore complete from $from"
}

if [ "${BASH_SOURCE[0]}" != "$0" ]; then
    return 0
fi

case "$COMMAND" in
    backup) do_backup "$@" ;;
    keys) do_keys "$@" ;;
    restore) do_restore "$@" ;;
    schedule|unschedule)
        is_local || { err "$COMMAND is for a local deploy: BACKUP_MODE=local"; exit 1; }
        [ $# -eq 0 ] || { err "$COMMAND takes no options"; exit 1; }
        "do_local_$COMMAND" "$@"
        ;;
    help|-h|--help|"") show_usage ;;
    *) err "unknown command: $COMMAND"; show_usage; exit 1 ;;
esac
