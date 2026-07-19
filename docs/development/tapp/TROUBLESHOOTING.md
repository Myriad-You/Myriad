# 故障排除指南

本文档汇总 Tapp 开发中常见的问题和解决方案，帮助你快速定位和修复错误。

---

## 控制台错误

### ❌ `Not allowed to load local resource: blob:...`

**症状**：控制台显示此错误，但 Tapp 功能正常。

**原因**：浏览器在 iframe 沙箱中清理 blob URL 时的正常行为。当 Tapp 页面卸载或刷新时触发。

**解决方案**：无需处理，这是预期行为，不影响功能。沙箱已配置全局错误处理器来抑制此类错误。

---

### ❌ `Failed to define: top` / `Failed to define: parent`

**症状**：控制台显示属性定义失败的警告。

**原因**：某些全局属性（如 `window.top`, `window.parent`）是不可配置的，无法被沙箱重新定义。

**解决方案**：无需处理，沙箱会静默跳过这些属性。确保你的 Tapp 代码不尝试访问这些属性。

---

### ❌ `Content Security Policy` 警告

**症状**：

```
Refused to load the script/style/image '...' because it violates the following Content Security Policy directive: ...
```

**原因**：Tapp 尝试加载不被 CSP 允许的资源。

**解决方案**：

1. **外部脚本**：不支持直接加载外部 JS 文件，将代码内联到 `main.js`
2. **外部样式**：使用 `styles.css` 文件或内联样式
3. **图片**：允许 `data:`、`blob:` 和 HTTP(S)，检查 URL 与响应类型
4. **媒体**：CSP 默认禁止直接加载；使用宿主媒体 API
5. **API 请求**：在 Manifest 的 `apis` 中声明，再调用 `Tapp.api(name, params)`

---

### ❌ `prefetch-src` 废弃警告

**症状**：

```
The Content-Security-Policy directive 'prefetch-src' is not implemented...
```

**原因**：旧版本代码包含已废弃的 CSP 指令。

**解决方案**：如果你在维护沙箱代码，移除 `prefetch-src` 指令。Myriad 最新版本已修复此问题。

---

## 文件加载问题

### ❌ Tapp 安装后显示空白或报错

**可能原因**：

1. **入口文件不存在或商店下载路径不一致**

   ```json
   // manifest.json：安装后的入口
   { "main": "main.js" }

   // 商店 index.json：仓库中的下载位置
   { "download": { "code": "apps/com.example/main.js" } }
   ```

2. **manifest 声明的资源缺失**

   ```json
   // manifest.json 声明了这些文件，但实际不存在
   {
     "styles": "styles.css", // 文件不存在
     "pageTemplate": "page.html", // 文件不存在
     "widgets": [
       {
         "templates": {
           "4x2": "widget-4x2.html" // 文件不存在
         }
       }
     ]
   }
   ```

**解决方案**：

1. 检查 `manifest.main` 是否指向安装根目录内真实存在的 JS 文件
2. 商店发布时检查 `index.json` 的 `download.code` 是否下载同一入口
3. 确保 Manifest 中声明的所有资源文件都实际存在
4. 文件名区分大小写

---

### ❌ Widget 模板不加载

**症状**：Widget 显示空白或只显示 JS 生成的内容，不显示 HTML 模板。

**可能原因**：

1. `manifest.json` 中未配置 `templates`
2. 模板文件路径错误或文件不存在
3. 模板 HTML 格式错误

**解决方案**：

```json
// manifest.json
{
  "widgets": [
    {
      "id": "my-widget",
      "templates": {
        "2x2": "widget-2x2.html",
        "4x2": "widget-4x2.html"
      }
    }
  ]
}
```

确保模板文件存在且 HTML 格式正确。

---

## 样式问题

### ✔️ Tailwind 类完整支持

Tapp 沙箱通过 Tailwind CDN 加载，支持所有 Tailwind 类名，包括：

- 布局：`flex`, `grid`, `items-center`, `justify-between`, `gap-*`
- 尺寸：`w-full`, `h-full`, `min-h-*`, `max-w-*`
- 间距：`p-*`, `m-*`, `px-*`, `py-*`
- 文字：`text-*`, `font-*`, `leading-*`
- 颜色：`bg-*`, `text-*`, `border-*`
- 效果：`rounded-*`, `shadow-*`, `opacity-*`
- 暗色：`dark:bg-*`, `dark:text-*`
- 任意值：`w-[200px]`, `bg-[#1da1f2]` 等

---

### ❌ `.glass` 类不生效

**症状**：使用 `.glass` 类但没有毛玻璃效果。

**原因**：旧版沙箱可能未包含 `.glass` 类定义。

**解决方案**：

1. 更新 Myriad 到最新版本
2. 或在 `styles.css` 中自行定义：

```css
.glass {
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid rgba(255, 255, 255, 0.3);
}

.dark .glass {
  background: rgba(26, 26, 26, 0.8);
  border: 1px solid rgba(255, 255, 255, 0.1);
}
```

