#!/bin/sh
set -eu

if [ "${MYRIAD_VOLUME_INIT_ONLY:-false}" = "true" ]; then
    if [ "$(id -u)" -ne 0 ]; then
        echo "backend volume initialization requires uid 0" >&2
        exit 1
    fi

    mkdir -p /app/cache /app/data
    chown -R --no-dereference myriad:myriad /app/cache /app/data
    exit 0
fi

exec "$@"
