# Playground 生成契约

本文档是 Tapp Playground 发送给 Pro 模型的精简开发上下文。完整解释仍以同目录的
`MANIFEST.md`、`API_REFERENCE.md`、`SANDBOX.md`、`STORE.md` 和 `STYLING.md` 为准
（检索目录见 `tapp_playground_knowledge.rs`）。

## 文件与运行模式

Playground 项目至少需要 **Page** 或 **Widgets** 之一（允许 Widget-only，不强制 Page）。

- `manifest.json` 描述身份、入口、资源、分类、权限和运行形态。
- **Page 模式**（`hasPage: true`）：`main.js` 可含共享 `core` 与可见 `page`；`page.html`
  只含 body 内静态语义结构；行为写在 `code.page` / `main.js` 对应段；
  `pageTemplate` 为 `page.html`。
- **Widget-only**（`hasPage: false`）：不要发明 stub 页面；UI 放在 `code.widget` 与
  `code.widgetHtml`；声明非空 `manifest.widgets` 与 `widget:register`；可省略
  `pageTemplate`，保持 `page` / `pageHtml` 为空。详见 [WIDGET.md](./WIDGET.md)。
- `styles.css` 使用普通 CSS，并通过 `var(--tapp-primary)` 读取宿主强调色
  （沙箱内没有 `--color-primary`）。
- Page 沙箱（有可用 Page 时）运行在没有 `allow-same-origin` 的 sandboxed iframe 中，
  CSP 使用每实例 nonce。Widget-only 预览不挂载 Page 沙箱。
- Canvas / WebGL / Three.js 只放在 **Page**。不要在 Widget 里跑 rAF 主循环或 3D 场景。
- 禁止输出 CDN 脚本（`unpkg` / `jsdelivr` / `cdnjs` / `esm.sh` / `threejs.org/build`）。
  沙箱 `connect-src` 只有 `blob:` / `data:`，也不能 `import` npm。Three 等库必须预打成 IIFE 写入
  `page/`，由 `manifest.pageModules` 加载；贴图和 `.glb` 走 `Tapp.assets`。
  先 `Tapp.assets.getUrlMap()`，再用 `rewriteUrl` 接 LoadingManager。
  `fetch` 只能打 blob/data。详见 [GRAPHICS.md](GRAPHICS.md)。

## 宿主展示文案 vs 应用内 i18n（勿混淆）

| 字段 | 用途 |
| ---- | ---- |
| 顶层 `manifest.name` / `description` | **兜底**标题与描述（商店/列表/详情未命中语言时） |
| `manifest.locales` | **宿主 chrome** 的多语言标题与短描述（BCP-47 → `{ name?, description? }`） |
| `catalog.json` `locales` | **仅发布到商店时**：长介绍与静态预览覆盖（`long_description` / `preview`）。Playground 不生成 catalog |
| `code.i18n` + `Tapp.i18n.t()` | **应用内 UI** 字符串（按钮、标签、提示等） |

规则：

- 始终填写顶层 `name`（及建议的 `description`）作为主语言兜底（常用 `zh-CN` 文案或
  指令语言的默认文案）。
- **默认同时填写** `locales["en-US"]` 与 `locales["ja-JP"]` 的 `name`/`description`
  （Myriad 宿主常用语言；简短标题也要翻译）。仅当用户明确要求单语包时才可省略。