---

### ❌ 暗色模式样式不生效

**症状**：`dark:` 前缀的类不生效。

**原因**：

1. 暗色模式类可能未被预设包含
2. 容器未正确继承 `dark` 类

**解决方案**：

1. 使用 JavaScript 检测主题并动态应用样式：

```javascript
Tapp.ui.onThemeChange(function (theme) {
  container.classList.toggle("dark-mode", theme === "dark");
});
```

2. 或使用 CSS 变量：

```css
.my-element {
  color: var(--tapp-text-primary);
  background: var(--tapp-bg-primary);
}
```

---

## 事件绑定问题

### ❌ 按钮点击无响应

**症状**：HTML 中的按钮或元素点击后没有反应。

**可能原因**：

1. 事件绑定在 DOM 更新后丢失
2. 选择器无法匹配元素
3. 混合模式下未正确绑定事件

**解决方案**：

```javascript
// ✅ 推荐：使用 data-* 属性标记元素
var btn = container.querySelector('[data-action="submit"]');
if (btn) {
  btn.addEventListener("click", handleSubmit);
}

// ✅ 检查元素是否存在再绑定
function bindEvents() {
  var elements = container.querySelectorAll("[data-action]");
  elements.forEach(function (el) {
    var action = el.getAttribute("data-action");
    if (action === "submit") {
      el.addEventListener("click", handleSubmit);
    }
  });
}
```

---

### ❌ `innerHTML` 更新后事件丢失

**症状**：使用 `innerHTML` 更新内容后，之前绑定的事件失效。

**原因**：`innerHTML` 会替换 DOM 元素，原来绑定的事件监听器随之消失。

**解决方案**：

```javascript
// ❌ 错误：每次更新都会丢失事件
container.innerHTML = newContent;

// ✅ 方法1：更新后重新绑定事件
container.innerHTML = newContent;
bindEvents();

// ✅ 方法2：只更新需要变化的部分
var contentEl = container.querySelector("[data-content]");
contentEl.textContent = newValue; // 不影响其他元素的事件

// ✅ 方法3：使用事件委托
container.addEventListener("click", function (e) {
  if (e.target.matches('[data-action="submit"]')) {
    handleSubmit(e);
  }
});
```

---

## Widget 问题

### ❌ Widget 显示"需要启动"

**症状**：Widget 可以添加到 Dashboard，但显示需要启动 Tapp。

**原因**：这是正常行为，Widget 只在 Tapp 运行时渲染。

**解决方案**：点击提示启动 Tapp。Widget 可见期间由自己的沙箱运行，不需要声明后台
需求；只有离开页面后仍需同步、媒体控制等任务时，才声明对应的 `sync`、`media` 等需求。

---

### ❌ Widget 尺寸不正确

**症状**：Widget 在某些尺寸下显示异常或溢出。

**原因**：

1. 未针对不同尺寸适配布局
2. 固定尺寸样式导致溢出

**解决方案**：

1. 为每个尺寸提供专门的模板
2. 使用响应式布局：

```javascript
Tapp.widgets["my-widget"] = {
  render: function (container, props) {
    var size = props.size || "2x2";
    var isSmall = size === "1x1" || size === "1x2" || size === "2x1";
    var isLarge = size === "4x4";

    if (isSmall) {
      // 紧凑布局
      container.innerHTML = "<div>简要内容</div>";
    } else if (isLarge) {
      // 详细布局
      container.innerHTML = "<div>详细内容 + 图表</div>";
    } else {
      // 标准布局
      container.innerHTML = "<div>标准内容</div>";
    }
  },
};
```

---

## 网络请求问题

### ❌ `fetch` 被阻止

**症状**：直接使用 `fetch` 发送请求被阻止。

**原因**：沙箱禁止直接网络请求，必须通过 SDK。

**解决方案**：

```javascript
// ❌ 错误
fetch("https://api.example.com/data");

// ✅ 正确：先在 manifest.apis 中声明 "data"
const data = await Tapp.api("data", {});
```

---

### ❌ API 请求返回 403

**症状**：通过 `Tapp.api(name, params)` 请求 API 返回 403 或定义不存在。

**原因**：

1. 未在 manifest 的 `apis` 中声明对应名称
2. 任意 `type: "http"` API（含 `public` 与 `protected`）未授予 `network:fetch`
3. 后端出站安全或参数模板校验拒绝了请求

**解决方案**：

在 `manifest.json` 中声明 API，并在 `permissions` 中申请 **`network:fetch`**（所有
HTTP 声明式 API 都需要，不只是 `protected`）：

```json
{
  "permissions": ["network:fetch"],
  "apis": {
    "data": {
      "type": "http",
      "endpoint": "https://api.example.com",
      "method": "GET",
      "access": "protected",
      "description": "数据 API"
    }
  }
}
```

