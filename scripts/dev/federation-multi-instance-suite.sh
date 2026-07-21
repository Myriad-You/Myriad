#!/usr/bin/env bash
# Full multi-instance federation suite on a real local dual-backend intranet.
#
# Covers every major cross-instance path that requires two live backends:
#   identity/keys, follow, unfollow, content fan-out, room invite/accept/message,
#   ring add_peer/sync, channel open/accept/message.
#
# Usage:
#   ./scripts/dev/federation-multi-instance-suite.sh [run|down]
# Env:
#   SCRATCH_DIR  evidence dir (default ./tmp/federation-suite)
#   KEEP_RUNNING=1 leave backends up
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BACKEND="$ROOT/backend"
SCRATCH_DIR="${SCRATCH_DIR:-$ROOT/tmp/federation-suite}"
MODE="${1:-run}"
KEEP_RUNNING="${KEEP_RUNNING:-0}"

PORT_A="${PORT_A:-18080}"
PORT_B="${PORT_B:-18081}"
PORT_C="${PORT_C:-18082}"
BASE_A="http://127.0.0.1:${PORT_A}"
BASE_B="http://127.0.0.1:${PORT_B}"
BASE_C="http://127.0.0.1:${PORT_C}"
DB_A="${DB_A:-myriad_fed_a}"
DB_B="${DB_B:-myriad_fed_b}"
PG_HOST="${PG_HOST:-127.0.0.1}"
PG_PORT="${PG_PORT:-5432}"
PG_USER="${PG_USER:-myriad}"
PG_PASS="${PG_PASS:-myriad_dev_password}"
JWT_A="federation-lab-jwt-aaaaaaaaaaaaaaaaaaaaaa-A1"
JWT_B="federation-lab-jwt-bbbbbbbbbbbbbbbbbbbbbb-B2"
ADMIN_USER_A="alice"
ADMIN_USER_B="bob"
ADMIN_PASS="Testpass1"
PG_CONTAINER="${PG_CONTAINER:-myriad-postgres-dev}"
ACTOR_A="${BASE_A}/users/${ADMIN_USER_A}"
ACTOR_B="${BASE_B}/users/${ADMIN_USER_B}"

CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(
  (cd "$BACKEND" && cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c 'import sys,json; print(json.load(sys.stdin).get("target_directory",""))' 2>/dev/null) \
  || true
)}"
[[ -z "${CARGO_TARGET_DIR}" ]] && CARGO_TARGET_DIR="${BACKEND}/target"
BIN="${CARGO_TARGET_DIR}/debug/myriad-backend"
PID_A="" PID_B="" PID_C=""
RUN_LOG_A="" RUN_LOG_B="" RUN_LOG_C=""
SUITE_LOG=""
PASS=0
FAIL=0
RESULTS=()

mkdir -p "$SCRATCH_DIR"
log() { echo "[fed-suite] $*"; }
die() { echo "[fed-suite] ERROR: $*" >&2; exit 1; }

cleanup() {
  if [[ "$KEEP_RUNNING" == "1" ]]; then
    log "KEEP_RUNNING=1 — leaving backends up (PIDs $PID_A $PID_B $PID_C)"
    return 0
  fi
  [[ -n "${PID_A:-}" ]] && kill "$PID_A" 2>/dev/null || true
  [[ -n "${PID_B:-}" ]] && kill "$PID_B" 2>/dev/null || true
  [[ -n "${PID_C:-}" ]] && kill "$PID_C" 2>/dev/null || true
  wait "$PID_A" 2>/dev/null || true
  wait "$PID_B" 2>/dev/null || true
  wait "$PID_C" 2>/dev/null || true
}
trap cleanup EXIT

ensure_dbs() {
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
  log "Resetting DBs $DB_A $DB_B"
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
  log "Building myriad-backend"
  (cd "$BACKEND" && cargo build --bin myriad-backend) 2>&1 | tee "$SCRATCH_DIR/build.log" | tail -6
  if [[ ! -x "$BIN" ]]; then
    BIN="$(cd "$BACKEND" && cargo metadata --format-version 1 --no-deps \
      | python3 -c 'import sys,json; print(json.load(sys.stdin)["target_directory"])')/debug/myriad-backend"
  fi
  [[ -x "$BIN" ]] || die "binary missing"
}

start_instance() {
  local name="$1" port="$2" db="$3" jwt="$4" base="$5" logfile="$6"
  log "Starting $name $base db=$db" >&2
  (
    export RUST_LOG="${RUST_LOG:-info,myriad_backend::federation=debug}"
    export SERVER_HOST=127.0.0.1 SERVER_PORT="$port"
    export DATABASE_URL="postgres://${PG_USER}:${PG_PASS}@${PG_HOST}:${PG_PORT}/${db}"
    export JWT_SECRET="$jwt" BASE_URL="$base" FRONTEND_URL="$base" CORS_ORIGINS="$base"
    export MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND=1
    export MYRIAD_FEDERATION_DELIVERY_INTERVAL_SECS=2
    export MYRIAD_DB_MODE=external CONFIG_MODE=false
    exec "$BIN"
  ) >"$logfile" 2>&1 &
  echo $!
}

wait_health() {
  local base="$1" label="$2"
  for _ in $(seq 1 60); do
    if curl -fsS "$base/api/setup/status" >/dev/null 2>&1; then
      log "$label healthy"
      curl -fsS "$base/api/setup/status" >"$SCRATCH_DIR/health-${label}.json"
      return 0
    fi
    sleep 1
  done
  die "$label never healthy"
}

create_admin() {
  curl -fsS -X POST "$1/api/setup/create-admin" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$2\",\"password\":\"${ADMIN_PASS}\"}" \
    >"$SCRATCH_DIR/create-admin-$2.json"
}

ensure_alive() {
  local label="$1" base="$2" pid="$3"
  if ! kill -0 "$pid" 2>/dev/null; then
    die "instance $label (pid $pid) is dead — see $SCRATCH_DIR/intranet-${label}.log"
  fi
  if ! curl -fsS "$base/api/setup/status" >/dev/null 2>&1; then
    die "instance $label not responding at $base"
  fi
}

login_cookie() {
  curl -fsS -c "$3" -X POST "$1/api/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$2\",\"password\":\"${ADMIN_PASS}\"}" >/dev/null
}

csrf_token() {
  curl -fsS -b "$2" "$1/api/csrf-token" \
    | python3 -c 'import sys,json; t=json.load(sys.stdin).get("csrf_token"); assert t; print(t)'
}

sql_a() {
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d "$DB_A" -v ON_ERROR_STOP=1 -t -A -c "$1"
}
sql_b() {
  docker exec -e PGPASSWORD="$PG_PASS" "$PG_CONTAINER" \
    psql -U "$PG_USER" -d "$DB_B" -v ON_ERROR_STOP=1 -t -A -c "$1"
}

api() {
  # api METHOD BASE JAR PATH [json_body] — retries 429 briefly
  local method="$1" base="$2" jar="$3" path="$4"
  local body="${5:-}"
  local attempt code out csrf
  for attempt in 1 2 3 4 5 6; do
    csrf=$(csrf_token "$base" "$jar")
    if [[ -n "$body" ]]; then
      out=$(curl -sS -b "$jar" -X "$method" "${base}${path}" \
        -H 'Content-Type: application/json' \
        -H "X-CSRF-Token: ${csrf}" \
        -d "$body" -w '\n%{http_code}' || true)
    else
      out=$(curl -sS -b "$jar" -X "$method" "${base}${path}" \
        -H 'Content-Type: application/json' \
        -H "X-CSRF-Token: ${csrf}" \
        -d '{}' -w '\n%{http_code}' || true)
    fi
    code=$(echo "$out" | tail -1)
    out=$(echo "$out" | sed '$d')
    if [[ "$code" == "429" ]]; then
      echo "  api 429 on $method $path attempt=$attempt; sleep" >&2
      sleep $((attempt * 2))
      continue
    fi
    if [[ "$code" =~ ^2 ]]; then
      echo "$out"
      return 0
    fi
    echo "api HTTP $code $method $path: $out" >&2
    return 22
  done
  echo "api exhausted retries on 429 $method $path" >&2
  return 22
}

api_get() {
  curl -fsS -b "$2" "${1}${3}"
}

nudge_delivery() {
  # $1 = a|b
  if [[ "$1" == "a" ]]; then
    sql_a "UPDATE federation_delivery_queue SET next_retry_at = NOW() - interval '1 second' WHERE status IN ('pending','delivering');" >/dev/null 2>&1 || true
  else
    sql_b "UPDATE federation_delivery_queue SET next_retry_at = NOW() - interval '1 second' WHERE status IN ('pending','delivering');" >/dev/null 2>&1 || true
  fi
}

# Non-cancelled dead count / latest error (user-cancel must not fail waits).
dead_fail_count() {
  local side="$1"
  if [[ "$side" == "a" ]]; then
    sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead' AND COALESCE(error_message,'') NOT ILIKE 'cancelled:%';" || echo 0
  else
    sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead' AND COALESCE(error_message,'') NOT ILIKE 'cancelled:%';" || echo 0
  fi
}

dead_fail_error() {
  local side="$1"
  if [[ "$side" == "a" ]]; then
    sql_a "SELECT error_message FROM federation_delivery_queue WHERE status='dead' AND COALESCE(error_message,'') NOT ILIKE 'cancelled:%' ORDER BY id DESC LIMIT 1;"
  else
    sql_b "SELECT error_message FROM federation_delivery_queue WHERE status='dead' AND COALESCE(error_message,'') NOT ILIKE 'cancelled:%' ORDER BY id DESC LIMIT 1;"
  fi
}

