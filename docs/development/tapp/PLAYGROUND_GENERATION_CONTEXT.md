# Playground 生成契约

本文档是 Tapp Playground 发送给 Pro 模型的精简开发上下文，与 `DESIGN_SPEC.md`
一样**无条件注入**生成 prompt，不依赖检索命中。完整解释仍以同目录的
`MANIFEST.md`、`API_REFERENCE.md`、`SANDBOX.md`、`STORE.md` 和 `STYLING.md` 为准
（其余模块文档按检索进入，见 `tapp_playground_knowledge.rs`）。

## 文件与运行模式

Playground 项目至少需要 **Page** 或 **Widgets** 之一（允许 Widget-only，不强制 Page）。

- `manifest.json` 描述身份、入口、资源、分类、权限和运行形态。
- **Page 模式**（声明 `page` 层）：共享逻辑写在 `code.core`（打包为 `core.js`），页面
  逻辑写在 `code.page`（打包为 `page/index.js`）；`page.html` 只含 body 内静态语义
  结构；`page.entry` 为 `page/index.js`、`page.template` 为 `page.html`。
- **Widget-only**（不声明 `page` 层）：不要发明 stub 页面；UI 放在 `code.widget` 与
  `code.widgetHtml`（打包为 `widget/index.js`）；声明非空 `manifest.widgets`。安装校验
  要求**声明权限**含 `widget:register`（privileged：普通用户不会被**授予**动态注册，
  Manifest Widget 仍预注册）。保持 `page` / `pageHtml` 为空。
  Widget 可见 UI **必须**注册 `Tapp.widgets[<id>] = { render(container, props) { ... } }`
  （宿主调用 `render`）。**不要**在 Widget 层写 `Tapp.widget.register`（那是 Page 动态注册）。
  Widget 沙箱没有 `confirm` / `setTitle` / `fullscreen` / `Tapp.game` / 联邦 /
  `tappList` / `brewList`。详见 [WIDGET.md](./WIDGET.md)。
- core 是共享层，三种沙箱都先执行它。Playground 一层一个文件（`core.js` /
  `page/index.js` / `widget/index.js`）；跨层共享在 core 里 `module.exports`，层入口
  `require('../core.js')`。再拆文件只在导出 `.tapp` 并用 CLI 打开之后。
- Playground 生成校验**始终**要求 `core.entry=core.js` 与 `core.styles=styles.css`
  （固定打包布局）。安装契约本身：没有 `backgroundRequirements` 时不强制 core；
  声明**常驻**必须有 core。
- `styles.css` 使用普通 CSS，并通过 `var(--tapp-primary)` 读取宿主强调色
  （沙箱内没有 `--color-primary`）。HTML / JS 里的 Tailwind 工具类由宿主按需编译，
  预览和安装后都会注入；不要依赖 `sm:`/`md:` 断点，也不要输出 CDN Tailwind。
- Page 沙箱（有可用 Page 时）运行在没有 `allow-same-origin` 的 sandboxed iframe 中，
  CSP 使用每实例 nonce。Widget-only 预览不挂载 Page 沙箱。
- **Playground Page 怎么画到屏幕上**：生成校验要求非空 `page.html`，宿主把它包进
  `#tapp-root` > `#tapp-content`（模板自己写了 `id="tapp-content"` / `tapp-background`
  时只包 `#tapp-root`）。有 HTML 模板时宿主**不会**调用 `Tapp.pages[id].render`。
  静态结构写在 `pageHtml`，`onReady` 里查询 `#tapp-content` 或模板里的 id 绑定事件。
  **不要** `document.getElementById('tapp-root').textContent = ...`（会拆掉背景层
  和内容层）。**不要**只登记 `Tapp.pages` 却等宿主来 `render`——那是「没有 page.html」
  的纯 JS 路径，不是 Playground。Widget 可见 UI 仍是
  `Tapp.widgets[id] = { render(container, props) }`，宿主调用 `render`。
- Canvas / WebGL / Three.js 只放在 **Page**。不要在 Widget 里跑 rAF 主循环或 3D 场景。
- 禁止输出 CDN 脚本（`unpkg` / `jsdelivr` / `cdnjs` / `esm.sh` / `threejs.org/build`）。
  沙箱 `connect-src` 只有 `blob:` / `data:`，也不能 `import` npm。
  3D 预览请声明 `runtimeModules: ["three"]`（game / developer），使用宿主注入的
  `THREE` / `GLTFLoader`；**包内**贴图和 `.glb` 走 `Tapp.assets`（Playground 预览从
  工作区内存提供 `list` / `get`，与安装后同一 API）。
  站点 Tripo 产物用 `Tapp.model3d.getUrl(assetId)` 拿沙箱 blob，不要 `load('/api/...')`。
  `Tapp.model3d.createTask` / `upload` / `3d:generate` 只在正式安装后可用。
  先 `Tapp.assets.getUrlMap()`，再用 `rewriteUrl` 接 LoadingManager。
  `fetch` 只能打 blob/data。详见 [GRAPHICS.md](GRAPHICS.md)。