如果 API 确实可匿名调用，可把 `access` 明确设为 `public`；这只改变调用者范围，
不会免除 `network:fetch`，并且仍会经过共享限流、Manifest 与后端出站安全校验。

---

## Federation / Bridge 载荷

### ❌ `Payload too large` / `Media data too large`

**症状**：调用 `Tapp.federation.uploadMedia` 或其它 bridge 请求立刻失败，错误含
`Payload too large` 或 `Media data too large`。

**原因**：

1. **默认 bridge 上限**：绝大多数 action 的 JSON payload 约 **1 MiB + 64 KiB**。
2. **媒体专用上限**：`uploadMedia` 允许更大 data URL/base64，但仍受 raw **图片 10 MiB /
   视频 50 MiB** 与 base64 膨胀约束（见 [SANDBOX](./SANDBOX.md#payload-大小)）。
3. 先把整文件转成 data URL 再 postMessage，体积约为 raw 的 4/3，更容易触顶。

**解决方案**：

1. 上传前按类型检查 `file.size`（图片 ≤10 MiB，视频 ≤50 MiB）。
2. 压缩图片或降低视频码率后再 `uploadMedia`。
3. 不要对非 media action 塞大 base64（会撞默认 1 MiB）。
4. `file.download` 内容上限 10 MiB，与 storage 单值 1 MiB 不同。

### ❌ 附件 URL 被拒绝 / `Attachment URL must look like /media/federation/...`

**症状**：`createNote` / `publish` 失败，提示 attachment URL 非法。

**原因**：附件必须是本实例 `uploadMedia` 返回的联邦媒体 URL
（路径形如 `/media/federation/{userId}/{filename}`），不能塞任意 CDN 或 data URL。

**解决方案**：先 `uploadMedia`，再用返回的 `url` + `media_type` 填 `attachments`。

### ❌ Playground 预览里 `Tapp.federation.*` 失败

**症状**：生成代码在 Playground 预览调用联邦 API 报错或无 handler。

**原因**：临时预览不签发 Runtime Grant，也不注册 `FederationBridge`。

**解决方案**：预览只测 UI/storage；联邦能力在安装后正式运行验证。见
[PLAYGROUND_GENERATION_CONTEXT](./PLAYGROUND_GENERATION_CONTEXT.md)。

---

## 商店安装 / storeSource

### ❌ `tappList.install` 找不到商店源或 502

**症状**：分享卡片或 Tapp 内「安装」失败；后端报无法拉取商店 / 502；或安装了错误源的包。

**原因**：

1. SDK `Tapp.tappList.install({ source, tappId })` 里的 **`source` 是商店源 ID**
   （写入 REST 的 `storeSource`），**不是** 模式字面量 `"store"` / `"direct"`。
2. 后端容器访问不了 raw.githubusercontent.com 等外网时 store 安装失败；宿主可能回退为
   浏览器下载 + `source: "direct"`。
3. 直接上传 `.tapp` / 代码安装应走宿主 `install-file` 或 `source: "direct"`，不经
   `tappList.install`。

**解决方案**：

1. 传入真实源 id（如配置里的 `"1"`），与 [REST API](./REST_API.md) 商店安装示例一致。
2. 确认商店源已启用且 `tappId` 存在于该源 `index.json`。
3. 需要离线/内网包时使用文件安装或 direct 安装路径。

---

## 调试技巧

### 启用详细日志

```javascript
// 在代码开头添加
console.log("[Tapp Debug] 初始化开始");

Tapp.lifecycle.onReady(function () {
  console.log("[Tapp Debug] onReady 触发");
});
```

### 检查 Widget 渲染

```javascript
Tapp.widgets["my-widget"] = {
  render: function (container, props) {
    console.log("[Widget] render 调用，props:", JSON.stringify(props));
    console.log("[Widget] container:", container);
    console.log("[Widget] size:", props.size);
    // ... 渲染逻辑
  },
};
```

### 检查主题和语言

```javascript
Tapp.lifecycle.onReady(async function () {
  var locale = await Tapp.ui.getLocale();
  var theme = await Tapp.ui.getTheme();
  var color = await Tapp.ui.getPrimaryColor();
  console.log("[Theme]", { locale: locale, theme: theme, color: color });
});
```

---

## 最佳实践清单

### 发布前检查

- [ ] `manifest.main` 与实际入口一致
- [ ] 商店 `index.json` 的 `download.code` 能下载同一入口
- [ ] manifest 声明的所有资源文件都存在
- [ ] 版本号在 `index.json` 和 `manifest.json` 中一致
- [ ] Widget 模板文件格式正确
- [ ] 在多个 Widget 尺寸下测试显示效果
- [ ] 在亮色和暗色模式下测试样式
- [ ] `Tapp.api` 使用的名称已在 manifest 的 `apis` 中声明
- [ ] 无控制台错误（忽略已知的安全警告）
