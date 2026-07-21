#!/usr/bin/env bash
# Dual-instance local federation intranet full-chain harness.
#
# Spins two real myriad-backend processes with separate Postgres DBs, lab
# private outbound enabled, cold-key identity → Follow A→B → delivery delivered.
#
# Usage:
#   ./scripts/dev/federation-dual-intranet.sh [once|twice|down]
# Env:
#   SCRATCH_DIR  evidence output (default: ./tmp/federation-dual)
#   KEEP_RUNNING leave backends up after success (1) / always stop (0, default)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BACKEND="$ROOT/backend"
SCRATCH_DIR="${SCRATCH_DIR:-$ROOT/tmp/federation-dual}"
MODE="${1:-twice}"
KEEP_RUNNING="${KEEP_RUNNING:-0}"

PORT_A="${PORT_A:-18080}"
PORT_B="${PORT_B:-18081}"
BASE_A="http://127.0.0.1:${PORT_A}"
BASE_B="http://127.0.0.1:${PORT_B}"
DB_A="${DB_A:-myriad_fed_a}"
DB_B="${DB_B:-myriad_fed_b}"
PG_HOST="${PG_HOST:-127.0.0.1}"
PG_PORT="${PG_PORT:-5432}"
PG_USER="${PG_USER:-myriad}"
PG_PASS="${PG_PASS:-myriad_dev_password}"
# Must not contain substrings like "secret"/"changeme" (config::validate_jwt_secret).
JWT_A="federation-lab-jwt-aaaaaaaaaaaaaaaaaaaaaa-A1"
JWT_B="federation-lab-jwt-bbbbbbbbbbbbbbbbbbbbbb-B2"
ADMIN_USER_A="alice"
ADMIN_USER_B="bob"
ADMIN_PASS="Testpass1"
PG_CONTAINER="${PG_CONTAINER:-myriad-postgres-dev}"

# Respect shared CARGO_TARGET_DIR (this machine uses ~/.cache/cargo-targets)
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(
  (cd "$BACKEND" && cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c 'import sys,json; print(json.load(sys.stdin).get("target_directory",""))' 2>/dev/null) \
  || true
)}"
if [[ -z "${CARGO_TARGET_DIR}" ]]; then
  CARGO_TARGET_DIR="${BACKEND}/target"
fi
BIN="${CARGO_TARGET_DIR}/debug/myriad-backend"
PID_A=""
PID_B=""
RUN_LOG_A=""
RUN_LOG_B=""

mkdir -p "$SCRATCH_DIR"
log() { echo "[fed-dual] $*"; }
die() { echo "[fed-dual] ERROR: $*" >&2; exit 1; }

cleanup() {
  if [[ "$KEEP_RUNNING" == "1" ]]; then
    log "KEEP_RUNNING=1 — leaving backends up (PIDs $PID_A $PID_B)"
    return 0
  fi
  [[ -n "$PID_A" ]] && kill "$PID_A" 2>/dev/null || true
  [[ -n "$PID_B" ]] && kill "$PID_B" 2>/dev/null || true
  wait "$PID_A" 2>/dev/null || true
  wait "$PID_B" 2>/dev/null || true
}
trap cleanup EXIT

psql_docker() {
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -h localhost -v ON_ERROR_STOP=1 "$@"
}

ensure_dbs() {
  log "Ensuring databases $DB_A and $DB_B on $PG_CONTAINER"
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d postgres -tc "SELECT 1 FROM pg_database WHERE datname='${DB_A}'" \
    | grep -q 1 || docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
      psql -U "$PG_USER" -d postgres -c "CREATE DATABASE ${DB_A};"
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d postgres -tc "SELECT 1 FROM pg_database WHERE datname='${DB_B}'" \
    | grep -q 1 || docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
      psql -U "$PG_USER" -d postgres -c "CREATE DATABASE ${DB_B};"
}

reset_dbs() {
  log "Resetting federation lab DBs (drop+create)"
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d postgres -c \
    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname IN ('${DB_A}','${DB_B}') AND pid <> pg_backend_pid();" \
    >/dev/null 2>&1 || true
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d postgres -c "DROP DATABASE IF EXISTS ${DB_A};" >/dev/null
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d postgres -c "DROP DATABASE IF EXISTS ${DB_B};" >/dev/null
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d postgres -c "CREATE DATABASE ${DB_A};" >/dev/null
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d postgres -c "CREATE DATABASE ${DB_B};" >/dev/null
}

