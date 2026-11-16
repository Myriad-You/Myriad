#!/usr/bin/env bash
# Tests for scripts/extra/backup.sh fail-closed backup/restore helpers.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=backup.sh
source "$ROOT/scripts/extra/backup.sh"

pass() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1" >&2; exit 1; }

echo "== SCRIPT-001: partial quiesce sets WRITERS_STOPPED_BY_US immediately"

service_is_running() {
    case "$1" in
        federation-worker|persona-worker) return 0 ;;
        *) return 1 ;;
    esac
}
compose() {
    if [ "$1" = stop ] && [ "$2" = persona-worker ]; then
        return 1
    fi
    return 0
}

WRITERS_STOPPED_BY_US=0
STOPPED_WRITERS=""
code=0
quiesce_writers || code=$?
[ "$code" -ne 0 ] || fail "quiesce must fail when a later stop fails"
[ "$WRITERS_STOPPED_BY_US" -eq 1 ] || fail "already-stopped writers must be marked for restart"
[ "$STOPPED_WRITERS" = "federation-worker" ] || fail "only successfully stopped units are listed, got '$STOPPED_WRITERS'"
pass "partial quiesce failure keeps writers eligible for restart"

echo "== SCRIPT-002: cleanup failure refuses unpack"

tmp="$(mktemp -d -t myriad-backup-test-XXXXXX)"
trap 'chmod -R u+w "$tmp" 2>/dev/null || true; rm -rf "$tmp"' EXIT

dest="$tmp/data"
src="$tmp/src"
mkdir -p "$dest/keep" "$src"
printf 'old\n' > "$dest/keep/old.txt"
printf 'new\n' > "$src/new.txt"
archive="$tmp/backend_data.tar.gz"
tar czf "$archive" -C "$src" .

chmod a-w "$dest/keep" "$dest"
set +e
restore_data_tree "$dest" "$archive"
unpack_code=$?
set -e
chmod -R u+w "$dest"
[ "$unpack_code" -ne 0 ] || fail "unpack must refuse when cleanup cannot empty dest"
[ ! -f "$dest/new.txt" ] || fail "archive must not be mixed onto leftover dest"
pass "failed cleanup does not unpack"

mkdir -p "$tmp/ok"
printf 'stale\n' > "$tmp/ok/stale.txt"
restore_data_tree "$tmp/ok" "$archive"
[ -f "$tmp/ok/new.txt" ] || fail "successful restore must unpack archive"
[ ! -f "$tmp/ok/stale.txt" ] || fail "successful restore must not keep leftover files"
pass "successful restore replaces dest"

echo "== SCRIPT-003: the data key's fingerprint names it without holding it"

secret="$(printf 'k%.0s' $(seq 1 32) | base64)"
printf '%s\n' "$secret" > "$tmp/key-a"
printf '  %s  ' "$secret" > "$tmp/key-b"
: > "$tmp/key-empty"
print_a="$(data_key_fingerprint "$tmp/key-a")"
[ "$print_a" = "$(data_key_fingerprint "$tmp/key-b")" ] || fail "whitespace around the key must not change its fingerprint"
[ "${#print_a}" -eq 16 ] || fail "fingerprint is 16 hex characters, got '$print_a'"
case "$secret" in *"$print_a"*) fail "fingerprint must not be part of the key" ;; esac
[ "$(data_key_fingerprint "$tmp/key-empty")" = none ] || fail "no key has the fingerprint none"
[ "$(data_key_fingerprint "$tmp/missing")" = none ] || fail "a missing key file has the fingerprint none"
pass "fingerprint is stable, short and not the key"

echo "== SCRIPT-004: the keys part never goes inside a data part"

require_keys_outside "$tmp/backups/one/keys" "$tmp/backups/one" >/dev/null 2>&1 \
    && fail "keys inside the data part must be refused"
require_keys_outside "$tmp/backups/one" "$tmp/backups/one" >/dev/null 2>&1 \
    && fail "keys equal to the data part must be refused"
