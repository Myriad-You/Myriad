# Myriad Updater Spec

> 自托管 Myriad 实例的"维护模式 + 单运行槽 + 版本标签 + 数据快照"更新系统设计规范。
> 本文档是 updater / proxy / release 流水线之间的契约，所有实现必须以此为准。

**重要边界**：Myriad updater 不是双活 A/B 分区系统。运行时始终只有一套
`backend` / `federation-worker` / `persona-worker` / `frontend` / `postgres`
业务容器（同一 backend 镜像的三个进程角色）；updater 只在维护模式下停止业务容器、
创建 `pgdata` 快照、切换 `.env` 中的镜像 tag，然后启动新版本。回滚依赖快照和旧
tag，而不是并行保留 A/B 两套在线分区。

## 0. 设计原则

1. 下载和检查在停机前完成；检查失败不改变正在运行的部署，也不关闭用户的自动更新设置。
2. 用实际运行镜像和健康状态判断结果，成功记录负责重启后的收尾。HTTP 受理响应不代表更新完成。
3. 数据快照和恢复必须在数据库写入者停止后执行。更新失败自动恢复旧版本，并检查恢复结果。
4. 以当前已发布版本为升级兼容基线。兼容转换自动执行，过渡代码注明下一版移除，避免长期维护多条更新流程。

### 本次重构的升级兼容

兼容基线为已发布的 v0.5.3。保留其 Compose、状态文件和 HTTP 请求格式；升级不要求用户编辑部署文件。测试中的 v0.5.3 Compose 是该发布标签的原文件，不能随当前模板一起改写。

以下过渡逻辑在本次改动发布后的下一版移除：

- v0.5.3 Guard 在写入受理状态前返回 HTTP 响应。新版 updater 在后台等待它写出新记录，期间保持更新互斥；新版 Guard 在响应中确认状态已写入，直接走异步流程。
- v0.5.3 proxy 接口同步等待结果，替换期间可能丢失响应。页面沿用旧版 90 分钟的结果观察窗口，使用同一个状态轮询等待新结果；组件返回的 `scheduled` 缺失不会被当成错误。
- 旧 Guard 的恢复 helper 镜像身份约定、混合版本恢复标签，以及旧的恢复耗尽容器名，由新版自动识别和处理。
- 自更新响应中的 `helper_container_id`，以及 proxy 响应中的 `image_ref`、`pulled_digest`，只为旧客户端保留。是否完成由持久状态确定。

