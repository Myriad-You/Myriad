# 外部 PostgreSQL 部署（1Panel / Docker Compose）

本文说明如何把 Myriad 的业务数据库放到 **Compose 栈之外** 的 PostgreSQL（云 RDS、1Panel 应用商店、宿主机 Postgres、独立 DB 容器等）。首次升级 updater/Guard 的要求见下文。

默认生产栈见 [DOCKER_DEPLOYMENT.md](./DOCKER_DEPLOYMENT.md)（栈内 `postgres` + `./pgdata` 快照）。  
Updater 日常操作见 [UPDATER_QUICKSTART.md](./UPDATER_QUICKSTART.md)。

> **契约摘要**  
> - `MYRIAD_DB_MODE=external`：updater **跳过** `pgdata` 快照 / 恢复；业务更新仍切换镜像 tag 与维护模式。  
> - **无** 本地 `postgres` 服务；backend **不** `depends_on` postgres。  
> - **`DATABASE_URL` 是 backend 的连接真相源**（不要依赖「拼出来的」默认串覆盖外部库）。  
> - 两个 worker 分别使用 `FEDERATION_DATABASE_URL` / `PERSONA_DATABASE_URL`，直接连接同一库/schema，网络配置不继承 backend。
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
        ├── backend ──► postgres:5432   (./pgdata bind)
        ├── federation-worker ──► postgres:5432
        └── persona-worker ──► postgres:5432
updater ──► docker-guard ── sock
            + 更新前快照 / 回滚恢复 pgdata
```

### 外部 Postgres

```text
proxy ──┬── frontend
        ├── backend ──► DATABASE_URL ──► 外部 PostgreSQL
        ├── federation-worker ──► FEDERATION_DATABASE_URL ──► 同一外部 PostgreSQL
        └── persona-worker ──► PERSONA_DATABASE_URL ──► 同一外部 PostgreSQL
updater ──► docker-guard ── sock
            + 通过网络预检后管理镜像 / 维护模式
            + MYRIAD_DB_MODE=external → 不碰 pgdata 快照
```

业务网络 `myriad-net` 上 **不再** 有 `postgres` 成员；`myriad-admin-net` / `myriad-docker-guard-net` 与默认栈一致。三个进程必须指向**同一** Myriad 库/schema；worker 登录与限额见 [WORKER_DATABASE.md](./WORKER_DATABASE.md)。

---

## 环境变量契约

在部署目录 `.env`（或 compose `environment`）中设置：

| 变量 | 必需 | 说明 |
| --- | --- | --- |
| `MYRIAD_DB_MODE` | 是（外部模式） | 设为 `external`。updater 跳过 pgdata 快照/恢复。缺省或其它值按本地库模式处理。 |
| `DATABASE_URL` | 是 | web / 迁移登录的连接串，例如 `postgres://user:pass@host:5432/myriad?sslmode=require` |
| `PERSONA_DATABASE_URL` | 是 | persona-worker 的独立登录；同一库/schema |
| `FEDERATION_DATABASE_URL` | 是 | federation-worker 的独立登录；同一库/schema |
| `PERSONA_DB_PASSWORD` / `FEDERATION_DB_PASSWORD` | 可选 | 迁移登录有建角色权限时，web 可按这两项预置保留角色；否则由 DBA 预置并省略这两项 |
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

# db 是外部数据库在 myriad-backend-ext 上的容器名或网络别名
DATABASE_URL=postgres://myriad:WEB_PASSWORD@db:5432/myriad?sslmode=prefer
PERSONA_DATABASE_URL=postgres://myriad_persona:PERSONA_PASSWORD@db:5432/myriad?sslmode=prefer
FEDERATION_DATABASE_URL=postgres://myriad_federation:FEDERATION_PASSWORD@db:5432/myriad?sslmode=prefer

# 其余与默认部署相同（版本号请换成当前 release）
MYRIAD_TAG=v0.5.1
PROXY_TAG=v0.5.1
UPDATER_TAG=v0.5.1
# 普通重建由 UPDATER_TAG 选镜像；下列实际摘要记录由 Guard 维护，不覆盖 TAG：
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
| `backend` | `DATABASE_URL` 来自 env；**去掉** `depends_on: postgres`；仍依赖 `backend-volume-init`；加入 `myriad-backend-ext` |
| `federation-worker` / `persona-worker` | 保留 `myriad-net` 并加入 `myriad-backend-ext`；分别用 `FEDERATION_DATABASE_URL` / `PERSONA_DATABASE_URL` |
| `frontend` / `proxy` | 不变；proxy 仍指向两个 worker |
| `docker-guard` / `updater` / `updater-gateway` | 拓扑不变；updater 设 `MYRIAD_DB_MODE=external` |
| `./pgdata` | **不需要** bind；不要挂空目录装样子 |