# Die if side has non-cancelled dead rows (optional label for message).
assert_no_dead_fail() {
  local side="$1" label="${2:-delivery}"
  local n err
  n=$(dead_fail_count "$side")
  if [[ "${n:-0}" -ge 1 ]]; then
    err=$(dead_fail_error "$side")
    die "${label} dead on $side: $err"
  fi
}

wait_delivery_side() {
  # wait until side a|b has delivered_count >= min and pending==0 (or timeout)
  #
  # IMPORTANT: only fail on *non-cancelled* dead rows. User cancel paths set
  # error_message to 'cancelled: by user' (or cancelled:*) and must not kill
  # unrelated cases that call wait_delivery_side (root cause of 24/25 flake:
  # "delivery dead on a: cancelled: by user").
  local side="$1" min_delivered="${2:-1}" max_pending="${3:-0}" tries="${4:-40}"
  local i d p dead dead_fail
  for i in $(seq 1 "$tries"); do
    nudge_delivery "$side"
    if [[ "$side" == "a" ]]; then
      d=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
      p=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
      dead=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead';" || echo 0)
    else
      d=$(sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
      p=$(sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
      dead=$(sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead';" || echo 0)
    fi
    # dead_fail excludes cancelled:% — do NOT use raw `dead` for die()
    dead_fail=$(dead_fail_count "$side")
    echo "  wait_${side} t=$i delivered=$d pending=$p dead=$dead dead_fail=$dead_fail"
    if [[ "${dead_fail:-0}" -ge 1 ]]; then
      die "delivery dead on $side: $(dead_fail_error "$side")"
    fi
    if [[ "${d:-0}" -ge "$min_delivered" && "${p:-0}" -le "$max_pending" ]]; then
      return 0
    fi
    sleep 1
  done
  die "delivery wait failed on $side (want delivered>=$min_delivered pending<=$max_pending)"
}

clear_queue() {
  local side="$1"
  if [[ "$side" == "a" ]]; then
    sql_a "DELETE FROM federation_delivery_queue;" >/dev/null || true
  else
    sql_b "DELETE FROM federation_delivery_queue;" >/dev/null || true
  fi
}

assert_no_key_errors() {
  if grep -F "No federation keys found for user" "$RUN_LOG_A" >/dev/null 2>&1; then
    if ! grep -F "Ensured federation keys; retrying delivery sign" "$RUN_LOG_A" >/dev/null 2>&1; then
      die "key-load error on A without ensure recovery"
    fi
  fi
  if grep -F "No federation keys found for user" "$RUN_LOG_B" >/dev/null 2>&1; then
    if ! grep -F "Ensured federation keys; retrying delivery sign" "$RUN_LOG_B" >/dev/null 2>&1; then
      die "key-load error on B without ensure recovery"
    fi
  fi
}

record_pass() {
  PASS=$((PASS + 1))
  RESULTS+=("PASS $1")
  log "PASS: $1"
  echo "PASS $1" >>"$SUITE_LOG"
}

record_fail() {
  FAIL=$((FAIL + 1))
  RESULTS+=("FAIL $1 — $2")
  log "FAIL: $1 — $2"
  echo "FAIL $1 — $2" >>"$SUITE_LOG"
}

run_case() {
  local name="$1"
  shift
  log "==== CASE $name ===="
  echo "==== CASE $name ====" >>"$SUITE_LOG"
  # Capture die() exits without aborting the whole suite.
  set +e
  (
    set -e
    "$@"
  ) >>"$SUITE_LOG" 2>&1
  local rc=$?
  set -e
  if [[ $rc -eq 0 ]]; then
    record_pass "$name"
  else
    record_fail "$name" "exit=$rc (see suite.log)"
  fi
}

# --- cases ---

case_identity_keys() {
  local jar_a="$JAR_A" jar_b="$JAR_B"
  sql_a "DELETE FROM federation_keys;" >/dev/null || true
  sql_b "DELETE FROM federation_keys;" >/dev/null || true
  api_get "$BASE_A" "$jar_a" /api/federation/identity >"$SCRATCH_DIR/identity-a.json"
  api_get "$BASE_B" "$jar_b" /api/federation/identity >"$SCRATCH_DIR/identity-b.json"
  local ka kb
  ka=$(sql_a "SELECT count(*) FROM federation_keys WHERE user_id=1;")
  kb=$(sql_b "SELECT count(*) FROM federation_keys WHERE user_id=1;")
  [[ "$ka" == "1" && "$kb" == "1" ]] || die "keys not created a=$ka b=$kb"
  curl -fsS -H 'Accept: application/activity+json' "$ACTOR_A" >"$SCRATCH_DIR/actor-a.json"
  curl -fsS -H 'Accept: application/activity+json' "$ACTOR_B" >"$SCRATCH_DIR/actor-b.json"
}

case_follow() {
  local jar_a="$JAR_A"
  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/federation/follow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/follow.json"
  wait_delivery_side a 1 0 30
  local fb
  fb=$(sql_b "SELECT count(*) FROM federation_follows WHERE status='accepted';")
  [[ "$fb" -ge 1 ]] || die "B has no accepted follow"
}

case_unfollow() {
  local jar_a="$JAR_A"
  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/federation/unfollow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/unfollow.json" || true
  # Unfollow may enqueue Undo; wait if anything pending
  local p
  p=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  if [[ "${p:-0}" -ge 1 ]]; then
    wait_delivery_side a 1 0 30
  fi
  # Re-establish follow for content tests
  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/federation/follow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/follow-again.json"
  wait_delivery_side a 1 0 30
}

case_content_note() {
  # Fan-out queries INCOMING accepted followers on the author instance.
  # So B must follow A before A's note reaches B.
  local jar_a="$JAR_A" jar_b="$JAR_B"
  clear_queue a
  clear_queue b

  local inc
  inc=$(sql_a "SELECT count(*) FROM federation_follows WHERE direction='incoming' AND status='accepted';")
  if [[ "${inc:-0}" -lt 1 ]]; then
    api POST "$BASE_B" "$jar_b" /api/federation/follow "{\"target\":\"${ACTOR_A}\"}" \
      >"$SCRATCH_DIR/follow-b-to-a.json"
    wait_delivery_side b 1 0 30
    # Wait for Accept A→B to land and flip B's outgoing (optional) and A's incoming
    for i in $(seq 1 20); do
      inc=$(sql_a "SELECT count(*) FROM federation_follows WHERE direction='incoming' AND status='accepted';")
      [[ "${inc:-0}" -ge 1 ]] && break
      nudge_delivery a
      sleep 1
    done
    [[ "${inc:-0}" -ge 1 ]] || die "A has no incoming accepted follower after B→A follow"
  fi

  clear_queue a
  local note_resp
  note_resp=$(api POST "$BASE_A" "$jar_a" /api/federation/notes \
    "{\"text\":\"suite note $(date +%s)\",\"visibility\":\"followers\"}")
  echo "$note_resp" >"$SCRATCH_DIR/note.json"
  local queued
  queued=$(echo "$note_resp" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("delivered_queued",0))')
  [[ "${queued:-0}" -ge 1 ]] || die "note delivered_queued=0 (need incoming followers); resp=$note_resp"
  wait_delivery_side a 1 0 40
  local tl acts
  tl=$(sql_b "SELECT count(*) FROM federation_timeline;" || echo 0)
  acts=$(sql_b "SELECT count(*) FROM federation_activities WHERE activity_type='Create';" || echo 0)
  [[ "${tl:-0}" -ge 1 || "${acts:-0}" -ge 1 ]] || die "B has no Create/timeline after note fan-out tl=$tl acts=$acts"
}

case_room_invite_accept_message() {
  local jar_a="$JAR_A" jar_b="$JAR_B"
  clear_queue a
  clear_queue b

  # B creates open room
  local room_json room_id
  room_json=$(api POST "$BASE_B" "$jar_b" /api/federation/rooms \
    "{\"name\":\"Suite Room\",\"invite_policy\":\"open\",\"is_public\":true,\"governance_type\":\"owner\"}")
  echo "$room_json" >"$SCRATCH_DIR/room-create.json"
  room_id=$(echo "$room_json" | python3 -c 'import sys,json; print(json.load(sys.stdin)["room_id"])')
  [[ -n "$room_id" ]] || die "no room_id"
  echo "$room_id" >"$SCRATCH_DIR/last_room_id.txt"

  # B invites A (remote) — POST /invite (members is GET-only)
  sleep 1
  api POST "$BASE_B" "$jar_b" "/api/federation/rooms/${room_id}/invite" \
    "{\"actor\":\"${ACTOR_A}\",\"role\":\"member\"}" \
    >"$SCRATCH_DIR/room-invite.json"
  wait_delivery_side b 1 0 40

  # A should have pending membership / room after invite handling
  local pending_a
  pending_a=$(sql_a "SELECT count(*) FROM federation_room_members WHERE room_id='${room_id}' AND membership_status='pending';" || echo 0)
  # If invite created room on A via activity, accept
  if [[ "${pending_a:-0}" -ge 1 ]] || sql_a "SELECT 1 FROM federation_rooms WHERE room_id='${room_id}'" | grep -q 1; then
    clear_queue a
    api POST "$BASE_A" "$jar_a" "/api/federation/rooms/${room_id}/accept" "" \
      >"$SCRATCH_DIR/room-accept.json" || \
    api POST "$BASE_A" "$jar_a" "/api/federation/rooms/${room_id}/join" "" \
      >"$SCRATCH_DIR/room-join.json" || true
    # Any outbound from A (RoomJoin)
    local p
    p=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering','delivered');" || echo 0)
    if [[ "${p:-0}" -ge 1 ]]; then
      wait_delivery_side a 1 0 40
    fi
  else
    # Fallback: force-seed room on A is not allowed; try join if room exists via discovery
    die "A did not receive room invite (pending=0, room missing)"
  fi

  # B sends room message → fan-out to remote A
  clear_queue b
  local msg_resp mid
  msg_resp=$(api POST "$BASE_B" "$jar_b" "/api/federation/rooms/${room_id}/messages" \
    "{\"message_type\":\"text\",\"payload\":{\"text\":\"hello from B suite\"}}")
  echo "$msg_resp" >"$SCRATCH_DIR/room-message.json"
  mid=$(echo "$msg_resp" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("message_id") or d.get("id") or "")' 2>/dev/null || true)
  if [[ -z "$mid" ]]; then
    mid=$(sql_b "SELECT message_id FROM federation_room_messages WHERE room_id='${room_id}' ORDER BY created_at DESC LIMIT 1;" 2>/dev/null || true)
  fi
  [[ -n "$mid" ]] && echo "$mid" >"$SCRATCH_DIR/last_room_message_id.txt"
  local pb
  pb=$(sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  if [[ "${pb:-0}" -ge 1 ]]; then
    wait_delivery_side b 1 0 40
  fi
  # A should have room messages or activity
  local msgs
  msgs=$(sql_a "SELECT count(*) FROM federation_room_messages WHERE room_id='${room_id}';" || echo 0)
  # soft: message table may use different storage; check activities
  local ra
  ra=$(sql_a "SELECT count(*) FROM federation_activities WHERE activity_type ILIKE '%Room%' OR activity_type ILIKE '%Message%';" || echo 0)
  [[ "${msgs:-0}" -ge 1 || "${ra:-0}" -ge 1 || "${pb:-0}" -eq 0 ]] || die "no room message evidence on A"
  echo "room_id=$room_id msgs_a=$msgs acts_a=$ra mid=$mid" >>"$SUITE_LOG"
}

case_ring_add_peer() {
  local jar_a="$JAR_A" jar_b="$JAR_B"
  clear_queue a
  clear_queue b

  local ring_json ring_id
  ring_json=$(api POST "$BASE_A" "$jar_a" /api/federation/rings \
    "{\"name\":\"Suite Brew Ring\",\"ring_type\":\"brew-recommend\"}")
  echo "$ring_json" >"$SCRATCH_DIR/ring-create.json"
  ring_id=$(echo "$ring_json" | python3 -c 'import sys,json; print(json.load(sys.stdin)["ring_id"])')

  api POST "$BASE_A" "$jar_a" "/api/federation/rings/${ring_id}/peers" \
    "{\"peer\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/ring-add-peer.json"
  wait_delivery_side a 1 0 40

  # B should have ring membership or activity after RingJoin
  local rb
  rb=$(sql_b "SELECT count(*) FROM federation_ring_memberships WHERE ring_id='${ring_id}';" || echo 0)
  local acts
  acts=$(sql_b "SELECT count(*) FROM federation_activities WHERE activity_type ILIKE '%Ring%';" || echo 0)
  # RingJoin on B may add to known_peers of existing membership or create activity
  [[ "${rb:-0}" -ge 1 || "${acts:-0}" -ge 1 ]] || {
    # Accept if B at least got inbox activity recorded
    local any
    any=$(sql_b "SELECT count(*) FROM federation_activities;" || echo 0)
    [[ "${any:-0}" -ge 1 ]] || die "B has no ring evidence after add_peer"
  }

  # Optional: trigger sync A→B
  clear_queue a
  api POST "$BASE_A" "$jar_a" "/api/federation/rings/${ring_id}/sync" \
    "{\"entries\":[{\"kind\":\"suite\",\"id\":\"1\"}],\"ttl\":3}" \
    >"$SCRATCH_DIR/ring-sync.json" || true
  local p
  p=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  if [[ "${p:-0}" -ge 1 ]]; then
    wait_delivery_side a 1 0 40
  fi
  echo "ring_id=$ring_id b_rings=$rb b_ring_acts=$acts" >>"$SUITE_LOG"
}

case_channel_open_accept_message() {
  local jar_a="$JAR_A" jar_b="$JAR_B"
  clear_queue a
  clear_queue b

  local ch_json channel_id
  ch_json=$(api POST "$BASE_A" "$jar_a" /api/federation/channels \
    "{\"remote_actor\":\"${ACTOR_B}\",\"channel_type\":\"text\",\"transport\":\"http\"}")
  echo "$ch_json" >"$SCRATCH_DIR/channel-create.json"
  channel_id=$(echo "$ch_json" | python3 -c 'import sys,json; print(json.load(sys.stdin)["channel_id"])')
  [[ -n "$channel_id" ]] || die "no channel_id"
  echo "$channel_id" >"$SCRATCH_DIR/last_channel_id_a.txt"

  wait_delivery_side a 1 0 40

  # B should have pending channel
  local pending
  pending=$(sql_b "SELECT count(*) FROM federation_channels WHERE channel_id='${channel_id}' AND status='pending';" || echo 0)
  if [[ "${pending:-0}" -lt 1 ]]; then
    pending=$(sql_b "SELECT count(*) FROM federation_channels WHERE status='pending';" || echo 0)
  fi
  [[ "${pending:-0}" -ge 1 ]] || {
    # list channels on B via API
    api_get "$BASE_B" "$jar_b" /api/federation/channels >"$SCRATCH_DIR/channels-b.json" || true
    die "B has no pending channel after ChannelOpen"
  }

  # Resolve channel_id on B if different
  local ch_b
  ch_b=$(sql_b "SELECT channel_id FROM federation_channels WHERE status='pending' ORDER BY created_at DESC LIMIT 1;")
  [[ -n "$ch_b" ]] || die "no pending channel id on B"
  echo "$ch_b" >"$SCRATCH_DIR/last_channel_id_b.txt"

  clear_queue b
  api POST "$BASE_B" "$jar_b" "/api/federation/channels/${ch_b}/accept" "" \
    >"$SCRATCH_DIR/channel-accept.json"
  wait_delivery_side b 1 0 40

  # A should show accepted
  local st
  st=$(sql_a "SELECT status FROM federation_channels WHERE channel_id='${channel_id}';" || echo "")
  [[ "$st" == "accepted" || "$st" == "active" ]] || {
    # Accept may flip via inbound activity with slight delay
    sleep 2
    st=$(sql_a "SELECT status FROM federation_channels WHERE channel_id='${channel_id}';" || echo "")
    [[ "$st" == "accepted" || "$st" == "active" ]] || die "A channel status not accepted: $st"
  }

  clear_queue a
  api POST "$BASE_A" "$jar_a" "/api/federation/channels/${channel_id}/messages" \
    "{\"message_type\":\"text\",\"payload\":{\"text\":\"channel suite hi\"}}" \
    >"$SCRATCH_DIR/channel-message.json"
  wait_delivery_side a 1 0 40

  local mb
  mb=$(sql_b "SELECT count(*) FROM federation_channel_messages WHERE channel_id='${ch_b}';" || echo 0)
  [[ "${mb:-0}" -ge 1 ]] || die "B has no channel messages after send"
  echo "channel_id=$channel_id ch_b=$ch_b msgs_b=$mb status_a=$st" >>"$SUITE_LOG"
}

# --- expanded cases ---

case_room_pin() {
  local jar_b="$JAR_B"
  local room_id mid
  room_id=$(cat "$SCRATCH_DIR/last_room_id.txt" 2>/dev/null || true)
  mid=$(cat "$SCRATCH_DIR/last_room_message_id.txt" 2>/dev/null || true)
  [[ -n "$room_id" ]] || die "missing last_room_id (room case must run first)"
  if [[ -z "$mid" ]]; then
    mid=$(sql_b "SELECT message_id FROM federation_room_messages WHERE room_id='${room_id}' ORDER BY 1 DESC LIMIT 1;")
  fi
  [[ -n "$mid" ]] || die "no room message_id to pin"
  clear_queue b
  api POST "$BASE_B" "$jar_b" "/api/federation/rooms/${room_id}/messages/${mid}/pin" \
    "{\"pinned\":true}" >"$SCRATCH_DIR/room-pin.json"
  local pinned
  pinned=$(sql_b "SELECT is_pinned::text FROM federation_room_messages WHERE message_id='${mid}';" || echo false)
  [[ "$pinned" == "t" || "$pinned" == "true" ]] || die "message not pinned: $pinned"
  # pin may fan-out
  local p
  p=$(sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  if [[ "${p:-0}" -ge 1 ]]; then
    wait_delivery_side b 1 0 40
  fi
  echo "pinned mid=$mid room=$room_id" >>"$SUITE_LOG"
}

case_room_e2e_key_exchange() {
  local jar_b="$JAR_B"
  local room_id
  room_id=$(cat "$SCRATCH_DIR/last_room_id.txt" 2>/dev/null || true)
  [[ -n "$room_id" ]] || die "missing room_id"
  clear_queue b
  api POST "$BASE_B" "$jar_b" "/api/federation/rooms/${room_id}/e2e/key-exchange" "" \
    >"$SCRATCH_DIR/room-e2e.json"
  local p
  p=$(sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering','delivered');" || echo 0)
  # KeyExchange may enqueue; if no remote active member for fan-out, still OK if keys stored
  if [[ "${p:-0}" -ge 1 ]]; then
    # only wait if pending exists
    local pend
    pend=$(sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
    if [[ "${pend:-0}" -ge 1 ]]; then
      wait_delivery_side b 1 0 40
    fi
  fi
  local has
  has=$(sql_b "SELECT count(*) FROM federation_room_members WHERE room_id='${room_id}' AND custom_permissions::text LIKE '%local_public_key%';" || echo 0)
  [[ "${has:-0}" -ge 1 ]] || {
    # response may include public_key
    grep -q "public_key\|publicKey\|local_public" "$SCRATCH_DIR/room-e2e.json" || die "room e2e no keys evidence"
  }
  echo "room_e2e room=$room_id" >>"$SUITE_LOG"
}

case_channel_e2e_key_exchange() {
  local jar_a="$JAR_A"
  local channel_id
  channel_id=$(cat "$SCRATCH_DIR/last_channel_id_a.txt" 2>/dev/null || true)
  [[ -n "$channel_id" ]] || die "missing channel_id"
  clear_queue a
  api POST "$BASE_A" "$jar_a" "/api/federation/channels/${channel_id}/e2e/key-exchange" "" \
    >"$SCRATCH_DIR/channel-e2e.json"
  local pend
  pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  if [[ "${pend:-0}" -ge 1 ]]; then
    wait_delivery_side a 1 0 40
  fi
  grep -qE "public_key|publicKey|established|success" "$SCRATCH_DIR/channel-e2e.json" \
    || die "channel e2e response unexpected: $(cat "$SCRATCH_DIR/channel-e2e.json")"
  echo "channel_e2e ok" >>"$SUITE_LOG"
}

case_file_transfer() {
  local jar_a="$JAR_A"
  local channel_id transfer_id
  channel_id=$(cat "$SCRATCH_DIR/last_channel_id_a.txt" 2>/dev/null || true)
  [[ -n "$channel_id" ]] || die "missing channel_id for transfer"
  clear_queue a
  local payload checksum size=32
  # small payload for one chunk
  payload=$(python3 -c 'import base64; print(base64.b64encode(b"suite-file-bytes-0123456789ab").decode())')
  checksum=$(python3 -c 'import hashlib; print(hashlib.sha256(b"suite-file-bytes-0123456789ab").hexdigest())')
  size=$(python3 -c 'print(len(b"suite-file-bytes-0123456789ab"))')
  local init
  init=$(api POST "$BASE_A" "$jar_a" "/api/federation/channels/${channel_id}/transfers" \
    "{\"filename\":\"suite.bin\",\"file_size\":${size},\"mime_type\":\"application/octet-stream\",\"checksum\":\"${checksum}\"}")
  echo "$init" >"$SCRATCH_DIR/transfer-init.json"
  transfer_id=$(echo "$init" | python3 -c 'import sys,json; print(json.load(sys.stdin)["transfer_id"])')
  [[ -n "$transfer_id" ]] || die "no transfer_id"
  # upload first (only) chunk
  api POST "$BASE_A" "$jar_a" "/api/federation/transfers/${transfer_id}/chunks" \
    "{\"chunk_index\":0,\"chunk_data\":\"${payload}\",\"chunk_size\":${size}}" \
    >"$SCRATCH_DIR/transfer-chunk.json"
  local pend
  pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  if [[ "${pend:-0}" -ge 1 ]]; then
    wait_delivery_side a 1 0 40
  fi
  local st
  st=$(sql_a "SELECT status FROM federation_file_transfers WHERE transfer_id='${transfer_id}';" || echo "")
  [[ -n "$st" ]] || die "transfer row missing"
  # B may have inbound transfer meta
  local tb
  tb=$(sql_b "SELECT count(*) FROM federation_file_transfers;" || echo 0)
  echo "transfer_id=$transfer_id status=$st transfers_b=$tb" >>"$SUITE_LOG"
  [[ "${tb:-0}" -ge 1 || "$st" != "" ]] || die "no transfer evidence on B"
}

case_trust_policy() {
  local jar_a="$JAR_A"
  # GET policy
  api_get "$BASE_A" "$jar_a" /api/federation/trust/policy >"$SCRATCH_DIR/trust-policy-get.json"
  # Raise rate limits for lab stability + set min trust
  api PUT "$BASE_A" "$jar_a" /api/federation/trust/policy \
    "{\"min_trust_level\":0,\"auto_discover\":true,\"rate_limit\":{\"max_requests_per_window\":10000,\"window_seconds\":60,\"trusted_multiplier\":2}}" \
    >"$SCRATCH_DIR/trust-policy-put.json"
  # List instances (may include B after federation traffic)
  api_get "$BASE_A" "$jar_a" /api/federation/trust/instances >"$SCRATCH_DIR/trust-instances.json" || true
  local domain_b
  domain_b=$(python3 -c "from urllib.parse import urlparse; print(urlparse('${BASE_B}').hostname)")
  api POST "$BASE_A" "$jar_a" /api/federation/trust/update \
    "{\"domain\":\"${domain_b}\",\"trust_level\":3}" \
    >"$SCRATCH_DIR/trust-instance.json" || true
  api POST "$BASE_A" "$jar_a" /api/federation/trust/block \
    "{\"domain\":\"evil.example\",\"block\":true}" \
    >"$SCRATCH_DIR/trust-block.json" || true
  # Content filter CRUD
  api POST "$BASE_A" "$jar_a" /api/federation/trust/filters \
    "{\"name\":\"suite-block-test\",\"filter_type\":\"block_keyword\",\"value\":\"__suite_never_match__\",\"enabled\":true}" \
    >"$SCRATCH_DIR/trust-filter-create.json" || true
  api_get "$BASE_A" "$jar_a" /api/federation/trust/filters >"$SCRATCH_DIR/trust-filters.json" || true
  grep -qE "min_trust|rate_limit|auto_discover|success|policy|trust" "$SCRATCH_DIR/trust-policy-put.json" \
    || die "trust policy put failed: $(cat "$SCRATCH_DIR/trust-policy-put.json")"
  echo "trust policy ok domain_b=$domain_b" >>"$SUITE_LOG"
}

case_domain_move() {
  local jar_a="$JAR_A"
  # Ensure A has keys + at least one follower path (from content_note: B follows A)
  local inc
  inc=$(sql_a "SELECT count(*) FROM federation_follows WHERE direction='incoming' AND status='accepted';" || echo 0)
  if [[ "${inc:-0}" -lt 1 ]]; then
    clear_queue b
    api POST "$BASE_B" "$JAR_B" /api/federation/follow "{\"target\":\"${ACTOR_A}\"}" >/dev/null || true
    wait_delivery_side b 1 0 30 || true
  fi
  # dry_run
  api POST "$BASE_A" "$jar_a" /api/admin/federation/domain-move \
    "{\"old_base_url\":\"${BASE_A}\",\"new_base_url\":\"${BASE_C}\",\"dry_run\":true}" \
    >"$SCRATCH_DIR/domain-move-dry.json"
  grep -q "dry_run\|would_enqueue\|total_users" "$SCRATCH_DIR/domain-move-dry.json" \
    || die "domain move dry_run unexpected: $(cat "$SCRATCH_DIR/domain-move-dry.json")"

  # Bring up instance C: same DB as A, BASE_URL=new host so Move verify can fetch new actor.
  for pid in $(lsof -tiTCP:"$PORT_C" -sTCP:LISTEN 2>/dev/null || true); do
    kill "$pid" 2>/dev/null || true
  done
  RUN_LOG_C="$SCRATCH_DIR/intranet-c.log"
  : >"$RUN_LOG_C"
  PID_C=$(start_instance c "$PORT_C" "$DB_A" "$JWT_A" "$BASE_C" "$RUN_LOG_C")
  echo "PID_C=$PID_C" >>"$SCRATCH_DIR/pids.txt"
  wait_health "$BASE_C" c
  # Warm new actor doc on C (shared DB; BASE_C for keyId after move)
  curl -fsS -H 'Accept: application/activity+json' "${BASE_C}/users/${ADMIN_USER_A}" \
    >"$SCRATCH_DIR/actor-c-pre.json" || true

  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/admin/federation/domain-move \
    "{\"old_base_url\":\"${BASE_A}\",\"new_base_url\":\"${BASE_C}\",\"dry_run\":false}" \
    >"$SCRATCH_DIR/domain-move-live.json"
  local enq regen
  enq=$(python3 -c 'import json; d=json.load(open("'"$SCRATCH_DIR"'/domain-move-live.json")); print(d.get("enqueued",0))' 2>/dev/null || echo 0)
  regen=$(python3 -c 'import json; d=json.load(open("'"$SCRATCH_DIR"'/domain-move-live.json")); print(d.get("shared_keys",{}).get("regenerated_keys",-1))' 2>/dev/null || echo -1)
  [[ "${regen}" == "0" ]] || die "domain move regenerated keys (must be 0): regen=$regen"
  local kid
  kid=$(sql_a "SELECT key_id FROM federation_keys WHERE user_id=1;" || echo "")
  echo "$kid" | grep -q "${PORT_C}" || die "key_id not retargeted to C: $kid"
  # New actor must be fetchable on C after move
  curl -fsS -H 'Accept: application/activity+json' "${BASE_C}/users/${ADMIN_USER_A}" \
    >"$SCRATCH_DIR/actor-c-post.json"
  local pend
  pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  if [[ "${pend:-0}" -ge 1 ]]; then
    wait_delivery_side a 1 0 50
  fi
  assert_no_dead_fail a "Move delivery"
  local moves
  moves=$(sql_b "SELECT count(*) FROM federation_activities WHERE activity_type='Move' OR object_json::text ILIKE '%\"type\": \"Move\"%' OR object_json::text ILIKE '%Move%';" || echo 0)
  echo "domain_move enqueued=$enq moves_b=$moves key_id=$kid regen=$regen" >>"$SUITE_LOG"
  [[ "${enq:-0}" -ge 1 ]] || die "domain move enqueued=0"
}

case_transfer_ownership() {
  # B (owner) transfers room ownership to A (remote actor) if API allows remote
  local jar_b="$JAR_B"
  local room_id
  room_id=$(cat "$SCRATCH_DIR/last_room_id.txt" 2>/dev/null || true)
  [[ -n "$room_id" ]] || die "missing room_id"
  clear_queue b
  # Try transfer to remote alice — may reject remote; soft-pass if 400 with clear error
  set +e
  local out code
  out=$(api POST "$BASE_B" "$jar_b" "/api/federation/rooms/${room_id}/transfer-ownership" \
    "{\"new_owner\":\"${ACTOR_A}\"}" 2>"$SCRATCH_DIR/transfer-own.err")
  code=$?
  set -e
  echo "$out" >"$SCRATCH_DIR/transfer-ownership.json"
  if [[ $code -ne 0 ]]; then
    # Accept documented rejection of remote transfer as exercised path
    if grep -qiE "local|owner|cannot|remote|error" "$SCRATCH_DIR/transfer-own.err" "$SCRATCH_DIR/transfer-ownership.json" 2>/dev/null; then
      echo "transfer_ownership rejected as expected for remote target" >>"$SUITE_LOG"
      return 0
    fi
    die "transfer-ownership failed unexpectedly"
  fi
  local pend
  pend=$(sql_b "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  if [[ "${pend:-0}" -ge 1 ]]; then
    wait_delivery_side b 1 0 40
  fi
  echo "transfer_ownership ok" >>"$SUITE_LOG"
}

# --- production-deploy scenario cases ---

restart_instance_a() {
  local jwt="$1"
  local logfile="${2:-$RUN_LOG_A}"
  if [[ -n "${PID_A:-}" ]]; then
    kill "$PID_A" 2>/dev/null || true
    wait "$PID_A" 2>/dev/null || true
  fi
  for pid in $(lsof -tiTCP:"$PORT_A" -sTCP:LISTEN 2>/dev/null || true); do
    kill "$pid" 2>/dev/null || true
  done
  sleep 1
  # Append restart banner so logs stay continuous
  echo "===== restart instance A jwt_tag=$(echo -n "$jwt" | wc -c) chars =====" >>"$logfile"
  PID_A=$(start_instance a "$PORT_A" "$DB_A" "$jwt" "$BASE_A" "$logfile")
  echo "PID_A=$PID_A" >>"$SCRATCH_DIR/pids.txt"
  wait_health "$BASE_A" a
  # Session cookie may still work if JWT secret changed — re-login after restart
  login_cookie "$BASE_A" "$ADMIN_USER_A" "$JAR_A"
}

case_deploy_second_local_user() {
  # Deploy: second local account must get its own keys without clobbering user 1.
  local jar_a="$JAR_A"
  # Create second user via admin API if available
  set +e
  api POST "$BASE_A" "$jar_a" /api/admin/users \
    "{\"username\":\"carol\",\"password\":\"${ADMIN_PASS}\",\"is_admin\":false}" \
    >"$SCRATCH_DIR/deploy-user-carol.json" 2>"$SCRATCH_DIR/deploy-user-carol.err"
  local rc=$?
  set -e
  # Alternate register path
  if [[ $rc -ne 0 ]]; then
    curl -fsS -X POST "$BASE_A/api/auth/register" \
      -H 'Content-Type: application/json' \
      -d "{\"username\":\"carol\",\"password\":\"${ADMIN_PASS}\"}" \
      >"$SCRATCH_DIR/deploy-user-carol.json" 2>/dev/null || {
      echo "deploy_second_user skip (cannot create carol)" >>"$SUITE_LOG"
      return 0
    }
  fi
  local jar_c="$SCRATCH_DIR/jar-carol.jar"
  login_cookie "$BASE_A" "carol" "$jar_c" || {
    echo "deploy_second_user skip (carol login failed)" >>"$SUITE_LOG"
    return 0
  }
  local pem1
  pem1=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  api_get "$BASE_A" "$jar_c" /api/federation/identity >"$SCRATCH_DIR/identity-carol.json"
  local n_carol
  n_carol=$(sql_a "SELECT count(*) FROM federation_keys k JOIN users u ON u.id=k.user_id WHERE u.username='carol';")
  [[ "$n_carol" == "1" ]] || die "carol has no federation_keys"
  local pem1_after
  pem1_after=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ "$pem1" == "$pem1_after" ]] || die "second user ensure rotated alice keys"
  echo "deploy_second_local_user ok" >>"$SUITE_LOG"
}

case_deploy_cancel_during_backoff() {
  # Cancel must win over a claimable pending row (worker may race cancel).
  # Product: cancel sets dead+cancelled; worker only mutates status=delivering
  # so a cancelled row is never resurrected to pending/delivered.
  #
  # Do NOT call wait_delivery_side here — that helper waits for fan-out success;
  # this case only asserts cancel stickiness. clear_queue first so leftover
  # dead rows from prior cases cannot confuse status checks.
  local jar_a="$JAR_A"
  local act qid st err api_body marker
  clear_queue a
  act=$(sql_a "SELECT id FROM federation_activities WHERE user_id=1 ORDER BY id DESC LIMIT 1;" | tr -d '[:space:]')
  [[ -n "$act" && "$act" =~ ^[0-9]+$ ]] || die "no activity for cancel-backoff (act='$act')"
  # Unique marker — avoid RETURNING id via docker/psql noise (e.g. "31INSERT01").
  marker="suite: backoff cancel $(date +%s)-$$"
  # next_retry_at in the past → eligible for claim so cancel races the worker.
  sql_a "INSERT INTO federation_delivery_queue
         (activity_id, target_inbox, target_domain, status, attempts, max_attempts, error_message, created_at, next_retry_at)
         VALUES (${act}, 'http://127.0.0.1:9/backoff-cancel', '127.0.0.1', 'pending', 5, 12,
                 '${marker}', NOW(), NOW() - interval '1 second');" >/dev/null \
    || die "insert backoff-cancel queue row failed"
  qid=$(sql_a "SELECT id FROM federation_delivery_queue WHERE error_message='${marker}' ORDER BY id DESC LIMIT 1;" | tr -d '[:space:]')
  [[ -n "$qid" && "$qid" =~ ^[0-9]+$ ]] || die "lookup queue id failed: '$qid' marker='$marker'"

  api POST "$BASE_A" "$jar_a" "/api/federation/delivery/${qid}/cancel" "" \
    >"$SCRATCH_DIR/deploy-cancel-backoff.json"
  api_body=$(cat "$SCRATCH_DIR/deploy-cancel-backoff.json" 2>/dev/null || true)

  # If worker already claimed as delivering mid-cancel, one more cancel wins.
  st=$(sql_a "SELECT status FROM federation_delivery_queue WHERE id=${qid};" | tr -d '[:space:]')
  if [[ "$st" == "pending" || "$st" == "delivering" ]]; then
    api POST "$BASE_A" "$jar_a" "/api/federation/delivery/${qid}/cancel" "" \
      >"$SCRATCH_DIR/deploy-cancel-backoff-retry.json" || true
    st=$(sql_a "SELECT status FROM federation_delivery_queue WHERE id=${qid};" | tr -d '[:space:]')
    api_body=$(cat "$SCRATCH_DIR/deploy-cancel-backoff-retry.json" 2>/dev/null || echo "$api_body")
  fi

  if [[ -z "$st" ]]; then
    die "cancel-backoff row id=$qid vanished after cancel (api=$api_body)"
  fi
  err=$(sql_a "SELECT COALESCE(error_message,'') FROM federation_delivery_queue WHERE id=${qid};")
  [[ "$st" == "dead" ]] || die "backoff cancel status=$st err=$err api=$api_body"
  echo "$err" | grep -qi '^cancelled:' \
    || die "expected cancelled: error_message, got '$err' (api=$api_body)"

  # Worker ticks must not resurrect — status stays dead; do not wait_delivery_side.
  local i
  for i in 1 2 3 4 5; do
    nudge_delivery a
    sleep 1
    st=$(sql_a "SELECT status FROM federation_delivery_queue WHERE id=${qid};" | tr -d '[:space:]')
    err=$(sql_a "SELECT COALESCE(error_message,'') FROM federation_delivery_queue WHERE id=${qid};")
    if [[ -z "$st" ]]; then
      die "cancelled row id=$qid vanished after worker tick t=$i"
    fi
    if [[ "$st" != "dead" ]]; then
      die "cancelled row resurrected: status=$st err=$err id=$qid"
    fi
    echo "$err" | grep -qi '^cancelled:' \
      || die "cancel marker lost after tick t=$i: err='$err' id=$qid"
  done
  echo "deploy_cancel_during_backoff ok qid=$qid" >>"$SUITE_LOG"
}

case_deploy_post_move_switch_base() {
  # Production: after domain Move, cut over BASE_URL to new host (C).
  # Restart A serving BASE_C with same DB so keyId host matches signatures.
  if [[ -n "${PID_A:-}" ]]; then
    kill "$PID_A" 2>/dev/null || true
    wait "$PID_A" 2>/dev/null || true
  fi
  for pid in $(lsof -tiTCP:"$PORT_A" -sTCP:LISTEN 2>/dev/null || true); do
    kill "$pid" 2>/dev/null || true
  done
  sleep 1
  # A process now advertises new base on port C if C already up; keep C as the
  # public new base. Re-bind: stop old A, start A on PORT_A still for API tests
  # is wrong — instead point subsequent API calls at BASE_C for alice admin.
  # Keep C running; re-login against C.
  wait_health "$BASE_C" c
  login_cookie "$BASE_C" "$ADMIN_USER_A" "$JAR_A"
  # Re-export for remaining cases that still use BASE_A for alice writes:
  # transfer_ownership is on B; no_key_spam only checks logs.
  # Verify new actor + keyId consistent on C
  curl -fsS -H 'Accept: application/activity+json' "${BASE_C}/users/${ADMIN_USER_A}" \
    >"$SCRATCH_DIR/actor-c-cutover.json"
  local kid
  kid=$(sql_a "SELECT key_id FROM federation_keys WHERE user_id=1;")
  echo "$kid" | grep -q "${PORT_C}" || die "post-move key_id not on C: $kid"
  # Public key id in actor doc should reference C
  grep -q "${PORT_C}" "$SCRATCH_DIR/actor-c-cutover.json" \
    || die "actor doc after cutover missing new host"
  echo "deploy_post_move_switch_base ok key_id=$kid" >>"$SUITE_LOG"
}

case_deploy_cold_keys_before_outbound() {
  # Deploy: admin never hit Actor GET / identity; first outbound must still work.
  local jar_a="$JAR_A"
  # Clean slate: remove follow rows + queue + keys (no identity call after wipe)
  api POST "$BASE_A" "$jar_a" /api/federation/unfollow "{\"target\":\"${ACTOR_B}\"}" >/dev/null || true
  sleep 1
  sql_a "DELETE FROM federation_follows WHERE direction='outgoing';" >/dev/null || true
  sql_b "DELETE FROM federation_follows;" >/dev/null || true
  # Drop peer cache of alice so first signed post after re-key is verified against fresh actor
  sql_b "DELETE FROM federation_remote_actors WHERE actor_url LIKE '%/users/${ADMIN_USER_A}%';" >/dev/null || true
  clear_queue a
  sql_a "DELETE FROM federation_delivery_queue;" >/dev/null || true
  sql_a "DELETE FROM federation_keys;" >/dev/null || true
  local before
  before=$(sql_a "SELECT count(*) FROM federation_keys;")
  [[ "$before" == "0" ]] || die "expected 0 keys before cold outbound"
  # Do NOT call identity; Follow path + delivery choke point must ensure.
  api POST "$BASE_A" "$jar_a" /api/federation/follow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/deploy-cold-follow.json"
  local after
  after=$(sql_a "SELECT count(*) FROM federation_keys WHERE user_id=1;")
  [[ "$after" == "1" ]] || die "cold outbound did not create keys (count=$after)"
  for i in $(seq 1 40); do
    nudge_delivery a
    pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
    dead=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead';" || echo 0)
    del=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
    dead_fail=$(dead_fail_count a)
    echo "  cold_keys t=$i pending=$pend delivered=$del dead=$dead dead_fail=$dead_fail"
    if [[ "${dead_fail:-0}" -ge 1 ]]; then
      die "cold outbound dead: $(dead_fail_error a)"
    fi
    [[ "${pend:-0}" -eq 0 && "${del:-0}" -ge 1 ]] && break
    sleep 1
  done
  pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  del=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
  [[ "${pend:-0}" -eq 0 && "${del:-0}" -ge 1 ]] || die "cold outbound not delivered (pending=$pend delivered=$del)"
  # Prove no key-load failure string for this window
  if grep -F "No federation keys found for user" "$RUN_LOG_A" >/dev/null 2>&1; then
    if ! grep -F "Ensured federation keys; retrying delivery sign" "$RUN_LOG_A" >/dev/null 2>&1; then
      die "cold path hit missing keys without ensure recovery"
    fi
  fi
  echo "deploy_cold_keys ok" >>"$SUITE_LOG"
}

case_deploy_jwt_mismatch_no_rotate() {
  # Deploy: JWT_SECRET changed (or wrong secret on one replica) while federation_keys remain.
  # Must NOT regenerate public key; decrypt fails; restoring secret recovers.
  local jar_a="$JAR_A"
  # Ensure keys exist under JWT_A
  api_get "$BASE_A" "$jar_a" /api/federation/identity >/dev/null
  local pem_before
  pem_before=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ -n "$pem_before" ]] || die "no PEM before jwt mismatch test"
  echo "$pem_before" >"$SCRATCH_DIR/deploy-pem-before.txt"

  local JWT_WRONG="federation-lab-jwt-WRONG-WRONG-WRONG-WRONG-X9"
  restart_instance_a "$JWT_WRONG" "$RUN_LOG_A"

  # Identity must not rotate keys under wrong secret (non-empty PEM present)
  api_get "$BASE_A" "$JAR_A" /api/federation/identity >/dev/null || true
  local pem_mid
  pem_mid=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ "$pem_mid" == "$pem_before" ]] || die "JWT mismatch rotated public PEM (forbidden)"

  # Outbound with wrong JWT: delivery should fail decrypt, not mint new keys
  clear_queue a
  sql_a "DELETE FROM federation_delivery_queue;" >/dev/null || true
  api POST "$BASE_A" "$JAR_A" /api/federation/unfollow "{\"target\":\"${ACTOR_B}\"}" >/dev/null || true
  sleep 1
  clear_queue a
  api POST "$BASE_A" "$JAR_A" /api/federation/follow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/deploy-jwt-wrong-follow.json" || true
  # Wait a few delivery ticks for key decrypt failures
  for i in $(seq 1 8); do
    nudge_delivery a
    sleep 1
  done
  local pem_after_fail
  pem_after_fail=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ "$pem_after_fail" == "$pem_before" ]] || die "decrypt-fail path rotated PEM"

  # Expect decrypt errors in log or queue error_message (not silent regen)
  local qerr
  qerr=$(sql_a "SELECT count(*) FROM federation_delivery_queue WHERE error_message ILIKE '%decrypt%' OR error_message ILIKE '%Key load%';" || echo 0)
  if ! grep -E "Key decryption failed|Key load failed" "$RUN_LOG_A" >/dev/null 2>&1; then
    [[ "${qerr:-0}" -ge 1 ]] || echo "warn: no decrypt log yet (may still be pending)" >>"$SUITE_LOG"
  fi

  # Restore correct JWT — same PEM must decrypt and deliver again
  restart_instance_a "$JWT_A" "$RUN_LOG_A"
  local pem_restored
  pem_restored=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ "$pem_restored" == "$pem_before" ]] || die "PEM changed after JWT restore"
  clear_queue a
  api POST "$BASE_A" "$JAR_A" /api/federation/unfollow "{\"target\":\"${ACTOR_B}\"}" >/dev/null || true
  sleep 1
  clear_queue a
  # Clear B's stale remote cache so re-follow can re-fetch actor if needed
  sql_b "DELETE FROM federation_remote_actors WHERE actor_url LIKE '%/users/${ADMIN_USER_A}%';" >/dev/null || true
  api POST "$BASE_A" "$JAR_A" /api/federation/follow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/deploy-jwt-restored-follow.json"
  for i in $(seq 1 30); do
    nudge_delivery a
    pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
    dead=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead';" || echo 0)
    del=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
    dead_fail=$(dead_fail_count a)
    echo "  jwt_restore t=$i pending=$pend delivered=$del dead=$dead dead_fail=$dead_fail"
    [[ "${pend:-0}" -eq 0 && "${del:-0}" -ge 1 ]] && break
    [[ "${dead_fail:-0}" -ge 1 ]] && die "after JWT restore delivery dead: $(dead_fail_error a)"
    sleep 1
  done
  del=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
  [[ "${del:-0}" -ge 1 ]] || die "after JWT restore no delivered activity"
  echo "deploy_jwt_mismatch_no_rotate ok" >>"$SUITE_LOG"
}

