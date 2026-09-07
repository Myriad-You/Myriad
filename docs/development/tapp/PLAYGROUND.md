# Tapp Playground

Tapp Playground 是 Myriad 内置的临时 Tapp 开发环境。它把 Pro AI、当前 Tapp
开发契约、生产沙箱预览和正式安装 / 导出链串成一个明确的工作流：

1. **管理员（桌面端）**用自然语言描述一个 Tapp（完整 Page、Widget-only，或两者兼有）。
2. Pro 模型先规划所需的 Tapp 能力和文档检索查询。
3. 后端从仓库拥有的完整 Tapp 文档集中返回有限、可审计的权威章节。
4. 模型生成完整结构化项目；校验失败会携带错误继续修复，最多三轮。
5. 前端在 **localStorage 多会话** 工作区中保存 revision checkpoint、Agent 轨迹与修改脉络。
6. 有可用 Page 时用正式 Page iframe / CSP / SDK 预览；有 Widget 时用交互式 Widget 沙箱预览。
7. **仅 Page 沙箱**运行错误会回传 Agent 自动修复（最多两轮）；Widget-only 不挂载 Page 沙箱、不触发 Page 自动修复。
8. 导出 `.tapp` 或安装前走客户端 preflight；安装成功后 `syncFromBackend` + `startTapp`。

相关契约：[`MANIFEST.md`](./MANIFEST.md)、[`WIDGET.md`](./WIDGET.md)、
[`SANDBOX.md`](./SANDBOX.md)、[`STORE.md`](./STORE.md)、
[`API_REFERENCE.md`](./API_REFERENCE.md)、
[`PLAYGROUND_GENERATION_CONTEXT.md`](./PLAYGROUND_GENERATION_CONTEXT.md)。

注入模型的文档目录由 `backend/src/services/tapp_playground_knowledge.rs` 的
`include_str!` 列表决定（含 STORE、MANIFEST、API_REFERENCE 等）。`DESIGN_SPEC.md`
与 `PLAYGROUND_GENERATION_CONTEXT.md` 无条件注入生成 prompt。重新编译 backend
后才会带上文档改动。

## 为什么不是直接运行 Grok Build

2026-07-15 开源的 Grok Build 是 coding-agent harness，而不是新的本地模型权重。
Playground 借鉴它的几个架构原则：上下文与工具边界分离、模型先规划检索、每轮修改
创建 checkpoint、模型输出必须经过工具或验证器反馈、运行错误进入下一轮、工作区和
正式发布是两个不同状态。

Myriad 不在后端启动 Grok Build CLI，也不向生成模型开放 shell、Git、主机文件系统或
任意网络访问。模型供应商由 Myriad 的 Pro AI 配置决定，可以是 Gemini、OpenAI
兼容端点或管理员配置的其他兼容服务；Playground 不绑定 xAI。

## 访问控制

| 层 | 行为 |
| --- | --- |
| API | `POST /api/tapp-playground/*` 挂载 `admin_middleware`，仅管理员 |
| 路由 | `/tapp/playground` 包在 `RequireAuth requiresAdmin` |
| 列表入口 | 桌面端管理员可见 Playground 按钮；**移动端列表不展示** |
| 直达 URL | 移动端访问 `/tapp/playground` **重定向到 `/tapp`** |

Playground 是桌面管理员工具，不是访客或普通用户功能。

## 工作区模式（Page / Widget-only）

项目至少需要 **Page** 或 **Widgets** 之一；两者都没有会在生成校验与
`validatePlaygroundPackage` preflight 中被拒绝。

| 模式 | 判定 | 资源期望 |
| --- | --- | --- |
| **Page 模式** | 声明了 `manifest.page` | 非空 `code.page` + `code.pageHtml`；`page.template: "page.html"`；可附带 widgets |
| **Widget-only** | 未声明 `page` 且 `manifest.widgets` 非空 | 非空 `code.widget` + `code.widgetHtml`；**不强制**生成 stub Page / `page.html`；声明权限含 `widget:register`（安装校验；不等于授予动态注册） |
| **Page + Widget** | 有可用 Page 且有 widgets | 预览拆成独立 Page / Widget 窗格 |

