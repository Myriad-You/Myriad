# Myriad Updater Spec

> 自托管 Myriad 实例的"维护模式 + 单运行槽 + 版本标签 + 数据快照"更新系统设计规范。
> 本文档是 updater / proxy / release 流水线之间的契约，所有实现必须以此为准。

**重要边界**：Myriad updater 不是双活 A/B 分区系统。运行时始终只有一套
`backend` / `frontend` / `postgres` 业务容器；updater 只在维护模式下停止业务容器、
创建 `pgdata` 快照、切换 `.env` 中的镜像 tag，然后启动新版本。回滚依赖快照和旧
tag，而不是并行保留 A/B 两套在线分区。

## 0. 设计原则

按优先级排序，冲突时按顺序优先：

1. **绝不破坏用户数据**：宁可卡在维护模式让用户求救，也不能丢/损坏 pgdata。
2. **绝不进入不可恢复态**：每一步都要可逆，或在不可逆前 force-stop。
3. **可断电恢复**：任何时刻断电，重启后 updater 能从持久化状态续上。
4. **配置容忍**：不假设 .env / compose 文件结构稳定，按 schema 解析+合并。
5. **零外部依赖**：除 GitHub 之外，所有外部依赖都是可选的。
6. **观测优先**：出问题时用户/外部工程师能立刻看到"卡在哪一步、上一步做了啥、下一步要做啥"。

`updater` 被设计为"近一次性发布的基础设施"——可能数年不更新。每一处异常都必须显式处理，不依赖"下个版本修一下"。

## 1. 总体架构

```
┌──────────────────────────────────────────────────────┐
│ GitHub                                                │
│  ├─ Actions: build & push images, publish release    │
│  ├─ Registry: 不可变 versioned tag                   │
│  └─ Release: release.json + CHANGELOG + SHA256SUMS   │
└────────────────────┬─────────────────────────────────┘
                     │ HTTPS (Release API)
                     ▼
┌──────────────────────────────────────────────────────┐
│ 用户宿主机 (docker compose)                           │
│                                                        │
│  [myriad-net]                                          │
│   proxy ─┬─► frontend                                  │
│          └─► backend ─► postgres                       │
│                │                                       │
│  [myriad-admin-net]                                    │
│                └──► updater-gateway ──► updater        │
│                                        │               │
│  [myriad-docker-guard-net internal]                    │
│                                        └──► docker-guard
│                                              └── sock  │
└──────────────────────────────────────────────────────┘
```

- `proxy`：唯一对外暴露端口的组件，负责维护页与反向代理。极少更新；兼入 admin-net 以便
  rescue 解析 `updater`。
- `updater-gateway`：同 updater 镜像的薄反代；校验 `X-Updater-Gateway-Secret` 后注入
  `X-Update-Token`；**仅** admin-net；不持 compose 挂载、不挂 docker.sock。`/healthz` 不需 secret。
- `updater`：接收更新指令，执行更新/回滚流程；不挂载 docker.sock；**不在**业务 `myriad-net`。
- `docker-guard`：唯一挂载 docker.sock 的内部服务，按 API、Compose 项目标签、镜像仓库和
  `containers/create` 请求体执行白名单策略。
- `backend` / `frontend`：业务组件，由 updater 拉取并启停；backend 双宿 business+admin，
  **生产 compose 不注入 `UPDATE_TOKEN`**，注入 `UPDATER_GATEWAY_SECRET` 以调用 gateway。
- `postgres`：数据存储。更新前后由 updater 做文件级快照。

## 2. 仓库产物

每次发版的产物：

| 产物 | 位置 | tag 策略 |
|---|---|---|
| backend 镜像 | `<registry>/myriad-backend` | `v1.2.3`、`v1.2`、`stable`（**不用 latest**） |
| frontend 镜像 | `<registry>/myriad-frontend` | 同上 |
| proxy 镜像 | `<registry>/myriad-proxy` | 独立节奏，多数版本不动 |
| updater 镜像 | `<registry>/myriad-updater` | 独立节奏 |
| `release.json` | GitHub Release asset | 一个 release 一份 |
| `SHA256SUMS` | GitHub Release asset | release 资产校验 |

**强制不可变 tag**：回滚依赖旧 tag 仍在 registry。CI 必须确保已发布版本的 tag 永不被覆盖。

## 3. release.json schema

updater 唯一权威数据源。完整 JSON Schema 见 [release/release-schema.json](../release/release-schema.json)。

