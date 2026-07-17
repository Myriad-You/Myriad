# Tapp 开发文档

> 📚 本文档已拆分为模块化结构，请根据需要查阅对应文档。

## 📖 模块文档

| 文档                                  | 说明                                      |
| ------------------------------------- | ----------------------------------------- |
| [架构总览](tapp/ARCHITECTURE.md)      | 安装态、运行态、沙箱、后台 core 与调度器  |
| [Tapp Playground](tapp/PLAYGROUND.md) | Pro AI 双模式（Page / Widget-only）生成、预览、导出与安装边界 |
| [快速入门](tapp/QUICKSTART.md)        | 5 分钟创建第一个 Tapp，代码架构，生命周期 |
| [Manifest 配置](tapp/MANIFEST.md)     | 完整的 manifest.json 配置参考             |
| [SDK API 参考](tapp/API_REFERENCE.md) | 所有 Tapp SDK API 详细文档                |
| [小组件开发](tapp/WIDGET.md)          | Widget 开发指南、尺寸适配、样式规范       |
| [安全沙箱](tapp/SANDBOX.md)           | CSP 策略、iframe 限制、权限系统           |
| [图形与轻量游戏](tapp/GRAPHICS.md)    | Canvas/WebGL、assets、音频、pause 约定    |
| [样式规范](tapp/STYLING.md)           | CSS 变量、Tailwind 集成、Glass 风格       |
| [设计规范摘要](tapp/DESIGN_SPEC.md)   | 注入 Playground agent 的设计语言摘要      |
| [REST API](tapp/REST_API.md)          | 后端 REST API 端点参考                    |
| [故障排除](tapp/TROUBLESHOOTING.md)   | 常见问题、调试技巧、发布检查清单          |

## 🚀 快速导航

### 新手入门

1. 阅读 [快速入门](tapp/QUICKSTART.md) 创建第一个 Tapp
2. 了解 [Manifest 配置](tapp/MANIFEST.md) 完善应用信息
3. 查阅 [SDK API 参考](tapp/API_REFERENCE.md) 使用各种功能

### 开发小组件

1. 查看 [小组件开发](tapp/WIDGET.md) 了解 Widget SDK 限制
2. 参考 [样式规范](tapp/STYLING.md) 实现 Glass 风格设计
3. 了解 [安全沙箱](tapp/SANDBOX.md) 避免 XSS 风险
4. 遇到问题查阅 [故障排除](tapp/TROUBLESHOOTING.md)

### 高级功能

1. [架构总览](tapp/ARCHITECTURE.md) - 先理解宿主、沙箱和后端边界
2. [Manifest 配置 - API 声明](tapp/MANIFEST.md#api-声明-apis) - API 声明与 Spoof 模式
3. [SDK API 参考 - 定时任务](tapp/API_REFERENCE.md#定时任务-api) - 持久化定时任务
4. [REST API](tapp/REST_API.md) - 宿主内部使用的后端契约

## 📁 目录结构

```
docs/development/tapp/
├── ARCHITECTURE.md     # 架构与数据流总览
├── PLAYGROUND.md       # Pro AI 临时开发环境（Page / Widget-only、会话、导出）
├── PLAYGROUND_GENERATION_CONTEXT.md # Playground 的模型开发上下文
├── QUICKSTART.md       # 快速入门
├── MANIFEST.md         # Manifest 配置
├── API_REFERENCE.md    # SDK API 参考
├── WIDGET.md           # 小组件开发
├── SANDBOX.md          # 安全沙箱
├── STYLING.md          # 样式规范
├── DESIGN_SPEC.md      # 设计语言摘要（无条件注入 Playground agent）
├── PAGE.md             # 页面样式规范
├── REST_API.md         # REST API 端点
└── TROUBLESHOOTING.md  # 故障排除
```

## 文档维护原则

- 架构结论以当前运行路径为准，不把旧更新日志当作契约。
- Manifest 字段以安装后的 round-trip 结果为准。
- SDK 方法必须同时存在权限映射和目标沙箱 handler。
- REST 端点以后端路由注册为准，Tapp 代码优先使用 SDK 而不是直接调用 REST。
