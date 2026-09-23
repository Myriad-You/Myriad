# Architecture Overview

Myriad is a self-hosted personal digital-life portal: Vite/React UI, Rust/Axum API, PostgreSQL, plus a production edge of **proxy + updater**.

## Runtime topologies

**Development**

```text
browser → Vite dev (:1102)
            └─ /api/*, /health, /ready, federation public paths → backend (:1103) → postgres
```

**Production**（详见 [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md)）

```text
host HTTP_PORT
  → proxy
       ├─► frontend (:1102)                 [myriad-net]
       ├─► backend (:1103) → postgres         MYRIAD_PROCESS_ROLE=web
       ├─► federation-worker (:1103)
       ├─► persona-worker (:1103)
       │
       └─► backend ─► updater-gateway → updater   [myriad-admin-net]
                                          └─► docker-guard → Docker sock
                                                [myriad-docker-guard-net]
       (rescue) updater when PROXY_ALLOW_DIRECT_UPDATER=true
```

- Only **proxy** publishes a host port. It routes SPA to frontend, persona
  prefixes to `persona-worker`, ActivityPub / federation HTTP+WS / Note media
  to `federation-worker`, and remaining `/api` + SEO shells to web. It does
  not rewrite HTML. Path table: [PORTS.md](../deployment/PORTS.md).
- Site identity in HTML: crawler / share shells are written by backend
  `load_site_branding`; the human SPA document is stamped by the frontend
  process from the same public `GET /api/config/metadata` fields.
- Updater is not dual-live A/B: one business slot, maintenance mode, immutable image tags, `pgdata` snapshots.
- Browser never sees `UPDATE_TOKEN`; admin UI uses `/api/admin/updater/*` via gateway.

## Major components

| Component | Path | Role |
| --- | --- | --- |
| Frontend | `frontend/` | Vite 8 + React 19 SPA; widgets, Library, Phantasi, reports, config, Tapp runtime |
| Backend | `backend/` | 同一镜像三个进程：web（迁移 + 剩余 API）、`federation-worker`、`persona-worker` |
| Proxy | `proxy/` | Host-facing reverse proxy + maintenance page (own Cargo tree) |
| Updater | `updater/` | Self-update, snapshots, docker-guard / gateway binaries (own Cargo tree) |
| Crates | `crates/` | Shared libs (error, outbound, image-proxy, tapp-*, …) |
| Shared | `shared/` | Cross-component static assets (e.g. image proxy host list) |

## Frontend startup

`frontend/src/main.tsx` initializes the document runtime and mounts React inside
an application error boundary. `DocumentReady` reveals the document when the
initial route commits, including its own data-loading UI, or when a recovery UI
commits. This is not a guarantee that every widget or network request has finished.
Missing route modules and unavailable locale catalogs must expose recovery rather
than leave the document hidden behind its loading screen.

Background TAPP startup, automatic Agent session startup, and speculative route
warming begin after this document signal. Explicitly opening the Agent session
wakes it immediately, and closing its panel does not unmount the session. Pending
timers and idle callbacks are cancelled on unmount; route
warming shares imports and contains speculative failures. These scheduling rules
do not alter the TAPP authorization or instance destruction rules.
`ApplicationStartup` owns TAPP startup and route-warming state locally; these
timers do not update the `App` component or recreate its provider tree.

`AuthProvider` owns the initial session probe. Pages consume its result rather
than queue another probe while it is pending. Profile changes and authenticated
actions may explicitly refresh it. Identity transitions invalidate old probes
and asynchronous completion events; logout and provider unmount also abort the
outstanding request.

## Backend surfaces (high level)

- **Platforms / profiles**：GitHub、Bilibili、Steam、网易云、YouTube、Bangumi、Discord、X、MAL、Xbox、PSN 等同步与资料库
- **Phantasi**（用户界面「手帐」，地址 `/journal`）：RSS / Notion / RSSHub / 笔记 / 工作台；笔记 RSS 默认关闭
- **Agent**（项目名 Arael）：Chat / Work / 记忆 / MCP；人设 / 频道 / 心跳在 `/agent/settings`，MCP 服务器列表仍在 `/config` 高级配置
  - `backend/src/persona/` owns initialization and supervised background drivers. Autonomy and speech ticks are serial within independent drivers; heartbeat runs one reserved batch with two active executions. `persona-worker` hosts this runtime together with Agent/speech/rig APIs, run hubs, cancellation, notification SSE, TAPP interaction callbacks, four channel bot connections and their recovery loop. Web excludes this domain; seven fixed first-party capabilities call authenticated private web RPC to reach web-owned schedulers/processors.
  - MCP runtime lives in `crates/myriad-mcp` (no DB or backend dependency); backend injects host transport policy and status notifications. Production blocks local stdio; operator-configured Streamable HTTP gateway connections support bounded JSON/SSE, session headers and non-retryable ambiguous calls. The optional fixed-container gateway deployment adds bubblewrap namespaces, cgroup limits and bounded guest destruction without a Docker socket.
  - MCP lifecycle: one cancellable actor per server; status/tool snapshots never wait on tool I/O. Each server admits at most 16 queued calls and **4 MiB** of encoded arguments across queued/in-flight calls. Request deadlines include pipe writes and queue wait. Changed/disabled definitions revoke their old actors; unchanged servers retain their sessions. New definitions require explicit activation.
  - MCP transport: max stdio line **4 MiB**, at most **64 lines / 16 MiB** per response including startup noise, max **32** concurrent direct children, env allowlist. Cancelled or malformed exchanges cannot reuse partial streams; Unix process groups and stderr tasks are reclaimed on destruction. **Development-only residual:** local stdio process groups do not isolate filesystem/network access. Production uses remote HTTP; the bundled tool deployment supplies OS/resource isolation, while arbitrary remote endpoints require their own enforcement.