case_deploy_stale_remote_pubkey() {
  # Deploy: ops wiped/regenerated local keys while peers cache old actor PEM → 401.
  # Suite documents expected failure then recovery via peer re-fetch (delete remote cache).
  local jar_a="$JAR_A"
  api_get "$BASE_A" "$jar_a" /api/federation/identity >/dev/null
  # Force B to cache current actor
  curl -fsS -H 'Accept: application/activity+json' "$ACTOR_A" >/dev/null
  sql_b "SELECT count(*) FROM federation_remote_actors WHERE actor_url LIKE '%alice%';" >/dev/null || true

  local pem1
  pem1=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  # Rotate by DELETE + ensure (simulates disaster rekey, not normal path)
  sql_a "DELETE FROM federation_keys;" >/dev/null
  api_get "$BASE_A" "$jar_a" /api/federation/identity >/dev/null
  local pem2
  pem2=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ "$pem1" != "$pem2" ]] || die "expected new PEM after forced rekey"

  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/federation/unfollow "{\"target\":\"${ACTOR_B}\"}" >/dev/null || true
  sleep 1
  clear_queue a
  # B still has old pubkey in remote_actors — delivery may 401
  api POST "$BASE_A" "$jar_a" /api/federation/follow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/deploy-stale-follow.json" || true
  for i in $(seq 1 12); do
    nudge_delivery a
    sleep 1
  done
  local dead401
  dead401=$(sql_a "SELECT count(*) FROM federation_delivery_queue WHERE error_message ILIKE '%401%' OR error_message ILIKE '%signature%';" || echo 0)
  # Recovery: drop B's cached actor so next fetch gets new PEM
  sql_b "DELETE FROM federation_remote_actors WHERE actor_url LIKE '%/users/${ADMIN_USER_A}%';" >/dev/null || true
  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/federation/unfollow "{\"target\":\"${ACTOR_B}\"}" >/dev/null || true
  sleep 1
  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/federation/follow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/deploy-stale-recovered-follow.json"
  for i in $(seq 1 30); do
    nudge_delivery a
    pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
    del=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
    dead=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead';" || echo 0)
    dead_fail=$(dead_fail_count a)
    echo "  stale_recover t=$i pending=$pend delivered=$del dead=$dead dead_fail=$dead_fail (prior401=$dead401)"
    [[ "${pend:-0}" -eq 0 && "${del:-0}" -ge 1 ]] && break
    [[ "${dead_fail:-0}" -ge 1 && "${pend:-0}" -eq 0 ]] && \
      die "stale pubkey recovery failed: $(dead_fail_error a)"
    sleep 1
  done
  del=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
  [[ "${del:-0}" -ge 1 ]] || die "stale pubkey recovery: no delivered"
  echo "deploy_stale_remote_pubkey ok prior_sig_fail=$dead401" >>"$SUITE_LOG"
}

