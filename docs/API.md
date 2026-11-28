# API

Base URL:

- Development（直连后端）: `http://localhost:1103`
- Production（经 proxy）: 与站点同源，例如 `https://yourdomain.com`

路径在两种模式下相同。完整路由以代码为准：`backend/src/router/`。

## Auth

多数管理 / 写接口需要登录后的 **JWT**（`Authorization: Bearer …`，或同源 cookie 会话，视前端调用约定）。

| 能力 | 路径前缀 / 说明 |
| --- | --- |
| 健康检查 | `GET /health`（公开，存活）；`GET /ready`（公开，业务就绪，未就绪 503）。经 proxy / 开发代理转发，也可直连 backend。 |
| 本地登录 / 注册 / 改密 | `/api/auth/…`（见 `auth_local`） |
| OAuth / OIDC | `/api/auth/oauth/:slug/*`；provider 列表 `GET /api/auth/oauth/providers` |
| 当前用户 | `GET /api/auth/me` 等 |

详情见 [OAUTH.md](development/OAUTH.md)。

## 常用公开或半公开面

| 区域 | 前缀 | 备注 |
| --- | --- | --- |
| 站点配置（脱敏） | `/api/config` | 写操作需管理员 |
| 资料 / 平台数据 | `/api/profile`、`/api/platforms` 等 | 受模块可见性与权限影响 |
| 图片 / 媒体代理 | `/api/proxy/…` | 热链域名见 `shared/image_proxy_hosts.json` |
| 站点媒体 | `/media/assets/{uuid}/{文件名}`、`/api/media/{id}/content` | 生产经 proxy 转到 **web**（不是 federation-worker）；见 [MEDIA.md](deployment/MEDIA.md) |
| 联邦 | `/.well-known/webfinger`、`/inbox`、`/users/*`、`/api/federation/*`、`/api/admin/federation/*`、`/api/tapp/federation/*`，以及对象解引用 `/activities/*` `/notes/*` `/reports/*` `/tapps/*` `/library/*` `/phantasi/articles/*` | 生产经 proxy 转到 **federation-worker**（不是 web）。精确 `/library` 仍走 SEO/SPA；精确 `/reports` 不再是页面 |
| Agent / 语音 / rig | `/api/agent`、`/api/speech`、`/api/merope/rig`、`/api/tapp/agent/v2/interactions` | 生产经 proxy 转到 **persona-worker** |

## 需管理员的典型面

| 区域 | 前缀 |
| --- | --- |
| 配置写入、用户管理、诊断 | `/api/config`、`/api/admin/…`、`/api/diagnostics` |
| Updater | `/api/admin/updater/*`（经 gateway，浏览器不持有 `UPDATE_TOKEN`） |
| 分析导出等 | `/api/analytics/…`（权限门控） |

## 形象 API

站外开发者调用的那一面，路径在站点的 `/api` 之外：`/rig-api/v1/*`，经 proxy 转到 **web**。只认租户的 API 密钥（`Authorization: Bearer mrk_…`），不认站点的登录态；API 密钥也进不了 `/api/*`。

| 能力 | 路径 | 说明 |
| --- | --- | --- |
| 自己是谁、配额用了多少 | `GET /rig-api/v1/me` | 租户 id、生效的配额、当前用量；响应带 `X-RateLimit-*` |
| 租户 | `GET`/`POST /api/admin/rig-api/tenants`，`GET`/`PATCH /api/admin/rig-api/tenants/{id}` | 站点管理员；详情里有密钥列表（只有前缀）和用量 |
| 密钥 | `POST /api/admin/rig-api/tenants/{id}/keys`，`DELETE …/keys/{key_id}` | 签发时响应里的 `secret` 只出现这一次（`Cache-Control: no-store`）；删除即吊销 |
| 配额 | `PUT /api/admin/rig-api/tenants/{id}/limits`，`GET`/`PUT /api/admin/rig-api/defaults` | 租户自己的一项为 null 时用站点默认 |

拒绝都带 `code`：401 `rig_api_key_required` / `rig_api_key_malformed` / `rig_api_key_invalid` / `rig_api_key_revoked` / `rig_api_key_expired`（带 `WWW-Authenticate`），403 `rig_api_tenant_disabled`，429 `rig_api_rate_limited`（带 `Retry-After`）。任务配额的拒绝（`api::rig_api::job_refused`；形象 API 不接任务）是 429 `rig_api_concurrency_limited` 与 `rig_api_job_quota_exhausted`（后者带 `Retry-After`）。

## Tapp / Agent / Phantasi

| 区域 | 文档 |
| --- | --- |
| Tapp REST（含宿主 AI/Agent 路由） | [tapp/REST_API.md](development/tapp/REST_API.md) |
| Tapp 总体 | [TAPP_DEVELOPMENT.md](development/TAPP_DEVELOPMENT.md) |
| Phantasi | `backend/src/api/phantasi`；笔记 RSS `/api/phantasi/notes.xml`（默认关） |
| 博客 | `backend/src/api/blog.rs`；设置与主题 `/api/blog/*`（站长）；静态页 `/blog/*` |
| Agent | `backend/src/api/agent`；人设 / Bot / 心跳 `/agent/settings`；MCP 配置仍在 `/config` 高级 |
| Agent Channel | [私聊通道生命周期](development/AGENT_CHANNELS.md)、[办事运输适配](design/agent-channel.md) |

## 约定

- JSON；错误体随 `AppError` 统一形状（见 `crates/myriad-error`）。
- 生产 CORS 由 `CORS_ORIGINS` 约束；联邦与媒体路径必须由外层 TLS 反代整站放行（见 [PORTS.md](deployment/PORTS.md)）。
- 本文不维护逐字段 OpenAPI；以路由模块与前端 `services/` 调用为准。