require_keys_outside "$tmp/backup-keys" "$tmp/backups/one" || fail "keys beside the backups are fine"
require_keys_outside "$tmp/backups/one-keys" "$tmp/backups/one" || fail "a sibling with a common prefix is not inside"
pass "keys part placement"

echo "== SCRIPT-005: a keys part must match its data part"

make_parts() {
    local data="$1" keys="$2" project="$3" print="$4" created="$5"
    mkdir -p "$data" "$keys"
    printf 'format=2\npart=data\ncreated=2026-11-05T10:00:00Z\nproject=myriad\ndata_key_fingerprint=%s\n' \
        "$print" > "$data/MANIFEST.txt"
    printf '%s\n' "$secret" > "$keys/data-key"
    printf 'POSTGRES_PASSWORD=x\n' > "$keys/env"
    for t in $HER_LOG_KEY_TABLES; do : > "$keys/$t.copy"; done
    printf 'format=2\npart=keys\ncreated=%s\nproject=%s\ndata_key_source=file\ndata_key_fingerprint=%s\n' \
        "$created" "$project" "$(data_key_fingerprint "$keys/data-key")" > "$keys/MANIFEST.txt"
}
refused() { ( "$@" ) >/dev/null 2>&1 && return 1; return 0; }

make_parts "$tmp/m/ok" "$tmp/m/ok-keys" myriad "$print_a" 2026-11-05T11:00:00Z
( require_matching_keys "$tmp/m/ok" "$tmp/m/ok-keys" ) >/dev/null 2>&1 || fail "matching parts must pass"

make_parts "$tmp/m/other" "$tmp/m/other-keys" myriad 0123456789abcdef 2026-11-05T11:00:00Z
refused require_matching_keys "$tmp/m/other" "$tmp/m/other-keys" || fail "a different data key must be refused"

make_parts "$tmp/m/proj" "$tmp/m/proj-keys" elsewhere "$print_a" 2026-11-05T11:00:00Z
refused require_matching_keys "$tmp/m/proj" "$tmp/m/proj-keys" || fail "a different project must be refused"

make_parts "$tmp/m/edit" "$tmp/m/edit-keys" myriad "$print_a" 2026-11-05T11:00:00Z
printf 'other\n' > "$tmp/m/edit-keys/data-key"
refused require_matching_keys "$tmp/m/edit" "$tmp/m/edit-keys" || fail "a key that is not its manifest's must be refused"

make_parts "$tmp/m/gone" "$tmp/m/gone-keys" myriad "$print_a" 2026-11-05T11:00:00Z
rm "$tmp/m/gone-keys/agent_log_object_keys.copy"
refused require_matching_keys "$tmp/m/gone" "$tmp/m/gone-keys" || fail "a keys part missing a file must be refused"

make_parts "$tmp/m/fresh" "$tmp/m/fresh-keys" myriad none 2026-11-05T11:00:00Z
( require_matching_keys "$tmp/m/fresh" "$tmp/m/fresh-keys" ) >/dev/null 2>&1 \
    || fail "a data part made before any data key takes the keys part's"

make_parts "$tmp/m/old" "$tmp/m/old-keys" myriad "$print_a" 2026-11-05T09:00:00Z
warned="$( ( require_matching_keys "$tmp/m/old" "$tmp/m/old-keys" ) 2>&1 )" || fail "an older keys part warns, not refuses"
case "$warned" in *older*) ;; *) fail "an older keys part must warn" ;; esac

all="$( ( for d in other proj edit gone; do require_matching_keys "$tmp/m/$d" "$tmp/m/$d-keys"; done ) 2>&1 || true)"
case "$all" in *"$secret"*) fail "refusals must not print the key" ;; esac
pass "mismatched keys parts are refused without printing the key"

echo "== SCRIPT-006: a split data part restores with the keys part where backup put it, never without one"