build_backend() {
  log "Building myriad-backend (debug) target_dir=$CARGO_TARGET_DIR"
  (cd "$BACKEND" && cargo build --bin myriad-backend) 2>&1 | tee "$SCRATCH_DIR/build.log" | tail -8
  # Re-resolve after build in case metadata was empty pre-build
  if [[ ! -x "$BIN" ]]; then
    BIN="$(cd "$BACKEND" && cargo metadata --format-version 1 --no-deps \
      | python3 -c 'import sys,json; print(json.load(sys.stdin)["target_directory"])')/debug/myriad-backend"
  fi
  [[ -x "$BIN" ]] || die "binary missing at $BIN"
  log "Using binary $BIN"
}

start_instance() {
  local name="$1" port="$2" db="$3" jwt="$4" base="$5" logfile="$6"
  # Log to stderr so stdout is only the PID (for capture).
  log "Starting instance $name on $base db=$db" >&2
  (
    export RUST_LOG="${RUST_LOG:-info,myriad_backend::federation=debug}"
    export SERVER_HOST=127.0.0.1
    export SERVER_PORT="$port"
    export DATABASE_URL="postgres://${PG_USER}:${PG_PASS}@${PG_HOST}:${PG_PORT}/${db}"
    export JWT_SECRET="$jwt"
    export BASE_URL="$base"
    export FRONTEND_URL="$base"
    export CORS_ORIGINS="$base"
    export MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND=1
    export MYRIAD_FEDERATION_DELIVERY_INTERVAL_SECS=2
    export MYRIAD_DB_MODE=external
    export CONFIG_MODE=false
    exec "$BIN"
  ) >"$logfile" 2>&1 &
  echo $!
}

wait_health() {
  local base="$1" label="$2" tries=60
  for i in $(seq 1 "$tries"); do
    if curl -fsS "$base/api/setup/status" >/dev/null 2>&1; then
      log "$label healthy at $base"
      curl -fsS "$base/api/setup/status" | tee "$SCRATCH_DIR/health-${label}.json"
      echo
      return 0
    fi
    sleep 1
  done
  die "$label never became healthy at $base (see log)"
}

create_admin() {
  local base="$1" user="$2"
  log "create-admin $user @ $base"
  curl -fsS -X POST "$base/api/setup/create-admin" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"${user}\",\"password\":\"${ADMIN_PASS}\"}" \
    | tee "$SCRATCH_DIR/create-admin-${user}.json"
  echo
}

login_cookie() {
  local base="$1" user="$2" jar="$3"
  curl -fsS -c "$jar" -X POST "$base/api/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"${user}\",\"password\":\"${ADMIN_PASS}\"}" >/dev/null
  [[ -s "$jar" ]] || die "empty cookie jar for $user"
}

# Fetch CSRF token for state-changing API calls (cookie session).
csrf_token() {
  local base="$1" jar="$2"
  curl -fsS -b "$jar" "$base/api/csrf-token" \
    | python3 -c 'import sys,json; t=json.load(sys.stdin).get("csrf_token"); assert t, "null csrf"; print(t)'
}

sql_a() {
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d "$DB_A" -v ON_ERROR_STOP=1 -t -A -c "$1"
}
sql_b() {
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d "$DB_B" -v ON_ERROR_STOP=1 -t -A -c "$1"
}

assert_no_key_error() {
  local logf="$1"
  if grep -E "No federation keys found for user|Failed to load keypair for user" "$logf" >/dev/null; then
    # After ensure, late recoveries may briefly log ensure path; hard-fail only on
    # the original load failure without subsequent ensure success.
    if grep -E "No federation keys found for user" "$logf" >/dev/null \
      && ! grep -E "Ensured federation keys; retrying delivery sign" "$logf" >/dev/null; then
      die "key-load error present in $logf without ensure recovery"
    fi
  fi
}