```json
{
  "schema_version": 1,
  "version": "v1.2.3",
  "commit_sha": "0123456789abcdef0123456789abcdef01234567",
  "channel": "stable",
  "released_at": "2026-05-16T10:00:00Z",
  "min_from_version": "v1.2.0",
  "images": {
    "backend":  { "ref": "<registry>/myriad-backend:v1.2.3",  "digest": "sha256:abc..." },
    "frontend": { "ref": "<registry>/myriad-frontend:v1.2.3", "digest": "sha256:def..." },
    "proxy":    { "ref": "<registry>/myriad-proxy:v1.0.0",    "digest": "sha256:..." }
  },
  "env": {
    "required":   ["DATABASE_URL", "JWT_SECRET", "POSTGRES_PASSWORD"],
    "new":        [{ "name": "MYRIAD_FEATURE_X", "required": false, "default": "true" }],
    "removed":    ["MYRIAD_LEGACY_FLAG"]
  },
  "migrations": {
    "irreversible": false,
    "estimated_seconds": 30,
    "requires_full_backup": true
  },
  "updater": {
    "min_updater_version": "v0.3.0",
    "self_update_required": false
  },
  "postgres": {
    "min_pg_version": "15",
    "max_pg_version": "unbounded"
  },
  "notes_url": "https://github.com/.../releases/tag/v1.2.3",
  "signature": null
}
```

### 字段规则

- `schema_version`：增量整数。updater 必须能解析自己 ≤ 的版本，更高版本拒绝。
- `min_from_version`：禁止跨太多版本直接升，强制走中间版本。
- `min_updater_version`：高于当前 updater → 拒绝业务更新，提示先升级 updater。
- `postgres.min_pg_version`：唯一有效的 PostgreSQL 兼容边界；不设置上限。
- `postgres.max_pg_version`：仅为兼容旧 updater 保留，可省略；`unbounded` 明确表示无上限。
- `signature`：M2 启用 cosign 签名，M1 留 null。
- 未知字段：updater 必须忽略不报错（向前兼容）。

## 4. GitHub Actions 流水线

### 4.1 release.yml（push tag `v*` 触发）

```
jobs:
  build-images:
    strategy.matrix.component: [backend, frontend, proxy, updater]
    steps:
      - build & push <registry>/myriad-<comp>:<tag>
      - 输出 digest 到 artifact
  publish-release:
    needs: build-images
    steps:
      - 拼 release.json (读 artifact 的 digest)
      - 生成 SHA256SUMS
      - gh release create
```

### 4.2 docker-publish.yml（push 条件打包 + workflow_dispatch）

- push 到 `main` / `preview` / `beta`：仅当**提交标题（第一行）包含子串 `-p`** 时才打包；否则跳过 build
- 也可在 Actions 中 `workflow_dispatch` 手动触发（可选 `tag`、`components` 参数）
- 仅推开发镜像，tag 使用 `dev-<sha>`、分支名，或输入的 `tag` 覆盖
- 默认组件：`backend` / `frontend` / `proxy`（**不含 updater**；updater 仅 release.yml 打 tag 时打包，或手动传入 `components` 包含 updater）
- **禁止再推 latest 到生产仓库**

### 4.3 PR check

只 build 不 push（或推到 `pr-<num>` 临时 tag）

## 5. 用户侧 docker-compose 结构

```yaml
services:
  proxy:
    image: <registry>/myriad-proxy:${PROXY_TAG}
    ports: ["80:80", "443:443"]
    volumes:
      - ./state:/state:ro

  frontend:
    image: <registry>/myriad-frontend:${MYRIAD_TAG}

  backend:
    image: <registry>/myriad-backend:${MYRIAD_TAG}
    env_file: .env
    depends_on: [postgres]

  postgres:
    image: postgres:18
    volumes:
      - ./pgdata:/var/lib/postgresql        # PostgreSQL 18+ bind mount, 必须

  docker-guard:
    image: <registry>/myriad-updater:${UPDATER_TAG}
    entrypoint: ["/usr/local/bin/myriad-docker-guard"]
    environment:
      UPDATE_TOKEN: ${UPDATE_TOKEN}
      COMPOSE_PROJECT_NAME: ${COMPOSE_PROJECT_NAME:-myriad}
      MYRIAD_DOCKER_NETWORK: ${MYRIAD_DOCKER_NETWORK:-myriad-net}
      MYRIAD_ADMIN_NETWORK: ${MYRIAD_ADMIN_NETWORK:-myriad-admin-net}
      MYRIAD_DOCKER_GUARD_NETWORK: ${MYRIAD_DOCKER_GUARD_NETWORK:-myriad-docker-guard-net}
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
      - ./:/host/compose:ro
    networks: [myriad-docker-guard-net]

  updater:
    image: <registry>/myriad-updater:${UPDATER_TAG}
    environment:
      UPDATE_TOKEN: ${UPDATE_TOKEN}
      CHANNEL: ${CHANNEL:-stable}
      GITHUB_TOKEN: ${GITHUB_TOKEN:-}
      REGISTRY_MIRROR: ${REGISTRY_MIRROR:-}
      UPDATER_STATE_DIR: /host/compose/state
      UPDATER_ENV_FILE: /host/compose/.env
      UPDATER_PGDATA: /host/compose/pgdata
      DOCKER_HOST: tcp://docker-guard:2375
      COMPOSE_PROJECT_NAME: ${COMPOSE_PROJECT_NAME:-myriad}
      MYRIAD_DOCKER_NETWORK: ${MYRIAD_DOCKER_NETWORK:-myriad-net}
    volumes:
      - ./:/host/compose
    networks: [myriad-admin-net, myriad-docker-guard-net]

  updater-gateway:
    image: <registry>/myriad-updater:${UPDATER_TAG}
    entrypoint: ["/usr/local/bin/myriad-updater-gateway"]
    environment:
      UPDATE_TOKEN: ${UPDATE_TOKEN}
      UPDATER_UPSTREAM: http://updater:1101
    networks: [myriad-admin-net]

networks:
  myriad-net:
    name: ${MYRIAD_DOCKER_NETWORK:-myriad-net}
  myriad-admin-net:
    name: ${MYRIAD_ADMIN_NETWORK:-myriad-admin-net}
  myriad-docker-guard-net:
    internal: true
```

