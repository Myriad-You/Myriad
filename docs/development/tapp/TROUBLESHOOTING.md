# 故障排除指南

本文档汇总 Tapp 开发中常见的问题和解决方案，帮助你快速定位和修复错误。

---

## 控制台错误

### ❌ `Not allowed to load local resource: blob:...`

**症状**：控制台显示此错误，但 Tapp 功能正常。

**原因**：浏览器在 iframe 沙箱中清理 blob URL 时的正常行为。当该沙箱被销毁或刷新时触发。

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

1. **外部脚本**：不支持直接加载外部 JS 文件；把代码放进包内并用相对路径 `require`
2. **外部样式**：使用 `styles.css` 文件或内联样式
3. **图片**：默认只允许 `data:`、`blob:` 和宿主同源。外链图要申请 `network:fetch`。
   `Tapp.persona.get()` 的 `portraitUrl` 已是同源路径，直接 `<img src>`，不要为此再申请
   `network:fetch`
4. **媒体**：CSP 默认禁止直接加载；使用宿主媒体 API
5. **API 请求**：在 Manifest 的 `apis` 中声明，再调用 `Tapp.api(name, params)`

---

### ❌ `prefetch-src` / `navigate-to` 未识别警告

**症状**：

```
The Content-Security-Policy directive 'prefetch-src' is not implemented...
Unrecognized Content-Security-Policy directive 'navigate-to'.
```

**原因**：浏览器未实现或已废弃的 CSP 指令出现在沙箱 / 预览 HTML 的 meta CSP 中。

**解决方案**：维护沙箱或预览 HTML 时不要写入 `prefetch-src`、`navigate-to` 等非标准指令。
导航与表单由 iframe `sandbox`、`form-action 'none'` 与 SDK 受控 API 约束。Myriad 正式沙箱
与商店预览 CSP 已不包含这些指令。

---

### ❌ 商店预览空白 / 只有主题色占位

**症状**：精选卡或详情没有应用 UI 预览，只显示图标+「预览不可用」。

**常见原因与处理**：

