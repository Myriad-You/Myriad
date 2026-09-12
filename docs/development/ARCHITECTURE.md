# Architecture Overview

Myriad is a self-hosted personal digital-life portal: Astro/React UI, Rust/Axum API, PostgreSQL, plus a production edge of **proxy + updater**.

## Runtime topologies

**Development**

```text
browser → Astro dev (:1102)
            └─ /api/*, /health, /ready, federation public paths → backend (:1103) → postgres
```

**Production**（详见 [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md)）

```text
host HTTP_PORT
  → proxy
       ├─► frontend (:1102)          [myriad-net]
       ├─► backend (:1103) → postgres
       │       └─► updater-gateway → updater   [myriad-admin-net]
       │                                 └─► docker-guard → Docker sock
       │                                       [myriad-docker-guard-net]
       └─ (rescue) updater when PROXY_ALLOW_DIRECT_UPDATER=true
```

- Only **proxy** publishes a host port.
- Updater is not dual-live A/B: one business slot, maintenance mode, immutable image tags, `pgdata` snapshots.
- Browser never sees `UPDATE_TOKEN`; admin UI uses `/api/admin/updater/*` via gateway.

## Major components

| Component | Path | Role |
| --- | --- | --- |
| Frontend | `frontend/` | Astro 7 + React 19 SPA; widgets, Library, Brew, reports, config, Tapp runtime |
| Backend | `backend/` | Axum API, SeaORM, platform sync, Agent, Brew, federation, analytics |
| Proxy | `proxy/` | Host-facing reverse proxy + maintenance page (own Cargo tree) |
| Updater | `updater/` | Self-update, snapshots, docker-guard / gateway binaries (own Cargo tree) |
| Crates | `crates/` | Shared libs (error, outbound, image-proxy, tapp-*, …) |
| Shared | `shared/` | Cross-component static assets (e.g. image proxy host list) |

## Backend surfaces (high level)

- **Platforms / profiles**：GitHub、Bilibili、Steam、网易云、YouTube、Bangumi、Discord、X、MAL、Xbox、PSN 等同步与资料库
- **Brew**：RSS / Notion / RSSHub 等阅读源
- **Agent**（项目名 Arael）：计划 / 执行 / 记忆 / MCP
  - MCP runtime lives in `crates/myriad-mcp` (no DB or backend dependency); backend injects status notifications.
  - MCP lifecycle: one cancellable actor per server; status/tool snapshots never wait on tool I/O. Each server admits at most 16 queued calls and **4 MiB** of encoded arguments across queued/in-flight calls. Request deadlines include pipe writes and queue wait. Changed/disabled definitions revoke their old actors; unchanged servers retain their sessions. New definitions require explicit activation.
  - MCP transport: max stdio line **4 MiB**, max **32** concurrent direct children, env allowlist. Cancelled exchanges cannot reuse partial streams; Unix process groups and stderr tasks are reclaimed on destruction. **Residual:** process groups are lifecycle cleanup, not a security boundary; full per-server OS/container isolation remains required.
- **Tapp**：安装校验、商店、Playground、沙箱 Bridge（见 [Tapp](TAPP_DEVELOPMENT.md)）
- **Federation**：ActivityPub + MFP（见 [FEDERATION.md](FEDERATION.md)）；file-transfer concurrent budgets (MYR-008) documented there
  - Federation HTTP, WebSockets and outbound delivery run in `federation-worker` in official Compose; proxy routes this domain directly and web owns migrations. Workers require the existing installation key/schema and have fixed storage subpath mounts and resource budgets. An unset process role is rejected; updater preflight requires the migrated topology and proxy. See [Runtime isolation](../deployment/RUNTIME_ISOLATION.md) for the boundaries and update/rollback behavior. Persona execution and MCP stdio remain in web; MCP OS isolation remains unfinished.
- **Auth**：本地用户 + GitHub / OIDC（见 [OAUTH.md](OAUTH.md)）
- **Updater admin**：`/api/admin/updater/*` → gateway

Schema 权威在 `backend/migrations/`（SeaORM），不是独立 `database/` SQL 树。

## Frontend surfaces (high level)

- 首页可编排 widgets（欢迎、音乐、天气、访客、游戏 Presence、Report Card、社交网络、Tapp 等）
- Library / Brew / Reports / Config / Agent / Tapp Store
- i18n：`zh-CN` / `en-US` / `ja-JP`

## Data & AI

- **PostgreSQL**：Compose 默认 `postgres:18-alpine`；release 兼容地板见 `release.json` 的 `min_pg_version`
- **AI**：可配置多 provider（Gemini、OpenAI 兼容端等），用于报告、Playground、Agent —— 非单一厂商绑定

## Related docs

- [QUICKSTART.md](../QUICKSTART.md) — 部署与本地开发
- [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md) — 生产拓扑与 env
- [PORTS.md](../deployment/PORTS.md) — 端口与代理路径
- [BUILD.md](BUILD.md) — 从源码构建
- [API.md](../API.md) — HTTP API 入口说明
