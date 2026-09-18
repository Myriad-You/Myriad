#!/usr/bin/env bash
# Site disaster backup / restore: PostgreSQL + backend_data + .env.
# Does not include regenerable cache volumes. Updater ./pgdata snapshots are
# not a substitute for this backup.
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
ok()   { echo -e "${GREEN}$1${NC}"; }
info() { echo -e "${CYAN}$1${NC}"; }
warn() { echo -e "${YELLOW}$1${NC}"; }
err()  { echo -e "${RED}$1${NC}"; }

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
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

data_volume() {
    printf '%s_backend_data' "$(active_project)"
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

ensure_data_volume() {
    local volume="$1"
    if ! $DOCKER volume inspect "$volume" >/dev/null 2>&1; then
        info "==> creating empty volume $volume"
        $DOCKER volume create "$volume" >/dev/null
    fi
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
    local backup_env="$1"
    local backup_dir backup live
    live="$(project_name)"
    backup_dir="$(dirname "$backup_env")"
    backup=""
    if [ -f "$backup_dir/MANIFEST.txt" ]; then
        backup="$(grep -E '^project=' "$backup_dir/MANIFEST.txt" 2>/dev/null | head -1 | cut -d= -f2-)"
    fi
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
Usage: $0 <backup|restore> [options]

  backup [--out DIR] [--no-stop]
      Dump Postgres, archive the backend_data volume, and copy .env.
      Stops backend / federation-worker / persona-worker briefly unless
      --no-stop is set. Excludes cache.
      If this script stopped a running writer, EXIT restarts it and
      keeps the original exit code.

  restore --from DIR [--no-stop]
      Restore Postgres, backend_data, and .env from a backup directory.
      Destructive. Stops writers unless --no-stop is set.
      Supported scope: same compose project and the same
      POSTGRES_PASSWORD as the running role. A different password or
      compose project is refused before any stop, copy, or restore.
      Order: match password and project → pin project → stop writers →
      restore .env → restore Postgres → restore volume → recreate
      backend → wait /ready → recreate workers.
      A failed restore leaves stopped writers down.

Notes:
  - Updater file-level ./pgdata snapshots are rollback material, not this backup.
  - Secrets are copied as files; this script never prints .env values.
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
    local out="" no_stop=0
    while [ $# -gt 0 ]; do
        case "$1" in
            --out) out="$2"; shift 2 ;;
            --no-stop) no_stop=1; shift ;;
            *) err "unknown option: $1"; exit 1 ;;
        esac
    done
    [ -f .env ] || { err ".env is missing; refuse to backup without deploy secrets"; exit 1; }
    [ -n "$out" ] || out="$(stamp_dir)"
    mkdir -p "$out"
    chmod 700 "$out"

    WRITERS_STOPPED_BY_US=0
    STOPPED_WRITERS=""
    trap 'code=$?; trap - EXIT; restart_writers_keep_status "$code"; exit $?' EXIT

    info "==> backup directory $out"
    if [ "$no_stop" -eq 0 ]; then
        quiesce_writers
    else
        warn "writers left running; dump may be crash-consistent only"
    fi

    info "==> PostgreSQL dump"
    compose exec -T postgres pg_dump -U myriad -d myriad -Fc > "$out/postgres.dump"
    chmod 600 "$out/postgres.dump"

    local volume
    volume="$(data_volume)"
    info "==> backend_data volume ($volume)"
    $DOCKER run --rm \
        -v "${volume}:/data:ro" \
        -v "$out:/out" \
        alpine:3.20 \
        tar czf /out/backend_data.tar.gz -C /data .
    chmod 600 "$out/backend_data.tar.gz"

    umask 077
    cp .env "$out/env"
    chmod 600 "$out/env"

    cat > "$out/MANIFEST.txt" <<EOF
created=$(date -u +%Y-%m-%dT%H:%M:%SZ)
project=$(project_name)
includes=postgres.dump backend_data.tar.gz env
excludes=backend_cache ./pgdata (updater rollback only)
EOF
    chmod 600 "$out/MANIFEST.txt"

    ok "backup complete: $out"
}

do_restore() {
    local from="" no_stop=0
    while [ $# -gt 0 ]; do
        case "$1" in
            --from) from="$2"; shift 2 ;;
            --no-stop) no_stop=1; shift ;;
            *) err "unknown option: $1"; exit 1 ;;
        esac
    done
    [ -n "$from" ] || { err "--from DIR is required"; exit 1; }
    [ -f "$from/postgres.dump" ] || { err "missing $from/postgres.dump"; exit 1; }
    [ -f "$from/backend_data.tar.gz" ] || { err "missing $from/backend_data.tar.gz"; exit 1; }
    [ -f "$from/env" ] || { err "missing $from/env"; exit 1; }

    require_same_postgres_password "$from/env"
    require_same_compose_project "$from/env"

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
    cp "$from/env" .env
    chmod 600 .env

    info "==> restoring PostgreSQL"
    compose exec -T postgres pg_restore -U myriad -d myriad --clean --if-exists < "$from/postgres.dump"

    local volume
    volume="$(data_volume)"
    ensure_data_volume "$volume"
    info "==> restoring backend_data ($volume)"
    $DOCKER run --rm \
        -e DEST=/data \
        -e ARCHIVE=/in/backend_data.tar.gz \
        -v "${volume}:/data" \
        -v "$from:/in:ro" \
        alpine:3.20 \
        sh -c "$(volume_restore_posix)"

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
    restore) do_restore "$@" ;;
    help|-h|--help|"") show_usage ;;
    *) err "unknown command: $COMMAND"; show_usage; exit 1 ;;
esac
