# Myriad Ports

Current port ownership after the proxy + updater migration.

## Production

| Service | Container port | Host exposure | Notes |
| --- | --- | --- | --- |
| proxy | `80` | `${HTTP_PORT:-80}` | The only public Docker Compose port. Routes SPA pages to frontend; backend paths below; optional rescue `/_updater/*`. |
| frontend | `1102` | none | `myriad-net` only; reached through `proxy`. |
| backend | `1103` | none | `myriad-net` + `myriad-admin-net`; reached through `proxy` for API + federation public paths. |
| postgres | `5432` | none | `myriad-net` only; data lives in `./pgdata`. |
| updater-gateway | `1104` | none | `myriad-admin-net` only; backend default hop; requires `X-Updater-Gateway-Secret`, injects `X-Update-Token`. |
| updater | `1101` | none | `myriad-admin-net` + guard-net; not on business net; browser uses backend `/api/admin/updater/*`. |
| docker-guard | `2375` | none | Internal guard-net only; not published on the host. |

### Proxy → backend path allowlist

Production `proxy` forwards these path prefixes/exact paths to backend (everything
else goes to the frontend SPA). Match path-only (no query):

| Path | Role |
| --- | --- |
| `/api/*` | App API (includes federation REST + WebSocket upgrades for `/api/*`) |
| `/health` | Backend health |
| `/.well-known/webfinger` | Federation discovery |
| `/.well-known/nodeinfo` | NodeInfo discovery |
| `/nodeinfo/2.1` | NodeInfo document |
| `/inbox` | Shared ActivityPub inbox |
| `/users/*` | Actor documents and per-user inboxes |

`/.well-known/acme-challenge/*` is **not** backend-routed (leave to TLS/ACME).

WebSocket: `proxy` detects `Upgrade: websocket` and bridges upgrades for any
backend-routed path (in practice federation WS under `/api/federation/*/ws`).

Outer reverse proxies (Nginx/Caddy/CDN) must either pass the **whole site** to
Myriad `proxy`, or explicitly allowlist the same ActivityPub paths above.
Proxying only `/api` breaks remote WebFinger/inbox federation.

Production should not define `BACKEND_PORT` or `FRONTEND_PORT`. Set `HTTP_PORT`
only when the proxy must listen on a non-default host port.

`/_updater/*` is disabled by default. It is exposed through the proxy only when
`PROXY_ALLOW_DIRECT_UPDATER=true` for rescue operations.

## Development

| Service | Local port | Started by | Notes |
| --- | --- | --- | --- |
| frontend dev server | `1102` | `pnpm dev` in `frontend/` | Serves the app; Astro dev proxy forwards the same backend path set as production (API + federation public paths). Dev proxy does **not** upgrade WebSockets. |
| backend | `1103` | `cargo run --bin myriad-backend` in `backend/` | Direct API development endpoint. |
| postgres dev | `5432` | `docker compose -f docker-compose.dev.yml up -d postgres` | Uses the `postgres_dev_data` named volume. |
| proxy | not started | n/a | Production-only in the normal dev loop. |
| updater harness | `1101` | `./scripts/dev/dev.sh start updater` or `start all-updater` | Optional direct updater port (legacy). Prefer gateway. |
| updater-gateway harness | `1104` | same as above | Host backend: `MYRIAD_UPDATER_URL=http://127.0.0.1:1104` + `UPDATER_GATEWAY_SECRET` (no `UPDATE_TOKEN`). |

The updater harness is for admin UI/backend proxy development. It uses isolated
runtime files under `.dev-updater/`. To test the real image replacement flow
against the production topology, use the production compose stack through
`scripts/docker/deploy.sh`.
