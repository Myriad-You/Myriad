# Manifest 配置

Manifest 是 Tapp 的核心配置文件，定义了应用的元数据、权限和功能。

## 基础字段

| 字段                     | 类型     | 必填 | 说明                               |
| ------------------------ | -------- | ---- | ---------------------------------- |
| `id`                     | string   | ✅   | 唯一标识符，推荐使用反向域名格式   |
| `name`                   | string   | ✅   | 应用名称                           |
| `version`                | string   | ✅   | 版本号（语义化版本）               |
| `description`            | string   | ❌   | 应用描述                           |
| `locales`                | object   | ❌   | 名称/描述的多语言覆盖（见下文）    |
| `main`                   | string   | ✅   | 入口文件名                         |
| `author`                 | object   | ❌   | 作者信息 `{name, email?, url?}`    |
| `permissions`            | string[] | ❌   | 所需权限列表                       |
| `icon`                   | string   | ❌   | 图标（emoji 或 URL）               |
| `iconSvg`                | string   | ❌   | 内联 SVG 图标代码（优先于 icon）   |
| `themeColor`             | string   | ❌   | 主题色（十六进制，如 #6366f1）     |
| `widgets`                | object[] | ❌   | 小组件定义                         |
| `hasPage`                | boolean  | ❌   | 是否有页面模块（可在页面模式运行） |
| `backgroundRequirements` | string[] | ❌   | 启动后需常驻的 headless core 能力  |
| `settings`               | object[] | ❌   | 用户可配置的设置项                 |
| `apis`                   | object   | ❌   | 命名 API 声明（代理+权限校验）     |
| `dataExchange`           | object   | ❌   | 跨 Tapp 具名 import/export 契约    |
| `ai`                     | object   | ❌   | 服务端治理的 AI Task 声明          |
| `events`                 | object   | ❌   | Event Broker 发布/订阅 topic 声明  |
| `agent`                  | object   | ❌   | Agent Interaction 声明             |
| `minSystemVersion`       | string   | ❌   | 最低兼容 Myriad 语义版本           |
| `homepage`               | string   | ❌   | 应用主页 URL                       |
| `repository`             | string   | ❌   | 代码仓库 URL                       |
| `styles`                 | string   | ❌   | 自定义样式文件路径                 |
| `cssMode`                | string   | ❌   | `unified`（默认）或 `separated`    |
| `widgetStyles`           | string   | ❌   | Widget 专用 CSS 路径               |
| `pageStyles`             | string   | ❌   | Page 专用 CSS 路径                 |
| `pageTemplate`           | string   | ❌   | 页面 HTML 模板路径                 |
| `pageModules`            | string[] | ❌   | `page/` 模块执行顺序               |
| `category`               | string   | ✅   | 应用用途分类（稳定 ID）            |
| `assets`                 | string[] | ❌   | 包内静态资源路径（须在 `assets/` 下） |
| `openUrls`               | object[] | ❌   | 宿主代开的外链 allowlist（配合 `ui:openUrl`） |

`author` 整体可选；**若提供**则 `author.name` 必填（1–255 字符），`email` / `url` 可选且须合法。
作者名称会显示在商店卡片和 Tapp 详情页，详情页还会显示邮箱，并为通过 HTTP(S) 校验的作者主页生成外部链接。

所有资源路径都是相对安装根目录的安全路径。`.tapp` 文件安装会保留经过校验的嵌套
目录，例如 `templates/widget-2x2.html`；direct/store 安装也会把内容写到 Manifest
声明的位置。绝对路径、隐藏组件和 `..` 会被拒绝。`pageModules` 的每项是 `page/`
目录内的文件名，不能再次包含目录前缀。`main` 必须是 `.js`，样式路径必须是 `.css`，
Page/Widget 模板必须是 `.html`；代码与模板类声明资源必须是安装目录内的普通 UTF-8 文本
文件。`assets` 允许二进制（贴图、音频、wasm、JSON 关卡等），路径必须位于 `assets/`
下，且不得使用 `.js` / `.html` 扩展名；默认单文件 ≤ 5 MiB，合计 ≤ 20 MiB，最多 64 项。
`category` 为 `game` 或 `developer`，并且声明了 `game` 或 `runtimeModules` 时，放宽到
单文件 12 MiB / 合计 48 MiB / 128 项。资源读取不会跟随安装后插入的符号链接。运行时通过
`Tapp.assets` 读取，详见 [图形与轻量游戏](GRAPHICS.md)。

### 游戏会话与宿主 Three（`game` / `runtimeModules`）

```json
{
  "category": "game",
  "permissions": ["game:session", "federation:read", "federation:write", "federation:message"],
  "game": { "protocol": "gomoku", "maxPlayers": 2 },
  "runtimeModules": ["three"]
}
```

- `game.protocol`：小写 `[a-z0-9._-]`，会组成房间消息类型 `game:<tappId>:<protocol>`。
- 声明 `game` 时必须同时申请 `game:session` 与 `federation:read` / `federation:write` / `federation:message`。
- `runtimeModules` 目前只允许 `three`，且仅 game / developer 分类。宿主注入钉死版本，不走 CDN。
- `Tapp.game.create()` 默认不公开。同一实例可用 `room_id@home_server` 加入；跨实例私房必须邀请（邀请会带上 `game` 配置），跨实例自助加入要 `{ isPublic: true }`。
- 发送/入站只接受该房间绑定的 `game:<tappId>:<protocol>`；`seq` / `nonce` 由对局自己去重。

### 外链 allowlist（openUrls）

沙箱**不能**使用 `window.open` / `<a target=_blank>` 离开 Myriad。需要打开外部页面时：

1. 在 Manifest 声明 `permissions: ["ui:openUrl"]` 与非空 `openUrls`；
2. 运行时只调用 `Tapp.ui.openUrl({ id, path?, query? })`（或简写 `openUrl("id")`）；
3. 宿主按声明重建 URL，未命中 allowlist **一律拒绝**。

| 字段 | 类型 | 必填 | 说明 |
| ---- | ---- | ---- | ---- |
| `id` | string | ✅ | 稳定 id，供 SDK 引用（1–64，字母数字与 `._-`） |
| `url` | string | ✅ | 基址：**HTTPS**；仅 `localhost` / `127.0.0.1` / `::1` 允许 `http`；禁止凭据与 `#fragment` |
| `match` | string | ❌ | `exact`（默认）/ `prefix` / `origin` |

