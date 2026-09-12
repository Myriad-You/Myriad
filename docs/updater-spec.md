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
| proxy 镜像 | `<registry>/myriad-proxy` | **独立节奏**：仅自身有改动时才编进该次 release；不对齐 app 版本号 |
| updater 镜像 | `<registry>/myriad-updater` | **独立节奏**：同上；多数 app release 的 `images` 可省略二者 |
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
    "min_updater_version": "v0.4.0",
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
- `images.backend` / `images.frontend`：**必填**。`images.proxy` / `images.updater`：**可选**；infra 无变动的 app release 应省略二者（不对齐版本、不 retag）。
- 未知字段：updater 必须忽略不报错（向前兼容）。

## 4. GitHub Actions 流水线

### 4.1 release.yml（push tag `v*` 触发）

原生多架构构建（**不用 QEMU**）：`linux/amd64` 跑在 `ubuntu-latest`，
`linux/arm64` 跑在 `ubuntu-24.04-arm`；按 digest 推送后再合成 manifest list。

相对上一 `v*` tag：`backend` / `frontend` **始终**编；`proxy` / `updater`
仅当 `proxy/`、`updater/` 有变动（或 tagged commit 标题含 `-full` / dispatch
`force_infra`）时编进本次 release，并写入 `release.json.images`。无变动则
**完全省略**（不 retag、不对齐 app 版本）。

```
jobs:
  resolve:                  # 算 version + build_components（含 infra 变动检测）
  frontend-assets:          # 静态产物只编一次（amd64），两平台 runtime 复用
  build:
    strategy.matrix:
      component: ${{ build_components }}   # 至少 backend,frontend
      platform:  [linux/amd64, linux/arm64]
    runs-on: native runner for platform
    steps:
      - build & push-by-digest <registry>/myriad-<comp>
      - 输出 per-arch digest artifact
  merge:
    strategy.matrix.component: ${{ build_components }}
    steps:
      - docker buildx imagetools create → :<version> manifest list
      - 输出 manifest-list digest（写入 release.json 的正是这个）
  publish:
    needs: merge
    steps:
      - 拼 release.json（只含本次 ship 的 images）
      - cosign sign-blob + 生成 SHA256SUMS
      - gh release create
```

镜像 CI 使用 `CARGO_PROFILE=ci-release`（thin LTO）；本地 `cargo build --release`
与默认 Docker 构建仍为全量 LTO。消费侧只认 tag + manifest-list digest，无需区分。

自更新 / proxy 更新：读 channel 内最近若干份 `release.json`，取**仍列出**
`images.updater` / `images.proxy` 的最新一份；都没有则回退 Docker Hub tip。

### 4.2 docker-publish.yml（push 条件打包 + workflow_dispatch）

- push 到 `main` / `preview` / `beta`：提交标题含 `-p` 或 `-full` 才打包；否则跳过
  - `-p`：编 backend/frontend，infra 仅路径有变动时加入
  - `-full`：同上，并**强制**编 proxy/updater（即使无变动）
- 也可在 Actions 中 `workflow_dispatch` 手动触发（可选 `tag`、`components`、`force_infra`）
- 仅推开发镜像，tag 使用 `dev-<sha>`、分支名，或输入的 `tag` 覆盖
- 构建方式与 §4.1 相同：原生双架构 + frontend assets 单编 + imagetools 合成
- **禁止再推 latest 到生产仓库**

### 4.3 PR check

只 build 不 push（或推到 `pr-<num>` 临时 tag）

## 5. 用户侧 docker-compose 结构

下面是结构示意，**以仓库根 `docker-compose.yml` 为准**。现行编排用
`UPDATER_IMAGE_REF` digest pin 跑 TCB，Guard 不持有 `UPDATE_TOKEN`，也不把
`MYRIAD_TAG` / `UPDATER_TAG` 盖进容器 `MYRIAD_VERSION`。

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
`MYRIAD_TAG`；updater 自更新改写 `UPDATER_IMAGE_REF`（`repo@sha256` digest pin）。
`UPDATER_TAG` 只作首次安装回退，compose **不以** tag 覆盖容器 `MYRIAD_VERSION`
（身份打在镜像 ENV 里）。`PROXY_TAG` 目前走手动 tag 升级路径。