- 不要在预览里调用 `Tapp.game` 或联邦房间。联机只写正式安装后的代码。
  若生成安装后才跑的对局：Manifest 声明 `game: { protocol }`，并同时申请
  `game:session` 与 `federation:read` / `federation:room` / `federation:message`，
  用 `Tapp.game`（不要自己拼 `gomoku.v1`）。只写权限没有 `manifest.game` 不能
  调用。`create()` 默认不公开；跨实例私房靠邀请，跨实例分享 ID 自助加入必须
  `{ isPublic: true }`。

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

Page 与 Widget 共用（预览内存实现 / 桩）：

```javascript
await Tapp.ui.getTheme();
await Tapp.ui.getPrimaryColor();
await Tapp.ui.getLocale();
await Tapp.ui.openUrl({ id: 'docs' }); // 需 ui:openUrl + manifest.openUrls
await Tapp.ui.listOpenUrls();

Tapp.i18n.t('title');
Tapp.i18n.t('progress', { done: 1, total: 3 });
Tapp.i18n.getLocale();

await Tapp.storage.get('key');
await Tapp.storage.set('key', { value: 1 });
await Tapp.storage.remove('key');
await Tapp.storage.keys();
await Tapp.storage.getAll();
await Tapp.storage.clear();

await Tapp.settings.get('compact');
await Tapp.settings.set('compact', true);
await Tapp.settings.getAll();
Tapp.settings.onChanged(({ key, operation }) => {});

await Tapp.shared.get('posts');
await Tapp.shared.set('posts', []);
await Tapp.shared.keys();

const urls = await Tapp.assets.getUrlMap();
const file = await Tapp.assets.getUrl('assets/icon.png');

await Tapp.context.getApp(); // { version, locale, theme, features }
await Tapp.context.getSystem();
// { online, serverConnected, version, preview: true, runtime: 'tapp-playground', ... }
await Tapp.context.getGeo(); // 预览固定 null；安装后才是 { lat, lon, city, region, country }
await Tapp.user.getRole();
await Tapp.file.download('hello', 'hello.txt', 'text/plain');

const card = await Tapp.persona.get();
// { enabled, name, moodBand, activity, portraitUrl }
// 预览里 portraitUrl 可能为 null；安装后是宿主同源路径，可直接 <img src>
```

**仅 Page 预览**另有 `Tapp.ui.setTitle` / `confirm` / `requestFullscreen`。
**Widget 预览没有这些方法。** Widget 入口这样写：