: > "$tmp/m/ok/postgres.dump"; : > "$tmp/m/ok/backend_data.tar.gz"
msg="$( ( BACKUP_KEYS_DIR="$tmp/m/nowhere" do_restore --from "$tmp/m/ok" ) 2>&1 || true)"
case "$msg" in *"no keys part at $tmp/m/nowhere"*"--keys DIR"*) ;; *) fail "restore must say the keys part is missing and how to give it, got: $msg" ;; esac
# Found at BACKUP_KEYS_DIR without --keys: the keys checks pass and restore
# goes on to the next refusal (the live .env), never to "no keys part".
msg="$( ( BACKUP_KEYS_DIR="$tmp/m/ok-keys" do_restore --from "$tmp/m/ok" ) 2>&1 || true)"
case "$msg" in *"keys part: $tmp/m/ok-keys"*) ;; *) fail "restore must take the keys part from BACKUP_KEYS_DIR, got: $msg" ;; esac
case "$msg" in *"no keys part"*) fail "a keys part at BACKUP_KEYS_DIR must be found" ;; esac
# A keys part found there is still checked against the data part.
msg="$( ( BACKUP_KEYS_DIR="$tmp/m/proj-keys" do_restore --from "$tmp/m/ok" ) 2>&1 || true)"
case "$msg" in *"different compose projects"*) ;; *) fail "a keys part found by default must still match, got: $msg" ;; esac
mkdir -p "$tmp/m/legacy"
: > "$tmp/m/legacy/postgres.dump"; : > "$tmp/m/legacy/backend_data.tar.gz"
printf 'project=myriad\n' > "$tmp/m/legacy/MANIFEST.txt"
msg="$( ( do_restore --from "$tmp/m/legacy" --keys "$tmp/m/ok-keys" ) 2>&1 || true)"
case "$msg" in *"drop --keys"*) ;; *) fail "an unsplit backup must refuse --keys, got: $msg" ;; esac
pass "restore finds the keys part it needs or refuses"

echo "== SCRIPT-007: a local deploy's DATABASE_URL reaches libpq without the password showing"

password='pa:ss@w\ord'
mkdir -p "$tmp/l/tmpd"
printf 'DATABASE_URL=postgres://my%%40role:%s@db.example:6543/my%%2Ddb?sslmode=disable\n' 'pa%3Ass%40w%5Cord' > "$tmp/l/url.env"
(
    export TMPDIR="$tmp/l/tmpd"
    PGPASSWORD=leftover
    local_db_setup "$tmp/l/url.env"
    [ "$PGHOST" = db.example ] || fail "host, got '$PGHOST'"
    [ "$PGPORT" = 6543 ] || fail "port, got '$PGPORT'"
    [ "$PGUSER" = "my@role" ] || fail "user is percent-decoded, got '$PGUSER'"
    [ "$PGDATABASE" = my-db ] && [ "$LOCAL_DB" = my-db ] && [ "$LOCAL_LIVE_DB" = my-db ] || fail "database, got '$PGDATABASE'"
    [ -z "${PGPASSWORD:-}" ] || fail "the password must not sit in the environment"
    [ "$(cat "$PGPASSFILE")" = 'db.example:6543:*:my@role:pa\:ss@w\\ord' ] || fail "password file line is escaped for libpq"
    [ "$(stat -f %Lp "$PGPASSFILE" 2>/dev/null || stat -c %a "$PGPASSFILE")" = 600 ] || fail "password file is private"
    env | grep -qF "$password" && fail "the password must not be in the environment"
    local_cleanup
    [ ! -e "$PGPASSFILE" ] || fail "cleanup removes the password file"
)
printf 'DATABASE_URL=mysql://u:%s@h/db\n' "$password" > "$tmp/l/bad.env"
msg="$( ( local_db_setup "$tmp/l/bad.env" ) 2>&1 || true)"
case "$msg" in *"not a postgres:// URL"*) ;; *) fail "a non-postgres URL is refused, got: $msg" ;; esac
case "$msg" in *"$password"*|*u:*) fail "the refusal must not print the URL" ;; esac
msg="$( ( local_db_setup "$tmp/l/missing.env" ) 2>&1 || true)"
case "$msg" in *"DATABASE_URL is missing"*) ;; *) fail "a missing DATABASE_URL is refused, got: $msg" ;; esac
[ -z "$(ls -A "$tmp/l/tmpd")" ] || fail "no password file is left behind"
pass "URL parsed, password only in a private file that is removed"