`.env` 必须包含：

```
MYRIAD_TAG=v0.4.9
PROXY_TAG=v0.4.6
UPDATER_TAG=v0.4.6
UPDATER_IMAGE_REF=docker.io/somekawahitomi/myriad-updater@sha256:<64hex>
DOCKER_GUARD_IMAGE=docker.io/somekawahitomi/myriad-updater@sha256:<64hex>
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
  └─► pre-swap cleanup → idle
       1. 按失败进度重启已停止的服务：
          - 已 stop frontend/backend → `compose up -d backend frontend`
          - 已 stop postgres（bundled 快照）→ 先确保 postgres 再起 app
       2. `MYRIAD_TAG` 未改写时**不**做 pgdata snapshot restore
       3. finalize job=`failed`，清 maintenance
       4. 若服务恢复本身失败 → `needs_manual`（保持维护页，提示人工 compose up / rescue）

任意步骤失败 (swap_tag 之后)：
  └─► rollback_in_progress
       └─► stop_new → restore_snapshot → swap_tag_back → start_old → health_probing
            └─► (成功) idle（job=`failed`，服务回到上一版本）
            └─► (失败) needs_manual

`swap_tag_back` 的目标 tag（上一正常业务版本）按以下优先级解析，**从不写死版本号**：

1. 更新流程在 `swap_tag` 前从 `.env` 读到的 `MYRIAD_TAG`（同 job 自动回滚）
2. 快照元数据 `source_version`（创建快照时记录；若 state 无版本则回填 `.env` 的 `MYRIAD_TAG`）
3. `updater.json.current_version`（成功升级后推进；daemon 启动时也会用 backend build stamp / `.env` 自动恢复）

手动 `POST /rollback` / rescue 与自动回滚共用同一解析逻辑；回滚健康通过后会把 `current_version` 写回恢复到的版本。`current_commit_sha` 在无法立即确认时清空，并在下次 daemon 启动时从 build stamp 或 GitHub tag 重新解析，避免保留错误 SHA。

```

业务更新在启动目标构建前，会把当时正在运行且已验证的 backend/frontend 镜像钉到
本地 `*:myriad-rollback`，并将该版本写入 `rollback_version`。目标构建健康通过后只推进
`current_version`，不会用当前构建覆盖回退槽位；下一次更新开始前才将槽位推进到届时的
当前构建。回滚时若快照解析出的原版本 tag 已不在本地，但它与 `rollback_version` 一致，
updater 会先从 `*:myriad-rollback` 重新创建原版本 tag，再交给 Compose 启动。

### 7.1 崩溃恢复表

启动时读 `state/job.current`：

| 上次状态 | 重启后动作 |
|---|---|
| `idle` | 正常启动 |
| `preflight` | 撤销 lock，回 idle（无副作用） |
| `maintenance_on` | 退维护，回 idle |
| `stopping` / `snapshotting` | 清 maintenance + 标记 job failed，并 **best-effort 重启** 上一栈（postgres + backend/frontend）；不自动进入 swap |
| `swap_tag` 之后任意步骤 | **不自动恢复**，进 `needs_manual` |
| `health_probing` 且 `active=false`（为测前端已抬起维护） | **仍算 post-swap**：进 `needs_manual`（**禁止**当 idle） |
| `job.current` 仍指向 Running 且 last step 为 post-swap | 即使 maintenance 文件 inactive / 损坏 → `needs_manual` |
| `rollback_in_progress` | 检查 snapshot 在 → 继续 restore；不在 → `needs_manual` |
| `needs_manual` | 维持，proxy 维护页显示 rescue 指令 |

**规则**：`swap_tag` 之后的任何步骤崩溃，必须人工 `POST /rescue/continue` 或 `/rescue/rollback`。
崩溃恢复决策以 `plan_crash_recovery` 为准，**不得**只看 `maintenance.active`。

