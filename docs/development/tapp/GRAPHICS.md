# Tapp 图形与轻量游戏能力

本文描述 **第一版** 轻量交互 / 游戏相关能力：Canvas、WebGL、包内资源、音频与
生命周期。整体沙箱边界见 [安全沙箱](SANDBOX.md)，API 细节见
[API 参考](API_REFERENCE.md)。

## 第一版承诺

| 支持 | 不支持（明确不做） |
| ---- | ------------------ |
| Page 内 Canvas 2D / WebGL / WebGL2 | Widget 上跑重 3D 场景 |
| 包内 `assets/` 静态资源 → blob URL | 任意外网贴图 / CDN 引擎脚本 |
| `media:audio` 时 `media-src blob: data:` | 任意 `https:` 音视频流 |
| `'wasm-unsafe-eval'`（CSP） | Worker / SharedArrayBuffer / 多线程 WASM |
| 全屏、`allow-pointer-lock`、pause/resume | 默认放开 `allow-same-origin` |

开发者可直接使用浏览器原生 API（`canvas.getContext('2d'|'webgl2')`、Web Audio、
`requestAnimationFrame`）。宿主 **不** 提供自研 WebGL 引擎封装，也 **不** 内置
Three.js。需要 Three / 自研引擎时，把库放进包内 `page/`（IIFE）并 require，当作普通
guest 依赖。

## 包内资源 `manifest.assets`

在 Manifest 中声明相对安装根的路径，且必须位于 `assets/` 下：

```json
{
  "assets": [
    "assets/sprite.png",
    "assets/beep.wav",
    "assets/level.json"
  ]
}
```

约束：

- 每包最多 128 个资源；单文件 ≤ 16 MiB；合计 ≤ 64 MiB
- 允许二进制；**不允许** `.js` / `.html` 作为 asset 路径
- 直接安装可在请求体中附带 `assets: { "assets/foo.png": "<base64>" }`
- `.tapp` 压缩包只需在 zip 内放入对应文件
- **不要** 把关卡贴图塞进 `Tapp.storage`（那是用户数据，有 8 MiB 硬限）

运行时：

```javascript
const list = await Tapp.assets.list();
const { url, mimeType, size } = await Tapp.assets.getUrl("assets/sprite.png");
const img = new Image();
img.src = url; // 仅 data: / blob: 可通过沙箱图片策略

const { buffer } = await Tapp.assets.getArrayBuffer("assets/level.json");
// iframe 销毁时自动 revoke；也可手动：
Tapp.assets.revoke(url);
Tapp.assets.revokeAll();
```

`blob:` URL **必须在沙箱内创建**（无 `allow-same-origin` 时不能与父页面共享 blob）。
宿主只返回 base64，SDK 在 iframe 内 `createObjectURL`。

## 音频

- 权限：`media:audio`（basic）
- 授予后 CSP 为 `media-src blob: data:`，可用 `new Audio(blobUrl)` 播放包内音频
- **Web Audio API**（`AudioContext` + 振荡器）不依赖 `media-src`，可用于程序化音效
- 切后台时请在 `Tapp.lifecycle.onPause` 中暂停；`onResume` 恢复

## WASM

CSP `script-src` 包含 `'wasm-unsafe-eval'`，可用包内 `.wasm` 经
`Tapp.assets.getArrayBuffer` 后 `WebAssembly.instantiate`。
仍禁止 Worker 与任意网络 `importScripts`。

## 生命周期与性能

宿主在页面不可见时发送 `lifecycle:pause` / `lifecycle:resume`。游戏循环应：

```javascript
let paused = false;
Tapp.lifecycle.onPause(() => { paused = true; /* stop audio */ });
Tapp.lifecycle.onResume(() => { paused = false; });
Tapp.lifecycle.onDestroy(() => {
  cancelAnimationFrame(raf);
  Tapp.assets.revokeAll();
});

function frame(ts) {
  if (!paused) update(ts);
  draw();
  requestAnimationFrame(frame);
}
```

建议：

- 重渲染只放在 **Page**（或全屏窗口），不要把主循环塞进多个 Widget
- 失焦后停止 rAF 累加逻辑与音频，避免后台空转
- 指针锁定依赖用户手势；全屏使用 `Tapp.ui.fullscreen.*` 并申请 `ui:fullscreen`

## Three.js / WebGL 库

Three.js 可以由宿主注入，也可以仍是 Page 里的 guest 依赖。不要写 `Tapp.three`，不要从
CDN / `unpkg` / `jsdelivr` / `esm.sh` 加载，也不要把 `three` 打进 Myriad 主包。

推荐（游戏 / developer 分类）：