示例要求预先存在外部网络 `myriad-backend-ext`，且外部 PostgreSQL 容器也接入该网络。主 backend 和两个 worker 都直接连接数据库，因此三者必须同时接入，不能只给 backend 配置。数据库容器名或网络别名用作三个数据库 URL 的 host。

在 Docker 宿主机准备网络：

```bash
docker network inspect myriad-backend-ext >/dev/null 2>&1 || docker network create myriad-backend-ext
# 替换为实际数据库容器名；已接入时跳过
docker network connect --alias db myriad-backend-ext YOUR_POSTGRES_CONTAINER
```

数据库所属的 Compose / 1Panel 编排也必须持久声明该外部网络，避免重建后丢失连接。`external: true` 表示 Myriad 不创建或删除它。

宿主机或云 RDS 不属于 Docker 网络成员。使用这些地址时，可同时移除三个服务和顶层声明里的 `myriad-backend-ext`，保留原有网络，并确保三个进程均能访问数据库。

### 在线更新与旧版本升级

updater 预检与 Guard 使用同一条规则：固定名称 `myriad-backend-ext` 只允许 backend、federation-worker、persona-worker 接入。前端、proxy、updater 等服务不能接入；worker 仍不能接入管理网或 Guard 网。网络必须由宿主机预先创建，其他任意网络名仍被拒绝。

**旧版 updater 和 Guard 只支持原有三网，必须先一起升级到包含此修复的构建。** 若已因额外网络被拦截，从宿主机按 [TCB 升级说明](./UPDATER_SECURITY_BASELINE.md) 更新这组服务；只升级业务 backend 镜像不生效。升级后可保留外部数据库网络执行站内更新/回滚，不要移除 worker 的数据库网络来绕过旧检查。

实现依据：[updater 网络预检](../../updater/src/docker/network_allowlist.rs) 与 [Guard 网络校验](../../updater/src/docker/guard/validate.rs)。`MYRIAD_DB_MODE=external` 只改变数据库备份/恢复行为，不授予任意网络访问。

### 启动

启动示例（把示例文件当主 compose，或与现有文件合并后）：

```bash
cd /path/to/deploy
cp /path/to/repo/docs/deployment/examples/docker-compose.external-db.example.yml docker-compose.yml
# 编辑 .env：MYRIAD_DB_MODE、三个数据库 URL、密钥与 tags

docker compose --env-file .env pull
docker compose --env-file .env up -d
# 或（若仍使用仓库脚本且 compose 文件名兼容）：
# bash scripts/extra/deploy.sh up
```

> **1Panel**：用「编排」导入上述 compose + `.env` 即可。应用商店里的 Postgres 与 Myriad 栈 **分开** 创建；Myriad 栈内不要再勾选/附带 postgres 容器。网络互通见下一节。
>
> 另外把 `.env` 里的 `MYRIAD_COMPOSE_HOST_ROOT` 设为编排目录的**绝对路径**（如 `/opt/1panel/docker/compose/myriad`）。否则 updater 容器内的部署根是 `/host/compose`，Compose 会把该容器内路径写进容器的 `com.docker.compose.project.config_files` 标签，1Panel 重建时会按这个标签在宿主机上找文件而报 `no such file or directory`。设为宿主绝对路径后，容器内外路径一致，标签即为宿主有效路径。

---

## 连通性：host / 网关 / SSL

backend、federation-worker 和 persona-worker 容器访问外部库时，hostname 必须是 **从容器内** 可解析、可路由的地址。

| 外部库位置 | `DATABASE_URL` host 写法 | 备注 |
| --- | --- | --- |
| 同一 Docker 网络上的其它容器 | 容器名或服务名 | 数据库与 backend、两个 worker 一起接入 `myriad-backend-ext` |
| 宿主机上的 Postgres（Linux） | 宿主机网桥网关，常见 `172.17.0.1` 或 compose 网络网关 | 也可用 `extra_hosts: ["host.docker.internal:host-gateway"]` 后写 `host.docker.internal` |
| 宿主机（Docker Desktop / 新版 Engine） | `host.docker.internal` | 建议显式 `extra_hosts: ["host.docker.internal:host-gateway"]` |
| 局域网 / 云 RDS | 内网 IP 或域名 | 安全组放行 **backend 和两个 worker 的出口** → DB 端口；优先私网 |

`extra_hosts` 示例（在 `backend`、`federation-worker`、`persona-worker` 三个服务下分别配置）：

```yaml
extra_hosts:
  - "host.docker.internal:host-gateway"
```

对应：

```bash
DATABASE_URL=postgres://myriad:WEB_PASSWORD@host.docker.internal:5432/myriad?sslmode=disable
PERSONA_DATABASE_URL=postgres://myriad_persona:PERSONA_PASSWORD@host.docker.internal:5432/myriad?sslmode=disable
FEDERATION_DATABASE_URL=postgres://myriad_federation:FEDERATION_PASSWORD@host.docker.internal:5432/myriad?sslmode=disable
```