**审计行**：`audit: pre_swap_cleanup_ok` / `audit: pre_swap_restore_failed`（swap 前失败恢复）；
`audit: auto_rollback_ok`（swap 后自动回滚成功）；`audit: recovery_needs_manual` / `recovery_pre_swap_clear`。

`/status` 返回 `last_failed_update`：自动回滚成功后 maintenance 已 idle 时，UI 仍能提示「上次更新未成功」。操作员可 `POST /last-failed/dismiss` 永久关掉这条提示（清掉 `updater.json` 里的记录）；下次失败会重新出现。成功更新也会清掉未确认的失败横幅。

### 7.2 更新主流程失败调度（实现）

`update::run` 在 `maintenance_on` 之后使用 `UpdateFlowCtx` 跟踪进度：

| 标志 | 含义 | 失败时动作 |
|------|------|------------|
| `scope` | pre-swap 已停哪些服务 | `finish_pre_swap_failure` 按 scope 重启 |
| `post_swap` | **`swap_tag` 已成功写入** 新 `MYRIAD_TAG` | `finish_with_rollback`（snapshot + tag + 起旧镜像） |
| `committed` | health 已通过（在任何 post-health 写状态之前置位） | **禁止**自动回滚；job 记 `Succeeded`，清维护 |

任何 `?` 错误都进入统一的 `dispatch_update_failure`，避免裸错误路径遗漏恢复。

硬不变量（实现注释 + 单测约束）：

1. `post_swap` 仅在 `swap_tag` **成功**后置位  
2. `committed` 在 health **一通过**立刻置位；之后禁止自动回滚，job 记 Succeeded  
3. preflight / maintenance 入口失败必须清维护  
4. rollback 中途 Err 后仍 best-effort `compose up` 业务容器  

回归：`./scripts/extra/test-updater-smoke.sh`；`./scripts/extra/test-updater-e2e.sh`；`cargo test -p myriad-updater --lib`。

## 8. 网络与下载

### 8.1 GitHub API

- 匿名默认（60 req/h），支持 `GITHUB_TOKEN`
- connect timeout 10s, total 60s
- 重试 3 次，指数退避 (1s, 4s, 16s)，**仅幂等 GET**
- ETag + Last-Modified 缓存到 `state/cache/`

### 8.2 镜像拉取

- 通过 docker 调用 `pull` (bollard)；**不钉死 platform**，由引擎按宿主机选 amd64/arm64
- **pull 后**读取 `RepoDigests`，与 `release.json` 中的 manifest-list digest 对账（完整 64 位 hex 相等；兼容省略 `sha256:` 前缀）
- 支持 `REGISTRY_MIRROR` env：retag 后 pull，digest 校验保持
- 开发镜像按拉取 digest 验 CI 签名；经确认的无签名/无清单路径只记录 digest，不能声称签名已验证
- 本次业务更新用临时 Compose 覆盖固定 backend/frontend/backend-volume-init 到预检 digest，禁止重新拉取；启动后核对容器镜像 ID。回滚继续使用原 Compose runner
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

**M1 要求 pgdata 是 bind mount（仅 bundled 模式）。** 跨设备会降级为 copy 兜底并写入 warning；
docker named volume、rootless Docker、Podman 仍会启动时报错。
**路径不存在**（例如首次部署、postgres 尚未初始化）在 env-probe 中为 **warning**，
不阻止 updater 启动；真正需要快照/回滚恢复的操作会以 Precondition 失败。

### 9.1.1 外部 Postgres（`MYRIAD_DB_MODE`）

当数据库不在 compose 内、也没有宿主侧 `./pgdata` 可快照时，在 `.env`（或进程环境）设置：

```bash
MYRIAD_DB_MODE=external
```

| 取值 | 默认 | 行为 |
|---|---|---|
| `bundled` | **是**（未设置时） | 现有行为：更新前快照 `./pgdata`，回滚可恢复 pgdata + 镜像 tag |
| `external` | 否 | **不要求** `UPDATER_PGDATA` / `./pgdata` 存在；**跳过** pgdata 快照与恢复；回滚/rescue **只恢复镜像 tag**（`MYRIAD_TAG` 等，与今日一致） |

