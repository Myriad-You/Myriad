# Tapp 快速入门

Tapp (Third-party App) 是 Myriad 的扩展应用系统，允许开发者创建自定义小组件、工具和功能扩展。
当前推荐使用 `@myriad-you/tapp-cli` 创建、校验和打包项目；Manifest 字段与 SDK 能力分别见
[Manifest 配置](MANIFEST.md)和 [API 参考](API_REFERENCE.md)。CLI 的完整命令契约见
[Myriad Tapp CLI](../../../tools/tapp-cli/README.md)。

## For agents

Agent 和 CI 应固定包版本、显式指定 `myriad-tapp` binary，并始终使用 `--json`：

```bash
npx --yes --package=@myriad-you/tapp-cli@0.1.2 myriad-tapp init ./my-tapp --type page
npx --yes --package=@myriad-you/tapp-cli@0.1.2 myriad-tapp check ./my-tapp --json
npx --yes --package=@myriad-you/tapp-cli@0.1.2 myriad-tapp pack ./my-tapp --json
```

任何非零退出状态都表示失败。`check` 返回状态 `1` 时，读取 `diagnostics`、修复项目并重新
执行；只有 `check` 成功后才执行 `pack`。默认产物为
`my-tapp/dist/{manifest.id}.tapp`。CLI 当前不负责登录 Myriad 或上传产物。

## For users

需要 Node.js 20 或更新版本。全局安装固定版本后，创建一个 Page、Widget 或两者兼有的
starter：

```bash
npm install --global @myriad-you/tapp-cli@0.1.2
myriad-tapp init ./my-tapp --type page
```

编辑生成的 `manifest.json`、层入口文件、模板和样式，然后校验权限并打包：

```bash
cd ./my-tapp
myriad-tapp check .
myriad-tapp permissions .
myriad-tapp pack .
```