当前仓库的 `docker-compose.yml` 使用单个宿主 `.env` 作为部署契约。它同时保存业务
镜像 tag、数据库/JWT 配置，以及 updater 所需的 token/channel。普通业务更新只改写
`MYRIAD_TAG`；updater 自更新只改写 `UPDATER_TAG`。`PROXY_TAG` 目前走手动 tag
升级路径。

`.env` 必须包含：

```
MYRIAD_TAG=v1.2.3
PROXY_TAG=v1.0.0
UPDATER_TAG=v0.3.0
COMPOSE_PROJECT_NAME=myriad
UPDATE_TOKEN=<32+ char random>
CHANNEL=stable
GITHUB_TOKEN=    # 可选，提升 rate limit
REGISTRY_MIRROR= # 可选
# MYRIAD_DOCKER_NETWORK=myriad-net # 可选；默认固定 Docker 网络名
```

## 6. 状态持久化

所有状态写到单一部署根 bind 下的 `/host/compose/state`（宿主 `./state`），**绝不**写容器层。

### 6.1 文件清单

| 文件 | 内容 | 写入方式 |
|---|---|---|
| `state/updater.json` | 当前版本、上次检查、updater 自身版本、channel | atomic write |
| `state/maintenance.json` | 维护模式 phase、心跳 ts、from/to 版本 | 每 phase 切换 atomic write |
| `state/job.<id>.json` | 任务完整 step log | append-only |
| `state/job.current` | 当前 in-flight job id | atomic |
| `state/lock` | `flock(LOCK_EX|LOCK_NB)` | 进程锁 |
| `state/snapshots/` | pgdata 快照目录 | mv (rename) |
| `state/snapshots.json` | 快照元数据 | atomic |
| `state/history.log` | 人类可读历史日志 | append-only |
| `state/manual-override` | 存在即停止所有自动行为 | 用户 touch 创建 |
| `state/cache/` | release.json ETag/缓存 | atomic |

### 6.2 Atomic write

```rust
fn write_atomic(path, data):
  let tmp = format!("{path}.tmp.{pid}.{nanos}");
  open(tmp, O_WRONLY|O_CREAT|O_EXCL);
  write + fsync;
  rename(tmp, path);
  fsync(parent_dir);
```

## 7. 状态机

```
idle
  └─► checking (周期/手动)
       └─► ready
            └─► preflight ────────► (失败) idle
                 └─► maintenance_on
                      └─► stopping
                           └─► snapshotting
                                └─► swap_tag
                                     └─► starting_new
                                          └─► health_probing
                                               └─► swapping_proxy
                                                    └─► finalize
                                                         └─► idle

任意步骤失败 (在 swap_tag 之前)：
  └─► cleanup → idle

任意步骤失败 (swap_tag 之后)：
  └─► rollback_in_progress
       └─► stop_new → restore_snapshot → swap_tag_back → start_old → health_probing
            └─► (成功) idle
            └─► (失败) needs_manual

`swap_tag_back` 的目标 tag（上一正常业务版本）按以下优先级解析，**从不写死版本号**：

1. 更新流程在 `swap_tag` 前从 `.env` 读到的 `MYRIAD_TAG`（同 job 自动回滚）
2. 快照元数据 `source_version`（创建快照时记录；若 state 无版本则回填 `.env` 的 `MYRIAD_TAG`）
3. `updater.json.current_version`（成功升级后推进；daemon 启动时也会用 backend build stamp / `.env` 自动恢复）

手动 `POST /rollback` / rescue 与自动回滚共用同一解析逻辑；回滚健康通过后会把 `current_version` 写回恢复到的版本。`current_commit_sha` 在无法立即确认时清空，并在下次 daemon 启动时从 build stamp 或 GitHub tag 重新解析，避免保留错误 SHA。
```

### 7.1 崩溃恢复表

启动时读 `state/job.current`：