解析顺序：进程环境 `MYRIAD_DB_MODE` → 挂载的 `UPDATER_ENV_FILE`（`.env`）→ 默认 `bundled`。
**不会**根据 `DATABASE_URL` 主机名静默切换模式。若 `MYRIAD_DB_MODE=bundled`（默认）且
`DATABASE_URL` 主机明显不是 compose 服务名 `postgres`，env-probe 写入 **warning**，提示改为
`external`。

`GET /status`、`state/env-probe.json` 与 diagnostics 暴露：

- `db_mode`: `"bundled"` | `"external"`
- `pgdata_snapshot_enabled`: `true` 当且仅当 bundled

外部模式下 preflight 磁盘检查（按 pgdata 体积估算快照空间）为 no-op 并打 warning。

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

- **备份上限**（`snapshot_limit_enabled`，默认开启；`snapshot_limit` 默认 3，范围 1–20）：
  对「非 keep / 非 in-use」的快照（**不限年龄**）只保留最新 N 份，其余删除。
- **何时执行 prune**（缺一不可，否则失败更新会无限堆积备份；已有堆积须能自愈）：
  1. `POST /prefs` 保存/改动上限设置时立即 prune
  2. **更新成功** finalize 后（先清 `job.current`，再 prune，使刚创建的备份计入 N）
  3. **更新失败**且已创建过快照：pre-swap 清理后、post-swap 自动回滚后、以及 needs_manual 时（rescue 引用的那份仍受 `in_use` 保护）
  4. **`GET /snapshots`**（打开备份 UI 时）：若已超限则 best-effort prune，响应带诊断字段
  5. **周期性 check tick**（`CHECK_INTERVAL` / prefs）：每次查询可用更新后 best-effort prune
- **诊断字段**（`GET /snapshots` 与 `POST /prefs` 响应）：`snapshot_limit_enabled`、`snapshot_limit`、
  `eligible_count`（可自动删的份数）、`protected_count`（keep / in-use）、`total_count`、
  以及本次 prune 的 `pruned_snapshot_ids`（可能为空）。前端据此展示有效计数，并在 status
  缺少 `snapshot_limit*` 时提示 updater 镜像可能过旧（需宿主运维按摘要策略升级 TCB）。
- **磁盘一致性**：meta 跟踪的 id 若 `remove_dir_all` 失败须打 error/warn 且**不得**从
  `snapshots.json` 删除该 id；prune 后扫 `state/snapshots/` 下不在 JSON 中、且名称符合
  安全模式（opaque id / `{id}.tmp` / `broken-inplace-*` / `restore-stage-*`）的孤儿目录并清理。
- 关闭上限后不再按数量自动清理（仍可手动 `DELETE /snapshots/{id}`；孤儿目录扫除仍可运行）。
- `keep=true`（如 major 升级前永久标记）永不自动删
- 删除前确认不是 in-flight job / needs_manual rescue 引用
- 年龄不再作为「永远保留」的例外；上限对自动管理备份是硬上限
- prune 失败须打 warn；读取 prefs 失败不得静默当成「上限关闭」

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
- 普通业务更新只修改 `.env` 中的 `MYRIAD_TAG`
- updater 不靠改 `UPDATER_TAG` 覆盖自身身份；TCB 由 Guard 自更新改写 `UPDATER_IMAGE_REF` / `DOCKER_GUARD_IMAGE`。`UPDATER_TAG` 只作无 digest pin 时的回退
- `PROXY_TAG` 目前由人工编辑 `.env` 后运行 `scripts/extra/deploy.sh upgrade`
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

`/health` 是存活探针：进程在就返回 HTTP 200。`db_connected` 表示**最近一次**
`SELECT 1` 成功，不是「进程里还有数据库句柄」。启动写入预检与当前可写是两
个字段。业务就绪看 `/ready`（未就绪 503）。

```json
{
  "status": "ok",
  "version": "v1.2.3",
  "commit_sha": "0123456789abcdef0123456789abcdef01234567",
  "schema_version": 1,
  "db_connected": true,
  "db_handle_present": true,
  "db_probed_at": 1710000000,
  "migrations_applied": true,
  "routes_full": true,
  "storage_preflight": true,
  "storage_writable": true,
  "uptime_seconds": 123
}
```