echo "== SCRIPT-008: a local backup splits into a data part and a keys part"

site="$tmp/site/backend"
mkdir -p "$site/data/media" "$site/data/sub"
printf 'DATABASE_URL=postgres://myriad:%s@localhost:5432/myriad\nJWT_SECRET=jwt-%s\nSERVER_PORT=1103\n' "$password" "$secret" > "$site/.env"
printf '%s\n' "$secret" > "$site/data/.secret-key"
printf 'half\n' > "$site/data/..secret-key.tmp-1"
printf 'png\n' > "$site/data/media/a.png"
printf 'nested\n' > "$site/data/sub/.secret-key"

psql() {
    local a
    printf '%s\n' "$*" >> "$tmp/psql.args"
    for a in "$@"; do
        case "$a" in
            *to_regclass*) echo t; return 0 ;;
            *pg_database_size*) echo 1; return 0 ;;
            *"FROM pg_tables"*) echo "${FAKE_TABLES:-0}"; return 0 ;;
            *"TO STDOUT"*) printf '1\tall\tsealed-row\t2026-11-12 00:00:00+00\n'; return 0 ;;
        esac
    done
    cat >> "$tmp/psql.stdin"
}
pg_dump() {
    local a out=""
    printf '%s\n' "$*" > "$tmp/pg_dump.args"
    while [ $# -gt 0 ]; do
        case "$1" in -f) out="$2"; shift 2 ;; *) shift ;; esac
    done
    printf 'DUMP\n' > "$out"
}
pg_restore() { printf '%s\n' "$*" > "$tmp/pg_restore.args"; }
tmutil() { :; }

local_env() {
    BACKUP_MODE=local LOCAL_BACKEND_DIR="$site" BACKUP_OUT="$tmp/bk/data" \
        BACKUP_KEYS_DIR="$tmp/bk/keys" BACKUP_MIN_FREE_GB=0 TMPDIR="$tmp/l/tmpd" "$@"
}
log="$( ( local_env do_backup ) 2>&1)" || fail "local backup failed: $log"
case "$log" in *"$secret"*|*"$password"*) fail "backup output must not hold a secret" ;; esac
part="$(ls -d "$tmp/bk/data"/myriad-*)"
[ "$(printf '%s\n' "$part" | wc -l | tr -d ' ')" = 1 ] || fail "one data part, got: $part"
case "$part" in *.partial) fail "the data part is renamed when whole" ;; esac
[ "$(manifest_value "$part/MANIFEST.txt" deploy)" = local ] || fail "data manifest names the local deploy"
[ "$(manifest_value "$part/MANIFEST.txt" format)" = 2 ] || fail "data manifest format 2"
[ "$(manifest_value "$part/MANIFEST.txt" data_key_fingerprint)" = "$print_a" ] || fail "data manifest carries the key's fingerprint"
[ ! -e "$part/env" ] || fail "the data part holds no .env"
grep -q 'exclude-table-data=public.agent_log_audience_keys' "$tmp/pg_dump.args" \
    && grep -q 'exclude-table-data=public.agent_log_object_keys' "$tmp/pg_dump.args" \
    || fail "the dump leaves out her log's key rows"
listing="$(tar -tzf "$part/backend_data.tar.gz")"
case "$listing" in *"./media/a.png"*) ;; *) fail "data files are archived" ;; esac
case "$listing" in *"./sub/.secret-key"*) ;; *) fail "only the top-level data key is left out" ;; esac
printf '%s\n' "$listing" | grep -qx './.secret-key' && fail "the data key stays out of the data part"
printf '%s\n' "$listing" | grep -q 'secret-key.tmp' && fail "a half-written data key stays out of the data part"
tar -xzOf "$part/backend_data.tar.gz" | grep -qF "$secret" && fail "no data key bytes in the archive"
keys="$tmp/bk/keys"
cmp -s "$keys/env" "$site/.env" || fail "the keys part holds the deploy .env"
cmp -s "$keys/data-key" "$site/data/.secret-key" || fail "the keys part holds the data key"
[ "$(manifest_value "$keys/MANIFEST.txt" project)" = local ] || fail "keys manifest names the local deploy"
[ "$(manifest_value "$keys/MANIFEST.txt" data_key_source)" = file ] || fail "the data key came from the file"
grep -q sealed-row "$keys/agent_log_audience_keys.copy" || fail "her log's key rows are in the keys part"
for f in env data-key MANIFEST.txt agent_log_audience_keys.copy agent_log_object_keys.copy; do
    [ "$(stat -f %Lp "$keys/$f" 2>/dev/null || stat -c %a "$keys/$f")" = 600 ] || fail "$f is private"
