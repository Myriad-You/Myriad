# Playground 生成契约

本文档是 Tapp Playground 发送给 Pro 模型的精简开发上下文。完整解释仍以同目录的
`MANIFEST.md`、`API_REFERENCE.md`、`SANDBOX.md` 和 `STYLING.md` 为准。

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

翻译资源通过 `code.i18n` 提供；Page、Widget 与 core 统一使用同步的 `Tapp.i18n.t()`。
每个语言表既可使用 `{"app.title": "..."}` 这种扁平点号键，也可使用嵌套对象；SDK
优先匹配完整键，再按点号读取嵌套路径。不要臆造其他 i18n SDK，也不要直接读取内部的
`window._TAPP_I18N`。

`Tapp.storage` 在正式运行中是 `(current_user_id, tapp_id)` 的用户私有空间；Playground
预览只提供当前标签页内存实现。不要用 storage 模拟安装级设置或公开数据。

## 正式安装才可用（预览不要依赖）

临时预览 **不签发 Runtime Grant**，handlers 仅覆盖 storage/settings、主题/语言、确认/
全屏、受限 context 等（见 `playgroundPreviewHandlers.ts`）。下列能力在 SDK 完整版里
可能仍有方法名，但 **Playground 预览中不可用**（调用失败或返回明确错误）：

- **Federation 全套**（`Tapp.federation.*`：Feed、关注、`uploadMedia` / `createNote` /
  `publish`、Channel/Room/Ring、传输与 trust）
- 平台写入、声明式网络 `Tapp.api` 执行、AI、宿主媒体控制、跨 Tapp 事件 Broker 等

生成联邦/社交类 Tapp 时：

- 可在 Manifest 声明真实 `federation:*`（或其它）权限，并按 [API_REFERENCE](./API_REFERENCE.md)
  写正式运行时代码；
- 预览只验证 UI、生命周期、主题、i18n 与内存 storage；
- **不要**臆造「预览专用 mock 联邦 API」或未在 SDK/`permissionConfig` 中存在的方法。

Bridge 默认 payload 约 1 MiB；`file.download` 与 `federation.uploadMedia` 在正式运行有
更大专用上限（见 [SANDBOX](./SANDBOX.md#payload-大小)）。预览侧勿假设可上传大媒体。

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