backend 必须实现此 schema，updater 严格校验：升级与回滚都要求数据库探测、
迁移、完整路由就绪，以及 `storage_writable`（缺字段时兼容为 true）。软通过
（仅 HTTP 200 / 镜像标签吻合）只记为降级，不得当作业务恢复成功。旧镜像没有
`routes_full` 时，updater 回退到 `mode == "full"`。

`/ready` 与 `/health` 一样由 backend 提供。生产入口经 proxy 转发，开发入口经
Astro dev proxy 转发；updater 仍直连 `http://backend:1103/health` 读 JSON。
不要用前端 HTML 的 HTTP 200 认定业务就绪。
新版 backend 在启动时以实际运行 UID 对 data/cache 及现有 Tapp owner 目录
执行写入探针；探针失败时不得进入健康状态。`storage_writable` 是最近一次
写入探测，不是启动预检本身。

### 11.2 frontend 健康

- HTTP 200
- index.html 包含 `<meta name="myriad-version" content="v1.2.3">`
- index.html 包含 `<meta name="myriad-commit" content="<40-char sha>">`
- updater 抓取并对比 target version

### 11.2.1 two-phase probe (post-swap)

After start_new, health probing is **two-phase** so frontend is checked on the live path (not only behind maintenance HTML):

1. **Backend-only** while maintenance is still `active=true`: direct `http://backend:1103/health` (version / commit_sha / image tag identity + DB). Users still see the maintenance page.
2. **Deactivate maintenance** (`active=false`) but **keep job id / phase** so rollback can re-enter maintenance if needed. Proxy cache settle ~2s.
3. **Live frontend via proxy** `http://proxy:80/`: prefer real-page meta (`myriad-version` / commit); reject maintenance HTML; soft path allows dual image-tag match if stamps lag.

Hard vs soft outcomes are logged with a `pass_kind` (e.g. `hard_backend`, `hard_fe_meta`, `soft_dual_image`). Needs **2** consecutive OK ticks per phase. Deadline recheck before destructive rollback uses live-via-proxy mode.

### 11.3 deadline

- 初始等待 ~5s（实现），再进入 phase loops
- 每 2s 探一次；每 phase 连续 2 次 hard（或 soft 窗口后）算通过
- 总超时 `max(300s, migrations.estimated_seconds × 3)`；phase1 约占总预算 55%（至少 60s）

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
| POST | `/last-failed/dismiss` | token | 永久关闭「上次更新未成功」横幅 |
| POST | `/rollback` | token | `{snapshot_id}` |
| POST | `/admin/self-update` | token | 一键请求可信 TCB 交接；Updater 仅提交 tag intent |
| POST | `/admin/proxy-update` | token | 手动升级 proxy（可选 body `{target_version}`；默认频道最新 release） |
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

### 13.5 更新信任路径

开发频道从 Docker Hub 查询前后端共有构建；分支目标需要 GitHub 解析后固定为
`dev-<sha>`。`vX.Y.Z` 目标即使从开发频道选中，也始终执行正式版本预检。

- `github_release`：优先使用 `release.json`，执行 digest、版本和迁移约束。签名状态
  分别记录 `verified`、`soft_unverified`、`off`，不能把清单存在等同于签名已验证。
- `signed_commit`：Cosign 验证 registry 中签名，核对 `docker-publish.yml` 的 GitHub
  Actions 身份（main/preview/beta）、完整 commit、组件和 digest；前后端完整 commit 相同。
- `dockerhub_tag`：清单不可得时，只能从配置仓库拉严格 `vX.Y.Z`，逐次确认。
- `dockerhub_commit`：历史开发镜像缺少签名，逐次确认；不是验证失败时的回退。

