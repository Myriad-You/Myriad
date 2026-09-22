#!/usr/bin/env bash
# Boot a disposable Postgres + this-process backend, then run API smoke.
# Never inherits DATABASE_URL. External DB must be MYRIAD_SMOKE_DATABASE_URL.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

SMOKE_DIR="${MYRIAD_SMOKE_DIR:-$ROOT/.tmp/smoke}"
PG_NAME="${MYRIAD_SMOKE_PG_NAME:-myriad-smoke-pg}"
PG_PORT="${MYRIAD_SMOKE_PG_PORT:-55432}"
BACKEND_PORT="${MYRIAD_SMOKE_BACKEND_PORT:-18103}"
SETUP_SECRET="${MYRIAD_SETUP_SECRET:-smoke-setup-secret}"
JWT_SECRET="${JWT_SECRET:-smoke-jwt-key-abcdefghijklmnopqrstuvwxyz012}"

mkdir -p "$SMOKE_DIR/data" "$SMOKE_DIR/cache"

port_in_use() {
    local port="$1"
    if command -v lsof >/dev/null 2>&1; then
        [ -n "$(lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null || true)" ]
        return
    fi
    if command -v ss >/dev/null 2>&1; then
        ss -ltn | grep -qE ":${port}\\s"
        return
    fi
    (echo >/dev/tcp/127.0.0.1/"$port") >/dev/null 2>&1
}

listen_pids() {
    local port="$1"
    if command -v lsof >/dev/null 2>&1; then
        lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null || true
    fi
}

backend_bin() {
    if [ -n "${CARGO_TARGET_DIR:-}" ]; then
        printf '%s/debug/myriad-backend' "$CARGO_TARGET_DIR"
    else
        printf '%s/target/debug/myriad-backend' "$ROOT"
    fi
}

cleanup() {
    if [ -n "${BACKEND_PID:-}" ]; then
        kill "$BACKEND_PID" 2>/dev/null || true
        wait "$BACKEND_PID" 2>/dev/null || true
    fi
    if [ "${MYRIAD_SMOKE_KEEP_PG:-}" != "1" ] && [ "${STARTED_PG:-}" = "1" ]; then
        docker rm -f "$PG_NAME" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

if port_in_use "$BACKEND_PORT"; then
    echo "smoke: port ${BACKEND_PORT} is already in use; refuse to reuse it" >&2
    exit 1
fi

if [ -n "${MYRIAD_SMOKE_DATABASE_URL:-}" ]; then
    export DATABASE_URL="$MYRIAD_SMOKE_DATABASE_URL"
elif [ -n "${DATABASE_URL:-}" ]; then
    echo "smoke: refusing inherited DATABASE_URL; set MYRIAD_SMOKE_DATABASE_URL" >&2
    exit 1
else
    if ! docker info >/dev/null 2>&1; then
        echo "smoke: docker is required to start a temporary Postgres" >&2
        exit 2
    fi
    docker rm -f "$PG_NAME" >/dev/null 2>&1 || true
    docker run -d --name "$PG_NAME" \
        -e POSTGRES_DB=myriad_smoke \
        -e POSTGRES_USER=myriad \
        -e POSTGRES_PASSWORD=myriad_smoke \
        -p "${PG_PORT}:5432" \
        postgres:18-alpine >/dev/null
    STARTED_PG=1
    for _ in $(seq 1 40); do
        if docker exec "$PG_NAME" pg_isready -U myriad -d myriad_smoke >/dev/null 2>&1; then
            break
        fi
        sleep 1
    done
    export DATABASE_URL="postgres://myriad:myriad_smoke@127.0.0.1:${PG_PORT}/myriad_smoke"
fi

export ENVIRONMENT="${ENVIRONMENT:-development}"
export MYRIAD_PROCESS_ROLE=all
export JWT_SECRET
export MYRIAD_SETUP_SECRET="$SETUP_SECRET"
export DATA_DIR="$SMOKE_DIR/data"
export CACHE_DIR="$SMOKE_DIR/cache"
export SERVER_PORT="$BACKEND_PORT"
export RUST_LOG="${RUST_LOG:-warn}"

cargo build -p myriad-backend --locked --quiet
BIN="$(backend_bin)"
if [ ! -x "$BIN" ]; then
    echo "smoke: missing backend binary $BIN" >&2
    exit 1
fi
"$BIN" &
BACKEND_PID=$!

HEALTH=""
for _ in $(seq 1 90); do
    if HEALTH="$(curl -fsS "http://127.0.0.1:${BACKEND_PORT}/health" 2>/dev/null)"; then
        break
    fi
    if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
        echo "smoke: backend exited before /health" >&2
        exit 1
    fi
    sleep 1
done
if [ -z "$HEALTH" ]; then
    echo "smoke: /health did not become ready" >&2
    exit 1
fi

if ! printf '%s' "$HEALTH" | grep -q 'myriad-backend'; then
    echo "smoke: /health is not this backend: $HEALTH" >&2
    exit 1
fi

LISTEN="$(listen_pids "$BACKEND_PORT")"
if ! printf '%s\n' "$LISTEN" | grep -qx "$BACKEND_PID"; then
    echo "smoke: /health listener is not this process (pid=$BACKEND_PID listen=${LISTEN:-none})" >&2
    exit 1
fi

export MYRIAD_SMOKE_BASE_URL="http://127.0.0.1:${BACKEND_PORT}"
export MYRIAD_SETUP_SECRET="$SETUP_SECRET"
cd frontend
pnpm exec playwright test --config playwright.smoke.config.ts "$@"