done
ls -d "$tmp/bk/keys".* >/dev/null 2>&1 && fail "no lock or half-written keys part is left"
[ -z "$(ls -A "$tmp/l/tmpd")" ] || fail "the password file is removed after a backup"
pass "data part has no key or secret; keys part has all of them"

mkdir -p "$tmp/p"
for d in 20261101_000000 20261102_000000 20261103_000000 20261104_000000; do
    mkdir "$tmp/p/myriad-$d"
    printf 'part=data\n' > "$tmp/p/myriad-$d/MANIFEST.txt"
done
mkdir "$tmp/p/myriad-20261105_000000.partial" "$tmp/p/myriad-hand-made"
prune_data_parts "$tmp/p" 2 >/dev/null
[ "$(cd "$tmp/p" && echo myriad-*)" = "myriad-20261103_000000 myriad-20261104_000000 myriad-20261105_000000.partial myriad-hand-made" ] \
    || fail "prune keeps the newest finished parts and nothing else is touched, got: $(cd "$tmp/p" && echo myriad-*)"
refused prune_data_parts "$tmp/p" 0 || fail "keeping no data part is refused"
pass "old data parts are pruned"

echo "== SCRIPT-009: a local restore checks where it writes"

live_env_sum="$(cksum < "$site/.env")"
msg="$( ( local_env do_restore --from "$part" --db scratch ) 2>&1 || true)"
case "$msg" in *"needs both --db and --data-dir"*) ;; *) fail "half a beside restore is refused, got: $msg" ;; esac
msg="$( ( local_env do_restore --from "$part" --db myriad --data-dir "$tmp/r" ) 2>&1 || true)"
case "$msg" in *"names the live database"*) ;; *) fail "the live database is not a beside target, got: $msg" ;; esac
msg="$( ( local_env do_restore --from "$part" --db scratch --data-dir "$site/data/../data" ) 2>&1 || true)"
case "$msg" in *"live data directory"*) ;; *) fail "the live data is not a beside target, got: $msg" ;; esac
msg="$( ( FAKE_TABLES=3 local_env do_restore --from "$part" --db scratch --data-dir "$tmp/r" ) 2>&1 || true)"
case "$msg" in *"already has tables"*) ;; *) fail "a beside restore needs an empty database, got: $msg" ;; esac
msg="$( ( port_listening() { return 0; }; local_env do_restore --from "$part" ) 2>&1 || true)"
case "$msg" in *"listens on port 1103"*) ;; *) fail "a live restore waits for the backend to stop, got: $msg" ;; esac
msg="$( ( BACKUP_MODE=local do_restore --from "$tmp/m/ok" ) 2>&1 || true)"
case "$msg" in *"not a data part of a local deploy"*) ;; *) fail "a compose data part is refused, got: $msg" ;; esac
printf 'DATABASE_URL=postgres://other@localhost/other\n' > "$tmp/l/other.env"
mkdir -p "$tmp/other-site"; cp "$tmp/l/other.env" "$tmp/other-site/.env"
msg="$( ( port_listening() { return 1; }; local_env do_restore --from "$part" ) 2>&1 || true)"
case "$msg" in *"restore complete"*) ;; *) fail "live restore with a stopped backend, got: $msg" ;; esac
msg="$( ( port_listening() { return 1; }; BACKUP_MODE=local LOCAL_BACKEND_DIR="$tmp/other-site" BACKUP_KEYS_DIR="$keys" \
    do_restore --from "$part" ) 2>&1 || true)"
