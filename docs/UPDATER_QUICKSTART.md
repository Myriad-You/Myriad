# Updater Quickstart

如何在自托管 Myriad 实例上启用 updater，并执行第一次升级。
完整设计参考 [docs/updater-spec.md](./updater-spec.md)。
安全基线（已完成态 + 运维红线）：[deployment/UPDATER_SECURITY_BASELINE.md](./deployment/UPDATER_SECURITY_BASELINE.md)。

> 运行模型：updater 不是 A/B 双活分区。Myriad 生产环境只有一套正在运行的
> backend/frontend（以及可选的 compose 内 postgres）；更新时进入维护模式，停止业务容器，
> 在 **bundled** 模式下快照 `pgdata`，切换 `.env` 里的镜像 tag，再启动新版本。
> 回滚在 bundled 下依赖快照 + 旧 tag；**external** 下只恢复镜像 tag（绝不写外部库数据目录）。

## 0. 准备

- Docker Engine 20.10+ 且支持 `docker compose` v2 子命令
- 单机部署（updater 当前只支持 single-node）
- **本地库 / bundled（默认）**：pgdata 在宿主文件系统的目录（不能是 docker named volume）
- **外部库 / external**：见下方 [外部 PostgreSQL](#外部-postgresqlmyriad_db_modeexternal)；不要求 `./pgdata`

## 1. 初始化当前生产布局

```bash
cd /path/to/myriad
bash scripts/docker/deploy.sh up
```

脚本做的事：

- 如缺少 `.env`，从 `.env.production.example` 复制
- 创建 `./pgdata`、`./state`、`./backups`
- 补齐 `MYRIAD_TAG`、`PROXY_TAG`、`UPDATER_TAG`、`COMPOSE_PROJECT_NAME=myriad` 等当前布局 key
- 若 `UPDATE_TOKEN` 为空则随机生成
- Docker 网络默认显式命名为 `myriad-net`；同机多套部署时可设置 `MYRIAD_DOCKER_NETWORK`

旧的 direct-port / named-volume 迁移脚本已移除。当前仓库只保留 proxy + updater 生产布局。

## 2. 确认服务拓扑

`deploy.sh up` 会完成 bootstrap 并启动 compose stack。此时栈的拓扑：

```
proxy (80) ─┬─► frontend
            └─► backend ─► postgres
updater (内网) ─► docker-guard ─► docker.sock
       └──────── 单一部署根挂载（pgdata + state + .env）
```

只有 `proxy` 暴露宿主端口。`HTTP_PORT` 可以在 `.env` 调（默认 80）。原始 Docker socket
只挂载给 `docker-guard`；updater 通过内部网络访问经项目/镜像/请求体白名单限制的 API。
updater 只挂载宿主部署根目录一次，宿主上的 `./pgdata`、`./state` 路径和救援命令不变。

从「updater 直接挂 sock」旧布局迁到当前拓扑：见
[deployment/MIGRATION_DOCKER_GUARD.md](./deployment/MIGRATION_DOCKER_GUARD.md)
（同目录 `compose pull && up -d`，保留 pgdata/.env；**仅 UI 无法切换拓扑**）。

自更新会同时重建 `docker-guard` 与 `updater`（共用 `UPDATER_TAG`）。该次 compose 走
宿主 unix socket 的固定 argv 路径，日常 Docker API 仍经 guard 策略代理。

## 3. 打开 updater UI

浏览器访问你的 Myriad 站点，登录管理员账户后进入：

```text
/config -> 关于 -> 更新管理
```

需要管理员登录。**默认走 backend 通道**：backend 持有 `UPDATE_TOKEN`，
浏览器只携带 admin session cookie，整个流程不需要手动输入 token。

如果 backend 本身挂了（极端情况），可以在 UI 的"高级：直连模式"切到 `direct` 并手输 `UPDATE_TOKEN`。前提是宿主机管理员显式启用了直连通道：

```bash
# 在 .env 里加这一行，然后重启 proxy/stack
PROXY_ALLOW_DIRECT_UPDATER=true
bash scripts/docker/deploy.sh restart
```

默认 `PROXY_ALLOW_DIRECT_UPDATER=false`，`/_updater/*` 返回 404，强制走 backend。

UI 提供：

- 当前 updater/business 版本、channel、维护状态
- **更新模式**：`release`（semver `vX.Y.Z`）或 `commit`（CI 的 `dev-<sha>` / 分支 tip）
- **频道**：`stable` / `preview`；**commit 模式仅在 `preview` 频道下可选**
- 检查可用更新；commit 模式可填具体 sha
- 触发升级（带确认 + 进度轮询）
- 列出快照、一键回滚
- 强制退出维护模式

**Release 安装与 GitHub / Docker Hub**

1. Preflight **优先**从 GitHub 拉该 tag 的 `release.json`（镜像 digest、cosign、`min_from_version`）。
2. 若 GitHub 不可用（无私有 Release、`GITHUB_TOKEN` 未设、404/401、网络错误、缺少 `release.json` asset），**回退**到 Docker Hub：用 `.env` 里的 `BACKEND_IMAGE` / `FRONTEND_IMAGE` 仓库 + 目标 tag `vX.Y.Z` 拉取前后端镜像；两边都必须成功，否则明确失败（不会静默安装）。
3. 无 `release.json` 时跳过 cosign 与 manifest digest 对账；`PreflightReport.manifest` 为 `None`，后续 swap 仍只依赖已拉取的 tag/digest。
4. 因此：**只要 Docker Hub 上有公开的 `vX.Y.Z` 业务镜像，即使没有 GitHub Release 也能完成正式版安装**。

```bash
# 切换到 commit 模式并跟踪 preview 分支 tip
curl -s -b "$COOKIE_JAR" -X POST http://localhost/api/admin/updater/prefs \
  -H "Content-Type: application/json" \
  -d '{"channel":"preview","mode":"commit"}'

# 升级到指定 commit（镜像 tag = dev-<shortsha>）
curl -s -b "$COOKIE_JAR" -X POST http://localhost/api/admin/updater/update \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: commit-$(date +%s)" \
  -d '{"target_commit":"abc1234","mode":"commit"}'
```

### Commit 新旧如何判断（不是比时间）

检查更新时会：

1. 把**当前** `MYRIAD_TAG` 解析成 git ref  
   - `v0.1.0` → tag `v0.1.0`  
   - `dev-abc1234` → sha `abc1234`
2. 把**目标频道**解析成分支 tip（如 `preview` 的 HEAD）
3. 调 GitHub `compare(当前...目标)`，用 **祖先关系**：
   - `ahead`：目标比当前新（可升级）
   - `behind`：目标比当前旧
   - `identical`：同一 commit
   - `diverged`：分叉（目标有你没有的 commit，也可能反之）

因此从 `v0.1.0` 切到 `preview` tip 时，只要 tag 在仓库里能解析到 commit，就能正确判断 tip 是否更新。

### 降级与风险确认

| 情况 | 需要的 API 字段 |
|------|-----------------|
| 目标比当前**旧** | `allow_downgrade: true` |
| 历史**分叉** | `allow_diverged: true` 或伞形 `allow_risk: true` |
| 方向**未知** / compare 失败强行 | `allow_unknown: true` 或 `allow_risk` |
| **不可逆迁移** + 降级 | `allow_downgrade` + (`allow_irreversible` 或 `allow_risk`) |
| 伞形 | `allow_risk: true` 启用上述全部（含降级） |

同版本 release / 同一 git commit 会 **直接拒绝**（无需「重装同 tag」）。

```bash
# 最近 commits / 发行版列表（供选择）
curl -s -b "$COOKIE_JAR" \
  'http://localhost/api/admin/updater/commits?branch=preview&limit=20' | jq
curl -s -b "$COOKIE_JAR" \
  'http://localhost/api/admin/updater/releases?channel=stable&limit=20' | jq

# 即时 compare（相对当前部署）
curl -s -b "$COOKIE_JAR" \
  'http://localhost/api/admin/updater/compare?to=dev-abc1234' | jq

curl -s -b "$COOKIE_JAR" -X POST http://localhost/api/admin/updater/update \
  -H "Content-Type: application/json" \
  -d '{"target_version":"v0.1.0","mode":"release","allow_downgrade":true,"allow_irreversible":true}'
```

触发更新时 `history.log` 与 `state/audit.log` 都会写结构化审计行：

```text
audit: update_request job=… target=… mode=… allow_downgrade=… allow_diverged=… …
```

`audit.log` 还会记录 rollback / self-update / rescue / job 终态（见
[updater-spec.md §16.1](./updater-spec.md)）。

Commit 模式成功后 **只写入 `dev-<shortsha>`** 到 `MYRIAD_TAG`。  
业务更新只换 **backend/frontend**；proxy / updater 本体仍按独立节奏（updater 自更新走 release channel：main→stable，preview→nightly，beta→beta）。

`.env` 必须包含 `BACKEND_IMAGE` / `FRONTEND_IMAGE`。

## 4. 触发一次升级（命令行）

### 4.1 通过 backend（推荐）

先用 admin 账号登录拿 cookie：

```bash
COOKIE_JAR=/tmp/myriad.cookies
curl -s -c "$COOKIE_JAR" -X POST http://localhost/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"admin","password":"YOUR_ADMIN_PASSWORD"}'

# 然后用 cookie 调 admin updater 路由
curl -s -b "$COOKIE_JAR" http://localhost/api/admin/updater/status | jq

curl -s -b "$COOKIE_JAR" -X POST http://localhost/api/admin/updater/update \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: update-$(date +%s)" \
  -d '{"target_version":"v0.2.0"}'
```

无需 `X-Update-Token` —— 由 backend 注入。

### 4.2 通过 direct（rescue/backend down）

需要 proxy 启用了 `PROXY_ALLOW_DIRECT_UPDATER=true`：

```bash
UPDATE_TOKEN='从部署目录 .env 读取的 UPDATE_TOKEN'

# 1. 查 status
curl -s http://localhost/_updater/status \
  -H "X-Update-Token: $UPDATE_TOKEN" | jq

# 2. 触发更新
curl -s -X POST http://localhost/_updater/update \
  -H "Content-Type: application/json" \
  -H "X-Update-Token: $UPDATE_TOKEN" \
  -H "Idempotency-Key: update-$(date +%s)" \
  -d '{"target_version":"v0.2.0"}'

# 3. 跟踪 job
JID=...
watch -n2 "curl -s -H 'X-Update-Token: $UPDATE_TOKEN' http://localhost/_updater/jobs/$JID | jq '.status, .steps[-1]'"
```

升级流程（spec §7）：

```
preflight → maintenance_on → stopping → snapshotting → swap_tag
  → starting_new → health_probing → swapping_proxy → finalize
```

任何步骤失败都会自动回滚到 snapshot，并恢复 **上一正常业务版本** 的 `MYRIAD_TAG`
（优先用 swap 前捕获的 tag，其次快照 `source_version`，再次 `updater.json.current_version`，
不是写死某个固定版本）。回滚再失败进入 `needs_manual`，proxy 维护页会展示恢复命令。

UI / API 手动回滚到某个快照时，同样会按快照的 `source_version` 写回 `MYRIAD_TAG`，
而不是只恢复数据库。

## 5. 出问题怎么办

### 5.1 proxy 维护页一直挂着

下面的 `docker exec` 是宿主机管理员的救援操作；updater 经 guard 调 Docker API 时不能使用
container exec。

```bash
# 看 updater 状态
docker exec myriad-updater myriad-rescue status

# 想强制退出（确认服务确实正常时）
touch ./state/manual-override
docker exec myriad-updater myriad-rescue exit-maintenance --force
```

`manual-override` 是宿主文件级 flag，proxy 一旦读到不存在的话所有 rescue 端点都返回 403。这阻止远端通过 API 单独触发 rescue。

### 5.2 升级后服务起不来 / needs_manual

**优先（管理 UI）**：配置 → 关于 → 更新管理 → 维护操作 →「一键回退到升级前版本」。
这会调用 `POST /rescue/continue`，对失败任务关联的快照恢复 **pgdata + MYRIAD_TAG**。

**命令行**：

```bash
# 看可用快照
docker exec myriad-updater myriad-rescue status | jq '.snapshots.items'

# 选一个回滚（同样会写回 MYRIAD_TAG）
docker exec myriad-updater myriad-rescue rollback --snapshot snap-<job-id>
```

启动新构建前，updater 会把升级前的 backend/frontend 镜像额外钉上本地回退 tag
`*:myriad-rollback`。成功升级后该 tag 仍指向上一个已验证构建；到下一次升级开始前，
才会推进到届时正在运行的构建。回滚时如果原版本 tag 已不在本地，updater 会从这个
本地槽位重新创建原版本 tag，再交给 Compose 启动。

### 5.3 把诊断包给开发者

```bash
docker exec myriad-updater myriad-rescue diagnose --output /myriad-diagnostics.tar.gz
docker cp myriad-updater:/myriad-diagnostics.tar.gz .
```

里面包含 state 文件、env-probe、`docker info`、`docker images` 列表、`history.log` 末 100 行。

## 6. 发版工程（仓库维护者侧）

打 tag 触发 `release.yml`：

```bash
git tag v0.2.0
git push origin v0.2.0
```

流程：

1. 4 个组件镜像 (backend/frontend/proxy/updater) build & push 到 docker.io
2. 自动生成 `release.json`，包含精确 `commit_sha` 以及每个镜像的 immutable tag + digest
3. 发布 GitHub Release，附 `release.json` + `SHA256SUMS`

tag 命名约定：

| 形态 | channel |
|---|---|
| `v0.2.0` | stable |
| `v0.2.0-beta.1` | beta |
| `v0.2.0-nightly.20260516` | nightly |

每个 release 必填 `min_from_version`（避免跨版本直升）和 `min_updater_version`（强制先升 updater）。如需覆盖默认值，在 `release/overrides/<version>.json` 留一份补丁，CI 会 deep-merge 进 `release.json`。

## 7. 启用 cosign 验证（推荐生产环境）

releases 自 v0.1.0 起由 GitHub Actions OIDC keyless 签名（`release.json.sig` +
`release.json.pem` 作为 release assets）。updater 镜像里已经预装 cosign CLI，只需把策略
切到 `strict`：

```bash
# .env
COSIGN_VERIFY=strict
```

随后重启 updater/stack：

```bash
bash scripts/docker/deploy.sh restart
```

从此每次拉 release.json 时：

- 缺失 `.sig`/`.pem` → 拒绝升级（"strict"）
- 签名校验失败 → 拒绝升级（"strict"）
- 校验通过 → 正常进入 preflight

`COSIGN_VERIFY=soft` 是过渡选项：失败仅 warning，不阻止升级。推荐先 soft 跑几个版本观察
日志，确认无误后切 strict。

要完全关闭验签（不推荐）必须双钥匙：

```bash
COSIGN_VERIFY=off
UPDATER_ALLOW_INSECURE_COSIGN=true   # 或 COSIGN_INSECURE_OK=true
```

仅设置 `COSIGN_VERIFY=off` 时 updater 会拒绝启动。

## 8. 关键约束（再次强调）

- **永远不要推 `:latest`**：updater 的回滚依赖旧版本 tag 仍在 registry。
- **本地库：pgdata 必须是 bind mount**：M1 不支持 docker named volume 上的快照。
- **首次部署 pgdata 尚不存在时 updater 仍可启动**（env-probe 记 warning）；完整更新快照会在路径就绪后才能执行。
- **外部库**：`MYRIAD_DB_MODE=external` 时跳过 pgdata 快照/恢复；库备份由运维自管。
- **修改 `.env`**：用户可以随便加自己的 key，updater 只触碰 `MYRIAD_TAG`/`PROXY_TAG`/`UPDATER_TAG` + release 声明的 `env.new`。
- **每次更新的 token 验证**：5 次/分钟错误后封 10 分钟。

更多 corner case 见 [updater-spec.md §16-17](./updater-spec.md)。

## 外部 PostgreSQL（`MYRIAD_DB_MODE=external`）

默认生产栈把 Postgres 放在 compose 内，并用 `./pgdata` bind 做升级前快照与失败回滚。
若数据库在 **栈外**（1Panel 应用商店、云 RDS、宿主机 Postgres、独立 DB 容器）：

| 项 | 行为 |
| --- | --- |
| `MYRIAD_DB_MODE=external` | updater **跳过** pgdata 快照与恢复（日志：`db_mode=external; skipping pgdata snapshot`） |
| `DATABASE_URL` | backend 连接的 **唯一真相源**（写完整连接串）；**不会**据此静默改 `MYRIAD_DB_MODE` |
| 栈内 `postgres` / `./pgdata` | **不要** 再部署；勿留空 `./pgdata` 装样子；更新**不要求**路径存在 |
| 镜像 tag / 维护模式 | updater **仍管理**；回滚/rescue **只恢复镜像 tag**，从不写 pgdata |
| `/status` · env-probe | `db_mode=external`、`pgdata_snapshot_enabled=false` |
| 库备份 / 库级回滚 | **运维自管**（`pg_dump`、云快照、面板备份） |

```bash
# .env 关键
MYRIAD_DB_MODE=external
DATABASE_URL=postgres://user:pass@db-host:5432/myriad?sslmode=prefer
```

未设置 `MYRIAD_DB_MODE` 时默认为 `bundled`（兼容现有部署）。完整契约见
[updater-spec.md §9.1.1](./updater-spec.md)。

Compose 形态与连通性（`host.docker.internal`、sslmode、容器内核对 `DATABASE_URL`）见：

- [deployment/EXTERNAL_POSTGRES.md](./deployment/EXTERNAL_POSTGRES.md)
- [deployment/examples/docker-compose.external-db.example.yml](./deployment/examples/docker-compose.external-db.example.yml)
