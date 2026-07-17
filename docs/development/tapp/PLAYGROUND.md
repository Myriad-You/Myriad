# Tapp Playground

Tapp Playground 是 Myriad 内置的临时 Tapp 开发环境。它把 Pro AI、当前 Tapp
开发契约、生产沙箱和正式安装链串成一个明确的工作流：

1. 管理员用自然语言描述一个 Tapp。
2. Pro 模型先规划所需的 Tapp 能力和文档检索查询。
3. 后端从仓库拥有的完整 Tapp 文档集中返回有限、可审计的权威章节。
4. 模型生成完整结构化项目，验证失败会携带工具错误继续修复，最多三轮。
5. 前端在会话级工作区中保存 revision checkpoint 和 Agent 工具轨迹。
6. 页面使用正式 Tapp 的 iframe、CSP、SDK 和消息 Bridge 运行，但不申请 Runtime Grant。
7. 沙箱运行错误会回传给 Agent 自动修复，连续自动修复最多两轮。
8. 用户明确点击安装后，项目才进入现有的权限审批、资源 staging 和安装流程。

## 为什么不是直接运行 Grok Build

2026-07-15 开源的 Grok Build 是 coding-agent harness，而不是新的本地模型权重。
Playground 借鉴它的几个架构原则：上下文与工具边界分离、模型先规划检索、每轮修改
创建 checkpoint、模型输出必须经过工具或验证器反馈、运行错误进入下一轮、工作区和
正式发布是两个不同状态。

Myriad 不在后端启动 Grok Build CLI，也不向生成模型开放 shell、Git、主机文件系统或
任意网络访问。模型供应商由 Myriad 的 Pro AI 配置决定，可以是 Gemini、OpenAI
兼容端点或管理员配置的其他兼容服务；Playground 不绑定 xAI。

## 当前工作区契约

工作区始终包含一个可立即预览的 Page，并可按用户需求附带完整 Tapp 资源：

- `manifest.json`
- `main.js` 的 `core` 与 `page` 段
- `page.html`
- `styles.css`
- `i18n` 的 `zh-CN`、`en-US`、`ja-JP` 资源
- Widget 代码和模板
- Page modules 与 package assets
- background requirements、declared APIs、AI、events、agent 与 data exchange Manifest

每次生成或修改都返回完整项目，而不是直接应用未经验证的文本 diff。前端最多保留
20 个 revision，支持前进和后退；状态写入 `sessionStorage`，关闭标签页后自然失效。

## 临时预览与正式运行的区别

| 能力 | 临时预览 | 安装后运行 |
| --- | --- | --- |
| iframe / CSP / SDK | 与正式沙箱相同 | 正式沙箱 |
| Runtime Grant | 不签发 | 按可见安装与当前角色签发 |
| `Tapp.storage` | 当前标签页内存 | 当前用户私有存储 |
| 主题、语言、确认、全屏 | 可用 | 按 Manifest 权限可用 |
| 平台、网络、AI、媒体、事件 | 禁用并返回明确错误 | 按 Manifest、角色和后端策略授权 |
| Widget 注册 | 不支持 | 仅管理员和获批安装可用 |
| 对访客可见 | 否 | 仅管理员公开安装可见 |

这条边界保证“能预览”不会等价于“已经安装”，也不会绕过 v0.2.3 的 Tapp
权限、存储命名空间或访客运行语义。

## 实时进度与取消

- 兼容路径：`POST /api/tapp-playground/generate` 仍返回完整 JSON（含最终
  `agentTrace`）。
- 流式路径：`POST /api/tapp-playground/generate-stream`（同样仅管理员）以 SSE
  推送真实 Agent 步骤：
  - `{ "type": "step", "tool", "status", "summary" }` — 规划 / 检索 / 生成 /
    校验 / 修复等步骤一完成即推送
  - `{ "type": "done", "response": <GeneratePlaygroundResponse> }` — 最终结果
  - `{ "type": "error", "message" }` — 失败或取消
- 前端优先使用 stream，不可用时回退 one-shot；Composer 忙碌态第二行展示最新
  `summary`，并保留耗时计时。
- 客户端 Abort / 断开连接会取消 in-flight 模型 HTTP（reqwest future drop +
  不再启动下一轮 attempt），并发信号量 permit 随任务结束 Drop 释放。整体超时
  与 one-shot 一致（约 20 分钟；单次模型调用最长 720s）。

## 生成验证

## 知识检索

Agent 不会声称把所有文档永久放进模型上下文。每轮先从以下仓库文档规划并检索相关
章节：`TAPP_DEVELOPMENT`、`QUICKSTART`、`ARCHITECTURE`、`MANIFEST`、
`API_REFERENCE`、`PAGE`、`WIDGET`、`SANDBOX`、`STYLING`、`GRAPHICS`、
`REST_API`、`RUNTIME_CONTRACT_DESIGN`、`TROUBLESHOOTING` 和
`TAPP_FILE_FORMAT`。检索结果总量受限，并随响应返回来源名称和章节，便于人工审计。

后端对模型输出执行多层校验：

1. 严格 JSON 结构、字段和总大小限制。
2. 复用正式 `TappManifest` 校验。
3. 检查 Manifest 与 Widget、Page module、asset 资源是否一致。
4. 检查 SDK 调用与权限声明、HTML 模板和禁用的直接网络/宿主访问。

输出失败时，错误会作为受限工具结果交给同一个 Pro 模型诊断并修复；三次仍失败就
终止，不把无效项目交给浏览器。HTML 中的脚本、inline handler、直接 fetch、XHR、
WebSocket、`eval`、宿主窗口与浏览器持久存储访问会被拒绝，而正式 CSP 仍作为运行时
的第二道防线。

## 后续阶段

- Widget 尺寸预览矩阵；当前 Agent 可以生成和安装 Widget 资源，但安全预览只运行 Page。
- 受控的文件级 patch 工具，patch 后仍生成完整 checkpoint 并重新验证。
- 使用语义 embedding 取代当前确定性章节评分，并继续保留可审计来源。
- 独立 Playground AI 用量账本、并发限制与管理员可配置的开放范围。
- 导出 `.tapp`、Manifest 权限差异审阅和安装前测试报告。
- 可选的服务器端短期会话，用于跨设备恢复；仍与 `tapps` 安装表隔离。
