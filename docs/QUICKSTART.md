# Myriad 快速开始

Myriad 当前生产部署使用 **proxy + updater** 拓扑：

```text
host HTTP_PORT
  |
  v
proxy ──┬── frontend:1102
        ├── backend:1103 ── postgres:5432
        └── updater:1101 (internal only)
```

生产环境只有 `proxy` 暴露宿主端口。不要暴露 backend/frontend 端口，也不要用
`:latest` 覆盖正在运行的容器。完整端口表见
[deployment/PORTS.md](./deployment/PORTS.md)。

## 生产部署

### 1. 准备仓库和配置

```bash
git clone https://github.com/myriad-you/Myriad.git
cd Myriad

cp .env.production.example .env
```

编辑 `.env`，至少填好：

```env
POSTGRES_PASSWORD=<32+ chars>
JWT_SECRET=<32+ chars>
CORS_ORIGINS=https://yourdomain.com
BASE_URL=https://yourdomain.com
FRONTEND_URL=https://yourdomain.com
```

可用下面的命令生成密码，然后把结果填进 `.env`：

```bash
openssl rand -base64 32  # POSTGRES_PASSWORD
openssl rand -base64 32  # JWT_SECRET
```

`UPDATE_TOKEN` 与 `UPDATER_GATEWAY_SECRET` 可以留空；`scripts/docker/deploy.sh up` 会在首次启动时生成。

`BASE_URL` 是联邦发现地址的来源。Aro 里显示的 Actor URL 会形如
`https://yourdomain.com/users/<username>`，别人可以用这个 Actor URL 或
`@<username>@yourdomain.com` 来关注、建频道、邀请进房间或加入环网。

### 2. 启动

```bash
bash scripts/docker/deploy.sh up
```

Windows:

```powershell
.\scripts\docker\deploy.ps1 up
```

打开 `http://localhost`，或 `.env` 中 `HTTP_PORT` 指向的端口。首次访问会进入初始化向导。

### 3. 常用运维命令

```bash
bash scripts/docker/deploy.sh status
bash scripts/docker/deploy.sh logs
bash scripts/docker/deploy.sh restart
bash scripts/docker/deploy.sh down
```

手动切换镜像 tag 时，先编辑 `.env` 里的 `MYRIAD_TAG` / `PROXY_TAG` /
`UPDATER_TAG`，再执行：

```bash
bash scripts/docker/deploy.sh upgrade
```

日常更新应从管理员界面执行：`/config` -> `关于` -> `更新管理`。
该面板通过 backend 的 `/api/admin/updater/*` 通道访问 updater-gateway，浏览器不会接触
`UPDATE_TOKEN` 或 `UPDATER_GATEWAY_SECRET`。

## 开发环境

```bash
# 本地开发只启动 PostgreSQL，不启动生产 proxy/updater
docker compose -f docker-compose.dev.yml up -d postgres

# 后端：读取 backend/.env，监听 1103
(cd backend && cp .env.example .env && cargo run)

# 前端：监听 1102，/api/* 通过 Astro dev proxy 转发到 1103
(cd frontend && pnpm install && pnpm dev)
```

也可以用开发脚本：

```bash
./scripts/dev/dev.sh start
./scripts/dev/dev.sh status
```

Windows:

```powershell
.\scripts\dev\dev.ps1 start
.\scripts\dev\dev.ps1 status
```

需要在开发 UI 里测试“更新管理”时，启动 updater harness：

```bash
./scripts/dev/dev.sh start all-updater
```

真实镜像替换、维护模式、`pgdata` 快照和回滚仍应使用生产栈
`scripts/docker/deploy.sh` 验证。

## 数据备份

```bash
mkdir -p backups
docker compose exec -T postgres pg_dump -U myriad -d myriad > "backups/backup_$(date +%Y%m%d_%H%M%S).sql"
```

恢复：

```bash
docker compose exec -T postgres psql -U myriad -d myriad < backups/backup_20240101_120000.sql
```

## 清理数据

以下操作会删除应用数据：

```bash
bash scripts/docker/deploy.sh down
docker compose down -v
rm -rf pgdata state backups
```

再次启动：

```bash
bash scripts/docker/deploy.sh up
```

## 常见问题

### 端口已被占用

```bash
sudo lsof -i :80
```

修改 `.env`：

```env
HTTP_PORT=8080
```

然后重启：

```bash
bash scripts/docker/deploy.sh restart
```

### 容器启动失败

```bash
bash scripts/docker/deploy.sh status
bash scripts/docker/deploy.sh logs
docker compose config
```

如果只想看某个服务：

```bash
docker compose logs backend
docker compose logs postgres
```

### 数据库连接失败

```bash
docker compose exec postgres psql -U myriad -d myriad -c "SELECT 1"
docker compose logs postgres
docker compose exec backend env | grep DATABASE_URL
```

### CORS 错误

确认 `.env` 中 `CORS_ORIGINS` 是用户实际访问 Myriad 的 origin：

```env
CORS_ORIGINS=https://yourdomain.com,https://www.yourdomain.com
```

本地开发后端可用：

```env
CORS_ORIGINS=http://localhost:1102,http://localhost:1103
```

## 生产检查清单

- `POSTGRES_PASSWORD` 长度至少 32 字符。
- `JWT_SECRET` 长度至少 32 字符。
- `CORS_ORIGINS` 是真实访问域名，不要用 `*`。
- `UPDATE_TOKEN` 与 `UPDATER_GATEWAY_SECRET` 已由部署脚本生成，或已手动设置为 32+ 字符。
- `pgdata` 使用仓库根目录下的 `./pgdata` bind mount。
- 外层 HTTPS/TLS 入口代理到 Myriad `proxy` 的 `HTTP_PORT`，不是 backend `1103`。
- `.env` 未提交到 Git，生产机上建议 `chmod 600 .env`。

## 更多文档

- [Docker 部署](deployment/DOCKER_DEPLOYMENT.md)
- [端口清单](deployment/PORTS.md)
- [Updater 运维](UPDATER_QUICKSTART.md)
- [README](../README.md)