run_full_chain() {
  # tag: log suffix; mode: "cold" (wipe keys, prove identity creates) | "warm" (keep PEM, prove no rotate + redeliver)
  local tag="$1"
  local mode="${2:-cold}"
  local chain_log="$SCRATCH_DIR/full-chain${tag}.log"
  local jar_a="$SCRATCH_DIR/cookies-a${tag}.jar"
  local jar_b="$SCRATCH_DIR/cookies-b${tag}.jar"
  : >"$chain_log"
  # Log to chain file and stdout; fail if the chain body fails (pipefail + PIPESTATUS).
  {
    echo "=== full-chain${tag} mode=${mode} start $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="
    echo "BASE_A=$BASE_A BASE_B=$BASE_B"

    PEM_BEFORE=""
    if [[ "$mode" == "cold" ]]; then
      # Cold keys: wipe federation_keys on A; also drop B's cached remote actors for A
      # so a later cold re-key would not 401 on stale pubkey (not used in warm path).
      sql_a "DELETE FROM federation_keys;" || true
      sql_b "DELETE FROM federation_remote_actors WHERE actor_url LIKE '%/users/${ADMIN_USER_A}%';" || true
      KEYS_BEFORE=$(sql_a "SELECT count(*) FROM federation_keys;")
      echo "keys_before_a=$KEYS_BEFORE"
      [[ "$KEYS_BEFORE" == "0" ]] || die "expected 0 keys before identity; got $KEYS_BEFORE"
    else
      KEYS_BEFORE=$(sql_a "SELECT count(*) FROM federation_keys WHERE user_id=1;")
      PEM_BEFORE=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
      echo "keys_before_a=$KEYS_BEFORE (warm — keep existing PEM)"
      [[ "$KEYS_BEFORE" == "1" ]] || die "warm mode expects existing key row"
      [[ -n "$PEM_BEFORE" ]] || die "warm mode missing PEM"
    fi

    login_cookie "$BASE_A" "$ADMIN_USER_A" "$jar_a"
    login_cookie "$BASE_B" "$ADMIN_USER_B" "$jar_b"

    # Identity ensures keys without Actor GET
    echo "--- GET /api/federation/identity on A ---"
    curl -fsS -b "$jar_a" "$BASE_A/api/federation/identity" | tee "$SCRATCH_DIR/identity-a${tag}.json"
    echo
    KEYS_AFTER_ID=$(sql_a "SELECT count(*) FROM federation_keys WHERE user_id=1;")
    PEM_AFTER=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
    PEM_PREFIX=$(sql_a "SELECT left(public_key_pem,30) FROM federation_keys WHERE user_id=1;")
    echo "keys_after_identity_a=$KEYS_AFTER_ID pem_prefix=$PEM_PREFIX"
    [[ "$KEYS_AFTER_ID" == "1" ]] || die "identity did not create federation_keys"
    [[ "$PEM_PREFIX" == *"BEGIN PUBLIC KEY"* ]] || die "unexpected PEM"
    if [[ "$mode" == "warm" ]]; then
      if [[ "$PEM_AFTER" != "$PEM_BEFORE" ]]; then
        die "warm ensure rotated PEM (must never rotate live keys)"
      fi
      echo "warm_no_rotate_ok=1"
    fi

    # Seed B actor via public fetch (also ensures B keys when A fetches B's actor)
    echo "--- GET actor B from host (public doc) ---"
    curl -fsS -H 'Accept: application/activity+json' \
      "$BASE_B/users/${ADMIN_USER_B}" | tee "$SCRATCH_DIR/actor-b${tag}.json" | head -c 400
    echo

    # Outbound Follow A → B (requires signed delivery)
    TARGET="${BASE_B}/users/${ADMIN_USER_B}"
    CSRF=$(csrf_token "$BASE_A" "$jar_a")
    echo "--- POST follow $TARGET (csrf ok) ---"
    FOLLOW_RESP=$(curl -fsS -b "$jar_a" -X POST "$BASE_A/api/federation/follow" \
      -H 'Content-Type: application/json' \
      -H "X-CSRF-Token: ${CSRF}" \
      -d "{\"target\":\"${TARGET}\"}")
    echo "$FOLLOW_RESP" | tee "$SCRATCH_DIR/follow${tag}.json"
    echo

    # Wait for delivery worker (lab poll 2s)
    echo "--- waiting for delivery queue delivered ---"
    sql_a "UPDATE federation_delivery_queue SET next_retry_at = NOW() - interval '1 second' WHERE status IN ('pending','delivering');" >/dev/null 2>&1 || true
    local ok=0
    local DELIVERED=0 DEAD=0 PENDING=0 STAT=""
    for i in $(seq 1 45); do
      STAT=$(sql_a "SELECT coalesce(string_agg(status || ':' || cnt::text, ','), '') FROM (SELECT status, count(*)::int AS cnt FROM federation_delivery_queue GROUP BY status) s;" 2>/dev/null || echo "sql_err")
      DELIVERED=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" 2>/dev/null || echo 0)
      DEAD=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead';" 2>/dev/null || echo 0)
      PENDING=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" 2>/dev/null || echo 0)
      echo "t=${i} queue_a=$STAT delivered=$DELIVERED dead=$DEAD pending=$PENDING"
      if [[ "${DELIVERED:-0}" -ge 1 ]]; then
        ok=1
        break
      fi
      if [[ "${DEAD:-0}" -ge 1 && "${PENDING:-0}" -eq 0 ]]; then
        ERR=$(sql_a "SELECT error_message FROM federation_delivery_queue WHERE status='dead' ORDER BY id DESC LIMIT 1;" 2>/dev/null || echo unknown)
        die "delivery dead: $ERR"
      fi
      # Re-nudge pending rows every few ticks (defensive against clock/DEFAULT race)
      if (( i % 5 == 0 )); then
        sql_a "UPDATE federation_delivery_queue SET next_retry_at = NOW() - interval '1 second' WHERE status='pending';" >/dev/null 2>&1 || true
      fi
      sleep 2
    done
    [[ "$ok" == "1" ]] || die "delivery never reached delivered (pending=$PENDING dead=$DEAD)"

    echo "delivered_count=$DELIVERED"
    # B should have recorded incoming follow (accepted)
    FOLLOWS_B=$(sql_b "SELECT count(*) FROM federation_follows WHERE status='accepted';" 2>/dev/null || echo 0)
    ACTS_B=$(sql_b "SELECT count(*) FROM federation_activities;" 2>/dev/null || echo 0)
    echo "b_follows_accepted=$FOLLOWS_B b_activities=$ACTS_B"
    [[ "${FOLLOWS_B:-0}" -ge 1 ]] || die "B has no accepted federation_follows"

    # No unrecovered key-load failures on A log
    assert_no_key_error "$RUN_LOG_A"
    if grep -F "No federation keys found for user" "$RUN_LOG_A" >/dev/null 2>&1; then
      if ! grep -F "Ensured federation keys; retrying delivery sign" "$RUN_LOG_A" >/dev/null 2>&1; then
        die "key-load error in instance A log without ensure recovery"
      fi
    fi

    echo "=== full-chain${tag} OK ==="
  } > >(tee -a "$chain_log") 2>&1
  local chain_rc=${PIPESTATUS[0]:-0}
  [[ "$chain_rc" -eq 0 ]] || die "full-chain${tag} failed (rc=$chain_rc); see $chain_log"

  # Fail if key error string appears without recovery in chain log itself
  if grep -F "No federation keys found for user" "$chain_log" >/dev/null; then
    die "chain log contains key-load error string"
  fi
}

