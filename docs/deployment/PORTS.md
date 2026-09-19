# Myriad Ports

Current port ownership after the proxy + updater migration.

## Production

The table describes bundled PostgreSQL. The [external DB example](./EXTERNAL_POSTGRES.md)
removes `postgres` and also attaches backend and both workers to
`MYRIAD_BACKEND_EXTRA_NETWORK` (default `myriad-backend-ext`).
Ports are unchanged; workers stay off the admin network.

| Service | Container port | Host exposure | Notes |
| --- | --- | --- | --- |
| proxy | `80` | `${HTTP_PORT:-80}` | The only public Docker Compose port. Routes SPA to frontend; web / federation / persona paths below; optional rescue `/_updater/*`. |
| frontend | `1102` | none | `myriad-net` only; reached through `proxy`. Stamps SPA title/icon from `GET /api/config/metadata`. Crawler / share HTML stays on backend. |
| backend (`MYRIAD_PROCESS_ROLE=web`) | `1103` | none | `myriad-net` + `myriad-admin-net`. Owns migrations, platform/Phantasi/TAPP schedulers, and remaining `/api/*`. Does **not** register federation or persona HTTP. |
| federation-worker | `1103` | none | `myriad-net` only. Same backend image; `command: ["/app/myriad-federation-worker"]`. Federation HTTP / WS / outbound delivery. |
| persona-worker | `1103` | none | `myriad-net` only. Same backend image; `command: ["/app/myriad-persona-worker"]`. Agent / speech / rig / TAPP interaction HTTP. |
| postgres | `5432` | none | `myriad-net` only; data lives in `./pgdata`. |
| updater-gateway | `1104` | none | `myriad-admin-net` only; backend default hop; requires `X-Updater-Gateway-Secret`, injects `X-Update-Token`. |
| updater | `1101` | none | `myriad-admin-net` + guard-net; not on business net; browser uses backend `/api/admin/updater/*`. |
| docker-guard | `2375` | none | Internal guard-net only; not published on the host. |

Official Compose sets `PROXY_FEDERATION_UPSTREAM=http://federation-worker:1103` and
`PROXY_PERSONA_UPSTREAM=http://persona-worker:1103`. Proxy refreshes backend
`/health` isolation flags every two seconds; a current web process reporting
`federation_http_isolated=true` / `persona_http_isolated=true` keeps those
domains on the workers. Worker failure does **not** fall back into web.
See [RUNTIME_ISOLATION.md](./RUNTIME_ISOLATION.md).

### Proxy routing

Match path-only (no query). Order in `proxy/src/main.rs` `handle`: persona
(when isolated) → federation (when isolated) → remaining backend paths → SPA.

`/.well-known/acme-challenge/*` is **not** routed into the stack (leave to TLS/ACME).

#### Proxy → persona-worker

| Path | Role |
| --- | --- |
| `/api/agent` `/api/agent/*` | Agent runs, notifications SSE, presence, cancel |
| `/api/speech` `/api/speech/*` | Voice sessions |
| `/api/merope/rig` `/api/merope/rig/*` | Merope rig |
| `/api/tapp/agent/v2/interactions` `/api/tapp/agent/v2/interactions/*` | TAPP interaction callbacks |

Persona WebSocket upgrades are rejected (400). `/internal/*` is 404 at the edge.

#### Proxy → federation-worker

| Path | Role |
| --- | --- |
| `/api/federation/*` | Federation REST + WebSocket upgrades (`/api/federation/*/ws`) |
| `/api/admin/federation/*` | Admin federation |
| `/api/tapp/federation/*` | TAPP federation |
| `/.well-known/webfinger` | Federation discovery |
| `/.well-known/nodeinfo` | NodeInfo discovery |
| `/nodeinfo/2.1` | NodeInfo document |
| `/inbox` | Shared ActivityPub inbox |
| `/users/*` | Actor documents and per-user inboxes |
| `/media/federation/*` | Public Note attachment media (Image/Video); must not hit SPA |
| `/activities/*` `/notes/*` `/reports/*` `/tapps/*` `/library/*` `/phantasi/articles/*` | ActivityPub object dereference (prefix longer than the SEO index path) |

Exact `/reports` and `/library` stay on the SEO / SPA owners below. They are
**not** federation prefixes.

#### Proxy → backend (web)

Everything else that is not SPA. Includes remaining `/api/*` (setup, Phantasi,
Tapps, updater admin proxy, SEO JSON, …).

