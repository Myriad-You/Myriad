# 安全响应头配置指南

当前状态：安全响应头已经由后端中间件
`backend/src/middleware/security.rs` 添加，并在 `backend/src/main.rs` 的 router
上启用。生产部署中，浏览器请求先进入 Myriad `proxy`，再转发到 backend/frontend。

## 后端已添加的响应头

- `Content-Security-Policy`：生产环境默认启用，`connect-src` 可用
  `CSP_CONNECT_SRC` 覆盖。
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `X-XSS-Protection: 1; mode=block`
- `Referrer-Policy: strict-origin-when-cross-origin`
- `Permissions-Policy: geolocation=(), microphone=(), camera=()`
- `Strict-Transport-Security`：仅 `ENVIRONMENT=production` 时添加。

开发环境默认不启用 CSP。如需本地验证 CSP：

```env
ENABLE_CSP_DEV=true
```

## 当前生产拓扑

```text
client -> optional TLS entrypoint -> Myriad proxy:${HTTP_PORT:-80}
                                    ├-> frontend:1102
                                    └-> backend:1103
```

外层 Nginx/Caddy/负载均衡器如果存在，应代理到 Myriad `proxy` 的宿主端口，
不要直接代理到 backend `1103`，否则会绕过维护页和 updater 救援路径。
只应信任实际代理节点，并在防火墙中限制 `HTTP_PORT` 不能被客户端绕过代理直连。

### Docker + 宿主反向代理（常见天气定位错误）

典型拓扑：

```text
client → 宿主 Nginx/Caddy → Docker 发布的 proxy 端口 → backend
```

此时 proxy 容器看到的 TCP 对端往往是 Docker 网桥地址（例如 `172.17.0.1`），
而不是真实客户端。若外层已正确设置 `X-Real-IP` / `X-Forwarded-For`：

- **`PROXY_TRUSTED_UPSTREAMS` 为空（默认）**：proxy 会**仅对**私网 / loopback /
  link-local 对端信任转发头（含上述 Docker 网桥场景），从 `X-Forwarded-For`
  右侧剥离可信跳，得到真实客户端 IP。公网对端仍不能伪造头。
- **显式填写 CIDR**：仅允许列表中的上游传递转发头（显式 allowlist，不再自动
  信任私网对端）。例如外层代理源地址是 `192.0.2.10`：

```env
PROXY_TRUSTED_UPSTREAMS=192.0.2.10/32
```

存在多层可信代理时，把各层地址或网段都列出。Myriad 会从
`X-Forwarded-For` 右侧依次剥离这些可信代理，得到最靠近用户的非代理地址。
可信对端时也会读取 `CF-Connecting-IP` / `True-Client-IP`（CDN）。

**切勿**将 `PROXY_TRUSTED_UPSTREAMS` 设为 `0.0.0.0/0`：那会允许任意客户端
伪造 `X-Forwarded-For`。

## Nginx TLS 入口示例

```nginx
server {
    listen 443 ssl http2;
    server_name your-domain.com;

    ssl_certificate     /etc/letsencrypt/live/your-domain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/your-domain.com/privkey.pem;

    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains" always;

    location / {
        proxy_pass http://127.0.0.1:80; # Myriad HTTP_PORT
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

如果 `.env` 中设置了 `HTTP_PORT=8080`，把 `proxy_pass` 改为
`http://127.0.0.1:8080`。在 Docker 宿主 Nginx 场景下，默认空的
`PROXY_TRUSTED_UPSTREAMS` 即可；仅在上游是公网 IP 或需要收紧信任范围时
再填写显式 CIDR。

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
  留空时仅自动信任私网/loopback/link-local 对端；切勿设为 `0.0.0.0/0`。

更多部署细节见 [Docker 部署](../deployment/DOCKER_DEPLOYMENT.md) 和
[端口清单](../deployment/PORTS.md)。
