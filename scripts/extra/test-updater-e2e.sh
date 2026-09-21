#!/usr/bin/env bash
# =============================================================================
# Myriad updater + proxy end-to-end smoke test
# =============================================================================
# Spins up the release binaries against a self-contained testbed in /tmp.
# Verifies:
#   1. proxy /healthz, /_proxy/status
#   2. maintenance.json switch -> proxy serves the maintenance HTML
#   3. docker guard permits a real project-scoped Compose lifecycle, but rejects exec
#      and hostile host binds
#   4. updater /healthz; /status requires token and returns the expected schema
#   5. /admin/self-update (direct mode) returns 401 without token
#   6. updater rescue CLI status
#
# Does NOT verify the full update flow (would require a working release.json on
# GitHub + a real docker stack). For that, run a real `docker compose up -d` and
# trigger an update from the admin UI.
# =============================================================================

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TESTBED="${TESTBED:-/tmp/myriad-e2e}"
# Prefer CARGO_TARGET_DIR when set (shared cache builds), else package-local target/.
CARGO_TARGET="${CARGO_TARGET_DIR:-}"
if [ -z "$CARGO_TARGET" ]; then
  if [ -x "$ROOT/updater/target/debug/myriad-updater" ]; then
    CARGO_TARGET="$ROOT/updater/target"
  elif [ -x "${HOME}/.cache/cargo-targets/debug/myriad-updater" ]; then
    CARGO_TARGET="${HOME}/.cache/cargo-targets"
  else
    CARGO_TARGET="$ROOT/updater/target"
  fi
fi
PROXY_TARGET="${PROXY_TARGET_DIR:-}"
if [ -z "$PROXY_TARGET" ]; then
  if [ -x "$ROOT/proxy/target/release/myriad-proxy" ]; then
    PROXY_TARGET="$ROOT/proxy/target"
  elif [ -x "${CARGO_TARGET}/release/myriad-proxy" ]; then
    PROXY_TARGET="$CARGO_TARGET"
  else
    PROXY_TARGET="$ROOT/proxy/target"
  fi
fi
UPDATER_BIN="${UPDATER_BIN:-$CARGO_TARGET/debug/myriad-updater}"
RESCUE_BIN="${RESCUE_BIN:-$CARGO_TARGET/debug/myriad-rescue}"
PROXY_BIN="${PROXY_BIN:-$PROXY_TARGET/release/myriad-proxy}"
DOCKER_GUARD_IMAGE="${DOCKER_GUARD_E2E_IMAGE:-myriad-updater-dev:e2e}"
DOCKER_GUARD_CONTAINER="myriad-e2e-docker-guard-${DOCKER_GUARD_PORT:-19375}"
UPDATER_PORT="${UPDATER_PORT:-19090}"
PROXY_PORT="${PROXY_PORT:-18080}"
DOCKER_GUARD_PORT="${DOCKER_GUARD_PORT:-19375}"
E2E_DOCKER_NETWORK="myriad-e2e-guard-${DOCKER_GUARD_PORT}"
ORIGINAL_DOCKER_HOST="${DOCKER_HOST:-}"
export E2E_DOCKER_NETWORK
# 32+ chars, no dictionary words (avoids weak-token check)
TOKEN="9xQ3vN8mP2rT5wY7zA1bC4dF6hJ8kL0n"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
ok()   { echo -e "${GREEN}✓ $1${NC}"; }
fail() { echo -e "${RED}✗ $1${NC}"; exit 1; }
info() { echo -e "${CYAN}== $1${NC}"; }
warn() { echo -e "${YELLOW}! $1${NC}"; }

