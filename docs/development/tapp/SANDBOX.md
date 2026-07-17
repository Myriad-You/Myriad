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
script-src 'nonce-<random>' https://cdn.tailwindcss.com;
default-src 'none';
style-src 'unsafe-inline' https://fonts.googleapis.com;
img-src data: blob: https: http:;
font-src data: https://fonts.gstatic.com;
connect-src 'none';
frame-src 'none';
object-src 'none';
media-src 'none';
worker-src 'none';
form-action 'none';
base-uri 'none';
manifest-src 'none'
```

图片允许 HTTP(S) 是为了头像、封面等展示，不代表脚本能够直接联网。
Manifest 不能覆盖这份 CSP；如果确实需要新的资源能力，应修改并审计宿主策略，而不是让
单个 Tapp 放宽隔离。

## 被禁用的浏览器能力

安全包装器在 Tapp 代码执行前处理这些能力：

| 浏览器能力                                               | 当前行为                 | 使用方式                                      |
| -------------------------------------------------------- | ------------------------ | --------------------------------------------- |
| `fetch`、`XMLHttpRequest`、`WebSocket`、`EventSource`    | 禁用                     | Manifest `apis` + `Tapp.api()`                |
| `localStorage`、`sessionStorage`、`indexedDB`、Cache API | 禁用或替换为空实现       | `Tapp.storage`                                |
| `eval`、带源码的 `Function`、字符串形式的 timer          | 禁用                     | 使用预打包代码和函数回调                      |
| `window.open`、`alert`、`confirm`、`prompt`、`print`     | 禁用                     | `Tapp.ui.showNotification/confirm` 等受控 API |
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

`protected` 默认要求 `network:fetch`；`public` 不要求该权限，但仍受 Manifest、输入模板、
后端出站安全和频率限制约束。端点、请求头和模板参数会在后端解析，HTTP 请求经过 SSRF
防护。系统不存在供 Tapp 使用的任意 URL 代理。

## 权限

Manifest 的 `permissions` 是申请集合，运行时以安装记录中的 `granted_permissions` 为准：

```json
{
  "permissions": ["storage", "ui:theme", "platform:read"]
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
遍历形式。后端会拒绝超过 1 MiB 的单值，并在事务内对同一安装 owner 与 Tapp 串行计算
写入后的总量；超过 5 MiB 会返回 413。数据库触发器执行相同的 5 MiB 硬限制，覆盖其他
内部写入路径；`Tapp.storage.usage()` 返回的 quota 因而也是实际安全边界。

持久 storage 命名空间跟随**安装 owner**（Runtime Grant 的 `ownerId`），不是当前 viewer。
打开站点公开安装时读写的是站点 owner 数据：已授权 viewer 可只读，仅 owner 可写/删/清空
（否则 403）。个人数据需要用户安装自己的私有副本后才会进入该用户命名空间。

## 开发检查清单

- Manifest 只申请实际使用的权限。
- 外部请求全部通过具名 `apis` 和 `Tapp.api()`。
- Tapp 不依赖主页面 DOM、Cookie 或浏览器存储。
- 不把密钥写入 Manifest、Tapp 代码、日志或参数。
- Page、Widget、headless 模式分别验证所需 handler。
- 用户输入和外部响应在进入 DOM 前完成类型、长度和内容校验。
- 新增 SDK action 时同步更新权限映射、宿主 handler、后端校验和文档。

## 已知控制台信息

卸载 iframe 时可能出现 blob URL 清理信息；某些浏览器也不允许重新定义 `top` 或
`parent`。这些信息不能被简单当作安全结论：如果 Tapp 功能异常，应先确认 CSP 违规的
具体指令、Bridge 返回的错误码，以及目标运行模式是否注册了 handler。详见
[故障排除](TROUBLESHOOTING.md)。
