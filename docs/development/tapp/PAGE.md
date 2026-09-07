# 页面样式规范

> **不是 Tapp 沙箱契约。** 本文档提取的是 **Myriad 宿主 React 页面**（首页、报告页、
> 资料库）的 chrome：`AnimatedView`、`className`、`sm:`/`md:`/`lg:`。
> Tapp 沙箱没有这些 React 组件，按需 Tailwind **不编译**断点前缀。
> 写 Tapp 页面请用 [STYLING.md](./STYLING.md) 与 [DESIGN_SPEC.md](./DESIGN_SPEC.md)：
> 结构进 `page.html`（宿主包进 `#tapp-content`），样式用 `dark:` / 媒体查询 /
> `var(--tapp-primary)`，不要抄下面的 `sm:px-6`。

本文档基于 Myriad 真实系统页面（首页、报告页、资料库页等）提取的样式规范，给宿主页面和视觉对照用，不作为 Playground 生成依据。

---

## 页面基础结构

### 标准页面布局

所有页面使用 `AnimatedView` 包装器：

```tsx
<AnimatedView className="min-h-screen px-4 sm:px-6 pt-20 pb-24 md:pb-12">
  <div className="max-w-6xl mx-auto">{/* 页面内容 */}</div>
</AnimatedView>
```

### 页面间距规范

| 位置         | 移动端 | 桌面端 | Tailwind 类      |
| ------------ | ------ | ------ | ---------------- |
| 水平内边距   | 16px   | 24px   | `px-4 sm:px-6`   |
| 顶部内边距   | 80px   | 80px   | `pt-20`          |
| 底部内边距   | 96px   | 48px   | `pb-24 md:pb-12` |
| 内容最大宽度 | -      | 1152px | `max-w-6xl`      |

---

## 深色模式背景色

> **重要**：深色模式必须使用纯黑色系（`black`, `neutral`），不使用带蓝色调的颜色（`gray`, `slate`）。

### 推荐背景色

| 层级     | Tailwind 类           | 实际颜色                 | 用途           |
| -------- | --------------------- | ------------------------ | -------------- |
| 最深背景 | `dark:bg-neutral-950` | `#0a0a0a`                | 页面底层背景   |
| 主要容器 | `dark:bg-black/90`    | `rgba(0,0,0,0.9)`        | 卡片、面板     |
| 次级容器 | `dark:bg-black/80`    | `rgba(0,0,0,0.8)`        | 悬浮层、工具栏 |
| 输入控件 | `dark:bg-neutral-900` | `#171717`                | 输入框背景     |
| 次级控件 | `dark:bg-neutral-800` | `#262626`                | 按钮、标签     |
| 微透明层 | `dark:bg-white/5`     | `rgba(255,255,255,0.05)` | 悬停效果       |
| 轻透明层 | `dark:bg-white/10`    | `rgba(255,255,255,0.1)`  | 高亮区域       |

### 禁止使用的背景色

以下颜色带有蓝色调，**不应在深色模式中使用**：

```css
/* ❌ 禁止使用 - 带蓝色调 */
dark:bg-gray-900      /* #111827 - 带蓝色 */
dark:bg-gray-800      /* #1f2937 - 带蓝色 */
dark:bg-slate-900     /* #0f172a - 带蓝色 */
dark:bg-slate-800     /* #1e293b - 带蓝色 */

/* ✅ 使用替代 */
dark:bg-neutral-900   /* #171717 - 纯中性灰 */
dark:bg-neutral-800   /* #262626 - 纯中性灰 */
dark:bg-black/90      /* 纯黑色 90% 不透明度 */
```

### 实际使用示例

```tsx
// 页面卡片背景
<div className="bg-white dark:bg-black/90 rounded-xl shadow-lg">

// TappListPage.tsx - 容器背景
<div className="bg-white/80 dark:bg-black/80 backdrop-blur-xl">

// WidgetGrid.tsx - 工具栏
<div className="bg-white/80 dark:bg-black/80 backdrop-blur-xl border-b border-gray-200/50 dark:border-white/5">

// 页面信息面板
<div className="bg-white/95 dark:bg-neutral-950/95 backdrop-blur-sm rounded-lg p-3">
```

---

## Glass 毛玻璃容器

### 系统定义

来自 [theme.css](../../../frontend/src/styles/theme.css)：

```css
.glass {
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(16px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.3);
  box-shadow: 0 8px 32px 0 rgba(31, 38, 135, 0.1);
}

html.dark .glass {
  background: rgba(26, 26, 26, 0.8); /* 纯黑 #1a1a1a */
  backdrop-filter: blur(16px) saturate(180%);
  border: 1px solid rgba(255, 255, 255, 0.1);
  box-shadow: 0 8px 32px 0 rgba(0, 0, 0, 0.5);
}
```

### 使用方式

