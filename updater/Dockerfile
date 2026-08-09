# syntax=docker/dockerfile:1.6

# ---------- builder ----------
# Pin with backend/proxy (sea-orm 2 / sqlx 0.9 need rustc >= 1.94).
FROM rust:1.94-bookworm AS builder
WORKDIR /work

COPY updater/Cargo.toml updater/Cargo.toml
COPY updater/Cargo.lock updater/Cargo.lock
COPY updater/src updater/src

ARG MYRIAD_VERSION=v0.0.0-dev
ENV MYRIAD_VERSION=${MYRIAD_VERSION}
ARG MYRIAD_COMMIT_SHA=unknown
ENV MYRIAD_COMMIT_SHA=${MYRIAD_COMMIT_SHA}

# `release` = full LTO (local/default). `ci-release` = thin LTO for image CI.
ARG CARGO_PROFILE=release

WORKDIR /work/updater
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/work/updater/target,sharing=locked \
    cargo build --profile="${CARGO_PROFILE}" --bins \
    && artifact_dir="${CARGO_PROFILE}" \
    && if [ "${artifact_dir}" = "dev" ]; then artifact_dir=debug; fi \
    && mkdir -p /out \
    && cp "target/${artifact_dir}/myriad-updater" \
          "target/${artifact_dir}/myriad-rescue" \
          "target/${artifact_dir}/myriad-docker-guard" \
          "target/${artifact_dir}/myriad-updater-gateway" \
          /out/ \
    && strip /out/*

# ---------- runtime ----------
FROM debian:bookworm-slim AS runtime
ARG TARGETARCH
ARG MYRIAD_COMMIT_SHA=unknown
LABEL org.opencontainers.image.revision=${MYRIAD_COMMIT_SHA}

# Install docker CLI + compose v2 plugin from Docker's official repo (the only reliable
# place to get an up-to-date docker-ce-cli for Debian).
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg tar gzip coreutils tini; \
    install -m 0755 -d /etc/apt/keyrings; \
    curl -fsSL https://download.docker.com/linux/debian/gpg \
        | gpg --dearmor -o /etc/apt/keyrings/docker.gpg; \
    chmod a+r /etc/apt/keyrings/docker.gpg; \
    echo "deb [arch=${TARGETARCH:-amd64} signed-by=/etc/apt/keyrings/docker.gpg] \
        https://download.docker.com/linux/debian bookworm stable" \
        > /etc/apt/sources.list.d/docker.list; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        docker-ce-cli docker-compose-plugin; \
    apt-get purge -y gnupg; apt-get autoremove -y; \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /out/myriad-updater /usr/local/bin/myriad-updater
COPY --from=builder /out/myriad-rescue  /usr/local/bin/myriad-rescue
COPY --from=builder /out/myriad-docker-guard /usr/local/bin/myriad-docker-guard
COPY --from=builder /out/myriad-updater-gateway /usr/local/bin/myriad-updater-gateway

# cosign CLI for release.json signature verification (M2). Single static binary, ~50 MB.
# MYR-047: pin release + per-arch sha256 (from cosign_checksums.txt on the same tag).
ARG COSIGN_VERSION=v2.4.1
RUN set -eux; \
    arch="$(dpkg --print-architecture)"; \
    case "$arch" in \
        amd64) cs_arch=amd64; cs_sha=8b24b946dd5809c6bd93de08033bcf6bc0ed7d336b7785787c080f574b89249b ;; \
        arm64) cs_arch=arm64; cs_sha=3b2e2e3854d0356c45fe6607047526ccd04742d20bd44afb5be91fa2a6e7cb4a ;; \
        *) echo "unsupported arch: $arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /tmp/cosign \
        "https://github.com/sigstore/cosign/releases/download/${COSIGN_VERSION}/cosign-linux-${cs_arch}"; \
    echo "${cs_sha}  /tmp/cosign" | sha256sum -c -; \
    install -m 0755 /tmp/cosign /usr/local/bin/cosign; \
    rm -f /tmp/cosign; \
    cosign version

# Updater receives one deployment-root bind at /host/compose. The separate docker-guard
# entrypoint is the sole consumer of /var/run/docker.sock.
ENV UPDATER_STATE_DIR=/host/compose/state \
    UPDATER_COMPOSE_DIR=/host/compose \
    UPDATER_ENV_FILE=/host/compose/.env \
    UPDATER_PGDATA=/host/compose/pgdata \
    UPDATER_LISTEN=0.0.0.0:1101

EXPOSE 1101

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/myriad-updater"]
