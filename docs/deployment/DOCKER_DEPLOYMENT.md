# Myriad Docker Deployment

This is the current production Docker topology. Direct backend/frontend host
ports, named-volume pgdata migration, and `:latest` image workflows have been
removed.

## Topology

```text
host HTTP_PORT
  |
  v
proxy ──┬── frontend:1102          [myriad-net]
        ├── backend:1103 ── postgres:5432
        │       │
        │       └── updater-gateway:1104 ──► updater:1101   [myriad-admin-net]
        │                                         │
        │                                         └── docker-guard:2375 ── sock
        │                                               [myriad-docker-guard-net]
        └── (rescue only) ──► updater:1101  when PROXY_ALLOW_DIRECT_UPDATER=true
```

Networks:

| Network | Members | Notes |
| --- | --- | --- |
| `myriad-net` | proxy, frontend, backend, postgres | Business L2. **Not** updater. |
| `myriad-admin-net` | backend, updater, updater-gateway, proxy | Token hop + rescue DNS. |
| `myriad-docker-guard-net` (internal) | updater, docker-guard | Only updater may join (guard policy). |

- Only `proxy` publishes a host port.
- **Frontend/postgres cannot reach updater** (no shared L2 with updater).
- The updater is not an A/B dual-live system. It uses one running business slot,
  maintenance mode, `pgdata` snapshots, and immutable image tags.
- Only `docker-guard` mounts the raw Docker socket. The updater reaches it over an
  internal-only network; the guard restricts methods, Compose project labels, image
  repositories, container-create privileges, and host bind mounts.
- **`UPDATE_TOKEN` lives in updater + updater-gateway + docker-guard**, not in the
  fat backend process. Gateway injects `X-Update-Token` on the admin-net hop.
- **`UPDATER_GATEWAY_SECRET` lives in backend + updater-gateway only.** Backend sends
  `X-Updater-Gateway-Secret` on every proxied call. Admin-net peers without the secret
  cannot drive the gateway. Leaking this secret is still sensitive (can trigger updates
  via gateway) but better than placing `UPDATE_TOKEN` in the fat backend.
- **Backend runs as non-root** (image `USER myriad`, uid 1000). Deploy scripts chown
  named volumes `backend_cache` / `backend_data` so `/app/cache` and `/app/data`
  remain writable. Healthcheck stays on `localhost:1103/health` (no privileged ports).
- Browser update requests go through backend admin routes:
  `/api/admin/updater/*`. The browser never receives `UPDATE_TOKEN`.
- Migrating topology: see
  [MIGRATION_DOCKER_GUARD.md](./MIGRATION_DOCKER_GUARD.md) (host `compose pull && up -d`;
  UI alone cannot switch topology).

## Files

| File | Role |
| --- | --- |
| `docker-compose.yml` | Production stack: postgres, backend, frontend, proxy, updater, updater-gateway, docker-guard |
| `.env.production.example` | Template for host `.env` |
| `scripts/docker/deploy.sh` | Linux/macOS bootstrap and stack management |
| `scripts/docker/deploy.ps1` | Windows bootstrap and stack management |
| `docs/deployment/PORTS.md` | Development and production port map |
| `docs/deployment/MIGRATION_DOCKER_GUARD.md` | Migrate from updater+sock to docker-guard dual-net |
| `docs/deployment/UPDATER_SECURITY_BASELINE.md` | Done-state security baseline + operator red lines |
| `docs/UPDATER_QUICKSTART.md` | Operator guide for update, rollback, rescue |
| `docs/updater-spec.md` | Updater protocol and failure-mode design |

## First Start

```bash
cp .env.production.example .env

# Edit at minimum:
# POSTGRES_PASSWORD, JWT_SECRET, CORS_ORIGINS
# deploy.sh fills UPDATE_TOKEN and UPDATER_GATEWAY_SECRET if empty.

bash scripts/docker/deploy.sh up
```

On Windows:

```powershell
.\scripts\docker\deploy.ps1 up
```

Open `http://localhost` or the port configured by `HTTP_PORT`.

## Environment

| Variable | Required | Purpose |
| --- | --- | --- |
| `POSTGRES_PASSWORD` | yes | PostgreSQL password; set this yourself before first start |
| `JWT_SECRET` | yes | JWT signing secret; set this yourself before first start |
| `CORS_ORIGINS` | yes | Public frontend origins |
| `BASE_URL` | no | Public HTTPS origin used for federation Actor URLs and OAuth fallback; required for federation |
| `FRONTEND_URL` | no | Public frontend origin for redirects/profile links; usually the same as `BASE_URL` |
| `MYRIAD_TAG` | yes | Backend/frontend image tag, maintained by updater |
| `PROXY_TAG` | yes | Proxy image tag |
| `UPDATER_TAG` | yes | Updater image tag |
| `COMPOSE_PROJECT_NAME` | yes | Compose project name, default `myriad` |
| `UPDATE_TOKEN` | yes | Updater token for updater/gateway/guard; deploy script fills it if empty; **not** injected into backend |
| `UPDATER_GATEWAY_SECRET` | yes | Shared secret for backend→gateway (`X-Updater-Gateway-Secret`); deploy fills if empty; backend + gateway only |
| `HTTP_PORT` | no | Published proxy port, default `80` |
| `CHANNEL` | no | Release channel, default `stable` |
| `MYRIAD_GITHUB_REPO` | no | Release source repo, default `Myriad-You/Myriad` |
| `MYRIAD_DOCKER_NETWORK` | no | Business network override, default `myriad-net` |
| `MYRIAD_ADMIN_NETWORK` | no | Admin plane network override, default `myriad-admin-net` |
| `MYRIAD_DOCKER_GUARD_NETWORK` | no | Internal updater/guard network override, default `myriad-docker-guard-net` |
| `PROXY_TRUSTED_UPSTREAMS` | no | Comma-separated IP/CIDR allowlist for outer proxies; empty = auto-trust private/loopback peers only (Docker host reverse-proxy). Never `0.0.0.0/0` |
| `PROXY_ALLOW_DIRECT_UPDATER` | no | Enables `/_updater/*` rescue path, default `false` |
| `COSIGN_VERIFY` | no | Release signature policy: `strict` (default), `soft`, or `off` |
| `UPDATER_ALLOW_INSECURE_COSIGN` | no | Required dual key when `COSIGN_VERIFY=off` (`true` / alias `COSIGN_INSECURE_OK`) |

