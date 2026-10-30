# AGENTS.md

本文件面向在本仓库工作的 AI 代理。人类读者请从 `README.md` 开始。

本仓库是 Myriad 的 2.5D 形象：演她的 rig 的运行时，和做 rig 的处理工具。站点（[Myriad-You/Myriad](https://github.com/Myriad-You/Myriad)）以子模块 `figure/` 钉住本仓库，作为 `@myriad/figure` 引用；macOS 原生 App 和 Android XR 端按钉住的提交照本仓库移植。改动只有经过一次钉住才会到达站点。

## 事实纪律

- 结论落到 `文件:行号`，或文件加符号名。没有位置的断言等于没有断言。
- 外部文档里的符号名不可信，引用前在本仓库里核实。
- 无法验证的判断标注 `uncertain`。浏览器和 GPU 驱动的行为不在本仓库控制内，说清楚。

## 硬约束

违反下列任一条的改动会被要求返工。

- **不引用宿主页面。** `src/` 里不 import 站点的任何模块（站点的 i18n、config、services、组件、上下文、全局 store）。形象需要宿主给的东西（文案、API 地址、一轮对话的记录）加进 `src/figureHost.ts` 的 `FigureHost`，并给一个单独运行也成立的默认值。
- **契约只在核心。** rig 契约和表演契约只经 `@myriad/core/shared/...` 读，不复制进本仓库。改契约在 myriad-core 里改，说明里写清影响哪几端。
- **形象不碰密钥。** 本仓库的代码跑在浏览器里，拿不到也不该拿任何宿主密钥或模型凭据；需要模型或第三方服务的步骤（拆层、生图、关键形拟合）由站点后端做，这里只收结果。
- **行为不变的重构要有证据。** 改播放器或它每帧调用的代码却说「行为不变」时，用确定性的黄金帧哈希对照前后（固定 `Math.random`、`performance.now`，按帧哈希每层的变形、变换、透明度和 `readPixels`），不用截图。
- **测试读核心文件经 `src/testing/coreFile.test-support.ts`。** 单独检出时它读本仓库的 `core/`，在 Myriad 里读站点的 `core/`。

## 输出去向

- 提交按 Conventional Commits，一个可评审单元一次提交。
- 每次改动说明站点换钉住的提交时要做什么（新的宿主字段、入口改名），以及原生和 XR 端移植时要注意的地方。
- 草稿、临时规格、代理笔记不进主干。