成功后，在 Myriad 的 Tapp 管理页或商店选择安装，上传
`dist/{manifest.id}.tapp`（或从商店安装）。宿主会弹出 **安装确认**（`InstallTappDialog`）
展示申请权限；服务器经 `POST /api/tapps/install-file` / `install` 写入安装态。浏览器
处理登录 Cookie 和 CSRF。接口见 [Tapp REST API](REST_API.md#需要登录的变更路由)，
包格式见 [`.tapp` 文件格式](../../features/TAPP_FILE_FORMAT.md)。

列表页支持 **我的 / 站点** 范围、卡片 `1x1`·`2x1` 与登录用户拖拽排序；布局 API 见
[列表布局](REST_API.md#列表布局-apitappslist-card-sizes)。若声明 `analytics:read`，
可用 [访问统计 SDK](API_REFERENCE.md#访问统计-api)（admin 完整 summary；user/guest 仅
访客卡片聚合；关闭采集时 `enabled: false` 短路）。站点 Agent 人设用
[人设名片](API_REFERENCE.md#人设名片-api)（无需权限；只读名字、心情带、同源立绘路径）。

Page 内可用 Canvas 2D / WebGL。Three.js 当作包内 guest 依赖放进 `page/` 并 require，贴图和
`.glb` 走 `Tapp.assets`，不要走 CDN。约定见 [图形与轻量游戏](GRAPHICS.md)；可安装的官方
示例是商店里的 `com.myriad.three-lab`。

---

## 代码架构

Tapp 按**层**组织代码：

```
manifest {
  core:    { entry: "core.js" }        // 共享层；也是 headless 后台入口
  page:    { entry: "page/index.js" }  // 页面层：页面渲染 + 生命周期
  widgets: [{ entry: "widget/index.js" }] // 小组件层：Widget 渲染逻辑
}
```

### 为什么分三层？

1. **避免代码冲突**：Widget 沙箱和 Page 沙箱加载不同的层，互不干扰
2. **更小的加载体积**：Widget 只加载 `core + widget`，Page 只加载 `core + page`
3. **清晰的职责分离**：每个部分专注于单一功能

运行时还有三种沙箱，不是同一套 `window.Tapp`：

### 代码加载规则

| 沙箱     | 加载的层                         | 能调用什么 |
| -------- | -------------------------------- | ---------- |
| Widget   | `core` + 该 widget               | 精简 SDK：生命周期、模板与 `render`；没有联邦 / 对局 / Tapp·Brew 管理 |
| Page     | `core` + `page`                  | 完整 SDK、生命周期和页面 UI |
| headless | 仅 `core`（常驻用这一套）        | 无可见 UI；`Tapp.widget` 只有定向 `invalidate({ widgetId })`；没有 register / `tappList` / 组件 / 快捷键 / 动态内容 / DOM / 文件下载 / `model3d` |

三种沙箱都会触发 `Tapp.lifecycle`（`onReady` / `onDestroy` / `onPause` / `onResume`），不能把
“是否有 onReady”当作分层边界。`onPause` 是**隐藏**，不是销毁。共享与调度逻辑放在 `core`；可见界面放在
`widget` / `page`（Widget 以 `Tapp.widgets[id].render` 为主，Page 可在 onReady 中挂载根 UI）。

### 代码结构示例

`core.js` 导出共享逻辑：

```javascript
function getThemeColors(isDark) {
  return {
    bg: isDark ? '#1a1a2e' : '#f8fafc',
    text: isDark ? '#e2e8f0' : '#1e293b',
    accent: '#6366f1',
  };
}

module.exports = { getThemeColors };
```

`widget/index.js` 只注册 Widget：

```javascript
var core = require('../core.js');

Tapp.widgets['my-widget'] = {
  render: async function(container, props) {
    var colors = core.getThemeColors(props.theme === 'dark');
    container.style.background = colors.bg;
    container.textContent = 'Widget Content';
  }
};
```

`page/index.js` 在已有 `page.html` 时绑定内容层（宿主不会调用 `Tapp.pages.render`）：

```javascript
var core = require('../core.js');

Tapp.lifecycle.onReady(async function() {
  var container = document.getElementById('tapp-content');
  var colors = core.getThemeColors(
    document.documentElement.classList.contains('dark')
  );
  container.style.color = colors.text;
  // 更新模板节点，不要把 #tapp-root 整层 textContent 清掉
});
```

---

## 渲染模式

Tapp 支持两种渲染模式，根据你的需求选择：

### 1. 纯 JS 模式（Pure JS）

代码完全通过 JavaScript 动态生成 DOM。适合简单应用或需要高度动态内容的场景。

```javascript
Tapp.widgets["my-widget"] = {
  render: function (container, props) {
    // 完全通过 JS 生成 HTML
    container.innerHTML = `
      <div class="glass rounded-xl p-4">
        <h3>${props.title || "标题"}</h3>
        <button id="my-btn">点击</button>
      </div>
    `;
    // 绑定事件
    container.querySelector("#my-btn").addEventListener("click", function () {
      alert("Clicked!");
    });
  },
};
```

### 2. 混合模式（Hybrid）

HTML 模板定义结构，JS 只负责事件绑定和数据填充。**推荐用于商店发布的 Tapp**。

**优势**：

- 模板与逻辑分离，更易维护
- 支持不同尺寸使用不同模板
- 模板可被设计工具预览

**文件结构**：

```
com.example.my-tapp/
├── manifest.json       # 必需：应用配置
├── core.js             # 共享层入口
├── page/index.js       # 可选：页面层入口
├── widget/index.js     # 可选：小组件层入口
├── page.html           # 可选：页面模板
├── styles.css          # 可选：自定义样式
├── widget-2x2.html     # 可选：2x2 尺寸模板
├── widget-4x2.html     # 可选：4x2 尺寸模板
└── widget-4x4.html     # 可选：4x4 尺寸模板
```

**manifest.json 配置**：

```json
{
  "id": "com.example.my-tapp",
  "core": { "entry": "core.js", "styles": "styles.css" },
  "page": { "entry": "page/index.js", "template": "page.html" },
  "widgets": [
    {
      "id": "my-widget",
      "entry": "widget/index.js",
      "defaultSize": "2x2",
      "sizes": ["2x2", "4x2", "4x4"],
      "templates": {
        "2x2": "widget-2x2.html",
        "4x2": "widget-4x2.html",
        "4x4": "widget-4x4.html"
      }
    }
  ]
}
```

**widget-2x2.html 示例**：

```html
<div
  class="h-full w-full flex flex-col p-3 glass rounded-xl"
  data-widget-root="true"
>
  <div class="flex items-center gap-2 mb-2">
    <span class="text-lg">🤖</span>
    <span class="font-semibold text-sm">我的应用</span>
  </div>
  <div class="flex-1 overflow-auto" data-content="main">
    <!-- JS 会填充这里 -->
  </div>
  <button
    data-action="refresh"
    class="mt-2 px-3 py-1 bg-indigo-500 text-white rounded-lg text-sm"
  >
    刷新
  </button>
</div>
```

**widget/index.js 混合模式示例**：

```javascript
Tapp.widgets["my-widget"] = {
  render: function (container, props) {
    // 模板已由系统加载到 container 中
    // 只需绑定事件和填充数据

    var contentEl = container.querySelector('[data-content="main"]');
    var refreshBtn = container.querySelector('[data-action="refresh"]');

    if (contentEl) {
      contentEl.textContent = "动态内容...";
    }

    if (refreshBtn) {
      refreshBtn.addEventListener("click", function () {
        loadData();
      });
    }

    function loadData() {
      // 加载数据逻辑
    }
  },
};
```

> **重要**：使用 `data-*` 属性标记需要 JS 操作的元素，避免使用硬编码的 ID 选择器。

---

## 商店发布（摘要）

完整目录协议、源管理、服务端/浏览器安装链路与检查清单见 **[Tapp 商店](STORE.md)**。

最少注意：

1. 商店 `index.json` 的 `download.code` 必须能下载到与 `core.entry` **同一份**入口代码（路径字符串不必相同）。
2. Manifest 声明的 `core.styles` / `page.template` / `page.styles` / Widget 模板等，索引 `download` 表与磁盘文件必须齐全。非 core 的层文件走 `download.modules`。
3. 索引与 Manifest 的 `version`、`category` 必须一致；`category` 用稳定用途 ID（见 [MANIFEST](MANIFEST.md#应用分类)）。
4. 二进制贴图等走 `manifest.assets`，**不要**写进 `download` 表；路径须在包根下的 `assets/`。
5. 大包在索引填写真实 `size`（字节），≥ 1 MiB 时宿主走客户端下载以显示进度。

官方目录仓库：[Myriad-You/tapp-store](https://github.com/Myriad-You/tapp-store)。

---

## 生命周期

不要把 SDK 回调名理解成「暂停=销毁」。沙箱实例和安装是两层：

| 动作 | 发生时机 | SDK | 结果 |
| ---- | -------- | --- | ---- |
| **隐藏** | 切 Tab、窗口最小化、滚出视野、多窗口最小化 | `onPause` / `onResume`（宿主发 `lifecycle:pause` / `lifecycle:resume`） | iframe、桥、授予权限和会话令牌都还在；只通知停止活动 |
| **销毁** | 停应用、代码变更、登录态/权限变更、暂存池淘汰、离开运行页（该页 Page iframe） | `onDestroy` | 这个沙箱实例释放，不可恢复；应用仍装着 |
| **卸载** | 用户从平台移除该 TAPP | 先销毁全部实例，再清存储与注册 | 应用不再装着，需重新安装 |
| **常驻** | 声明了 `backgroundRequirements` 或运行时 `Tapp.background.require` | headless core 跨页承接 | 没有页面 iframe，只有后台 core |

```javascript
Tapp.lifecycle.onReady(async () => {
  // 初始化；三种沙箱都会触发
});

Tapp.lifecycle.onPause(() => {
  // 隐藏：停 rAF / 音频，不要当作已经销毁
});

Tapp.lifecycle.onResume(() => {
  // 重新可见
});

Tapp.lifecycle.onDestroy(async () => {
  // 本实例即将释放
});
```

---

## Widget 预注册机制

**重要**：Widget 从 Manifest 自动预注册，无需在代码中手动注册！

### 注册时机

| 时机        | 行为                                    |
| ----------- | --------------------------------------- |
| Tapp 安装时 | 从 `manifest.widgets` 预注册所有 Widget |
| Tapp 运行时 | Widget 渲染函数可用                     |
| Tapp 未运行 | Widget 显示"需要启动"提示               |

### Dashboard 显示规则

- Widget 在 Tapp 安装后即可添加到 Dashboard
- Widget 只在 Tapp **运行中**时真正渲染
- 未运行时显示启动提示，用户可点击启动 Tapp

```javascript
// manifest.json 中声明 widgets（自动注册）
{
  "widgets": [
    {
      "id": "my-widget",
      "name": "我的小组件",
      "defaultSize": "2x2",
      "sizes": ["1x1", "2x2", "4x2"]
    }
  ]
}

// Widget 代码中只需定义渲染函数
Tapp.widgets['my-widget'] = {
  render: function(container, props) {
    // 渲染逻辑
  }
};
```

---

## 数据加载与更新

Widget / Page **不能**直接 `fetch`。外部 HTTP 必须写在 `manifest.apis` 里，再用
`Tapp.api(name, params)`。同一 **subject** 的 Page、Widget、headless 共享 `Tapp.storage`；
宿主默认在 storage 变更时刷新可见 Widget（`widgets[].refreshPolicy`）。公开部署要展示
站长数据时写 `Tapp.shared`（安装级，访客可读），不要写 `Tapp.storage`（每人一份空仓库）。

### 推荐模式

```
core: Tapp.api(...) → Tapp.storage.set(key, data)
                    ↓ 宿主广播
Widget: render() 里 Tapp.storage.get(key)   （或 onChanged 局部更新）
```

**1. Manifest 声明 API 与刷新策略**

```json
{
  "permissions": ["network:fetch", "storage:read", "storage:write", "scheduler:register"],
  "apis": {
    "feed": {
      "type": "http",
      "endpoint": "https://api.example.com/feed",
      "method": "GET",
      "access": "protected"
    }
  },
  "backgroundRequirements": ["scheduler", "sync"],
  "widgets": [
    {
      "id": "feed",
      "name": "Feed",
      "defaultSize": "2x2",
      "sizes": ["2x2"],
      "refreshPolicy": { "mode": "event", "refreshOnVisible": true }
    }
  ]
}
```

> 权限字符串必须是宿主白名单里的完整 token。存储读取使用 **`storage:read`**，写入使用
> **`storage:write`**；定时任务权限是 **`scheduler:register`**
> （`backgroundRequirements` 里的 `"scheduler"` 是后台能力类型，不是权限 token）。
> 未知权限会在安装时被拒绝或过滤。

**2. core 拉数写 storage**

```javascript
// core.js — 三种模式都会先加载共享层
// ⚠️ 只写 storage 即可。无 options 的 invalidate 只存在于 Widget 沙箱；
//    Page/headless 必须带 { target: { widgetId } }，且已有 storage:write。
async function pullFeed() {
  const data = await Tapp.api("feed", {});
  await Tapp.storage.set("feed.latest", data);
}

Tapp.lifecycle.onReady(async () => {
  await pullFeed();
});
```

**3. Widget 读 storage 渲染**

```javascript
Tapp.widgets["feed"] = {
  render: async function (container) {
    const feed = (await Tapp.storage.get("feed.latest")) || { items: [] };
    container.textContent = `${feed.items.length} items`;
  },
};
```

### 何时用哪种触发

| 触发 | 用法 | 谁能用 | 说明 |
| ---- | ---- | ------ | ---- |
| storage 写入 | `await Tapp.storage.set(k, v)` | **Page / Widget / headless** | **默认路径**：同 Tapp 广播，刷新全部可见 Widget（与 `mode` 无关） |
| 显式 invalidate（自己） | `await Tapp.widget.invalidate("reason")` | **仅 Widget 沙箱** | 当前实例 remount |
| 定向 invalidate | `await Tapp.widget.invalidate("reason", { target: { widgetId } })` | Page / headless / Widget | 需授予的 `storage:write`；每卡 15s、每 Tapp 2 次/分；没有 `all` |
| 订阅变更 | `Tapp.storage` / `settings` / `shared.onChanged(cb)` | Page / Widget / headless | 局部改 DOM；`settings` **只通知不拆卡**；写者自己不会收到 |
| 可见轮询 | `refreshPolicy.mode: "interval"` | 宿主计时器 | 额外的可见节拍；**不要**当后台同步 |
| 后台同步 | `backgroundRequirements` + scheduler / headless | headless core | 离开 UI 后仍要跑的任务 |

要点：

- `render` 保持幂等：优先读 storage，避免每次 re-render 都打外部 API。
- 平台只读接口（如 `Tapp.platform.*`）≠ 你声明的业务 `apis`。
- Page 的 `Tapp.widget` 是 **register 系列**（`widget:register`）加上定向
  `invalidate({ widgetId })`（`storage:write`）。headless 只有后者。Widget 沙箱里
  无 options 的 `invalidate` 仍只刷自己。
- 完整示例与注意项见 [WIDGET — 数据加载与更新](WIDGET.md#数据加载与更新)；
  API 细节见 [API 参考](API_REFERENCE.md)；`refreshPolicy` 字段见 [Manifest](MANIFEST.md)。

---

## 常驻（headless core）

离开运行页会**销毁**该页的 Page 沙箱，不是把页面 iframe「冻结」起来。跨页仍要跑的逻辑必须**声明常驻**（headless core）；没声明时离开页面后实例就没了。

需要在应用重载后、尚未打开 Page/Widget 时就启动 core 的任务，应在 manifest 中声明需求：

```json
{
  "backgroundRequirements": ["scheduler", "sync"]
}
```

运行时的 `Tapp.background.require/release` 适合动态增减需求；manifest 声明则负责首次启动和刷新后的恢复。两类来源独立计数，`release` 不会取消 manifest 的常驻声明。常驻实例只加载共享层 `core`，不加载 Page HTML/CSS 或 Widget 视图代码。

后台同步结果应写入 `Tapp.storage`，让可见 Widget 通过默认 `refreshPolicy` 自动更新（见上一节），不要依赖 Widget 可见 interval。

### 后台需求类型

| 类型             | 说明                     | 典型场景                 |
| ---------------- | ------------------------ | ------------------------ |
| `media`          | 媒体控制                 | 音乐播放器扩展           |
| `sync`           | 后台数据同步             | 定时从 API 拉取数据      |
| `notification`   | 定时通知                 | 提醒类应用               |
| `scheduler`      | 定时任务                 | 自动执行脚本             |
| `event-listener` | 事件监听（跨 Tapp 通信） | 需要响应其他 Tapp 的事件 |
| `realtime`       | 实时数据更新             | 需要 WebSocket 类通信    |

### 使用示例

```javascript
Tapp.lifecycle.onReady(async function () {
  // 声明需要后台同步数据
  await Tapp.background.require("sync", "每5分钟同步一次数据");

  // 周期任务请用 Tapp.scheduler + onTask（持久、可在 headless 恢复），
  // 不要用 setInterval 冒充离开页面后仍运行的后台
  await Tapp.scheduler.register({
    taskId: "sync-feed",
    name: "同步 Feed",
    scheduleType: "interval",
    schedule: { interval: 5 * 60 * 1000 },
    executionTarget: "frontend",
  });
  Tapp.scheduler.onTask("sync-feed", async () => {
    await pullFeed(); // 内部 Tapp.api → storage.set，Widget 自动刷新
  });
});
```

---

## 下一步

- [Manifest 配置](./MANIFEST.md) - 完整的配置选项说明
- [Tapp 商店](./STORE.md) - 远程目录、安装与发布
- [API 参考](./API_REFERENCE.md) - 所有可用 API 的详细文档
- [小组件开发](./WIDGET.md) - Widget 样式与[数据加载与更新](WIDGET.md#数据加载与更新)
- [页面样式规范](./PAGE.md) - 宿主 React chrome 对照（沙箱样式用 STYLING / DESIGN_SPEC）
- [样式规范](./STYLING.md) - Glass Morphism 设计规范
- [安全沙箱](./SANDBOX.md) - 沙箱限制和安全机制
- [故障排除](./TROUBLESHOOTING.md) - 安装与运行时问题
