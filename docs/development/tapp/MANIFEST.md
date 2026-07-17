# Manifest 配置

Manifest 是 Tapp 的核心配置文件，定义了应用的元数据、权限和功能。

## 基础字段

| 字段                     | 类型     | 必填 | 说明                               |
| ------------------------ | -------- | ---- | ---------------------------------- |
| `id`                     | string   | ✅   | 唯一标识符，推荐使用反向域名格式   |
| `name`                   | string   | ✅   | 应用名称                           |
| `version`                | string   | ✅   | 版本号（语义化版本）               |
| `description`            | string   | ❌   | 应用描述                           |
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

`author.name` 必填；`author.email` 与 `author.url` 可选。作者名称会显示在商店卡片和 Tapp
详情页，详情页还会显示邮箱，并为通过 HTTP(S) 校验的作者主页生成外部链接。

所有资源路径都是相对安装根目录的安全路径。`.tapp` 文件安装会保留经过校验的嵌套
目录，例如 `templates/widget-2x2.html`；direct/store 安装也会把内容写到 Manifest
声明的位置。绝对路径、隐藏组件和 `..` 会被拒绝。`pageModules` 的每项是 `page/`
目录内的文件名，不能再次包含目录前缀。`main` 必须是 `.js`，样式路径必须是 `.css`，
Page/Widget 模板必须是 `.html`；代码与模板类声明资源必须是安装目录内的普通 UTF-8 文本
文件。`assets` 允许二进制（贴图、音频、wasm、JSON 关卡等），路径必须位于 `assets/`
下，且不得使用 `.js` / `.html` 扩展名；单文件 ≤ 5 MiB，合计 ≤ 20 MiB，最多 64 项。
资源读取不会跟随安装后插入的符号链接。运行时通过 `Tapp.assets` 读取，详见
[图形与轻量游戏](GRAPHICS.md)。

Manifest 采用严格字段校验：未声明字段、拼写错误以及已经移除的字段都会让安装失败，
不会再被静默忽略。所有运行能力都必须直接写入 `permissions`；宿主只会在真正调用时
按权限和运行时策略决定是否授权。

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
    公开安装时，每个已登录用户仍读写自己的私有 storage，不会读取站点 owner 的数据。
  - Manifest 声明的安装级设置（含沙箱 `_settings.*` 与宿主 settings 路由）挂在
    **installation owner** 命名空间：owner 或管理员可写，其他已登录运行者只读声明过的键。
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
    "storage",
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

`refreshPolicy.mode` 默认为事件驱动语义：同一 Tapp 的其他运行实例发生 storage 变更时
宿主会通知并刷新 Widget；当前 Widget 可用 `Tapp.widget.invalidate()` 显式请求刷新。
确实需要轮询时可设为 `interval` 并提供
`intervalSeconds`（15–86400 秒）；计时器仅在页面和 Widget 可见且 Tapp 运行时工作。
`refreshOnVisible` 默认为 `true`。后台同步应使用 scheduler/headless core，而不是依赖
Widget 的可见计时器。

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

Manifest 设置属于安装级配置：安装 owner 或管理员可修改，运行该安装的已登录用户可以读取。
`Tapp.storage` 是当前用户的私有空间，不能使用 `_settings.` 等宿主保留前缀访问安装级设置。

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
| `access`      | string | ❌   | 调用者范围：`protected`（默认，需登录）或 `public`（游客也可调用）；**不**表示可否免 `network:fetch` |
| `endpoint`    | string | HTTP | HTTP URL，可使用 `{{params.*}}` 等模板            |
| `method`      | string | ❌   | HTTP 方法，默认 `GET`                             |
| `headers`     | object | ❌   | 请求头模板                                        |
| `body`        | object | ❌   | JSON 请求体模板                                   |
| `builtin`     | string | 内置 | `geo`、`ai:chat` 或 `ai:generate`                 |
| `inject`      | object | ❌   | 将宿主模板值映射为可复用别名                      |
| `cacheTtl`    | number | ❌   | 响应缓存秒数；缓存按 Tapp、用户、客户端上下文隔离 |
| `spoof`       | string | ❌   | 区域伪装：`china`、`japan` 或 `us`                |
| `description` | string | ❌   | API 描述                                          |

`inject` 的键是新别名，值是宿主上下文模板。例如
`{"city":"{{geo.city}}"}` 会创建 `{{city}}`，供 `endpoint`、`headers` 或 `body`
复用；精确引用会保留数字、布尔值等 JSON 类型。别名不能覆盖 `user.*`、`geo.*` 或
`params.*`。Tapp 不提供 `secrets.*` 模板；Manifest 中出现宿主 secret 引用会在安装时被拒绝。
HTTP API 必须声明 `endpoint`，查询参数直接写在 URL 中；
内置 API 只接受 `geo`、`ai:chat`、`ai:generate`，不能混入 HTTP 字段。
AI 内置 API 除对应 `ai:*` 权限外，还必须在 `manifest.ai` 中声明 V2 的相同 operation 和
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
| `storage`            | 本地数据存储     |
| `ui:notification`    | 显示通知         |
| `ui:theme`           | 读取主题信息     |
| `ui:confirm`         | 显示确认对话框   |
| `ui:fullscreen`      | 请求全屏显示     |
| `platform:read`      | 读取平台数据     |
| `tappList:read`      | 读取 Tapp 列表   |
| `brew:read`          | 读取 Brew 内容   |
| `brew:write`         | 修改 Brew 状态   |
| `brew:comment`       | 操作 Brew 评论   |
| `report:read`        | 读取报告         |
| `media:read`         | 读取媒体状态     |
| `media:audio`        | 播放包内/blob/data 音频 |
| `event:subscribe`    | 订阅声明的 topic |
| `federation:read`    | 读取联邦数据     |
| `federation:write`   | 联邦个人操作     |
| `federation:message` | 联邦消息         |
| `federation:files`   | 联邦文件传输     |

### 提升权限（管理员可配置下放）

| 权限                 | 说明              |
| -------------------- | ----------------- |
| `ai:generate`        | AI 文本生成       |
| `ai:analyze`         | AI 数据分析       |
| `ai:chat`            | AI 对话           |
| `ai:image`           | AI 图片生成       |
| `network:fetch`      | 发送 HTTP 请求    |
| `media:control`      | 控制媒体播放      |
| `component:theme`    | 注册自定义主题    |
| `shortcut:register`  | 注册键盘快捷键    |
| `event:publish`      | 发布本 Tapp topic |
| `scheduler:register` | 注册定时任务      |
| `speech:tts`         | 文本转语音        |
| `speech:asr`         | 语音转文本        |

`brew:write` 与 `brew:comment` 描述的是 Tapp 能力，不按宿主用户角色下放。Tapp 仍必须在
Manifest 中声明并在安装时获授；实际读写始终落在当前会话可访问的 Brew 数据范围内。

“基础”表示不需要管理员额外下放 elevated 权限，不等于匿名访客一定可用。访客没有持久
用户主体，因此不会获得 `storage`、`platform:read`、`brew:write`、
`brew:comment`、`report:read` 或 `ui:notification`；这些能力的真实后端路由均要求登录。

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