生产代码只保留一份 Compose 文件发现、一份业务镜像准备流程和一套页面状态轮询。worker 的停止、镜像能力和健康检查共用读取结果，但 federation 正常退出与 persona 持续运行的条件分别保留。

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
│          ├─► backend ─► postgres                       │
│          ├─► federation-worker                         │
│          └─► persona-worker                            │
│                │                                       │
│  [myriad-admin-net]                                    │
│   backend ──► updater-gateway ──► updater              │
│                                        │               │
│  [myriad-docker-guard-net internal]                    │
│                                        └──► docker-guard
│                                              └── sock  │
└──────────────────────────────────────────────────────┘
```

- `proxy`：唯一对外暴露端口的组件，负责维护页与反向代理。极少更新；兼入 admin-net 以便
  rescue 解析 `updater`。官方 compose 设 `PROXY_FEDERATION_UPSTREAM` /
  `PROXY_PERSONA_UPSTREAM`，把联邦与 persona 域直接打到对应 worker。
- `updater-gateway`：同 updater 镜像的薄反代；校验 `X-Updater-Gateway-Secret` 后注入
  `X-Update-Token`；**仅** admin-net；不持 compose 挂载、不挂 docker.sock。`/healthz` 不需 secret。
- `updater`：接收更新指令，执行更新/回滚流程；不挂载 docker.sock；**不在**业务 `myriad-net`。
  编排根只读，可写叠加 `.env` / `state` /（bundled）`pgdata`。
- `docker-guard`：唯一挂载 docker.sock 的内部服务，按 API、Compose 项目标签、镜像仓库和
  `containers/create` 请求体执行白名单策略。**不**持有 `UPDATE_TOKEN`；自更新用
  `GUARD_SELF_UPDATE_TOKEN`。
- `backend` / `federation-worker` / `persona-worker` / `frontend`：业务组件，由 updater
  拉取并启停。三个进程共用 backend 镜像；web 双宿 business+admin，worker 只在
  `myriad-net`。**生产 compose 不注入 `UPDATE_TOKEN`**，web 注入 `UPDATER_GATEWAY_SECRET`
  以调用 gateway。
- `postgres`：数据存储。bundled 模式下更新前后由 updater 做文件级快照。

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
- `postgres.min_pg_version`：唯一有效的 PostgreSQL 兼容边界；不设置上限。仅对 bundled 生效（updater 读取旧式 `PGDATA/PG_VERSION` 或 PostgreSQL 18 官方镜像的 `PGDATA/<major>/docker/PG_VERSION`）；外置数据库（`MYRIAD_DB_MODE=external`）无本地 PGDATA 可探测，updater 只记录该下限、不据此拒绝更新，版本由运维自行保证。
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
# 结构示意。字段与挂载以仓库根 docker-compose.yml 为准，不要照抄本块去部署。
services:
  postgres:            # bundled only; ./pgdata bind → /var/lib/postgresql
  backend-volume-init: # 一次性修 named volume 属主；更新器用 --force-recreate 原地换新，不另起 disposable
  backend:             # MYRIAD_PROCESS_ROLE=web；双宿 myriad-net + admin-net
  federation-worker:   # 同 backend 镜像；command /app/myriad-federation-worker
  persona-worker:      # 同 backend 镜像；command /app/myriad-persona-worker
  frontend:
  proxy:               # 唯一宿主端口 HTTP_PORT:80
                       # PROXY_FEDERATION_UPSTREAM / PROXY_PERSONA_UPSTREAM 必填
  docker-guard:        # 唯一 docker.sock；DOCKER_GUARD_IMAGE digest + GUARD_SELF_UPDATE_TOKEN
  updater:             # 编排根只读；可写叠加 .env / state /（bundled）pgdata
  updater-gateway:     # admin-net only；注入 X-Update-Token
```

当前仓库的 `docker-compose.yml` 使用单个宿主 `.env` 作为部署契约。它同时保存业务
镜像 tag、数据库/JWT 配置，以及 updater 所需的 token/channel。普通业务更新只改写
`MYRIAD_TAG`；updater 自更新改写 `UPDATER_IMAGE_REF`（`repo@sha256` digest pin）。
`UPDATER_TAG` 只作首次安装回退，compose **不以** tag 覆盖容器 `MYRIAD_VERSION`
（身份打在镜像 ENV 里）。`PROXY_TAG` 目前走手动 tag 升级路径。

`.env` 必须包含（完整键以 `.env.production.example` 为准）：

```
MYRIAD_TAG=v0.5.3
PROXY_TAG=v0.5.2
UPDATER_TAG=v0.5.2
UPDATER_IMAGE_REF=docker.io/somekawahitomi/myriad-updater@sha256:<64hex>
DOCKER_GUARD_IMAGE=docker.io/somekawahitomi/myriad-updater@sha256:<64hex>
COMPOSE_PROJECT_NAME=myriad
UPDATE_TOKEN=<32+ char random>
UPDATER_GATEWAY_SECRET=<32+ char random>
GUARD_SELF_UPDATE_TOKEN=<32+ char random>
MYRIAD_SETUP_SECRET=<32+ char random>
PERSONA_DB_PASSWORD=<32+ URL-safe>      # bundled；external 另需 PERSONA_DATABASE_URL
FEDERATION_DB_PASSWORD=<32+ URL-safe>   # bundled；external 另需 FEDERATION_DATABASE_URL
CHANNEL=stable
GITHUB_TOKEN=    # 可选，提升 rate limit
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
（两个 worker 共用 backend 镜像，随同一 `MYRIAD_TAG` 启停）
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
| `stopping` / `snapshotting` | 清 maintenance + 标记 job failed，并 **best-effort 重启** 上一栈（postgres + backend + federation-worker + persona-worker + frontend）；不自动进入 swap |
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
- **pull 后**读取 `RepoDigests`，与 `release.json` 中的 manifest-list digest 对账（相等或 `ends_with`）
- 拉取和启动使用同一官方镜像引用；加速由 Docker daemon 的原生 registry mirror 处理。
  旧 `REGISTRY_MIRROR` 环境变量不再改写仓库地址。
- Docker Hub tip / 列表回退路径不做 digest 对账（只按 tag pull）
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

数据库模式与网络授权独立：预检和 Guard 额外允许 backend 与两个 worker 接入 `MYRIAD_BACKEND_EXTRA_NETWORK` 指定的网络（默认 `myriad-backend-ext`）；其他服务不能接入，worker 仍不能接入管理网或 Guard 网。旧版 updater/Guard 须一起升级到包含此支持的构建。三个进程分别配置连接同一库/schema 的 URL，详见[外部 PostgreSQL 部署](deployment/EXTERNAL_POSTGRES.md)。以下模式行为以通过网络预检为前提。

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
Vite dev proxy 转发；updater 仍直连 `http://backend:1103/health` 读 JSON。
不要用前端 HTML 的 HTTP 200 认定业务就绪。
新版 backend 在启动时以实际运行 UID 对 data/cache 及现有 Tapp owner 目录
执行写入探针；探针失败时不得进入健康状态。`storage_writable` 是最近一次
写入探测，不是启动预检本身。