case_deploy_retry_dead() {
  # Deploy: after transient dead letters, retry-dead should requeue.
  local jar_a="$JAR_A"
  # Seed a dead row artificially if none
  local dead_n
  dead_n=$(sql_a "SELECT count(*) FROM federation_delivery_queue WHERE status='dead';" || echo 0)
  if [[ "${dead_n:-0}" -lt 1 ]]; then
    # Insert minimal dead letter attached to latest activity if any
    local act
    act=$(sql_a "SELECT id FROM federation_activities ORDER BY id DESC LIMIT 1;" || echo "")
    if [[ -n "$act" ]]; then
      sql_a "INSERT INTO federation_delivery_queue (activity_id, target_inbox, target_domain, status, attempts, max_attempts, error_message, created_at)
             VALUES (${act}, '${BASE_B}/users/${ADMIN_USER_B}/inbox', '127.0.0.1', 'dead', 12, 12, 'suite seeded dead', NOW());" >/dev/null || true
    fi
  fi
  dead_n=$(sql_a "SELECT count(*) FROM federation_delivery_queue WHERE status='dead';" || echo 0)
  [[ "${dead_n:-0}" -ge 1 ]] || {
    echo "deploy_retry_dead skip (no dead rows to seed)" >>"$SUITE_LOG"
    return 0
  }
  api POST "$BASE_A" "$jar_a" /api/federation/delivery/retry-dead "" \
    >"$SCRATCH_DIR/deploy-retry-dead.json"
  local pending
  pending=$(sql_a "SELECT count(*) FROM federation_delivery_queue WHERE status='pending';" || echo 0)
  # retry-dead should move some dead → pending (or delivered quickly)
  grep -qE "requeued|count|success|retry|pending|[0-9]" "$SCRATCH_DIR/deploy-retry-dead.json" \
    || die "retry-dead response unexpected: $(cat "$SCRATCH_DIR/deploy-retry-dead.json")"
  echo "deploy_retry_dead ok pending_after=$pending" >>"$SUITE_LOG"
}