UPDATER_PID=""
PROXY_PID=""
DOCKER_GUARD_PID=""
E2E_NETWORK_CREATED=""
host_docker() {
    if [ -n "$ORIGINAL_DOCKER_HOST" ]; then
        DOCKER_HOST="$ORIGINAL_DOCKER_HOST" docker "$@"
    else
        env -u DOCKER_HOST docker "$@"
    fi
}
cleanup() {
    set +e
    if [ -n "${DOCKER_HOST:-}" ] && [ -f "$TESTBED/compose.yaml" ]; then
        docker compose -p myriad-e2e -f "$TESTBED/compose.yaml" \
            --project-directory "$TESTBED" --env-file "$TESTBED/.env" \
            rm -sf backend updater >/dev/null 2>&1
    fi
    [ -n "$UPDATER_PID" ] && kill "$UPDATER_PID" 2>/dev/null
    [ -n "$PROXY_PID" ] && kill "$PROXY_PID" 2>/dev/null
    [ -n "$DOCKER_GUARD_PID" ] && kill "$DOCKER_GUARD_PID" 2>/dev/null
    host_docker rm -f "$DOCKER_GUARD_CONTAINER" >/dev/null 2>&1 || true
    [ -n "$E2E_NETWORK_CREATED" ] && host_docker network rm "$E2E_DOCKER_NETWORK" >/dev/null 2>&1
    # Wait briefly for processes to exit
    wait 2>/dev/null
}
trap cleanup EXIT

# Pre-flight: binaries must exist
[ -x "$UPDATER_BIN" ] || fail "debug updater binary missing: $UPDATER_BIN (run: cd updater && cargo build --bins)"
[ -x "$RESCUE_BIN" ]  || fail "rescue binary missing: $RESCUE_BIN"
[ -x "$PROXY_BIN" ]   || fail "proxy binary missing: $PROXY_BIN (run: cd proxy && cargo build --release)"

# Guard's production identity check deliberately cannot run a host binary. Build
# an explicit debug-only container: release builds reject the unpinned dev tag.
info "Building debug-only docker guard image $DOCKER_GUARD_IMAGE"
host_docker build --build-arg CARGO_PROFILE=dev -t "$DOCKER_GUARD_IMAGE" -f updater/Dockerfile . >/dev/null

info "Preparing testbed at $TESTBED"
rm -rf "$TESTBED"
mkdir -p "$TESTBED"/{state,pgdata,backups,guard-policy}

# Minimal compose file referencing the required tag variables (probe needs this).
cat > "$TESTBED/guard-policy/docker-guard.env" <<EOF
DOCKER_GUARD_IMAGE=$DOCKER_GUARD_IMAGE
GUARD_COMPOSE_PROJECT_NAME=myriad-e2e
GUARD_MYRIAD_DOCKER_NETWORK=$E2E_DOCKER_NETWORK
GUARD_MYRIAD_ADMIN_NETWORK=myriad-admin-net
GUARD_MYRIAD_DOCKER_GUARD_NETWORK=myriad-e2e-guard
MYRIAD_GUARD_ENV_FILE=guard-policy/docker-guard.env
EOF

cat > "$TESTBED/compose.yaml" <<'YML'
services:
  postgres:
    image: postgres:18-alpine
  backend:
    image: postgres:18-alpine
  frontend:
    image: docker.io/somekawahitomi/myriad-frontend:${MYRIAD_TAG}
  proxy:
    image: docker.io/somekawahitomi/myriad-proxy:${PROXY_TAG}
  updater:
    image: postgres:18-alpine
    network_mode: none
    volumes:
      - ./:/host/compose:ro
      - ./.env:/host/compose/.env:rw
      - ./state:/host/compose/state:rw
      - ./pgdata:/host/compose/pgdata:rw
      - ./guard-policy:/run/secrets:ro
networks:
  default:
    name: ${E2E_DOCKER_NETWORK}
    external: true
YML

cat > "$TESTBED/.env" <<EOF
MYRIAD_TAG=v0.0.0-e2e
PROXY_TAG=v0.0.0-e2e
UPDATER_TAG=v0.0.0-e2e
COMPOSE_PROJECT_NAME=myriad-e2e
POSTGRES_PASSWORD=e2etestpassword12345678901234567890
PERSONA_DB_PASSWORD=testpersonaDbPassword12345678901234567890
FEDERATION_DB_PASSWORD=testfederationDbPassword12345678901234567890
JWT_SECRET=e2etestjwtsecret12345678901234567890
CORS_ORIGINS=http://localhost
EOF

# Updater env (for `from_env` config loader)
export UPDATE_TOKEN="$TOKEN"
export CHANNEL="stable"
export MYRIAD_GITHUB_REPO="Myriad-You/Myriad"
export CHECK_INTERVAL_SECS=0   # disable periodic ticker for the test
export COMPOSE_PROJECT_NAME="myriad-e2e"

