# 主题控制系统（Surface / Glow Theme）

站主可控的**外观主题系统**，统一驱动全站「玻璃质感」组件的表面样式与小组件光晕。
管理员在首页编辑模式的「样式」面板切换，配置存后端、对访客生效。

---

## 1. 两个维度

| 维度 | 取值 | 作用 |
| --- | --- | --- |
| **surface（表面）** | `glass` / `solid` / `flat` / `outline` / `liquid` | 卡片底的质感：玻璃 / 不透明纯色 / 半透明无模糊 / 极淡描边 / 流体玻璃（Liquid Glass 风格：高透底吸附壁纸主色 + 镜面高光边 + 边缘弯光环）。作用于**全站**所有 `.glass` 组件 |
| **glow（光晕）** | `identity` / `primary` / `none` | 小组件光晕取色：各组件身份色 / 统一主题色 / 关闭。仅作用于 `GlowBackground` |

默认 `{ surface: 'glass', glow: 'identity' }` —— 等于系统引入前的历史观感，**零视觉回归**。

---

## 2. 分层架构

```
┌─ 令牌层    theme.css：--surface-rgb / --surface-alpha / --surface-filter / -border / -shadow
│            .glass / .glass-surface { background: rgb(var(--surface-rgb) / var(--surface-alpha)); … }
│            表面模式 = :root[data-surface='…'] 只调 alpha + filter(+border/shadow)
├─ 状态层    useWidgetTheme.ts：模块级全局 state + 订阅者 + 防抖保存（仿 useTitleFont 范式，无 Context）
├─ 应用层    updateGlobalState → html[data-surface] + html[data-glow] 两个根属性，
│            其后全部由 CSS 驱动（组件零订阅）；React 订阅仅选择器 UI 与
│            SurfaceThemeApplier.tsx（常驻 AppLayout、渲染 null，保证全页面应用）
├─ 持久层    后端 widget_theme 字段（JSON 字符串），复用 /api/config/dashboard
├─ 消费层    .glass / .glass-surface（全站）· .glow-orb（data-glow 覆盖色/隐藏）· chrome 清单（见 §5）
└─ 第三方源  Tapp registerTheme → useTappThemes 白名单消毒 → 预设
```

### 令牌层（core abstraction）

核心表达是 **`基色 + 透明度`**，透明度是可被组件覆盖的语义抽象点：

```css
:root {
  --surface-rgb: 255 255 255;              /* 基色，html.dark 改为 26 26 26 */
  --surface-alpha: 80%;                    /* 透明度：组件可覆盖以保留自身配置 */
  --surface-filter: blur(16px) saturate(180%);
  --surface-border: rgb(255 255 255 / 30%);
  --surface-shadow: 0 8px 32px 0 rgb(31 38 135 / 10%);
}
.glass {
  background: rgb(var(--surface-rgb) / var(--surface-alpha));
  backdrop-filter: var(--surface-filter);
  border: 1px solid var(--surface-border);
  box-shadow: var(--surface-shadow);
}
```

基础四模式只调 `alpha` 与 `filter`：`glass` 80% / `solid` 100%+none / `flat` 65%+none / `outline` 20%+blur(3px)。

**逃逸令牌 `--surface-bg`**：背景表达式默认 `rgb(rgb基色 / alpha)`，模式可用 `--surface-bg` 声明任意表达式覆盖。

**`liquid` 的背景三层令牌**（`--surface-bg` 只组合一次，明暗/chrome 各自只覆盖需要的一层，靠 CSS 变量惰性求值自动取值）：
- `--surface-noise`：SVG feTurbulence 磨砂纹理（~5% 白噪点，明暗通用，兼消渐变色带）
- `--surface-sheen`：顶部浸润高光渐变（亮 14% / 暗 6%）
- `--surface-tint`：吸色玻璃底 `color-mix(rgb基色, --color-primary)` —— 换壁纸自动换吸附色，纯 CSS

