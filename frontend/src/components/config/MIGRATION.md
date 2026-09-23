# Config 区块与设置原语

`ConfigForm` 的各配置区块已拆到 `components/config/*ConfigSection.tsx`，并优先复用 `components/settings/` 统一原语。

壳层逻辑在 `components/config/form/`：

| 模块 | 职责 |
| ---- | ---- |
| `configDomain` / `useConfigDomain` | 单配置域的草稿、已保存快照、加载、保存、重置与可重试生效动作 |
| `domains/*` / `useConfigDomains` | 各独立端点的持久化适配与页面归属；不依赖区块组件的实现 |
| `useConfigBagState` / `configBagReset` / `configBagEffects` | bag 字段编辑、按页默认转换与运行时刷新 |
| `useConfigEditor` | 汇总 dirty、串行执行、保存/重置互斥、错误反馈与 beforeunload |
| `useConfigSearch` / `buildSearchableContent` | 搜索索引与防抖排序 |
| `useConfigNavigation` | 侧栏、移动分层、收藏域与页面切换 |
| `uiBagOwnership` | bag 字段的页面归属与刷新判定 |

`ConfigForm.tsx` 只做编排与 JSX 壳。

## 设置原语（必读）

详见 [`../settings/README.md`](../settings/README.md)。

| 场景 | 使用 |
| ---- | ---- |
| **操作按钮** | **`SettingsButton`**（禁止手写 `btn-base`） |
| 带标签的按钮行 | `ButtonItem` |
| **单选 / 多选分段** | **`SegmentedControl`**（`mode` / `columns`） |
| 带标签的开关行 | `SwitchItem` |
| 卡片头 / 紧凑开关 | `ToggleSwitch` |
| 文本 / 密码 / URL / email | `InputItem` |
| 数字 + 单位 | `NumberItem` |
| 下拉 | `FieldSelect` 或 `SelectItem`（禁止原生 `<select>`） |
| 卡片式多选 | `CheckboxGroupItem` |
| 可操作列表（含可选添加表单 `form`） | `ManagedList` |
| 信息卡 + 操作按钮 | `InfoActionCard` |
| 区块结构 | `SettingSection` + `SettingGroup` |

## 已接入的配置区块

| 组件 | 区块 |
| ---- | ---- |
| `ModuleConfigSection` | 模块（可见性、库来源、报告、一言、音乐播放器） |
| `OAuthConfigSection` | OAuth |
| `UiConfigSection` | UI |
| `PermissionsConfigSection` | 权限 |
| `UsersConfigSection` | 用户管理 |
| `NotificationConfigSection` | 通知 |
| `UpdaterConfigSection` | 更新器（主状态机）；UI 子块在 `config/updater/*` |
| `AiConfigSection` | AI |
| `FederationConfigSection` | 联邦 |
| `AdvancedConfigSection` | 高级（**网络代理** / API 镜像、导入导出重置） |
| `AboutConfigSection` | 关于 |
| `PlatformsConfigSection` | 数据平台（列表 + 二级页 + 自动刷新编排） |
| `PlatformAutoRefreshSettings` | 数据平台 · 自动刷新 |
| `PlatformDataManagement` | 数据平台 · 单平台数据管理（二级页内嵌，非独立路由） |

> 独立路由 `/data-management` 及其兼容重定向均已移除。

`ConfigForm` 本身还负责：导航搜索等壳层 UI、**浮动统一保存**（`handleSave` + dirty 草稿）。数据平台 UI 已拆到 **`PlatformsConfigSection`**（列表拖拽 / 二级凭证 / `PlatformDataManagement` / `AutoHeight`）。

**移动端（&lt;1024px）分层导航**：`data-mobile-pane=nav|section` —— 一级为完整分类列表；点入后为二级内容；平台凭证/数据管理为三级（区块内返回）。桌面仍为左栏 + 右内容双栏。