### 11.2 部署健康与镜像一致性

预检保存拉取后的 backend/frontend 本地 Image ID；启动迁移前检查 Compose 选择的
backend、frontend 和 backend-volume-init 确实使用这些镜像。Compose 启动使用
`--pull never`，避免预检后再次拉取浮动 tag。

维护模式保持开启，同时检查：

- backend/frontend 实际容器 Image ID 与预检选择一致，容器运行且 Docker health 为 healthy。
- backend 私有 /health 的数据库、迁移、路由和存储状态就绪。
- federation/persona worker 健康，proxy /healthz 正常。

frontend 使用容器自身的本地 HTTP healthcheck；不再猜测页面文本、版本 tag 子串，
也不为探测提前开放用户流量。连续两次通过后持久化成功，再解除维护。
这不等价于从公网验证用户入口；公网 DNS/TLS 不属于此探针。

### 11.3 deadline

- 每 2s 探测一次，连续两次完整通过。
- 总超时 `max(300s, migrations.estimated_seconds × 3)`。
- 未通过则在维护模式内自动回滚；成功落盘后重启可自动完成状态收尾和解除维护。
- 健康通过后的状态写入故障自动重试，不重新回滚，也不改为人工恢复。
- 预检失败保留自动更新偏好，下一次定时检查可重新尝试；已切换后失败的同一目标不反复自动安装。

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
| GET | `/process-logs` | token | 核心 Compose 服务最近的 warning/error 与无级别 stderr 报告 |
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

## 14. 更新 Guard / updater TCB

### 14.1 只允许宿主机运维升级

Guard 持有 Docker socket，是独立于 updater 的宿主 TCB。运行中的 updater、其 API、
`UPDATE_TOKEN` 和 updater 可写的 `.env` 都不得选择仓库、镜像 digest、Guard 命令或
Compose 服务集合。`POST /admin/self-update` 保留一键体验；Updater 使用宿主策略中的
专用 capability 向 Guard 的 `/_myriad/self-update` 提交 `{target_tag, trust_path}`，请求体
拒绝额外 repo、digest、command。Guard network 隔离是第一层边界，capability 用于避免
错误接入 guard-net 的其他容器直接触发 TCB 操作；它不用于防御本就有权读该值的 updater。

生产 Guard 的持久化镜像身份使用
`docker.io/somekawahitomi/myriad-updater@sha256:<64 hex>`。启动时通过原始 socket
inspect 自身容器和实际 image ID，核验 Compose 项目、服务、官方仓库摘要及镜像内置版本；
宿主手动部署可使用官方版本 tag，旧 `DOCKER_GUARD_EXPECTED_IMAGE` 不再阻止新镜像启动。
`.env`、容器覆盖的版本环境变量、Updater 提供的仓库/digest 或 updater 状态均不是身份权威。