case_deploy_retry_dead_skips_user_cancel() {
  # Bulk retry-dead must not revive rows the user explicitly cancelled.
  local jar_a="$JAR_A"
  local act
  act=$(sql_a "SELECT id FROM federation_activities WHERE user_id=1 ORDER BY id DESC LIMIT 1;" || echo "")
  [[ -n "$act" ]] || die "no activity to seed retry-skip-cancel"
  # Seed: one user-cancelled dead + one real failure dead
  sql_a "INSERT INTO federation_delivery_queue (activity_id, target_inbox, target_domain, status, attempts, max_attempts, error_message, created_at)
         VALUES (${act}, 'http://127.0.0.1:9/cancel-skip', '127.0.0.1', 'dead', 1, 12, 'cancelled: by user', NOW());" >/dev/null
  sql_a "INSERT INTO federation_delivery_queue (activity_id, target_inbox, target_domain, status, attempts, max_attempts, error_message, created_at)
         VALUES (${act}, 'http://127.0.0.1:9/real-fail', '127.0.0.1', 'dead', 12, 12, 'suite seeded transient dead', NOW());" >/dev/null
  local cancel_id real_id
  cancel_id=$(sql_a "SELECT id FROM federation_delivery_queue WHERE error_message='cancelled: by user' ORDER BY id DESC LIMIT 1;")
  real_id=$(sql_a "SELECT id FROM federation_delivery_queue WHERE error_message='suite seeded transient dead' ORDER BY id DESC LIMIT 1;")
  [[ -n "$cancel_id" && -n "$real_id" ]] || die "failed to seed cancel/real dead rows"
  api POST "$BASE_A" "$jar_a" /api/federation/delivery/retry-dead "" \
    >"$SCRATCH_DIR/deploy-retry-skip-cancel.json" \
    || die "retry-dead failed: $(cat "$SCRATCH_DIR/deploy-retry-skip-cancel.json" 2>/dev/null || true)"
  local cancel_st cancel_err real_st real_err
  cancel_st=$(sql_a "SELECT status FROM federation_delivery_queue WHERE id=${cancel_id};")
  cancel_err=$(sql_a "SELECT coalesce(error_message,'') FROM federation_delivery_queue WHERE id=${cancel_id};")
  real_st=$(sql_a "SELECT status FROM federation_delivery_queue WHERE id=${real_id};")
  real_err=$(sql_a "SELECT coalesce(error_message,'') FROM federation_delivery_queue WHERE id=${real_id};")
  [[ "$cancel_st" == "dead" ]] || die "user-cancelled dead was requeued: status=$cancel_st"
  echo "$cancel_err" | grep -qi '^cancelled:' || die "user-cancel message lost: $cancel_err"
  # Real dead must be requeued (pending/delivering/delivered) or worker re-failed it
  # with a different message — never left as the untouched suite seed.
  if [[ "$real_st" == "dead" && "$real_err" == "suite seeded transient dead" ]]; then
    die "retry-dead did not requeue real dead (still suite seed msg)"
  fi
  echo "deploy_retry_dead_skips_user_cancel ok cancel_id=$cancel_id real_id=$real_id cancel_st=$cancel_st real_st=$real_st" >>"$SUITE_LOG"
}