```tsx
// Details.tsx - 标准 Glass 容器
<div className="glass rounded-xl p-6 md:p-8">
  <h1 className="text-2xl md:text-3xl font-bold mb-4">标题</h1>
  <p className="text-gray-600 dark:text-gray-400">内容</p>
</div>

// SetupWizard.tsx - Glass 步骤指示器
<div className="glass rounded-xl p-1.5 inline-flex gap-1">
  <button className="bg-white dark:bg-neutral-900 shadow-sm">步骤 1</button>
</div>
```

### 移动端优化

移动端自动降低模糊强度以提升性能：

```css
@media (max-width: 767px) {
  .glass {
    backdrop-filter: blur(12px);
    box-shadow: 0 4px 16px 0 rgba(31, 38, 135, 0.08);
  }
}
```

---

## 边框样式

### 深色模式边框

| 用途     | Tailwind 类                  | 效果               |
| -------- | ---------------------------- | ------------------ |
| 主要边框 | `dark:border-white/10`       | 10% 白色，清晰可见 |
| 次级边框 | `dark:border-white/5`        | 5% 白色，微弱分隔  |
| 容器边框 | `dark:border-neutral-700/50` | 半透明中性灰       |
| 分隔线   | `dark:border-neutral-700`    | 实色中性灰         |

### 使用示例

```tsx
// 卡片边框
<div className="border border-gray-200/50 dark:border-white/5">

// 分隔线
<div className="h-5 w-px bg-gray-300 dark:bg-white/10 mx-2" />

// 容器边框
<div className="border dark:border-neutral-700/50 rounded-xl">
```

---

## 卡片样式

### 标准卡片

来自 `ReportCardWidget.tsx`、`TappListPage.tsx` 的真实卡片样式：

```tsx
// 基础卡片
<div className="relative rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
  {/* 卡片内容 */}
</div>

// 可交互卡片
<div className="relative bg-white rounded-xl shadow-md
                hover:shadow-2xl transition-all duration-300
                transform hover:-translate-y-1 hover:scale-[1.02]
                overflow-hidden">
  {/* 卡片内容 */}
</div>

// 信息卡片（带背景渐变）
<div className="relative h-full w-full overflow-hidden">
  {/* 背景渐变 */}
  <div className="absolute inset-0 bg-gradient-to-br from-gray-100/50 to-transparent dark:from-white/[0.02] dark:to-transparent" />
  {/* 内容 */}
</div>
```

### 封面卡片（带遮罩）

用于展示媒体内容：

```tsx
<div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
  {/* 封面图片 */}
  <div className="absolute inset-0">
    <img src={cover} className="w-full h-full object-cover" loading="lazy" />
    {/* 渐变遮罩 */}
    <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent" />
  </div>
  {/* 文字内容（位于遮罩上方） */}
</div>
```

---

## 文字颜色

### 深色模式文字

| 用途     | Tailwind 类                              | 效果       |
| -------- | ---------------------------------------- | ---------- |
| 主要标题 | `dark:text-white` / `dark:text-gray-100` | 最高对比度 |
| 副标题   | `dark:text-gray-200`                     | 高对比度   |
| 正文     | `dark:text-gray-300`                     | 标准可读性 |
| 次要文字 | `dark:text-gray-400`                     | 次要信息   |
| 辅助文字 | `dark:text-gray-500`                     | 低优先级   |

### 使用示例

```tsx
// ReportCardWidget.tsx - 数字显示
<span className="text-5xl font-black text-gray-800 dark:text-gray-100 leading-none">
  {score}
</span>

// 标签文字
<span className="text-xs text-gray-500 dark:text-gray-400 font-bold">
  /100
</span>

// 类型标签
<span className="text-[10px] font-bold text-gray-100 dark:text-gray-900 uppercase tracking-wide">
  {type}
</span>
```

---

## 控件样式

### 输入框

```tsx
<input
  type="text"
  className="w-full px-3 py-2 rounded-lg 
             bg-white dark:bg-neutral-800 
             border border-gray-200 dark:border-neutral-700 
             text-gray-800 dark:text-gray-100 
             placeholder-gray-400 
             focus:border-indigo-500 focus:ring-2 focus:ring-indigo-500/20 
             transition-all outline-none"
  placeholder="输入内容..."
/>
```

### 按钮

```tsx
// 主要按钮
<button className="px-4 py-2 rounded-lg
                   bg-indigo-600 hover:bg-indigo-700
                   text-white font-medium
                   transition-colors">
  主要操作
</button>

// 透明背景按钮（深色模式）
<button className="px-3 py-1.5 rounded-xl backdrop-blur-md shadow-lg
                   bg-white/90 dark:bg-black/90
                   border border-white/30 dark:border-white/10">
  透明按钮
</button>
```

### 标签徽章

