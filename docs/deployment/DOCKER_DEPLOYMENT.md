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
- **Proxy routing**: SPA/static via frontend; `/api/*`, `/health`, and ActivityPub
  public paths (`/.well-known/webfinger`, `/.well-known/nodeinfo`, `/nodeinfo/2.1`,
  `/inbox`, `/users/*`, **`/media/federation/*`**) via backend. WebSocket upgrades
  under `/api/*` are bridged by proxy. See [PORTS.md](./PORTS.md). An outer TLS
  reverse proxy must pass the whole site (or the same AP + media allowlist) —
  `/api`-only breaks federation discovery **and** Note attachment media display.
- **Frontend/postgres cannot reach updater** (no shared L2 with updater).
- The updater is not an A/B dual-live system. It uses one running business slot,
  maintenance mode, `pgdata` snapshots, and immutable image tags.
- Only `docker-guard` mounts the raw Docker socket. The updater reaches it over an
  internal-only network; the guard restricts methods, Compose project labels, image
  repositories, container-create privileges, and host bind mounts.
- **`UPDATE_TOKEN` lives in updater + updater-gateway only**, not in the backend or
  docker-guard. Gateway injects `X-Update-Token` on the admin-net hop; Guard trust
  comes from the host-owned digest policy instead.
- **`UPDATER_GATEWAY_SECRET` lives in backend + updater-gateway only.** Backend sends
  `X-Updater-Gateway-Secret` on every proxied call. Admin-net peers without the secret
  cannot drive the gateway. Leaking this secret is still sensitive (can trigger updates
  via gateway) but better than placing `UPDATE_TOKEN` in the fat backend.
- **Backend runs as non-root** (image `USER myriad`, uid 1000). Deploy scripts repair
  owner-only access on named volumes `backend_cache` / `backend_data`, then perform
  a real write/remove probe as uid 1000. Backend startup repeats the storage probe and
  refuses to become healthy when `/app/cache`, `/app/data`, or an existing Tapp owner
  directory is not writable. Healthcheck stays on `localhost:1103/health`.
- Browser update requests go through backend admin routes:
  `/api/admin/updater/*`. The browser never receives `UPDATE_TOKEN`.
- **External PostgreSQL** (no in-stack `postgres`, `MYRIAD_DB_MODE=external`,
  `DATABASE_URL` as source of truth): see
  [EXTERNAL_POSTGRES.md](./EXTERNAL_POSTGRES.md) and
  [examples/docker-compose.external-db.example.yml](./examples/docker-compose.external-db.example.yml).

## Files

| File | Role |
| --- | --- |
| `docker-compose.yml` | Production stack: postgres, backend, frontend, proxy, updater, updater-gateway, docker-guard |
| `.env.production.example` | Template for host `.env` |
| `scripts/docker/deploy.sh` | Linux/macOS bootstrap and stack management |
| `scripts/docker/deploy.ps1` | Windows bootstrap and stack management |
| `docs/deployment/PORTS.md` | Development and production port map |
| `docs/deployment/EXTERNAL_POSTGRES.md` | External / 1Panel Postgres: `MYRIAD_DB_MODE=external`, no local pgdata |
| `docs/deployment/examples/docker-compose.external-db.example.yml` | Compose without `postgres`; external `DATABASE_URL` |
| `docs/deployment/UPDATER_SECURITY_BASELINE.md` | Done-state security baseline + operator red lines |
| `docs/UPDATER_QUICKSTART.md` | Operator guide for update, rollback, rescue |
| `docs/updater-spec.md` | Updater protocol and failure-mode design |

## First Start

```bash
cp .env.production.example .env
sudo install -d -m 0750 /etc/myriad
sudo install -m 0600 docker-guard.env.example /etc/myriad/docker-guard.env
# Replace the placeholder with the independently verified release repo@sha256 digest.

# Edit at minimum:
# POSTGRES_PASSWORD, JWT_SECRET, CORS_ORIGINS
# deploy.sh fills UPDATE_TOKEN / UPDATER_GATEWAY_SECRET and the host-policy
# GUARD_SELF_UPDATE_TOKEN if empty.

bash scripts/docker/deploy.sh up
```

On Windows:

```powershell
.\scripts\docker\deploy.ps1 up
```

Copy `docker-guard.env.example` to
`C:\ProgramData\Myriad\docker-guard.env`, change its
`MYRIAD_GUARD_ENV_FILE` entry to that exact resolved Windows path, and set the
verified digest before running the script.

Open `http://localhost` or the port configured by `HTTP_PORT`.

The setup mutations require the server-local capability. For the named-volume
deployment, read it without sending it through logs:

```bash
docker exec myriad-backend sh -c 'cat /app/data/.bootstrap-token'
```