Guard 开始提供健康检查后，启动核验等待 Guard、updater、gateway 全部健康且运行相同
image ID、摘要和内置版本，再同步 `.env` 的 `UPDATER_TAG`、`UPDATER_IMAGE_REF`、
`DOCKER_GUARD_IMAGE` 和 `guard-policy/docker-guard.env` 的 `DOCKER_GUARD_IMAGE`。
回写由实际镜像中的固定维护入口执行，不拉取镜像、不重建容器；`.env` 原位写入以保留
运行中 updater 的文件绑定挂载。两份文件不是跨文件事务，中断后再次启动核验可收敛。
正在进行的升级/回滚优先；组件混版、不健康或无法验证时不覆盖固定值，并记录原因。
只改 tag 而没有真正换镜像不会被视为已升级，配置会同步为实际运行版本。

自更新直接从组件自己的 Docker Hub 仓库选版本，不下载业务 release.json 或签名文件。
版本偏好只用于默认选择；Guard 接受合法 Docker tag，不按版本号、构建时间或分支名称拒绝请求。

1. Worker 持久化 Pending 后立即返回，在后台选版本。`queued` 表示尚未交给 Guard；
   重启后继续同一请求，查询失败写入失败结果。
2. Guard 用独立 capability 受理，固定官方仓库，通过 Docker daemon 拉取并解析精确镜像。
   更新前记录各服务的实际镜像，不要求旧服务已经健康。
3. helper 使用宿主 Compose，只检查目标服务存在且选择了本次镜像；不限制健康命令、
   挂载列表、网络列表或启动脚本的具体写法。执行 `up --pull never --no-deps --force-recreate`。
4. helper 检查替换后的实际镜像与健康状态。它不发布失败终态，也不在内部再做一套回滚。
   Guard 负责恢复；恢复使用先前 Guard 镜像中的 helper 和各服务原有镜像。
5. Guard 在执行结束后写入最终结果，暂时写入失败只重试保存，不重新替换。
   已停止 helper 的清理失败不会丢失结果或无限锁住更新。恢复失败、执行已停止时允许再次更新。

策略文件各自原子写入，不把多文件或多容器切换称作事务。Guard 重启从 Docker 中保留的
helper 配置恢复执行信息；helper 不存在时核对实际运行状态并结束遗留 Pending。
`self-update-last.json` 保存结果，`queued` 记录交接阶段；运行中的任务和 Guard 负责互斥。

这里信任官方镜像仓库及 registry 传输。digest 用于固定本次执行与恢复的内容身份，
不声称它证明了发布者签名，也不要求用户填写摘要。

### 14.4 proxy 升级

先持久化 Pending，再后台执行。Proxy 与自更新共用组件版本选择，不借用业务程序版本。
选定后拉取镜像，按固定 digest 重建并验证健康；失败按原容器镜像恢复，即使原 tag 已移动。
临时的 Compose 环境变量固定本次镜像，不向宿主配置添加永久覆盖文件。
结果保存失败自动重试保存；重启后自动继续 Pending。旧结果文件无法解析时不阻断整个状态页。


### 14.2 兜底

用户可在部署目录手动执行（宿主机管理员路径）：