| 上次状态 | 重启后动作 |
|---|---|
| `idle` | 正常启动 |
| `preflight` | 撤销 lock，回 idle（无副作用） |
| `maintenance_on` | 退维护，回 idle |
| `stopping` / `snapshotting` | 检查 snapshot 完整性 → 完整则进 swap，否则 cleanup → idle |
| `swap_tag` 之后任意步骤 | **不自动恢复**，进 `needs_manual` |
| `rollback_in_progress` | 检查 snapshot 在 → 继续 restore；不在 → `needs_manual` |
| `needs_manual` | 维持，proxy 维护页显示 rescue 指令 |

**规则**：`swap_tag` 之后的任何步骤崩溃，必须人工 `POST /rescue/continue` 或 `/rescue/rollback`。

## 8. 网络与下载

### 8.1 GitHub API

- 匿名默认（60 req/h），支持 `GITHUB_TOKEN`
- connect timeout 10s, total 60s
- 重试 3 次，指数退避 (1s, 4s, 16s)，**仅幂等 GET**
- ETag + Last-Modified 缓存到 `state/cache/`

### 8.2 镜像拉取

- 通过 docker 调用 `pull` (bollard)
- pull 前 `manifest inspect` 校验 digest 与 release.json 一致
- 支持 `REGISTRY_MIRROR` env：retag 后 pull，digest 校验保持
- 失败分类：401/403 token 错误；404 版本失效；5xx/网络 重试

### 8.3 代理与时钟

- 透传 `HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY`
- 系统时间与 GitHub `Date` 差 > 5min → warning，但不阻止；TLS 失败时把"时钟漂移"列入诊断 hint

## 9. 磁盘与快照

### 9.1 pgdata 探测（启动时）

```
bind mount, 同设备     → 支持 (cp -a --reflink=auto)
bind mount, 跨设备     → 支持，但 warning；回滚走 copy 兜底
docker named volume    → 拒绝 (M1)
btrfs subvolume        → 支持，可选用 btrfs snapshot
zfs dataset            → 支持，可选用 zfs snapshot
其他                   → cp -a 兜底
```

**M1 要求 pgdata 是 bind mount。** 跨设备会降级为 copy 兜底并写入 warning；
docker named volume、rootless Docker、Podman 仍会启动时报错。

### 9.2 快照流程

```
1. df 检查空闲 ≥ pgdata 大小 × 1.5
2. docker compose stop postgres
3. sync -f pgdata
4. cp -a --reflink=auto pgdata snapshots/<job-id>.tmp
5. fsync snapshots/<job-id>.tmp
6. mv snapshots/<job-id>.tmp snapshots/<job-id>
7. fsync snapshots/
8. snapshots.json: 记录 size + file_count + 抽样 checksum
9. docker compose start postgres
```

### 9.3 保留策略

- 最近 3 份自动保留
- `pre-major-upgrade` 标记的永久保留
- < 24h 内的快照永不自动删
- 删除前确认不是 in-flight job 引用

## 10. Docker 适配

### 10.1 命令探测

启动时探测可用的 compose 命令：

```
docker compose (v2)   ← 首选
docker-compose (v1)   ← fallback
都没有                 → 启动失败
```

结果写 `state/env-probe.json`。

### 10.2 不假设 compose 文件结构

- 不直接修改 `compose.yaml`
- 普通更新只修改 `.env` 中的 `MYRIAD_TAG`
- self-update 只修改 `.env` 中的 `UPDATER_TAG`
- `PROXY_TAG` 目前由人工编辑 `.env` 后运行 `scripts/docker/deploy.sh upgrade`
- compose 文件必须用 `${MYRIAD_TAG}` 引用版本变量
- 启动时验证 compose 引用了这些变量，没有则拒绝启动

### 10.3 .env 改写

```
1. 解析 .env 为有序 dict（保留注释和空行位置）
2. 改目标 key
3. 写到 .env.new + fsync + rename
4. 保留 .env.bak.<ts>，最多 5 份
```

`.env` 中重复 key → 启动时拒绝。

### 10.4 容器名/项目名

- 默认 `COMPOSE_PROJECT_NAME=myriad`
- 所有 docker compose 命令带 `-p <COMPOSE_PROJECT_NAME>`
- Docker 网络显式命名为 `MYRIAD_DOCKER_NETWORK`，默认 `myriad-net`

### 10.5 rootless/podman

M1 显式拒绝，启动时探测 `docker info` 中 `rootless: true` 或 podman shim → 拒绝。

## 11. 健康检查

### 11.1 backend /health 响应 schema

```json
{
  "status": "ok",
  "version": "v1.2.3",
  "commit_sha": "0123456789abcdef0123456789abcdef01234567",
  "schema_version": 1,
  "db_connected": true,
  "migrations_applied": true,
  "uptime_seconds": 123
}
```

backend 必须实现此 schema，updater 严格校验。

### 11.2 frontend 健康

- HTTP 200
- index.html 包含 `<meta name="myriad-version" content="v1.2.3">`
- index.html 包含 `<meta name="myriad-commit" content="<40-char sha>">`
- updater 抓取并对比 target version

### 11.3 deadline