Do not set `BACKEND_PORT` or `FRONTEND_PORT` for production. Those are internal
container ports.

For the full port map, see [PORTS.md](./PORTS.md).

## Security baseline

Updater security for **single-tenant self-hosted** Myriad is treated as **done
enough** on current topology. Operator red lines, deliberate accepts, and
out-of-scope items live in:

**[UPDATER_SECURITY_BASELINE.md](./UPDATER_SECURITY_BASELINE.md)**

### Security defaults (short)

- Keep **`COSIGN_VERIFY=strict`**, **`PROXY_ALLOW_DIRECT_UPDATER=false`**, and do **not**
  publish updater `1101`, updater-gateway `1104`, or docker-guard `2375` on the host.
- `COSIGN_VERIFY=off` alone is refused: set `UPDATER_ALLOW_INSECURE_COSIGN=true`
  (or `COSIGN_INSECURE_OK=true`) only when you intentionally accept that risk.
- Topology check (read-only; no auto-migrate):

```bash
bash scripts/docker/deploy.sh doctor
```

- Optional host scan for unexpected privileged containers / `docker.sock` binds
  (not run on every upgrade): `bash scripts/security/docker-audit-example.sh scan`
  or `bash scripts/docker/deploy.sh doctor --host`.

### Hygiene (low-friction)

- **Secrets**: `UPDATE_TOKEN` / `UPDATER_GATEWAY_SECRET` / `JWT_SECRET` / `POSTGRES_PASSWORD` /
  `GITHUB_TOKEN` are redacted from updater/backend error bodies and log paths that might echo them.
- **Admin mutative updater** routes (`POST …/update|rollback|self-update|rescue/*`) use a
  stricter per-IP rate limit; status/jobs polling stays on the normal limit.
- **Audit actor**: backend proxies pass `X-Update-Actor: admin:<id>:<user>` after admin
  JWT; gateway injects `UPDATE_TOKEN` (included in updater `audit.log` when present).
- **High-risk updates**: when `allow_risk` / `allow_downgrade` / related flags are set,
  request body must also include `confirm_risk: true` (or header `X-Myriad-Confirm-Risk: true`).
  Normal upgrades without risk flags need no extra confirm field.
- **Self-update visibility**: authenticated `GET /status` includes optional `self_update_last`
  from `state/self-update-last.json`; also `GET /self-update/last` (token/gateway required).
- **Root**: backend warns once at boot if running as uid 0 (compose should stay non-root).
- **Deploy soft-check**: `deploy.sh|ps1 up|upgrade` runs topology doctor in warn-only mode.
- **Doctor host tip**: `deploy.sh doctor` prints the optional host audit path; `doctor --host`
  runs a non-fatal privileged / unexpected `docker.sock` scan.
- **Updater `/healthz`**: public and minimal (`{"ok":true}` only — no versions/token status).
  Gateway `/healthz` likewise needs no secret (compose healthcheck).

## Operations

```bash
bash scripts/docker/deploy.sh status
bash scripts/docker/deploy.sh doctor
bash scripts/docker/deploy.sh logs
bash scripts/docker/deploy.sh restart
bash scripts/docker/deploy.sh down
```

Manual tag upgrade path:

```bash
# Edit MYRIAD_TAG / PROXY_TAG / UPDATER_TAG in .env first.
bash scripts/docker/deploy.sh upgrade
```

Day-to-day updates should be started from the admin UI:

```text
/config -> About/关于 -> Update Management/更新管理
```

The updater then handles maintenance mode, container stop/start, `pgdata`
snapshotting, tag switching, health probes, rollback, and rescue state.
Health probes use direct HTTP on the Compose network; they do not use Docker exec
or create temporary probe containers.

## Data Layout

| Path | Git status | Purpose |
| --- | --- | --- |
| `./pgdata` | ignored | PostgreSQL bind mount used for updater snapshots |
| `./state` | ignored | Proxy maintenance state and updater lock/history |
| `./backups` | ignored | Operator-managed backups and diagnostics |
| `backend_cache` volume | Docker volume | Backend cache |
| `backend_data` volume | Docker volume | Backend app data |

`pgdata` must be a bind mount, not a named Docker volume, because updater
rollback needs file-level snapshots.
The updater mounts the deployment root once at `/host/compose`; `pgdata`, state,
snapshots, and `.env` are accessed below that root without additional host binds.

## Development

Use `docker-compose.dev.yml` only for local PostgreSQL:

```bash
docker compose -f docker-compose.dev.yml up -d postgres
(cd backend && cargo run --bin myriad-backend)
(cd frontend && pnpm install && pnpm dev)
```

Development directly uses `localhost:1102` and `localhost:1103`; production does
not.