The deploy scripts inspect the backend data volume. For a fresh or actively
unclaimed install, `up` temporarily enables `MYRIAD_ALLOW_REMOTE_BOOTSTRAP=true` so the
standard browser path through the bundled proxy works. Use `up
--local-bootstrap` only when intentionally calling the backend from loopback.
An older non-empty data volume with no claim marker fails safe and needs an
explicit `up --remote-bootstrap` if it is genuinely unclaimed.
A successful database initialization invalidates that token and writes a
distinct owner-claim token to the same file, so read it again before creating
the owner. After owner creation, rerun `deploy.sh up` / `deploy.ps1 up`; the
durable `.bootstrap-claimed` marker makes the script disable remote bootstrap
and recreate the backend. `doctor` reports a **FAIL** if a claimed installation
still has remote bootstrap enabled. The marker also prevents reopening setup
during a database outage.

## Environment

| Variable | Required | Purpose |
| --- | --- | --- |
| `POSTGRES_PASSWORD` | yes\* | PostgreSQL password for **in-stack** `postgres`; set yourself before first start. \*Not used when `MYRIAD_DB_MODE=external` (see [EXTERNAL_POSTGRES.md](./EXTERNAL_POSTGRES.md)). |
| `DATABASE_URL` | yes\*\* | Backend connection string. Default compose builds it from `POSTGRES_PASSWORD` + `postgres` service. \*\*Required explicitly for external DB. |
| `MYRIAD_DB_MODE` | no | Omit or default = local stack postgres + updater `pgdata` snapshots. Set `external` to skip pgdata snapshot/restore (operator owns DB backups). |
| `JWT_SECRET` | yes | JWT signing secret; set this yourself before first start |
| `ANALYTICS_SALT` | recommended | Salt for first-party visitor hashes on Data & stats. Generate with `openssl rand -hex 32`. If unset, a built-in default is used (fine for single-instance personal sites; set it for any shared/production deploy). |
| `TZ` | optional | Process timezone for container local time (and thus Data-page “today” / daily buckets). Analytics does **not** hard-code an offset; it uses whatever the backend process sees as local time. Compose examples may set `Asia/Shanghai` for convenience — change to match your host. Keep the host/container clock correct. |
| `CORS_ORIGINS` | yes (prod) | Comma-separated public frontend origin(s). Required when `ENVIRONMENT=production` (startup panics if empty). Never `*`. |
| `BASE_URL` | no | Public HTTPS origin used for federation Actor URLs and OAuth fallback; required for federation |
| `FRONTEND_URL` | no | Public frontend origin for redirects/profile links; usually the same as `BASE_URL` |
| `MYRIAD_TAG` | yes | Backend/frontend image tag, maintained by updater |
| `PROXY_TAG` | yes | Proxy image tag |
| `UPDATER_TAG` | yes | Updater image tag |
| `COMPOSE_PROJECT_NAME` | yes | Compose project name, default `myriad` |
| `UPDATE_TOKEN` | yes | Updater token for updater/gateway; deploy script fills it if empty; **not** injected into backend or docker-guard |
| `GUARD_SELF_UPDATE_TOKEN` | yes (Guard policy) | Dedicated self-update capability in host-owned `docker-guard.env`; deploy generates it if empty; Guard + updater only |
| `MYRIAD_ALLOW_REMOTE_BOOTSTRAP` | no | Backend default `false`; deploy `up` temporarily enables it for fresh/active browser setup and disables it after the claim marker. |
| `MYRIAD_BOOTSTRAP_TTL_SECS` | no | Short-lived setup capability TTL, default 1800 seconds (range 60..86400). |
| `UPDATER_GATEWAY_SECRET` | yes | Shared secret for backend→gateway (`X-Updater-Gateway-Secret`); deploy fills if empty; backend + gateway only |
| `HTTP_PORT` | no | Published proxy port, default `80` |
| `CHANNEL` | no | Release channel, default `stable` |
| `MYRIAD_GITHUB_REPO` | no | Release source repo, default `Myriad-You/Myriad` |
| `MYRIAD_DOCKER_NETWORK` | no | Business network override, default `myriad-net` |
| `MYRIAD_ADMIN_NETWORK` | no | Admin plane network override, default `myriad-admin-net` |
| `MYRIAD_DOCKER_GUARD_NETWORK` | no | Internal updater/guard network override, default `myriad-docker-guard-net` |
| `PROXY_TRUSTED_UPSTREAMS` | no | Comma-separated IP/CIDR allowlist for outer proxies; empty = trust no forwarded headers. Docker host reverse-proxies must be listed explicitly. Never `0.0.0.0/0` |
| `TRUST_PROXY_HEADERS` | no | Backend: honor `X-Forwarded-For` / `X-Real-IP` from trusted reverse-proxy peers (compose default `true` behind bundled proxy). Required for weather / client-geo to use the visitor IP instead of the server egress. |
| `TRUST_PROXY_PEERS` | no | Backend: CIDR/IP allowlist of reverse-proxy TCP peers. **Unset/empty = narrow built-in default** (loopback + docker0: `127.0.0.0/8,::1,172.17.0.0/16`) — **not** entire RFC1918. Non-empty = only listed peers. Compose stock defaults to loopback + docker0 + fixed `myriad-net` (`172.28.0.0/16`). Prefer an explicit reverse-proxy network only (from `docker network inspect`). Backend warns at startup when peers look overly broad. Rate-limit client IP uses the same extraction path. |
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
- Before Compose, place `docker-guard.env.example` outside the deployment root
  (`/etc/myriad/docker-guard.env` by default), set its exact independently verified
  Guard `repo@sha256`, and restrict it to the host administrator. The deploy scripts
  pass this second env file to Compose; updater cannot write it. The scripts
  also generate a dedicated `GUARD_SELF_UPDATE_TOKEN` there when absent.