独立 `allow_tag_install` 不属于 `allow_risk` 伞形许可；它为 true 时 API 仍要求
`confirm_risk`。预检缺少确认时返回结构化任务失败（`confirmation_required`、`trust`、
原有 `risk_flags`），不进入维护。前端从异步结果发起同一目标的新任务。
任务记录拉取的镜像 ref/digest、实际签名状态和 commit，审计与历史同步落盘；成功任务
通过 `snapshot_id` 关联升级前快照。旧任务字段缺省不赋予信任。

开发签名使用 updater 镜像固定的 Cosign 2.4.1。只有 exit 10 表示无签名；其他非零退出、
网络错误、JSON/签名/身份/commit/digest 错误均拒绝，不能经确认跳过。清单路径的硬错误
也不能回退。自动更新只派发未授予 tag 许可的请求；遇到缺签名/清单需手动处理。
没有 GitHub 祖先关系本身不阻止开发更新，也不能充当签名验证。

完整部署说明与历史镜像升级方式见 [UPDATER_QUICKSTART.md](deployment/UPDATER_QUICKSTART.md)。

## 14. 更新 Guard / updater TCB

### 14.1 只允许宿主机运维升级

Guard 持有 Docker socket，是独立于 updater 的宿主 TCB。运行中的 updater、其 API、
`UPDATE_TOKEN` 和 updater 可写的 `.env` 都不得选择仓库、镜像 digest、Guard 命令或
Compose 服务集合。`POST /admin/self-update` 保留一键体验；Updater 使用宿主策略中的
专用 capability 向 Guard 的 `/_myriad/self-update` 提交 `{target_tag, trust_path}`，请求体
拒绝额外 repo、digest、command。Guard network 隔离是第一层边界，capability 用于避免
错误接入 guard-net 的其他容器直接触发 TCB 操作；它不用于防御本就有权读该值的 updater。

生产 Guard 镜像必须来自 `./guard-policy/docker-guard.env`（首次由 Guard 从 `.env` 写入），且形式严格为
`docker.io/somekawahitomi/myriad-updater@sha256:<64 hex>`。Guard 启动时通过原始 socket
inspect 自身容器，要求实际 `Config.Image` 与 `DOCKER_GUARD_EXPECTED_IMAGE` 完全一致。
`.env`、Updater 提供的仓库/digest 或 updater 状态均不是该身份的权威来源。

当前私有仓库阶段的自动交接流程（#265 的显式 `dockerhub_tag` 路径）：

1. Guard 只接受 `vX.Y.Z` / `dev-<sha>`，固定编译内置的官方 updater 仓库；tag 仅是意图。
2. Guard 通过宿主 Docker daemon 拉取 `official_repo:tag`，从实际镜像 `RepoDigests` 得到
   `official_repo@sha256`；禁止 release semver 与镜像创建时间回退。
3. Guard 先确认当前 updater/gateway 与宿主固定的旧 Guard digest 一致，再从目标精确
   digest 启动固定入口 `myriad-tcb-self-update`。请求方不能提供 entrypoint/argv/mount。
4. 交接程序校验渲染后的三项服务模型、镜像、网络、挂载、entrypoint 与安全选项，只执行
   `docker compose up --no-deps --force-recreate docker-guard updater updater-gateway`。
5. 成功时原子写入 `.env` 的 `UPDATER_TAG` / `UPDATER_IMAGE_REF` 与宿主策略的
   `DOCKER_GUARD_IMAGE`；任一步失败恢复旧文件并用旧精确 digest 回滚。状态写入
   `state/self-update-last.json`，UI 在短暂断线后轮询结果。

这里的“原子”仅指单个策略文件的临时文件替换，不表示 Docker Compose 的三容器切换是
事务。交接程序以固定摘要、健康检查、稳定性等待和最多两次回滚收敛保证最终一致；Guard
异常重启时从保留的固定 helper 容器恢复意图，且在 helper 未清理或回滚未收敛时保持
Docker mutation gate 关闭。
三次旧摘要恢复都失败时，Guard 将失败 helper 固定重命名为
`myriad-tcb-self-update-recovery-exhausted`；该 Docker daemon sentinel 跨 Guard 重启保留，
阻止重启后重置重试预算。宿主完成手动 TCB 恢复和校验后才能删除它并重启 Guard。

