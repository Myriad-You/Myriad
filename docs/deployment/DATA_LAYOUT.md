# 部署目录与数据卷迁移

新部署运行 `bash scripts/extra/deploy.sh up` 后，`data/` 和 `cache/` 与
`docker-compose.yml` 同级。容器内仍是 `/app/data` 和 `/app/cache`，卷名仍为
`${COMPOSE_PROJECT_NAME}_backend_data` / `${COMPOSE_PROJECT_NAME}_backend_cache`。
`data/` 包含媒体、Tapp、人设资产及运行时密钥，须像 `.env` 一样保护。

宿主脚本创建 `local` 卷，选项为 `type=none,o=bind,device=<部署目录绝对路径>/data`
（缓存对应 `/cache`）。目录先以 700 创建，再由原有 uid 1000 权限修复与写探测处理。
[Docker 要求 device 是已存在的绝对宿主路径](https://docs.docker.com/reference/compose-file/volumes/)。
脚本须在本地 Docker 环境运行；远程 Docker context 不适用。Docker Desktop / WSL
的路径共享由 Docker 管理；本次存储演练已在 macOS Docker Desktop（Engine 29.7.2、
Compose 5.5.1）通过，其他 Desktop / WSL 组合仍为 uncertain。不要用 Windows Git Bash
路径直接替代 Linux daemon 路径。

Compose 把卷声明为 `external`，由宿主管理其创建和删除。这样 updater 只复用卷，
不需要放开 Guard 的卷写 API 或 backend 的宿主 bind 禁令。不要在服务挂载上设置
`VolumeOptions.DriverConfig`。旧部署卷照常使用，脚本会明确提示它们尚不在部署目录；
不会自动搬迁、删除或覆盖旧卷。历史发布版测试夹具保留原布局以验证兼容性。

宿主 `data/`、`cache/` 必须是真实目录，不能是符号链接（包括悬空链接）。绑定目录丢失
时拒绝启动，不会悄悄创建空目录。目录检查在 root 权限修复之前执行；部署根目录及卷
仍须由可信管理员控制，部署/恢复期间不要并发移动目录或改卷。

## 不使用 deploy.sh 的安装

在部署根目录、填写好 `.env` 后，先在宿主准备卷，再运行 Compose。
`project` 必须与 `.env` 的 `COMPOSE_PROJECT_NAME`、Guard 项目名相同：

```bash
set -e
project=myriad
source scripts/extra/backend-volumes.sh
ensure_backend_volume docker "${project}_backend_data" "$(pwd -P)/data"
ensure_backend_volume docker "${project}_backend_cache" "$(pwd -P)/cache"
docker compose up -d
```

不要用 `-p` 临时改项目名而不修改 `.env`。初次部署仍需按部署文档补齐密钥和
Guard 策略，推荐使用 `deploy.sh`。

## 已有 Docker 管理卷迁移

这是停机迁移，不承诺几十秒窗口。先做 [整站备份](BACKUP.md)，确认磁盘能同时容纳
旧卷、归档和新目录。以下针对官方本地 Docker 单机布局；外置数据库另行备份。
全程保持相同项目名，暂停外部自动拉起/更新任务。不要运行 `down -v` 或 volume prune。

1. **用原 Compose 文件**运行 `docker compose down`，停止包括两个 worker、updater
   和 PostgreSQL 在内的整栈并移除容器，释放卷。不要在业务仍写入时复制文件。
2. 保存原 Compose / 脚本，换成新版文件。在部署根目录执行以下 Bash 块；按实际值
   修改 `project`。`data/`、`cache/` 必须尚不存在，避免把新旧内容混合。

```bash
set -e
project=myriad
root="$(pwd -P)"
saved="$root/backups/volume-layout-$(date +%Y%m%d-%H%M%S)"
umask 077
mkdir -p "$saved"
test ! -e "$root/data"
test ! -e "$root/cache"
for kind in data cache; do
    volume="${project}_backend_${kind}"
    # inspect 失败即停，不能让 docker run 隐式创建空源卷。
    docker volume inspect "$volume" >/dev/null
    docker run --rm --mount "type=volume,src=$volume,dst=/old,readonly" \
        --mount "type=bind,src=$saved,dst=/out" alpine:3.20 \
        tar czf "/out/$kind.tar.gz" -C /old .
    docker run --rm --mount "type=bind,src=$saved,dst=/in,readonly" \
        alpine:3.20 tar tzf "/in/$kind.tar.gz" >/dev/null
    mkdir "$root/$kind"
    docker run --rm --mount "type=bind,src=$saved,dst=/in,readonly" \
        --mount "type=bind,src=$root/$kind,dst=/new" alpine:3.20 \
        tar xzpf "/in/$kind.tar.gz" -C /new
done
printf 'Retained old volume archives: %s\n' "$saved"
```

3. 检查归档和新目录里的媒体、Tapp 与人设文件完整。两个卷均成功归档、解包后，
   才删除旧卷注册及其 Docker 管理的数据；上一步归档保留作回滚副本：

```bash
docker volume rm "${project}_backend_data" "${project}_backend_cache"
bash scripts/extra/deploy.sh up
docker volume inspect "${project}_backend_data" "${project}_backend_cache" \
    --format '{{.Name}} {{json .Options}}'
```

4. 确认 device 指向当前部署目录；检查 backend `/ready`、两个 worker、媒体读取、
   上传、Tapp 和人设资产。之后做一次正常升级/回退验证，保留归档直到确认完成。
   updater 的快照范围仍只有 `pgdata`，没有新增媒体快照。

中途失败就保持服务停止，先核查归档与目录，勿从头重复执行覆盖。若只是卷注册错误，
移除引用它的容器后重建卷注册即可，bind 卷删除不会删除 `data/`、`cache/` 中的文件。

## 回滚与换机

要回到 Docker 管理卷，先 `docker compose down`。若切换后已有新写入，按上面的
归档命令从**当前卷**再导出 data/cache，使用这一份恢复；旧归档会丢失切换后的写入。
归档验证后删除两个 bind 卷注册，`docker volume create <原卷名>` 创建普通卷，
再用 `docker run --rm -v <原卷名>:/new -v <归档目录>:/in:ro alpine:3.20
tar xzpf /in/data.tar.gz -C /new` 恢复数据卷（缓存同理，改为 cache）。运行
`deploy.sh up` 会保留普通卷并提示旧布局。保留宿主 data/cache 直到回滚验证完成。

整目录换机必须先完成旧卷迁移并验证两个 device。然后 `docker compose down`，
在停机状态下以保留数字 uid/gid、权限和隐藏文件的方式复制部署目录，包括 `.env`、
`guard-policy/`、`pgdata/`、`data/`、`state/`；`cache/` 可不复制。不要在线直接
打包正在写入的 PostgreSQL。跨 PostgreSQL 主版本或 CPU 架构用 BACKUP.md 的逻辑备份，
外置数据库不在此目录中。

目标机运行 `bash scripts/extra/deploy.sh up`，它会按**目标绝对路径**重新创建卷；
不需要修改 `.env` 中的路径。首次启动前必须准备卷，单独 `docker compose up -d`
不会创建 external 卷。在同一 daemon 上换目录时，先停止原栈并删除旧 bind 卷注册，
再在新目录启动；脚本会拒绝复用指向旧目录的 bind 卷。

## 回归演练

运行 `python3 scripts/extra/test-backend-volumes.py`，需要 Docker daemon 和 Compose。
脚本只操作随机命名的临时项目及卷，结束时清理；使用正式 Compose 的挂载声明，验证
新卷、旧卷迁移、worker 子目录读写边界、容器重建、PostgreSQL/媒体恢复和保留新写入的
回滚。业务服务由轻量测试服务替代，因此不代表完整 updater 发布升级或业务功能验证。