- `COSIGN_VERIFY=off` alone is refused: set `UPDATER_ALLOW_INSECURE_COSIGN=true`
  (or `COSIGN_INSECURE_OK=true`) only when you intentionally accept that risk.
- Topology check (read-only; no auto-migrate):

```bash
bash scripts/docker/deploy.sh doctor
```

- Optional host scan for unexpected privileged containers / `docker.sock` binds
  (not run on every upgrade): `bash scripts/docker/deploy.sh doctor --host`.
  To watch new containers: `bash scripts/docker/deploy.sh doctor --events`.

### Hygiene (low-friction)

- **Secrets**: `UPDATE_TOKEN` / `UPDATER_GATEWAY_SECRET` / `JWT_SECRET` / `POSTGRES_PASSWORD` /
  `GITHUB_TOKEN` are redacted from updater/backend error bodies and log paths that might echo them.
- **Admin mutative updater** routes (`POST …/update|rollback|rescue/*`) use a
  stricter per-IP rate limit; status/jobs polling stays on the normal limit.
- **Audit actor**: backend proxies pass `X-Update-Actor: admin:<id>:<user>` after admin
  JWT; gateway injects `UPDATE_TOKEN` (included in updater `audit.log` when present).
- **High-risk updates**: when `allow_risk` / `allow_downgrade` / related flags are set,
  request body must also include `confirm_risk: true` (or header `X-Myriad-Confirm-Risk: true`).
  Normal upgrades without risk flags need no extra confirm field.
- **TCB upgrades**: the admin UI keeps one-click self-update. The lower-trust updater
  authenticates with the dedicated host-policy capability and sends only an
  immutable-shaped target tag. Guard fixes the official updater repository,
  pulls through the host daemon, converts the result to an exact digest, checks the running
  TCB and downgrade fences, and starts a fixed helper from that digest. The helper validates
  the rendered Compose model, updates only Guard/updater/gateway, persists
  `UPDATER_IMAGE_REF`, and restores the previous digest/config on failure. The current
  private-repository distribution uses the explicit `dockerhub_tag` trust path from #265.
  The resolved registry digest is not byte-bound to a signed release manifest;
  this path is therefore not reported as Cosign/release-manifest verification.
- **Root**: backend warns once at boot if running as uid 0 (compose should stay non-root).
- **Deploy soft-check**: `deploy.sh|ps1 up|upgrade` runs topology doctor in warn-only mode.
- **Doctor host checks**: `doctor --host` runs a non-fatal privileged / unexpected
  `docker.sock` scan; `doctor --events` streams container create/start.
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
snapshotting (local DB mode only), tag switching, health probes, rollback, and
rescue state.
Health probes use direct HTTP on the Compose network; they do not use Docker exec
or create temporary probe containers.

### External database

When PostgreSQL is **outside** this compose project (cloud RDS, 1Panel Postgres,
host install, separate DB stack):

1. Set `MYRIAD_DB_MODE=external` and a full `DATABASE_URL` in `.env`.
2. Do **not** run a `postgres` service or mount `./pgdata` for Myriad.
3. Updater still manages image tags and maintenance; **you** own DB backups
   (`pg_dump` / cloud / panel).

Full runbook and example compose:
[EXTERNAL_POSTGRES.md](./EXTERNAL_POSTGRES.md).

## Data Layout

| Path | Git status | Purpose |
| --- | --- | --- |
| `./pgdata` | ignored | PostgreSQL bind mount used for updater snapshots (**local DB only**; omit when external) |
| `./state` | ignored | Proxy maintenance state and updater lock/history |
| `./backups` | ignored | Operator-managed backups and diagnostics |
| `backend_cache` volume | Docker volume | Backend cache |
| `backend_data` volume | Docker volume | Backend app data |

`pgdata` must be a bind mount, not a named Docker volume, because updater
rollback needs file-level snapshots.
The updater mounts the deployment root once at `/host/compose`; `pgdata`, state,
snapshots, and `.env` are accessed below that root without additional host binds.
With `MYRIAD_DB_MODE=external`, do not leave an empty unused `./pgdata` directory
as if it were live data.

## Development

Use `docker-compose.dev.yml` only for local PostgreSQL:

```bash
docker compose -f docker-compose.dev.yml up -d postgres
(cd backend && cargo run --bin myriad-backend)
(cd frontend && pnpm install && pnpm dev)
```

Development directly uses `localhost:1102` and `localhost:1103`; production does
not.