down_only() {
  log "down mode: killing listeners on $PORT_A $PORT_B if any"
  for p in "$PORT_A" "$PORT_B"; do
    pid=$(lsof -tiTCP:"$p" -sTCP:LISTEN 2>/dev/null || true)
    [[ -n "$pid" ]] && kill $pid 2>/dev/null || true
  done
  exit 0
}

main() {
  [[ "$MODE" == "down" ]] && down_only

  ensure_dbs
  build_backend
  reset_dbs

  RUN_LOG_A="$SCRATCH_DIR/intranet-a.log"
  RUN_LOG_B="$SCRATCH_DIR/intranet-b.log"
  : >"$RUN_LOG_A"
  : >"$RUN_LOG_B"

  # free ports
  for p in "$PORT_A" "$PORT_B"; do
    pid=$(lsof -tiTCP:"$p" -sTCP:LISTEN 2>/dev/null || true)
    [[ -n "$pid" ]] && kill $pid 2>/dev/null || true
  done
  sleep 1

  PID_A=$(start_instance a "$PORT_A" "$DB_A" "$JWT_A" "$BASE_A" "$RUN_LOG_A")
  PID_B=$(start_instance b "$PORT_B" "$DB_B" "$JWT_B" "$BASE_B" "$RUN_LOG_B")
  log "PIDs A=$PID_A B=$PID_B"
  echo "PID_A=$PID_A" >"$SCRATCH_DIR/pids.txt"
  echo "PID_B=$PID_B" >>"$SCRATCH_DIR/pids.txt"

  wait_health "$BASE_A" a | tee -a "$SCRATCH_DIR/boot.log"
  wait_health "$BASE_B" b | tee -a "$SCRATCH_DIR/boot.log"

  create_admin "$BASE_A" "$ADMIN_USER_A"
  create_admin "$BASE_B" "$ADMIN_USER_B"

  # Also ensure B has keys (actor GET) before follow — A will fetch actor too
  curl -fsS -H 'Accept: application/activity+json' "$BASE_B/users/${ADMIN_USER_B}" >/dev/null

  # Run 1: cold key (identity creates material; signed Follow A→B delivers)
  run_full_chain "" cold
  if [[ "$MODE" == "twice" ]]; then
    # Run 2: warm path — keep PEM (no rotate), clear queue/follows, Follow again
    log "Second full-chain: warm ensure (no rotate) + re-follow"
    sql_a "DELETE FROM federation_delivery_queue;" || true
    sql_a "DELETE FROM federation_follows WHERE direction='outgoing';" || true
    sql_b "DELETE FROM federation_follows;" || true
    run_full_chain "-rerun" warm
  fi

  log "SUCCESS dual-instance full-chain ($MODE)"
  # Copy logs for gating names expected by plan
  cp -f "$RUN_LOG_A" "$SCRATCH_DIR/intranet-a.log"
  cp -f "$RUN_LOG_B" "$SCRATCH_DIR/intranet-b.log"
}

main