```
# 修改 UPDATER_TAG 后重建；启动时自动同步实际镜像身份
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
  frontend / postgres / federation-worker / persona-worker 不得 dual-home 到 guard 网，避免在 updater 被攻破后把业务容器拉进
  未鉴权的 Docker API（`:2375`）。
- **允许的网络名**（create/connect）：业务 `myriad-net`、管理平面 `myriad-admin-net`
  （`MYRIAD_ADMIN_NETWORK`）、guard-net，以及 `MYRIAD_BACKEND_EXTRA_NETWORK`（默认 `myriad-backend-ext`，仅 backend / federation-worker / persona-worker）。内置兜底还放行面板网络 `1panel-network`（服务范围同上，1Panel 自行挂载，无需显式配置）。其它网络名拒绝。
- **更新 preflight**（不改编排，仅只读探测，失败则**不停服**）：
  1. **本地环境**：`.env` 仍含 `MYRIAD_TAG` / `PROXY_TAG` / `UPDATER_TAG`；compose 仍引用
     `${MYRIAD_TAG}`；`state/`（及 bundled 下 `state/snapshots/`）与 `.env` 可写；经
     docker-guard 的 Docker API `ping` 可达。external 模式要求 `DATABASE_URL`（不要求
     `POSTGRES_PASSWORD`）；bundled 要求 `POSTGRES_PASSWORD`。
  2. **Compose 契约**（`docker compose config`，且**更新时重扫** compose 文件）：必备服务
     `backend`/`frontend`/`federation-worker`/`persona-worker`（bundled 含 `postgres`）；
     backend 必须 `MYRIAD_PROCESS_ROLE=web`；`container_name` 须为 `myriad-backend` /
     `myriad-federation-worker` / `myriad-persona-worker` 等固定名；运行中 proxy 必须已带
     `PROXY_FEDERATION_UPSTREAM` / `PROXY_PERSONA_UPSTREAM` 与对应 capability label；
     bundled 下 postgres 的 pgdata 须为 **bind**；运行中容器 project 标签一致
     （external 不 inspect 残留 `myriad-postgres`）。
  3. **网络 allowlist**：原有三网与限定服务的 `MYRIAD_BACKEND_EXTRA_NETWORK`（默认 `myriad-backend-ext`）+ 已存在；Compose 与运行中容器均按服务校验，禁止越权挂网。
  4. **release 无 manifest**：GitHub `release.json` 不可用时 **允许** 回退 Docker Hub
     `vX.Y.Z` 镜像（开发频道 / 无私有 GitHub 常态）；该路径无 digest/cosign/min_from。
     cosign **硬失败** 仍不 fallback。
  5. **NeedsManual**：卡住时拒绝新的业务 update / auto-install（rescue/rollback 仍可用）。
  6. **健康检查**：实际镜像身份与服务健康共同通过；不以维护页或 tag 子串判定成功。
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
- 健康探测使用 Docker inspect（实际镜像和已有 healthcheck）与 Compose 内网 HTTP；不新增 Docker exec 或临时探测容器。
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

`GET /process-logs` 按需读取当前 Compose 项目的核心服务日志，仅保留 warning、error
和无法判定级别的 stderr。响应按来源限制扫描行数、保留条目与单条大小，并标明
`complete`、`truncated`、`collection_errors` 和未运行的服务；不会读取浏览器 console
或外部 MCP 部署。日志先做尽力脱敏，但仍属于仅管理员可下载的敏感诊断数据。

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
- Docker daemon 原生 registry mirror（保持镜像仓库引用不变）

### M3（可选，可能永不做）

- named volume 支持
- rootless docker
- 离线包

## 19. 当前部署基线

当前仓库只保留 proxy + updater 生产布局：

1. `pgdata` 使用 `./pgdata` bind mount
2. `.env` 包含 `MYRIAD_TAG`、`PROXY_TAG`、`UPDATER_TAG`、`COMPOSE_PROJECT_NAME`、`UPDATE_TOKEN`；生产另含 digest 钉死的 `UPDATER_IMAGE_REF` / `DOCKER_GUARD_IMAGE`。可选 `MYRIAD_DOCKER_NETWORK` / `MYRIAD_ADMIN_NETWORK` / `MYRIAD_DOCKER_GUARD_NETWORK` / `MYRIAD_BACKEND_EXTRA_NETWORK`
3. 业务镜像用 `${MYRIAD_TAG}` / `${PROXY_TAG}`；TCB 用 `UPDATER_IMAGE_REF` digest pin；不使用 `:latest`
4. 只有 `proxy` 暴露宿主端口
5. 只有 `docker-guard` 挂载原始 docker.sock；updater 仅通过内部策略代理访问 Docker API
6. updater 编排根只读；`.env` / `state` /（bundled）`pgdata` 以可写子挂载叠加
7. 网络三分：business / admin / guard；`updater-gateway` 在 admin-net 为 backend 注入 token；worker 不进 admin-net / guard-net
8. `./state` 和 `./backups` 由部署脚本创建
9. `UPDATE_TOKEN` 保存在同一个 `.env`，不暴露给浏览器，也不进入 backend / worker / docker-guard 容器 env

`scripts/extra/deploy.sh` 是当前生产布局的 bootstrap 入口。
