# 整站备份与恢复

快速开始里的 `pg_dump` 只覆盖数据库。形象 atlas、Tapp 安装、Agent 记忆和
小组件字体在 `backend_data`（容器内 `/app/data`）。密钥在部署目录的 `.env`。
这三样都要带上，才能从空机器恢复。

一次备份分成两份，分开放：

- **数据份**：数据库和 `backend_data`，但不含任何钥匙和密钥。每次一个新目录，
  留几份、放在哪里都可以。
- **钥匙份**：`.env`、数据钥匙（`backend_data/.secret-key`）和她的日志的钥匙行。
  只有一份，每次覆盖。**只保留最新的这一份。**

为什么要分开：她的日志按对象加密，「忘掉」一件事就是删掉它的钥匙。钥匙和数据放在
同一份备份里的话，拿到旧备份的人同时拿到了旧钥匙和旧数据，忘掉对这份备份就不起作用。
分开以后，数据份里没有钥匙；钥匙份只有一份，下一次覆盖时，忘掉的钥匙就从所有地方
消失了。

新部署的 `backend_data` 对应部署目录 `./data`，旧部署仍可能使用 Docker 管理卷。
恢复完成后，数据根目录收紧为 `700`、归属 `1000:1000`，避免旧归档中的根目录权限
暴露宿主数据或阻止非 root backend 访问；解包失败也会收紧根目录权限并保持写入者停止。
子树同步归属 `1000:1000`，只补足所有者的文件读写与目录遍历权限，不增加组或其他用户
权限，也不跟随符号链接修改归档目录外的目标。恢复重建使用 `--no-deps`，因此这一步
由恢复脚本完成，不依赖卷初始化服务再次运行。
备份和恢复脚本读取当前 Compose 中的实际挂载来源，两种布局均可用，归档名保持不变。
脚本需要本地 Docker、Compose v2+ 和 `jq`；缺少目录或旧卷时会拒绝操作。布局迁移、停机打包和换机步骤见 [DATA_LAYOUT.md](DATA_LAYOUT.md)。