- 初始等待 10s
- 每 2s 探一次，连续 3 次通过算 healthy
- 总超时 `max(300s, migrations.estimated_seconds × 3)`

## 12. proxy 维护页

### 12.1 可靠性原则

- proxy 启动不依赖任何外部服务
- 维护页是 proxy 内嵌静态资源
- 状态只读 `state/maintenance.json`
- 文件不存在/损坏 → 当作 `active=false`（fail-open 到正常代理）

### 12.2 stale 检测

```
now - maintenance.updated_at > 10min → 维护页加红色横幅 "更新疑似失败"
                              > 30min → 显示完整 rescue 命令
```

### 12.3 proxy 升级

- proxy 自身升级有短暂 downtime（< 10s）
- 不在自动更新流程中，需用户手动触发

## 13. HTTP API

### 13.1 双通道

UI 与 updater 之间有两条通道：

| 通道 | 路径前缀 | 鉴权 | 默认状态 | 用途 |
|---|---|---|---|---|
| **backend** | `/api/admin/updater/*` | admin session；backend → `updater-gateway`（网关注入 token） | 总是开启 | 默认通道，浏览器不接触 token；backend **不**持 `UPDATE_TOKEN` |
| **direct** | `/_updater/*` (proxy → updater) | `X-Update-Token` header | 默认**关闭**，需 `PROXY_ALLOW_DIRECT_UPDATER=true` | 运维 fallback（backend 挂掉时用） |

backend 通道实现：`UpdaterClient` 仅需 `MYRIAD_UPDATER_URL`（默认 `http://updater-gateway:1104`）；
token 由 gateway 注入。可选 `UPDATE_TOKEN` 仅用于无 gateway 的遗留/开发直连。

proxy 通道开关：proxy 启动时读 `PROXY_ALLOW_DIRECT_UPDATER`，未开启时 `/_updater/*` 直接 404。

### 13.2 接口列表（两个通道共享）

| Method | Path（updater 上） | 权限（updater 侧） | 说明 |
|---|---|---|---|
| GET | `/status` | token | 当前版本/状态 |
| GET | `/available?channel=stable` | token | 可升级版本 |
| GET | `/commits?branch=preview&limit=25` | token | GitHub 提交目标列表 |
| GET | `/builds?limit=25` | token | Docker Hub 中前后端共有的不可变 `dev-<sha>` 构建 |
| GET | `/releases?channel=stable&limit=25` | token | GitHub Release 目标列表 |
| GET | `/compare?to=<ref>` | token | 当前版本与目标提交的祖先关系 |
| GET | `/jobs` | token | 历史任务 |
| GET | `/jobs/{id}` | token | 任务详细 log |
| POST | `/update` | token | `{target_version, allow_skip_versions: false}` |
| POST | `/rollback` | token | `{snapshot_id}` |
| POST | `/admin/self-update` | token | updater 自更新 |
| GET | `/snapshots` | token | 可恢复快照 |
| POST | `/rescue/exit-maintenance` | token + manual | 强制清维护 |
| POST | `/rescue/continue` | token | 一键回退：对 stuck job 的 snapshot 执行与 `/rollback` 相同的恢复（pgdata + MYRIAD_TAG） |
| POST | `/rescue/forget-current` | token + manual | 放弃当前 job |
| GET | `/diagnostics` | token | 环境探测报告 |
| GET | `/healthz` | 公开 | updater 自己活着 |

**manual 权限**：宿主机 `touch state/manual-override` 后才生效。防止 API 打穿后被远程救援。

除 `/healthz` 外，updater 的全部接口都要求 `X-Update-Token`。这既限制更新操作，
也避免同一 Docker 网络中的其他容器读取部署元数据或反复触发外部版本查询。

通过 backend 通道时，**所有**这些接口还需要 admin session（JWT `is_admin=true`），相当于在 updater 侧 token 校验之上再加一道。

### 13.3 Idempotency

`POST /update` 带 `Idempotency-Key`，最近 100 个 key 内重复请求返回原 job id。

### 13.4 schema_version

所有响应包含 `schema_version`，前端按版本兼容。

### 13.5 Docker Hub 提交构建回退

提交模式默认使用 GitHub 提交列表和 compare API。GitHub 请求失败或没有返回提交时，UI
改用 `/builds`，查询 `.env` 中 `BACKEND_IMAGE` 与 `FRONTEND_IMAGE` 对应的 Docker Hub
仓库，只返回两边同时存在的不可变 `dev-<sha>` 标签；`preview` 等可变标签不会成为候选。

自动检查同样在 GitHub 失败时选择最新的共有构建。因为 Docker Hub 标签无法证明 Git
祖先关系，安装前必须显式允许 unknown risk；预检随后实际拉取前端和后端两个镜像，任一
不存在都会在进入维护模式前失败。release 模式不使用此回退，仍要求 release manifest、
迁移元数据和镜像摘要校验。

## 14. 自更新

### 14.1 两阶段独立

