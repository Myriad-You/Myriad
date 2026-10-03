# 升级说明

## 最低支持版本 0.6.1

早于 0.6.1 的库不能直接升到本版本：`Migrator::up` 在任何改动之前拒绝启动，并提示先升到
0.6.1、启动一次，再升级。更新器的发布清单同样把 `min_from_version` 定为 `v0.6.1`，预检就会
拒绝。新装的库不受影响。

设置项的环境变量（平台凭据、站点品牌、Tripo 等）只在库里还没有这项设置时作为初值写进库一次，
之后只读库。原来 `TRIPO_*` 会在运行时覆盖库里的值，现在不会：变量与库里的值不同时，启动日志
会列出变量名，到设置页改。

## 旧媒体地址退场（破坏性变更）

`/media/federation/…`、`/api/brew/image-cache/…` 两条路由和 `media_url_aliases` /
`media_migration_jobs` 两张表一起删掉，后台媒体升级任务与 `/api/media/migration` 接口随之移除。
`Migrator::up` 先确认 `platform_media_v2` 第 4 版已经完成，没完成的库拒绝启动；启动时在开放服务前，
把内容里引用的旧地址改写成永久地址，再删表。外站按旧附件地址的拉取会失效。不支持滚动升级。
细节见 [MEDIA.md](../deployment/MEDIA.md)。

## 每日 AI 配额改为平台设施

每日 AI 配额计数原名 `tapp_quota_usage`，以 `tapp_id` 为键，Agent 和游客全站上限只能占用
`__agent__`、`__anonymous_ai_site__` 这类伪 Tapp id。现在改名为 `ai_quota_usage`，由 `001`
创建；键列改名为 `scope`：Tapp 仍用安装 id，站点自身的功能用 `site:<名字>`（Tapp id 不能
含冒号，不会相撞）。已有库在 `Migrator::up` 里原地改名，当日计数保留。

## AI 费用账本改为平台设施

全站 AI 费用账本原名 `tapp_ai_cost_ledger`，站点自身的调用（Agent、报告、人设等）只能往
`tapp_id` 里填 `__agent__`、`__merope__` 这类占位值。现在改名为 `ai_cost_ledger`，由 `001`
创建；`tapp_id` 可空，只有 Tapp 发起的调用才有，站点调用靠 `source` 区分。已有库在
`Migrator::up` 里原地改名，并把占位值置空。`GET /api/tapp/ai/v2/ledger` 的流水里站点调用的
`tappId` 为 `null`，按 Tapp 的汇总不再包含站点调用。

## 运行时注册表改为平台设施

跨副本共享的租约、邮箱和带过期的记录原名 `tapp_runtime_registry` / `tapp_runtime_mailbox`，
实际上智能体运行、工作检查点、AI 任务、IM 通道、限流和 Tapp 运行时都在用。现在改名为
`runtime_registry` / `runtime_mailbox`（crate `myriad-runtime-registry`，模块
`services::runtime_registry`）。绿场由 `001` 创建。已有库在 `Migrator::up` 里原地改名，
行、主键、索引、序列和用户守卫触发器都保留；新旧两张表同时存在会启动失败。Tapp 的行仍靠
`tapp_id` 区分。多副本部署需要一起升级：旧版本副本找不到旧表名。

## 出站代理与 API 镜像不再写 `.env`

管理台保存不再把 `PROXY_ENABLED` / `PROXY_URL` / `PROXY_BYPASS` /
`GEMINI_BASE_URL` / `GITHUB_API_BASE_URL` 双写进 `.env`。运行时一直读
`configurations`（`/config` → 高级）。宿主 `.env` 里残留的这些键可以删，
删不删都不影响行为。站点公网 origin 仍写 `BASE_URL`（以及随之适配的
`FRONTEND_URL` / `CORS_ORIGINS`）。

## Phantasi / 手帐品牌硬切（破坏性变更）

Brew 内部名改为 Phantasi，用户界面改为 **手帐**（en: Journal）。**没有新的 SeaORM 迁移编号。** 绿场直接跑改名后的 `003_phantasi_system`。已有库在 `Migrator::up` 里、丢掉无文件的历史行之前，走一次性临时改名：把 `brew_*` 表、列内 URL/JSON、TAPP 批准权限和 `seaql_migrations` 的 `003_brew_system` 行改成 phantasi。两套表同时存在会启动失败。无 301，旧路径 `/brew`、`/api/brew`、`/api/brewlia`、`Tapp.brewList`、`brew:*` 一律失效。

- **TAPP：** 批准列里的 `brew:read|write|manage|commentWrite` 会被改写成 `phantasi:*`。包内 Manifest 或源码仍声明 `brew:*` / 调用 `Tapp.brewList` 的安装校验与签发 fail-closed，站长必须改 Manifest 后更新或重装。退役串 `brew:comment` 不映射。
- **卫星品牌：** `source_type=brewlia` 改为 `phantasiai`（界面「AI 增强」）；导出格式改为 `.pipack`。本机旧 `.brewpack` 不再能导入，从库重新导出。
- **阅读器：** 用户地址是 `/journal/articles/{id}`，不是 `/phantasi/item/{id}`。协同仍走 `/api/phantasi/ws`。浏览器里的 `brew-reader-settings` 不双读。无 301。
- **联邦：** 对象 ID 仍是 `/phantasi/articles/{id}`。本实例不再认对端的 `brew-article` / `brew-recommend`。
- **RSS：** `/brew/notes.xml` 改为 `/journal/notes.xml`（另有 `/api/phantasi/notes.xml` 同一份）。默认关闭，管理员在工作台打开；关闭或手帐不对游客可见时 404。无 301。旧 `/phantasi/notes.xml` 不是用户地址。
- **磁盘：** 启动时若存在 `data/brew` 且没有 `data/phantasi`，会改名过去。

