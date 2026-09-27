#!/usr/bin/env bash
set -euo pipefail
# Called from the repository root by release and development image builds.
printf 'io.myriad.compose.bundled=%s\n' "$(base64 < docker-compose.yml | tr -d '\n')"
printf 'io.myriad.compose.external=%s\n' "$(base64 < docs/deployment/examples/docker-compose.external-db.example.yml | tr -d '\n')"
