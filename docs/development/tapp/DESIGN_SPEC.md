# UI 设计规范摘要

本文档是 Myriad 当前设计语言的精简摘要，会被无条件注入 Tapp Playground 生成
agent 的系统提示。完整细节见同目录 `STYLING.md`。改动本文档时保持精炼：它占用
每次生成的固定 token 预算。

## 设计语言

- **壁纸主色驱动**：全局强调色来自用户壁纸，随主题变化。任何强调色都用
  `var(--tapp-primary)`，透明度变体用 `rgba(var(--tapp-primary-rgb), α)`。
  **不要硬编码品牌色**（indigo/violet/fuchsia 等 Tailwind 色相不能当强调色）。
- **Glass 毛玻璃**：容器用半透明背景 + `backdrop-filter: blur(10px)` + 细边框。
  沙箱**不内置** `.glass` 工具类，需要在 `styles.css` 中自己定义：

  ```css
  .glass {
    background: rgba(255, 255, 255, 0.6);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
    border: 1px solid rgba(255, 255, 255, 0.2);
  }
  .dark .glass {
    background: rgba(26, 26, 26, 0.8);
    border: 1px solid rgba(255, 255, 255, 0.05);
  }
  ```

- **中性灰暗色**：深色模式用纯中性灰（Tailwind `neutral` 色系或
  `rgba(255,255,255,α)` 层次），不用带蓝调的 `gray`/`slate`。暗色层次靠白色低
  透明度（`dark:bg-white/5`、`dark:bg-white/10`）而非更亮的灰。
- **语义状态色**：成功 green、错误 red、警告 yellow、信息 blue，均配
  `dark:` 变体（如 `text-green-600 dark:text-green-400`）。

## 沙箱提供的 CSS 变量

| 变量 | 说明 |
| --- | --- |
| `--tapp-primary` / `--tapp-primary-rgb` | 壁纸主色及其 RGB 分量 |
| `--text-primary` / `--text-secondary` | 语义文字色（随主题切换） |
| `--bg-primary` | 语义背景色（浅色 `#fff`，深色 `#0a0a0a`） |
| `--tapp-scale` / `--tapp-font-scale` | 容器与字体缩放因子 |
| `--tapp-safe-inset-top/right/bottom/left` | 安全区域边距 |

沙箱内**没有** `--color-primary`（那是宿主变量），不要引用它。

## Tailwind 约束

沙箱按需构建 Tailwind，支持标准类、`dark:`、`hover:`/`focus:`、数值与
CSS 变量任意值（`w-[200px]`、`bg-[var(--tapp-primary)]`）。**不支持**：
响应式断点（`sm:`/`md:`/`lg:`，用 CSS 媒体查询或 JS 判断容器宽度）、
`@apply`、任意属性语法、hex 任意值（`bg-[#1da1f2]` 无效，写进 `styles.css`）。

## 尺度规范

- **字号**：标题 `text-xl`/`text-2xl`，正文 `text-base`，辅助 `text-sm`/`text-xs`；
  大数字展示用 `text-3xl`/`text-4xl` + `font-bold`。
- **间距**：内边距 `p-3`/`p-4`，间隙 `gap-2`/`gap-3`/`gap-4`；同一界面保持一致。
- **圆角**：按钮 `rounded-md`/`rounded-lg`，卡片 `rounded-lg`/`rounded-xl`，
  对话框 `rounded-2xl`，外层圆角大于内层。
- **z-index**：装饰 `-1`、内容 `0`、浮动 `10`、固定栏 `20`、弹层 `30-40`、
  模态/Toast `50+`。

## 组件基调

- 主按钮：`background: var(--tapp-primary)` + 白字 + `rounded-lg` +
  `transition-colors`，hover 用透明度或 `filter: brightness()` 调整。
- 次要按钮：`bg-neutral-100 dark:bg-neutral-700` + 中性文字。
- 输入框：中性边框，聚焦态用主色环
  `focus:ring-2` + `box-shadow: 0 0 0 2px rgba(var(--tapp-primary-rgb), .2)`。
- 卡片：glass 容器 + `rounded-xl p-4`，标题 `text-lg font-semibold`。
- 过渡统一 `transition-all duration-200`，hover 可 `scale-105`。

## 硬性要求

- 同时支持浅色/深色主题（`dark:` 类或 `Tapp.ui.onThemeChange`），
  不硬编码文字/背景色。
- 窄屏与宽屏都可用；间距字号可乘 `var(--tapp-scale)` / `var(--tapp-font-scale)`。
- 交互控件带可访问标签（`aria-label`）与可见聚焦态。
- 文案通过 `Tapp.i18n.t()` 输出，提供 `zh-CN`、`en-US`、`ja-JP` 语言表。