1. **`preview.html` 写成了内联 HTML**
   `preview.html` / `styles[]` 必须是相对 `base_url` 的**路径**（或 https URL），不能是
   markup 字符串。见 [STORE · 静态预览](STORE.md#静态预览-preview商店-merchandising-快照)。
2. **页面依赖 JS / 外链图**
   宿主会剥脚本与非 `data:`/`blob:` 的 `url()`；应用只剩空挂载点会被判定为运行时壳而丢弃。
   请提供纯静态快照，或确保 page_template 回退后仍有可见文本/背景。
3. **详情页横向溢出过大**
   `isRenderedPreviewAdapted` 在文档宽远超 viewport 时会 fallback；检查预览 CSS 是否把
   布局撑到极宽。
4. **路径 404**
   `downloadAppPreview` 失败只打 warn，不阻断安装；核对 `base_url` + 路径。

精选卡约 2.8s 超时后仍会尽量显示已清洗内容；详情对「不适配」更严。

---

### ❌ 游客运行公开 Tapp 时 `settings` / `federation/channels` 401

**症状（旧版或未对齐宿主时）**：

```
Failed to load resource: the server responded with a status of 401 ()
/api/tapps/<id>/settings
/api/federation/channels
```

**当前契约**：

1. **Settings 读**：`GET …/settings` 为 **optional_auth**。游客打开**公开**安装时应能读到
   installation owner 已保存的声明设置；写仍需登录。Tapp 应用 `get`/`getAll`，未保存则用
   Manifest 默认值。
2. **Channel 列表**：`Tapp.federation.getChannels()` 在无登录会话时由宿主返回空列表，不发
   401 请求。Channel 写路径仍要求登录 + 相应 `federation:*` 权限。
3. Tapp 侧应用 `Tapp.user.getRole()` 隐藏游客不可用的 UI，不要把 401 当正常分支展示给用户。

若仍看到 settings 401：确认打开的是可见的公开安装（非 `visibility: admin` 对游客），且宿主
版本已包含 optional_auth settings 路由。

---

### ❌ 媒体 API 里 VIP 歌播不了 / 列表没有 VIP

**两层问题不要混**：

1. **宿主「跳过 VIP」开关**（默认开启）
   - `getSkipVip()` → `{ skipVip: true }` 时自动切歌会跳过 VIP 曲。
   - **打开 VIP 显示/入队**：`await Tapp.media.setSkipVip(false)`（需 `media:control`）。
   - 与系统音乐播放器「显示 VIP 歌曲」为同一状态。
2. **平台会员/试听**
   - 即使 `skipVip === false`，网易云等仍可能对 VIP 曲返回不可播；这是源站策略，不是
     Tapp SDK 开关能绕过的。应提示用户或换曲，不要死循环 `next()`。

详见 [API 参考 · 媒体控制](API_REFERENCE.md#媒体控制-api)。

---

## 文件加载问题

### ❌ Tapp 安装后显示空白或报错

**可能原因**：

1. **入口文件不存在或商店下载路径不一致**

   ```json
   // manifest.json：安装后的入口
   { "core": { "entry": "core.js" } }

   // 商店 index.json：仓库中的下载位置
   { "download": { "code": "apps/com.example/core.js" } }
   ```

2. **manifest 声明的资源缺失**

   ```json
   // manifest.json 声明了这些文件，但实际不存在
   {
     "styles": "styles.css", // 文件不存在
     "page": { "template": "page.html" }, // 文件不存在
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

1. 检查层入口是否指向安装根目录内真实存在的 JS 文件
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
2. 任意 `type: "http"` API（含 `public`、`protected` 与 `manager`）未授予 `network:fetch`
3. 后端出站安全或参数模板校验拒绝了请求
4. 游客调用 `access: "public"` API，但站点的游客 `network:fetch` 策略未开启（默认关闭）
5. 其他程序调用 `/tapi/{tappId}/{path}` 时未带 HMAC / timestamp / nonce，或密钥未配置 / 重放（一律 401；`ROUTE_VERIFY_REPLAY` 只在 HMAC 已通过后出现）

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
不会免除 `network:fetch`。管理员还须在权限设置中开启游客 `network:fetch`，安装时批准该权限；
调用仍会经过共享限流、Manifest 与后端出站安全校验。

### ❌ 凭据 API 返回“未配置”或“需要重新授权”

**症状**：绑定了 `apis.*.credential`，但执行具名 API 时提示 credential 未配置，或管理页显示
需要重新授权。

**原因与处理**：

- 新安装尚未由 installation owner / 当前管理员输入值：在 Tapp 详情页配置；SDK 与普通 viewer
  没有读取或写入凭据的接口。
- endpoint、credential `in`/field/header/prefix/`sign`、`access` 或同一凭据绑定的其他 API 定义发生变化：这是
  授权指纹变化，必须由 owner 重新输入，不会自动沿用旧授权。
- 从 settings 迁移：先在第三方服务轮换已可能暴露的旧 Key，再配置 credential；不要复制旧
  settings 值继续使用。
- CLI 报 fixed HTTPS、undeclared、forbidden header、duplicate header 或 unbound：按提示修复
  Manifest。凭据只能绑定固定绝对 HTTPS origin 的具名 HTTP API，每个声明都必须至少绑定一次。

---

## AI 生图参考图

### ❌ 参考图报错或未按预期参与生图

`Tapp.ai.tasks.create` 使用 `operation: "image"`，参考图放在 `input.referenceImages`，
例如 `{ prompt: "保留第一张图的人物，采用第二张图的画风", referenceImages: [subjectDataUrl, styleDataUrl] }`。
Manifest 必须同时声明 `ai:image` **和** `manifest.ai`（`protocolVersion: 2`、`operations`
含 `"image"`、`outputFormats` 含 `"image"`）；缺 `ai` 块会返回 `AI_V2_NOT_DECLARED`。
`create()` 返回 `queued`，要 `get` / `subscribe` 等到 `completed` 再读
`task.result.value.url`。实际调用由授予权限决定。Playground 临时预览不执行 AI。

| 症状 / 错误码 | 检查与处理 |
| ------------- | ---------- |
| 参考图没有传入 | 使用 `input.referenceImages` 有序数组；不要只把 URL 写进 prompt，也不要放在 `context` 或自行命名 `images` 字段 |
| `INVALID_AI_IMAGE_REFERENCE` | 使用完整 PNG/JPEG/WebP base64 data URL；文件类型必须匹配文件头。本地缓存路径必须存在，不能直接传外部 URL、`blob:`、包内路径或 SVG/GIF |
| `AI_IMAGE_REFERENCE_LIMIT` | 最多 4 张，解码后合计 ≤10 MiB；先缩图或压缩，再转为 data URL |
| `AI_TASK_INPUT_LIMIT` | 除参考图外的序列化 `input` ≤256 KiB；含 base64 的完整输入也有总上限 |
| Bridge `Payload too large` | 带参考图数组的生图请求 JSON payload ≤14 MiB；其余任务仍使用默认预算 |
| 任务进入 `failed`，`AI_PROVIDER_ERROR` | 检查宿主生图配置和所选模型是否接受参考图及当前张数；供应商详情不会回传沙箱。平台不会丢弃参考图后重试文生图 |
| `IDEMPOTENCY_KEY_REUSED` | 修改或调换参考图后使用新幂等键；同一键只能对应同一请求 |

通过 `FileReader.readAsDataURL(file)` 转换本地文件；包内图片先用 `Tapp.assets.getArrayBuffer`
取得字节。复用生成图片时，传已完成任务结果的 `value.url` 缓存路径。代码示例见
[AI API](API_REFERENCE.md#ai-api)。Playground 临时预览不执行 AI 任务，需安装后验证。

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
4. `file.download` 文本 / base64 / 宿主代取生图·3D 上限 32 MiB，与 storage 单值 1 MiB 不同。

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

完整协议见 **[Tapp 商店](STORE.md)**。

### ❌ `tappList.install` 找不到商店源或 502

**症状**：分享卡片或 Tapp 内「安装」失败；后端报无法拉取商店 / 502；或安装了错误源的包。

**原因**：

1. SDK 商店路径的 **catalog 引用**只认 `storeSource`，或 **http(s) 形式的 `source`**。
   裸 `{ source: "1", tappId }` **不会**把 `"1"` 当源 id，会直接 `Invalid source`。
   `storeSource` 也不能是模式字面量 `"store"` / `"direct"`。
2. 后端容器访问不了 raw.githubusercontent.com 等外网时 store 安装失败；宿主可能回退为
   浏览器下载 + REST `source: "direct"`。
3. 上传 `.tapp` 走 `install-file`；SDK 内联包用 `tappList.install({ source: "direct",
   manifest, modules, ... })`（见 [API 参考 · Tapp 列表](./API_REFERENCE.md#tapp-列表-api)）。

**解决方案**：

1. 使用 `{ source: "store", storeSource: "1", tappId }` 或
   `{ source: "https://…/index.json", tappId }`（见 [STORE](./STORE.md) /
   [REST API](./REST_API.md)）。
2. 确认商店源已启用且 `tappId` 存在于该源 `index.json`。
3. 离线/内网包使用 `install-file` 或 SDK/REST direct 安装。

### ❌ 商店安装报 category 不匹配 / 缺 page 资源

**症状**：后端 `BAD_GATEWAY` 提示 category 不一致，或缺少 `page_styles` / `page_template`。

**原因**：索引 `apps[].category` 与 `manifest.category` 不一致；或 Manifest 声明了
`page.styles` / `page.template` 但 `download` 表未给出对应路径或远程 404。

**解决方案**：对齐分类稳定 ID；补齐 `download` 路径并保证 `base_url` 下可 GET。见
[STORE · 索引检查清单](STORE.md#索引检查清单)。

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

- [ ] 层入口与实际文件一致
- [ ] 商店 `index.json` 的 `download.code` 能下载同一入口
- [ ] manifest 声明的所有资源文件都存在
- [ ] 版本号与 **category** 在 `index.json` 和 `manifest.json` 中一致
- [ ] 声明了 `page.styles` / `page.template` / Widget 模板时，索引 `download` 路径齐全
- [ ] `manifest.assets` 文件位于包根 `assets/` 且远程可下载
- [ ] 大包填写索引 `size`（字节）
- [ ] Widget 模板文件格式正确
- [ ] 在多个 Widget 尺寸下测试显示效果
- [ ] 在亮色和暗色模式下测试样式
- [ ] `Tapp.api` 使用的名称已在 manifest 的 `apis` 中声明
- [ ] 无控制台错误（忽略已知的安全警告）
- [ ] 完整清单见 [STORE.md](STORE.md#索引检查清单)