- `locales` **不能**替代 `code.i18n`；应用内文案仍走 `Tapp.i18n`。
- **不要**把 `long_description` 或 `preview` 写进 `manifest.locales`（安装校验会拒绝未知字段）。
- 完整字段与回退链见 [MANIFEST · 多语言名称与描述](./MANIFEST.md#多语言名称与描述locales)。
  商店展示层见 [STORE](./STORE.md)。

示例（顶层中文兜底 + 宿主多语言目录文案）：

```json
{
  "name": "番茄钟",
  "description": "专注计时与休息提醒",
  "locales": {
    "en-US": { "name": "Pomodoro", "description": "Focus timer and break reminders" },
    "ja-JP": { "name": "ポモドーロ", "description": "集中タイマーと休憩リマインダー" }
  }
}
```

## 生命周期

```javascript
Tapp.lifecycle.onReady(async function () {
  const locale = await Tapp.ui.getLocale();
  const theme = await Tapp.ui.getTheme();
  document.documentElement.dataset.locale = locale;
  document.documentElement.dataset.theme = theme;
});
```

初始化、SDK 查询和事件绑定都应从 `onReady` 开始。不要假设 DOM、宿主消息 Bridge 或
异步资源在脚本首次求值时已经就绪。

## 首版预览可用 API

```javascript
await Tapp.ui.getTheme();
await Tapp.ui.getPrimaryColor();
await Tapp.ui.getLocale();
await Tapp.ui.confirm('Continue?');
await Tapp.ui.requestFullscreen();

Tapp.i18n.t('title');
Tapp.i18n.t('progress', { done: 1, total: 3 });
Tapp.i18n.getLocale();

await Tapp.storage.get('key');
await Tapp.storage.set('key', { value: 1 });
await Tapp.storage.remove('key');
await Tapp.storage.keys();
await Tapp.storage.getAll();
await Tapp.storage.clear();
```

应用内翻译资源通过 `code.i18n` 提供；Page、Widget 与 core 统一使用同步的
`Tapp.i18n.t()`。每个语言表既可使用 `{"app.title": "..."}` 这种扁平点号键，也可使用
嵌套对象；SDK 优先匹配完整键，再按点号读取嵌套路径。不要臆造其他 i18n SDK，也不要
直接读取内部的 `window._TAPP_I18N`。

宿主列表/商店标题不要只靠 `Tapp.i18n`：请同时写好 `manifest.name` /
`manifest.description` 与可选 `manifest.locales`（见上一节）。

`Tapp.storage` 在正式运行中是 `(current_user_id, tapp_id)` 的用户私有空间；Playground
预览只提供当前标签页内存实现。不要用 storage 模拟安装级设置或公开数据。

## 正式安装才可用（预览不要依赖）

临时预览 **不签发 Runtime Grant**。`playgroundPreviewHandlers.ts` 实际注册的大致是：
内存 `storage` / `settings`、`ui` 主题·语言·确认·全屏（通知禁用）、`context.*` 预览桩、
`assets.list`（空）/`assets.get`（失败）、`api.list`（空）/`api.execute`（禁用）。

下列能力在完整 SDK 里可能有方法名，但 **Playground 预览中不可用**（失败或明确错误）：

- **Federation** 全套（`uploadMedia` → `createNote` 附件 URL、Channel/Room/Ring 等）
- **platform** / **report** / **brewList** / **tappList**（含列表与商店/直接安装）
- **dataExchange**、**ai**、**agent**、**event** Broker、**scheduler**、宿主 **media** 控制
- 声明式 **`Tapp.api` 执行**（预览仅 list 空表）
- **`Tapp.background.require`**（预览无后台常驻；勿空写 `backgroundRequirements`）

生成安装后才有意义的能力时：

- 可在 Manifest 声明真实权限与正式运行时代码（见 [API_REFERENCE](./API_REFERENCE.md)）；
- 声明式 HTTP API 必须申请 `network:fetch`。请求体默认使用 `bodyMode: "json"`；纯文本、XML
  使用 UTF-8 `raw`，表单使用 `form`。第三方密钥用 Manifest `credentials` +
  `apis.*.credential`（`in`: `header` / `query` / `form` / `sign`）；密钥不进
  `settings` 或模板。`raw`/`form`/`sign` 仅允许 `POST`、`PUT`、`PATCH`、`DELETE`。
  临时预览不会实际执行 `Tapp.api`；完整字段见
  [MANIFEST · API 声明](./MANIFEST.md#api-声明-apis) 与
  [安装级凭据](./MANIFEST.md#安装级-api-凭据-credentials)。
  **不要**在源码、`settings`、i18n 或注释里写入真实或示例密钥；`credentials` 只声明
  `key`/`label`，值由站主安装后写入。
- 不要把 Three / Pocket 运行时打进生成物，也不要输出 CDN。完整 guest Three 样例见商店
  `com.myriad.three-lab`；Playground 只需在需要 3D 时按 [GRAPHICS](./GRAPHICS.md) 声明
  `assets/` 并调用 `getUrlMap` / `rewriteUrl`。
- 预览只验证 UI、生命周期、主题、`code.i18n`、`manifest.locales` 与内存 storage；
- **不要**臆造预览 mock 联邦 / Brew / platform API。
- 若生成 **正式运行后** 调用 `Tapp.tappList.install` 的商店安装代码，必须使用合法 SDK 形状
  （见 [STORE](./STORE.md) / [API_REFERENCE · Tapp 列表](./API_REFERENCE.md#tapp-列表-api)）：
  - ✅ `{ source: "store", storeSource: "<源 id 或 catalog URL>", tappId }`
  - ✅ `{ source: "https://…/index.json", tappId }`（HTTP `source` 即 catalog）
  - ❌ `{ source: "1", tappId }`（裸非 HTTP `source` **不会**当作源 id）
  - ✅ direct：`{ source: "direct", manifest, code, … }`（须有 manifest + code）
  Playground **工作区「安装到本机」**走宿主 direct 包安装，不是商店 `source=store` 路径。

Bridge 默认 payload 约 **1 MiB**；正式运行特例：`file.download` 内容 **10 MiB**，
`federation.uploadMedia` 对齐图片 10 MiB / 视频 50 MiB 的 base64 预算（见
[SANDBOX](./SANDBOX.md#payload-大小)）。预览侧勿假设可上传大媒体。

## Manifest 质量字段（可选但推荐）

- **`locales`**：见上文；默认 `en-US` + `ja-JP`。
- **`iconSvg`**：生产包优先内联 SVG（优先于 emoji `icon`）；简单 demo 可用 emoji。
- **`minSystemVersion`**：可选语义版本；声明后安装/更新会与宿主 Myriad 版本比较并拒绝过旧实例。新能力依赖新 runtime 时建议填写。
- **`backgroundRequirements`**：仅当 `code.core` 真正依赖后台常驻（如 scheduler/sync）时填写；勿为「好看」空挂。

## 安全与兼容性

- 不使用 `fetch`、XHR、WebSocket 或外链脚本；外部访问必须在正式 Manifest 中声明并由
  宿主代理；Playground 生成侧通常不依赖此类声明，预览中也不可用。
- 不使用 `eval`、`Function`、`document.write`、动态脚本、inline event handler 或
  `javascript:` URL。
- 不读取 Cookie、localStorage、sessionStorage、父窗口 DOM 或宿主 token。
- 页面在窄屏和宽屏都必须可用，并支持浅色/深色背景。
- Manifest 只声明代码真实调用的权限；读取主题和语言不需要额外权限，storage 需要
  `storage`，确认和全屏分别需要 `ui:confirm` 与 `ui:fullscreen`。
- 应用分类和 Widget 分类不是同一枚举；Widget 分类仅允许 `stats`、`activity`、
  `visualization`、`utility`、`custom`，声明 Widget 时必须请求 `widget:register`。
- 顶层 `manifest.settings` 是安装级设置；用户个人偏好放入 `Tapp.storage`，单个 Widget
  实例偏好放入对应的 `widgets[].settings`。
- 所有设置定义的默认值字段都必须写成 `defaultValue`；`default` 不是合法别名。
- 应用用途分类只使用 `ai`、`data`、`developer`、`game`、`media`、`productivity`、
  `social`、`utility`。
- `manifest.assets` / `code.assets` 仅用于 `assets/` 下的静态二进制或数据文件；
  不要把 `page.html`、`*.js`、Widget 模板（如 `templates/*.html`）放进 assets。
