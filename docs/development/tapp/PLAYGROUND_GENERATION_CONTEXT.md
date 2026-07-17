# Playground 生成契约

本文档是 Tapp Playground 发送给 Pro 模型的精简开发上下文。完整解释仍以同目录的
`MANIFEST.md`、`API_REFERENCE.md`、`SANDBOX.md` 和 `STYLING.md` 为准。

## 文件与运行模式

- `manifest.json` 描述身份、入口、资源、分类、权限和运行形态。
- `main.js` 可以包含共享 `core` 与可见 `page` 代码。
- `page.html` 只包含 body 内的静态语义结构；行为写在 `main.js`。
- `styles.css` 使用普通 CSS，并通过 `var(--tapp-primary)` 读取宿主强调色
  （沙箱内没有 `--color-primary`）。
- Page 运行在没有 `allow-same-origin` 的 sandboxed iframe 中，CSP 使用每实例 nonce。

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

## 安全与兼容性

- 不使用 `fetch`、XHR、WebSocket 或外链脚本；外部访问必须在正式 Manifest 中声明并由
  宿主代理，但首版 Playground 不生成此类声明。
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
