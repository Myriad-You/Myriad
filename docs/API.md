# API

Base URL:

- Development（直连后端）: `http://localhost:1103`
- Production（经 proxy）: 与站点同源，例如 `https://yourdomain.com`

路径在两种模式下相同。完整路由以代码为准：`backend/src/router/`。

## Auth

多数管理 / 写接口需要登录后的 **JWT**（`Authorization: Bearer …`，或同源 cookie 会话，视前端调用约定）。

| 能力 | 路径前缀 / 说明 |
| --- | --- |
| 健康检查 | `GET /health`（公开） |
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
| 联邦 | `/.well-known/webfinger`、`/inbox`、`/users/*`、`/media/federation/*` 等 | 经 proxy 转到 backend |

## 需管理员的典型面

| 区域 | 前缀 |
| --- | --- |
| 配置写入、用户管理、诊断 | `/api/config`、`/api/admin/…`、`/api/diagnostics` |
| Updater | `/api/admin/updater/*`（经 gateway，浏览器不持有 `UPDATE_TOKEN`） |
| 分析导出等 | `/api/analytics/…`（权限门控） |

## Tapp / Agent / Brew

| 区域 | 文档 |
| --- | --- |
| Tapp REST（含宿主 AI/Agent 路由） | [tapp/REST_API.md](development/tapp/REST_API.md) |
| Tapp 总体 | [TAPP_DEVELOPMENT.md](development/TAPP_DEVELOPMENT.md) |
| Brew / Agent | 前端服务与 `backend/src/api/brew`、`backend/src/api/agent` |
| Agent Channel | [办事运输适配与 QQ 单聊](design/agent-channel.md) |

## 约定

- JSON；错误体随 `AppError` 统一形状（见 `crates/myriad-error`）。
- 生产 CORS 由 `CORS_ORIGINS` 约束；联邦与媒体路径必须由外层 TLS 反代整站放行（见 [PORTS.md](deployment/PORTS.md)）。
- 本文不维护逐字段 OpenAPI；以路由模块与前端 `services/` 调用为准。
