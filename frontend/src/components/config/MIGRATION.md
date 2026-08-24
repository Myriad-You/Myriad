# Config 区块与设置原语

`ConfigForm` 的各配置区块已拆到 `components/config/*ConfigSection.tsx`，并优先复用 `components/settings/` 统一原语。

壳层逻辑在 `components/config/form/`：

| 模块 | 职责 |
| ---- | ---- |
| `useConfigBagState` | bag 主状态、字段更新、loadConfig |
| `useConfigSideDrafts` | 旁路 draft（OAuth / 权限 / 联邦 / 库 / 一言…） |
| `useConfigDirty` | 多源 dirty + beforeunload |
| `useConfigSave` | 两阶段原子保存 + 按需硬刷 |
| `useConfigReset` | 全量 / 本页重置 |
| `useConfigSearch` | 搜索索引 + 防抖 rank |
| `useConfigNavigation` | 侧栏、移动分层、收藏、section 切换 |
| `buildSearchableContent` | 纯函数搜索索引 |
| `uiBagOwnership` | bag 归属 key / hard-reload 判定 |

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
| **UI**（基础） | `wallpaper_url` `wallpaper_blur` · `site_title` `site_description` `site_favicon` · `site_keywords` `site_og_image` `site_noindex` `site_visibility_policy` `site_ai_intro` `pwa_enabled` · `site_icp` `site_gongan` `cloud_sponsors` `site_footer_custom` · `evocative_*` | `UI_RESET_KEYS`（**不含** `base_url`：域名走 `SiteUrlField` 独立 API） |
| **Platforms**（数据及统计） | `analytics_enabled` · `ga_measurement_id` `umami_website_id` `umami_script_url` | `PLATFORMS_UI_RESET_KEYS` |
| **Modules** | `music_enabled` `music_source` `music_playlist_id` | `MODULE_UI_RESET_KEYS` |
| **Advanced** | `proxy_enabled` `proxy_url` `proxy_bypass` `gemini_base_url` `github_api_base_url` | `ADVANCED_RESET_KEYS` |
| **OAuth** | 只读 `base_url`（编辑走独立 API） | — |

**勿再 emit 到 admin bag 的死字段**（DB / 公开 API / legacy 保存 match 可保留）：

- `pet_enabled` `pet_image_url`
- `wallpaper_parallax`（已被 evocative 动效替代）
- `github_client_id` `github_client_secret`（OAuth 专用端点 + legacy 平铺字段）

新增 bag 字段时：在后端 `build_config` 注明归属 Section，并同步对应 `*_RESET_KEYS` 与默认值。

### 保存语义（踩坑）

- **可清空非敏感串**必须在 `collect_database_updates` 里 early-insert（空串也写库）：`site_*` / `wallpaper_url` / `music_playlist_id` / `proxy_*` / `*_base_url` 镜像等。默认路径 `if !value.is_empty()` 会吞掉「重置本页」写的空串。
- **`base_url` 空串不得覆盖**已生效域名（改域名走 `SiteUrlField` 独立 API）。
- **`silent` 更新 bag**（旁路 API 已落库）必须同步 patch `initialConfig`，否则 `deepEqual(config, initialConfig)` 仍会点亮浮动保存。
- **多 draft 统一保存**：阶段 1 全部写库 → 阶段 2 再 mark clean（`setInitialConfig` / 各 `setSaved*`）。禁止中途 clean，避免 OAuth 失败后 bag 已 clean 的状态分裂。
- **保存后刷新策略**（见 `uiBagOwnership.ts`）与 toast 文案：
  - AI / platforms / auto_fetch / 纯 UI bag → 软保存 `savedSuccess`（platforms 另 `clearDedupCache` library）
  - 代理 / API 镜像 → `configChangesNeedRuntimeReload`：`reloadSystemConfig` **无** `location.reload`；toast `savedSuccessRuntimeReload`
  - 壁纸 / Evocative → `configChangesNeedWallpaperReload` 软刷壁纸层
  - 硬刷路径（导入 / 清缓存 / 预留 hard save）：`hardReloadPreparing` → `savedSuccessHardReload`（勿盖成泛用 `savedSuccess`）；导入确认文案也说明将整页刷新
  - `configChangesNeedHardReload` 当前主表单恒 false（保留分支供未来进程级变更）
- **全量重置 bag**：只用 `ALL_OWNED_UI_BAG_KEYS`（见 `uiBagOwnership.ts`），勿重置 `base_url`。

### 管理端 vs 公开 API

| 端点 | 形状 |
| ---- | ---- |
| `GET /api/config` · `ui_config` | **仅** `config_fields` bag（无 typed 镜像） |
| `GET /api/config/ui` | 公开运行时扁平 JSON；已去掉 `pet_*` / `wallpaper_parallax` |

备份 registry 仍可含 legacy 键（`pet_*` 等），用于还原旧备份；**勿再 emit 到 admin bag**。

## 新增设置时的约定

1. 不要手写 `.toggle-switch` DOM 或原生 `<select>` option 列表。
2. 深色对比与 focus 样式跟 `settings/items/*.css` 走。
3. 领域专用控件（如来源 chip、更新通道卡）可以保留自定义 DOM，但开关/输入/下拉仍优先原语。