`cache` / `backend_cache` 可再生，不要当灾备。媒体资产在 `backend_data/media`，
会进 `backend_data.tar.gz`。内容里引用的都是 `/media/assets/…` 永久地址，
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
# 钥匙份默认在部署目录的 backup-keys/，可以改：
BACKUP_KEYS_DIR=/srv/myriad-keys bash scripts/extra/backup.sh backup
```

数据份目录（权限 700）里有：

| 文件 | 内容 |
| --- | --- |
| `postgres.dump` | `pg_dump -Fc`，`agent_log_audience_keys`、`agent_log_object_keys` 只有表结构、没有行 |
| `backend_data.tar.gz` | Compose 的 `/app/data` 来源（`./data` 或旧 named volume；phantasi / tapps / agent / site），不含 `.secret-key` |
| `MANIFEST.txt` | `format=2`、时间戳、项目、数据钥匙的指纹（用来和钥匙份对上，推不出钥匙本身），不含秘密值 |

钥匙份目录（`backup-keys/`，权限 700，文件 600）里有：

| 文件 | 内容 |
| --- | --- |
| `env` | 当时的 `.env`（含 JWT、数据库口令等） |
| `data-key` | 数据钥匙。它封着平台配置里的密钥、联邦私钥和她的日志的受众钥匙，**永远不变** |
| `agent_log_audience_keys.copy`、`agent_log_object_keys.copy` | 她的日志的钥匙行（本身也是封好的） |
| `MANIFEST.txt` | 时间戳、项目、数据钥匙从哪里来、指纹、行数 |

钥匙份的位置不能在数据份里面，脚本会拒绝。写钥匙份时先写进旁边的临时目录，写完再换掉
旧的，中途失败时旧的那份还完整；之前中断留下的旧副本会一起删掉。脚本不会把任何钥匙或
`.env` 的值打进输出。

### 钥匙份怎么保管

- **只保留最新的一份。** 同步到别处时用不留历史的方式（例如 `rsync --delete` 到一个
  固定位置，或者不开版本的存储桶）。不要把 `backup-keys/` 放进 `backups/`、带版本历史
  的网盘或整盘快照里：旧的钥匙份还在，忘掉对它就不成立。这一点脚本管不到。
- **数据钥匙另外抄一份到密码管理器。** 它永远不变，所以多抄几份没有问题；钥匙份丢了
  而数据钥匙还在的话，至少配置密钥和联邦身份还能恢复。钥匙份和数据钥匙都丢了，配置
  密钥、ActivityPub 身份和她的日志正文都恢复不了。
- **刷新钥匙份是可选的。** 每次 `backup` 都会顺带重写钥匙份，什么都不做也能照常备份和
  恢复。另外刷新只影响一件事：忘掉一件事以后，它的钥匙会在钥匙份里留到下一次重写为止，
  刷新得勤，忘掉就更快传到备份里。只刷新钥匙份不用停服务，几秒就完：

  ```bash
  bash scripts/extra/backup.sh keys
  ```

  想自动刷新的话，cron 示例（部署目录换成你的）：

  ```text
  0 * * * * cd /opt/myriad && bash scripts/extra/backup.sh keys >/dev/null
  ```

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

顺序：核对钥匙份、口令和项目 → 检查数据卷及真实目录、探测挂载与写入 → 停写（worker + backend）→
用钥匙份的 `env` 覆盖 `.env` → 恢复 Postgres → 导入日志钥匙行 → 恢复数据卷 → 写回数据钥匙 →
`compose up -d --force-recreate --no-deps backend` → 容器内探测 `/ready` → 同样重建两个 worker。
中途失败会保持已停的写进程停止。
不要用前端 HTML 的 200 认定就绪。

预检不创建目录或卷。存储来源缺失、目录为符号链接、bind 指向其他目录、Docker
无法挂载或写入时，拒绝发生在
停服务、覆盖 `.env` 和数据库恢复之前。预检只证明此时可用，恢复期间仍须避免管理员
并发移动目录或更改卷；它不提供整站恢复的事务原子性。

`python3 scripts/extra/test-backend-volumes.py` 使用独立临时 Docker 项目演练真实
PostgreSQL 表和媒体文件的备份/恢复，并检查预检拒绝时库、`.env` 和写进程保持原状；
它还检查数据份里没有钥匙和密钥、哪里都找不到钥匙份时拒绝恢复、不加 `--keys` 时从默认
位置找到钥匙份、忘掉并刷新钥匙份以后，用旧数据份恢复出来那一条读不出。
业务进程由测试服务替代，账户、Tapp、形象文件与凭据的端到端可用性仍为 uncertain。

```bash
bash scripts/extra/backup.sh restore --from /var/backups/myriad-20260101
```

不加 `--keys` 时，脚本去备份时放钥匙份的地方找（`BACKUP_KEYS_DIR`，默认部署目录的
`backup-keys/`），找到了就和拆分以前一样一条命令完成。钥匙份放在别处（比如换机器时从
别处拷回来）就用 `--keys` 指给它：

```bash
bash scripts/extra/backup.sh restore --from /var/backups/myriad-20260101 --keys /path/to/backup-keys
```

哪里都没有钥匙份、两份的项目或数据钥匙指纹对不上，都会在停服务之前拒绝，并说明要提供
钥匙份。没有「不要钥匙也恢复」的选项：后端读不到数据钥匙时会自己生成一把新的，之前
封好的东西就再也打不开了。

钥匙份比数据份新是正常的（每次备份和刷新都会重写它）：数据份之后新建的账户的钥匙行导入时会跳过；
数据份之后被忘掉的东西，钥匙份里已经没有它的钥匙，恢复出来也读不出。钥匙份比数据份
旧时脚本会警告：比钥匙份新的日志正文打不开。

拆分之前做的旧备份（目录里有 `env`、`MANIFEST.txt` 里没有 `format=2`）照旧恢复，
不要加 `--keys`。它们自带数据钥匙和日志钥匙，忘掉对它们不成立；有了新格式的备份以后，
要不要删掉旧备份由你决定。

业务字段看 `db_connected`（最近一次探测）、`migrations_applied`、
`routes_full`、`storage_writable`。

## 忘掉对备份做到了哪一步

如实说现状：

- **她的日志正文**（`agent_log_payloads`）：按对象加密。只要钥匙份只留最新一份，被忘掉的
  那一条在钥匙份下一次重写（下一次备份或刷新）以后，任何数据份里都读不出。
- **不经这个脚本的数据库副本**（升级器的 `./pgdata` 快照、手动 `pg_dump`、只读副本、
  云主机的整盘快照）：里面有日志的钥匙行，数据钥匙又永远不变，所以拿到这样的副本再加上
  数据钥匙，仍然读得出已经忘掉的日志正文。这一点没有修，只能等这些副本自己过期或被删掉。
- **她的记忆、节点、话题和聊天记录**：在数据库里是明文，每一份数据份和上面那些副本里都有。
  忘掉删得掉线上库里的，删不掉已经做好的副本里的，只能等你按自己的轮换删掉旧的。

## 不要做的事

- 只备份 SQL、不备份 `backend_data`：Tapp 与形象文件会丢。
- 把 Updater 的 `./pgdata` 快照当整站备份。
- 把 `backend_cache` 打进灾备包。
- 用 `rm -rf` 清 `federation_media` 或 image-cache 来「完成迁移」。
- 在聊天、工单或日志里粘贴 `.env`、`data-key` 或钥匙份里的任何文件。
- 把 `backup-keys/` 放进 `backups/`、带版本历史的网盘或整盘快照，或者把钥匙份和数据份
  打进同一个压缩包。
