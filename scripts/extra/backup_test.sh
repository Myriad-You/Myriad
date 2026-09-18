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

echo "backup.sh helpers ok"
