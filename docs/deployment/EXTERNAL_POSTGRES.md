# 外部 PostgreSQL 部署（1Panel / Docker Compose）

本文说明如何把 Myriad 的业务数据库放到 **Compose 栈之外** 的 PostgreSQL（云 RDS、1Panel 应用商店、宿主机 Postgres、独立 DB 容器等），同时继续使用 updater 管理镜像 tag 与维护模式。

默认生产栈见 [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md)（栈内 `postgres` + `./pgdata` 快照）。  
Updater 日常操作见 [UPDATER_QUICKSTART.md](./UPDATER_QUICKSTART.md)。

> **契约摘要**  
> - `MYRIAD_DB_MODE=external`：updater **跳过** `pgdata` 快照 / 恢复；业务更新仍切换镜像 tag 与维护模式。  
> - **无** 本地 `postgres` 服务；backend **不** `depends_on` postgres。  
> - **`DATABASE_URL` 是 backend 的连接真相源**（不要依赖「拼出来的」默认串覆盖外部库）。  
> - **数据库备份由运维自管**（`pg_dump` / 云快照 / 1Panel 备份），Myriad updater 不再替你做库级回滚。

---

## 何时使用

| 场景 | 推荐 |
| --- | --- |
| 单机快速自托管、接受 `./pgdata` bind | 默认栈内 `postgres` |
| 1Panel / 已有 Postgres、云 RDS、多实例共用库 | **外部 DB**（本文） |
| 需要 updater 自动 `pgdata` 快照回滚 | 必须用栈内 `postgres` + bind mount |

---

## 拓扑对比

### 默认（栈内 Postgres）

```text
proxy ──┬── frontend
        └── backend ──► postgres:5432   (./pgdata bind)
updater ──► docker-guard ── sock
            + 更新前快照 / 回滚恢复 pgdata
```

### 外部 Postgres

```text
proxy ──┬── frontend
        └── backend ──► DATABASE_URL ──► 外部 PostgreSQL
updater ──► docker-guard ── sock
            + 仍管理 MYRIAD_TAG / PROXY_TAG / 维护模式；TCB 看 UPDATER_IMAGE_REF
            + MYRIAD_DB_MODE=external → 不碰 pgdata 快照
```

业务网络 `myriad-net` 上 **不再** 有 `postgres` 成员；`myriad-admin-net` / `myriad-docker-guard-net` 与默认栈一致。

---

## 环境变量契约

在部署目录 `.env`（或 compose `environment`）中设置：

| 变量 | 必需 | 说明 |
| --- | --- | --- |
| `MYRIAD_DB_MODE` | 是（外部模式） | 设为 `external`。updater 跳过 pgdata 快照/恢复。缺省或其它值按本地库模式处理。 |
| `DATABASE_URL` | 是 | backend 唯一连接串，例如 `postgres://user:pass@host:5432/myriad?sslmode=require` |
| `JWT_SECRET` / `CORS_ORIGINS` / tags / tokens | 同默认部署 | 见 [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md) 环境表 |

说明：

- **不要** 再依赖 compose 里写死的  
  `postgres://myriad:${POSTGRES_PASSWORD}@postgres:5432/myriad`  
  作为外部库连接；请显式写 `DATABASE_URL`。
- `POSTGRES_PASSWORD` 在外部模式下 **不必** 再填（除非你还在别处复用同名 secret）。栈内无 `postgres` 服务时它不会被使用。
- `UPDATER_PGDATA` 在 `MYRIAD_DB_MODE=external` 下 **可省略**；若仍写在 compose 里，updater 也应忽略路径（实现侧以 `MYRIAD_DB_MODE` 为准）。空目录 `./pgdata` **不要** 长期留着当「假库」——见下方运维注意。

示例片段：

```bash
# .env（外部 DB）
MYRIAD_DB_MODE=external

# 云 RDS / 1Panel Postgres / 宿主机端口 等
DATABASE_URL=postgres://myriad:CHANGE_ME@192.168.1.10:5432/myriad?sslmode=prefer

# 其余与默认部署相同（版本号请换成当前 release）
MYRIAD_TAG=v0.4.1
PROXY_TAG=v0.4.1
UPDATER_TAG=v0.4.1
# 生产 TCB 以 digest 为准，不要只靠 UPDATER_TAG：
# UPDATER_IMAGE_REF=docker.io/somekawahitomi/myriad-updater@sha256:...
# DOCKER_GUARD_IMAGE=docker.io/somekawahitomi/myriad-updater@sha256:...
JWT_SECRET=...
CORS_ORIGINS=https://example.com
UPDATE_TOKEN=...
UPDATER_GATEWAY_SECRET=...
```

