# 升级说明

## 手帐访客界面下线，笔记改由博客发布（破坏性变更）

访客侧 `/journal` 整棵树下线，**不做 301**：板块、阅读器、收藏、主题流、朋友们与工作台都不再有前端地址，爬虫薄壳和 sitemap 里的手帐条目一并删除。

- **博客：** 已发布笔记由 Hugo 生成静态站，挂在 `/blog`，文章地址 `/blog/posts/{slug}/`。镜像自带 Hugo extended 与 Dart Sass；本机直接跑后端时需要 `hugo` 在 `PATH` 上（或设 `HUGO_BIN`），没有 Hugo 时博客显示为不可用，不挡启动。
- **slug：** 启动时给已发布笔记补 slug，并把库里笔记行上旧的 `/journal...` 链接改成博客地址。联邦对象 ID 仍是 `/phantasi/articles/{id}`。
- **创作台：** 笔记编辑与创作侧管理搬到 `/studio`，仅站长可见；订阅源管理不再有内置界面，改用示例应用「订阅」（RSS reader TAPP）。已安装的「订阅」要重新安装才会带上新声明的 `storage:read` / `storage:write`；不装的话阅读偏好只在当次有效。
- **RSS：** `/journal/notes.xml` 删除，只剩 `/api/phantasi/notes.xml`。
- **反代：** 自带 proxy 把 `/blog` 转给后端；自己按路径分流到后端和前端的反代，也要把 `/blog` 交给后端。`/embed/*` 是 SPA 页面，只允许同源 iframe。
- **AI 注释与播客：** 改在博客文章页的伴读岛里显示。非管理员读 `/api/phantasiai/items/{id}/annotations` 和 `/podcast` 现在只限博客上的文章（非 `admin_only` 的笔记源），并受博客模块可见性约束，其余返回 404；新增只读的 `/api/phantasiai/items/{id}/companion`。站长在创作台的发布设置里生成，生成或重新生成后博客自动重建。

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

Brew 内部名改为 Phantasi，用户界面改为 **手帐**（en: Journal）。手帐访客界面后来已下线，现行地址以文首说明为准。**没有新的 SeaORM 迁移编号。** 绿场直接跑改名后的 `003_phantasi_system`。已有库在 `Migrator::up` 里、丢掉无文件的历史行之前，走一次性临时改名：把 `brew_*` 表、列内 URL/JSON、TAPP 批准权限和 `seaql_migrations` 的 `003_brew_system` 行改成 phantasi。两套表同时存在会启动失败。无 301，旧路径 `/brew`、`/api/brew`、`/api/brewlia`、`Tapp.brewList`、`brew:*` 一律失效。

- **TAPP：** 批准列里的 `brew:read|write|manage|commentWrite` 会被改写成 `phantasi:*`。包内 Manifest 或源码仍声明 `brew:*` / 调用 `Tapp.brewList` 的安装校验与签发 fail-closed，站长必须改 Manifest 后更新或重装。退役串 `brew:comment` 不映射。
- **卫星品牌：** `source_type=brewlia` 改为 `phantasiai`（界面「AI 增强」）；导出格式改为 `.pipack`。本机旧 `.brewpack` 不再能导入，从库重新导出。
- **阅读器：** 当时的用户地址是 `/journal/articles/{id}`，不是 `/phantasi/item/{id}`；它已随手帐下线删除，文章现在是 `/blog/posts/{slug}/`。阅读进度同步 `POST /api/phantasi/sync-states` 与新条目推送 `GET /api/phantasi/ws` 也已删除，依赖它们的外部客户端需要改用逐条的 `/items/{id}/read` 等接口。订阅主题墙的 `GET /api/phantasi/topics`、`PUT /api/phantasi/topics/cards`、`PUT /api/phantasi/items/{id}/topic`、`POST /api/phantasi/items/{id}/suggest-topic` 同样已删除，入库也不再调用 Lite 给订阅文章归类；配置表里残留的 `phantasi_feed_topic_cards` 行不再被读取。阅读进度随之退场：条目响应不再带 `read_progress`，已有库启动时删掉 `phantasi_user_states.read_progress` 列（存过的进度不保留）。浏览器里的 `brew-reader-settings` 不双读。无 301。
- **联邦：** 对象 ID 仍是 `/phantasi/articles/{id}`。本实例不再认对端的 `brew-article` / `brew-recommend`。
- **RSS：** `/brew/notes.xml` 当时改为 `/journal/notes.xml`，后者已随手帐下线删除，现在只有 `/api/phantasi/notes.xml`，默认关闭。无 301。旧 `/phantasi/notes.xml` 不是用户地址。
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

