# Myriad figure

Myriad 的 2.5D 形象：在浏览器里演她的 rig 的运行时，和从分层的图做出一套 rig 的处理工具。

- **运行时**：`anime25drig/`（网格变形、头壳转头与关键形、头发和胸的物理、表情、嘴形、WebGL 渲染、播放器）、`motion/`（行为排程、表演、说话和唱歌时的身体、触摸）、`expressionShapes/`（特殊表情的形状）、`singing/`（音乐信号、节拍、歌词时间线）、`speech/`（韵律、口型、重音）、`interaction/`（触摸手势和反应）、`events/`（表演事件和表演契约）、`performance/`（表演类型、心情分档）。
- **处理工具**：`rig/`（PSD 与拆层导入、图集、骨骼、部件、补遮挡、转头和转身关键形的拟合与放置），PSD 解码在 Worker 里跑。
- **宿主**：`figureHost.ts`。形象自己不知道语言、API 在哪、怎么记一轮对话；这些由宿主页面 `installFigureHost` 借给它。没装时说英文、按页面地址取、不记录。

谁在用：[Myriad](https://github.com/Myriad-You/Myriad) 的站点以子模块 `figure/` 钉住本仓库，作为 pnpm workspace 包 `@myriad/figure` 引用（`@myriad/figure/anime25drig/player` 这样按文件取）。Myriad 的 macOS 原生 App 和 Android XR 端按钉住的提交照本仓库移植。

## 共用核心

rig 契约和表演契约在共用核心（[Myriad-You/myriad-core](https://github.com/Myriad-You/myriad-core)）的 `shared/` 里，本仓库按 `@myriad/core/shared/...` 引用。单独检出时 `@myriad/core` 指本仓库的子模块 `core/`；在 Myriad 里这个子模块不检出，指 Myriad 自己的 `core/`（`tsconfig.json` 的 `paths` 两处都列了，站点的 vite 和 tsconfig 另配同名别名）。所以在 Myriad 里形象总是跟站点用同一份核心。

## 开发

```sh
git clone --recursive https://github.com/Myriad-You/myriad-figure.git
cd myriad-figure
pnpm install
pnpm typecheck
pnpm test
```

在 Myriad 里，`frontend/` 的 `pnpm install` 会一起装好本仓库的依赖，`pnpm test:unit` 和 `pnpm typecheck` 也会跑本仓库的测试和类型检查。

本仓库 2026-10-31 带着历史从 Myriad 拆出来，之前的提交记录照原样保留（提交说明里的 `#数字` 指 Myriad 的 PR）。

## 许可

AGPL-3.0-only，见 [LICENSE](LICENSE)。`src/anime25drig/vendor/` 是运行时参照的 Anime2.5DRig 的 MIT 许可和说明。
