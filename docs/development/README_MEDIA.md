# README 动图

README 里的动图都是真前端在无头 Chrome 里离线录的：页面、组件、动效是生产构建原样跑出来的，数据是合成的样例站长「星野澄」。脚本在 [`frontend/scripts/readme-media/`](../../frontend/scripts/readme-media/)。

## 哪些是真的

- **真的**：前端生产构建；一个隔离的后端实例（独立数据库、独立数据目录），布局、博客、Hugo 构建、编辑器同步都走它；2.5D 形象的实时渲染和口型。
- **替身**：报告、媒体库、天气、社交卡片等数据接口在浏览器里拦截后返回 `content.mjs` 的样例；Agent 回复和 Playground 生成过程是按 `stream.mjs`、`pomodoro.mjs` 写好的流，不调用模型。没有替身的外部请求一律中止。
- **时间**：页面时钟固定在 2026-10-23 10:20（东京），逐帧推进，随机数固定种子。重录时画面内容一致，但跟着真实时间走的东西（后端返回后才出现的问候、悬停和切换的过渡）可能早晚一两帧，帧不会逐字节相同。

## 准备

1. 一个空数据库，起一个只给录制用的后端，`FRONTEND_URL`/`BASE_URL`/`CORS_ORIGINS` 指向下面的前端地址。博客场景需要后端能找到 Hugo（`HUGO_BIN`、`DART_SASS_DIR`）。
2. 打开实例完成安装，建好站长账号。
3. 前端构建到单独目录，用 `spa-server` 起在 4391：

   ```bash
   pnpm vite build --outDir /tmp/myriad-readme-dist
   ```

   ```bash
   DIST_DIR=/tmp/myriad-readme-dist PORT=4391 node scripts/spa-server.mjs
   ```

4. 一份形象：在实例上用项目自带的风格参考图 [`backend/assets/merope/style-reference.png`](../../backend/assets/merope/style-reference.png) 导入生成，再把 `GET /api/merope/rig/active` 的结果存成 `active.json`，把其中纹理地址改成 `/api/merope/rig/assets/demo-atlas`、`portraitUrl` 改成 `/api/merope/rig/assets/demo-portrait`，纹理图存成同目录的 `atlas.png`。导出的形象不进仓库。

## 环境变量

| 变量 | 说明 |
| --- | --- |
| `README_MEDIA_OWNER` / `README_MEDIA_PASSWORD` | 隔离实例的站长账号 |
| `README_MEDIA_RIG` | 放 `active.json` 和 `atlas.png` 的目录 |
| `README_MEDIA_DATABASE_URL` | 隔离实例的数据库，博客准备脚本用来改站长显示名（没有站长接口能改） |
| `README_MEDIA_APP` | 前端地址，默认 `http://localhost:4391` |
| `README_MEDIA_BACKEND` | 后端地址，默认 `http://127.0.0.1:1199` |
| `README_MEDIA_WORK` | 帧和日志目录，默认系统临时目录下的 `myriad-readme-media` |

需要本机装有 Chrome（Playwright 用 `channel: 'chrome'`）和 `psql`。

## 录制

全部场景、三种语言、亮暗两套，录完直接编码进 `docs/assets/readme/`：

```bash
./record.sh
```

只录一部分：`SCENES="chat face" LANGS=zh ./record.sh`。单个场景：`node scenes/<场景>.mjs <zh|ja|en> <light|dark>`，再 `node encode.mjs <输出目录> <场景>`。

| 场景 | 内容 |
| --- | --- |
| `home` | 首页铺开，hero 另出一张减少动态用的静帧 |
| `chat` | 长按空白处打开面板、切到聊天、提问、看回复和口型 |
| `face` | 形象工作台「动作调整」页依次点表情 |
| `report` | 播放 Steam 报告卡的舞台 |
| `library` | 媒体库画布 |
| `tapp` | Playground 描述需求、生成、检查、在预览里使用 |
| `blog` | 工作室编辑器写一篇、发布、静态博客里出现 |

博客场景会**删除实例上全部笔记**再按语言重建样例，所以只能在隔离实例上跑，且各语言依次执行。博客主题有自己的亮暗开关、不跟系统，只出亮色。

## 两处录制专用的绕法

- Playground 预览是沙箱 iframe，会替换 `window.eval`，而假时钟每帧都要用它；录制时加 `--disable-features=IsolateSandboxedIframes` 让初始化脚本进到沙箱里，并把 `eval` 固定成原生的。不影响页面行为，只为让时钟走得动。
- 编辑器同步走 WebSocket，录制时把发往前端地址的连接改到后端。
