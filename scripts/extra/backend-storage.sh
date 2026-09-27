#!/usr/bin/env bash
# Host-side storage checks. Compose is the source of truth; these helpers never
# create or replace Docker volumes and never migrate data implicitly.

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

# Resolve storage from Compose rather than recording a second layout flag.
# Sets BACKEND_DATA_MOUNT / BACKEND_CACHE_MOUNT as Docker --mount source options.
# "prepare" is host bootstrap; "existing" is non-migrating backup/restore preflight.
load_backend_storage() {
    local docker_bin="$1" project="$2" root="$3" mode="${4:-existing}"
    local model kind mount type source name names legacy="" current inspection device
    command -v jq >/dev/null || { echo "jq is required to read Compose storage" >&2; return 1; }
    model="$("$docker_bin" compose --project-directory "$root" --env-file "$root/.env" \
        -p "$project" config --no-env-resolution --format json)" || return 1
    names="$("$docker_bin" volume ls --format '{{.Name}}')" || return 1
    for kind in data cache; do
        mount="$(printf '%s' "$model" | jq -ce --arg target "/app/$kind" '
            [.services.backend.volumes[] | select(.target == $target)] |
            if length == 1 and .[0].read_only != true and .[0].volume.subpath == null then .[0] else error("invalid backend storage") end')" || return 1
        type="$(printf '%s' "$mount" | jq -r .type)"
        source="$(printf '%s' "$mount" | jq -r .source)"
        case "$type" in
            bind)
                [ "$source" = "$root/$kind" ] || { echo "Backend bind must use $root/$kind" >&2; return 1; }
                validate_backend_directory "$source" || return 1
                if [ "$mode" = prepare ]; then
                    if grep -Fxq -- "${project}_backend_${kind}" <<< "$names"; then
                        echo "Legacy volume ${project}_backend_${kind} still exists; migrate it before switching Compose to bind mounts. See docs/deployment/DATA_LAYOUT.md" >&2
                        return 1
                    fi
                elif [ ! -d "$source" ]; then
                    echo "Backend storage directory missing: $source" >&2
                    return 1
                fi
                current="type=bind,src=$source"
                ;;
            volume)
                [ "$source" = "backend_$kind" ] || { echo "Unexpected backend volume source" >&2; return 1; }
                name="$(printf '%s' "$model" | jq -r --arg source "$source" '.volumes[$source].name')"
                [ "$name" = "${project}_backend_${kind}" ] || { echo "Unexpected backend volume name" >&2; return 1; }
                grep -Fxq -- "$name" <<< "$names" || { echo "Backend volume missing: $name; restore it before continuing" >&2; return 1; }
                inspection="$("$docker_bin" volume inspect "$name")" || return 1
                # Older releases may have registered a local bind-backed volume.
                # Validate its host path as strictly as a direct bind before root repair.
                device="$(printf '%s' "$inspection" | jq -er --arg root "$root/$kind" '
                    .[0] | if .Driver != "local" then error("unsupported volume driver")
                    elif (.Options // {}) == {} then "managed"
                    elif .Options == {type:"none",o:"bind",device:$root} then .Options.device
                    else error("unexpected backend volume options") end')" || return 1
                if [ "$device" != managed ]; then
                    [ -d "$device" ] || { echo "Backend storage directory missing: $device" >&2; return 1; }
                    validate_backend_directory "$device" || return 1
                    for source in "$device"/*; do
                        case "${source##*/}" in
                            media|federation|federation_media|images) validate_backend_directory "$source" || return 1 ;;
                        esac
                    done
                fi
                current="type=volume,src=$name,volume-nocopy"
                ;;
            *) echo "Unsupported backend storage type" >&2; return 1 ;;
        esac
        [ -z "$legacy" ] || [ "$legacy" = "$type" ] || { echo "Mixed backend storage layouts are unsupported" >&2; return 1; }
        legacy="$type"
        if [ "$kind" = data ]; then BACKEND_DATA_MOUNT="$current"; else BACKEND_CACHE_MOUNT="$current"; fi
    done
    if [ "$legacy" = bind ]; then
        # --mount uses CSV; reject an ambiguous source before any filesystem write.
        case "$root" in *','*) echo "Deployment path must not contain a comma" >&2; return 1 ;; esac
        if [ "$mode" = prepare ]; then
            (umask 077; mkdir -p "$root/data" "$root/cache") || return 1
        fi
        for source in "$root/data" "$root/cache" "$root/data/media" \
            "$root/data/federation" "$root/data/federation_media" "$root/cache/images"; do
            validate_backend_directory "$source" || return 1
        done
    fi
}