匹配规则：

- **`exact`**：只能打开声明的完整 URL；不允许 `path` / `query`
- **`prefix`**：同 origin，路径须落在声明 path 前缀下（可带 `path` / `query`）
- **`origin`**：同 origin 任意 path/query（自由度最高，商店审核应更严）

```json
{
  "permissions": ["ui:openUrl"],
  "openUrls": [
    {
      "id": "docs",
      "url": "https://docs.example.com/guide/",
      "match": "prefix"
    },
    {
      "id": "status",
      "url": "https://status.example.com/health"
    }
  ]
}
```

```javascript
// Widget / Page
await Tapp.ui.openUrl({ id: "docs", path: "install" });
// → https://docs.example.com/guide/install

await Tapp.ui.openUrl({ id: "status" });
// → https://status.example.com/health

// 未声明 id 或逃出 prefix → 失败
await Tapp.ui.openUrl({ id: "docs", path: "../evil" }); // reject
```

约束摘要：最多 32 条；`ui:openUrl` 与 `openUrls` 必须同时出现；headless core **无**此 API；
宿主有打开速率限制；实现细节见 [API 参考 · openUrl](API_REFERENCE.md#打开声明链接-openurl)。

Manifest 采用严格字段校验：未声明字段、拼写错误以及已经移除的字段都会让安装失败，
不会再被静默忽略。所有运行能力都必须直接写入 `permissions`；宿主只会在真正调用时
按权限和运行时策略决定是否授权。

### 多语言名称与描述（locales）

`locales` 为顶层 `name` / `description` 提供按语言的展示文案覆盖，宿主会按当前界面
语言解析（商店卡片、应用列表、详情页、运行页标题、Widget 回退文案等）：

```json
{
  "name": "我的应用",
  "description": "一个功能丰富的 Tapp 示例",
  "locales": {
    "en-US": { "name": "My App", "description": "A feature-rich Tapp example" },
    "ja-JP": { "description": "機能豊富な Tapp サンプル" }
  }
}
```

- 键必须是 BCP-47 语言标签（如 `zh-CN`、`en-US`、`ja-JP`），最多 32 个语言；
  值中 `name`（1-255 字符）与 `description`（≤ 2000 字符）均可选。
- 解析回退链：精确匹配（忽略大小写）→ 语言前缀匹配（`zh-CN` ↔ `zh`）→ 顶层
  `name` / `description`。顶层字段是所有语言未命中时的兜底。
- `locales` 只覆盖清单展示文案。应用内 UI 仍走 `i18n/{lang}.json` 与 `Tapp.i18n`
  （见 [PAGE.md](PAGE.md)）。
- 商店详情长介绍与静态预览**不要**写进 Manifest `locales`（安装会拒绝未知字段）。
  那些字段属于 `catalog.json` / `index.json`，见 [STORE](STORE.md)。

安装级第三方密钥见下文 [安装级 API 凭据](#安装级-api-凭据-credentials)。

### 应用分类

`category` 只表示应用用途，值必须是下列稳定 ID 之一：

| ID             | 用途                       |
| -------------- | -------------------------- |
| `ai`           | AI 应用                    |
| `data`         | 数据处理、管理与展示       |
| `developer`    | 开发、调试与部署工具       |
| `game`         | 游戏                       |
| `media`        | 音频、视频与其他媒体体验   |
| `productivity` | 笔记、任务与效率工具       |
| `social`       | 社交、消息与协作           |
| `utility`      | 无法归入上述用途的通用工具 |

Page、Widget 和 headless core 是运行形态，由 `hasPage`、`widgets` 和
`backgroundRequirements` 表达，不得填入 `category`。`demo` 和 `test`
属于发布阶段，应使用商店标签表达。宿主会把旧值 `tools`、`games`、
`development`、`music`、`visualization` 等规范为上述 ID；新包应直接使用规范值。
界面仅翻译显示名称，Manifest 和商店索引不存储本地化分类文本。
从商店安装时，后端会在规范化旧别名后比对索引和 Manifest；两者分类不一致会拒绝安装。
完整别名（Rust `TappCategory` / 前端 `CATEGORY_ALIASES`）：`data`←`data-extension|platform|visualization`，
`developer`←`development|dev`，`game`←`games`，`media`←`entertainment|music`，
`social`←`communication`，`utility`←`demo|page|test|tool|tools|utilities|widget`。

`version` 必须是语义版本；`themeColor` 使用 `#RRGGBB`；`homepage`、`repository` 和
作者主页只接受 HTTP(S)。声明 Widget 必须同时声明 `widget:register`；所有 HTTP API
必须声明 `network:fetch`，内置 AI API 必须声明对应的 `ai:*` 权限。`pageModules` 只接受
不重复的 `.js` 文件名。无效声明会在安装或更新时直接拒绝，不留到运行时静默失败。

`minSystemVersion` 使用语义版本。直接安装、商店安装和更新都会由后端与当前 Myriad
包版本比较；当前版本过低或字段格式无效时会拒绝写入，避免出现“安装成功但运行时才
发现 API 不兼容”。最低版本只写在包内 Manifest；商店 index 不重复维护第二份版本来源。

### 所有权、可见性与同 ID 并存

完整模型以 [ARCHITECTURE.md · 所有权与可见性](./ARCHITECTURE.md#所有权与可见性) 为准；
本处只固定与 Manifest / 安装相关的要点：

- **公开与私有可并存**：站点管理员在规范公开 owner 命名空间安装的公开 Tapp，与普通用户
  在自己命名空间安装的私有 Tapp，可以共用同一个 `tapp_id`，各自保留独立的文件、
  Manifest 与 `approved_permissions`。
- **冲突检查按可写命名空间**：安装冲突只检查操作者允许写入的 owner 集合——普通用户只
  检查自己是否已安装该 ID；管理员只检查规范公开 owner。因此用户私有安装**不能**占用 ID
  阻止管理员后续发布公开版；管理员公开安装也**不会**仅因同 ID 就擦掉用户私有副本。
- **解析优先私有**：列表、详情、资源、运行时、Runtime Grant 与 Manifest 声明 API
  （`apis`）对同一 `tapp_id` 一律优先当前 viewer 的私有安装；没有私有副本时再使用站点
  公开安装。不要写成“公共版本优先显示”。
- **Storage 与 Settings 不同命名空间**：
  - `Tapp.storage` 的持久主体是 Runtime Grant **subject**（`user_id + tapp_id`）。打开
    公开安装时，每个 subject（持久用户或**签名游客 session**）读写自己的私有 storage，
    不会读取站点 owner 的数据。`storage:read` 为 guest-safe basic：签名游客可获 Grant 与负 id
    命名空间下的持久 storage；无签名 session 则无 storage。
  - Manifest 声明的安装级设置（宿主 `Tapp.settings` / REST `GET|POST …/settings`）挂在
    **installation owner** 命名空间：owner 或管理员可**写**；凡能解析到该安装的运行者
    （含**游客打开公开安装**）可**读**已保存的声明键。未写入时回落 `defaultValue`。
  - 不要笼统说“用户 storage/settings 按用户 + 稳定 Tapp ID 连续保留并在公/私同 ID 间复用”；
    storage 随 subject 私有，settings 随安装 owner，两者不可混为一谈。
- 安装/更新采用 staging 校验和原子目录切换，失败不会把半份 Manifest 或资源留在在线目录。

## 完整示例

```json
{
  "id": "com.example.my-tapp",
  "name": "我的应用",
  "version": "1.0.0",
  "description": "一个功能丰富的 Tapp 示例",
  "category": "utility",
  "main": "index.js",
  "author": {
    "name": "开发者名称",
    "email": "dev@example.com",
    "url": "https://example.com"
  },
  "icon": "🚀",
  "themeColor": "#6366f1",
  "permissions": [
    "storage:read",
    "ui:notification",
    "platform:read",
    "network:fetch"
  ],
  "hasPage": true,
  "backgroundRequirements": ["scheduler", "sync"],
  "homepage": "https://example.com",
  "repository": "https://github.com/example/my-tapp",
  "minSystemVersion": "0.2.1",
  "apis": {
    "weather": {
      "type": "http",
      "access": "protected",
      "endpoint": "https://api.weather.com/v1/current",
      "method": "GET",
      "description": "获取天气信息",
      "spoof": "china",
      "inject": { "city": "{{geo.city}}" }
    }
  },
  "widgets": [
    {
      "id": "stats-widget",
      "name": "数据统计",
      "description": "展示平台统计数据",
      "icon": "📊",
      "defaultSize": "2x2",
      "sizes": ["1x1", "1x2", "2x1", "2x2", "4x2", "4x4"],
      "category": "utility"
    }
  ],
  "settings": [
    {
      "key": "refreshInterval",
      "type": "number",
      "label": "刷新间隔",
      "description": "自动刷新间隔（秒）",
      "defaultValue": 60,
      "min": 10,
      "max": 3600
    }
  ]
}
```

---

## widgets 配置

小组件定义允许管理员将应用提供的 Widget 添加到 Dashboard。

Manifest 是这些注册元数据的权威来源。安装和每次更新都会 upsert 当前声明，并删除上一版
Manifest 已移除的 Widget。运行时 `Tapp.widget.register()` 创建的是独立动态注册，只允许
当前管理员调用，并且必须有 `widget:register` 与 Runtime Grant；动态代码不能覆盖或注销 Manifest 声明项。公共安装的
Manifest Widget 对所有可见主体共享；动态 Widget 同时绑定注册主体和 Runtime Grant 中的
安装 owner，只返回给该主体，并在对应安装卸载时清理。

普通用户仍可安装包含 `widgets` / `widget:register` 声明的 Tapp；安装时只会从该用户的最终
Runtime Grant 中剔除管理员专属的动态注册能力，不会因应用带有 Widget 功能而拒绝安装，
Page、Core 与其余获授能力仍可正常使用。

```json
{
  "widgets": [
    {
      "id": "my-widget",
      "name": "我的小组件",
      "description": "示例 Widget",
      "icon": "🧊",
      "defaultSize": "2x2",
      "sizes": ["1x1", "1x2", "2x1", "2x2", "3x2", "4x2", "4x4"],
      "category": "utility",
      "templates": {
        "2x2": "templates/widget-2x2.html",
        "4x2": "templates/widget-4x2.html"
      },
      "settings": [
        {
          "key": "compact",
          "type": "toggle",
          "label": "紧凑布局",
          "defaultValue": false
        }
      ],
      "refreshPolicy": {
        "mode": "event",
        "refreshOnVisible": true
      }
    }
  ]
}
```

### Widget 字段说明

| 字段            | 类型     | 必填 | 说明                                                           |
| --------------- | -------- | ---- | -------------------------------------------------------------- |
| `id`            | string   | ✅   | Widget 唯一标识符                                              |
| `name`          | string   | ✅   | Widget 显示名称                                                |
| `description`   | string   | ❌   | Widget 描述                                                    |
| `icon`          | string   | ❌   | Widget 图标（emoji 或 URL）                                    |
| `defaultSize`   | string   | ✅   | 默认尺寸（如 "2x2"）                                           |
| `sizes`         | string[] | ✅   | 支持的尺寸列表                                                 |
| `category`      | string   | ❌   | Widget 分类（stats, activity, visualization, utility, custom） |
| `templates`     | object   | ❌   | HTML 模板（按尺寸覆盖）                                        |
| `settings`      | object[] | ❌   | 每个 Dashboard 实例独立的设置声明                              |
| `refreshPolicy` | object   | ❌   | 宿主管理的刷新策略                                             |

单个 Tapp 最多声明或动态注册 64 个 Widget；每个 Widget 最多声明 10 个尺寸，且
`defaultSize` 必须包含在 `sizes` 中。超出限制会在安装或注册时被后端拒绝。
Widget `category` 只接受表中列出的五个稳定 ID；旧值 `tool` 会规范为 `utility`，
其他未知值会在 Manifest 解析或动态注册时被拒绝。旧数据库记录仍可读取，但不会再写入
新的非规范分类。
顶层 `settings` 是整个 Tapp 共用的全局设置；`widgets[].settings` 则属于单个 Dashboard
Widget 实例，因此同一种 Widget 添加两次时可以采用不同配置。实例设置会由 Dashboard
设置面板保存并通过 `props.config`、`Tapp.widget.getInstanceSettings()` 提供给沙箱。

`refreshPolicy.mode` 默认为事件驱动语义：同一 Tapp 的其他运行实例发生
`Tapp.storage` 变更时，宿主会通知并刷新可见 Widget（**跨沙箱首选路径**）。
当前 **Widget 沙箱**还可用 `Tapp.widget.invalidate()` 对本实例显式 re-render；
Page / headless **没有**该方法——共用 core 里调用会抛错。确实需要轮询时可设为
`interval` 并提供 `intervalSeconds`（15–86400 秒）；计时器仅在页面和 Widget 可见
且 Tapp 运行时工作。`refreshOnVisible` 默认为 `true`。后台同步应使用
scheduler/headless core，而不是依赖 Widget 的可见计时器。

模板按 `Widget ID + 尺寸` 隔离。同一个 Tapp 的多个 Widget 可以各自声明不同的 `2x2`
模板，不会互相覆盖。商店索引中的 `download.widget_templates` 也必须使用
`{ "widgetId": { "2x2": "path/to/template.html" } }` 结构。

### templates 配置说明

`templates` 字段允许为不同尺寸的 Widget 指定 HTML 模板文件。系统会在渲染前加载模板内容到容器中，然后调用 JS 渲染函数进行事件绑定和数据填充。

```json
{
  "templates": {
    "2x2": "widget-2x2.html",
    "4x2": "widget-4x2.html",
    "4x4": "widget-4x4.html"
  }
}
```

**⚠️ 重要**：

1. **文件必须存在**：如果声明了模板路径，对应文件必须实际存在，否则会导致 Widget 渲染失败
2. **路径相对于应用目录**：模板路径相对于 Tapp 应用根目录
3. **未声明的尺寸**：对于未在 `templates` 中声明的尺寸，系统会完全依赖 JS 渲染

**模板文件示例** (widget-2x2.html)：

```html
<div
  class="h-full w-full flex flex-col p-3 glass rounded-xl"
  data-widget-root="true"
>
  <div class="flex items-center gap-2 mb-2">
    <span class="text-lg" data-icon>🤖</span>
    <span class="font-semibold text-sm" data-title>标题</span>
  </div>
  <div class="flex-1 overflow-auto" data-content="main">
    <!-- JS 会填充这里 -->
  </div>
</div>
```

**推荐实践**：

- 使用 `data-*` 属性标记需要 JS 操作的元素
- 模板定义静态结构，JS 负责动态内容和事件绑定
- 不同尺寸的模板可以有完全不同的布局

### 支持的尺寸

| 尺寸  | 像素（默认） | 适用场景         |
| ----- | ------------ | ---------------- |
| `1x1` | 100×100      | 图标、状态指示器 |
| `1x2` | 100×200      | 竖向简报         |
| `2x1` | 200×100      | 简单统计、标题   |
| `2x2` | 200×200      | 标准小组件       |
| `2x3` | 200×300      | 列表 / 纵向卡片  |
| `3x2` | 300×200      | 横向信息块       |
| `4x1` | 400×100      | 紧凑横幅         |
| `4x2` | 400×200      | 宽幅展示、图表   |
| `2x4` | 200×400      | 长列表 / Feed    |
| `3x3` | 300×300      | 中等复杂组件     |
| `4x4` | 400×400      | 大型展示         |

---

## hasPage 配置

声明应用是否有页面模块。设为 `true` 后，运行中的 Tapp 可以点击打开页面视图。

```json
{
  "hasPage": true
}
```

### 页面模块的作用

页面模块允许 Tapp 提供完整的页面体验，而不仅仅是小组件。当用户点击运行中的 Tapp 时，会打开一个全屏页面视图。

### 何时声明 `hasPage: true`

- 应用需要提供详细的配置界面
- 应用需要展示大量数据（如列表、报告、仪表盘）
- 应用需要复杂的交互界面（如编辑器、游戏）
- 应用希望提供比 Widget 更丰富的功能

### 代码结构要求

声明 `hasPage: true` 后，需要在 `PAGE_CODE` 中定义页面渲染逻辑：

```javascript
// PAGE_CODE 中
Tapp.pages["my-page"] = {
  render: function (container, locale, isDark, primaryColor) {
    var bgLayer = document.getElementById("tapp-background");
    var contentLayer = document.getElementById("tapp-content");
    // 渲染页面...
  },
};

Tapp.lifecycle.onReady(async function () {
  var locale = await Tapp.ui.getLocale();
  var theme = await Tapp.ui.getTheme();
  var primaryColor = await Tapp.ui.getPrimaryColor();

  Tapp.pages["my-page"].render(null, locale, theme === "dark", primaryColor);
});
```

---

## settings 配置

允许用户自定义 Tapp 行为。

```json
{
  "settings": [
    {
      "key": "refreshInterval",
      "type": "number",
      "label": "刷新间隔",
      "description": "自动刷新间隔（秒）",
      "defaultValue": 60,
      "min": 10,
      "max": 3600
    },
    {
      "key": "theme",
      "type": "select",
      "label": "主题",
      "defaultValue": "auto",
      "options": [
        { "value": "auto", "label": "跟随系统" },
        { "value": "light", "label": "亮色" },
        { "value": "dark", "label": "暗色" }
      ]
    },
    {
      "key": "showDetails",
      "type": "toggle",
      "label": "显示详情",
      "defaultValue": true
    }
  ]
}
```

### 支持的设置类型

| 类型     | 说明     | 额外字段                                 |
| -------- | -------- | ---------------------------------------- |
| `toggle` | 开关     | -                                        |
| `select` | 下拉选择 | `options: [{value, label}]`              |
| `input`  | 文本输入 | `placeholder`, `maxLength`               |
| `number` | 数字输入 | `min`, `max`, `step`                     |
| `color`  | 颜色选择 | `presets: string[]` (可选的预设颜色列表) |

### 读取设置

```javascript
// 使用 Tapp.settings API
const refreshInterval = await Tapp.settings.get("refreshInterval");
const allSettings = await Tapp.settings.getAll();
```

Manifest 设置属于安装级配置：安装 owner 或管理员可修改；能打开该安装的运行者（含游客打开
**公开**安装）可通过 `Tapp.settings.get` / `getAll` 读取已保存值，未保存则用上表
`defaultValue`。`Tapp.storage` 是当前登录用户的私有空间，不能使用 `_settings.` 等宿主保留
前缀访问安装级设置。公开安装请勿把密钥写入 settings。

### 安装级 API 凭据 (`credentials`)

`credentials` 最多声明 16 项。每项字段为：

| 字段 | 类型 | 必填 | 限制 / 说明 |
| ---- | ---- | ---- | ----------- |
| `key` | string | ✅ | 最长 128；使用字母、数字、`.`、`_`、`-`，不能以 `.` 开头/结尾或连续使用 `.` |
| `label` | string | ✅ | 非空，最长 255；仅用于宿主管理界面 |
| `description` | string | ❌ | 最长 2000；仅用于宿主管理界面 |
| `placeholder` | string | ❌ | 最长 255；仅用于输入提示，不是默认值 |

`apis.*.credential` 必须包含已声明的 `key`，并用 `in` 选择放置方式（与密钥是否出现在请求里互斥）：

| `in` | 密钥去向 | 必填 |
| --- | --- | --- |
| `header`（省略 `in` 且声明 `header` 时的旧写法） | 固定请求头 `field`/`header`，可选 `prefix`、`encoding` | `field` 或 `header` |
| `query` | 解析后的 URL 查询参数 `field` | `field` |
| `form` | `bodyMode: form` 的表单字段 `field` | `field` |
| `sign` | 密钥不上传；宿主按 `sign` 计算并写入 `field` | `field` + `sign` |

`header` 放置不能选择 `Host`、`Content-Length`、`Connection`、`Transfer-Encoding`、`Upgrade`、
代理或其他 hop-by-hop 头，也不能在同一 API 的普通 `headers` 中再次声明同名头。`prefix` 最长
256 字节（例如 `"Bearer "`），仅用于 header。`encoding` 目前只接受 `base64`（先编码再加
prefix）。凭据值最长 16 KiB；该限制由管理 API 与宿主界面执行，值不写入 Manifest 或 `.tapp` 包。

`in: "sign"` 的 `sign` 块：

| 字段 | 说明 |
| --- | --- |
| `alg` | 白名单算法。已实现 `md5-sorted-kv`（`md5(token + sort(over).map(k+v).join(""))`）和 `hmac-sha256-raw`（`hex(HMAC-SHA256(secret, sort(over).map(k+v).join("")))`） |
| `over` | 参与签名的对象字段，1–16 个、不重复；不能包含签名字段本身 |
| `timestampField` | 可选。若声明则必须列入 `over`；宿主在签名前用当前 UNIX 秒覆盖该字段 |

签名只适用于 `json` / `form` 对象 body，不能用于 `bodyMode: raw`，且 method 必须是
`POST`/`PUT`/`PATCH`/`DELETE`（默认 `GET` 会被拒绝）。`over` 中除 `timestampField`
外的键必须出现在声明的 `body` 里，且声明值只能是字符串、数字、布尔或 `null`
（模板写在字符串里）。

需要由公开 Tapp 代站主调用第三方 API 时，使用只写 `credentials`，不要使用 `settings`：

```json
{
  "permissions": ["network:fetch"],
  "settings": [
    { "key": "userId", "label": "爱发电 user_id", "type": "input" }
  ],
  "credentials": [
    {
      "key": "wegame",
      "label": "WeGame API Key",
      "description": "用于同步站点游戏资料"
    },
    {
      "key": "afdianToken",
      "label": "爱发电 API Token"
    }
  ],
  "apis": {
    "wegameSync": {
      "type": "http",
      "access": "public",
      "endpoint": "https://api.example.com/games/{{params.userId}}",
      "credential": {
        "key": "wegame",
        "header": "Authorization",
        "prefix": "Bearer "
      }
    },
    "sponsors": {
      "type": "http",
      "access": "public",
      "method": "POST",
      "endpoint": "https://afdian.com/api/open/query-sponsor",
      "cacheTtl": 300,
      "body": {
        "user_id": "{{settings.userId}}",
        "params": "{\"page\":1,\"per_page\":20}"
      },
      "credential": {
        "key": "afdianToken",
        "in": "sign",
        "field": "sign",
        "sign": {
          "alg": "md5-sorted-kv",
          "over": ["params", "ts", "user_id"],
          "timestampField": "ts"
        }
      }
    }
  }
}
```

凭据值由安装 owner / 当前管理员在 Tapp 详情页输入。宿主复用安装 owner 的 `tapp_storage`
记录，但只把密文放入专用 `encrypted_value` 字段，并使用宿主保留的 `_credentials.` key；
沙箱 storage API 无法读取、列举、覆盖或清除此类记录。
读取接口只返回 `configured`、需否重新授权、固定目标 origin 以及每条绑定的 method/path/
`access`/放置方式，永远不返回明文或密文；沙箱没有 credential 读取 API。宿主仅在执行绑定的
具名 HTTP API 时按声明把值放入请求头、query、form，或只用来签名。

绑定凭据的 endpoint 必须使用固定绝对 HTTPS origin，host 不能模板化；凭据也不能出现在
endpoint、headers、body 或 `inject` 模板中。凭据声明及所有使用它的 API 定义共同参与授权
指纹：endpoint、`in`/`field`/`header`/`prefix`/`sign`、`access` 等发生变化后，旧凭据停止
使用，owner 必须重新输入。一个凭据必须至少绑定一个 HTTP API。目标服务本身可能收到凭据，
因此 owner 只应授权可信声明；第三方返回内容仍按不可信输入处理。宿主会在响应进入缓存或返回
沙箱前，对原样凭据及编码/带前缀形式做文本层和解析后 JSON 字符串/键双重脱敏；无法通用识别
第三方主动生成的哈希等派生表示，因此这不能替代可信目标与最小权限的 API Key。

HTTP 模板只读前缀：`user.*`、`geo.*`、`params.*`、`time.unix` / `time.unixMs` /
`time.iso8601` / `time.nonce`、`settings.{声明键}`。`settings.*` 取安装级已保存值或
Manifest `defaultValue`，不是调用方参数。`{{secrets.*}}` 仍然 fail closed。

`credentials[].key` 不得与 `settings[].key` 重名，避免同一名称同时存在公开可读值和宿主私密值。

`access: "public"` 只表示 API 可由游客调用，不自动授予游客出站权限。Manifest 仍须申请
`network:fetch`，安装时须批准，而且站点的游客 `network:fetch` 策略也须开启（默认关闭）；
否则公开安装中的游客调用会被拒绝。已登录用户同样受其角色权限策略与安装批准集约束。

从旧 settings 迁移时，先删除 Manifest 中旧 settings 声明并发布更新，再在第三方后台轮换已经
暴露的 Key，最后写入同名 credential。保存新凭据时，宿主会在同一事务中删除遗留数据库里的
`_settings.{key}` 值，但不会自动沿用可能已泄露的旧值。

---

## API 声明 (`apis`)

声明 Tapp 需要调用的外部或内置 API。每个键是沙箱调用时使用的 API 名称，后端统一执行权限校验、模板注入、SSRF 防护和可选缓存。

```json
{
  "apis": {
    "data": {
      "type": "http",
      "access": "protected",
      "endpoint": "https://api.example.com/data?city={{city}}",
      "method": "GET",
      "headers": { "X-Region": "{{params.region}}" },
      "cacheTtl": 60,
      "spoof": "china",
      "description": "获取数据",
      "inject": { "city": "{{geo.city}}" }
    },
    "summarize": {
      "type": "builtin",
      "access": "protected",
      "builtin": "ai:generate",
      "description": "生成摘要"
    }
  }
}
```

### API 声明字段

| 字段          | 类型   | 必填 | 说明                                              |
| ------------- | ------ | ---- | ------------------------------------------------- |
| `type`        | string | ❌   | `http`（默认）或 `builtin`                        |
| `access`      | string | ❌   | 调用者范围：`protected`（默认，需登录）、`public`（游客也可调用）或 `manager`（仅安装 owner / 当前管理员）；**不**表示可否免 `network:fetch` |
| `endpoint`    | string | HTTP | HTTP URL，可使用 `{{params.*}}` 等模板            |
| `method`      | string | ❌   | HTTP 方法，默认 `GET`；仅接受大写的 `GET`/`HEAD`/`POST`/`PUT`/`DELETE`/`CONNECT`/`OPTIONS`/`TRACE`/`PATCH` |
| `headers`     | object | ❌   | 请求头模板                                        |
| `credential`  | object | ❌   | 安装级凭据绑定；`in` 为 `header` / `query` / `form` / `sign`（互斥）。旧清单 `{key, header, prefix?}` 仍视为 header |
| `bodyMode`    | string | ❌   | `json`（默认）、`raw` 或 `form`                   |
| `body`        | any    | ❌   | 按 `bodyMode` 解析的请求体模板                    |
| `builtin`     | string | 内置 | `geo`、`ai:chat` 或 `ai:generate`                 |
| `inject`      | object | ❌   | 将宿主模板值映射为可复用别名                      |
| `cacheTtl`    | number | ❌   | 响应缓存秒数；缓存按 Tapp、用户、客户端上下文隔离 |
| `spoof`       | string | ❌   | 区域伪装：`china`/`japan`/`us`/`korea`/`taiwan`/`hongkong`（及别名，见下表） |
| `description` | string | ❌   | API 描述                                          |
| `route`       | object | ❌   | 入站挂载。有此字段时必须含 HMAC `verify`，且 `access` 必须是 `public` |

`bodyMode` 控制模板解析后的最终请求体字节：

- `json`：默认模式，接受任意 JSON 值；未声明 `Content-Type` 时使用
  `application/json`，与旧版行为一致。
- `raw`：Manifest 中的 `body` 和运行时模板解析结果都必须是字符串。宿主直接发送
  字符串的 UTF-8 字节，不增加 JSON 引号、转义或末尾换行，也不自动设置
  `Content-Type`；模板外的空格和换行原样保留，无法解析的模板会直接报错。这适用于纯文本、XML、Webhook，
  以及需要对最终 payload 字节计算摘要或签名的接口。
- `form`：`body` 必须是对象，字段值只能是字符串、数字、布尔值、`null` 或最终解析为这些类型的模板；
  `null` 编码为空字符串。宿主使用 `application/x-www-form-urlencoded` 编码，未声明
  `Content-Type` 时自动设置相应请求头。字段顺序不属于 Manifest 契约，接收方应将表单视为无序键值集合；
  如果目标接口的签名或摘要依赖固定字段顺序，请改用 `raw` 明确提供最终请求体。

`raw` 和 `form` 只允许 `POST`、`PUT`、`PATCH`、`DELETE`，并在序列化完成后按
UTF-8/编码后的实际字节检查 1 MiB 请求体上限；默认 `json` 模式不新增这一兼容性限制。Manifest 已声明的 `headers.Content-Type`
始终保留；它不会改变 `bodyMode` 的序列化规则。`raw` 是精确 UTF-8 文本模式，不用于发送任意二进制字节。

```json
{
  "apis": {
    "submitUrls": {
      "type": "http",
      "access": "protected",
      "endpoint": "https://example.com/api",
      "method": "POST",
      "bodyMode": "raw",
      "headers": {
        "Content-Type": "text/plain; charset=utf-8"
      },
      "body": "{{params.body}}"
    }
  }
}
```

`inject` 的键是新别名，值是宿主上下文模板。例如
`{"city":"{{geo.city}}"}` 会创建 `{{city}}`，供 `endpoint`、`headers` 或 `body`
复用；精确引用会保留数字、布尔值等 JSON 类型。别名不能覆盖 `user.*`、`geo.*`、
`params.*`、`time.*` 或 `settings.*`。Tapp 不提供 `secrets.*` 模板；Manifest 中出现宿主 secret 引用会在安装时被拒绝。
HTTP API 必须声明 `endpoint`，查询参数直接写在 URL 中；
内置 API 只接受 `geo`、`ai:chat`、`ai:generate`，不能混入 HTTP 字段。
AI 内置 API 除对应 `ai:*` 权限外，还必须在 `manifest.ai` 中以 `protocolVersion: 2` 声明相同 operation 和
`text` output；模型层级取自该 AI 声明。调用仍进入统一 AI Task registry、并发限制和持久配额
账本，不是独立的模型直连入口。
单个 Manifest 最多声明 64 个 API，每个 API 最多声明 32 个注入别名，`cacheTtl` 上限
为 86400 秒。

HTTP endpoint 在请求前解析并钉扎全部公网 DNS 地址，禁止自动重定向、URL credentials 与
Host/Connection 等路由或 hop-by-hop 请求头；响应体以流式方式强制限制为 2 MiB。Scheduler
`fetch` 使用相同边界，不能通过 DNS rebinding、跳转或超大响应绕过宿主。

### 区域伪装 (`spoof`)

用于绕过地区限制，自动添加对应地区的请求头：

| 代码                     | 地区     |
| ------------------------ | -------- |
| `china` / `cn`           | 中国大陆 |
| `japan` / `jp`           | 日本     |
| `us` / `usa` / `america` | 美国     |
| `korea` / `kr`           | 韩国     |
| `taiwan` / `tw`          | 台湾     |
| `hongkong` / `hk`        | 香港     |

### 使用示例

```javascript
// 调用已声明的 API
const response = await Tapp.api("data", { region: "jp" });
const summary = await Tapp.api("summarize", { prompt: "总结这些数据" });
```

> `Tapp.api(name, params)` 只能调用当前解析到的 manifest 的 `apis[name]`。可见安装与
> `resolve_accessible_tapp` 相同：viewer 有私有副本时用私有 Manifest，否则用站点公开版。
> 声明解析缓存键包含 owner 和 `apis` 内容指纹，其他副本更新 Manifest 后不会继续执行旧定义。
> 响应缓存还包含 owner、当前用户/角色、客户端上下文、API 定义指纹和参数摘要，不会跨安装或
> 旧 endpoint 复用；进程内解析、响应和 Geo 缓存均有 TTL 与容量回收。
>
> 所有 `type: http` 的声明 API 都需要安装已授予 `network:fetch`；`access: public` 只放宽
> 调用者范围（游客可调），**不能**代替 `network:fetch`。`access: protected` 额外要求登录主体。
> `access: manager` 只允许安装 owner 或当前站点管理员调用。

### 入站路由 (`apis.*.route`)

给其他程序调用的稳定 HTTP 面。沙箱仍走 `Tapp.api`；`route` 把同一条声明挂到：

`GET|POST /tapi/{tappId}{path}`

有 `route` 就必须声明 HMAC `verify`，且该 API 的 `access` 必须是 `public`。
`protected` / `manager` 和 `ai:*` builtin 不能挂入站。没有 `route` 的 API 行为不变。

```json
{
  "route": {
    "path": "/sponsors",
    "methods": ["GET"],
    "verify": {
      "key": "inboundSecret",
      "alg": "hmac-sha256-raw",
      "header": "X-Signature",
      "prefix": "sha256=",
      "over": "canonical-query",
      "timestampHeader": "X-Timestamp",
      "nonceHeader": "X-Nonce",
      "maxSkewSecs": 300
    }
  }
}
```

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `path` | 是 | `/` + 1–64 位字母数字/`_`/`-`，Manifest 内唯一 |
| `methods` | 否 | 仅 `GET` / `POST`，默认 `["GET"]`。`HEAD` 不按 GET 验签，宿主直接拒绝 |
| `verify.key` | 是 | 顶层 `credentials[].key`，可只绑入站 |
| `verify.alg` | 是 | 仅 `hmac-sha256-raw` |
| `verify.header` | 是 | `X-` 头；禁止会话头和代理头（`X-CSRF-Token`、`X-Tapp-Runtime-Grant`、`X-Forwarded-*`、`X-Real-IP`、`X-Request-Id` 等） |
| `verify.prefix` | 否 | 如 `sha256=` |
| `verify.over` | 是 | GET 用 `canonical-query`；POST 用 `raw-body` |
| `verify.encoding` | 否 | `hex`（默认）或 `base64` |
| `verify.timestampHeader` / `nonceHeader` | 是 | 与签名头互不相同的 `X-` 头 |
| `verify.maxSkewSecs` | 否 | 默认 300，范围 30–3600 |

签名材料（UTF-8，LF）：

```text
{METHOD}\n/tapi/{tappId}{path}\n{TIMESTAMP}\n{NONCE}\n{PAYLOAD}
```

GET 的 PAYLOAD 是解码后按键排序的 `k=v&k2=v2`（重复键拒绝），并作为 `params`。
POST 的 PAYLOAD 是原始 body，超过 1 MiB 先拒；`params` 只来自身体（JSON 对象或 form），
**未签名的 query 不会并入、也不能覆盖** 已签名字段。
时间窗内同一 nonce 只能用一次。调用方必须自备 16–128 位 `[A-Za-z0-9_-]` nonce。

入站 **不** 使用 Runtime Grant，也 **不** 查看游客 `network:fetch` 策略；HTTP 出站只要求该公开安装已批准 `network:fetch`。`/tapi` 忽略站点登录 Cookie，只解析 `visibility = all` 的公开安装。
没有 `GET /tapi/{tappId}` 目录；验签参数写在 Manifest 里交给调用方，不由宿主对外广播本机装了哪些路由。
入站密钥泄露后，owner 在详情页重填即可作废旧指纹；宿主另有每小时 180 次的凭据封顶。
验签失败过多会按调用方指纹自动拉黑（不落原始 IP）；详情页可暂停该安装的入站或解除本安装拉黑。暂停/拉黑按安装 owner 隔离。

---

## 跨 Tapp 数据契约 (`dataExchange`)

Tapp 私有 storage、报告和内部状态不会因为知道另一个 `tappId` 而开放。提供方必须声明
具名 `exports`，调用方必须声明匹配的 `imports`；声明只表示接口兼容，每次真实调用仍会
进入宿主授权队列，并显示包含双方 Tapp、请求范围、用途、返回上限和过期时间的“仅本次”
授权弹窗。

```json
{
  "dataExchange": {
    "exports": [
      {
        "id": "playlist.current",
        "description": "当前播放列表",
        "maxBytes": 262144,
        "maxRecords": 200,
        "schema": {
          "type": "array",
          "maxItems": 200,
          "items": {
            "type": "object",
            "required": ["id", "title"],
            "properties": {
              "id": { "type": "string" },
              "title": { "type": "string", "maxLength": 200 },
              "artist": { "type": "string", "maxLength": 200 }
            },
            "additionalProperties": false
          }
        }
      }
    ],
    "imports": [
      {
        "tappId": "com.example.player",
        "exportId": "playlist.current"
      }
    ]
  }
}
```

约束：

- 每个方向最多 32 条声明；export ID 最长 128 字节，只允许字母、数字、`_-.`；
- `maxBytes` 为 1–524288，`maxRecords` 可选且为 1–10000；
- `schema` 必须是最多 64 KiB 的内联对象，当前支持 `type`、`properties`、`required`、
  `additionalProperties: false`、`items`、`min/maxItems`、`min/maxLength`、
  `minimum/maximum`、`enum` 和 `const`；不支持 `$ref` 或外部 schema；
- 响应失败、超限或 schema 不匹配同样会耗尽一次性 Grant，不能修改参数后重放；
- 相同 Tapp 内部读取应使用自己的私有 API，不走跨 Tapp 交换。

运行时用法见 [Data Exchange API](API_REFERENCE.md#跨-tapp-data-exchange-api)。

---

## AI、Event 与 Agent Interaction 声明

```json
{
  "permissions": ["ai:generate", "event:publish", "event:subscribe"],
  "ai": {
    "protocolVersion": 2,
    "operations": ["generate", "chat"],
    "modelTier": "standard",
    "contextSources": ["platform", "report", "profile", "custom"],
    "outputFormats": ["text", "json"]
  },
  "events": {
    "publish": ["tapp.com.example.my-tapp.status.changed"],
    "subscribe": [
      "system.theme.changed",
      "tapp.com.example.player.track.changed"
    ]
  },
  "agent": {
    "protocolVersion": 2,
    "interactions": [
      {
        "type": "report.compose",
        "inputSchema": "schemas/report-input.json",
        "resultSchema": "schemas/report-result.json"
      }
    ],
    "intents": ["ui.open", "report.create", "dataExchange.request"]
  }
}
```

- AI operation 必须同时声明匹配的 `ai:*` 权限；模型供应商、模型名和生成参数不进入 Manifest；
- Event publish topic 必须位于 `tapp.<当前 id>.*`；Tapp 不能发布 `system.*`；每个方向最多
  100 个 topic；
- `system.*` 只能由宿主发布；当前提供 theme、network、locale、visibility 和 navigation
  状态变更 producer；
- Event `owner` 作用域只允许有界状态元数据，跨 Tapp 正文必须使用 `dataExchange`；
- Agent interaction type 最多 32 个。schema 是安装根目录内的 JSON 资源，安装时校验存在，
  运行时限制为 64 KiB、禁止 `$ref`，输入和结果都由后端验证；
- Interaction type 由应用自行命名，但必须与 Agent 能选择的任务类型一致；Tapp 不会获得任意
  DOM 操作权限。

---

## 权限列表

权限等级与运行时边界见 [架构文档的权限模型](./ARCHITECTURE.md#权限模型)。Manifest
中的权限仍需经过安装授权；“基础”不表示 Tapp 可以省略申请。

### 基础权限

| 权限                 | 说明             |
| -------------------- | ---------------- |
| `storage:read`       | 读取本地数据存储 |
| `ui:notification`    | 显示通知         |
| `ui:theme`           | 读取主题信息     |
| `ui:confirm`         | 显示确认对话框   |
| `ui:openUrl`         | 打开 Manifest `openUrls` 声明的链接（宿主代开；未声明一律拒绝） |
| `ui:fullscreen`      | 请求全屏显示     |
| `platform:read`      | 读取平台数据     |
| `analytics:read`     | 读取站点访问统计（聚合；admin 完整 summary，非 admin 仅访客卡片） |
| `tappList:read`      | 读取 Tapp 列表   |
| `brew:read`          | 读取 Brew 内容   |
| `brew:write`         | 修改 Brew 状态   |
| `brew:comment`       | 操作 Brew 评论   |
| `report:read`        | 读取报告         |
| `media:read`         | 读取媒体状态     |
| `media:control`      | 控制媒体播放     |
| `media:audio`        | 播放包内/blob/data 音频 |
| `event:subscribe`    | 订阅声明的 topic |
| `federation:read`    | 读取联邦数据     |
| `federation:write`   | 联邦个人操作     |
| `federation:message` | 联邦消息         |
| `federation:files`   | 联邦文件传输     |
| `game:session`       | 游戏房间会话（`Tapp.game`；仍需对应联邦权限） |

### 提升权限（管理员可配置下放）

| 权限                 | 说明              |
| -------------------- | ----------------- |
| `ai:generate`        | AI 文本生成       |
| `ai:analyze`         | AI 数据分析       |
| `ai:chat`            | AI 对话           |
| `ai:image`           | AI 图片生成       |
| `network:fetch`      | 发送 HTTP 请求    |
| `component:theme`    | 注册自定义主题    |
| `shortcut:register`  | 注册键盘快捷键    |
| `event:publish`      | 发布本 Tapp topic |
| `scheduler:register` | 注册定时任务      |
| `speech:tts`         | 文本转语音        |
| `speech:asr`         | 语音转文本        |
| `storage:write`      | 写入本地数据存储  |

`brew:write` 与 `brew:comment` 描述的是 Tapp 能力，不按宿主用户角色下放。Tapp 仍必须在
Manifest 中声明并在安装时获授；实际读写始终落在当前会话可访问的 Brew 数据范围内。

“基础”表示不需要管理员额外下放 elevated 权限，不等于匿名访客一定可用。访客没有持久
用户主体时，部分能力仍可通过签名游客 session 使用（如 `storage:read`、`platform:read`、
`analytics:read`）。下列能力的真实后端路由仍要求登录：`brew:write`、
`brew:comment`、`report:read`、`ui:notification` 等。

`component:theme`、`shortcut:register`、`scheduler:register`、`speech:tts` 与 `speech:asr`
也要求持久登录主体，不会下放给匿名访客；管理配置中的旧字段仅为兼容历史配置而保留，
并始终按关闭处理。

### 特权权限

| 权限                | 说明           |
| ------------------- | -------------- |
| `widget:register`   | 动态注册小组件 |
| `platform:write`    | 写入平台数据   |
| `platform:register` | 注册自定义平台 |
| `component:agent`   | 注册 AI Agent  |
| `tappList:manage`   | 管理 Tapp      |
| `brew:manage`       | 管理 Brew      |
| `federation:trust`  | 管理联邦信任   |
| `report:write`      | 创建/修改报告  |