---

## Compose 形态

完整示例（无 `postgres` 服务、backend 无 postgres `depends_on`、updater 注释说明 external 跳过 pgdata）：

**[examples/docker-compose.external-db.example.yml](./examples/docker-compose.external-db.example.yml)**

与默认生产栈一致的服务：

| 服务 | 外部 DB 下的变化 |
| --- | --- |
| `postgres` | **删除** |
| `backend` | `DATABASE_URL` 来自 env；**去掉** `depends_on: postgres`；仍依赖 `backend-volume-init` |
| `frontend` / `proxy` | 不变 |
| `docker-guard` / `updater` / `updater-gateway` | 拓扑不变；updater 设 `MYRIAD_DB_MODE=external` |
| `./pgdata` | **不需要** bind；不要挂空目录装样子 |

启动示例（把示例文件当主 compose，或与现有文件合并后）：

```bash
cd /path/to/deploy
cp /path/to/repo/docs/deployment/examples/docker-compose.external-db.example.yml docker-compose.yml
# 编辑 .env：MYRIAD_DB_MODE、DATABASE_URL、密钥与 tags

docker compose --env-file .env pull
docker compose --env-file .env up -d
# 或（若仍使用仓库脚本且 compose 文件名兼容）：
# bash scripts/extra/deploy.sh up
```

> **1Panel**：用「编排」导入上述 compose + `.env` 即可。应用商店里的 Postgres 与 Myriad 栈 **分开** 创建；Myriad 栈内不要再勾选/附带 postgres 容器。网络互通见下一节。

---

## 连通性：host / 网关 / SSL

Backend 容器访问外部库时，hostname 必须是 **从容器内** 可解析、可路由的地址。

| 外部库位置 | `DATABASE_URL` host 写法 | 备注 |
| --- | --- | --- |
| 同一 Docker 网络上的其它容器 | 容器名或服务名 | 把 backend 加入对方网络，或把对方加入 `myriad-net` |
| 宿主机上的 Postgres（Linux） | 宿主机网桥网关，常见 `172.17.0.1` 或 compose 网络网关 | 也可用 `extra_hosts: ["host.docker.internal:host-gateway"]` 后写 `host.docker.internal` |
| 宿主机（Docker Desktop / 新版 Engine） | `host.docker.internal` | 建议显式 `extra_hosts: ["host.docker.internal:host-gateway"]` |
| 局域网 / 云 RDS | 内网 IP 或域名 | 安全组放行 **backend 出口** → DB 端口；优先私网 |

`extra_hosts` 示例（写在 `backend` 服务下）：

```yaml
extra_hosts:
  - "host.docker.internal:host-gateway"
```

对应：

```bash
DATABASE_URL=postgres://myriad:secret@host.docker.internal:5432/myriad?sslmode=disable
```

### `sslmode`

| 场景 | 建议 |
| --- | --- |
| 公网 RDS / 强制 TLS | `sslmode=require` 或 `verify-full`（按云厂商文档带 CA） |
| 同机 / 内网明文 | `sslmode=disable` 或 `prefer`（按库配置） |
| 不确定 | 先在 backend 容器内用 `psql`/`wget` 探测，再写入 `DATABASE_URL` |

把密码 URL 编码（`@` → `%40` 等），避免连接串被截断。

---

## 运维 Runbook

### 1. 从 backend 容器内核实 `DATABASE_URL`

```bash
docker compose exec backend env | grep -E '^(DATABASE_URL|MYRIAD_DB_MODE)='
```

应看到 `MYRIAD_DB_MODE=external` 与完整连接串（日志/工单里注意脱敏）。

健康检查：

```bash
docker compose exec backend wget -qO- http://localhost:1103/health
# 或经 proxy：
curl -fsS "http://127.0.0.1:${HTTP_PORT:-80}/health"
```

若 backend 反复 unhealthy，优先查：DNS、防火墙、认证、库名、`sslmode`、是否仍指向已删除的 `postgres` 主机名。

