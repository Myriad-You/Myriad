#!/usr/bin/env bash
# Host-only provisioning. Compose treats these volumes as external so the updater
# can reuse them without permission to create/remove volumes through Docker Guard.

validate_backend_directory() {
    local directory="$1" parent physical
    case "$directory" in
        /*) ;;
        *) echo "Backend volume directory must be absolute: $directory" >&2; return 1 ;;
    esac
    parent="$(dirname "$directory")"
    physical="$(cd "$parent" && pwd -P)" || return 1
    # Compare the parent too: checking only the final component would allow a
    # symlinked ancestor (or ../) to redirect a privileged ownership repair.
    if [ "$parent" != "$physical" ] || [ -L "$directory" ] \
        || { [ -e "$directory" ] && [ ! -d "$directory" ]; }; then
        echo "Backend volume path must be a real directory below its physical parent: $directory" >&2
        return 1
    fi
    case "${directory##*/}" in
        ''|.|..) echo "Invalid backend volume directory: $directory" >&2; return 1 ;;
    esac
}

ensure_backend_volume() {
    local docker_bin="$1" volume="$2" directory="$3"
    local names layout
    validate_backend_directory "$directory" || return 1

    # A failed inspect must not be mistaken for an absent volume (daemon outage,
    # access denied, etc.). List first and fail before any mutation on API errors.
    names="$("$docker_bin" volume ls --format '{{.Name}}')" || return 1
    if ! grep -Fxq -- "$volume" <<< "$names"; then
        (umask 077; mkdir -p "$directory") || return 1
        validate_backend_directory "$directory" || return 1
        "$docker_bin" volume create --driver local \
            --opt type=none --opt o=bind --opt "device=$directory" \
            "$volume" >/dev/null || return 1
    fi

    layout="$("$docker_bin" volume inspect --format '{{if eq .Driver "local"}}{{if not .Options}}legacy{{else if and (eq (len .Options) 3) (eq (index .Options "type") "none") (eq (index .Options "o") "bind")}}bind:{{index .Options "device"}}{{else}}unsupported{{end}}{{else}}unsupported{{end}}' "$volume")" || return 1
    case "$layout" in
        "bind:$directory")
            validate_backend_directory "$directory" || return 1
            if [ ! -d "$directory" ]; then
                echo "Backend bind directory is missing; restore it before starting: $directory" >&2
                return 1
            fi
            ;;
        legacy)
            echo "Keeping existing Docker-managed volume $volume; data is NOT in $directory." >&2
            echo "To move it, follow docs/deployment/DATA_LAYOUT.md before packing the deployment directory." >&2
            ;;
        *)
            echo "Volume $volume does not point to $directory; refusing to replace or write it." >&2
            echo "Follow docs/deployment/DATA_LAYOUT.md to migrate or rebind it explicitly." >&2
            return 1
            ;;
    esac
}