## TAPP phantasi 权限拆分（历史：Brew 权限拆分）

现行权限名是 `phantasi:write` / `phantasi:read` / `phantasi:commentWrite`。下面保留升级当时的 `brew:*` 写法，只说明那次拆分做了什么，不要再往新 Manifest 里写 `brew:*`。

当时：`brew:write` 覆盖已读/未读/全部已读和收藏等当前用户状态；评论与回复的读取并入 `brew:read`，创建、更新与删除改用 `brew:commentWrite`。
- `brew:commentWrite`（现 `phantasi:commentWrite`）是 Elevated 权限，普通用户需由站长显式下放，游客不会获得该授予权限。只有授予层决定能否写评。

## TAPP 联邦写权限拆分（破坏性变更）

声明旧权限 `federation:write` 的 TAPP Manifest 从本版本起会显式失败，不会自动映射或静默降权。请按实际操作改用 `federation:post`、`federation:interact`、`federation:channel`、`federation:room` 和/或 `federation:ring`，然后更新或重装应用。

其中 `federation:post`、`federation:channel`、`federation:room` 为 Elevated 权限，普通用户默认不下放；`federation:interact`、`federation:ring` 为 Basic 权限。所有联邦写操作仍要求持久登录主体，游客无法获得这些能力。

## 设备同步链接仅支持协议 2（破坏性变更）

自本版本起，站点 `/api/agent/link`（`her_link_contract.json`）仅接受 **协议 2** 的连接：

- 收到协议 1 或未携带帧版本 `v` 的 `hello` 时，站点会关闭连接并记录 `agent_sync_devices.app_outdated_at`，设备管理页会提示对应设备上的应用需要更新。
- 新增 `versions` 能力（`link.versions` / `link.verdict`）及设备调用权限组批准（`agent_device_call_groups`、`agent_device_calls`），升级站点时请同步升级连接的 Mac / 移动端应用。
- 新增的「与电脑同步」开关默认关闭，开启前须阅读并确认说明；关闭时心智回到站点。

## 记忆改由统一节点存储与旧话题表退场

她的长期记忆已全面迁入共享核心节点（`memory_nodes.sql`、`agent_node_rows`、`agent_node_heads`、`agent_node_placements`、`agent_node_inbox`、`agent_node_instructions`、`agent_node_migrations`、`agent_node_kept`、`agent_takeback_check`），不再提供新旧切换开关：

- 此前由 `topic_nodes.sql` 创建的 `agent_topic_nodes` 与 `agent_memory_topics` 在日常读写中已停用，当前版本仅保留表结构供历史数据迁移读取。
- 站长可在「记忆 → 节点」管理页按受众查看节点、记忆、经历与意图并执行改名、删除或取消操作。

## 数据库结构升级与启动期自动补齐（`004` / `005` 与 `schema_check`）

`backend/migrations/` 新增 6 个 SQL 迁移文件（`memory_nodes.sql`、`topic_nodes.sql`、`forgotten.sql`、`channel_rows.sql`、`message_embeddings.sql`、`speech_said.sql`），并更新了 `persona_log.sql`、`persona_sync.sql` 与 `user_lifecycle.sql`：

- **旧主机租约拆分**：`agent_host_lease` 拆分为 `agent_her_copy` 与 `agent_her_moves`，`agent_lease_grants` 支持 `freeze` / `thaw`（`holder` 可空）；启动期 `ensure_persona_log_tables` 会把旧 `agent_host_lease` 行迁入新表后删表。
- **同实例联邦私信拆行**：`channel_rows.sql` 移除了 `federation_channels.channel_id` 单列唯一约束与 `fk_fed_channel_messages_channel` 外键，改为 `(channel_id, user_id)` 唯一索引及末尾频道删除触发器。
- **其它新增表与列**：新增 `agent_speech_said`、`agent_message_embeddings`、`agent_forgotten`、`agent_lease_deputies`、`agent_her_segments`、`agent_her_state_rows`、`agent_device_call_groups`、`agent_device_calls`，以及 `agent_messages.origin` / `log_row` 和 `agent_sync_devices.app_outdated_at` / `link_versions`。
- 上述结构变更对已跑过 `004`/`005` 的旧库会在启动期 `schema_check`（`SCHEMA_VERSION = "2026.11.20.1"`）自动幂等补齐；由于涉及旧表迁移与索引重建，**升级前务必先做完整数据库备份**。