### 2. 连通性快速探测

在 **backend 容器内**（与进程同一网络命名空间）探测 TCP：

```bash
# 将 host/port 换成 DATABASE_URL 中的值
docker compose exec backend sh -c 'wget -qO- --timeout=3 http://127.0.0.1:1103/health >/dev/null && echo backend_ok'
# 若镜像带有 nc/psql 可进一步测 DB；没有则用宿主机：
psql "$DATABASE_URL" -c 'select 1'
```

### 3. 备份与回滚（用户自管）

`MYRIAD_DB_MODE=external` 时：

- Updater **不会** 在升级前 snapshot `./pgdata`，也 **不会** 在失败时用 pgdata 回滚库。
- 升级前自行备份，例如：

```bash
pg_dump "$DATABASE_URL" -Fc -f "backups/myriad_$(date +%Y%m%d_%H%M%S).dump"
# 或 SQL：
# pg_dump "$DATABASE_URL" > "backups/myriad_$(date +%Y%m%d_%H%M%S).sql"
```

云 RDS：用厂商自动备份 / PITR。1Panel：用面板备份任务或上述 `pg_dump`。

应用层回滚（镜像 tag）仍可由 updater 执行；**数据** 一致性需你自己的 dump/PITR 策略。

### 4. 不要留下无用的 `./pgdata`

- 外部模式下 **不要** 创建空的 `./pgdata` 挂进任何服务。
- 若从栈内 Postgres **迁出** 后：确认外部库数据完整、backend 已切 `DATABASE_URL`，再归档并删除旧 `./pgdata`（先备份再删）。
- 避免 updater/运维误以为本地库仍在服务流量。

### 5. 从栈内 Postgres 迁到外部（概要）

1. 维护窗口：`docker compose stop frontend backend`（或整栈 stop，按你的流程）。  
2. `pg_dump` 栈内库 → 导入外部 Postgres（建库/用户/权限先就绪）。  
3. 换用外部 compose（无 `postgres` 服务），设置 `MYRIAD_DB_MODE=external` 与 `DATABASE_URL`。  
4. 使用 `docker compose --env-file .env up -d`（若 `./guard-policy/docker-guard.env` 已存在可再加 `--env-file ./guard-policy/docker-guard.env`），执行上一节容器内校验。
5. 确认无误后处理旧 `./pgdata`（备份后删除或离线归档）。

Schema migration 仍由 **backend 启动路径** 负责（与默认部署相同）；确保外部库用户具备所需 DDL 权限。

### 6. Updater 行为（期望）

| 能力 | 本地 `postgres` + pgdata | `MYRIAD_DB_MODE=external` |
| --- | --- | --- |
| 维护模式 / 停业务容器 | 是 | 是 |
| 切换 `MYRIAD_TAG` 等镜像 tag | 是 | 是 |
| `pgdata` 快照 / 回滚恢复 | 是 | **否（跳过）** |
| 库备份 | 快照近似「整库文件」 | **运维 `pg_dump`/云备份** |

外部库模式下请 **手动** 备份（`pg_dump` / 云快照），不要点 UI 里的「pgdata 回滚」。

---

## 安全注意

- `DATABASE_URL` 含密码：权限收紧 `.env`（如 `chmod 600`），勿提交 git，勿贴进公开 issue。  
- 不要对公网暴露 Postgres `5432`；优先私网 + 安全组。  
- `docker-guard` 允许镜像列表里的 `postgres` 字符串仅影响 **允许拉取的镜像名**；外部模式下栈内无 postgres 服务即可。  
- 其余 updater / 三网 / token 红线见 [UPDATER_SECURITY_BASELINE.md](./UPDATER_SECURITY_BASELINE.md)。

---

## 相关文档

- [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md) — 默认生产拓扑与环境变量  
- [UPDATER_QUICKSTART.md](./UPDATER_QUICKSTART.md) — 更新 / 回滚 / 救援  
- [updater-spec.md](../updater-spec.md) — updater 协议与快照设计（本地 pgdata）  
- [examples/docker-compose.external-db.example.yml](./examples/docker-compose.external-db.example.yml) — 外部 DB compose 示例  
- 仓库根 [`docker-compose.yml`](../../docker-compose.yml) — 默认栈内 Postgres 拓扑  