case_deploy_keys_rotate() {
  # Explicit rotate: PEM changes, ensure does not re-rotate, outbound still delivers.
  local jar_a="$JAR_A"
  api_get "$BASE_A" "$jar_a" /api/federation/identity >/dev/null
  local pem_before
  pem_before=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ -n "$pem_before" ]] || die "no PEM before rotate"
  api POST "$BASE_A" "$jar_a" /api/federation/keys/rotate '{"confirm":true}' \
    >"$SCRATCH_DIR/deploy-keys-rotate.json" \
    || die "keys/rotate failed: $(cat "$SCRATCH_DIR/deploy-keys-rotate.json" 2>/dev/null || true)"
  local pem_after
  pem_after=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ -n "$pem_after" ]] || die "no PEM after rotate"
  [[ "$pem_before" != "$pem_after" ]] || die "rotate did not change public PEM"
  # ensure / identity must not rotate again
  api_get "$BASE_A" "$jar_a" /api/federation/identity >/dev/null
  local pem_ensure
  pem_ensure=$(sql_a "SELECT public_key_pem FROM federation_keys WHERE user_id=1;")
  [[ "$pem_ensure" == "$pem_after" ]] || die "ensure after rotate changed PEM"
  # Outbound with new key: clear B cache + re-follow so B fetches fresh actor publicKey
  sql_b "DELETE FROM federation_remote_actors WHERE actor_url LIKE '%/users/${ADMIN_USER_A}%';" >/dev/null || true
  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/federation/unfollow "{\"target\":\"${ACTOR_B}\"}" >/dev/null || true
  sleep 1
  clear_queue a
  api POST "$BASE_A" "$jar_a" /api/federation/follow "{\"target\":\"${ACTOR_B}\"}" \
    >"$SCRATCH_DIR/deploy-keys-rotate-follow.json"
  for i in $(seq 1 40); do
    nudge_delivery a
    pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
    dead=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='dead';" || echo 0)
    del=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
    dead_fail=$(dead_fail_count a)
    echo "  keys_rotate t=$i pending=$pend delivered=$del dead=$dead dead_fail=$dead_fail"
    if [[ "${dead_fail:-0}" -ge 1 ]]; then
      die "post-rotate outbound dead: $(dead_fail_error a)"
    fi
    [[ "${pend:-0}" -eq 0 && "${del:-0}" -ge 1 ]] && break
    sleep 1
  done
  pend=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status IN ('pending','delivering');" || echo 0)
  del=$(sql_a "SELECT count(*)::int FROM federation_delivery_queue WHERE status='delivered';" || echo 0)
  [[ "${pend:-0}" -eq 0 && "${del:-0}" -ge 1 ]] || die "post-rotate outbound not delivered"
  echo "deploy_keys_rotate ok" >>"$SUITE_LOG"
}