case "$msg" in *"another DATABASE_URL"*) ;; *) fail "a live restore keeps the live DATABASE_URL, got: $msg" ;; esac
[ "$(cksum < "$site/.env.bak.restore")" = "$live_env_sum" ] || fail "the previous .env is kept"
pass "beside and live targets are checked before anything is written"

rm -f "$tmp/psql.stdin"
log="$( ( local_env do_restore --from "$part" --db scratch --data-dir "$tmp/r" ) 2>&1)" || fail "beside restore failed: $log"
case "$log" in *"$secret"*|*"$password"*) fail "restore output must not hold a secret" ;; esac
grep -q -- '--single-transaction' "$tmp/pg_restore.args" && grep -q -- '-d scratch' "$tmp/pg_restore.args" \
    || fail "the dump goes into the beside database in one transaction"
grep -q 'INSERT INTO agent_log_audience_keys' "$tmp/psql.stdin" && grep -q sealed-row "$tmp/psql.stdin" \
    || fail "her log's key rows go into the restored database"
[ -f "$tmp/r/media/a.png" ] && [ -f "$tmp/r/sub/.secret-key" ] || fail "data files are restored"
cmp -s "$tmp/r/.secret-key" "$site/data/.secret-key" || fail "the data key is restored"
[ "$(stat -f %Lp "$tmp/r/.secret-key" 2>/dev/null || stat -c %a "$tmp/r/.secret-key")" = 600 ] || fail "the restored data key is private"
[ "$(cksum < "$site/.env")" = "$live_env_sum" ] || fail "a beside restore leaves the live .env alone"
[ -z "$(ls -A "$tmp/l/tmpd")" ] || fail "the password file is removed after a restore"
pass "a beside restore brings back the database, her log keys, data and data key"

echo "== SCRIPT-010: schedule writes only its own two launchd jobs"

if command -v plutil >/dev/null 2>&1 && type -P pg_dump >/dev/null 2>&1; then
    launchctl() { printf '%s\n' "$*" >> "$tmp/launchctl.calls"; }
    mkdir -p "$tmp/agents"
    printf 'other\n' > "$tmp/agents/com.example.other.plist"
    log="$( ( export LAUNCH_AGENTS_DIR="$tmp/agents" BACKUP_LOG_DIR="$tmp/logs" BACKUP_KEEP=5
        local_env do_local_schedule ) 2>&1)" || fail "schedule failed: $log"
    for label in com.myriad-you.backup-data com.myriad-you.backup-keys; do
        plutil -lint "$tmp/agents/$label.plist" >/dev/null || fail "$label plist is valid"
        grep -q "<string>local</string>" "$tmp/agents/$label.plist" || fail "$label runs the local mode"
        grep -q "<string>5</string>" "$tmp/agents/$label.plist" || fail "$label keeps BACKUP_KEEP"
        grep -qF "$secret" "$tmp/agents/$label.plist" && fail "$label holds no secret"
        grep -qF "$password" "$tmp/agents/$label.plist" && fail "$label holds no password"
        grep -q "bootstrap gui/$(id -u) $tmp/agents/$label.plist" "$tmp/launchctl.calls" || fail "$label is loaded"
    done
    grep -q '<string>backup</string>' "$tmp/agents/com.myriad-you.backup-data.plist" \
        && grep -q StartCalendarInterval "$tmp/agents/com.myriad-you.backup-data.plist" || fail "the data part runs daily"
    grep -q '<string>keys</string>' "$tmp/agents/com.myriad-you.backup-keys.plist" \
        && grep -q '<integer>3600</integer>' "$tmp/agents/com.myriad-you.backup-keys.plist" || fail "the keys part runs hourly"
    ( export LAUNCH_AGENTS_DIR="$tmp/agents"; local_env do_local_unschedule ) >/dev/null 2>&1 \
        || fail "unschedule failed"
    [ "$(ls "$tmp/agents")" = com.example.other.plist ] || fail "unschedule removes exactly its own plists"
    pass "two valid jobs, no secret in them, removed without touching others"
else
    echo "  - skipped (needs macOS plutil and pg_dump)"
fi

echo "backup.sh helpers ok"