```
1. updater 检测 self_update_required=true 或自身版本 < min_updater_version
2. 标记 state/pending-self-update.json
3. 没有进行中任务时：
   a. 经 docker-guard pull <new-updater> 并校验 digest
   b. 改 .env 的 UPDATER_TAG
   c. 调用 docker-guard 的鉴权 self-update 端点（X-Update-Token）
   d. docker-guard 延迟后执行 **固定 argv** 的 TCB 自替换：
        docker compose up -d --no-deps docker-guard updater updater-gateway
      该次 compose 走 **unix:///var/run/docker.sock**（直连宿主 daemon），
      不经过 guard 的 policy proxy。原因：create 白名单只有
      backend|frontend|postgres|updater，无法经 API 创建 `docker-guard` /
      `updater-gateway`；自替换是固定 compose 服务列表，不是任意 Docker API。
      为避免「在 guard 容器内 recreate 自己」中途被杀，compose 由短生命周期
      helper 容器（同一 updater 镜像、entrypoint=`myriad-tcb-self-update`）执行。
      请求体携带 `previous_tag` / `target_tag`（安全字符集校验）。
```

**必须同时重建 `docker-guard`、`updater` 与 `updater-gateway`**：三者共用
`UPDATER_TAG` 镜像；只重建 updater 会使 guard/gateway 二进制落后于 tag。

其余 updater 日常流量仍经 `DOCKER_HOST=tcp://docker-guard:2375` 受请求体策略约束。

调度仍是异步（HTTP 202）：updater 改写 `.env` 后请求 docker-guard，guard 延迟后启动
短生命周期 helper（`myriad-tcb-self-update`，compose 目录 **rw** 挂载、`network=none`）。
- **调度 HTTP 失败**：updater 进程立即把 `UPDATER_TAG` 恢复为旧值。
- **helper 的 `compose up docker-guard updater updater-gateway` 失败**：helper 将
  `UPDATER_TAG` 恢复为请求体中的 `previous_tag`，并写入部署卷上的耐久状态
  `state/self-update-last.json`（`status: succeeded|failed`、tags、`at`、可选 `error`）。
- **成功路径**：重建三者并写 `status: succeeded`。
- **可见性**：`GET /status`（token）返回可选字段 `self_update_last`；亦可
  `GET /self-update/last`。backend admin 代理原样转发 `/status`。

### 14.2 兜底

用户可在部署目录手动执行（宿主机管理员路径）：

```
# 先修改 .env 中的 UPDATER_TAG，再由宿主 Docker CLI 重建 TCB 服务
docker compose up -d docker-guard updater updater-gateway
```

### 14.3 前向兼容

- 未知 release.json 字段忽略
- `min_updater_version` 高于自己 → 优雅拒绝，提示先升 updater
- 即使数年不升 updater，遇到不兼容版本会停在"无法升级业务"，不误升级

## 15. 安全

### 15.1 信任边界

- updater 不挂载 docker.sock，只能访问内部 docker-guard 网络。
- docker-guard 仍持有原始 socket；它若自身被攻破仍等同宿主 root，因此不对外发布端口，
  并设置只读根文件系统和 `no-new-privileges`。
- guard 拒绝 Docker exec、任意/未知变更 API、非本项目容器操作、非白名单镜像，以及带
  `Privileged`、host namespace、device、额外 capability 或越界宿主 bind mount 的 create。
- **仅 Compose 服务 `updater` 可附着 docker-guard 网络**（`MYRIAD_DOCKER_GUARD_NETWORK` /
  默认 `myriad-docker-guard-net`）：`networks/{id}/connect|disconnect`、create 时的
  `NetworkingConfig.EndpointsConfig` 与 `HostConfig.NetworkMode` 均强制该规则。backend /
  frontend / postgres 不得 dual-home 到 guard 网，避免在 updater 被攻破后把业务容器拉进
  未鉴权的 Docker API（`:2375`）。
- **允许的网络名**（create/connect）：业务 `myriad-net`、管理平面 `myriad-admin-net`
  （`MYRIAD_ADMIN_NETWORK`）、guard-net。其它网络名拒绝。
- **updater 不在业务 `myriad-net`**：frontend/postgres 与 updater HTTP 无共享 L2；
  backend 经 admin-net 访问 `updater-gateway`。
- **推荐部署中 backend 进程不持有 `UPDATE_TOKEN`**：token 仅在 updater、updater-gateway、
  docker-guard（自更新鉴权）环境中；gateway 在 admin-net 上注入 header。攻击面从
  胖 backend 进程剥离。
- **backend→gateway 另需 `UPDATER_GATEWAY_SECRET`**（≥32）：admin-net 上的 peer 不能在
  无 secret 时调用 gateway 代理路由。泄露 gateway secret ≈ 可驱动更新（仍优于
  `UPDATE_TOKEN` 进入 backend 进程）。客户端若自带 `X-Update-Token`，gateway 拒绝并覆盖注入。
