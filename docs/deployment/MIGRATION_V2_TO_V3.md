# 从 v2（updater 直挂 sock）迁移到 v3（docker-guard + gateway + 三网）

本文面向自托管运维：在**同一部署目录**把旧拓扑迁到当前生产拓扑，保留
`pgdata`、`.env` 密钥与业务数据。

> **Admin UI alone 无法切换拓扑。**  

> 网络、服务定义、volume 挂载必须以宿主上的 `docker-compose.yml` 为准，执行
> `docker compose pull && docker compose up -d`（或 `bash scripts/docker/deploy.sh upgrade`）。
> 仅改 `MYRIAD_TAG` / 仅点「检查更新」**不会**创建 `docker-guard`、
> `updater-gateway` 或三张网络。

英文分步细节（guard 阶段 + admin-net 跟进）见
[MIGRATION_DOCKER_GUARD.md](./MIGRATION_DOCKER_GUARD.md)。  
当前拓扑总览见 [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md)。

## 示例文件

| 文件 | 用途 |
| --- | --- |
| [examples/v3-docker-compose.yml](./examples/v3-docker-compose.yml) | 与仓库根 `docker-compose.yml` 一致的 v3 拓扑快照（中文运维头注释） |
| [examples/v3.env.example](./examples/v3.env.example) | 按 kiseki.blog 运维形状脱敏的 `.env` 示例（`v0.3.1` 统一 tag / preview / `HTTP_PORT=8080`） |

复制 compose 时可用仓库根文件，也可用 `examples/v3-docker-compose.yml`（内容对齐当前根文件）。

## v2 → v3 差异一览

| 项目 | v2（旧） | v3（当前） |
| --- | --- | --- |
| Docker socket | `updater` 直接挂 `/var/run/docker.sock` | 仅 **`docker-guard`** 挂 sock |
| 网络 | 多为单业务网 + updater 同网 | **三网**：`myriad-net` / `myriad-admin-net` / `myriad-docker-guard-net`（internal） |
| updater 位置 | 常与 frontend/postgres 同 L2 | **离开业务网**；只在 admin-net + guard-net |
| backend 调更新 | 可能直接持有 `UPDATE_TOKEN` 打 updater | backend → **`updater-gateway:1104`**（注入 token）；backend **不**持有 `UPDATE_TOKEN` |
| 新增密钥 | — | **`UPDATER_GATEWAY_SECRET`**（backend ↔ gateway） |
| 自更新重建对象 | 主要是 `updater` | `docker-guard` + `updater` + `updater-gateway`（同一 `UPDATER_TAG`） |
| 宿主端口 | 仅 proxy 发布 HTTP（保持） | 不变；**切勿**发布 `1101` / `1104` / `2375` |

```text
v3 数据面 / 控制面：

host HTTP_PORT
  │
  ▼
proxy ──┬── frontend:1102          [myriad-net]
        ├── backend:1103 ── postgres
        │       │
        │       └── updater-gateway:1104 ──► updater:1101   [myriad-admin-net]
        │                                         │
        │                                         └── docker-guard:2375 ── sock
        │                                               [myriad-docker-guard-net]
        └── (救援，默认关) ──► updater  when PROXY_ALLOW_DIRECT_UPDATER=true
```

## 密钥策略（尽量少动）

**保留**现有生产 secret，不要轮换（除非已泄露）：

- `POSTGRES_PASSWORD`
- `JWT_SECRET`
- `UPDATE_TOKEN`
- 以及你自行配置的 OAuth / 平台密钥等

**仅新增**（若 `.env` 尚无）：

```bash
# ≥32 字符；也可用 deploy 脚本在空值时自动生成
UPDATER_GATEWAY_SECRET=   # openssl rand -base64 36 | tr -d '\n=' | tr '+/' '-_'
```

不要把真实密钥写进文档或 git。对照脱敏模板：
[examples/v3.env.example](./examples/v3.env.example)。

## 前置条件

1. Docker Engine + `docker compose` v2  
2. `UPDATER_TAG` 指向的镜像**必须**包含：
   - `myriad-docker-guard`
   - `myriad-updater-gateway`  
   以及原有 updater 守护进程。过旧的 updater-only 镜像无法拉起 v3。  
3. 建议备份：

```bash
cd /path/to/myriad   # 含 docker-compose.yml 与 .env 的部署根
mkdir -p backups
docker compose exec -T postgres pg_dump -U myriad -d myriad \
  > "backups/pre-v3-migration_$(date +%Y%m%d_%H%M%S).sql"
cp -a .env "backups/env.pre-v3.$(date +%Y%m%d_%H%M%S)"
```

## 迁移步骤（同目录、不停换盘）

