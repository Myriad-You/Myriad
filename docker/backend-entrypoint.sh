#!/bin/sh
set -eu

if [ "${MYRIAD_VOLUME_INIT_ONLY:-false}" = "true" ]; then
    if [ "$(id -u)" -ne 0 ]; then
        echo "backend volume initialization requires uid 0" >&2
        exit 1
    fi

    mkdir -p /app/cache /app/data
    chown -R --no-dereference myriad:myriad /app/cache /app/data
    # Ownership alone is insufficient when an older/root deployment left
    # directories without owner-write/search bits (for example 0555). Restore
    # only the owner's access; never broaden group/world permissions.
    chmod -R u+rwX /app/cache /app/data

    # Verify access as the same uid/gid used by the regular backend. Testing as
    # root would miss root_squash, ACL, read-only-mount and mode-bit failures.
    # Probe the volume roots plus every existing Tapp owner directory, because
    # staging files are created directly below data/tapps/<owner_id>.
    setpriv --reuid=1000 --regid=1000 --clear-groups sh -eu -c '
        umask 077
        probe_dir() {
            dir="$1"
            if [ -L "$dir" ]; then
                echo "refusing symlink in backend volume write probe: $dir" >&2
                exit 1
            fi
            [ -d "$dir" ] || return 0
            probe="$dir/.myriad-volume-write-probe-$$"
            (set -C; : > "$probe") || {
                echo "backend volume is not writable by uid 1000: $dir" >&2
                exit 1
            }
            rm -f -- "$probe"
        }
        probe_dir /app/cache
        probe_dir /app/data
        probe_dir /app/data/tapps
        for dir in /app/data/tapps/*; do
            [ -e "$dir" ] || [ -L "$dir" ] || continue
            [ -L "$dir" ] && {
                echo "refusing symlink in Tapp owner directory: $dir" >&2
                exit 1
            }
            owner_id="${dir##*/}"
            case "$owner_id" in
                ""|*[!0-9]*) continue ;;
            esac
            probe_dir "$dir"
        done
    '
    echo "backend volume ownership and write access verified for uid 1000"
    exit 0
fi

exec "$@"
