# 媒体资产：备份、恢复与旧目录

持久媒体在 `DATA_DIR/media`（compose 里是 `backend_data` 的 `media` 子路径）。
新部署对应宿主 `./data/media`；旧卷需按 [DATA_LAYOUT.md](DATA_LAYOUT.md) 显式迁移。
外链抓取缓存在 `CACHE_DIR/images`（`backend_cache`），可再生，不进灾备。

新上传、生成、编辑和联邦附件都写入媒体服务。每个资产只有一个地址
`/media/assets/{uuid}/{文件名}`：公开资产带公共缓存头返回，未公开资产只对有权限的
登录者返回且不可缓存。发布与撤回不改地址。`/api/media/{id}/content` 是登录后的
内容路由。Web 进程提供公开与鉴权读取；联邦 worker 停掉不影响本站媒体。
磁盘路径只来自资产的 `storage_key` 或图片缓存自己的目录结构，不从请求 URL 拼接。

## 备份与恢复

与 [BACKUP.md](BACKUP.md) 相同：Postgres + `backend_data` + `.env`。
不要挂 `backend_cache` 当恢复条件。生成图、人设、贴纸、手帐上传和联邦附件
应能从数据卷里的 `media/` 读回。

旧地址退场（见下）之后不能回滚到更早的二进制：旧版本仍要读已删除的
`media_url_aliases` / `media_migration_jobs`。

## 不要做的事

- 启动时扫描整个 `federation_media` 或 `cache/images` 当常规写入。
- 部署脚本里 `rm -rf` 清旧媒体目录来「完成迁移」。
- 把 image-cache 当永久素材备份。
- 未另行授权就对生产做破坏性迁移或删除旧副本。

## 旧地址退场

早期版本用 `/media/federation/…` 和 `/api/brew/image-cache/…` 存放媒体，并由一个升级任务
把这些文件复制进资产存储、登记别名。当前版本在启动时（开放服务之前）一次性退掉这一层：

1. `Migrator::up` 先确认升级任务（`platform_media_v2` 第 4 版）已经完成；没完成的库拒绝启动，
   需先用 0.6 系列的 web 角色跑到完成再升级。没有 `media_migration_jobs` 表的库直接通过。
   任务扫完一遍后只剩「媒体卷上找不到文件」的失败时，它会一直重试、永远到不了完成。卷没挂上
   也是这个样子，所以启动会拒绝并报出缺几个文件：先确认卷已挂载；确认那些文件确实已经没了，
   再设 `MYRIAD_ACCEPT_MISSING_MEDIA=1` 启动一次，它们会被记为 `missing`，引用它们的旧地址
   保持原样（打开是 404）。
2. 同一事务里：所有文本/JSON 列中引用旧地址（以及登记过别名的缓存路径）的内容改写成资产的
   永久地址，改写时关闭该表的用户触发器，不产生笔记历史版本；`/api/brew/image-cache/`
   统一改成 `/api/phantasi/image-cache/`。只改相对地址和本站 origin（`BASE_URL` 等）下的
   绝对地址：别的站点的同名路径是它自己的文件，不动。目录记录的 `url` 规范成永久地址；
   `state` 为空（从未复制进存储）的目录行标为 `missing`；删除 `media_url_aliases`
   与 `media_migration_jobs`。
3. 之后 `/media/federation/…` 与 `/api/brew/…` 不再提供，远端按旧附件地址的拉取会失效。
   `data/federation_media` 卷仍挂载，里面的旧源文件不会被自动删除。
   恢复一份更早导出的设置备份时，其中的 `/media/federation/…` 地址无法对应到资产，会原样
   写回并列在恢复结果的「未解析媒体」里；`/api/brew/image-cache/…` 按新拼写处理。
4. 已知限制：0.6 升级任务从缓存转存的资产，public id 是随机的，退场后缓存路径不再指向它。
   这类资产之后被删除或改为不公开时，同一缓存路径仍会从缓存提供那份远端图片的副本，直到缓存
   淘汰它。

不支持滚动升级：旧副本会读已删除的表。停机后一次性换新版本。

## 图片缓存与转存

`/api/phantasi/image-cache/…` 是可淘汰缓存（RSS 图片、代理下载），不属于退场范围。
笔记、人设、站点设置等作者写的内容引用缓存里的图片时，保存会就地转存为公开资产；
资产的 public id 由缓存路径推导，所以缓存被清理后，这个缓存路径仍由该资产提供。
资产被删除或撤回公开后，缓存路径也不再回落到缓存文件。RSS 条目本身仍是可淘汰缓存，
不会被整体转存。缓存文件已不存在时，保存返回 `MEDIA_NOT_READY`。
本站绝对地址仅按配置的站点 origin 识别，外站同路径不导入。

## 自动恢复

现有进程维护循环每分钟最多恢复 16 个过期暂存资产，并重试最多 16 个删除中资产。
同一循环还会修剪已失效的引用：会话删除后级联消失的 Agent 消息、被删除的 Tapp 存储行、
过期超过一天的引用。运行时写入的 Agent 消息与 Tapp 存储值都会登记它们显示的媒体
（存储值按写入者登记：成员写进安装共享区的图也受保护；报表、组件等直接写存储的路径同样登记）。
媒体库里的「未引用」可以作为删除依据；平台不会自动删除未引用的资产。
访客写入的存储值不登记引用。
循环随进程退出取消，不额外创建永久后台进程。数据库断开时等待下一次维护。
写入每 200 秒续租；最终发布文件前持有资产行锁并核对写入令牌与租约。
过期 writer 被恢复任务接管后不能再发布文件。删除失败保留 `deleting`，后续维护重试。

## 回归测试

在可丢弃的 PostgreSQL 库上设置 `MYRIAD_MEDIA_TEST_DATABASE_URL` 后运行：

```sh
cargo test -p myriad-backend services::media:: -- --nocapture --test-threads=1
cargo test -p myriad-phantasi-notes
```

数据库测试各自创建独立 schema 并在成功后删除，不依赖现有业务数据。
未设置该变量时数据库测试会跳过，不能据此声称已完成数据库验收。


## 媒体目录分页

管理员 `GET /api/media` 每页默认返回 48 条，`limit` 范围为 1–100，按创建时间和 ID 倒序排列。响应包含 `items`、`next_cursor`；首个页面另有匹配当前筛选的 `total`，用于概览统计。

下一页传入 `next_cursor.created_at` 和 `next_cursor.id`，参数名分别为 `before_created_at`、`before_id`。时间字符串应原样传回，保留微秒精度；`next_cursor: null` 表示结束。筛选条件改变时应从第一页重新开始。

`kind` 支持 `all`、`upload`、`generated`；`format` 支持 `all`、`jpeg`、`png`、`gif`、`webp`、`mp4`、`webm`、`mov`、`other`；`query` 对文件名、MIME 和来源作不区分大小写的字面子串匹配。引用标签仅统计当前页资产的未过期引用。
