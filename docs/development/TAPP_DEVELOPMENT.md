# Tapp 开发文档

> 本文档已拆分为模块化结构，请根据需要查阅对应文档。  
> 字段与端点以当前代码路径为准，不以历史更新日志为契约。

## 模块文档

| 文档 | 说明 |
| ---- | ---- |
| [架构总览](tapp/ARCHITECTURE.md) | 安装态、运行态、列表布局、私有安装清理、沙箱与调度器 |
| [Tapp 商店](tapp/STORE.md) | 远程目录 `index.json`、源管理、安装链路、商店索引 UI 字段（如 `icon_shell`）与发布 |
| [Tapp Playground](tapp/PLAYGROUND.md) | Pro AI 双模式（Page / Widget-only）生成、预览、导出与安装边界 |
| [Playground 生成上下文](tapp/PLAYGROUND_GENERATION_CONTEXT.md) | 注入模型的开发上下文与能力边界 |
| [快速入门](tapp/QUICKSTART.md) | CLI 创建/校验/打包，代码架构，生命周期 |
| [Manifest 配置](tapp/MANIFEST.md) | 完整的可安装 `manifest.json` 配置参考（含 `analytics:read` 等权限） |
| [SDK API 参考](tapp/API_REFERENCE.md) | 所有 Tapp SDK API（含 `Tapp.analytics`、`Tapp.persona`） |
| [小组件开发](tapp/WIDGET.md) | Widget 开发指南、尺寸适配、样式规范 |
| [页面样式规范](tapp/PAGE.md) | Page 布局、深色模式与 i18n |
| [安全沙箱](tapp/SANDBOX.md) | CSP 策略、iframe 限制、权限系统 |
| [图形与轻量游戏](tapp/GRAPHICS.md) | Canvas/WebGL、assets、音频、pause 约定 |
| [样式规范](tapp/STYLING.md) | CSS 变量、Tailwind 集成、Glass 风格 |
| [设计规范摘要](tapp/DESIGN_SPEC.md) | 注入 Playground agent 的设计语言摘要 |
| [运行时契约](tapp/RUNTIME_CONTRACT_DESIGN.md) | Runtime Grant、Data Exchange、AI Task、Event、Agent |
| [REST API](tapp/REST_API.md) | 宿主路由：安装、列表布局、清理策略、analytics Grant 等 |
| [故障排除](tapp/TROUBLESHOOTING.md) | 常见问题、调试技巧、发布检查清单 |
| [`.tapp` 文件格式](../features/TAPP_FILE_FORMAT.md) | ZIP 安装包布局与安装入口 |
| [权限 fixtures](tapp/fixtures/README.md) | host 路由 / action 权限对照（先改 fixture） |

官方远程目录仓库：[Myriad-You/tapp-store](https://github.com/Myriad-You/tapp-store)（目录与发布说明以 [Tapp 商店](tapp/STORE.md) 为准）。

新手从 [快速入门](tapp/QUICKSTART.md) 开始；Widget 看 [WIDGET.md](tapp/WIDGET.md)；契约与宿主边界看 [架构](tapp/ARCHITECTURE.md) 和 [运行时契约](tapp/RUNTIME_CONTRACT_DESIGN.md)。CLI 见 `tools/tapp-cli/README.md`。

## 文档维护原则

- 架构结论以当前运行路径为准，不把旧更新日志当作契约。
- Manifest 字段以安装后的 round-trip 结果为准。
- SDK 方法必须同时存在权限映射和目标沙箱 handler。
- REST 端点以后端路由注册为准；Tapp 代码优先使用 SDK 而不是直接调用 REST。
- 商店目录字段以官方 `index.json` + 安装器实际读取的键为准；见 [STORE.md](tapp/STORE.md)。
- 权限字符串与 host 归因以 `tapp/fixtures/*.json` 为 source of truth（先改 fixture 再改代码）。