# ============================================================================
# Start Docker guard. The updater below talks only to this TCP endpoint.
# ============================================================================
if ! host_docker network inspect "$E2E_DOCKER_NETWORK" >/dev/null 2>&1; then
    host_docker network create "$E2E_DOCKER_NETWORK" >/dev/null
    E2E_NETWORK_CREATED=1
fi
info "Starting docker guard container on :$DOCKER_GUARD_PORT"
host_docker run --rm --name "$DOCKER_GUARD_CONTAINER" --network host \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v "$TESTBED:$TESTBED:ro" \
  -v "$TESTBED/guard-policy:/guard-policy" \
  -e DOCKER_GUARD_LISTEN="127.0.0.1:$DOCKER_GUARD_PORT" \
  -e DOCKER_GUARD_HOST_COMPOSE_ROOT="$TESTBED" \
  -e DOCKER_GUARD_COMPOSE_DIR="$TESTBED" \
  -e DOCKER_GUARD_EXPECTED_IMAGE="$DOCKER_GUARD_IMAGE" \
  -e DOCKER_GUARD_ALLOW_UNPINNED_DEV=true \
  -e DOCKER_GUARD_HOST_POLICY_PATH=/guard-policy/docker-guard.env \
  -e DOCKER_GUARD_SELF_UPDATE_TOKEN="$TOKEN" \
  -e COMPOSE_PROJECT_NAME="myriad-e2e" \
  -e MYRIAD_DOCKER_NETWORK="$E2E_DOCKER_NETWORK" \
  -e MYRIAD_DOCKER_GUARD_NETWORK="myriad-e2e-guard" \
  --entrypoint /usr/local/bin/myriad-docker-guard \
  "$DOCKER_GUARD_IMAGE" >"$TESTBED/docker-guard.log" 2>&1 &
DOCKER_GUARD_PID=$!
export DOCKER_HOST="tcp://127.0.0.1:$DOCKER_GUARD_PORT"
export UPDATER_DEBUG_GUARDED_DOCKER_HOST="$DOCKER_HOST"

# pgdata: stub a layout that satisfies the probe (it just needs a directory)
mkdir -p "$TESTBED/pgdata/PG_VERSION_STUB"

# ============================================================================
# Start proxy first (so we can probe maintenance behavior independently)
# ============================================================================
info "Starting proxy on :$PROXY_PORT"
PROXY_LISTEN="0.0.0.0:$PROXY_PORT" \
PROXY_STATE_FILE="$TESTBED/state/maintenance.json" \
PROXY_BACKEND_UPSTREAM="http://127.0.0.1:65500" \
PROXY_FRONTEND_UPSTREAM="http://127.0.0.1:65501" \
PROXY_UPDATER_UPSTREAM="http://127.0.0.1:$UPDATER_PORT" \
PROXY_ALLOW_DIRECT_UPDATER=true \
  "$PROXY_BIN" >"$TESTBED/proxy.log" 2>&1 &
PROXY_PID=$!
sleep 1

# ============================================================================
# Start updater
# ============================================================================
info "Starting updater on :$UPDATER_PORT"
UPDATER_GUARD_ENV_FILE="$TESTBED/guard-policy/docker-guard.env" \
"$UPDATER_BIN" \
  --state-dir "$TESTBED/state" \
  --compose-dir "$TESTBED" \
  --env-file "$TESTBED/.env" \
  --pgdata "$TESTBED/pgdata" \
  --listen "0.0.0.0:$UPDATER_PORT" \
  >"$TESTBED/updater.log" 2>&1 &
UPDATER_PID=$!

# Wait for HTTP readiness
info "Waiting for services to be ready"
for i in $(seq 1 30); do
    if curl -fsS -o /dev/null "http://127.0.0.1:$PROXY_PORT/healthz" 2>/dev/null \
       && curl -fsS -o /dev/null "http://127.0.0.1:$DOCKER_GUARD_PORT/_ping" 2>/dev/null \
       && curl -fsS -o /dev/null "http://127.0.0.1:$UPDATER_PORT/healthz" 2>/dev/null; then
        break
    fi
    if [ "$i" = "30" ]; then
        echo "--- proxy.log ---"; tail -30 "$TESTBED/proxy.log"
        echo "--- updater.log ---"; tail -30 "$TESTBED/updater.log"
        echo "--- docker-guard.log ---"; tail -30 "$TESTBED/docker-guard.log"
        fail "services did not become ready in 30s"
    fi
    sleep 0.3