1. **停止栈**（数据与 bind 目录留在磁盘）：

   ```bash
   docker compose down
   # 或: bash scripts/docker/deploy.sh down
   ```

2. **更新部署文件**到含 v3 拓扑的版本（根 `docker-compose.yml` 或复制
   [examples/v3-docker-compose.yml](./examples/v3-docker-compose.yml)）：
   - 服务：`docker-guard`、`updater-gateway`
   - updater：`DOCKER_HOST=tcp://docker-guard:2375`，**无** raw sock 挂载
   - backend：`MYRIAD_UPDATER_URL=http://updater-gateway:1104` +
     `UPDATER_GATEWAY_SECRET`，**无** `UPDATE_TOKEN`
   - 网络：三网定义（见上表）

   典型做法：`git pull` / 解压 release 到**同一目录**，或只替换
   `docker-compose.yml` 与 `scripts/docker/deploy.*`，**不要**覆盖
   `.env`、`pgdata/`、`state/`、`backups/`。

3. **编辑 `.env`（保留旧 secret，只加缺口）**：

   ```bash
   # 必查：版本与 token
   grep -E '^(MYRIAD_TAG|PROXY_TAG|UPDATER_TAG|UPDATE_TOKEN|UPDATER_GATEWAY_SECRET|COMPOSE_PROJECT_NAME)=' .env

   # 若无 UPDATER_GATEWAY_SECRET 行，追加空键即可，由 deploy 生成：
   # echo 'UPDATER_GATEWAY_SECRET=' >> .env
   ```

   可选对照 [examples/v3.env.example](./examples/v3.env.example) 补注释项
   （`PROXY_TRUSTED_UPSTREAMS`、三网名、`CHANNEL` / `UPDATE_MODE` 等）。  
   示例中的 `MYRIAD_TAG` / `PROXY_TAG` / `UPDATER_TAG=v0.3.1`、`CHANNEL=preview`、
   `UPDATE_MODE=commit`、`HTTP_PORT=8080`、`https://kiseki.blog` 仅为运维形状参考，
   **请改成你的实际值**。

4. **拉起并校正卷权限**（backend 以 uid 1000 运行）：

   ```bash
   bash scripts/docker/deploy.sh up
   # 或:
   # bash scripts/docker/deploy.sh upgrade
   ```

5. **验收**：

   ```bash
   bash scripts/docker/deploy.sh doctor
   docker compose ps
   # guard 健康；updater / gateway 无 docker.sock
   docker inspect myriad-updater --format '{{json .Mounts}}' | head
   docker inspect myriad-docker-guard --format '{{json .Mounts}}' | head
   docker inspect myriad-updater-gateway --format '{{json .Mounts}}' | head
   curl -fsS "http://localhost:${HTTP_PORT:-80}/api/health"
   ```

   期望要点：

   - `doctor` 无 **FAIL**
   - backend 环境**没有** `UPDATE_TOKEN`，有 `UPDATER_GATEWAY_SECRET`
   - updater 不在 `myriad-net`；gateway 仅在 admin-net；sock 仅在 docker-guard
   - 宿主未发布 `1101` / `1104` / `2375`

6. **日常更新**仍走管理后台：

   ```text
   /config → 关于 → 更新管理
   ```

   业务 tag 与自更新仍由 updater 处理；**拓扑变更**（网络/服务增删）仍须宿主 compose。

## 回滚本次迁移

若新 compose 起不来：

1. `docker compose down`
2. 恢复迁移前的 `docker-compose.yml`（必要时恢复 `.env` 中的 tag）
3. `docker compose up -d`

`pgdata` 与既有 secret 在正确迁移流程中不会被拓扑切换改写。  
若已写入新的 `UPDATER_GATEWAY_SECRET` 而回退到无 gateway 的 compose，旧布局一般不读该键，可忽略或保留。

## 运维红线（迁移后）

- 保持 `COSIGN_VERIFY=strict`（除非你明确接受 dual-key 关闭验证）
- 保持 `PROXY_ALLOW_DIRECT_UPDATER=false`（救援时临时打开，事后关掉）
- `UPDATE_TOKEN` / `UPDATER_GATEWAY_SECRET` 不进前端、工单、CI 日志
- 详见 [UPDATER_SECURITY_BASELINE.md](./UPDATER_SECURITY_BASELINE.md)

## 相关文档

- [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md) — 当前生产拓扑与环境变量表
- [MIGRATION_DOCKER_GUARD.md](./MIGRATION_DOCKER_GUARD.md) — guard 迁移 + admin-net/gateway 英文跟进
- [PORTS.md](./PORTS.md) — 端口表
- [UPDATER_QUICKSTART.md](../UPDATER_QUICKSTART.md) — 日常更新与救援
- [updater-spec.md](../updater-spec.md) — 协议与失败模式
