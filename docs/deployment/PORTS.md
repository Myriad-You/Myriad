# Myriad Ports

Current port ownership after the proxy + updater migration.

## Production

| Service | Container port | Host exposure | Notes |
| --- | --- | --- | --- |
| proxy | `80` | `${HTTP_PORT:-80}` | The only public Docker Compose port. Routes pages, `/api/*`, `/health`, and optional rescue updater traffic. |
| frontend | `1102` | none | `myriad-net` only; reached through `proxy`. |
| backend | `1103` | none | `myriad-net` + `myriad-admin-net`; reached through `proxy` for `/api/*` and `/health`. |
| postgres | `5432` | none | `myriad-net` only; data lives in `./pgdata`. |
| updater-gateway | `1104` | none | `myriad-admin-net` only; backend default hop; injects `X-Update-Token`. |
| updater | `1101` | none | `myriad-admin-net` + guard-net; not on business net; browser uses backend `/api/admin/updater/*`. |
| docker-guard | `2375` | none | Internal guard-net only; not published on the host. |

Production should not define `BACKEND_PORT` or `FRONTEND_PORT`. Set `HTTP_PORT`
only when the proxy must listen on a non-default host port.

`/_updater/*` is disabled by default. It is exposed through the proxy only when
`PROXY_ALLOW_DIRECT_UPDATER=true` for rescue operations.

## Development

| Service | Local port | Started by | Notes |
| --- | --- | --- | --- |
| frontend dev server | `1102` | `pnpm dev` in `frontend/` | Serves the app and proxies `/api/*` to backend. |
| backend | `1103` | `cargo run --bin myriad-backend` in `backend/` | Direct API development endpoint. |
| postgres dev | `5432` | `docker compose -f docker-compose.dev.yml up -d postgres` | Uses the `postgres_dev_data` named volume. |
| proxy | not started | n/a | Production-only in the normal dev loop. |
| updater harness | `1101` | `./scripts/dev/dev.sh start updater` or `start all-updater` | Optional direct updater port (legacy). Prefer gateway. |
| updater-gateway harness | `1104` | same as above | Host backend: `MYRIAD_UPDATER_URL=http://127.0.0.1:1104` without `UPDATE_TOKEN`. |

The updater harness is for admin UI/backend proxy development. It uses isolated
runtime files under `.dev-updater/`. To test the real image replacement flow
against the production topology, use the production compose stack through
`scripts/docker/deploy.sh`.