done

# ============================================================================
# Tests
# ============================================================================
PASS=0
FAIL=0
run_check() {
    local label="$1"
    shift
    if "$@"; then
        ok "$label"; PASS=$((PASS+1))
    else
        warn "$label FAILED"; FAIL=$((FAIL+1))
    fi
}

# 1. proxy /healthz
check_proxy_health() {
    local code
    code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PROXY_PORT/healthz")
    [ "$code" = "200" ]
}
run_check "proxy /healthz returns 200" check_proxy_health

# 2. proxy /_proxy/status (no maintenance)
check_proxy_status_inactive() {
    local body active
    body=$(curl -fsS "http://127.0.0.1:$PROXY_PORT/_proxy/status")
    active=$(echo "$body" | jq -r '.maintenance.active')
    [ "$active" = "false" ]
}
run_check "proxy /_proxy/status reports inactive maintenance" check_proxy_status_inactive

# 3. Docker guard policy: a real Compose lifecycle works, while exec and a hostile bind do not.
check_docker_guard_policy() {
    local ping exec_code bind_code symlink_code propagation_code compose_rc
    ping=$(curl -fsS "http://127.0.0.1:$DOCKER_GUARD_PORT/_ping")
    [ "$ping" = "OK" ] || return 1
    exec_code=$(curl -s -o /dev/null -w '%{http_code}' \
        -X POST "http://127.0.0.1:$DOCKER_GUARD_PORT/v1.51/containers/myriad-backend/exec" \
        -H 'Content-Type: application/json' -d '{"Cmd":["id"]}')
    [ "$exec_code" = "403" ] || return 1
    bind_code=$(curl -s -o /dev/null -w '%{http_code}' \
        -X POST "http://127.0.0.1:$DOCKER_GUARD_PORT/v1.51/containers/create?name=host-breakout" \
        -H 'Content-Type: application/json' \
        -d '{"Image":"docker.io/somekawahitomi/myriad-backend:v1","Labels":{"com.docker.compose.project":"myriad-e2e","com.docker.compose.service":"backend"},"HostConfig":{"Binds":["/:/host:rw"]}}')
    [ "$bind_code" = "403" ] || return 1

    mv "$TESTBED/pgdata" "$TESTBED/pgdata.real"
    ln -s / "$TESTBED/pgdata"
    symlink_code=$(curl -s -o /dev/null -w '%{http_code}' \
        -X POST "http://127.0.0.1:$DOCKER_GUARD_PORT/v1.51/containers/create?name=symlink-breakout" \
        -H 'Content-Type: application/json' \
        -d "{\"Image\":\"postgres:18-alpine\",\"Labels\":{\"com.docker.compose.project\":\"myriad-e2e\",\"com.docker.compose.service\":\"postgres\"},\"HostConfig\":{\"Binds\":[\"$TESTBED/pgdata:/var/lib/postgresql:rw\"]}}")
    unlink "$TESTBED/pgdata"
    mv "$TESTBED/pgdata.real" "$TESTBED/pgdata"
    [ "$symlink_code" = "403" ] || return 1

    propagation_code=$(curl -s -o /dev/null -w '%{http_code}' \
        -X POST "http://127.0.0.1:$DOCKER_GUARD_PORT/v1.51/containers/create?name=propagation-breakout" \
        -H 'Content-Type: application/json' \
        -d "{\"Image\":\"docker.io/somekawahitomi/myriad-updater:v1\",\"Labels\":{\"com.docker.compose.project\":\"myriad-e2e\",\"com.docker.compose.service\":\"updater\"},\"HostConfig\":{\"Binds\":[\"$TESTBED:/host/compose:rw,rshared\"]}}")
    [ "$propagation_code" = "403" ] || return 1

    set +e
    docker compose -p myriad-e2e -f "$TESTBED/compose.yaml" \
        --project-directory "$TESTBED" --env-file "$TESTBED/.env" \
        up -d --no-deps backend updater \
        >"$TESTBED/guard-compose.log" 2>&1
    compose_rc=$?
    docker compose -p myriad-e2e -f "$TESTBED/compose.yaml" \
        --project-directory "$TESTBED" --env-file "$TESTBED/.env" \
        rm -sf backend updater \
        >>"$TESTBED/guard-compose.log" 2>&1
    set -e
    [ "$compose_rc" -eq 0 ]
}
run_check "docker guard allows project lifecycle and denies exec/bind/symlink/propagation" check_docker_guard_policy