```tsx
// 亮色背景标签
<div className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md
                bg-gray-800/90 dark:bg-white/90 backdrop-blur-sm">
  <span className="text-[10px] font-bold text-gray-100 dark:text-gray-900 uppercase tracking-wide">
    {label}
  </span>
</div>

// 动态颜色标签
<div className="px-2 py-0.5 rounded-md text-[9px] font-bold shadow-sm"
     style={{
       backgroundColor: `${color}20`,
       color: color
     }}>
  {text}
</div>
```

---

## 渐变与装饰

### 背景渐变

```tsx
// 微弱对角渐变（用于卡片背景）
<div className="absolute inset-0 bg-gradient-to-br from-gray-100/50 to-transparent dark:from-white/[0.02] dark:to-transparent" />

// 底部渐变遮罩（用于封面图片）
<div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/40 to-transparent" />

// 强调色渐变
<div className="absolute inset-0 bg-gradient-to-br from-indigo-500/5 to-transparent dark:from-indigo-500/10" />
```

### 进度条

```tsx
<div className="w-full bg-gray-200 dark:bg-neutral-800 rounded-full h-2 overflow-hidden">
  <div className="h-full bg-indigo-500 rounded-full" style={{ width: `${progress}%` }} />
</div>

// 轻量进度条
<div className="h-[3px] w-full bg-gray-200 dark:bg-white/5 rounded-full overflow-hidden">
  <div className="h-full" style={{ width: `${progress}%`, backgroundColor: color }} />
</div>
```

---

## 动画效果

### 进入动画

```tsx
// 淡入上滑
<div className="animate-in fade-in slide-in-from-bottom-4 duration-700 ease-out">

// Framer Motion 动画
<motion.div
  initial={{ y: -20, opacity: 0 }}
  animate={{ y: 0, opacity: 1 }}
  transition={{ duration: 0.6, delay: 0.1 }}
>
```

### 交互动画

```tsx
// 悬停放大
<div className="transition-transform hover:scale-[1.02] hover:-translate-y-1">

// 悬停阴影
<div className="shadow-md hover:shadow-2xl transition-all duration-300">

// 弹性缩放
<motion.span
  initial={{ scale: 0.5 }}
  animate={{ scale: 1 }}
  transition={{ duration: 0.5, type: "spring", stiffness: 200 }}
>
```

### 切换动画

```tsx
<AnimatePresence mode="wait">
  {condition ? (
    <motion.div
      key="state-a"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.5 }}
    >
      内容 A
    </motion.div>
  ) : (
    <motion.div
      key="state-b"
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      transition={{ duration: 0.5 }}
    >
      内容 B
    </motion.div>
  )}
</AnimatePresence>
```

---

## 响应式设计

### 断点

| 断点   | 宽度     | Tailwind 前缀 |
| ------ | -------- | ------------- |
| 移动端 | < 640px  | 默认          |
| 小平板 | ≥ 640px  | `sm:`         |
| 平板   | ≥ 768px  | `md:`         |
| 笔记本 | ≥ 1024px | `lg:`         |
| 桌面   | ≥ 1280px | `xl:`         |

### 常见响应式模式

```tsx
// 内边距
<div className="p-4 md:p-6 lg:p-8">

// 网格列数
<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">

// 字号
<h1 className="text-2xl md:text-3xl lg:text-4xl font-bold">

// 显示/隐藏
<div className="hidden md:block">桌面端显示</div>
<div className="md:hidden">移动端显示</div>
```

---

## 最佳实践

### 1. 统一使用纯黑色系

```tsx
// ✅ 正确
<div className="dark:bg-black/90">
<div className="dark:bg-neutral-900">
<div className="dark:bg-neutral-950">

// ❌ 错误 - 带蓝色调
<div className="dark:bg-gray-900">
<div className="dark:bg-slate-800">
```

### 2. 保持层次清晰

```tsx
// 使用透明度区分层级
<div className="dark:bg-black/80">
  {" "}
  {/* 底层 */}
  <div className="dark:bg-white/5">
    {" "}
    {/* 中层 */}
    <div className="dark:bg-white/10"> {/* 顶层高亮 */}</div>
  </div>
</div>
```

### 3. 考虑性能

```tsx
// 移动端减少 backdrop-filter 使用
<div className="backdrop-blur-xl md:backdrop-blur-2xl">

// 使用 GPU 加速动画
<div className="transform-gpu hover:scale-105">
```

### 4. 保持视觉一致性

```tsx
// 统一圆角
<div className="rounded-xl">      {/* 大容器 */}
  <div className="rounded-lg">    {/* 中型元素 */}
    <button className="rounded-md"> {/* 小型控件 */}
    </button>
  </div>
</div>

// 统一阴影
<div className="shadow-lg">       {/* 主要卡片 */}
<div className="shadow-md">       {/* 次级卡片 */}
<div className="shadow-sm">       {/* 小型元素 */}
```