- updater 重建只允许单一部署根 bind；postgres pgdata bind 会逐级拒绝符号链接、异常文件
  类型和共享/从属 mount propagation。
- 健康探测只走 Compose 内网 HTTP，不使用 Docker exec，也不临时创建探测容器。
- 所有外部输入严格校验
- updater 侧仍强制 `UPDATE_TOKEN`；backend→gateway 强制 `UPDATER_GATEWAY_SECRET`（不再仅靠
  admin-net 成员关系）

### 15.2 输入校验

- 所有 docker / shell 子进程调用使用参数数组，不拼 shell 字符串
- version / tag 走白名单：`^v\d+\.\d+\.\d+(-[a-z0-9.]+)?$`
- compose 路径限定预设路径
- 高风险更新：当 body 含 `allow_risk` / `allow_downgrade` / `allow_diverged|unknown|irreversible`
  为 true 时，另需 `confirm_risk: true` 或 header `X-Myriad-Confirm-Risk: true`。普通升级无额外字段。

### 15.3 token / gateway secret

- `UPDATE_TOKEN` 必须 ≥ 32 字符
- `UPDATER_GATEWAY_SECRET` 必须 ≥ 32 字符（backend + updater-gateway）
- 启动时检查不在弱密码列表（UPDATE_TOKEN）
- 5 次/min 401 后封 10min（updater token）
- 生产 compose：`UPDATE_TOKEN` 注入 updater / updater-gateway / docker-guard；**不**注入 backend
- 生产 compose：`UPDATER_GATEWAY_SECRET` 注入 backend + updater-gateway；**不**注入 frontend

### 15.4 release.json 校验

M1：HTTPS + digest 校验 + version 正则
M2：cosign 签名（已实现）

**cosign keyless 流程**：

1. release.yml 在 GitHub Actions runner 上用 OIDC token 调 `cosign sign-blob` 签 release.json
   和 SHA256SUMS，产出 `.sig` + `.pem` 一并发到 Release assets。
2. updater 拉 release.json 时同时拉 `release.json.sig` 和 `release.json.pem`，调本地 cosign
   CLI 校验：
   - `--certificate-identity-regexp ^https://github\.com/<repo>/\.github/workflows/.+@refs/tags/v[0-9].+$`
   - `--certificate-oidc-issuer https://token.actions.githubusercontent.com`
3. 用户通过 `COSIGN_VERIFY` 环境变量切换策略：`strict`（默认，验签失败即拒绝）/ `soft`
   （失败仅 warning）/ `off`（关闭验证）。`off` 必须再设 `UPDATER_ALLOW_INSECURE_COSIGN=true`
   （或别名 `COSIGN_INSECURE_OK=true`），否则 updater 拒绝启动。
4. updater 镜像里预装 cosign CLI（`sigstore/cosign` v2.4.1 单文件二进制）。

## 16. 观测与诊断

### 16.1 日志

- `state/history.log`：人类可读，append-only（含 phase / job 进度）
- `state/audit.log`：安全与运维审计，append-only + fsync；行格式与 history 中
  `audit: …` 前缀一致，便于 `grep '^\[.*\] audit:'`
- `state/job.<id>.json`：结构化，每步 stdout/stderr 末尾 64KB

#### 16.1.1 audit.log 事件

由 `StateDir::append_audit` 写入（实现见 `updater/src/state/audit.rs`）。超过约 8 MiB
时轮转到 `audit.log.1`（单文件、best-effort）。

| 事件前缀 | 触发点 |
|---|---|
| `audit: update_request job=…` | 业务更新开始（含 risk flags） |
| `audit: update_succeeded job=…` | 业务更新成功 finalize 前 |
| `audit: auto_rollback_ok job=…` | 更新失败后自动回滚成功 |
| `audit: rollback_start job=… snapshot=…` | 独立回滚任务开始 |
| `audit: self_update_scheduled …` | 自更新已调度（docker-guard 执行双服务重建） |
| `audit: job_terminal job=… status=…` | 任意 job finalize（Succeeded/Failed/NeedsManual 等） |
| `audit: rescue_*` | rescue CLI / API（exit maintenance、continue、forget、rollback） |

`history.log` 仍会保留同名或相近行，便于现有 UI/诊断；`audit.log` 是更干净的保留副本。

### 16.2 诊断包

`GET /diagnostics` 返回 JSON 诊断摘要；`myriad-rescue diagnose` 输出 tar.gz：

- updater 版本、配置
- env-probe 结果
- pgdata 大小、可用空间
- 最近 10 个 job 摘要
- 当前 maintenance.json
- 最近 100 行 history.log
- `state/audit.log`（若存在）
- `docker images | grep myriad`
- `docker compose ps` 输出

### 16.3 rescue CLI

镜像附带 `myriad-rescue` 二进制：

```
myriad-rescue status
myriad-rescue exit-maintenance --force
myriad-rescue rollback --snapshot=<id>
myriad-rescue diagnose --output diag.tar.gz
myriad-rescue forget-job
myriad-rescue clean-snapshots --keep=3
```

