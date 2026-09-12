# syntax=docker/dockerfile:1.6
# Pin with backend/updater (sea-orm 2 / sqlx 0.9 need rustc >= 1.94).
FROM rust:1.94-bookworm AS builder
WORKDIR /work

COPY proxy/Cargo.toml proxy/Cargo.toml
COPY proxy/Cargo.lock proxy/Cargo.lock
COPY proxy/src proxy/src

ARG MYRIAD_VERSION=v0.0.0-dev
ENV MYRIAD_VERSION=${MYRIAD_VERSION}
ARG MYRIAD_COMMIT_SHA=unknown
ENV MYRIAD_COMMIT_SHA=${MYRIAD_COMMIT_SHA}

# `release` = full LTO (local/default). `ci-release` = thin LTO for image CI.
ARG CARGO_PROFILE=release

WORKDIR /work/proxy
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/work/proxy/target,sharing=locked \
    cargo build --profile="${CARGO_PROFILE}" --bin myriad-proxy \
    && cp "target/${CARGO_PROFILE}/myriad-proxy" /usr/local/bin/myriad-proxy \
    && strip /usr/local/bin/myriad-proxy

FROM debian:bookworm-slim AS runtime
ARG MYRIAD_VERSION=v0.0.0-dev
ENV MYRIAD_VERSION=${MYRIAD_VERSION}
ARG MYRIAD_COMMIT_SHA=unknown
ENV MYRIAD_COMMIT_SHA=${MYRIAD_COMMIT_SHA}
LABEL org.opencontainers.image.revision=${MYRIAD_COMMIT_SHA}
LABEL io.myriad.proxy.federation-routing="1"
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/myriad-proxy /usr/local/bin/myriad-proxy

ENV PROXY_LISTEN=0.0.0.0:80 \
    PROXY_STATE_FILE=/state/maintenance.json \
    PROXY_BACKEND_UPSTREAM=http://backend:1103 \
    PROXY_FRONTEND_UPSTREAM=http://frontend:1102 \
    PROXY_UPDATER_UPSTREAM=http://updater:1101

EXPOSE 80

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/myriad-proxy"]