低模糊 `blur(12px)` 让壁纸细节透出（比传统毛玻璃更"流体"）；`saturate(200%)` + 明暗分向 `brightness`
（亮 1.06 提亮 / 暗 0.88 **压暗**——暗卡提亮会发灰泛白，故暗色走烟熏玻璃）；双层投影（贴地短影 + 环境长影）+
底部暗色发丝线（`inset 0 -1px 0`，玻璃厚度感）。

**`liquid` 的边缘色散弯光环**：`::before` 环形 mask + 角向 conic 渐变（顶部最亮、两侧收暗、底部回亮），
掺极低透明度冷蓝/暖橙/紫模拟边缘色散。纯绘制层渐变，零额外 backdrop 采样，桌面移动通用。
**不能用嵌套 backdrop-filter 做真折射**：父级玻璃自身就是 backdrop root，子层的 backdrop-filter
采样不到卡片背后的内容，只会渲染成乳白带（已踩坑验证）。
门控：`@supports (mask-composite: exclude)` + 排除 `.fixed`/`.absolute` 定位的玻璃
（避免强制 relative 破坏锚定），不满足时静默退化为无环的流体观感。移动端重滤镜由专属媒体降级回 blur(8px)。

**内容密集 chrome 变体**：控制面板信息岛 / Agent 面板原生 90%+ 不透明底，流体 55% 高透会压穿可读性 ——
只覆盖 `--surface-tint` 一层升到 84% 不透明，噪点/高光/滤镜/弯光环全部自动保留。

---

## 3. 数据流

**读（页面加载）**
```
/api/config/ui → useWidgetTheme.init → 白名单校验 → globalState
              → html[data-surface] + html[data-glow] → CSS 切换 → 全站跟随
```
访客也走这条：读到站主保存的最终 surface/glow，无需登录、无需解析 Tapp。

**写（管理员切换）**
```
TitleFontSelector（编辑模式）→ setSurface/setGlowMode
  → updateGlobalState（即时写根属性，全站纯 CSS 即时生效，零组件重渲染）
  → 防抖 500ms → POST /api/config/dashboard { widget_theme: JSON }
```

---

## 4. 关键设计决策

- **两个维度都是根属性 + CSS 驱动，组件零订阅**：切主题只改 `html[data-surface]` / `html[data-glow]` 两个属性，`.glass` 靠令牌继承、`.glow-orb` 靠属性选择器覆盖（primary 换色 / none 隐藏）。WidgetShell 与 GlowBackground 均无 hook 订阅——切主题零组件重渲染，GlowBackground 的 `memo` 优化完整生效（光晕身份色经内联 `--glow-color` 变量下发，CSS 可覆盖）。
- **层叠胜出确认过**：构建产物里 App CSS（含 theme.css）排在 astro 内联 `<style>` 之后 → theme.css 的 `.glass` 胜出。故只改 theme.css 一处即驱动全站，未碰两个 astro 入口页的冗余内联 `.glass`（它们只兜首屏、同 head 渲染阻塞无闪烁）。
- **outline 保留可读性**：描边态是 20% 底 + 3px 模糊 + 明显边框，**非纯透明**，故功能性弹窗在任何 surface 下都可读，无需为它们做例外。
- **真正的性能档**：`solid`/`flat` 的 `--surface-filter: none`（不是 blur 0），彻底去掉 backdrop-filter 合成开销。
- **不用 localStorage**：正经设置走后端 config，访客也能看到站主配置。
- **持久化名沿用 `widget_theme`**：字段名是历史内部名，前端概念已是「全站表面」，未改名以免数据迁移。

---

## 5. 扩展指南

### 加一种新表面模式（如 `frost`）
1. `useWidgetTheme.ts`：`WidgetSurface` 加 `'frost'`；`SURFACE_OPTIONS` 加一项（含预览色块类名）。
2. `theme.css`：加 `:root[data-surface='frost'] { --surface-alpha/filter/… }` 和 `.surface-swatch--frost`。
3. i18n 四语言加 `surfaceFrost` 标签。

