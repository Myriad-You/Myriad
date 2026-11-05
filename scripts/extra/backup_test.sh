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

echo "backup.sh helpers ok"
