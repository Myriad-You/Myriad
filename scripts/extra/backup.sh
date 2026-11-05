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
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
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
    printf '%s' "${BACKUP_KEYS_DIR:-$ROOT/backup-keys}"
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

table_exists() {
    [ "$(compose exec -T postgres psql -U myriad -d myriad -At -v ON_ERROR_STOP=1 \
        -c "SELECT to_regclass('public.$1') IS NOT NULL")" = "t" ]
}

# Write the keys part into a fresh sibling directory, then swap it in, so a
# failure leaves the previous copy whole. Every older copy left by an earlier
# crash is removed: it may hold keys forgotten since.
write_keys() {
    local dir="$1" next old table stale key_from rows=""
    dir="$(absolute_path "$dir")"
    mkdir -p "$(dirname "$dir")"
    for stale in "$dir".next.*; do
        [ -e "$stale" ] && rm -rf -- "$stale"
    done
    next="$dir.next.$$"
    umask 077
    mkdir -m 700 "$next"

    cp .env "$next/env"
    chmod 600 "$next/env"

    # The official compose reads the data key from backend_data/.secret-key.
    # MYRIAD_DATA_KEY reaches the backend only through a compose override; .env
    # goes into the keys part whole, so its value is kept either way. It names
    # the data key here only when there is no file.
    "$DOCKER" run --rm --network none \
        --mount "$BACKEND_DATA_MOUNT,dst=/data,readonly" \
        alpine:3.20 \
        sh -c 'if [ -f /data/.secret-key ]; then cat /data/.secret-key; fi' > "$next/data-key"
    if [ -s "$next/data-key" ]; then
        key_from=file
    elif grep -Eq '^MYRIAD_DATA_KEY=.+' .env; then
        key_from=env
        env_file_value MYRIAD_DATA_KEY > "$next/data-key"
    else
        key_from=none
        warn "no data key yet (backend never started?); the keys part has none"
    fi
    chmod 600 "$next/data-key"

    for table in $HER_LOG_KEY_TABLES; do
        : > "$next/$table.copy"
        if table_exists "$table"; then
            compose exec -T postgres psql -U myriad -d myriad -v ON_ERROR_STOP=1 \
                -c "COPY $table ($(her_log_key_columns "$table")) TO STDOUT" > "$next/$table.copy"
        fi
        chmod 600 "$next/$table.copy"
        rows="${rows:+$rows }$table=$(wc -l < "$next/$table.copy" | tr -d ' ')"
    done

    cat > "$next/MANIFEST.txt" <<EOF
format=2
part=keys
created=$(date -u +%Y-%m-%dT%H:%M:%SZ)
project=$(project_name)
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
        } | compose exec -T postgres psql -U myriad -d myriad -q -v ON_ERROR_STOP=1 >/dev/null
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

show_usage() {
    cat <<EOF
Usage: $0 <backup|keys|restore> [options]

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

Notes:
  - Keep many data parts if you like; keep only the newest keys part. An
    older keys part still opens what was forgotten since.
  - Updater file-level ./pgdata snapshots are rollback material, not this backup.
  - Secrets are copied as files; this script never prints them.
  - cache / backend_cache is omitted on purpose (regenerable).
EOF
}

stamp_dir() {
    local dest="${BACKUP_OUT:-$ROOT/backups}"
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
    trap 'code=$?; trap - EXIT; restart_writers_keep_status "$code"; exit $?' EXIT

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
    [ $# -eq 0 ] || { err "keys takes no options; set BACKUP_KEYS_DIR to move it"; exit 1; }
    [ -f .env ] || { err ".env is missing; refuse to write a keys part without deploy secrets"; exit 1; }
    load_backup_storage || return 1
    write_keys "$(keys_dir)"
}

do_restore() {
    local from="" keys="" no_stop=0 split=0
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
    help|-h|--help|"") show_usage ;;
    *) err "unknown command: $COMMAND"; show_usage; exit 1 ;;
esac
