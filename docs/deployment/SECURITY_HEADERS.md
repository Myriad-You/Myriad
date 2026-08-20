# 安全响应头配置指南

当前状态：安全响应头由多层协作。生产部署中，浏览器请求先进入 Myriad `proxy`，
再转发到 backend / frontend。

## 分层职责

| 层 | 路径 | 作用 |
| --- | --- | --- |
| **backend** | `backend/src/middleware/security.rs` | API 响应：CSP、XFO、Permissions-Policy 等 |
| **proxy** | `proxy/src/main.rs` | 上游**未带** `Permissions-Policy` 时补齐（覆盖静态 SPA 文档） |
| **frontend** | `frontend/public/serve.json` + Astro dev middleware | 静态 `serve` / 本地 dev 文档级策略 |

浏览器定位（天气）依赖 **HTML 文档** 上的
`Permissions-Policy: geolocation=(self)`，而不是仅 API 响应头。
因此 proxy 会在转发 frontend 时注入该头（若缺失）。

统一策略字符串（三处保持一致）：

```text
geolocation=(self), microphone=(), camera=()
```

含义：本站可请求定位；麦克风/摄像头仍禁用。

## 后端已添加的响应头

- `Content-Security-Policy`：生产环境默认启用，`connect-src` 可用
  `CSP_CONNECT_SRC` 覆盖。
- **CSP `img-src` dual-path (MYR-039)**：生产/开发均使用
  `img-src 'self' data: blob: https: http:`（**不是**裸 `*`）。
  前端 dual-path 图片策略对非热链主机使用**原始 https URL** 作为 `<img src>`，
  因此 CSP 必须允许远程 `https:`/`http:` 图片；不能收紧为仅 `'self'` 或仅
  代理路径，否则 RSS/博客封面与外链头像会空白。若未来强制全部走
  `/api/proxy/image`，才可再收窄 `img-src`。
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `X-XSS-Protection: 1; mode=block`
- `Referrer-Policy: strict-origin-when-cross-origin`
- `Permissions-Policy: geolocation=(self), microphone=(), camera=()`（本站可请求定位，用于天气等）
- `Strict-Transport-Security`：仅 `ENVIRONMENT=production` 时添加。

开发环境默认不启用 CSP。如需本地验证 CSP：

```env
ENABLE_CSP_DEV=true
```

### 天气定位与安全头 / proxy

1. **浏览器定位**：需 secure context（HTTPS 或 localhost）+ 文档级
   `geolocation=(self)`。不经 proxy 解析 IP。
2. **IP 兜底**：`GET /api/proxy/client-geo` 经 proxy 解析真客户端 IP
   （`X-Real-IP` / 可信 XFF / CDN 头）。见下文「Docker + 宿主反向代理」。
3. **外层 Nginx/Caddy**：不要设置 `Permissions-Policy: geolocation=()`，否则会覆盖
   Myriad 允许本站定位的策略。若外层自行加 Permissions-Policy，请使用与上表相同的
   `geolocation=(self), microphone=(), camera=()`。

## 当前生产拓扑

```text
client -> optional TLS entrypoint -> Myriad proxy:${HTTP_PORT:-80}
                                    ├-> frontend:1102  (+ Permissions-Policy if missing)
                                    └-> backend:1103   (full security headers)
```

外层 Nginx/Caddy/负载均衡器如果存在，应**整站**代理到 Myriad `proxy` 的宿主端口，
不要直接代理到 backend `1103`，否则会绕过维护页和 updater 救援路径。
不要只转发 `/api`：联邦还依赖：

| 路径 | 用途 |
| --- | --- |
| `/.well-known/webfinger` | 发现 |
| `/.well-known/nodeinfo` | NodeInfo 发现 |
| `/nodeinfo/2.1` | NodeInfo 文档 |
| `/inbox` | 共享 Inbox |
| `/users/*` | Actor / outbox / followers / avatar |
| `/media/federation/*` | **Note 附件媒体（图片/视频公开 GET）** |

完整表见 [PORTS.md](../deployment/PORTS.md)。漏掉 `/media/federation/*` 时，联邦发帖可成功，但时间线图片会空白（请求落到 SPA）。
只应信任实际代理节点，并在防火墙中限制 `HTTP_PORT` 不能被客户端绕过代理直连。

### Docker + 宿主反向代理（常见天气定位错误）

典型拓扑：

```text
client → 宿主 Nginx/Caddy → Docker 发布的 proxy 端口 → backend
```

此时 proxy 容器看到的 TCP 对端往往是 Docker 网桥地址（例如 `172.17.0.1`），
而不是真实客户端。若外层已正确设置 `X-Real-IP` / `X-Forwarded-For`：

- **`PROXY_TRUSTED_UPSTREAMS` 为空（默认）**：proxy 不信任任何转发头，全部根据
  TCP 对端、请求协议和 `Host` 重建。这样即使直接暴露 proxy，客户端也不能伪造来源。
- **显式填写 CIDR**：仅允许列表中的上游传递转发头。Docker 宿主反向代理也必须
  显式列出它在容器视角下的源地址；例如外层代理源地址是 `192.0.2.10`：

```env
PROXY_TRUSTED_UPSTREAMS=192.0.2.10/32
```

存在多层可信代理时，把各层地址或网段都列出。Myriad 会从
`X-Forwarded-For` 右侧依次剥离这些可信代理，得到最靠近用户的非代理地址。
可信对端时也会读取 `CF-Connecting-IP` / `True-Client-IP`（CDN）。