策略类设置（权限、OAuth、模块、通知、**联邦信任策略** 等）只改草稿，写入走统一保存；即时动作（封禁实例、过滤规则 CRUD、测语音、换域名、平台数据刷新/清缓存）可保留区块内按钮。

## 各一级分类下的子分组（现行）

| 一级 | 子分组顺序 |
| ---- | ---------- |
| 数据平台 | 列表 → 自动刷新；**进入某平台二级页**：凭证字段 → **数据管理**（刷新 / 智能过滤 / 清缓存） |
| AI | 服务商 → 大语言模型 → Agent 人设 → 图片 → 语音 |
| 基础 | 站点地址 → 元数据 → 页脚 → 域名更换 → 背景 → 壁纸动效 |
| 第三方登录 | 标题栏添加登录方式 → 登录方式列表 |
| 联邦 | 身份 → 策略 → 已知实例 → 内容过滤 → 投递队列 → 限流（折叠） |
| 权限 | Agent 预设 → 用户/游客 elevated → 用户/游客 AI 配额 |
| 用户 | 注册策略开关 → **ManagedList**（统计 / 搜索筛选 / 创建 / 展开详情） |
| 通知 | 总开关 → 显示方式 → 各来源 |
| 模块 | 可见性 → 媒体库 → 报告 → 一言 → 音乐 |
| 高级 | 网络代理 → API 镜像 → 备份与恢复 |
| 关于 | 开发信息 + 更新器内联 |

## `ui_config` bag 字段归属

`GET /api/config` 的 `ui_config.config_fields` 仍是跨页共享大袋子；**保存只读 bag**。按 Section 所有权消费与重置（Section 导出 `*_RESET_KEYS`）：

| Section | bag keys | 导出常量 |
| ------- | -------- | -------- |
| **UI**（基础） | `wallpaper_url` `wallpaper_blur` · `site_title` `site_description` `site_favicon` · `site_keywords` `site_og_image` `site_noindex` `site_visibility_policy` `site_ai_intro` `site_seo_review_cadence` `pwa_enabled` · `site_icp` `site_gongan` `cloud_sponsors` `site_footer_custom` · `evocative_*` | `UI_RESET_KEYS`（**不含** `base_url`：域名走 `SiteUrlField` 独立 API） |
| **Platforms**（数据及统计） | `analytics_enabled` · `ga_measurement_id` `umami_website_id` `umami_script_url` | `PLATFORMS_UI_RESET_KEYS` |
| **Modules** | `music_enabled` `music_source` `music_playlist_id` `music_proxy_enabled` `music_preload_enabled` | `MODULE_UI_RESET_KEYS` |
| **Advanced** | `proxy_enabled` `proxy_url` `proxy_bypass` `gemini_base_url` `github_api_base_url` | `ADVANCED_RESET_KEYS` |
| **OAuth** | 只读 `base_url`（编辑走独立 API） | — |

**勿再 emit 到 admin bag 的死字段**（DB / 公开 API / legacy 保存 match 可保留）：

- `pet_enabled` `pet_image_url`
- `wallpaper_parallax`（已被 evocative 动效替代）
- `github_client_id` `github_client_secret`（OAuth 专用端点 + legacy 平铺字段）

新增 bag 字段时：在后端 `build_config` 注明归属 Section，并同步对应 `*_RESET_KEYS` 与默认值。

### 保存语义（踩坑）