前端判定「可用 Page」：声明了 `page` 层且 `pageHtml` 非空。仅有 Widget 时
左列整列给 Widget 预览，并展示「无页面 — 仅小组件」说明，**不挂载**
`TappPageSandbox`。

Playground 打包到固定三文件布局：`core.js`、`page/index.js`、`widget/index.js`。一层一个
文件；再拆文件只在导出之后。Widget 入口必须赋值 `Tapp.widgets[<id>] = { render }`，
不要写 `Tapp.widget.register`。交接：导出 `.tapp` → 解压 → `myriad-tapp check`。完整项目
仍可包含：`manifest.json`、三个层入口、`styles.css`、`i18n`、package assets、
background / APIs / AI / events / agent / dataExchange 等 Manifest 扩展（见
[MANIFEST](./MANIFEST.md)、[WIDGET](./WIDGET.md)、[CLI](../../../tools/tapp-cli/README.md)）。
Playground 生成/打包始终落 `core.js` 与 `styles.css`。安装契约本身：没有
`backgroundRequirements` 时不强制 core；声明**常驻**才必须有 core。

### 目录文案：`manifest.locales` ≠ 应用内 `code.i18n`

生成面向商店/列表的包时，优先为 **宿主 chrome 标题与短描述** 填写
`manifest.locales`（BCP-47 → `{ name?, description? }`），并保留顶层
`name` / `description` 作为语言未命中时的兜底。商店详情长介绍与静态预览的多语言
属于 `catalog.json` 的 `locales`（`long_description` / `preview`），不要写进
Manifest。这与 **应用内 UI** 的 `code.i18n` / `Tapp.i18n` 也不是同一套机制。详见
[MANIFEST · 多语言名称与描述](./MANIFEST.md#多语言名称与描述locales)、
[STORE](./STORE.md) 与
[PLAYGROUND_GENERATION_CONTEXT](./PLAYGROUND_GENERATION_CONTEXT.md)。

每次生成或修改都返回**完整项目 JSON**，而不是未经验证的文本 diff。

## 会话、历史与修改脉络

| 项 | 实现 |
| --- | --- |
| 存储 | 当前多会话 `localStorage` 记录 |
| 迁移 | 首次从旧版 `sessionStorage` 单会话记录迁入后使用多会话 |
| 会话上限 | 约 **10** 个（按 `updatedAt` 淘汰；始终保留当前会话） |
| Revision 上限 | 每会话约 **20** 个 |
| 体积预算 | 约 **4.5MB** JSON；超出时裁剪并给用户 **prune notice** |
| UI | History 面板：`revisions` / `sessions` / **修改脉络**（memory / edit trail） |
| 来源标记 | `user`（用户生成）、`runtime-repair`（Page 自动修复）、`manual`（手改代码） |
| 手改合并 | 连续 manual 编辑在约 4s 内合并为同一 revision，避免刷屏 |

### 多轮记忆（发给 Pro 的 wire 格式）

前端把当前会话到当前 checkpoint 的 revision（及可选失败尾）编成 `history[]`。
后端 **adaptive** 处理：

- 最近 **2** 个成功 turn：附带**完整** project JSON
- 更早的成功 turn：仅 **compact summary**（manifest / 文件体量 / widgets / 权限等，无源码正文）
- 当前用户消息上的 `currentProject` 始终完整，作为代码真值
- 失败 attempt 可作为 tail，提示勿重复同一错误
- 请求侧最多约 20 个 history turn

前端 localStorage 仍可保留完整 revision 快照；自适应只作用于模型上下文体积。

## 生成 API

| 路径 | 形态 |
| --- | --- |
| `POST /api/tapp-playground/generate` | 一次性 JSON（含最终 `agentTrace`） |
| `POST /api/tapp-playground/generate-stream` | SSE：`step` / `done` / `error` |

两端均需管理员。

### 流式事件

- `{ "type": "step", "tool", "status", "summary" }` — 规划 / 检索 / 生成 / 校验 / 修复等
- `{ "type": "done", "response": <GeneratePlaygroundResponse> }` — 最终结果
- `{ "type": "error", "message" }` — 失败或取消

### 前端行为

- **优先** stream，不可用时回退 one-shot。
- Composer 忙碌态展示最新 step `summary` 与耗时。
- **取消**：`AbortController`；取消为**软提示**（状态带失败 / 可重试），不损坏已有 revision。
- 客户端总超时与 dev proxy：约 **30 分钟**（`30 * 60 * 1000` ms）。
- 单次模型 HTTP：后端 **1080s**。
- 并发 Agent 运行：全进程信号量 **2**。

### 校验与规范化

1. 严格 JSON 结构、字段与体积限制。
2. 复用正式 `TappManifest` 校验。
3. Manifest 与 Widget / Page module / asset 一致性。
4. SDK 调用与权限、HTML 模板、禁用的网络 / 宿主访问。
5. 输出失败时同一 Pro 模型诊断修复，最多 **3** 次 attempt。
6. **Asset path normalize**：从 `manifest.assets` / `code.assets` 剔除非法路径
   （非 `assets/` 前缀，或 `.html` / `.js` / `.css` 等入口类路径）；模板应落在
   `code.widgetHtml` / widget `templates`，不要塞进 package assets。
7. 设置字段别名规范化（如 `default` → `defaultValue`）。

HTML 中的脚本、inline handler、直接 fetch / XHR / WebSocket、`eval`、宿主窗口与
浏览器持久存储访问会被拒绝；正式 CSP 仍是运行时第二道防线。

## 知识检索

Agent 不会声称把所有文档永久放进模型上下文。

无条件注入（不依赖检索命中）：

- `DESIGN_SPEC.md` — 设计语言
- `PLAYGROUND_GENERATION_CONTEXT.md` — 预览边界与调用契约（含 `Tapp.ai.tasks`
  的 `generate` / `analyze` / `chat` / `image` / `search`、`manifest.ai`、
  `create` 返回 queued 后须 `get`/`subscribe`，以及 `{ format, value,
  contextProvenance }` 信封）

每轮再从仓库文档规划并检索相关章节，例如：`TAPP_DEVELOPMENT`、`QUICKSTART`、
`ARCHITECTURE`、`MANIFEST`、`API_REFERENCE`、`WIDGET`、`SANDBOX`、
`STYLING`、`GRAPHICS`、`REST_API`、`RUNTIME_CONTRACT_DESIGN`、`TROUBLESHOOTING`、
`STORE`、`TAPP_FILE_FORMAT`。`PLAYGROUND_GENERATION_CONTEXT` 仍出现在检索目录里
给规划器点名，但生成时跳过重复摘录。`PAGE.md` 是宿主 React 页面 chrome（`AnimatedView`、
`sm:`/`md:`），不是沙箱契约，生成检索会跳过；沙箱样式以 `DESIGN_SPEC` / `STYLING` 为准。
检索结果总量受限，并随响应返回来源名称和章节，便于人工审计。

权威文档由后端 `tapp_playground_knowledge.rs` 以 `include_str!` 编译进二进制；
改 markdown 后需重新编译 backend 才会进入注入文本与检索目录。

## 临时预览与正式运行

| 能力 | 临时预览 | 安装后运行 |
| --- | --- | --- |
| Page iframe / CSP / SDK | 有可用 Page 时与正式沙箱同构 | 正式沙箱 |
| Widget 沙箱 | 有 Widget 时挂载（可交互） | 主页 / 运行态按宿主策略 |
| Runtime Grant | **不签发** | 按可见安装与当前角色签发 |
| `Tapp.storage` / `settings` / `shared` | 当前**标签页内存**；预览需声明 `storage:read` / `storage:write`；三种 KV 都有 `onChanged`（settings 不拆卡） | 正式命名空间 |
| 主题、语言、`openUrl` | Page / Widget 预览可用（通知禁用；`openUrl` 需声明 `ui:openUrl` + `openUrls`） | 按授予权限 |
| 标题、确认、全屏 | **仅 Page 预览**（Widget SDK 没有这些方法） | 按授予权限；仍仅 Page |
| `Tapp.assets` | 工作区 `code.assets` 内存；与安装后同一 `list` / `get` / `getUrlMap` | 安装包资源 |
| Tailwind 工具类 | 预览按 HTML/JS 按需编译，与安装后同一套宿主编译 | 安装时写入 page/widget CSS |
| `Tapp.context` / `Tapp.user` / `file.download` | 预览桩 / 与正式相同的宿主下载（public） | 正式上下文与下载 |
| `Tapp.persona.get` | 固定样例名片，不打真实 API | 站点公开名片 |
| 平台、网络、AI、宿主媒体、事件 Broker | 禁用；未注册的宿主 API 返回 `PREVIEW_UNAVAILABLE`（不是安装后的结果） | 按声明权限、角色与授予权限 |
| **Federation**（Feed、Note/媒体、Channel/Room/Ring 等） | **不可用**（无 Grant、无 FederationBridge） | 按 `federation:*` + Runtime Grant |
| 对访客可见 | 否 | 仅管理员公开安装可见 |
| Page 运行时自动修复 | 有可用 Page 时最多 **2** 轮；**预览预期失败**（AI / 联邦 / 未授予权限等）不触发修复 | 不适用 |
| Widget-only | **不**挂载 Page 沙箱；**无** Page 自动修复；Widget 错误仅状态展示 | 安装后按正式 Widget 路径 |

这条边界保证「能预览」不等于「已安装」，也不会绕过 Tapp 权限、存储命名空间或
访客运行语义。

## 导出与安装

| 步骤 | 行为 |
| --- | --- |
| 包文件映射 | `buildPlaygroundPackageFiles`：**导出 ZIP 与 install-from-code 共用**同一路径布局 |
| Preflight | `validatePlaygroundPackage`：导出前与安装前均执行；失败则阻断并展示错误列表 |
| 导出 | 客户端 JSZip 下载 `{manifest.id}.tapp`，无需后端写库 |
| 安装 | 宿主 **direct** 安装（内联 manifest/code/资源 → `POST /api/tapps/install` `source:direct`），**不是**远程商店 `source:store` 路径 |
| 安装后 | `getTappRuntime().syncFromBackend(true)` + `startTapp(id)`（启用） |
| 部分失败 | 安装本身已成功但 sync/start 失败时：提示仍可安装成功，并给出 start 失败文案；用户可在详情页手动启用 |

Playground 自己的「安装到本机」与生成代码里调用的 `Tapp.tappList.install` 是两回事：

- **Playground UI 安装**：当前工作区包 → direct install（见上表）。
- **生成代码若写商店安装**：必须用 SDK 合法形状
  `{ source: "store", storeSource: "<源 id 或 index.json URL>", tappId }`，或
  `{ source: "https://…/index.json", tappId }`；**不要**写裸 `{ source: "1", tappId }`。
  完整协议见 [STORE.md](./STORE.md) 与 [API_REFERENCE · Tapp 列表](./API_REFERENCE.md#tapp-列表-api)。

## 后续阶段

以下能力**尚未**实现（或仅部分存在时仍算未完成）：

- 使用语义 embedding 取代当前确定性章节评分，并继续保留可审计来源。
- 独立 Playground AI **用量账本**，以及管理员可配置的开放范围（并发上限 2 已存在，但不是可配置账本）。
- 可选的**服务器端**短期会话，用于跨设备恢复；仍与 `tapps` 安装表隔离。
- 受控的**文件级 patch** 工具（patch 后仍生成完整 checkpoint 并重新验证）。

### 已并入正文的能力（勿再写成「未做」）

- Widget 交互预览与尺寸切换；Widget-only 无强制 Page。
- 客户端导出 `.tapp` 与 install/export 共用 package file map。
- 安装 / 导出 preflight（`validatePlaygroundPackage`）。
- 流式真实 Agent 步骤 + Abort 取消。
- multi-session localStorage、修改脉络、自适应多轮记忆、manual revision。
- 安装后 sync + start。
- 非法 asset 路径规范化。