该路径信任 Docker Hub 官方仓库身份和 TLS/registry 控制面。Guard 解析出的 registry
digest **没有**与签名 release manifest 中的 `expected_digest` 做字节级绑定，因此不声称
做了 release.json/Cosign 验证。公开 GA 前按 #265 增加该签名证明路径；正常用户操作仍
保持同一个一键按钮。
4. 运行 `deploy.sh doctor`，确认运行镜像与期望 digest 完全一致且无旧版动态策略/token。

回滚同样由宿主把策略文件恢复到此前已验证的 digest 后重建 TCB。不得从 updater 的
`state/` 或 `.env` 自动决定回滚 Guard 身份。业务镜像的日常更新仍经
`DOCKER_HOST=tcp://docker-guard:2375` 受固定请求体与镜像策略约束。

### 14.4 手动 proxy 升级的失败回退（轻量）

`POST /admin/proxy-update`（同步）在改写 `PROXY_TAG` 并 `compose up proxy` 后：

1. 在 compose 网络上轮询 `http://proxy:80/healthz`（约 25s）。
2. **compose 失败或 healthz 失败**：将 `PROXY_TAG` 写回 `previous_tag`，再
   `compose up proxy` 尽量拉起旧镜像；写入
   `state/proxy-update-last.json`（`status: failed`、`rolled_back`、`error`）。
3. **成功**：写 `status: succeeded`。
4. **`GET /status`** 可选字段 `proxy_update_last`（与 `self_update_last` 同形）。

仍不保证「健康检查误报」或「旧镜像也已损坏」时的二次恢复；此时需主机手动改
`PROXY_TAG` 后 `docker compose up -d proxy`。

### 14.2 兜底

用户可在部署目录手动执行（宿主机管理员路径）：

```
# 生产请改 UPDATER_IMAGE_REF 与 DOCKER_GUARD_IMAGE（同一 digest），再重建 TCB
# 无 pin 的开发安装才改 UPDATER_TAG
docker compose --env-file .env --env-file ./guard-policy/docker-guard.env up -d docker-guard updater updater-gateway
```

### 14.3 前向兼容

- 未知 release.json 字段忽略
- `min_updater_version` 高于自己 → 优雅拒绝，提示先升 updater
- 即使数年不升 updater，遇到不兼容版本会停在"无法升级业务"，不误升级

## 15. 安全

运维「已完成」基线、红线与刻意接受项见
[deployment/UPDATER_SECURITY_BASELINE.md](./deployment/UPDATER_SECURITY_BASELINE.md)
（单租户自托管；后续加固仅事件驱动）。

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
- **更新 preflight**（不改编排，仅只读探测，失败则**不停服**）：
  1. **本地环境**：`.env` 仍含 `MYRIAD_TAG` / `PROXY_TAG` / `UPDATER_TAG`；compose 仍引用
     `${MYRIAD_TAG}`；`state/`（及 bundled 下 `state/snapshots/`）与 `.env` 可写；经
     docker-guard 的 Docker API `ping` 可达。external 模式要求 `DATABASE_URL`（不要求
     `POSTGRES_PASSWORD`）；bundled 要求 `POSTGRES_PASSWORD`。
  2. **Compose 契约**（`docker compose config`，且**更新时重扫** compose 文件）：必备服务
     `backend`/`frontend`（bundled 含 `postgres`）；`container_name` 须为 `myriad-backend`
     等固定名；bundled 下 postgres 的 pgdata 须为 **bind**；运行中容器 project 标签一致
     （external 不 inspect 残留 `myriad-postgres`）。
  3. **网络 allowlist**：三网 allowlist + 已存在 + 运行中容器不得挂外来网。
  4. **release 无 manifest**：GitHub `release.json` 不可用时，须逐次确认后才可从配置
     仓库按严格 `vX.Y.Z` 安装。记录 digest，但无清单 digest/Cosign/min_from 保证。
     Cosign/schema/digest **硬失败** 不能 fallback。
  5. **NeedsManual**：卡住时拒绝新的业务 update / auto-install（rescue/rollback 仍可用）。
  6. **健康 recheck**：探针超时后仅 **HardOk** 可跳过回滚；SoftOk（含维护页）必须回滚。
  面板改 project / container_name / 外来网络 / pgdata named volume 时，应在此阶段被拦下。