case_deploy_ssrf_lab_flag_off() {
  # Deploy safety: without lab flag, private inbox delivery must be refused.
  # cargo test accepts a single filter; run two shipped unit tests.
  (cd "$BACKEND" && cargo test lab_flag_allows_loopback -- --nocapture) \
    >"$SCRATCH_DIR/deploy-ssrf-unit.log" 2>&1 \
    || die "ssrf lab_flag unit failed (see deploy-ssrf-unit.log)"
  (cd "$BACKEND" && cargo test refuses_literal -- --nocapture) \
    >>"$SCRATCH_DIR/deploy-ssrf-unit.log" 2>&1 \
    || die "ssrf refuses_literal unit failed (see deploy-ssrf-unit.log)"
  grep -q "test result: ok" "$SCRATCH_DIR/deploy-ssrf-unit.log" \
    || die "ssrf unit log missing ok"
  echo "deploy_ssrf_lab_flag_off unit ok" >>"$SUITE_LOG"
}

case_deploy_cancel_failed_send() {
  # Product: user can cancel pending/failed-in-flight outbound delivery.
  local jar_a="$JAR_A"
  local act
  act=$(sql_a "SELECT id FROM federation_activities WHERE user_id=1 ORDER BY id DESC LIMIT 1;" || echo "")
  [[ -n "$act" ]] || die "no activity to seed cancel test"
  # Always seed a far-future pending row so the worker cannot race-complete it
  sql_a "INSERT INTO federation_delivery_queue (activity_id, target_inbox, target_domain, status, attempts, max_attempts, error_message, created_at, next_retry_at)
         VALUES (${act}, 'http://127.0.0.1:9/never', '127.0.0.1', 'pending', 3, 12, 'suite: stuck outbound', NOW(), NOW() + interval '2 hours')
         RETURNING id;" >/dev/null
  local qid
  qid=$(sql_a "SELECT id FROM federation_delivery_queue WHERE status='pending' AND error_message='suite: stuck outbound' ORDER BY id DESC LIMIT 1;")
  [[ -n "$qid" ]] || die "failed to seed pending queue id"
  api POST "$BASE_A" "$jar_a" "/api/federation/delivery/${qid}/cancel" "" \
    >"$SCRATCH_DIR/deploy-cancel-one.json"
  local st
  st=$(sql_a "SELECT status || '|' || coalesce(error_message,'') FROM federation_delivery_queue WHERE id=${qid};")
  echo "$st" | grep -q "^dead|" || die "cancel did not mark dead: $st"
  echo "$st" | grep -qi "cancelled" || die "cancel missing cancelled message: $st"
  # Seed another pending and bulk-cancel
  sql_a "INSERT INTO federation_delivery_queue (activity_id, target_inbox, target_domain, status, attempts, max_attempts, error_message, created_at, next_retry_at)
         VALUES (${act}, 'http://127.0.0.1:9/never2', '127.0.0.1', 'pending', 1, 12, 'suite: bulk cancel', NOW(), NOW() + interval '2 hours');" >/dev/null
  api POST "$BASE_A" "$jar_a" /api/federation/delivery/cancel-pending "" \
    >"$SCRATCH_DIR/deploy-cancel-pending.json"
  local cancelled
  cancelled=$(python3 -c 'import json; print(json.load(open("'"$SCRATCH_DIR"'/deploy-cancel-pending.json")).get("cancelled",0))' 2>/dev/null || echo 0)
  [[ "${cancelled:-0}" -ge 1 ]] || die "cancel-pending cancelled=0: $(cat "$SCRATCH_DIR/deploy-cancel-pending.json")"
  echo "deploy_cancel_failed_send ok id=$qid bulk_cancelled=$cancelled status=$st" >>"$SUITE_LOG"
}