临时改名机制（`backend/migrations/phantasi_legacy_rename.rs` 与 `myriad_phantasi::legacy`）已在最低支持版本提到 0.6.1 时删除：更早的库须先升到 0.6.1 并启动一次。

## TAPP 旧权限清理与重新授权

本次升级会从**已安装** TAPP 的批准权限与授予权限两列中移除三项已退役的粗权限：`storage`、`brew:comment`、`federation:write`。这些权限名已从权限枚举中删除，继续保留会让已装应用读回即失败。仍然有效的 `ui:theme`、`media:control` 会原样保留，不会触发重新授权标记。历史权限名 `brew:write` 已随品牌硬切改写成 `phantasi:write`，不再作为有效权限名保留。

**这不是自动权限转换**：迁移不做任何旧权限到新权限的映射，也不会自动授予替代权限。被移除权限的安装会被标记为「需重新授权」，在站长重新授权前：

- 运行时不签发、不校验 runtime grant（fail-closed），`start` 也拒绝；
- 已是 `Running` 的安装会被打回 `Installed`，不能继续当活实例；
- 已签发的 runtime grant 会按安装 `tapp_id` 从 `runtime_registry` 删除（与 `revoke_all_tapp_runtime_grants` 同一条过滤），飞在路上、已经过校验的请求仍可能走完，下一次 rebind / 新签发失败关闭；
- `/tapi` 入站与调度器出站同样拒绝，不会按清理后剩下的批准权限继续跑；
- Agent 探权（`permission.check`）与详情里的授予权限投影在标记为真时为空，不会把批准列剩余名字报成仍已授予；
- 目录列表/详情中该应用呈现为需重新授权状态，详情的授予权限投影为空，可发起更新/重新授权。

`needs_reauthorization` 列只由迁移 016 写入 `true`。启动时的 schema heal 只会 `ADD COLUMN … DEFAULT false`，不会跑数据清理。只 heal、没跑迁移时，退役串仍在列里，签发会因未知名失败，但 start / inbound / 调度器只认持久标记。

**数据清理不可逆**：被移除的旧权限串无法恢复，且**回滚该迁移不会删除 `needs_reauthorization` 列，也不会清除已落库的重新授权标记**——持久信号必须在回滚后继续存在，否则受影响安装会在权限已清空的情况下被静默放行。

受影响安装的**审批权限与授予权限会各自独立清理**，与旧权限无关的权限及其顺序保持不变。请站长在升级后**逐台检查被标记的应用**，为其重新授权当前有效的权限名。新安装与正常更新不受影响。

## Runtime 配置 seed

启动时会为数据库中缺失的运行时配置写入当前默认值，且不会覆盖站长已有配置。

本次升级有两项显式默认开放的**授予权限**：普通用户默认获得 `component:theme`（主题组件注册）和 `shortcut:register`（快捷键注册）。站长已经保存的关闭值会保留。

此前已保存但因解析缺失未生效的 AI 配额配置，升级后会开始生效；暂存池和常驻配额也会写入默认值：隐藏容量 `8`、隐藏空闲淘汰 `300` 秒、单应用常驻 `1`、全站常驻 `3`。

## TAPP 存储权限拆分（破坏性变更）

声明旧权限 `storage` 的 TAPP Manifest 从本版本起在**安装校验与运行授权签发时一律失败关闭**（fail-closed）：该权限名已从平台权限枚举中移除，不再被识别。

- **不会自动映射。** `storage` 不会被解码成任何新权限，能力也不会被静默丢弃——安装或签发直接报错，而不是缺斤少两地放行。
- **站长必须更新 Manifest。** 把声明的 `storage` 改为 `storage:read`（读取私有存储）和/或 `storage:write`（写入私有存储），然后更新或重装该应用。
- **改变平台下放配置无法修复。** `storage` 是声明层不存在的权限名，调整平台侧的权限下放开关对未知的声明权限没有任何作用。

已安装应用的 `approved_permissions` 里如果还留着无法识别的旧名（例如 `storage`），列表和详情会标 `needs_reauthorization`，授予权限为空，直到更新 Manifest 并更新或重装。不会自动把 `storage` 改写成 `storage:read` / `storage:write`。

## TAPP 手帐权限拆分（历史：Brew 权限拆分）

现行权限名是 `phantasi:write` / `phantasi:read` / `phantasi:commentWrite`。下面保留升级当时的 `brew:*` 写法，只说明那次拆分做了什么，不要再往新 Manifest 里写 `brew:*`。

当时：`brew:write` 覆盖已读/未读/全部已读和收藏等当前用户状态；评论与回复的读取并入 `brew:read`，创建、更新与删除改用 `brew:commentWrite`。
- `brew:commentWrite`（现 `phantasi:commentWrite`）是 Elevated 权限，普通用户需由站长显式下放，游客不会获得该授予权限。只有授予层决定能否写评。

## TAPP 联邦写权限拆分（破坏性变更）

声明旧权限 `federation:write` 的 TAPP Manifest 从本版本起会显式失败，不会自动映射或静默降权。请按实际操作改用 `federation:post`、`federation:interact`、`federation:channel`、`federation:room` 和/或 `federation:ring`，然后更新或重装应用。

其中 `federation:post`、`federation:channel`、`federation:room` 为 Elevated 权限，普通用户默认不下放；`federation:interact`、`federation:ring` 为 Basic 权限。所有联邦写操作仍要求持久登录主体，游客无法获得这些能力。