### `sslmode`

| 场景 | 建议 |
| --- | --- |
| 公网 RDS / 强制 TLS | `sslmode=require` 或 `verify-full`（按云厂商文档带 CA） |
| 同机 / 内网明文 | `sslmode=disable` 或 `prefer`（按库配置） |
| 不确定 | 按数据库配置确定 TLS 要求，再分别验证三个进程；HTTP 工具不能验证 PostgreSQL TLS |

把密码 URL 编码（`@` → `%40` 等），避免连接串被截断。

---

## 运维 Runbook

### 1. 检查三个容器的网络和健康状态

```bash
docker compose ps -a backend federation-worker persona-worker
for service in backend federation-worker persona-worker; do
  container_id=$(docker compose ps -a -q "$service")
  docker inspect "$container_id" --format '{{.Name}} {{json .NetworkSettings.Networks}}'
  docker compose exec -T "$service" wget -qO- http://localhost:1103/health
done
```

独立 DB 容器部署下，三个容器都应接入 `myriad-net` 和 `myriad-backend-ext`，数据库也应出现在 `docker network inspect myriad-backend-ext` 的成员中。backend 保留管理网，worker 不加入管理网。

backend 检查 `database_connected`；worker 检查 `database` 和 `ready`。联邦因地域策略被禁用时允许正常退出，不应误判为数据库故障。

经 proxy 的 `/health` 只检查 web，不能代替两个 worker 的检查。三个部署 URL 分别映射为各容器自己的 `DATABASE_URL`；`MYRIAD_DB_MODE` 配在 updater 上，不应到 backend 的环境中寻找。不要输出含密码的完整环境。

若 backend 反复 unhealthy，优先查：DNS、防火墙、认证、库名、`sslmode`、是否仍指向已删除的 `postgres` 主机名。

### 2. 排查数据库连接失败

逐个核对三个 URL 的 host、端口、库名、独立登录及 TLS 配置。`db` 必须是外部数据库的容器名或网络别名，不能继续使用已删除的栈内 `postgres` 主机名。worker 还会校验角色权限和资源限制，见 [WORKER_DATABASE.md](./WORKER_DATABASE.md)。

backend 或宿主机 `psql` 能连通，不能证明 worker 的网络、DNS 和独立登录正确。查看失败服务的状态和日志，分别排查网络、认证及数据库角色约束。

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

通过网络预检的部署可由 updater 回滚镜像；旧版 updater/Guard 须先按上文升级。**数据** 一致性需你自己的 dump/PITR 策略。

### 4. 不要留下无用的 `./pgdata`

- 外部模式下 **不要** 创建空的 `./pgdata` 挂进任何服务。
- 若从栈内 Postgres **迁出** 后：确认外部库数据完整、backend 和两个 worker 均已切换 URL 并验证健康，再归档旧 `./pgdata`（先备份再删）。
- 避免 updater/运维误以为本地库仍在服务流量。

### 5. 从栈内 Postgres 迁到外部（概要）

1. 维护窗口：`docker compose stop frontend backend federation-worker persona-worker`（或整栈 stop，按你的流程）。  
2. `pg_dump` 栈内库 → 导入外部 Postgres（建库/用户/权限先就绪）。  
3. 换用外部 compose，设置 `MYRIAD_DB_MODE=external` 和三个数据库 URL，预置独立 worker 角色；独立 DB 容器与三个进程均接入 `myriad-backend-ext`。
4. 使用 `docker compose --env-file .env up -d`（若 `./guard-policy/docker-guard.env` 已存在可再加 `--env-file ./guard-policy/docker-guard.env`），执行上一节容器内校验。
5. 确认无误后处理旧 `./pgdata`（备份后删除或离线归档）。

Schema migration 仍由 **backend 启动路径** 负责（与默认部署相同）；确保外部库用户具备所需 DDL 权限。

### 6. Updater 行为（期望）

以下行为以通过网络预检为前提；旧版本迁移见「在线更新与旧版本升级」。

| 能力 | 本地 `postgres` + pgdata | `MYRIAD_DB_MODE=external` |
| --- | --- | --- |
| 维护模式 / 停业务容器 | 是 | 是 |
| 切换 `MYRIAD_TAG` 等镜像 tag | 是 | 是 |
| `pgdata` 快照 / 回滚恢复 | 是 | **否（跳过）** |
| 库备份 | 快照近似「整库文件」 | **运维 `pg_dump`/云备份** |

外部库模式下请 **手动** 备份（`pg_dump` / 云快照），不要点 UI 里的「pgdata 回滚」。

---

## 安全注意

- 三个数据库 URL 均含密码：权限收紧 `.env`（如 `chmod 600`），勿提交 git，勿贴进公开 issue。
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
