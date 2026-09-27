# 部署目录与数据迁移

新部署直接将 `docker-compose.yml` 同级的 `./data`、`./cache` 挂载到容器的
`/app/data`、`/app/cache`，不再注册 backend named volumes。`data/` 包含媒体、
Tapp、人设资产及运行时密钥，须像 `.env` 一样保护。`cache/` 可以重建。

`deploy.sh up` 先以 700 创建目录，再修复 uid 1000 的所有权和 owner 权限并探测写入。
Compose 的 `create_host_path: false` 禁止 Docker 自动补空目录；单独运行 Compose
前必须准备目录。两个 worker 复用相同数据；federation-worker 的 `/app/data` 根挂载
只读，只有 federation、federation_media、media 和缓存 images 四个子目录可写。

Guard 只允许这些固定的宿主来源、容器目标和读写模式，拒绝符号链接及越界路径；
没有开放卷创建、删除 API。宿主目录必须由可信管理员控制；部署、升级、备份与恢复期间
不要并发移动目录、替换链接或改卷。路径预检不提供对并发管理员操作的事务保证。

## 版本与旧部署

此布局需要 **updater/Guard v0.5.8 或更新版本**，发布清单将最低 updater 设为
v0.5.8。已有部署先升级 updater/Guard，再升级业务版本；不要直接把新模板交给旧 Guard。
发布前使用对应源码构建的开发镜像验证，不要假设尚未发布的版本标签已经可拉取。

更新模板时，updater 从当前 Compose 读取存储来源，并将它保留到目标模板中：旧部署
继续用旧卷，直接 bind 的部署继续用当前目录，回退代码版本也不会自动搬迁数据。
Compose 是布局的唯一记录，没有额外的环境变量或迁移状态文件。混合或非官方来源会
拒绝升级，要求先由管理员核查。原有 bind-backed named volumes 的定义也会保留。

宿主脚本需要 **本地 Docker、Compose v2+、jq**。不支持远程 Docker context。
macOS Docker Desktop（Engine 29.7.2、Compose 5.5.1）已通过存储演练；其他 Desktop /
WSL 组合仍为 uncertain，路径共享由 Docker 管理。部署目录路径不支持逗号。

不使用 `deploy.sh` 时，在完成 `.env` 和 Guard 策略配置后，执行：

```bash
set -e
project=myriad # 与 .env / Guard 中的项目名相同
source scripts/extra/backend-storage.sh
load_backend_storage docker "$project" "$(pwd -P)" prepare
docker compose up -d
```

如果新 Compose 指向目录，但同项目旧 backend 卷仍注册，宿主部署脚本会拒绝启动，
避免误把旧站点启动为空站。必须先完成以下显式迁移。

## Docker 管理卷迁移到目录

这是停机迁移。先做 [整站备份](BACKUP.md)，预留旧卷、归档和新目录同时存在的空间。
以下只适用于官方本地 Docker 单机布局；外置数据库另行备份。保持项目名不变，暂停
外部自动拉起/更新任务。不要运行 `down -v` 或 volume prune。

1. 保存当前 Compose 和脚本；**用原 Compose** 运行 `docker compose down`，停止
   整栈并移除容器，释放旧卷。确认没有其他进程写入。
2. 在部署根目录按实际值修改 `project`，执行下面的 Bash 块。`data/`、`cache/`
   必须尚不存在（包括悬空链接），避免混合新旧内容。

```bash
set -e
project=myriad
root="$(pwd -P)"
saved="$root/backups/storage-layout-$(date +%Y%m%d-%H%M%S)"
umask 077
mkdir -p "$saved"
for kind in data cache; do
    test ! -e "$root/$kind"
    test ! -L "$root/$kind"
done
for kind in data cache; do
    volume="${project}_backend_${kind}"
    docker volume inspect "$volume" >/dev/null
    docker run --rm --network none --mount "type=volume,src=$volume,dst=/old,readonly" \
        --mount "type=bind,src=$saved,dst=/out" alpine:3.20 \
        tar czf "/out/$kind.tar.gz" -C /old .
    docker run --rm --network none --mount "type=bind,src=$saved,dst=/in,readonly" \
        alpine:3.20 tar tzf "/in/$kind.tar.gz" >/dev/null
    mkdir "$root/$kind"
    docker run --rm --network none --mount "type=bind,src=$saved,dst=/in,readonly" \
        --mount "type=bind,src=$root/$kind,dst=/new" alpine:3.20 \
        tar xzpf "/in/$kind.tar.gz" -C /new
done
printf 'Retained old volume archives: %s\n' "$saved"
```

3. 检查归档和新目录里的媒体、Tapp、人设文件完整。两个卷都成功归档、解包后，再
   删除旧卷注册及其 Docker 管理的数据；保留归档作为恢复副本：

```bash
docker volume rm "${project}_backend_data" "${project}_backend_cache"
```

4. 换成新版直接 bind 的 Compose 模板和脚本，保留 `.env`、Guard 策略与项目名，运行
   `bash scripts/extra/deploy.sh up`。核对 backend `/ready`、两个 worker、媒体读取、
   上传、Tapp 和人设资产，再做一次正常升级/回退验证。保留归档直到确认完成。

中途失败应保持停止，核查已有归档与目录；不要重复解包覆盖。对于已使用 local
bind-backed named volumes 的部署，先确认 `docker volume inspect` 的 `Options.device`
就是当前 `data/`、`cache/` 的绝对路径，完成停机备份后仅移除这两个卷注册，再切换
直接 bind 模板；不要重复复制或删除已有目录。

## 回退与换机

代码回退继续使用当前存储来源。若确需把目录改回 Docker 管理卷，先停止整栈，从
**当前目录**重新归档并验证 data/cache，创建原卷名并解包进去，然后恢复旧卷布局的
Compose。不能直接使用迁移前归档，否则会丢失迁移后的写入。保留宿主目录直到回退
验证完成。updater 的文件快照仍只覆盖 `pgdata`，不包含媒体。

整目录换机前，确认已迁移旧卷且 PostgreSQL 在本机。运行 `docker compose down`，
停机后保留数字 uid/gid、权限和隐藏文件，复制部署目录，包括 `.env`、`guard-policy/`、
`pgdata/`、`data/`、`state/`；`cache/` 可不复制。跨 PostgreSQL 主版本或 CPU 架构应使用
[逻辑备份](BACKUP.md)，不要在线打包正在写入的数据库。外置数据库不在此目录中。

目标机执行 `bash scripts/extra/deploy.sh up`；相对 bind 自动解析到新目录，不需要重建
卷注册。若显式配置过 `MYRIAD_COMPOSE_HOST_ROOT`，应改为目标机实际路径。

## 回归演练

`python3 scripts/extra/test-backend-volumes.py` 使用随机命名的临时 Docker 项目和正式
挂载声明，验证新目录、旧卷迁移、worker 读写边界、容器重建、PostgreSQL/媒体恢复、
恢复预检拒绝，以及保留新写入的存储回退。业务服务用轻量测试服务替代，不代表完整
业务功能或带签名发布清单的 updater 升级验证。