```json
{
  "category": "game",
  "runtimeModules": ["three"],
  "assets": ["assets/check.png", "assets/cube.glb"]
}
```

宿主会把钉死的 Three r170 + `GLTFLoader` 当作带 nonce 的脚本注入沙箱（全局 `THREE` /
`GLTFLoader`）。未声明 `runtimeModules` 的 Tapp 行为不变，CSP 也不变。

仓库里的权威文件是 `frontend/public/tapp-runtime/three.0.170.iife.js`（SHA-256
`0ca6ee7e41840a8b95d416f7f38b204126838f277f248a1761acdb7662f2b60d`）。
`frontend/scripts/bundle-tapp-three.mjs` 只是可选重建脚本，**不要**在没有把
`three@0.170` / `esbuild` 装进 frontend 的情况下当构建步骤跑。

也可以继续自己打 IIFE 放进 `page/`，用相对路径 require 进来。沙箱里 core 入口先执行，
模块按 CommonJS 子集语义加载（不是 ES module）：

1. 用 esbuild / Rollup 把 `three`（以及需要的 addons）打成 **IIFE**，输出到 `page/`。
2. 在页面入口里 `require('./scene.js')`——层内文件不需要在 Manifest 里声明。
3. `.js` 不能放进 `manifest.assets`；库源码属于页面模块，贴图 / glTF / wasm 才走
   `assets/`。
4. 默认合计 64 MiB / 单文件 16 MiB / 128 项。声明了 `game` 或 `runtimeModules` 的
   game/developer 包放宽到合计 128 MiB / 单文件 32 MiB / 256 项。整包对应普通
   64/128 MiB、游戏 128/256 MiB。

```bash
# 在 Tapp 项目里（three 只做 devDependency）
npx esbuild src/scene.js --bundle --format=iife --minify --outfile=page/scene.js
```

```json
{
  "page": { "entry": "page/index.js", "template": "page.html" },
  "assets": ["assets/check.png", "assets/cube.glb"]
}
```

### 先铺一层 URL 表，再交给 Loader

先 `await Tapp.assets.getUrlMap()`，再把 `Tapp.assets.rewriteUrl` 交给
`THREE.LoadingManager.setURLModifier`。`fetch` 只放行 `blob:` / `data:`（CSP
`connect-src` 同步），所以 `TextureLoader` / `GLTFLoader.load` 可以吃改写后的
blob，**仍然不能** `load('https://…')`。

```javascript
const urls = await Tapp.assets.getUrlMap();
const manager = new THREE.LoadingManager();
manager.setURLModifier(function (url) {
  return Tapp.assets.rewriteUrl(url);
});

const texture = await new THREE.TextureLoader(manager).loadAsync(
  "assets/check.png",
);
const gltf = await new GLTFLoader(manager).loadAsync("assets/cube.glb");
```

`rewriteUrl` 只映射已声明且已缓存的 `assets/…`（以及唯一的文件名）。先调用
`getUrlMap` / `getUrl` / `resolve`，否则改写结果仍是原字符串，Loader 会失败。

分离的 `.gltf` + `.bin` 只要都在 `manifest.assets` 里，改写后可以加载。不要留
指向 CDN 或未声明路径的 `buffers[].uri` / `images[].uri`。单文件 `.glb` 仍然最省事。

没有 `getUrlMap` 的旧宿主可继续用 `getUrl` + `Image`，或
`getArrayBuffer` + `GLTFLoader.parse`。

### Widget 与预览

- **不要**在 Dashboard Widget 里跑 Three / 重 WebGL。Widget 要快 ready，主循环只放
  Page（或全屏窗口）。
- 商店静态预览 iframe **不执行脚本**。预览用 CSS / 静态图，不要指望里面跑场景。
- Playground 可以生成「canvas + `Tapp.assets`」骨架，但不要输出 CDN `<script>`，
  也不要假设沙箱能 `import` npm 包。

## 官方示例

内置示例仅 **helloWorld**（`com.myriad.hello-world`）。Three.js 官方 Page 是商店里的
**Three 实验室**（`com.myriad.three-lab`）：`getUrlMap` + `rewriteUrl` 接 Loader、
pause 停 rAF、destroy 时 `dispose`。社交客户端 **Aro**、联机 **斗地主** 等完整应用同样发布在官方
[tapp-store](https://github.com/Myriad-You/tapp-store)
（`apps/com.myriad.aro`、`apps/com.myriad.doudizhu`、`apps/com.myriad.three-lab`），
经商店索引安装。目录协议、`manifest.assets` 在商店中的路径拼接与安装回退见
[Tapp 商店](STORE.md)。

## 后续（非第一版）

blob Worker、更大资源预算、WebGPU opt-in 等单独设计，不改变默认沙箱哲学。