- **Tapp**：安装校验、商店、Playground、沙箱 Bridge（见 [Tapp](TAPP_DEVELOPMENT.md)）
- **Federation**：ActivityPub + MFP（见 [FEDERATION.md](FEDERATION.md)）；file-transfer concurrent budgets (MYR-008) documented there
  - Federation HTTP, WebSockets and outbound delivery run in `federation-worker` in official Compose; proxy routes this domain directly and web owns migrations. Geographic disablement exits that worker; web and persona keep running. Workers require the existing installation key/schema and have fixed storage subpath mounts and resource budgets. An unset process role is rejected; updater preflight requires the migrated topology and proxy. See [Runtime isolation](../deployment/RUNTIME_ISOLATION.md) for the boundaries and update/rollback behavior. Persona execution and MCP stdio run in the separate trusted `persona-worker`; all three roles still use one image/version. Development stdio retains persona filesystem/network access; production blocks it. The optional fixed-container gateway supplies per-tool OS/resource limits and bounded cleanup; see the [gateway deployment](../deployment/MCP_GATEWAY.md) and optional [network/persistence controls](../deployment/MCP_CAPABILITIES.md). Worker DB roles and server-side budgets are documented in [Worker database budgets](../deployment/WORKER_DATABASE.md).
- **Auth**：本地用户 + GitHub / OIDC（见 [OAUTH.md](OAUTH.md)）
- **Updater admin**：`/api/admin/updater/*` → gateway

Schema 权威在 `backend/migrations/`（SeaORM），不是独立 `database/` SQL 树。

## Frontend surfaces (high level)

- 首页可编排 widgets（欢迎、音乐、天气、访客、游戏 Presence、Report Card、社交网络、Tapp 等）
- Library / Phantasi（`/journal`）/ Reports / Config / Agent 设置（`/agent/settings`）/ Tapp Store
- i18n：`zh-CN` / `zh-TW` / `en-US` / `ja-JP` / `ko-KR` / `fr-FR` / `de-DE`

## Frontend conventions

These rules keep one mechanism per concern. New code follows them; existing code
converges when it is touched rather than in dedicated migrations.

- **HTTP.** Same-origin API calls go through `apiService` (`src/services/api.ts`),
  which owns CSRF, session-loss notification, timeouts, transient read retry and
  `ApiError`. Domain modules wrap it (`src/services/*Api.ts`); components do not
  call `fetch` for API data. TAPP host calls use `tappRequest` / `apiRequest`
  (`src/tapp/services/TappHttpClient.ts`), which add the runtime grant on top of
  `apiService`. Raw `fetch` is reserved for: the pre-session bootstrap (CSRF,
  session probe, logout, setup), the updater's separate trust domain, streaming
  bodies (SSE, audio, binary exports) and third-party URLs.
- **Shared state.** Module-level state that React reads is a `createStore`
  (`src/utils/store.ts`) consumed with `useStore`; a store publishes only real
  changes, so subscribers re-render only then. Hand-rolled listener sets are not
  added.
- **Window events.** Every host-level `CustomEvent` is declared with its payload
  in `AppEventMap` (`src/utils/appEvents.ts`) and dispatched with `emitAppEvent`.
  An event with no listener is deleted, not kept for symmetry. Parent/child
  communication stays in props; events are for decoupled surfaces only.
- **Startup budget.** The initial bundle holds only what the first paint of any
  route needs. Panels, dialogs and capability engines (notification list, Agent
  global actions, liquid-glass lenses) load on demand or at idle, and are gated
  by the condition that makes them useful — a viewport, an open intent — rather
  than by a timer alone. `pnpm test:home-budget` guards the home page.
- **Motion.** Components use `motionShim` / `AnimatePresenceShim`
  (`src/lib/motionShim.tsx`), which render static markup until `motion` loads.
  The shim changes element type when `motion` arrives, which remounts its
  subtree; surfaces that must not remount either wait for `ensureMotionReady()`
  before mounting animated children (dashboards, TAPP store) or keep static
  markup for the committed view (the route frame in `App.tsx`).
- **Layout.** New domain code lives in `src/features/<domain>/` (see
  `features/merope`). Global styles live in `src/styles/`; component styles are
  imported by the component, so they ship with its chunk.

## Data & AI

- **PostgreSQL**：Compose 默认 `postgres:18-alpine`；release 兼容地板见 `release.json` 的 `min_pg_version`
- **AI**：可配置多 provider（Gemini、OpenAI 兼容端等），用于报告、Playground、Agent —— 非单一厂商绑定

## Related docs

- [QUICKSTART.md](../QUICKSTART.md) — 部署与本地开发
- [DOCKER_DEPLOYMENT.md](../deployment/DOCKER_DEPLOYMENT.md) — 生产拓扑与 env
- [PORTS.md](../deployment/PORTS.md) — 端口与代理路径（含 web / federation / persona 分流）
- [RUNTIME_ISOLATION.md](../deployment/RUNTIME_ISOLATION.md) — 进程边界与升级
- [WORKER_DATABASE.md](../deployment/WORKER_DATABASE.md) — worker 独立数据库登录
- [BUILD.md](BUILD.md) — 从源码构建
- [API.md](../API.md) — HTTP API 入口说明
- [AGENT_CHANNELS.md](AGENT_CHANNELS.md) — 私聊通道
- [FEDERATION.md](FEDERATION.md) — 联邦行为与闸门
