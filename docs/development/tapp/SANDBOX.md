# Tapp 安全沙箱

本文说明当前 Tapp Page、Widget 与 headless core 的浏览器隔离边界。整体运行链路见
[Tapp 架构](ARCHITECTURE.md)，可调用能力见 [SDK API](API_REFERENCE.md)。

## 安全边界

Tapp 代码不会进入 Myriad 主页面的 JavaScript 上下文，而是在 `srcdoc` iframe 中运行。
宿主按顺序建立以下边界：

1. iframe sandbox 只开放脚本和 pointer lock；
2. nonce CSP 阻止未授权脚本、连接、媒体、子 frame 和表单提交；
3. 安全包装器禁用网络、动态代码、本地存储、弹窗和直接父窗口操作；
4. 随机会话 token 与 `event.source` 约束 `postMessage` 来源；
5. `TappBridge` 对每个 action 做静态权限检查并只分发已注册 handler；
6. 涉及数据、出站网络或持久化的调用由后端再次校验身份、owner 和权限。

前端沙箱是纵深防御的一层，不是后端授权的替代品。

## Widget 加载与宿主骨架

Dashboard 上的 Widget 沙箱在 **iframe 发出 ready（`tapp.ready` / lifecycle ready）之前**，
宿主会盖一层通用 **Widget Skeleton**（`preset=block`，默认延迟 100ms 再绘制 shimmer，
避免极快路径闪屏；`manifest.themeColor` 用于染色）。

- 骨架由宿主绘制，不进入 Tapp JS 上下文，也不占用沙箱权限。
- Tapp 侧应尽快 ready；超过默认 **12s** 仍未 ready 时，骨架底部显示「加载较慢」提示
  （`common.loadingSlow`），仍优于永久空白。