- **updater 不在业务 `myriad-net`**：frontend/postgres 与 updater HTTP 无共享 L2；
  backend 经 admin-net 访问 `updater-gateway`。
- **推荐部署中 backend 进程不持有 `UPDATE_TOKEN`**：token 仅在 updater、updater-gateway
  环境中；Guard 不持有 updater 凭据，gateway 在 admin-net 上注入 header。攻击面从
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
- 高风险更新：当 body 含 `allow_tag_install` / `allow_risk` / `allow_downgrade` / `allow_diverged|unknown|irreversible`
  为 true 时，另需 `confirm_risk: true` 或 header `X-Myriad-Confirm-Risk: true`。普通升级无额外字段。

### 15.3 token / gateway secret

- `UPDATE_TOKEN` 必须 ≥ 32 字符
- `UPDATER_GATEWAY_SECRET` 必须 ≥ 32 字符（backend + updater-gateway）
- 启动时检查不在弱密码列表（UPDATE_TOKEN）
- 5 次/min 401 后封 10min（updater token）
- 生产 compose：`UPDATE_TOKEN` 只注入 updater / updater-gateway；**不**注入 backend 或 docker-guard
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
| `audit: auto_rollback_ok job=…` | 更新失败后（swap 后）自动回滚成功 |
| `audit: pre_swap_cleanup_ok job=…` | swap 前失败后已重启上一栈并退出维护 |
| `audit: pre_swap_restore_failed job=…` | swap 前失败且重启上一栈也失败 → needs_manual |
| `audit: rollback_start job=… snapshot=…` | 独立回滚任务开始 |
| `audit: self_update_* …` | TCB 一键交接请求、trust path、目标 tag 与执行结果 |
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
- **e2e** — 通过 `scripts/extra/test-updater-e2e.sh` 自动验证（本地二进制 + /tmp testbed）
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
| 11b | compose 网络不在 docker-guard allowlist | preflight 拒绝（停服前） | unit + manual |
| 11c | 面板改 container_name / project / pgdata named volume / .env 只读 | preflight 拒绝（停服前） | unit + manual |
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

> 完整业务升级 + 回滚 flow 的 manual 验证需要本地 `docker compose up -d` + 至少
> 一个真实 release.json on GitHub。第一个公开 tag (`v0.1.0`) 是天然的端到端测试。

## 18. 实施分期

### M1（必须，v1.0 包含）

- §1-9, §10-13 完整实现
- §14.1 Guard 独立固定官方仓库/digest 并自动交接 TCB
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
2. `.env` 包含 `MYRIAD_TAG`、`PROXY_TAG`、`UPDATER_TAG`、`COMPOSE_PROJECT_NAME`、`UPDATE_TOKEN`；生产另含 digest 钉死的 `UPDATER_IMAGE_REF` / `DOCKER_GUARD_IMAGE`。可选 `MYRIAD_DOCKER_NETWORK` / `MYRIAD_ADMIN_NETWORK` / `MYRIAD_DOCKER_GUARD_NETWORK`
3. 业务镜像用 `${MYRIAD_TAG}` / `${PROXY_TAG}`；TCB 用 `UPDATER_IMAGE_REF` digest pin；不使用 `:latest`
4. 只有 `proxy` 暴露宿主端口
5. 只有 `docker-guard` 挂载原始 docker.sock；updater 仅通过内部策略代理访问 Docker API
6. updater 只挂载一次部署根目录；pgdata、state、快照和 `.env` 均从该目录下访问
7. 网络三分：business / admin / guard；`updater-gateway` 在 admin-net 为 backend 注入 token
8. `./state` 和 `./backups` 由部署脚本创建
9. `UPDATE_TOKEN` 保存在同一个 `.env`，不暴露给浏览器，也不进入 backend 容器 env

`scripts/extra/deploy.sh` 是当前生产布局的 bootstrap 入口。