- **可清空非敏感串**必须在 `collect_database_updates` 里 early-insert（空串也写库）：`site_*` / `wallpaper_url` / `music_playlist_id` / `proxy_*` / `*_base_url` 镜像等。默认路径 `if !value.is_empty()` 会吞掉「重置本页」写的空串。
- **出站代理 / API 镜像只写库**：`proxy_*` / `gemini_base_url` / `github_api_base_url` 不双写 `.env`。进程里的 `PROXY_ENABLED` / `PROXY_URL` / `PROXY_BYPASS` / `GEMINI_BASE_URL` / `GITHUB_API_BASE_URL` 不是运行时来源。`base_url` 仍写 `.env`（改域名时适配 `FRONTEND_URL` / `CORS_ORIGINS`）。
- **`base_url` 空串不得覆盖**已生效域名（改域名走 `SiteUrlField` 独立 API）。
- **`silent` 更新 bag**（旁路 API 已落库）必须通过 `acceptPatch` 同步更新草稿与已保存快照，否则仍会点亮浮动保存。
- **多域统一保存并非跨端点事务**：每个端点写入成功立即更新其快照；后续端点失败时保留未保存域的草稿。所有已落库域的生效动作仍执行，不能因为后续失败而跳过。
- **生效动作可重试**：运行时重载、权限授予刷新、壁纸/元数据/PWA/语音等各有动作标识；失败动作保留，下次保存只重试剩余动作，不重复写已保存域。
- **编辑会话边界**：站点与 Agent 编辑器按 `authSubject.revision` 建立实例；账号、角色变更及强制重新认证都会清除旧实例草稿。`useConfigEditor` 将身份信号与组件生命周期合并，离开后停止尚未开始的端点写入/刷新，旧加载与保存不再向新页面发布结果。已开始的请求可能完成，不作回滚。
- **并发加载**：过期请求的成功与失败都被忽略；取消加载不覆盖快照。错误提示只属于仍然有效的加载请求。保存/重置的加载检查与部分成功判断读取配置域的实时快照。初始化按当前页优先、其余最多 2 路并发；未打开页失败只在该页显示重试，不把整页打成断连。
- **离页提示**：原生页面卸载在 dirty 或保存/重置进行中触发提示；站内路由切换不属于 `beforeunload`，不声称此处拦截所有导航。
- **写入后读回**：bag 与 OAuth 的写请求和规范化快照读请求分开计账；读回失败保留可重试动作，不重复已确认写入。读回的服务器掩码与规范化字段同样保留期间的新编辑。
- **保存期间继续编辑**：只对自提交后未再编辑的字段应用服务器返回值（包括密钥掩码）。新编辑保留为 dirty。字段数组按稳定身份合并，不能按可能重排的数组位置合并。
- **本页重置**：请求由已保存快照生成，只修改该页拥有的字段；其他页的未保存草稿既不提交也不清除。重置成功走与保存相同的生效动作。
- **全量重置**：对编辑器拥有的配置域应用默认值（包括独立端点）；收藏属于本地导航偏好，不参与。bag 只重置 `ALL_OWNED_UI_BAG_KEYS`，不重置 `base_url`。
- **加载失败**：只在依赖该域的页面显示重试入口，不阻止其他已加载域保存；未加载域不能提交初始默认值。资料库偏好由配置域加载，区块只读取展示统计，不回写编辑器快照。
- **权限边界**：权限域保存与重置改变平台下放配置，并刷新运行时授予权限；不修改 TAPP 的声明权限或批准权限。
- **默认值提示**：`ConfigDefaultsProvider` 注入产品提示源。`settings/` 只消费接口，不持有模型名称、历史默认值或持久化策略。

### 管理端 vs 公开 API

| 端点 | 形状 |
| ---- | ---- |
| `GET /api/config` · `ui_config` | **仅** `config_fields` bag（无 typed 镜像） |
| `GET /api/config/ui` | 公开运行时扁平 JSON；已去掉 `pet_*` / `wallpaper_parallax` |

备份只维护下线黑名单（`pet_*` / `ui_wallpaper_parallax` / `github_client_*` / `github_redirect_url`）；旧备份里这些键进 ignored。启动时按同一名单从 `configurations` 删行。**勿再 emit 到 admin bag**。

## 新增设置时的约定

1. 不要手写 `.toggle-switch` DOM 或原生 `<select>` option 列表。
2. 深色对比与 focus 样式跟 `settings/items/*.css` 走。
3. 领域专用控件（如来源 chip、更新通道卡）可以保留自定义 DOM，但开关/输入/下拉仍优先原语。
