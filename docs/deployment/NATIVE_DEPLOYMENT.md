# Myriad Native (Docker-free) Deployment

This guide deploys Myriad **without Docker or Docker Compose** — straight onto a
Linux host using PostgreSQL, a compiled Rust binary, and (optionally) a reverse
proxy for TLS. It is a complete alternative to
[DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md).

> **TL;DR** — Myriad's backend binary serves the API **and** the built frontend
> from one process, and it runs its own database migrations on startup. So a
> production install is just two long-running things: **PostgreSQL** and the
> **`myriad-backend`** process. A reverse proxy (Caddy/nginx) is only needed for
> HTTPS.

---

## 1. How the native topology differs from Docker

The Docker stack runs five containers (`proxy`, `frontend`, `backend`,
`postgres`, `updater`). Natively, most of that collapses:

```text
                         (optional)
   Internet ──HTTPS──►  Caddy / nginx  ──HTTP──►  myriad-backend  ──►  PostgreSQL
                        TLS + gzip only          127.0.0.1:1103        127.0.0.1:5432
                                                 serves /api/*  +
                                                 static frontend
```

| Docker component | Native equivalent |
| --- | --- |
| `postgres` container | A normal PostgreSQL 16+ server (system package) |
| `backend` container | The `myriad-backend` binary, run under systemd |
| `frontend` container (`serve dist`) | **Gone** — the backend serves `frontend/dist` directly via `FRONTEND_DIST_PATH` |
| `proxy` container (Rust reverse proxy) | **Optional** — replaced by Caddy/nginx purely for TLS, or omitted for HTTP-only/LAN |
| `updater` container | **Not available** — it drives Docker via `docker.sock`. Updates are done by rebuild (see [§10](#10-updating)) |

**What you lose without Docker:** the in-app **Update Management** page
(`/config → About → Update Management`) proxies to the updater service, which
only works with Docker. On a native install that page is non-functional; use the
manual update procedure in [§10](#10-updating) instead. Everything else —
accounts, AI agents, Tapps, federation, all platform integrations — works
identically, because they are pure application features backed by PostgreSQL.

---

## 2. Prerequisites

| Requirement | Version | Notes |
| --- | --- | --- |
| Linux host | any modern distro | Examples below use Debian/Ubuntu `apt`. Adapt package names for RHEL/Arch. |
| **Rust** | stable **1.94+** | Install via [rustup](https://rustup.rs/). Image builds pin `rust:1.94-bookworm`; see `docs/development/BUILD.md`. |
| **Node.js** | **20+** (22 recommended) | For building the frontend only — not needed at runtime. |
| **pnpm** | **10.x** | Enable via `corepack enable`. |
| **PostgreSQL** | **16+** | Server + client tools. |
| C toolchain + libs | — | `gcc`, `pkg-config`, `libssl-dev`, `libpq-dev` to build; `libssl3`, `libpq5`, `ca-certificates` at runtime. |

Recommended machine size: **2 vCPU / 4 GB RAM** minimum (the AI agent and
fetchers are I/O heavy; PostgreSQL likes RAM). Building the backend needs
~2–4 GB RAM and 5–10 minutes on first compile.

### Install build/runtime dependencies (Debian/Ubuntu)

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config \
  libssl-dev libpq-dev \
  ca-certificates curl git \
  postgresql postgresql-contrib

# Rust (as the build user, not root)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Node + pnpm
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt-get install -y nodejs
corepack enable
```

---

## 3. Get the source

```bash
sudo mkdir -p /opt/src && sudo chown "$USER" /opt/src
cd /opt/src
git clone https://github.com/Myriad-You/Myriad.git
cd Myriad
```

---

## 4. Build the backend

```bash
cd /opt/src/Myriad/backend
cargo build --release
```

Output: `target/release/myriad-backend`.

Optionally stamp the version reported by `/health` (compile-time only):

```bash
MYRIAD_VERSION="v$(git describe --tags --always)" cargo build --release
```

> **OpenSSL build error?** Ensure `libssl-dev` and `pkg-config` are installed
> (they are required by `reqwest`'s default TLS backend).

---

## 5. Build the frontend

The frontend is a **static** Astro/React SPA (`output: 'static'`). Building it
produces a `dist/` folder of plain files that the backend will serve.

```bash
cd /opt/src/Myriad/frontend
pnpm install --frozen-lockfile
pnpm run build            # outputs to frontend/dist/
```

Leave `PUBLIC_API_URL` **empty** (the default). Because the backend serves the
SPA on the same origin, the frontend should call the API via relative `/api/*`
paths. Only set `PUBLIC_API_URL` if you host the frontend on a *different* origin
than the API (uncommon for native installs).

---

## 6. Assemble the runtime layout

Create a self-contained runtime directory. **The working directory matters:**
the backend loads its agent seed files from `data/agent` **relative to the
current working directory**, loads `.env` from the working directory, and creates
`data/` and `cache/` there. Keep everything under one root:

```bash
sudo mkdir -p /opt/myriad
sudo useradd --system --home /opt/myriad --shell /usr/sbin/nologin myriad || true

# Binary
sudo cp /opt/src/Myriad/backend/target/release/myriad-backend /opt/myriad/

# Frontend build
sudo mkdir -p /opt/myriad/frontend
sudo cp -r /opt/src/Myriad/frontend/dist /opt/myriad/frontend/dist

# Agent seed data (SOUL.md / USER.md / agents/*.md / mcp_servers.json …)
# These are committed in the repo and REQUIRED at runtime.
sudo cp -r /opt/src/Myriad/backend/data /opt/myriad/data

sudo chown -R myriad:myriad /opt/myriad
```

Resulting layout:

```text
/opt/myriad/
├── myriad-backend          # the binary
├── .env                    # created in the next step
├── data/                   # agent templates (seed) + runtime brew/tapps data
│   └── agent/              # SOUL.md, USER.md, agents/, mcp_servers.json, ...
├── cache/                  # created automatically at first run
└── frontend/dist/          # static SPA served by the backend
```

> Keep `DATA_DIR`/`CACHE_DIR` at their defaults and just set the process's
> **WorkingDirectory** to `/opt/myriad`. The agent seed path (`data/agent`) is
> hard-wired relative to the working directory and is **not** affected by
> `DATA_DIR`.

---

## 7. Create the database

```bash
# Create a role and database. Match the Docker stack's encoding/locale.
sudo -u postgres psql <<'SQL'
CREATE ROLE myriad WITH LOGIN PASSWORD 'CHANGE_ME_STRONG_PASSWORD';
CREATE DATABASE myriad
  WITH OWNER = myriad
       ENCODING = 'UTF8'
       LC_COLLATE = 'C'
       LC_CTYPE = 'C'
       TEMPLATE = template0;
SQL
```

You do **not** need to run migrations by hand — the backend runs
`Migrator::up()` automatically on every startup.

Keep PostgreSQL bound to localhost (default). Only the backend needs to reach it.

---

## 8. Configure `.env`

Create `/opt/myriad/.env`. This file is the **single source of truth** for
configuration — the backend both loads it at startup and re-reads it when config
changes from the UI.

```bash
sudo -u myriad tee /opt/myriad/.env >/dev/null <<'ENV'
# ---- Runtime mode (REQUIRED for production) ----
# Enables the strict security posture: CSP + HSTS headers, enforced CORS_ORIGINS,
# and hard-fail on a weak JWT_SECRET. WITHOUT this the server runs in the relaxed
# development posture (no CSP, no HSTS) even when reachable from the internet.
ENVIRONMENT=production

# ---- Database ----
DATABASE_URL=postgres://myriad:CHANGE_ME_STRONG_PASSWORD@localhost:5432/myriad

# ---- Server ----
# Bind to localhost when a reverse proxy runs on the same host.
# Use 0.0.0.0 for HTTP-only / LAN access with no proxy.
SERVER_HOST=127.0.0.1
SERVER_PORT=1103

# Absolute path to the built SPA. The backend serves it with SPA fallback.
FRONTEND_DIST_PATH=/opt/myriad/frontend/dist

# ---- Security (REQUIRED) ----
# Must be >= 32 chars. Generate: openssl rand -base64 48
# The placeholder below is REJECTED at startup — the backend refuses to boot
# until you replace it, so a known signing key can never reach production.
JWT_SECRET=CHANGEME_generate_via_openssl_rand_base64_48

# ---- CORS / public URLs ----
# Comma-separated public origins allowed to call the API.
CORS_ORIGINS=https://yourdomain.com
# Public HTTPS origin (federation Actor URLs, WebFinger, OAuth fallback).
BASE_URL=https://yourdomain.com
# Where to redirect after OAuth login (usually same as BASE_URL).
FRONTEND_URL=https://yourdomain.com

# ---- Logging ----
RUST_LOG=info
ENV
sudo chmod 600 /opt/myriad/.env
sudo chown myriad:myriad /opt/myriad/.env
```

Generate strong secrets:

```bash
openssl rand -base64 48   # JWT_SECRET
```

> Everything else — AI provider keys, GitHub/Steam/Bilibili/Notion tokens, OAuth
> apps, UI settings — is configured **through the web UI** after first start and
> stored in the database. You do not put them in `.env`.

### Environment variable reference

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `DATABASE_URL` | **yes** | — | `postgres://user:pass@host:port/db`. URL-encode special chars in the password. |
| `JWT_SECRET` | **yes** | — | Session/JWT signing key. Rejected at startup if `< 32` chars or an obvious default. |
| `SERVER_HOST` | no | `127.0.0.1` | Bind address. `0.0.0.0` to expose directly. |
| `SERVER_PORT` | no | `1103` | Listen port (API + static frontend + `/health`). |
| `FRONTEND_DIST_PATH` | no | `../frontend/dist` | Path to the static SPA. If missing, the backend runs API-only. |
| `CORS_ORIGINS` | prod: yes | `http://localhost:1102,http://localhost:1103` | Comma-separated allowed origins. Set to your real domain(s). |
| `BASE_URL` | for federation/OAuth | — | Public HTTPS origin used for Actor/WebFinger/OAuth callback URLs. |
| `FRONTEND_URL` | no | — | Redirect target after OAuth; usually equals `BASE_URL`. |
| `RUST_LOG` | no | `info` | `error\|warn\|info\|debug\|trace`, e.g. `info,myriad_backend=debug`. |
| `DATA_DIR` | no | `data` | App data root (brew, tapps). Relative to working dir. Leave default. |
| `CACHE_DIR` | no | `cache` | Cache root. Relative to working dir. Leave default. |
| `MYRIAD_UPDATER_URL` / `UPDATE_TOKEN` | no | — | Docker updater proxy only. **Leave unset** on native installs; a harmless startup warning is logged. |

---

## 9. Run as a systemd service

First, smoke-test in the foreground to confirm the DB connection and migrations:

```bash
cd /opt/myriad
sudo -u myriad env $(grep -v '^#' .env | xargs) ./myriad-backend
# Expect: migrations applied, then "🚀 Server listening on http://127.0.0.1:1103"
# Ctrl-C to stop.
```

Then install the unit at `/etc/systemd/system/myriad.service`:

```ini
[Unit]
Description=Myriad backend (API + static frontend)
After=network-online.target postgresql.service
Wants=network-online.target
Requires=postgresql.service

[Service]
Type=exec
User=myriad
Group=myriad
WorkingDirectory=/opt/myriad
ExecStart=/opt/myriad/myriad-backend
Restart=on-failure
RestartSec=5s

# The app loads /opt/myriad/.env itself (via dotenvy), so no EnvironmentFile is
# required. WorkingDirectory MUST be /opt/myriad so data/agent and .env resolve.

# Hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/opt/myriad/data /opt/myriad/cache
# .env is re-read on config changes, so keep it writable if you edit via ops:
# ReadWritePaths also implicitly covers files you rewrite under these dirs.

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now myriad
sudo systemctl status myriad
journalctl -u myriad -f          # live logs
```

Verify health:

```bash
curl -fsS http://127.0.0.1:1103/health
# {"status":"ok","version":"v0.1.0",...}
```

---

## 10. Reverse proxy + HTTPS (recommended for public installs)

The backend already serves the whole app on one port, so the proxy has exactly
**one upstream** (`127.0.0.1:1103`) and only adds TLS. Pick one.

### Option A — Caddy (automatic HTTPS)

`/etc/caddy/Caddyfile`:

```caddy
yourdomain.com {
    encode zstd gzip
    reverse_proxy 127.0.0.1:1103
}
```

```bash
sudo systemctl reload caddy
```

Caddy obtains and renews Let's Encrypt certificates automatically.

### Option B — nginx

`/etc/nginx/sites-available/myriad.conf`:

```nginx
server {
    listen 443 ssl http2;
    server_name yourdomain.com;

    ssl_certificate     /etc/letsencrypt/live/yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/yourdomain.com/privkey.pem;

    # .tapp uploads and multipart imports can be large.
    client_max_body_size 50m;

    location / {
        proxy_pass http://127.0.0.1:1103;
        proxy_http_version 1.1;

        proxy_set_header Host              $host;
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-Host  $host;

        # WebSocket / SSE (federation chat, agent streaming)
        proxy_set_header Upgrade    $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_buffering off;
        proxy_read_timeout 3600s;
    }
}

# HTTP -> HTTPS redirect
server {
    listen 80;
    server_name yourdomain.com;
    return 301 https://$host$request_uri;
}
```

Add the connection-upgrade map once in `http {}` (e.g. `/etc/nginx/nginx.conf`):

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    ''      close;
}
```

```bash
sudo ln -s /etc/nginx/sites-available/myriad.conf /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
# Certs: sudo certbot --nginx -d yourdomain.com
```

When you put a proxy in front, keep `SERVER_HOST=127.0.0.1` and make sure
`CORS_ORIGINS`, `BASE_URL`, and `FRONTEND_URL` all use the **public HTTPS**
domain.

---

## 11. First-run setup

1. Open `https://yourdomain.com` (or `http://SERVER_IP:1103` for a bare LAN
   install).
2. You are redirected to `/setup`. Follow the wizard to create the **admin
   account** and initial configuration. (Schema migrations already ran at
   startup.)
3. After login, open **`/config`** to add AI provider keys, platform tokens, and
   OAuth apps as needed. These persist in PostgreSQL.

---

## 12. Operations

| Task | Command |
| --- | --- |
| Status | `systemctl status myriad` |
| Live logs | `journalctl -u myriad -f` |
| Restart | `sudo systemctl restart myriad` |
| Stop | `sudo systemctl stop myriad` |
| Health | `curl -fsS http://127.0.0.1:1103/health` |

### Backups

All durable state lives in **PostgreSQL** plus the small `/opt/myriad/data`
directory (tapps, brew assets, agent memory files). `cache/` is disposable.

```bash
# Database (run as a user that can auth as 'myriad')
pg_dump -U myriad -h 127.0.0.1 -d myriad -Fc -f /var/backups/myriad_$(date +%F).dump

# App data
tar czf /var/backups/myriad_data_$(date +%F).tar.gz -C /opt/myriad data
```

Restore:

```bash
pg_restore -U myriad -h 127.0.0.1 -d myriad --clean --if-exists /var/backups/myriad_YYYY-MM-DD.dump
```

Automate with a cron/systemd-timer entry.

---

## 13. Updating

Because there is no updater service, updates are a **rebuild-and-swap**. Your
data is safe: it lives in PostgreSQL (external) and `/opt/myriad/data`, neither
of which is touched by a rebuild. Migrations run automatically on the next start.

```bash
cd /opt/src/Myriad
git pull

# Rebuild artifacts
(cd backend  && cargo build --release)
(cd frontend && pnpm install --frozen-lockfile && pnpm run build)

# Swap in the new build (brief downtime)
sudo systemctl stop myriad
sudo cp backend/target/release/myriad-backend /opt/myriad/myriad-backend
sudo rm -rf /opt/myriad/frontend/dist
sudo cp -r frontend/dist /opt/myriad/frontend/dist
# Refresh agent seed templates if they changed upstream (won't clobber runtime data):
sudo cp -rn backend/data/agent/. /opt/myriad/data/agent/
sudo chown -R myriad:myriad /opt/myriad
sudo systemctl start myriad

journalctl -u myriad -f   # watch migrations + startup
```

> Take a `pg_dump` before updating across a schema change so you can roll back.

---

## 14. Ports (native)

| Port | Bound to | Purpose |
| --- | --- | --- |
| `1103` | `127.0.0.1` (or `0.0.0.0`) | Backend: `/api/*`, `/health`, and the static SPA |
| `5432` | `127.0.0.1` | PostgreSQL (local only) |
| `80` / `443` | public | Reverse proxy (Caddy/nginx), if used |

Only the reverse proxy (or `1103` itself, for HTTP-only installs) should be
reachable from the internet.

---

## 15. Troubleshooting

| Symptom | Cause / fix |
| --- | --- |
| `JWT_SECRET is too weak...` at startup | Secret `< 32` chars or a default value. Regenerate: `openssl rand -base64 48`. |
| `DATABASE_URL is required` / connection refused | `.env` not loaded (wrong WorkingDirectory) or Postgres down. Confirm `WorkingDirectory=/opt/myriad` and `systemctl status postgresql`. |
| Blank page / API calls 404 | `FRONTEND_DIST_PATH` doesn't point at a real `dist/`, or the frontend was built with a wrong `PUBLIC_API_URL`. Rebuild with `PUBLIC_API_URL` empty. |
| `[Identity]` warnings / agents act generic | `data/agent` missing from the working directory. Re-copy `backend/data` into `/opt/myriad/data`. |
| CORS errors in browser | Add the exact public origin to `CORS_ORIGINS` (scheme + host, no trailing slash). |
| OAuth redirect fails | Set `BASE_URL`/`FRONTEND_URL` to the public HTTPS origin and match the callback URL in the provider's app settings. |
| `updater /healthz: unreachable` in logs | Expected on native installs — the updater is Docker-only. Harmless. Use [§10/§13](#10-updating) for updates. |

---

## 16. Security checklist

- [ ] `JWT_SECRET` and the Postgres password are 32+ random chars (`openssl rand -base64 48`).
- [ ] `.env` is `chmod 600`, owned by the `myriad` user.
- [ ] PostgreSQL listens on `127.0.0.1` only.
- [ ] `SERVER_HOST=127.0.0.1` with a TLS reverse proxy in front (public installs).
- [ ] `CORS_ORIGINS` / `BASE_URL` / `FRONTEND_URL` use HTTPS and your real domain.
- [ ] The service runs as the unprivileged `myriad` user with systemd hardening.
- [ ] Regular `pg_dump` + `data/` backups are scheduled.

---

## See also

- `scripts/native/dev.sh` — Docker-free **development** loop (`cargo run` +
  `pnpm dev` against a local PostgreSQL); `scripts/native/assemble.sh` builds the
  deploy bundle described above
- [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md) — the containerized topology
- [PORTS.md](./PORTS.md) — full port map
- [../development/BUILD.md](../development/BUILD.md) — build details and troubleshooting
- [../API.md](../API.md) — HTTP API reference