| Path | Role |
| --- | --- |
| `/api/*` (not persona / federation prefixes above) | App API |
| `/health` | Backend liveness (process up). Not business readiness. |
| `/ready` | Backend readiness (live DB probe, migrations, full routes, storage). 503 when not ready. |
| `/sitemap.xml` | Public SEO sitemap (also `/api/seo/sitemap.xml`); empty urlset when durable origin (`FRONTEND_URL`/`BASE_URL`) is unset — no client Host fallback |
| `/journal/notes.xml` | Public RSS of published notes (also `/api/phantasi/notes.xml`). Off by default. 404 when the owner switch is off or Phantasi is not guest-visible. Item links are absolute only when `FRONTEND_URL`/`BASE_URL` is set |
| `/robots.txt` | Dynamic robots; absolute `Sitemap:` line only when `FRONTEND_URL` or `BASE_URL` is set (omitted when unset) |
| `/llms.txt` | AI-facing site index (when GEO policy allows) |
| `/` `/tapp` `/journal` `/journal/feeds` `/journal/notes` `/library` `/reports` | **Crawler / WeChat-Weibo in-app share UA** → backend SEO HTML shell (proxy routes only; it does not rewrite HTML); `?_spa=1` and ordinary browsers → SPA |
| `/tapp/run/*` | **Crawler UA only** → backend SEO HTML shell; browsers → SPA |
| `/journal/articles/*` | **Crawler UA only** → own Journal articles SEO shell (`我` category, article body); browsers → SPA |
| `/journal/friends` `/journal/topics/*` | **Crawler / in-app share UA** → thin `noindex, follow` shell (no reprinted friend-link or topic bodies). Not in sitemap. Browsers → SPA |
| `/api/seo/tapp/{id}` | Public Tapp share summary JSON |

WebSocket: `proxy` detects `Upgrade: websocket` and bridges upgrades for
backend-routed and federation-routed paths (in practice federation WS under
`/api/federation/*/ws`).

### Federation file transfer (chat file-meta) via proxy

These live under **`/api/federation/*`**, so production Myriad `proxy` already
routes them to **federation-worker** (no extra allowlist entry). Operators still
need correct **body size** and **read timeouts** on any *outer* reverse proxy:

| Path | Role |
| --- | --- |
| `POST /api/federation/channels/{id}/transfers` | Start DM chunked upload |
| `POST /api/federation/rooms/{id}/transfers` | Start group chunked upload |
| `POST /api/federation/transfers/{id}/chunks` | Upload one base64 chunk (~1.4 MiB JSON) |
| `GET /api/federation/transfers/{id}/content` | **Stream download** completed file (can be large) |
| `GET /api/federation/transfers/{id}` | Transfer status |

Production `proxy` **streams** request/response bodies (does not buffer full
files). Outer Nginx/Caddy must not use a short `proxy_read_timeout` or a tiny
`client_max_body_size` (default 1m) or chunk upload / download will fail while
small chat messages still work.

Outer reverse proxies (Nginx/Caddy/CDN) must either pass the **whole site** to
Myriad `proxy`, or explicitly allowlist the same ActivityPub **and media** paths
above. Proxying only `/api` breaks remote WebFinger/inbox federation **and**
federation Note attachment images/videos (upload may still succeed via `/api/federation/media`,
but public GET `/media/federation/{userId}/{file}` never reaches the stack).

Quick smoke (after deploy, replace host + a real uploaded file path):

```bash
curl -sI "https://your.domain/media/federation/1/<uuid>.jpg" | head -5
# expect: HTTP/2 200 (or 404 if file missing) — NOT text/html SPA shell

# Transfer download (auth cookie required; expect attachment headers, not SPA HTML)
curl -sI -b 'session=...' "https://your.domain/api/federation/transfers/<transfer_id>/content" | head -10
# expect: content-disposition: attachment; ...  content-type: <mime>
```

Production should not define `BACKEND_PORT` or `FRONTEND_PORT`. Set `HTTP_PORT`
only when the proxy must listen on a non-default host port.

`/_updater/*` is disabled by default. It is exposed through the proxy only when
`PROXY_ALLOW_DIRECT_UPDATER=true` for rescue operations.

## Development

| Service | Local port | Started by | Notes |
| --- | --- | --- | --- |
| frontend dev server | `1102` | `pnpm dev` in `frontend/` | Serves the app. Astro proxy matches production `is_backend_path` (`/api/*`, health/SEO, webfinger/inbox/`/users/*`/`/media/federation/*`). It does **not** upgrade WebSockets, does **not** split persona/federation onto workers, and does **not** claim ActivityPub object prefixes such as `/activities/*`. |
| backend | `1103` | `cargo run --bin myriad-backend` in `backend/` | Combined process (`MYRIAD_PROCESS_ROLE=all`). Production rejects `all`. |
| postgres dev | `5432` | `docker compose -f docker-compose.dev.yml up -d postgres` | Uses the `postgres_dev_data` named volume. |
| proxy | not started | n/a | Production-only in the normal dev loop. |
| updater harness | `1101` | `./scripts/dev.sh start updater` or `start all-updater` | Optional direct updater port (legacy). Prefer gateway. |
| updater-gateway harness | `1104` | same as above | Host backend: `MYRIAD_UPDATER_URL=http://127.0.0.1:1104` + `UPDATER_GATEWAY_SECRET` (no `UPDATE_TOKEN`). |

The updater harness is for admin UI/backend proxy development. It uses isolated
runtime files under `.dev-updater/`. To test the real image replacement flow
against the production topology, use the production compose stack through
`scripts/extra/deploy.sh`.