**切勿**将 `PROXY_TRUSTED_UPSTREAMS` 设为 `0.0.0.0/0`：那会允许任意客户端
伪造 `X-Forwarded-For`。

#### Backend 也必须信任 proxy 改写后的客户端 IP

链路第二段：

```text
proxy → backend（compose 内网）
```

proxy 会把解析到的访客 IP 写入 `X-Real-IP` / `X-Forwarded-For` 再转给
backend。backend 侧：

| 变量 | 作用 |
| --- | --- |
| `TRUST_PROXY_HEADERS=true` | 允许从可信 peer 读取转发头（compose 默认开启） |
| `TRUST_PROXY_PEERS` | 可信任的 TCP peer CIDR。**空 = 窄默认**（loopback + docker0，非 RFC1918 全段）。compose 默认含 `172.28.0.0/16`（myriad-net） |

库存 compose 的完整两段示例（实际地址必须以部署网络为准）：

```env
# 宿主 Nginx/Caddy → myriad-proxy
PROXY_TRUSTED_UPSTREAMS=172.17.0.1/32
# myriad-proxy → backend
TRUST_PROXY_HEADERS=true
TRUST_PROXY_PEERS=172.28.0.0/16
```

compose 不为第一段写死默认 CIDR：宿主网关地址会随 Docker 网络、rootless
模式和发布端口实现变化，而且 proxy 也可能被直接暴露。错误的“常见默认”要么
不起作用，要么扩大伪造转发头的范围，因此必须显式配置。

若 `TRUST_PROXY_HEADERS` 关闭，或 peer 不在信任范围，backend 会把 Docker 内网
地址当成「客户端 IP」，`/api/proxy/client-geo` 再回退到**服务器出口公网 IP**，
天气/欢迎语就会显示机房城市而不是访客位置。响应里会带
`source: "server-egress"`；前端会软失败并尝试浏览器侧 IP 定位。

排查：

```bash
# backend 日志应看到 resolved=公网访客 IP，而不是 10.x / 172.x
docker logs myriad-backend 2>&1 | grep -i 'Client IP detection'

# 经 proxy 打 client-geo：ip 应为访客公网，source 应为 client-ip
curl -sS http://127.0.0.1:${HTTP_PORT:-80}/api/proxy/client-geo
```

## Nginx TLS 入口示例

```nginx
server {
    listen 443 ssl http2;
    server_name your-domain.com;

    ssl_certificate     /etc/letsencrypt/live/your-domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/your-domain.com/privkey.pem;

    # Federation media upload is up to ~50MB (images 10MB / video 50MB).
    # .tapp file install accepts game packages up to 128 MiB.
    # Chat file-meta chunks are ~1.4 MiB JSON each (still needs >1m default).
    # Default nginx 1m will break POST /api/federation/media, /api/tapps/install-file,
    # and transfer chunks.
    client_max_body_size 130m;

    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;

    # Prefer whole-site → Myriad proxy (handles SPA + AP + media routing).
    location / {
        proxy_pass http://127.0.0.1:80; # Myriad HTTP_PORT
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        # WebSocket (federation WS under /api/federation/*/ws)
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        # Long enough for WS + multi-minute file download streams
        # (GET /api/federation/transfers/{id}/content is streamed, not buffered).
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
        # Do not buffer large transfer downloads into temp files on the edge.
        proxy_request_buffering off;
        proxy_buffering off;
    }
}

# Optional map for Connection upgrade (http context):
# map $http_upgrade $connection_upgrade {
#     default upgrade;
#     ''      close;
# }
```

如果 `.env` 中设置了 `HTTP_PORT=8080`，把 `proxy_pass` 改为
`http://127.0.0.1:8080`。在 Docker 宿主 Nginx 场景下，应把宿主代理在容器
视角下的固定源地址或最窄 CIDR 写入 `PROXY_TRUSTED_UPSTREAMS`；保持为空时
转发头会被忽略，定位和审计会使用 Docker 网桥对端地址。

**不要**写成分路径只放行 `/api`（除非你完整复制 [PORTS.md](../deployment/PORTS.md) 的 backend 白名单，且包含 `/media/federation/`）。

## 验证

生产路径：

```bash
curl -I https://your-domain.com
```

本机直接验证 proxy：

```bash
curl -I http://localhost:${HTTP_PORT:-80}
```

本地开发直接验证 backend：

```bash
curl -I http://localhost:1103/health
```

应该能看到 `x-frame-options`、`x-content-type-options`、`referrer-policy`、
`permissions-policy` 等响应头。生产环境还应看到
`strict-transport-security`。

## 相关配置

- `ENVIRONMENT=production`：启用生产 CSP 和 HSTS。
- `CSP_CONNECT_SRC`：覆盖生产 CSP 的 `connect-src`，默认为 `'self' https:`。
- `ENABLE_CSP_DEV=true`：开发环境也启用 CSP。
- `PROXY_TRUSTED_UPSTREAMS`：允许传递真实客户端 IP 的外层代理 IP/CIDR 列表。
  留空时不信任任何转发头；切勿设为 `0.0.0.0/0`。

更多部署细节见 [Docker 部署](../deployment/DOCKER_DEPLOYMENT.md) 和
[端口清单](../deployment/PORTS.md)。