- 屏外 Dashboard 卡使用 **hold** 面（无 bone、无动画），不在屏外空转 shimmer。
- 完整规范见 [小组件开发指南 · 宿主加载骨架](WIDGET.md#宿主加载骨架widget-skeleton)。

## iframe 属性

当前宿主实际设置：

```html
<iframe
  sandbox="allow-scripts allow-pointer-lock"
  referrerpolicy="no-referrer"
></iframe>
```

- `allow-scripts` 是运行 Tapp 的必要能力。
- `allow-pointer-lock` 用于游戏等交互。
- 不开放 `allow-same-origin`。`srcdoc + allow-scripts + allow-same-origin` 会显著削弱
  sandbox 隔离。
- 不开放 popups、modals、forms、downloads 或 top navigation。需要通知、确认、全屏、
  文件下载等能力时必须调用对应的 Tapp SDK，由宿主决定是否执行。
- `allowFullscreen` 由宿主单独设置，但调用仍经过 `ui:fullscreen` 权限与 handler。

## CSP

每个沙箱生成独立随机 nonce。当前默认策略等价于：

```text
script-src 'nonce-<random>' 'wasm-unsafe-eval';
default-src 'none';
style-src 'unsafe-inline' https://fonts.googleapis.com;
img-src data: blob: <host-origin>;   /* + https: http: 当授予 network:fetch */
font-src data: https://fonts.gstatic.com;
connect-src blob: data:;
frame-src 'none';
object-src 'none';
media-src 'none';   /* media:audio → blob: data:；远程音视频/图需 network:fetch */
worker-src 'none';
form-action 'none';
base-uri 'none';
manifest-src 'none'
```

- `script-src` 仅 nonce（+ 可选 `'wasm-unsafe-eval'`），不放行任何外部脚本 host（含
  Tailwind CDN）。Tailwind 在安装时预编译为 CSS，经 `sandbox/styles.ts` 的
  `TAILWIND_MAP` 注入。
- `img-src` 默认仅 `data:` / `blob:` / 宿主同源。需要外链封面、CDN 图时，在
  `manifest.permissions` 声明 **`network:fetch`**（安装时由用户授权）；CSP 会
  追加 `https:` / `http:`。不要用 `/api/proxy/image` 折中绕过声明。
  `Tapp.persona.get()` 的 `portraitUrl` 已是宿主同源路径，直接 `<img src>` 即可，不要
  再申请 `network:fetch` 去拉立绘。
- `connect-src` 仅 `blob:` / `data:`：包内 Loader（Three `FileLoader`、`.glb`）
  可以读沙箱自己创建的 blob。即使有 `network:fetch`，也不能直接 `fetch` `https:` /
  XHR/WS，出站仍走 Manifest `apis` + Bridge。包装器同样拒绝非 blob/data 的 `fetch`。
- 联邦入站的 `game:<tappId>:<protocol>` 房间消息不过关键词过滤（仍检查成员、签名、
  体积、频率和域名拉黑）。`message_type` 必须等于该房间 `shared_data_config.game`
  绑定的类型，体积按房间 `maxMessageBytes`（默认 64 KiB）。普通聊天和 Note 仍走
  内容过滤。`body` 是不透明 JSON，关键词看不到里面的文本。
- `'wasm-unsafe-eval'` 仅用于 WebAssembly 编译，不等于开放 `eval`。
- `media:audio` 仅把 `media-src` 放宽到 `blob: data:`；任意远程音视频同样要
  `network:fetch`。
- Manifest 不能覆盖这份 CSP；如果确实需要新的资源能力，应修改并审计宿主策略，而不是让
  单个 Tapp 放宽隔离。
- 不要在沙箱 HTML 里追加浏览器未实现或已废弃的指令（如 `navigate-to`、`prefetch-src`）：
  只会在控制台产生 `Unrecognized Content-Security-Policy directive` 噪音，不提高安全性。
  导航与表单已由 `sandbox` 属性 + `form-action 'none'` + 禁用 `window.open` 约束。

轻量游戏 / Canvas / 包内资源约定见 [图形与轻量游戏](GRAPHICS.md)。

## 被禁用的浏览器能力

安全包装器在 Tapp 代码执行前处理这些能力：

| 浏览器能力                                               | 当前行为                 | 使用方式                                      |
| -------------------------------------------------------- | ------------------------ | --------------------------------------------- |
| `fetch`、`XMLHttpRequest`、`WebSocket`、`EventSource`    | `fetch` 仅 blob:/data:；其余禁用 | 包内资源用 `Tapp.assets`；出站用 Manifest `apis` |
| `localStorage`、`sessionStorage`、`indexedDB`、Cache API | 禁用或替换为空实现       | `Tapp.storage`                                |
| `eval`、带源码的 `Function`、字符串形式的 timer          | 禁用                     | 使用预打包代码和函数回调                      |
| `window.open`、`alert`、`confirm`、`prompt`、`print`     | 禁用                     | `Tapp.ui.showNotification/confirm`；外链用 `Tapp.ui.openUrl` + Manifest `openUrls` |
| `window.parent/top/opener`                               | 限制                     | 只使用生成的 Tapp SDK                         |
| 直接下载                                                 | sandbox 未开放 downloads | `Tapp.file.download()`                        |

部分浏览器属性不可重新定义，因此安全性不能依赖包装器单点；iframe sandbox、CSP、消息
来源校验和后端检查共同构成边界。

## Bridge 消息验证

Tapp SDK 把调用转换为请求消息。宿主只接受同时满足以下条件的消息：

- 消息结构、请求 ID、时间戳和 payload 合法；
- `event.source` 是当前 iframe 的 `contentWindow`；
- 消息携带该实例的随机 session token；
- action 存在于权限映射中；
- action 所需权限已出现在后端返回的 `granted_permissions` 中；
- 当前 Page/Widget/headless 宿主注册了对应 handler。

未知 action 默认拒绝。Page 与 Widget 注册的 handler 集不同，因此“SDK 上能看到方法”
不等于每个运行模式都支持该方法；新增能力时要同步核对两类沙箱。

### Payload 大小

`TappBridge` 在校验阶段限制 request payload，防止内存攻击（实现见
`frontend/src/tapp/runtime/TappBridge.ts`）：

| action | 上限 | 说明 |
| ------ | ---- | ---- |
| **默认**（绝大多数 API） | JSON 序列化后约 **1 MiB + 64 KiB** envelope | 超限返回 `Payload too large` |
| `file.download` | 内容 Blob **10 MiB**；`filename` ≤ 1024；可选 `mimeType` ≤ 256 | 不走默认 1 MiB；非法/过大返回 `Invalid or oversized file payload` |
| `federation.uploadMedia` | raw 媒体按业务 **图片 10 MiB / 视频 50 MiB**；bridge 允许 data URL/base64 字符约 `ceil(50 MiB * 4/3) + 256` | 对齐后端 multipart 路由（body 上限 55 MiB）；字段 `data` 必填字符串 |
| `tappList.install` / `tappList.getInstallPackage` | JSON 序列化后约 **128 MiB + 512 KiB** | 对齐游戏档整包上限；商店安装只传元数据，不占这笔预算 |

说明：

- 默认 1 MiB 针对的是 **postMessage JSON payload**，与 `Tapp.storage` 单值 1 MiB 是不同层，
  但数量级一致，避免大 blob 经 bridge 灌入主线程。
- **商店**安装：`tappList.install` 只传元数据（`source: "store"` + `storeSource`，或 HTTP
  catalog `source`，加 `tappId`），包体不经 Bridge；由后端 REST `source=store` 下载，或宿主
  在失败/大包时浏览器下载后再 REST `source=direct` 安装。
- **SDK 直接安装**：`source: "direct"` 时 `manifest`/`code`/可选资源会经 Bridge 传到宿主
  `installDirect`（JSON 上限约 128 MiB；base64 资源会膨胀，超大包仍优先商店或 `install-file`）。
- **`.tapp` 文件上传**：走宿主 `POST /api/tapps/install-file`，不经 sandbox Bridge。
- Playground 临时预览的 storage 单值同样限制约 1 MiB（内存实现），且**不**提供
  federation handlers。

## 网络请求

不要把任意 URL 交给 SDK。先在 Manifest 声明固定能力：

```json
{
  "permissions": ["network:fetch"],
  "apis": {
    "profile": {
      "type": "http",
      "endpoint": "https://api.example.com/users/{{params.id}}",
      "method": "GET",
      "access": "protected",
      "description": "读取指定用户资料"
    }
  }
}
```

沙箱按名称调用：

```javascript
const profile = await Tapp.api("profile", { id: "42" });
```

所有 HTTP API 都要求实时 `network:fetch`；`public`/`protected` 只决定调用者范围，并且都受 Manifest、输入模板、
后端出站安全和共享频率限制约束。端点、请求头和模板参数会在后端解析，HTTP 请求经过 SSRF
防护。系统不存在供 Tapp 使用的任意 URL 代理。

## 权限

Manifest 的 `permissions` 是申请集合；安装批准后写入 `approved_permissions`，运行时的
`granted_permissions` 则由批准集与当前角色/下放策略动态求交集：

```json
{
  "permissions": ["storage:read", "ui:theme", "platform:read"]
}
```

当前权限等级语义：

| 等级         | 含义                                         |
| ------------ | -------------------------------------------- |
| `public`     | action 不要求 Manifest 权限                  |
| `basic`      | 基础能力，仍须申请并被授予                   |
| `elevated`   | 管理员可以通过权限下放配置授权给非管理员     |
| `privileged` | 仅管理员，例如平台写入/注册和 Agent 组件注册 |

前端显示的等级只用于说明和快速拒绝，后端动态授权结果才是最终事实。

## DOM 与输入

DOM 本身可在 iframe 内使用，但所有不可信内容都必须转义：

```javascript
const item = Tapp.dom.createElement("li", { text: userInput });
container.appendChild(item);

// 或使用原生安全赋值
container.textContent = userInput;
```

不要把外部输入直接拼进 `innerHTML` 或事件属性。`Tapp.dom.setSafeHtml()` 会把输入当文本
转义，它不是 HTML sanitizer。

存储 key 只允许字母、数字、下划线、连字符、点和冒号，长度上限为 256，并拒绝路径
遍历形式。后端会拒绝超过 1 MiB 的单值，并在事务内对同一当前用户与 Tapp 串行计算
写入后的总量；超过 8 MiB 会返回 413。数据库触发器执行相同的 8 MiB 硬限制，覆盖其他
内部写入路径；`Tapp.storage.usage()` 返回的 quota 因而也是实际安全边界。

持久 storage 命名空间跟随 Runtime Grant **subject**（持久用户或**签名游客 session**），
即使运行的是站点公开安装也不会读取安装 owner 的 storage。每个 subject 读写自己的
`user_id + tapp_id` 空间（游客为负 id）；`storage:read` 为 guest-safe basic，签名游客可获
Grant 与持久 storage。`_settings.`、`_shared.`、`_component:`、`_shortcut:`、`_report:` 是宿主保留
前缀，不能通过 `Tapp.storage` 读取、写入、列举或清除。

Manifest **安装级 settings** 由 owner / 管理员写入 installation owner 命名空间；通过
`Tapp.settings` 读取时，凡能打开该安装的 viewer（**含游客打开公开安装**）都能读到已保存
值。这与 storage 的 subject 隔离不同——公开 Tapp 的配置对访客可见是预期行为。

**安装级 shared** 与 settings 同一隔离、同一读写角色，但键不必在 Manifest 声明，语义是
站长数据仓库。公开部署把要展示的 owner 数据写入 `Tapp.shared`，不要塞进 settings。

## 开发检查清单

- Manifest 只申请实际使用的权限。
- 外部请求全部通过具名 `apis` 和 `Tapp.api()`。
- Tapp 不依赖主页面 DOM、Cookie 或浏览器存储。
- 不把密钥写入 Manifest、Tapp 代码、日志或参数。
- 站主第三方 Key 使用 Manifest `credentials` + `apis.*.credential`；沙箱只调用具名 API，
  不读取凭据。按声明放入请求头、query、form 或仅用于签名。只绑定可信的固定 HTTPS origin。
- Page、Widget、headless 模式分别验证所需 handler。
- 用户输入和外部响应在进入 DOM 前完成类型、长度和内容校验。
- 新增 SDK action 时同步更新权限映射、宿主 handler、后端校验和文档。
- 若涉及 speech / brew / federation 宿主代理：先改
  `docs/development/tapp/fixtures/action_permissions.json`（及需要时的
  `host_route_permissions.json`），再改 `permissionConfig.ts` 与后端
  `host_attribution`；跑
  `node --experimental-strip-types --test src/tapp/runtime/permissionMapConsistency.test.ts`
  与 `cargo test -p myriad-backend host_attribution`（或对应模块过滤）。

## 已知控制台信息

卸载 iframe 时可能出现 blob URL 清理信息；某些浏览器也不允许重新定义 `top` 或
`parent`。这些信息不能被简单当作安全结论：如果 Tapp 功能异常，应先确认 CSP 违规的
具体指令、Bridge 返回的错误码，以及目标运行模式是否注册了 handler。详见
[故障排除](TROUBLESHOOTING.md)。
