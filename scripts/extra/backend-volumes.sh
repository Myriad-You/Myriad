#!/usr/bin/env bash
# Host-only provisioning. Compose treats these volumes as external so the updater
# can reuse them without permission to create/remove volumes through Docker Guard.

ensure_backend_volume() {
    local docker_bin="$1" volume="$2" directory="$3"
    local names layout
    case "$directory" in
        /*) ;;
        *) echo "Backend volume directory must be absolute: $directory" >&2; return 1 ;;
    esac

    # A failed inspect must not be mistaken for an absent volume (daemon outage,
    # access denied, etc.). List first and fail before any mutation on API errors.
    names="$("$docker_bin" volume ls --format '{{.Name}}')" || return 1
    if ! grep -Fxq -- "$volume" <<< "$names"; then
        (umask 077; mkdir -p "$directory") || return 1
        "$docker_bin" volume create --driver local \
            --opt type=none --opt o=bind --opt "device=$directory" \
            "$volume" >/dev/null || return 1
    fi

    layout="$("$docker_bin" volume inspect --format '{{if eq .Driver "local"}}{{if not .Options}}legacy{{else if and (eq (len .Options) 3) (eq (index .Options "type") "none") (eq (index .Options "o") "bind")}}bind:{{index .Options "device"}}{{else}}unsupported{{end}}{{else}}unsupported{{end}}' "$volume")" || return 1
    case "$layout" in
        "bind:$directory") return 0 ;;
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
