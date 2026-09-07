# `.tapp` 文件格式

`.tapp` 是 ZIP 格式的 Tapp 安装包。安装来源有三条，最终安装态相同：`POST /api/tapps/install`
（`source=direct` 或 `source=store`），以及本文件描述的文件安装 `POST /api/tapps/install-file`
（multipart 字段名 `file`）。不存在 `/api/tapp-store` 路由。

开发模型与运行时边界见 [Tapp 架构](../development/tapp/ARCHITECTURE.md)，完整字段见
[Manifest 配置](../development/tapp/MANIFEST.md)。远程目录安装（非 ZIP）见
[Tapp 商店](../development/tapp/STORE.md)。

## 最小包

```text
com.example.app.tapp
├── manifest.json
└── core.js
```

`manifest.json`：

```json
{
  "id": "com.example.app",
  "name": "Example App",
  "version": "1.0.0",
  "description": "示例应用",
  "category": "utility",
  "core": { "entry": "core.js" },
  "permissions": []
}
```

`category` 在安装时必填，取值见 [Manifest · 应用分类](../development/tapp/MANIFEST.md#应用分类)。

包按**层**组织：`core` 是三层共享层，`page` 是页面层，`widgets[]` 各自一层。每层用
`entry` 声明自己的 `.js` 入口（相对包根目录），层内其余文件由入口用相对路径 `require`
进来，不需要在 Manifest 里逐个声明。安装器校验声明的层入口解包后真实存在，没有任何
按文件名猜测的回退。

至少要声明 `core`、`page`、`widgets` 之一。声明了 `backgroundRequirements` 的应用必须
有 `core`——常驻只运行 core。

## 完整结构示例

```text
com.example.app.tapp
├── manifest.json
├── src/
│   └── core.js
├── styles.css
├── widget/
│   └── index.js
├── widget.css
├── page.css
├── page.html
├── templates/
│   ├── widget-2x2.html
│   └── widget-4x2.html
├── assets/
│   ├── icon.png
│   └── level.json
├── i18n/
│   ├── zh-CN.json
│   ├── en-US.json
│   └── ja-JP.json
└── page/
    ├── state.js
    ├── helpers.js
    └── index.js
```

对应 Manifest 片段：

```json
{
  "category": "utility",
  "core": { "entry": "src/core.js", "styles": "styles.css" },
  "page": {
    "entry": "page/index.js",
    "template": "page.html",
    "styles": "page.css"
  },
  "assets": ["assets/icon.png", "assets/level.json"],
  "widgets": [
    {
      "id": "summary",
      "name": "摘要",
      "defaultSize": "2x2",
      "sizes": ["2x2", "4x2"],
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
      "refreshPolicy": { "mode": "event", "refreshOnVisible": true }
    }
  ]
}
```

经过安全校验的嵌套目录会在安装和导出时保留，保证“安装 → 导出 → 再安装”不会因
路径被拍平而丢失 Page 模块或 Widget 模板。

`widgets[].settings` 描述每个 Dashboard 实例独立保存的配置；它与 Manifest 顶层、整个
Tapp 共享的 `settings` 不同。`refreshPolicy` 采用事件优先策略，可选的 interval 只会在
Widget 可见时运行，后台任务仍由 scheduler/headless core 承担。

## 路径规则

Tapp ID 和 Manifest 资源路径用于构造安装目录，必须遵守严格规则：

- 只允许相对路径；
- 不允许 `..`、绝对路径或反斜杠；
- 每个路径组件只允许 ASCII 字母、数字、点、下划线和连字符；
- 不允许以点开头的隐藏路径；
- Manifest 中的层入口、层样式、Page template 和 Widget templates 都会校验；
- ZIP 内不符合规则的条目会使安装失败，而不是静默写到包根目录之外。

不要依赖 ZIP 中的符号链接或平台特定路径语义。

为避免压缩炸弹和歧义覆盖，普通包上传限制为 64 MiB、最多 1024 个条目、单文件最多
64 MiB、总解压量最多 128 MiB；`category` 为 `game` / `developer` 且声明了 `game`
或 `runtimeModules` 时放宽到 128 MiB / 2048 个条目 / 单文件 128 MiB / 总解压
256 MiB。`manifest.json` 最多 256 KiB；重复 ZIP 路径会被拒绝。导出只包含安装目录内的
普通文件，不跟随符号链接。上传入口按游戏档先收包，解析 Manifest 后再按普通档回卡。

## 代码与资源如何进入运行时

包内目录结构不等于浏览器可直接访问的静态站点：

- 后端按 mode 只下发相关层的 `.js` 文件：widget 沙箱拿不到 Page 的代码；
- 宿主在生成 iframe 时把这些文件静态包装成模块工厂，`require` 由宿主在编译期解析
  （沙箱禁用了 `eval` / `Function`，模块系统不是沙箱内的运行时能力）；
- 每层执行顺序固定为 core 入口先跑、再跑该层入口；
- `page.template` 和 Widget `templates` 以字符串注入 sandbox iframe；
- 作者层样式（`core.styles` / `page.styles` / `widgets[].styles`）与宿主预编译
  Tailwind 是两条通道，后者写在 `host/` 下，不会与作者同名文件相撞；
- `i18n/*.json` 以 `window._TAPP_I18N` 数据注入。

### 包内 `assets/`

静态资源通过 Manifest `assets` 声明（路径必须在 `assets/` 下），安装后由沙箱 SDK
`Tapp.assets` 读取，后端入口为 `GET /api/tapps/{tappId}/asset?path=...`（返回 base64，
SDK 在 iframe 内转为 `blob:` / `data:`）。

- 字段、数量与体积上限见 [Manifest 配置](../development/tapp/MANIFEST.md)（默认单文件 ≤ 16 MiB，
  合计 ≤ 64 MiB，最多 128 项；禁止 `.js` / `.html` 作为 asset）。
- 使用方式与 Canvas/音频示例见 [图形与轻量游戏](../development/tapp/GRAPHICS.md)。
- 任意放入 `assets/` 但未写入 `manifest.assets` 的文件不会暴露给运行时。
- 不要假设 `<img src="assets/a.png">` 会直接读后端安装目录；应使用
  `Tapp.assets.getUrl(...)` 得到的 URL，或允许的远程 / `data:` / `blob:` 源。

## 样式的两条通道

作者样式由层声明，各层只加载 `core.styles` 加自己那层：

```json
{
  "core": { "entry": "core.js", "styles": "shared.css" },
  "page": { "entry": "page/index.js", "styles": "page.css" },
  "widgets": [{ "id": "summary", "entry": "widget/index.js", "styles": "widget.css" }]
}
```

层没有声明专用样式时，效果就是只有共享样式，不需要额外的模式开关（旧的 `cssMode`
已移除）。

宿主预编译的 Tailwind 是另一条通道，写在 `host/widget.css` 与 `host/page.css`，
不参与 Manifest 声明校验，也不会覆盖作者同名文件。商店已提供预编译 CSS 时，安装器
不能用前端按需生成结果覆盖它。

## Widget 模板

模板路径位于 `manifest.widgets[].templates`，key 是尺寸：

```json
{
  "id": "summary",
  "name": "摘要",
  "defaultSize": "2x2",
  "sizes": ["2x2", "4x2"],
  "templates": {
    "2x2": "templates/widget-2x2.html",
    "4x2": "templates/widget-4x2.html"
  }
}
```

安装/更新会保留 Widget 的 `description`、`icon`、`category`、`templates`、`settings`
和 `refreshPolicy`。
这些字段有后端 round-trip 测试保护，不能只在前端 TypeScript 类型中添加。刷新策略由
宿主按事件优先、可见 interval 的规则执行；后台周期工作仍由 scheduler/headless core
负责。顶层 `settings` 是 Tapp 全局设置，`widgets[].settings` 是 Dashboard 实例设置。
模板内容按 `Widget ID + 尺寸` 传输，同一 Tapp 的多个 Widget 可以为相同尺寸使用不同
文件。旧 `minRefreshInterval` 从未接入刷新调度，现已作为无效字段拒绝；需要轮询时使用
`refreshPolicy`。

## 层内多文件

层内想拆多个文件时，用相对路径 `require`，不需要在 Manifest 里声明清单：

```js
// page/index.js
var core = require('../core.js')
var state = require('./state.js')
```

规则：只接受字符串字面量的相对路径；不支持动态 `require`；不解析 `node_modules`；
第一版不支持 `require(json)` 与目录 `index.js`。路径逃出包根会被拒绝而不是折叠回根内，
所以根目录的文件不能用 `../`——`core.js` 里写 `require('../core.js')` 是错误，不会
悄悄命中自己。模块语义是 CommonJS 子集——同步 `require`、模块级缓存、循环依赖拿到部分
导出、每个模块顶层声明彼此隔离，跨文件共享必须走 `exports`。

层内文件不需要在 Manifest 里逐个声明，但安装时会全部受检：必须是沙箱内的普通 UTF-8
文件、不超体积上限，且每个 `require` 目标真实存在——引用了不存在的文件在装包时就失败，
不会等到打开应用。

### 推荐目录

脚手架和 Playground 把 Page 入口放在 `page/index.js`、Widget 入口放在
`widget/index.js`。这只是作者布局约定。下发范围由 manifest 声明的层入口及其
`require` 闭包决定：widget 沙箱拿不到 Page 入口闭包里的文件，也不会拿到同一个
目录里其它 widget 的源码。入口和依赖可以放在任意安全的包内路径。

core 是三层共享层，三种模式都先执行它；只有后台专属逻辑需要用
`_TAPP_MODE === 'core'` 自行守卫。后台 headless 模式只运行 core。

## 安装流程

1. 浏览器获取 CSRF token 并上传 multipart；
2. 后端读取并反序列化 `manifest.json`；
3. 校验 `minSystemVersion`，当前 Myriad 版本过低时立即拒绝；
4. 校验 ID、所有 Manifest 路径和 Widget 模板路径；
5. 检查同 owner 下是否已安装；
6. 安全解包并保留合法相对目录；
7. 确认声明的层入口文件存在；
8. 按当前实时角色和动态权限配置过滤最终授权；
9. 把 Manifest/状态/授权写入 PostgreSQL，把资源保存在 owner 目录；
10. 前端下一次同步加载实例并补注册 Manifest Widget。

## 导出与往返保证

`GET /api/tapps/{tappId}/export` 会递归打包该安装目录并保留相对路径。导出的包应能再次
通过 `install-file` 安装，且以下内容不能静默丢失：

- 每个层入口及其文件；
- Manifest 未知于 UI 但已纳入后端结构的字段；
- Widget 完整元数据和 templates；
- nested CSS/Page/i18n/template 文件；
- `backgroundRequirements` 和 `apis`。

修改包格式后至少运行后端 Manifest 定向测试、Tapp 定向测试、前端 build check，并做
一次真实的导出 ZIP 文件清单检查。

## 常见错误

| 现象                            | 原因                                    | 修正                                    |
| ------------------------------- | --------------------------------------- | --------------------------------------- |
| 安装报层入口缺失                | 层 `entry` 与 ZIP 文件不一致             | 修正路径和大小写                        |
| Widget 模板为空                 | `templates` 路径不存在或尺寸 key 不匹配 | 核对 Manifest 与 ZIP 清单               |
| 层内文件找不到                  | `require` 目标路径写错                   | 用相对路径，带上 `.js`                  |
| 样式被覆盖                      | 作者层样式与宿主预编译产物混用          | 作者样式写进层声明，宿主产物在 `host/`  |
| 本地图片 404                    | 未走 `Tapp.assets` 或未声明 `manifest.assets` | 用 `Tapp.assets.getUrl` 或受支持 URL/data/blob |
| 权限少于 Manifest               | 当前用户角色不允许全部申请权限          | 以 `granted_permissions` 为准           |