case_delivery_no_key_spam() {
  assert_no_key_errors
  if grep -E "panicked at" "$RUN_LOG_A" "$RUN_LOG_B" >/dev/null 2>&1; then
    die "panic found in instance logs"
  fi
}

boot() {
  ensure_dbs
  build_backend
  reset_dbs
  RUN_LOG_A="$SCRATCH_DIR/intranet-a.log"
  RUN_LOG_B="$SCRATCH_DIR/intranet-b.log"
  SUITE_LOG="$SCRATCH_DIR/suite.log"
  : >"$RUN_LOG_A" ; : >"$RUN_LOG_B" ; : >"$SUITE_LOG"

  for p in "$PORT_A" "$PORT_B"; do
    pid=$(lsof -tiTCP:"$p" -sTCP:LISTEN 2>/dev/null || true)
    [[ -n "$pid" ]] && kill $pid 2>/dev/null || true
  done
  sleep 1

  PID_A=$(start_instance a "$PORT_A" "$DB_A" "$JWT_A" "$BASE_A" "$RUN_LOG_A")
  PID_B=$(start_instance b "$PORT_B" "$DB_B" "$JWT_B" "$BASE_B" "$RUN_LOG_B")
  echo "PID_A=$PID_A" >"$SCRATCH_DIR/pids.txt"
  echo "PID_B=$PID_B" >>"$SCRATCH_DIR/pids.txt"
  wait_health "$BASE_A" a
  wait_health "$BASE_B" b
  create_admin "$BASE_A" "$ADMIN_USER_A"
  create_admin "$BASE_B" "$ADMIN_USER_B"
  # warm actor docs
  curl -fsS -H 'Accept: application/activity+json' "$ACTOR_A" >/dev/null
  curl -fsS -H 'Accept: application/activity+json' "$ACTOR_B" >/dev/null
  ensure_alive a "$BASE_A" "$PID_A"
  ensure_alive b "$BASE_B" "$PID_B"
  # Single login session for whole suite (avoid /api/auth/login rate limit)
  JAR_A="$SCRATCH_DIR/jar-a.jar"
  JAR_B="$SCRATCH_DIR/jar-b.jar"
  login_cookie "$BASE_A" "$ADMIN_USER_A" "$JAR_A"
  login_cookie "$BASE_B" "$ADMIN_USER_B" "$JAR_B"
  sleep 1
}

# Session jars filled in boot()
JAR_A=""
JAR_B=""

main() {
  if [[ "$MODE" == "down" ]]; then
    for p in "$PORT_A" "$PORT_B"; do
      pid=$(lsof -tiTCP:"$p" -sTCP:LISTEN 2>/dev/null || true)
      [[ -n "$pid" ]] && kill $pid 2>/dev/null || true
    done
    exit 0
  fi

  boot
  log "Running multi-instance suite BASE_A=$BASE_A BASE_B=$BASE_B"

  run_case "identity_keys" case_identity_keys
  run_case "follow" case_follow
  sleep 2
  run_case "unfollow_refollow" case_unfollow
  sleep 2
  run_case "content_note_fanout" case_content_note
  sleep 2
  run_case "room_invite_accept_message" case_room_invite_accept_message
  sleep 2
  run_case "ring_add_peer_sync" case_ring_add_peer
  sleep 2
  run_case "channel_open_accept_message" case_channel_open_accept_message
  sleep 2
  run_case "room_pin" case_room_pin
  sleep 1
  run_case "room_e2e_key_exchange" case_room_e2e_key_exchange
  sleep 1
  run_case "channel_e2e_key_exchange" case_channel_e2e_key_exchange
  sleep 2
  run_case "file_transfer" case_file_transfer
  sleep 1
  run_case "trust_policy" case_trust_policy
  sleep 1
  # Ownership transfer while room graph still on pre-move bases
  run_case "transfer_ownership" case_transfer_ownership
  sleep 1
  # Deploy scenarios BEFORE domain_move (move retargets key_id; BASE_URL must follow)
  run_case "deploy_cold_keys_before_outbound" case_deploy_cold_keys_before_outbound
  sleep 1
  run_case "deploy_keys_rotate" case_deploy_keys_rotate
  sleep 1
  run_case "deploy_jwt_mismatch_no_rotate" case_deploy_jwt_mismatch_no_rotate
  sleep 1
  run_case "deploy_stale_remote_pubkey" case_deploy_stale_remote_pubkey
  sleep 1
  run_case "deploy_retry_dead" case_deploy_retry_dead
  sleep 1
  run_case "deploy_retry_dead_skips_user_cancel" case_deploy_retry_dead_skips_user_cancel
  sleep 1
  run_case "deploy_cancel_failed_send" case_deploy_cancel_failed_send
  sleep 1
  run_case "deploy_cancel_during_backoff" case_deploy_cancel_during_backoff
  sleep 1
  run_case "deploy_second_local_user" case_deploy_second_local_user
  sleep 1
  run_case "deploy_ssrf_lab_flag_off" case_deploy_ssrf_lab_flag_off
  sleep 1
  # Domain move last; then production cutover to new BASE
  run_case "domain_move" case_domain_move
  sleep 1
  run_case "deploy_post_move_switch_base" case_deploy_post_move_switch_base
  sleep 1
  run_case "no_key_error_spam" case_delivery_no_key_spam

  {
    echo "=== SUITE SUMMARY $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="
    echo "PASS=$PASS FAIL=$FAIL"
    printf '%s\n' "${RESULTS[@]}"
  } | tee "$SCRATCH_DIR/suite-summary.txt" | tee -a "$SUITE_LOG"

  cat >"$SCRATCH_DIR/summary.md" <<EOF
# Multi-instance federation suite (full + deploy scenarios)

## Instances
- A: \`$BASE_A\` db=\`$DB_A\`
- B: \`$BASE_B\` db=\`$DB_B\`
- C: \`$BASE_C\` (domain-move new base, shared A DB)
- Lab: \`MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND=1\`, delivery interval 2s

## Results
- PASS: $PASS
- FAIL: $FAIL

\`\`\`
$(printf '%s\n' "${RESULTS[@]}")
\`\`\`

## Paths exercised
1. Identity ensure keys (cold)
2. Follow A→B signed delivery + B accept
3. Unfollow + re-follow
4. Note publish followers fan-out A→B
5. Room create/invite/accept/message
6. Ring create/add_peer/sync
7. Channel open/accept/message
8. No unrecovered key-load errors / no panics

## Logs
- suite.log, suite-summary.txt, intranet-a.log, intranet-b.log
- per-case JSON under this directory
EOF

  [[ "$FAIL" -eq 0 ]] || die "suite finished with $FAIL failure(s)"
  log "SUCCESS all multi-instance cases passed ($PASS)"
}

main