CLI 直接读 state + 调 docker，**不依赖 updater 进程活着**。

## 17. 测试矩阵

M1 release 前必须跑通。状态：
- **e2e** — 通过 `scripts/test-updater-e2e.sh` 自动验证（本地二进制 + /tmp testbed）
- **unit** — 通过 `cargo test --lib` 验证
- **manual** — 需要真实 docker stack（本地或预发环境），尚未自动化

| # | 场景 | 期望 | 当前状态 |
|---|---|---|---|
| 1 | 正常更新 | 成功 | manual |
| 2 | pull 中断网 | 维护未启动，回 idle | manual |
| 3 | 新 backend 启动失败 | 自动回滚成功 | manual |
| 4 | 新 backend health 持续 false | 5min 超时回滚 | manual |
| 5 | migration 失败 | 回滚 pgdata，旧 backend 起来 | manual |
| 6 | 回滚中 snapshot 丢失 | needs_manual | manual |
| 7 | 更新中拔电源 | 重启续状态机 | manual |
| 8 | 更新中 docker daemon 重启 | 重试或失败回滚 | manual |
| 9 | 磁盘满 | preflight 拒绝 | manual |
| 10 | .env 缺新 required env | preflight 拒绝 | manual |
| 11 | min_from_version 不满足 | preflight 拒绝 | manual |
| 12 | min_updater_version 高于自己 | 拒绝并提示 | manual |
| 13 | 并发 POST /update | 第二个 409 | manual |
| 14 | pgdata 是 named volume | 启动时拒绝 | unit + smoke |
| 15 | compose 不引用 MYRIAD_TAG | 启动时拒绝 | **e2e ✓** + smoke |
| 16 | token 错误 | 401，5 次后限流 | **e2e ✓** (401 验证); 限流 unit |
| 17 | GitHub rate limit（commit 模式） | 回退 Docker Hub 共有构建并标 unknown | unit + manual |
| 18 | release.json 未知字段 | 忽略继续 | unit (serde flatten + skip_unknown) |
| 19 | frontend cache 旧版本 | health meta 失败 → 回滚 | manual |
| 20 | proxy 重启 | 维护状态从磁盘恢复 | **e2e ✓** (fail-open + maintenance.json 切换) |

E2E 实际覆盖（11 项 / 全过，2026-07-17）：

- proxy `/healthz`、`/_proxy/status`、`/_updater/*` 转发、维护页 503 切换、删 maintenance.json 后 fail-open
- updater `/healthz` 公开存活探测、其余接口鉴权、`/status` schema 合规、版本格式校验
- docker-guard 允许本项目真实 Compose create/start/remove 生命周期，并拒绝 Docker exec 与
  `/:/host` 恶意 bind create
- rescue CLI 在 updater 运行时仍可调（无 lock 冲突）

> 完整升级 + 回滚 + self-update flow 的 manual 验证需要本地 `docker compose up -d` + 至少
> 一个真实 release.json on GitHub。第一个公开 tag (`v0.1.0`) 是天然的端到端测试。

## 18. 实施分期

### M1（必须，v1.0 包含）

- §1-9, §10-13 完整实现
- §14.1 自更新最小通路
- §16 日志 + rescue CLI
- §17 测试矩阵全过

### M2（强烈建议）

- btrfs/zfs snapshot 优化
- ~~cosign 验签~~ ✅ 已实现（§15.4）
- registry mirror（基础支持已实现，通过 `REGISTRY_MIRROR` env）

### M3（可选，可能永不做）

- named volume 支持
- rootless docker
- 离线包

## 19. 当前部署基线

当前仓库只保留 proxy + updater 生产布局：

1. `pgdata` 使用 `./pgdata` bind mount
2. `.env` 包含 `MYRIAD_TAG`、`PROXY_TAG`、`UPDATER_TAG`、`COMPOSE_PROJECT_NAME`、`UPDATE_TOKEN`，可选 `MYRIAD_DOCKER_NETWORK` / `MYRIAD_ADMIN_NETWORK` / `MYRIAD_DOCKER_GUARD_NETWORK`
3. compose 文件 image 使用 immutable tag 变量，不使用 `:latest`
4. 只有 `proxy` 暴露宿主端口
5. 只有 `docker-guard` 挂载原始 docker.sock；updater 仅通过内部策略代理访问 Docker API
6. updater 只挂载一次部署根目录；pgdata、state、快照和 `.env` 均从该目录下访问
7. 网络三分：business / admin / guard；`updater-gateway` 在 admin-net 为 backend 注入 token
8. `./state` 和 `./backups` 由部署脚本创建
9. `UPDATE_TOKEN` 保存在同一个 `.env`，不暴露给浏览器，也不进入 backend 容器 env

`scripts/docker/deploy.sh` 和 `scripts/docker/deploy.ps1` 是当前生产布局的 bootstrap 入口。