### 让一个新组件跟随主题
- **完整玻璃卡片**：给根元素用 `.glass` 类 —— 自带底色/边框/阴影，自动跟随，什么都不用做。
- **手写玻璃卡片（自带边框/阴影，只想统一玻璃底）**：用 `.glass-surface` 替换内联的
  `bg-white/X dark:bg-Y backdrop-blur-Z`，边框/阴影/圆角等工具类保留。透明度加档位类
  `.glass-60/70/90/95`（默认 80%）—— **档位仅玻璃默认态生效，切主题时让位给全局收敛**。
  Tapp/Brew 卡片、共享控制岛（`ISLAND_GLASS`）走的都是这条。
- **保留自定义透明度的浮动 chrome（CSS 文件里的具名类）**：在 theme.css 的 chrome 覆盖清单里加选择器。
  当前清单：`.dynamic-island` · `.secondary-island` · `.control-bar-trigger` · `.site-footer-content` · `.arael-panel`。
  用 `:root[data-surface] <selector>` 作用域 —— 默认态保留组件硬编码透明度，仅切非玻璃主题时收敛。

### Tapp 注册主题（第三方源）—— 安全边界
Tapp 经 `component.registerTheme()` 注册，后端存 `_component:theme:{id}`，`useTappThemes` 消费。**务必守住**：
- 只消费白名单枚举 `surface` / `glow`，其它值一律丢弃。
- **绝不消费 `styles`**（自由 CSS 字符串）—— 注入风险。
- **暂不消费 `colors`** —— 会与壁纸取色系统争夺 `--color-*`。

一个 Tapp 主题在宿主侧只等价于「surface + glow 的具名组合」，应用走 `useWidgetTheme` 的二次校验 setter。列表接口需登录态、按 user_id 过滤，故仅管理员编辑态可见。

---

## 6. 文件清单

| 层 | 文件 |
| --- | --- |
| 令牌 / 表面模式 / chrome 清单 / 预览色块 | `frontend/src/styles/theme.css` |
| 状态 + 持久化 + `html[data-surface]` applier | `frontend/src/hooks/useWidgetTheme.ts` |
| 全站应用器（AppLayout 常驻） | `frontend/src/components/SurfaceThemeApplier.tsx` |
| 表面消费（小组件外壳） | `frontend/src/components/widgets/shared/WidgetShell.tsx` |
| 光晕消费 | `frontend/src/components/widgets/shared/GlowBackground.tsx`（下发 `--glow-color`）· `GlowBackground.css`（`data-glow` 模式规则） |
| 切换 UI（编辑模式「样式」面板） | `frontend/src/components/TitleFontSelector.tsx` |
| Tapp 主题消毒消费 | `frontend/src/hooks/useTappThemes.ts` |
| Tapp 主题契约 | `frontend/src/tapp/services/TappApiService.ts`（`ThemeComponentConfig`） |
| 后端字段 / 解析 / 读写 | `backend/src/config.rs` · `services/config_service.rs` · `api/config.rs` |

---

## 7. 已适配 / 未纳入

**已跟随主题**：所有 `.glass` 组件 · 小组件 · 浮动 chrome（§5 清单）· Tapp 应用卡片与详情弹窗 ·
Brew 卡片/空态/编辑弹窗 · 共享控制岛（`ISLAND_GLASS`）—— 后三类经 `.glass-surface` 整合。

按设计**不跟随**主题的：
- 功能性遮罩 / hover 态 / 徽章 / 骨架屏等零散 `backdrop-blur`（低透明度小元素，非卡片面）。
- 部分内容卡片硬编码玻璃（`.config-section` / `.modal-content` / 音乐播放器 / 设置向导等）——
  如需纳入，把内联 `bg-white/X … backdrop-blur` 换成 `.glass-surface [+ 档位类]` 即可，但要评估 outline 态下表单的可读性。
- `.user-modal-login-only .glass`：登录框专属强制透明（`!important`），是既有的特意设计。
