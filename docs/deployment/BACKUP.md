# 整站备份与恢复

快速开始里的 `pg_dump` 只覆盖数据库。形象 atlas、Tapp 安装、Agent 记忆和
小组件字体在 `backend_data`（容器内 `/app/data`）。密钥在部署目录的 `.env`。
这三样都要带上，才能从空机器恢复。

新部署的 `backend_data` 对应部署目录 `./data`，旧部署仍可能使用 Docker 管理卷。
备份和恢复脚本读取当前 Compose 中的实际挂载来源，两种布局均可用，归档名保持不变。
脚本需要本地 Docker、Compose v2+ 和 `jq`；缺少目录或旧卷时会拒绝操作。布局迁移、停机打包和换机步骤见 [DATA_LAYOUT.md](DATA_LAYOUT.md)。

`cache` / `backend_cache` 可再生，不要当灾备。媒体资产在 `backend_data/media`，
会进 `backend_data.tar.gz`。旧 `/media/federation` 与 image-cache 地址靠别名读，
不依赖缓存卷。细节见 [MEDIA.md](MEDIA.md)。Updater 对 `./pgdata` 的文件级
快照只服务升级回滚，**不是**整站备份。

## 备份

允许短暂停写。官方拓扑里 `federation-worker` / `persona-worker` 与 web
共用 `backend_data` 并各自写库；`backup.sh` 会停/起这三个服务（先停 worker，再停
web）。`--no-stop` 只保证崩溃一致，不保证跨进程一致。

```bash
bash scripts/extra/backup.sh backup
# 或
bash scripts/extra/backup.sh backup --out /var/backups/myriad-20260101
```

产物目录（权限 700）里有：

| 文件 | 内容 |
| --- | --- |
| `postgres.dump` | `pg_dump -Fc` |
| `backend_data.tar.gz` | Compose 的 `/app/data` 来源（`./data` 或旧 named volume；phantasi / tapps / agent / site） |
| `env` | 当时的 `.env`（含 JWT、数据库口令等） |
| `MANIFEST.txt` | 时间戳与包含清单，不含秘密值 |

脚本不会把 `.env` 打进日志。备份目录应与主机权限一并保管。

原生部署没有 Docker volume 时，用
[NATIVE_DEPLOYMENT.md](NATIVE_DEPLOYMENT.md) 的 `pg_dump` + `tar` `data/`。

备份失败时，若脚本确实停过正在运行的写进程，EXIT 会按 backend → worker
的顺序拉起来，并保留原错误码。`compose stop` 失败会中止，不会假装“本来就没在跑”。

## 恢复

新机器先按 [部署文档](DOCKER_DEPLOYMENT.md) 运行 `deploy.sh up`，准备 data/cache
目录并启动 PostgreSQL，再执行恢复。旧 Compose 布局必须先准备对应 named volumes。

**支持范围：** 同一 compose 项目、运行中的 Postgres 角色口令与备份 `.env`
的 `POSTGRES_PASSWORD` 相同。官方 compose 用该值初始化角色
（`postgres://myriad:${POSTGRES_PASSWORD}@postgres:5432/myriad`）。

**不支持：** 目标集群角色口令与备份不同。脚本在停写、覆盖 `.env`、
`pg_restore` 之前拒绝，不会改口令、也不会重做 Postgres 初始化。要对齐口令，
须先按部署文档把集群收成备份那套口令，再跑恢复。

会覆盖当前库、数据卷和 `.env`。现有 `.env` 会先复制为 `.env.bak.restore`。

顺序：核对口令和项目 → 检查数据卷及真实目录、探测挂载与写入 → 停写（worker + backend）→
覆盖 `.env` → 恢复 Postgres → 恢复数据卷 → `compose up -d --force-recreate --no-deps backend`
→ 容器内探测 `/ready` → 同样重建两个 worker。中途失败会保持已停的写进程停止。
不要用前端 HTML 的 200 认定就绪。

预检不创建目录或卷。存储来源缺失、目录为符号链接、bind 指向其他目录、Docker
无法挂载或写入时，拒绝发生在
停服务、覆盖 `.env` 和数据库恢复之前。预检只证明此时可用，恢复期间仍须避免管理员
并发移动目录或更改卷；它不提供整站恢复的事务原子性。

`python3 scripts/extra/test-backend-volumes.py` 使用独立临时 Docker 项目演练真实
PostgreSQL 表和媒体文件的备份/恢复，并检查预检拒绝时库、`.env` 和写进程保持原状。
业务进程由测试服务替代，账户、Tapp、形象文件与凭据的端到端可用性仍为 uncertain。

```bash
bash scripts/extra/backup.sh restore --from /var/backups/myriad-20260101
```

业务字段看 `db_connected`（最近一次探测）、`migrations_applied`、
`routes_full`、`storage_writable`。

## 不要做的事

- 只备份 SQL、不备份 `backend_data`：Tapp 与形象文件会丢。
- 把 Updater 的 `./pgdata` 快照当整站备份。
- 把 `backend_cache` 打进灾备包。
- 用 `rm -rf` 清 `federation_media` 或 image-cache 来「完成迁移」。
- 在聊天、工单或日志里粘贴 `.env`。