```javascript
Tapp.widgets['my-widget'] = {
  render: function (container, props) {
    var settings = Tapp.widget.getInstanceSettings();
    container.textContent = Tapp.i18n.t('title');
  }
};
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

临时预览 **不签发 Runtime Grant**。预览里大致能用：
内存 `storage` / `settings` / `shared`、`ui` 主题·语言·`openUrl`（通知禁用）、
`context.*` / `user.*` 预览桩（`getGeo` 为 `null`）、`persona.get` 固定样例（不打真实人设 API）、工作区
`Tapp.assets`（`code.assets` 内存）、`file.download`、`Tapp.api.list()`（空）/
`Tapp.api()`（禁用）。**标题 / 确认 / 全屏**只在 **Page 预览**；Widget 沙箱没有这些方法。

下列能力在完整 SDK 里可能有方法名，但 **Playground 预览中不可用**（失败或明确错误）：

- **Federation** 全套（`uploadMedia` → `createNote` 附件 URL、Channel/Room/Ring 等）
- **Tapp.model3d** / `3d:generate`（生成要 Grant；预览无 handlers。包内 GLB 仍走 `Tapp.assets`）
- **platform** / **report** / **brewList** / **tappList**（含列表与商店/直接安装）
- **dataExchange**、**ai**、**agent**、**event** Broker、**scheduler**、宿主 **media** 控制
- 声明式 **`Tapp.api` 执行**（预览仅 list 空表）
- **`Tapp.background.require`**（预览无常驻；勿空写 `backgroundRequirements`）

生成安装后才有意义的能力时：

- 可在 Manifest 声明真实权限与正式运行时代码（见 [API_REFERENCE](./API_REFERENCE.md)）；
- AI **只**走 `Tapp.ai.tasks`（`create` / `get` / `cancel` / `usage` / `subscribe`）。
  不要发明 `Tapp.ai.generate`、`Tapp.search`、直接 `fetch` 搜索，或供应商 / 模型字段。
  临时预览**不执行** AI：调用会失败（明确错误，不是安装后的结果）。把 AI 按钮放进
  `try/catch`，预览里展示「安装后可用」，不要假设 `create` 已经返回图片。
  `create()` **立刻**返回 `status: "queued"` 的快照，此时 `task.result` 为空。
  必须再 `get` / `subscribe` 等到 `completed` 之后才能读信封。不要写成
  `const task = await Tapp.ai.tasks.create(...); img.src = task.result.value.url`。
  `operation` 必须与**声明权限** `ai:*`、`manifest.ai.operations`、
  `manifest.ai.outputFormats` 对齐（`ai.protocolVersion: 2`）。只声明
  `permissions: ["ai:image"]` 而不写 `manifest.ai` 会在安装后得到
  `AI_V2_NOT_DECLARED`。安装后是否真能调用由**授予权限**决定。
  `operation` 为 `generate` / `analyze` / `chat` / `image` / `search`：

  | operation | 权限 | `input` | `output.format` |
  | --------- | ---- | ------- | --------------- |
  | `generate` | `ai:generate` | 字符串或 `{ prompt }` | `text` 或 `json` |
  | `analyze` | `ai:analyze` | `{ data, instruction? }` | `text` 或 `json` |
  | `chat` | `ai:chat` | `{ messages: [{ role, content }] }` | `text` 或 `json` |
  | `image` | `ai:image` | 字符串或 `{ prompt, width?, height?, referenceImages? }` | **必须** `image` |
  | `search` | `ai:search` | 字符串或 `{ query, searchType?, maxResults?, searchPrompt? }` | **必须** `json` |

  完成态读 `task.result`：`{ format, value, contextProvenance }`。业务数据在 `value`
  （生图是 `{ url, width, height }`，`url` 是 `/api/brew/image-cache/...`，可直接作
  `<img src>`；下载把 `url`、整份 `task` 或 `task.result` 交给 `Tapp.file.download`，由宿主读缓存，不要 `fetch`。
  3D 用 `/api/model3d/assets/{id}` 或 `getUrl` 的返回对象；TTS 用 `{ audio }` 或 `{ base64: audio }`。
  搜索是 JSON 结果对象）。`subscribe` 的 `result` / 终态 `snapshot`
  事件的 `data` 是整份任务快照，信封在 `data.result`。

  生图最小 Manifest（权限 + `ai` 缺一不可）：

  ```json
  {
    "permissions": ["ai:image"],
    "ai": {
      "protocolVersion": 2,
      "operations": ["image"],
      "modelTier": "standard",
      "contextSources": [],
      "outputFormats": ["image"]
    }
  }
  ```

  ```javascript
  async function awaitAiTask(created) {
    var task = created;
    while (task.status === "queued" || task.status === "running") {
      await new Promise(function (resolve) { setTimeout(resolve, 400); });
      task = await Tapp.ai.tasks.get(task.taskId);
    }
    if (task.status !== "completed" || !task.result) {
      throw new Error((task.error && task.error.message) || task.status || "AI task failed");
    }
    return task;
  }

  const created = await Tapp.ai.tasks.create({
    version: 2,
    operation: "image",
    input: { prompt: "a cat sitting on a windowsill, soft light" },
    output: { format: "image" },
  });
  const task = await awaitAiTask(created);
  imageEl.src = task.result.value.url;
  ```

  文本流式用 `subscribe` 听 `delta`；生图 / 搜索不要 `delivery: "stream"`。
  生图参考图放在 `input.referenceImages` 有序数组中，支持 PNG/JPEG/WebP base64 data URL
  或 `/api/brew/image-cache/...`，最多 4 张、解码后合计 10 MiB。不要放到 `context`，
  不要直接传 `blob:` / 外部 URL。文件选择后可用 `FileReader.readAsDataURL` 转换。
  完整字段见 [AI API](./API_REFERENCE.md#ai-api)。
- 声明式 HTTP API 必须申请 `network:fetch`。请求体默认使用 `bodyMode: "json"`；纯文本、XML
  使用 UTF-8 `raw`，表单使用 `form`。第三方密钥用 Manifest `credentials` +
  `apis.*.credential`（`in`: `header` / `query` / `form` / `sign`）；密钥不进
  `settings` 或模板。`raw`/`form`/`sign` 仅允许 `POST`、`PUT`、`PATCH`、`DELETE`。
  临时预览不会实际执行 `Tapp.api`；完整字段见
  [MANIFEST · API 声明](./MANIFEST.md#api-声明-apis) 与
  [安装级凭据](./MANIFEST.md#安装级-api-凭据-credentials)。
  若要给其他程序调用，再声明 `apis.*.route`（必须 `hmac-sha256-raw` + timestamp/nonce，
  且 `access: public`）；不要编造未签名的 `/tapi` 或把密钥写进源码。
  **不要**在源码、`settings`、i18n 或注释里写入真实或示例密钥；`credentials` 只声明
  `key`/`label`，值由站主安装后写入。
- 不要把 Three / Pocket 运行时打进生成物，也不要输出 CDN。完整 guest Three 样例见商店
  `com.myriad.three-lab`；Playground 只需在需要 3D 时按 [GRAPHICS](./GRAPHICS.md) 声明
  `assets/` 并调用 `getUrlMap` / `rewriteUrl`。
- 预览验证 UI、生命周期、主题、`code.i18n`、`manifest.locales`、内存 storage/settings/shared，
  以及工作区包内 `Tapp.assets`；
- **不要**臆造预览 mock 联邦 / Brew / platform API。
- 若生成 **正式运行后** 调用 `Tapp.tappList.install` 的商店安装代码，必须使用合法 SDK 形状
  （见 [STORE](./STORE.md) / [API_REFERENCE · Tapp 列表](./API_REFERENCE.md#tapp-列表-api)）：
  - ✅ `{ source: "store", storeSource: "<源 id 或 catalog URL>", tappId }`
  - ✅ `{ source: "https://…/index.json", tappId }`（HTTP `source` 即 catalog）
  - ❌ `{ source: "1", tappId }`（裸非 HTTP `source` **不会**当作源 id）
  - ✅ direct：`{ source: "direct", manifest, code, … }`（须有 manifest + code）
  Playground **工作区「安装到本机」**走宿主 direct 包安装，不是商店 `source=store` 路径。

Bridge 默认 payload 约 **1 MiB**；正式运行特例：`file.download` 文本 / base64 / 宿主代取
生图·3D **32 MiB**，`federation.uploadMedia` 对齐图片 10 MiB / 视频 50 MiB 的 base64 预算（见
[SANDBOX](./SANDBOX.md#payload-大小)）；带 `input.referenceImages` 数组的生图任务 JSON payload
上限为 **14 MiB**，后端另检查图片数量、解码后总大小与文本大小。预览侧勿假设可上传大媒体。

## Manifest 质量字段（可选但推荐）

- **`locales`**：见上文；默认 `en-US` + `ja-JP`。
- **`iconSvg`**：生产包优先内联 SVG（优先于 emoji `icon`）；简单 demo 可用 emoji。
- **`minSystemVersion`**：可选语义版本；声明后安装/更新会与宿主 Myriad 版本比较并拒绝过旧实例。新能力依赖新 runtime 时建议填写。
- **`backgroundRequirements`**：仅当 `code.core` 真正依赖常驻（如 scheduler/sync）时填写；勿为「好看」空挂。

## 安全与兼容性

- 不使用 `fetch`、XHR、WebSocket 或外链脚本；外部访问必须在正式 Manifest 中声明并由
  宿主代理；Playground 生成侧通常不依赖此类声明，预览中也不可用。
- 不使用 `eval`、`Function`、`document.write`、动态脚本、inline event handler 或
  `javascript:` URL。
- 不读取 Cookie、localStorage、sessionStorage、父窗口 DOM 或宿主 token。
- 页面在窄屏和宽屏都必须可用，并支持浅色/深色背景。
- Manifest 只声明代码真实调用的权限；读取主题和语言不需要额外权限。读取
  `Tapp.storage` / `Tapp.settings` / `Tapp.shared` 需要 `storage:read`，写入、删除与清空需要
  `storage:write`。`Tapp.file.download` 是 public，不要为了把生成文件存到本机去申请
  `storage:read`。不要再声明已移除的 `storage`。确认和全屏分别需要
  `ui:confirm` 与 `ui:fullscreen`。
- 应用分类和 Widget 分类不是同一枚举；Widget 分类仅允许 `stats`、`activity`、
  `visualization`、`utility`、`custom`。声明 `widgets` 时安装校验要求声明权限含
  `widget:register`；不要把它理解成「普通用户运行时可以动态注册 Widget」。
- 顶层 `manifest.settings` 是安装级设置；用户个人偏好放入 `Tapp.storage`，要给访客看的
  站长数据放入 `Tapp.shared`，单个 Widget 实例偏好放入对应的 `widgets[].settings`。
- 所有设置定义的默认值字段都必须写成 `defaultValue`；`default` 不是合法别名。
- 应用用途分类只使用 `ai`、`data`、`developer`、`game`、`media`、`productivity`、
  `social`、`utility`。
- `manifest.assets` / `code.assets` 仅用于 `assets/` 下的静态二进制或数据文件；
  不要把 `page.html`、`*.js`、Widget 模板（如 `templates/*.html`）放进 assets。