# 4. updater /healthz
check_updater_health() {
    local code body
    body=$(curl -fsS "http://127.0.0.1:$UPDATER_PORT/healthz")
    [ "$(echo "$body" | jq -r '.ok')" = "true" ]
}
run_check "updater /healthz returns ok" check_updater_health

# 5. updater /status schema
check_updater_status() {
    local code body
    code=$(curl -s -o /dev/null -w '%{http_code}' \
        "http://127.0.0.1:$UPDATER_PORT/status")
    [ "$code" = "401" ] || return 1
    body=$(curl -fsS \
        -H "X-Update-Token: $TOKEN" \
        "http://127.0.0.1:$UPDATER_PORT/status")
    # Must contain required fields per spec §13
    echo "$body" | jq -e '
        .schema_version == 1
        and (.updater_version | type) == "string"
        and (.channel | type) == "string"
        and (.maintenance_active | type) == "boolean"
        and (.maintenance_phase | type) == "string"
        and (.update_available | type) == "boolean"
        and (.requires_self_update | type) == "boolean"
        and (
          (has("last_failed_update") | not)
          or .last_failed_update == null
          or (.last_failed_update | type) == "object"
        )
        and (
          (has("pgdata_snapshot_enabled") | not)
          or (.pgdata_snapshot_enabled | type) == "boolean"
        )
    ' >/dev/null
}
run_check "updater /status requires token and returns spec-conformant shape" check_updater_status

# 5b. last_failed_update surfaces on /status when present in updater.json
check_last_failed_update_status() {
    cat > "$TESTBED/state/updater.json" <<'JSON'
{
  "schema_version": 1,
  "current_version": "v0.0.0-e2e",
  "channel": "stable",
  "update_mode": "release",
  "auto_install": false,
  "last_failed_update": {
    "from_version": "v0.0.0-e2e",
    "to_version": "v9.9.9",
    "at": "2026-01-01T00:00:00Z",
    "reason": "e2e simulated failure after auto-rollback",
    "job_id": "e2e-failed-job"
  }
}
JSON
    # Give the running daemon a moment if it caches (it re-reads each request).
    local body
    body=$(curl -fsS -H "X-Update-Token: $TOKEN" "http://127.0.0.1:$UPDATER_PORT/status")
    echo "$body" | jq -e '
        .last_failed_update.job_id == "e2e-failed-job"
        and (.last_failed_update.reason | type) == "string"
        and (.last_failed_update.to_version | tostring | length) > 0
    ' >/dev/null
}
run_check "updater /status exposes last_failed_update when recorded" check_last_failed_update_status

# 6. updater /update without token returns 401
check_update_no_token() {
    local code
    code=$(curl -s -o /dev/null -w '%{http_code}' \
        -X POST "http://127.0.0.1:$UPDATER_PORT/update" \
        -H 'Content-Type: application/json' \
        -d '{"target_version":"v1.0.0"}')
    [ "$code" = "401" ]
}
run_check "POST /update without token returns 401" check_update_no_token

# 7. updater /update with token but invalid version → 400
check_update_bad_version() {
    local code
    code=$(curl -s -o /dev/null -w '%{http_code}' \
        -X POST "http://127.0.0.1:$UPDATER_PORT/update" \
        -H 'Content-Type: application/json' \
        -H "X-Update-Token: $TOKEN" \
        -d '{"target_version":"not-a-version"}')
    [ "$code" = "400" ]
}
run_check "POST /update with bad version returns 400" check_update_bad_version

# 8. proxy /_updater/* forwards when ALLOW_DIRECT_UPDATER=true
check_proxy_forwards_updater() {
    local body
    body=$(curl -fsS "http://127.0.0.1:$PROXY_PORT/_updater/healthz")
    [ "$(echo "$body" | jq -r '.ok')" = "true" ]
}
run_check "proxy /_updater/healthz forwards to updater" check_proxy_forwards_updater

