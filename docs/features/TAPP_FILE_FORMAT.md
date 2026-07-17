# `.tapp` 文件格式

`.tapp` 是 ZIP 格式的 Tapp 安装包。当前文件安装入口是
`POST /api/tapps/install-file`，multipart 文件字段名为 `file`。

开发模型与运行时边界见 [Tapp 架构](../development/tapp/ARCHITECTURE.md)，完整字段见
[Manifest 配置](../development/tapp/MANIFEST.md)。

## 最小包

```text
com.example.app.tapp
├── manifest.json
└── main.js
```

`manifest.json`：

```json
{
  "id": "com.example.app",
  "name": "Example App",
  "version": "1.0.0",
  "description": "示例应用",
  "category": "utility",
  "main": "main.js",
  "permissions": []
}
```

`category` 在安装时必填，取值见 [Manifest · 应用分类](../development/tapp/MANIFEST.md#应用分类)。
`main` 是相对包根目录的入口路径。安装器会校验该路径并确认解包后文件真实存在，
不会再默认猜测一个不存在的 `main.js` 或 `index.js`。

## 完整结构示例

```text
com.example.app.tapp
├── manifest.json
├── src/
│   └── main.js
├── styles.css
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
  "main": "src/main.js",
  "cssMode": "separated",
  "styles": "styles.css",
  "widgetStyles": "widget.css",
  "pageStyles": "page.css",
  "pageTemplate": "page.html",
  "pageModules": ["state.js", "helpers.js", "index.js"],
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
- Manifest 中的 `main`、CSS、Page template、Page modules 和 Widget templates
  都会校验；
- ZIP 内不符合规则的条目会使安装失败，而不是静默写到包根目录之外。

不要依赖 ZIP 中的符号链接或平台特定路径语义。

为避免压缩炸弹和歧义覆盖，上传包限制为 25 MiB、最多 512 个条目、单文件最多
25 MiB、总解压量最多 100 MiB，`manifest.json` 最多 256 KiB；重复 ZIP 路径会被
拒绝。导出只包含安装目录内的普通文件，不跟随符号链接。

## 代码与资源如何进入运行时

包内目录结构不等于浏览器可直接访问的静态站点：

- `main` 由后端读取后交给 ResourceLoader 拆分为 core/widget/page；
- `pageTemplate` 和 Widget `templates` 以字符串注入 sandbox iframe；
- `styles`、`widgetStyles`、`pageStyles` 和预生成 CSS 会按模式组合；
- `i18n/*.json` 以 `window._TAPP_I18N` 数据注入；
- `page/*.js` 按 `pageModules` 顺序组合；省略顺序时按文件名排序并把 `index.js`
  放到最后。

### 包内 `assets/`

静态资源通过 Manifest `assets` 声明（路径必须在 `assets/` 下），安装后由沙箱 SDK
`Tapp.assets` 读取，后端入口为 `GET /api/tapps/{tappId}/asset?path=...`（返回 base64，
SDK 在 iframe 内转为 `blob:` / `data:`）。

- 字段、数量与体积上限见 [Manifest 配置](../development/tapp/MANIFEST.md)（单文件 ≤ 5 MiB，
  合计 ≤ 20 MiB，最多 64 项；禁止 `.js` / `.html` 作为 asset）。
- 使用方式与 Canvas/音频示例见 [图形与轻量游戏](../development/tapp/GRAPHICS.md)。
- 任意放入 `assets/` 但未写入 `manifest.assets` 的文件不会暴露给运行时。
- 不要假设 `<img src="assets/a.png">` 会直接读后端安装目录；应使用
  `Tapp.assets.getUrl(...)` 得到的 URL，或允许的远程 / `data:` / `blob:` 源。

## CSS 模式

### unified

```json
{
  "cssMode": "unified",
  "styles": "styles.css"
}
```

共享样式由 Widget 和 Page 使用；宿主可按源码生成各模式需要的 Tailwind CSS。

### separated

```json
{
  "cssMode": "separated",
  "styles": "shared.css",
  "widgetStyles": "widget.css",
  "pageStyles": "page.css"
}
```

`styles` 是可选共享层，专用 CSS 只进入对应模式。商店已提供 separated CSS 时，安装器
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

## Page 模块

`pageModules` 声明的是 `page/` 目录内的文件名和执行顺序：

```json
{
  "hasPage": true,
  "pageModules": ["state.js", "helpers.js", "index.js"]
}
```

存在 Page 模块时，它们是 Page 逻辑来源；`main` 仍是 Widget/core 的安装入口和兼容
回退。后台 headless 模式只运行 core，不运行 Page 模块。

## 安装流程

1. 浏览器获取 CSRF token 并上传 multipart；
2. 后端读取并反序列化 `manifest.json`；
3. 校验 `minSystemVersion`，当前 Myriad 版本过低时立即拒绝；
4. 校验 ID、所有 Manifest 路径和 Widget 模板路径；
5. 检查同 owner 下是否已安装；
6. 安全解包并保留合法相对目录；
7. 确认 `manifest.main` 文件存在；
8. 按当前实时角色和动态权限配置过滤最终授权；
9. 把 Manifest/状态/授权写入 PostgreSQL，把资源保存在 owner 目录；
10. 前端下一次同步加载实例并补注册 Manifest Widget。

## 导出与往返保证

`GET /api/tapps/{tappId}/export` 会递归打包该安装目录并保留相对路径。导出的包应能再次
通过 `install-file` 安装，且以下内容不能静默丢失：

- `manifest.main` 及其文件；
- Manifest 未知于 UI 但已纳入后端结构的字段；
- Widget 完整元数据和 templates；
- nested CSS/Page/i18n/template 文件；
- `backgroundRequirements`、`pageModules` 和 `apis`。

修改包格式后至少运行后端 Manifest 定向测试、Tapp 定向测试、前端 build check，并做
一次真实的导出 ZIP 文件清单检查。

## 常见错误

| 现象                            | 原因                                    | 修正                                    |
| ------------------------------- | --------------------------------------- | --------------------------------------- |
| 安装返回 `Main entry not found` | `manifest.main` 与 ZIP 文件不一致       | 修正路径和大小写                        |
| Widget 模板为空                 | `templates` 路径不存在或尺寸 key 不匹配 | 核对 Manifest 与 ZIP 清单               |
| Page 模块顺序错误               | `pageModules` 遗漏/拼写错误             | 显式声明顺序                            |
| 样式被覆盖                      | separated 资源被当成 unified 重新生成   | 设置 `cssMode: separated`               |
| 本地图片 404                    | 未走 `Tapp.assets` 或未声明 `manifest.assets` | 用 `Tapp.assets.getUrl` 或受支持 URL/data/blob |
| 权限少于 Manifest               | 当前用户角色不允许全部申请权限          | 以 `granted_permissions` 为准           |