# 9. Maintenance mode switch: write maintenance.json, proxy must serve maintenance page
check_maintenance_engages() {
    cat > "$TESTBED/state/maintenance.json" <<JSON
{
  "schema_version": 1,
  "active": true,
  "phase": "stopping",
  "from_version": "v0.3.37",
  "to_version": "v0.4.0",
  "started_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "updated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "job_id": "e2e-job",
  "message_key": "updater.phase.stopping"
}
JSON
    sleep 0.3
    local code body
    code=$(curl -s -o /tmp/myriad-e2e-maintresp -w '%{http_code}' "http://127.0.0.1:$PROXY_PORT/")
    body=$(cat /tmp/myriad-e2e-maintresp)
    [ "$code" = "503" ] && echo "$body" | grep -q 'Myriad' && echo "$body" | grep -q 'maintenance'
}
run_check "maintenance mode serves 503 + maintenance HTML" check_maintenance_engages

# 10. maintenance OFF: clear file, proxy should fail open (forward, then get connection refused
#    because there is no real backend on 65500 — expecting 502)
check_maintenance_clears() {
    rm -f "$TESTBED/state/maintenance.json"
    sleep 0.3
    local code
    code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PROXY_PORT/")
    # 502 means proxy forwarded normally but upstream is unreachable (expected in this testbed)
    [ "$code" = "502" ]
}
run_check "maintenance OFF: proxy forwards (502 from missing upstream)" check_maintenance_clears

# 11. rescue CLI status
check_rescue_status() {
    "$RESCUE_BIN" --state-dir "$TESTBED/state" --pgdata "$TESTBED/pgdata" \
        --env-file "$TESTBED/.env" --compose-dir "$TESTBED" \
        status >/dev/null 2>&1
}
run_check "myriad-rescue status succeeds against testbed" check_rescue_status

# 12. Legacy recovery with no prepared deployment and a missing snapshot cannot restore.
check_recovery_health_probe_lifted() {
    # Stop running updater so we can rewrite state and re-run recover_or_idle on start.
    if [ -n "$UPDATER_PID" ]; then
        kill "$UPDATER_PID" 2>/dev/null || true
        wait "$UPDATER_PID" 2>/dev/null || true
        UPDATER_PID=""
    fi

    cat > "$TESTBED/state/job.e2e-probe.json" <<'JSON'
{
  "id": "e2e-probe",
  "kind": "update",
  "created_at": "2026-01-01T00:00:00Z",
  "finished_at": null,
  "from_version": "v0.0.0-e2e",
  "to_version": "v9.9.9",
  "snapshot_id": "snap-e2e-probe",
  "status": "running",
  "steps": [
    {
      "phase": "health_probing",
      "started_at": "2026-01-01T00:00:00Z",
      "finished_at": null,
      "ok": null,
      "log_tail": "",
      "error": null
    }
  ],
  "idempotency_key": null
}
JSON
    echo -n "e2e-probe" > "$TESTBED/state/job.current"
    cat > "$TESTBED/state/maintenance.json" <<'JSON'
{
  "schema_version": 1,
  "active": false,
  "phase": "health_probing",
  "from_version": "v0.0.0-e2e",
  "to_version": "v9.9.9",
  "started_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-01T00:00:00Z",
  "job_id": "e2e-probe",
  "message_key": "updater.phase.health_probing_live"
}
JSON

    # Restart updater — recover_or_idle runs at boot.
    "$UPDATER_BIN" \
        --state-dir "$TESTBED/state" \
        --compose-dir "$TESTBED" \
        --env-file "$TESTBED/.env" \
        --pgdata "$TESTBED/pgdata" \
        --listen "127.0.0.1:$UPDATER_PORT" \
        >"$TESTBED/updater-recovery.log" 2>&1 &
    UPDATER_PID=$!

    local ready=0 i
    for i in $(seq 1 60); do
        if curl -fsS "http://127.0.0.1:$UPDATER_PORT/healthz" >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.25
    done
    [ "$ready" = "1" ] || return 1

    # Recovery runs in the background; wait for the missing-snapshot failure.
    for i in $(seq 1 120); do
        jq -e '.status == "needs_manual"' "$TESTBED/state/job.e2e-probe.json" >/dev/null && break
        sleep 0.25
    done
    local body phase active
    body=$(curl -fsS -H "X-Update-Token: $TOKEN" "http://127.0.0.1:$UPDATER_PORT/status")
    phase=$(echo "$body" | jq -r '.maintenance_phase')
    active=$(echo "$body" | jq -r '.maintenance_active')
    [ "$phase" = "needs_manual" ] || {
        echo "expected needs_manual phase, got: $body" >&2
        return 1
    }
    [ "$active" = "true" ] || {
        echo "expected maintenance_active=true, got: $body" >&2
        return 1
    }
    # job file should be needs_manual
    jq -e '.status == "needs_manual"' "$TESTBED/state/job.e2e-probe.json" >/dev/null
}
run_check "legacy recovery: missing snapshot retains maintenance" check_recovery_health_probe_lifted

# 13. Crash recovery: pre-swap stopping → clear + failed job (stack restore best-effort)
check_recovery_pre_swap_clear() {
    if [ -n "$UPDATER_PID" ]; then
        kill "$UPDATER_PID" 2>/dev/null || true
        wait "$UPDATER_PID" 2>/dev/null || true
        UPDATER_PID=""
    fi

    cat > "$TESTBED/state/job.e2e-pre.json" <<'JSON'
{
  "id": "e2e-pre",
  "kind": "update",
  "created_at": "2026-01-01T00:00:00Z",
  "finished_at": null,
  "from_version": "v0.0.0-e2e",
  "to_version": "v9.9.9",
  "snapshot_id": null,
  "status": "running",
  "steps": [
    {
      "phase": "stopping",
      "started_at": "2026-01-01T00:00:00Z",
      "finished_at": null,
      "ok": null,
      "log_tail": "",
      "error": null
    }
  ],
  "idempotency_key": null
}
JSON
    echo -n "e2e-pre" > "$TESTBED/state/job.current"
    cat > "$TESTBED/state/maintenance.json" <<'JSON'
{
  "schema_version": 1,
  "active": true,
  "phase": "stopping",
  "from_version": "v0.0.0-e2e",
  "to_version": "v9.9.9",
  "started_at": "2026-01-01T00:00:00Z",
  "updated_at": "2026-01-01T00:00:00Z",
  "job_id": "e2e-pre",
  "message_key": "updater.phase.stopping"
}
JSON

    "$UPDATER_BIN" \
        --state-dir "$TESTBED/state" \
        --compose-dir "$TESTBED" \
        --env-file "$TESTBED/.env" \
        --pgdata "$TESTBED/pgdata" \
        --listen "127.0.0.1:$UPDATER_PORT" \
        >"$TESTBED/updater-recovery-pre.log" 2>&1 &
    UPDATER_PID=$!

    local ready=0 i
    for i in $(seq 1 60); do
        if curl -fsS "http://127.0.0.1:$UPDATER_PORT/healthz" >/dev/null 2>&1; then
            ready=1
            break
        fi
        sleep 0.25
    done
    [ "$ready" = "1" ] || return 1

    local body active
    body=$(curl -fsS -H "X-Update-Token: $TOKEN" "http://127.0.0.1:$UPDATER_PORT/status")
    active=$(echo "$body" | jq -r '.maintenance_active')
    [ "$active" = "false" ] || {
        echo "expected maintenance cleared after pre-swap recovery, got: $body" >&2
        return 1
    }
    jq -e '.status == "failed"' "$TESTBED/state/job.e2e-pre.json" >/dev/null
    [ ! -f "$TESTBED/state/job.current" ] || [ ! -s "$TESTBED/state/job.current" ]
}
run_check "crash recovery: pre-swap stopping → clear maintenance + failed job" check_recovery_pre_swap_clear

# ============================================================================
# Summary
# ============================================================================
echo ""
if [ "$FAIL" -eq 0 ]; then
    ok "All $PASS checks passed"
    echo ""
    info "Testbed left at $TESTBED for inspection"
    info "Logs:"
    info "  $TESTBED/proxy.log"
    info "  $TESTBED/updater.log"
    info "  $TESTBED/docker-guard.log"
    exit 0
else
    fail "$FAIL/$((PASS+FAIL)) checks failed (see logs in $TESTBED/*.log)"
fi
