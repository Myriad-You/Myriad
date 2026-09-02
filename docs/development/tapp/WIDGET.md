# 小组件开发指南

本文档基于 Myriad 系统内置小组件的实际实现，提供完整的风格规范和开发指南。

> 📌 本文档内容参考系统小组件：QuickStatsWidget、WeatherWidget、WelcomeWidget、QuoteWidget、MusicPlayerWidget 等。
>
> 📌 **加载态**：Dashboard 第三方 Widget 的整卡 loading 由宿主 `WidgetSkeleton` 统一处理（见 [宿主加载骨架](#宿主加载骨架widget-skeleton)）；内置小组件也复用同一组件。

> **✨ 样式推荐**：虽然小组件完全支持 Tailwind CSS，但我们**强烈建议使用语义化的原生 CSS**：
>
> - 更好的可维护性，避免冗长的 Tailwind 类名列表
> - 更小的体积，无需 Tailwind 运行时编译
> - 更容易实现复杂的 hover/focus/动画效果
> - Widget 层可声明自己的样式文件
>
> 详见 [样式规范 - 推荐原生 CSS](STYLING.md#-推荐语义化原生-css)。

---

## Widget SDK 限制

> ⚠️ **重要**：Widget 模式使用**简化版 SDK**，仅包含以下 API。需要完整功能请使用 Page 模式。

| 分类               | Widget SDK                                     | Full SDK (Page/headless) |
| ------------------ | ---------------------------------------------- | ------------------------ |
| **存储/全局设置**  | ✅ 完整，含 `getAll`/`usage`                   | ✅ 相同                  |
| **实例设置 / invalidate** | ✅ `getInstanceSettings` / `updateInstanceSettings` / **`invalidate`** | ❌ **仅 Widget 沙箱**（Page 的 `Tapp.widget` 是 register 系列；headless 无 `Tapp.widget`） |
| **UI/用户/上下文** | ✅ 主题、通知、语言、角色和运行上下文；**`Tapp.ui.openUrl` / `listOpenUrls`**（需 `ui:openUrl` + `openUrls`） | ✅ 另含 fullscreen/title；同样支持 openUrl |
| **人设名片**       | ✅ `Tapp.persona.get`（无需权限；只读名字、心情带、同源立绘路径） | ✅ 相同                  |
| **DOM**            | ✅ 与 Full 共用安全 helper                     | ✅ 相同                  |
| **包内资源**       | ✅ `Tapp.assets`（list / getUrl / getArrayBuffer / revoke） | ✅ 相同       |
| **AI**             | ✅ Manifest 声明的 AI Task                     | ✅ 相同                  |
| **平台数据/报告**  | ✅ 只读                                        | ✅ 读写                  |
| **媒体/语音/动画** | ✅ 按 Manifest 权限                            | ✅ 相同                  |
| **事件/数据交换**  | ✅ Event、一次性授权 Data Exchange、Agent 交互 | ✅ 相同                  |
| **后台需求/调度**  | ✅ 完整                                        | ✅ 相同                  |
| **声明 API**       | ✅ `Tapp.api()` 与 `Tapp.api.list()`           | ✅ 相同                  |
| **生命周期**       | ✅ `onReady` / `onDestroy` / pause / resume    | ✅ 相同                  |
| **管理/联邦能力**  | ❌ Tapp/Brew 管理、组件、快捷键、Federation 等 | ✅ 按权限提供            |

Widget 不是纯静态展示层：共享 core 可在其中使用事件、调度和数据交换。但宿主不会给 Widget
注册平台/报告写入、Tapp/Brew 管理、组件、快捷键和 Federation handler。

---

## 基础结构

Widget 层入口里写渲染逻辑：

```javascript
// WIDGET_CODE - 小组件渲染代码
Tapp.widgets["my-widget"] = {
  render: async function (container, props) {
    // 渲染逻辑
  },
};
```

> **生命周期**：Widget SDK **会**在 document load 后触发 `Tapp.lifecycle.onReady`（以及
> pause/resume/destroy）。可见 UI 仍应主要通过 `Tapp.widgets[id].render(container, props)`
> 由宿主驱动；`onReady` 适合初始化订阅、预取数据或配合 core 的共享逻辑，不要假设只有
> Page 模式才有 onReady。

Manifest 顶层 `settings` 由整个 Tapp 共享；`widgets[].settings` 则为每个 Dashboard
实例独立保存。Widget 可读取 `props.config` 或 `Tapp.widget.getInstanceSettings()`，并用
`Tapp.widget.updateInstanceSettings(patch)` 更新已声明字段。

**数据怎么进 Widget、怎么更新**：推荐用声明式 `Tapp.api` + 共享 `Tapp.storage` +
宿主 `refreshPolicy`（默认 storage 变更即刷）。详见下方
[数据加载与更新](#数据加载与更新)。

**加载态由宿主统一处理**（第三方勿再画整卡 Spinner）：见
[宿主加载骨架](#宿主加载骨架widget-skeleton)。

---

## 宿主加载骨架（Widget Skeleton）

Dashboard 上**所有第三方 Tapp 小组件**默认接入宿主通用骨架
（`frontend/src/components/widgets/shared/WidgetSkeleton`），内置系统小组件也复用同一套。

### 设计原则（最高效默认）

| 原则 | 说明 |
| --- | --- |
| **宿主兜底** | chunk 懒加载、runtime 同步、iframe `tapp.ready` 前、屏外占位均由宿主画骨架；Tapp 作者**不要**再实现整卡 loading 闪屏 |
| **单骨 block** | Tapp 外壳默认 `preset="block"`（一块 shimmer），避免复杂多块布局增加 DOM/合成成本 |
| **延迟绘制 `deferMs=100`** | 用于 **Suspense / sandbox / 库预览**；&lt;100ms 完成则不画 shimmer。chunk 已解析后的 runtime `loading` 用 **`deferMs=0`**，避免「Suspense 结束后再空 100ms」 |
| **屏外 hold** | 未进视口：`hold` 模式 = **零 bone DOM** + soft 底 + `content-visibility: auto`，**无 shimmer**（多卡零持续 GPU） |
| **Shimmer 实现** | bone 用 `::after` + `translate3d` 扫光（合成层友好），不用 `background-position` 动画 |
| **染色** | accent 优先 `manifest.themeColor` 字符串；回退用 CSS `var(--color-primary)`，**禁止**在 render 里 `getComputedStyle` 读色 |
| **一层一阶段** | 同一阶段不要叠多层动画骨架（Suspense → runtime → sandbox 是串联替换，不是并行叠三层 shimmer） |
| **ready 超时** | iframe 超过 `readyTimeoutMs`（默认 12s）仍未 ready：骨架底部显示 `common.loadingSlow`（优于无限无反馈） |
| **尽快 ready** | Widget HTML 应尽早发 `tapp.ready`（或走宿主 bridge 的 ready 路径），缩短骨架可见时间 |
| **退出动画** | 用 `WidgetSkeletonCover`（`active` 驱动）：ready/数据到位后 **280ms** 淡出 + 微放大溶解，内容在下方露出；禁止 `{cond && <WidgetSkeleton />}` 瞬切 |

### 链路（宿主侧）

```
useTappWidgets
  Suspense fallback → WidgetSkeleton(block, deferMs=100, themeColor)
        │  chunk resolve
        ▼
TappWidgetRuntime
  loading → WidgetSkeleton(block, deferMs=0)     // 接上一段，不再二次 defer
  未进视口 → WidgetSkeleton(hold)                // 零 bone / 无动画
  已运行 + 视口内 → TappWidgetSandbox
        │
        ▼
TappWidgetSandbox
  WidgetSkeletonCover active={!isReady}  // ready 后 280ms 淡出
  !isReady && >12s → stallMessage (loadingSlow)
  isReady  → cover 退出动画 → unmount
```

宿主内置小组件数据加载同理：内容先挂好，`WidgetSkeletonCover active={loading}` 盖在上面再淡出。

**未运行**的 Tapp 仍显示「启动 / 详情」Glass 卡，**不用**骨架冒充加载。

### 作者侧约定

1. **不要**在 `render` 里再铺全屏 Spinner 盖住宿主骨架；若需局部数据 loading，用卡片**内部**小块即可。
2. 首帧尽快完成 DOM 并触发 **`Tapp.lifecycle.onReady` / 宿主 ready**，以便骨架收起。
3. 主题色写在 `manifest.themeColor`，加载染色更准。
4. 需要自定义宿主骨架形状时（仅内置小组件）：`WidgetSkeleton` 的 `preset` /
   `SkeletonBone` / `accent`；Tapp 作者无此 API，宿主已默认最优路径。

实现参考：

- `frontend/src/components/widgets/shared/WidgetSkeleton.tsx`（`TAPP_WIDGET_SKELETON`）
- `frontend/src/hooks/useTappWidgets.ts`（Suspense 包装）
- `frontend/src/tapp/runtime/TappWidgetSandbox.tsx`（ready 前遮罩）

---

## Props 参数

渲染函数接收的 `props` 对象：

| 属性           | 类型    | 说明                          |
| -------------- | ------- | ----------------------------- |
| `size`         | string  | 当前尺寸 ('1x1', '2x2' 等)    |
| `config`       | object  | 当前 Dashboard 实例的有效配置 |
| `isEditMode`   | boolean | 是否处于编辑模式              |
| `isPreview`    | boolean | 是否预览模式                  |
| `theme`        | string  | 当前主题 ('light' \| 'dark')  |
| `primaryColor` | string  | 系统主题色（如 #8b5cf6）      |
| `scale`        | number  | 缩放比例（0.1-2）             |
| `fontScale`    | number  | 字体缩放（0.6-1.2）           |
| `locale`       | string  | 用户语言（如 'zh-CN'）        |

---

## 尺寸规格

| 尺寸  | 推荐场景                   | 布局模式            |
| ----- | -------------------------- | ------------------- |
| `1x1` | 图标、状态指示器           | 居中单元素          |
| `2x1` | 简单统计、紧凑信息         | 横向紧凑            |
| `2x2` | 标准小组件、信息卡片       | 纵向堆叠            |
| `4x1` | 横幅通知、紧凑预报         | 横向分区（75%+25%） |
| `4x2` | 宽幅展示、图表、音乐播放器 | 横向分区或上下分区  |
| `4x4` | 大型展示、详细数据         | 自由布局            |
| `2x4` | 垂直列表、时间线           | 纵向堆叠            |

---

## 原生 CSS 示例（推荐）

相比内联 Tailwind 类，使用语义化原生 CSS 更易维护：

```css
/* widget.css */
.stats-widget {
  position: relative;
  height: 100%;
  width: 100%;
  border-radius: 12px;
  overflow: hidden;
  background: rgba(255, 255, 255, 0.6);
  backdrop-filter: blur(12px);
}

.dark .stats-widget {
  background: rgba(255, 255, 255, 0.03);
}

.stats-widget-glow {
  position: absolute;
  right: -2rem;
  top: -2rem;
  width: 8rem;
  height: 8rem;
  border-radius: 50%;
  filter: blur(64px);
  opacity: 0.1;
  pointer-events: none;
}

.stats-widget-content {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  padding: 12px;
}

.stats-widget-value {
  font-size: 30px;
  font-weight: 900;
  color: #1f1f1f;
  line-height: 1;
}

.dark .stats-widget-value {
  color: #f5f5f5;
}
```

```javascript
// 简洁的 JS
container.innerHTML = `
  <div class="stats-widget">
    <div class="stats-widget-glow" style="background: ${themeColor}"></div>
    <div class="stats-widget-content">
      <span class="stats-widget-value" style="font-size: ${30 * fontScale}px;">
        ${value}
      </span>
    </div>
  </div>
`;
```

---

## Glass 容器规范

系统小组件统一使用 `glass` class 实现毛玻璃效果：

```html
<div class="relative h-full w-full rounded-xl overflow-hidden glass">
  <!-- 内容 -->
</div>
```

**`glass` class 提供的效果：**

- 半透明背景（亮色：白色 60%，暗色：白色 3%）
- `backdrop-filter: blur(12px)` 毛玻璃模糊
- 细微边框 `ring-1 ring-black/5 dark:ring-white/10`

---

## 光晕背景效果（GlowBackground）

系统小组件统一使用光晕背景装饰，实现方式：

```html
<!-- 右上角光晕（默认） -->
<div
  class="absolute -right-8 -top-8 w-32 h-32 rounded-full blur-3xl opacity-10"
  style="background: ${themeColor}"
></div>

<!-- 左下角光晕 -->
<div
  class="absolute -left-6 -bottom-6 w-24 h-24 rounded-full blur-3xl opacity-10"
  style="background: ${themeColor}"
></div>
```

**光晕尺寸规范：**

| 组件尺寸 | 光晕大小 | 模糊程度 |
| -------- | -------- | -------- |
| 1x1/2x1  | 6rem     | blur-xl  |
| 2x2      | 8rem     | blur-3xl |
| 4x2/4x4  | 12rem    | blur-3xl |

**JavaScript 实现：**

```javascript
function renderGlow(themeColor, position = "right", size = "md") {
  const sizes = { sm: "6rem", md: "8rem", lg: "12rem" };
  const positions = {
    right: "right: -2rem; top: -2rem;",
    left: "left: -1.5rem; bottom: -1.5rem;",
  };

  return `<div style="
    position: absolute;
    ${positions[position]}
    width: ${sizes[size]};
    height: ${sizes[size]};
    border-radius: 9999px;
    background: ${themeColor};
    filter: blur(64px);
    opacity: 0.1;
    pointer-events: none;
  "></div>`;
}
```

---

## 颜色规范（来自系统小组件）

### 背景色

```css
/* Glass 容器 */
glass                                    /* 自动处理亮/暗主题 */

/* 内嵌卡片 */
bg-white/60 dark:bg-white/[0.03]        /* 卡片背景 */
bg-white/40 dark:bg-white/5             /* 悬停效果 */

/* 渐变装饰 */
bg-gradient-to-br from-gray-50/50 to-transparent dark:from-white/[0.02] dark:to-transparent

/* 分隔线 */
border-gray-200/10 dark:border-white/5
border-gray-200/10 dark:border-white/10
```

### 文字色

```css
/* 主标题/大数字 */
text-gray-800 dark:text-gray-100        /* 最强调 */
font-black                              /* 数字使用最粗字重 */

/* 次级标题 */
text-gray-700 dark:text-gray-200        /* 标题 */
font-bold                               /* 粗体 */

/* 正文/描述 */
text-gray-600 dark:text-gray-400        /* 次要文本 */
font-medium                             /* 中等字重 */

/* 辅助信息 */
text-gray-500 dark:text-gray-400        /* 辅助文本 */
text-gray-500 dark:text-gray-500        /* 最弱化 */
```

### 小标签样式

```css
/* 标签/徽章 */
text-[10px] px-2 py-0.5 rounded-full bg-black/5 dark:bg-white/10 text-gray-500 dark:text-gray-400

/* 大写追踪标签（如 "ITEMS"） */
text-xs uppercase tracking-wider font-bold text-gray-500 dark:text-gray-400
```

---

## 字体规范（来自系统小组件）

### 大数字（统计数据）

```javascript
// QuickStatsWidget 风格
`<span class="text-3xl font-black text-gray-800 dark:text-gray-100 leading-none"
       style="font-size: ${30 * fontScale}px;">
  ${value}
</span>` // 带单位标签
`<div class="flex items-baseline gap-1">
  <span class="text-3xl font-black text-gray-800 dark:text-gray-100 leading-none"
        style="font-size: ${30 * fontScale}px;">1234</span>
  <span class="text-xs text-gray-500 dark:text-gray-400 font-bold">ITEMS</span>
</div>`;
```

### 标题（组件顶部）

```javascript
// 小标题（如 "我的资料库"）
`<h3 class="text-xs text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold"
     style="font-size: ${12 * fontScale}px;">
  ${title}
</h3>`;
```

### 内容文本

```javascript
// 主要信息（如歌曲名）
`<div class="font-bold text-gray-800 dark:text-gray-100 truncate"
      style="font-size: ${14 * fontScale}px;">
  ${name}
</div>` // 次要信息（如艺术家）
`<div class="text-gray-600 dark:text-gray-400 truncate"
      style="font-size: ${12 * fontScale}px;">
  ${artist}
</div>`;
```

### 微型文本

```javascript
// 极小标签（如分类标签）
`<span class="text-[7px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold"
       style="font-size: ${7 * fontScale}px;">
  ${label}
</span>`;
```

---

## 布局模式（来自系统小组件）

### 2x2 标准布局

```javascript
// 纵向堆叠：图标 → 主数据 → 次要信息
container.innerHTML = `
  <div class="relative h-full w-full rounded-xl overflow-hidden glass">
    ${renderGlow(themeColor, "right", "md")}
    
    <div class="absolute inset-0 flex flex-col p-3" style="padding: ${
      12 * scale
    }px;">
      <!-- 顶部：图标 -->
      <div class="mb-1">
        <span class="text-3xl" style="font-size: ${30 * scale}px;">🎵</span>
      </div>
      
      <!-- 中部：主要数据（flex-1 占据剩余空间） -->
      <div class="flex-1 flex flex-col justify-center">
        <div class="font-black text-gray-800 dark:text-gray-100"
             style="font-size: ${14 * fontScale}px;">
          ${title}
        </div>
        <div class="text-gray-600 dark:text-gray-400"
             style="font-size: ${12 * fontScale}px;">
          ${subtitle}
        </div>
      </div>
      
      <!-- 底部：操作或辅助信息 -->
      <div class="flex items-center justify-between">
        <!-- 操作按钮 -->
      </div>
    </div>
  </div>
`;
```

### 4x2 横向分区布局

```javascript
// WeatherWidget 风格：左60% + 右40%
container.innerHTML = `
  <div class="relative h-full w-full rounded-xl overflow-hidden glass">
    ${renderGlow(themeColor, "right", "lg")}
    
    <div class="absolute inset-0 flex flex-row px-4 py-3">
      <!-- 左侧：主要信息 (60%) -->
      <div class="w-[60%] flex flex-col justify-between border-r border-gray-200/10 dark:border-white/10">
        <div class="font-bold text-gray-700 dark:text-gray-200">${city}</div>
        <div class="flex items-center gap-3 my-auto">
          <div class="text-4xl">${icon}</div>
          <div class="font-black text-gray-800 dark:text-gray-100"
               style="font-size: ${36 * fontScale}px;">${temperature}</div>
        </div>
        <div class="flex items-center gap-3 text-gray-500" style="font-size: 10px;">
          <span>💧 ${humidity}%</span>
          <span>🍃 ${windSpeed}km/h</span>
        </div>
      </div>
      
      <!-- 右侧：次要信息 (40%) -->
      <div class="w-[40%] pl-2 flex flex-col justify-between gap-1">
        <!-- 预报列表 -->
      </div>
    </div>
  </div>
`;
```

### 4x2 上下分区布局

```javascript
// MusicPlayerWidget 4x2 风格：上2/3（歌词）+ 下1/3（控制）
container.innerHTML = `
  <div class="relative h-full w-full rounded-xl overflow-hidden glass flex flex-col">
    <!-- 上半部分：主要内容 (flex-1) -->
    <div class="flex-1 relative overflow-hidden flex items-center justify-center px-8">
      <!-- 背景模糊封面 -->
      <div class="absolute inset-0 bg-cover bg-center blur-xl opacity-30"
           style="background-image: url(${cover})"></div>
      <!-- 歌词文本 -->
      <div class="relative z-10 font-bold text-gray-800 dark:text-white line-clamp-2"
           style="font-size: ${18 * fontScale}px;">
        ${lyric}
      </div>
    </div>
    
    <!-- 下半部分：控制区 (h-[36%]) -->
    <div class="h-[36%] border-t border-gray-200/10 dark:border-white/5 
                bg-white/30 dark:bg-black/20 backdrop-blur-md 
                flex items-center justify-between px-4">
      <!-- 封面 + 信息 + 控制按钮 -->
    </div>
  </div>
`;
```

### 4x1 紧凑横向布局

```javascript
// WeatherWidget 4x1 风格：左75% + 右25%
container.innerHTML = `
  <div class="relative h-full w-full rounded-xl overflow-hidden glass">
    ${renderGlow(themeColor, "right", "lg")}
    
    <div class="absolute inset-0 flex flex-row px-4 py-2">
      <!-- 左侧：主要信息 (75%) -->
      <div class="w-[75%] flex items-center pr-3 border-r border-gray-200/10 gap-3">
        <div class="text-3xl">${icon}</div>
        <div class="flex flex-col">
          <div class="font-black" style="font-size: ${
            24 * fontScale
          }px;">${temperature}</div>
          <div class="text-gray-500" style="font-size: ${
            12 * fontScale
          }px;">${weather}</div>
        </div>
      </div>
      
      <!-- 右侧：次要信息 (25%) -->
      <div class="w-[25%] pl-1 flex flex-col items-center justify-center">
        <div class="text-[10px] text-gray-400">明天</div>
        <span class="text-base">${tomorrowIcon}</span>
      </div>
    </div>
  </div>
`;
```

---

## 内嵌卡片样式

```javascript
// QuickStatsWidget 统计项样式
`<div class="flex flex-col items-center justify-center 
             bg-white/60 dark:bg-white/[0.03] 
             backdrop-blur-sm rounded-md relative overflow-hidden"
      style="padding: ${6 * scale}px;">
  <!-- 装饰光斑 -->
  <div class="absolute top-0 right-0 w-6 h-6 rounded-full blur-xl opacity-20"
       style="background: ${itemColor}; width: ${24 * scale}px; height: ${
         24 * scale
       }px;"></div>
  
  <!-- 内容 -->
  <div class="relative z-10 flex flex-col items-center gap-0.5">
    <div class="text-gray-700 dark:text-white/60">${icon}</div>
    <span class="text-base font-black text-gray-800 dark:text-gray-200"
          style="font-size: ${16 * fontScale}px;">${value}</span>
    <span class="text-[7px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold"
          style="font-size: ${7 * fontScale}px;">${label}</span>
  </div>
</div>`;
```

---

## 加载与空状态

```javascript
// 加载状态（与系统一致）
`<div class="h-full w-full flex items-center justify-center">
  <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
</div>` // 空状态（居中图标+文字）
`<div class="relative h-full w-full rounded-xl overflow-hidden glass">
  ${renderGlow("#ef4444", "right", "md")}
  <div class="absolute inset-0 flex flex-col items-center justify-center p-3">
    <span class="text-3xl mb-2" style="font-size: ${30 * scale}px;">🎵</span>
    <span class="text-gray-500 dark:text-gray-400"
          style="font-size: ${12 * fontScale}px;">暂无播放</span>
  </div>
</div>` // 不可用状态
`<div class="h-full w-full flex items-center justify-center text-gray-400">
  <span>数据不可用</span>
</div>`;
```

---

## 编辑模式指示器

```javascript
`${
  props.isEditMode
    ? `
  <div class="absolute inset-0 border-2 border-dashed border-blue-400 rounded-xl pointer-events-none"></div>
`
    : ""
}`;
```

---

## 按钮样式

```javascript
// 主按钮（圆形播放按钮风格）
`<button class="rounded-full bg-white/80 dark:bg-black/80 backdrop-blur-sm shadow-md 
                flex items-center justify-center hover:scale-110 transition-transform"
         style="color: ${themeColor}; width: ${32 * scale}px; height: ${
           32 * scale
         }px;">
  ${playIcon}
</button>` // 次要按钮
`<button class="px-3 py-1.5 text-xs font-medium 
                bg-black/5 dark:bg-white/10 hover:bg-black/10 dark:hover:bg-white/15 
                text-gray-700 dark:text-gray-200 rounded-lg transition-colors">
  详情
</button>` // 强调按钮（渐变背景）
`<button class="px-3 py-1.5 text-xs font-medium text-white rounded-lg shadow-sm hover:shadow-md"
         style="background: linear-gradient(135deg, ${themeColor}, color-mix(in srgb, ${themeColor} 80%, black));">
  启动
</button>`;
```

---

## 多尺寸适配模式

```javascript
Tapp.widgets["my-widget"] = {
  render: async function (container, props) {
    const size = props.size || "2x2";
    const scale = props.scale || 1;
    const fontScale = props.fontScale || 1;
    const themeColor = props.primaryColor || "#8b5cf6";

    const isCompact = size === "1x1" || size === "2x1";
    const isWide = size === "4x2" || size === "4x1";
    const isLarge = size === "4x4" || size === "2x4";

    if (isCompact) {
      // 紧凑模式：只显示核心元素（图标+数字）
      container.innerHTML = this.renderCompact(props);
    } else if (isWide) {
      // 宽模式：横向分区布局
      container.innerHTML = this.renderWide(props);
    } else {
      // 标准/大型模式：纵向堆叠
      container.innerHTML = this.renderStandard(props);
    }
  },

  renderCompact(props) {
    return `
      <div class="relative h-full w-full rounded-xl overflow-hidden glass">
        <div class="absolute inset-0 flex items-center justify-center">
          <span class="text-2xl font-black text-gray-800 dark:text-gray-100">42</span>
        </div>
      </div>
    `;
  },

  // ... 其他布局方法
};
```

---

## 国际化适配

```javascript
// 非中文语言时减小字体（英文单词较长）
const isNonChinese = !props.locale.startsWith("zh");
const titleFontScale = isNonChinese ? fontScale * 0.88 : fontScale;
const infoFontScale = isNonChinese ? fontScale * 0.92 : fontScale;

// 日期格式化
const formattedDate = new Date().toLocaleDateString(props.locale, {
  month: "long",
  day: "numeric",
  weekday: "long",
});
```

---

## 安全渲染（防 XSS）

**❌ 错误**：

```javascript
container.innerHTML = `<div>${userData.name}</div>`;
```

**✅ 正确**：

```javascript
const div = Tapp.dom.createElement("div", { className: "widget-content" });
const title = document.createElement("h3");
Tapp.dom.setText(title, userData.name);
div.appendChild(title);
container.appendChild(div);
```

---

## 数据加载与更新

Widget **不是**平台首页数据源，也**不能**在沙箱里直接 `fetch` / 开 WebSocket。
外部 HTTP 与受控能力必须先写进 `manifest.apis`，再通过 `Tapp.api(name, params)` 调用。
同一 Tapp 内 Page、Widget、headless core 共享一份 storage；宿主按 `refreshPolicy`
决定何时重新调用 `render`。

### 推荐链路（core 拉数 → storage → Widget 刷新）

```
manifest.apis + permissions（含 network:fetch、storage:read、storage:write）
        │
        ▼
core / Page / headless（scheduler 或 onReady）
  Tapp.api("stats", …)  ──►  Tapp.storage.set("stats.summary", data)
        │                      ▲ 到此为止；不要在 core 里调 invalidate
        │  同 Tapp storage 变更由宿主广播
        ▼
Dashboard Widget（refreshPolicy 默认 event）
  宿主 re-render → render() 里 Tapp.storage.get("stats.summary")
  可选（仅本 Widget 沙箱）：Tapp.storage.onChanged 局部更新
  可选（仅本 Widget 沙箱）：Tapp.widget.invalidate("data-ready")
```

> **`Tapp.widget` 在不同沙箱含义不同**
>
> | 沙箱 | `Tapp.widget` 上有什么 |
> | ---- | ---------------------- |
> | **Page** | `register` / `unregister` / `listRegistered` / `updateConfig`（需 `widget:register`） |
> | **Widget** | `getInstanceSettings` / `updateInstanceSettings` / **`invalidate`** |
> | **headless** | **无** `Tapp.widget`（整对象删除） |
>
> 共用 core 代码里若写 `await Tapp.widget.invalidate(...)`，在 Page/headless 会抛
> `TypeError`，常见后果是 `storage.set` 已成功但整段同步 Promise 失败、Page UI 卡在
> 「无法更新」。正确做法：core 只 `storage.set`；invalidate 仅放在 Widget `render`
> 或 Widget 专属逻辑里，且先判断 `typeof Tapp.widget?.invalidate === "function"`。

1. **声明 API**（`manifest.json`）

```json
{
  "permissions": ["network:fetch", "storage:read", "storage:write", "scheduler:register"],
  "apis": {
    "stats": {
      "type": "http",
      "endpoint": "https://api.example.com/stats",
      "method": "GET",
      "access": "protected",
      "description": "拉取统计摘要"
    }
  },
  "backgroundRequirements": ["scheduler", "sync"],
  "widgets": [
    {
      "id": "stats",
      "name": "统计",
      "defaultSize": "2x2",
      "sizes": ["2x2"],
      "refreshPolicy": {
        "mode": "event",
        "refreshOnVisible": true
      }
    }
  ]
}
```

> 存储读取使用 **`storage:read`**，写入、删除与清空使用 **`storage:write`**。旧 `storage`
> token 会在安装时被拒绝。

2. **core 拉数并写入 storage**（可在 headless 中被 scheduler 周期性执行）

```javascript
// CORE_CODE — Page / Widget / headless 都会加载
// 只写 storage；宿主会广播给可见 Widget。不要在这里 invalidate。
async function refreshStats() {
  const data = await Tapp.api("stats", {});
  await Tapp.storage.set("stats.summary", {
    total: data.total,
    updatedAt: Date.now(),
  });
}

Tapp.lifecycle.onReady(async () => {
  // 前台打开时立刻拉一次
  await refreshStats();

  // 需要离开 UI 后仍同步时：manifest 已声明 backgroundRequirements，
  // 再用 scheduler + onTask（不要 setInterval 冒充后台）
  if (Tapp.scheduler) {
    await Tapp.scheduler.register({
      taskId: "stats-sync",
      name: "统计同步",
      scheduleType: "interval",
      schedule: { interval: 5 * 60 * 1000 },
      executionTarget: "frontend",
    });
    Tapp.scheduler.onTask("stats-sync", async () => {
      await refreshStats();
    });
  }
});
```

3. **Widget 只读 storage 渲染**；宿主在 storage 变更时默认会 re-render

```javascript
// WIDGET_CODE
Tapp.widgets["stats"] = {
  render: async function (container, props) {
    const summary = (await Tapp.storage.get("stats.summary")) || { total: "—" };
    container.innerHTML = `
      <div class="stats-widget glass rounded-xl p-3">
        <div class="stats-widget-value">${summary.total}</div>
        <div class="text-xs opacity-60">统计</div>
      </div>
    `;
  },
};
```

### 四种触发更新的方式

| 方式 | 谁发起 | 宿主行为 | 适用场景 |
| ---- | ------ | -------- | -------- |
| **`Tapp.storage.set/remove/clear`** | Page / Widget / headless | 同 Tapp 广播；`refreshPolicy.mode` 默认 `event` 时刷新可见 Widget | **首选**：后台同步、Page 改配置、core 写缓存 |
| **`Tapp.widget.invalidate(reason)`** | **仅当前 Widget 沙箱**（Page/headless 无此方法） | 请求宿主对该实例 re-render（与 storage 刷新一样会去抖） | 内存态算完、未写 storage、或只想刷自己 |
| **`refreshPolicy.mode: "interval"`** | 宿主计时器 | 仅在**页面与 Widget 均可见**且 Tapp 运行时按 `intervalSeconds`（15–86400）重渲染 | 可见轮询；**不要**用它做后台同步 |
| **`refreshOnVisible: true`**（默认） | 宿主 | Widget 重新进入可见区域时刷新 | 切回 Dashboard 时补最新 storage |

```javascript
// ✅ Widget 沙箱内：数据已在本实例算完，显式请求刷新
if (typeof Tapp.widget?.invalidate === "function") {
  await Tapp.widget.invalidate("data-ready");
}

// ✅ 任意沙箱：订阅 storage 做局部更新（不依赖整卡 re-render）
const unsubscribe = Tapp.storage.onChanged(({ key, operation }) => {
  if (key === "stats.summary" || operation === "clear") {
    // 自行 patch DOM；一般不必 invalidate
  }
});
// 在 onDestroy / 下次 render 前调用 unsubscribe()

// ❌ core / Page / headless — 会 TypeError，且可能中断 await 链
// await Tapp.widget.invalidate("metrics-synced");
```

### 实践约定

- **拉网关 / 外部 HTTP**：只用 `Tapp.api`；名称必须与 `manifest.apis` 一致。`Tapp.api.list()`
  可查看当前已解析声明。
- **跨 Page / Widget / headless 共享状态**：写 `Tapp.storage`，不要假设全局变量在各 iframe
  之间存活。
- **后台周期同步**：`backgroundRequirements` + headless core + `Tapp.scheduler`；不要依赖
  Widget 的 interval 计时器（不可见时不计时）。
- **render 保持幂等**：宿主可能因 storage 批次写入、`invalidate`、可见性多次调用 `render`；
  避免在 render 里无节制打外部 API。缓存进 storage，render 读缓存。
- **刷新去抖**：宿主对 storage 变更与 `invalidate` 统一去抖，避免同一批写入连刷多次。
- **平台数据**：`Tapp.platform.*` 是只读平台统计等，与你在 `apis` 里声明的业务 API 无关。

更多字段见 [Manifest `refreshPolicy`](MANIFEST.md)、[Storage / Widget / 声明 API](API_REFERENCE.md)、
[后台 core](ARCHITECTURE.md#后台-core)。快速入门中的浓缩版见
[QUICKSTART — 数据加载与更新](QUICKSTART.md#数据加载与更新)。

---

## 完整示例：数据统计小组件

```javascript
Tapp.widgets["stats"] = {
  render: async function (container, props) {
    const scale = props.scale || 1;
    const fontScale = props.fontScale || 1;
    const themeColor = props.primaryColor || "#8b5cf6";

    // 光晕效果
    const glow = `
      <div class="absolute -right-8 -top-8 w-32 h-32 rounded-full blur-3xl opacity-10"
           style="background: ${themeColor}"></div>
    `;

    // 渐变背景
    const gradient = `
      <div class="absolute inset-0 bg-gradient-to-br from-gray-50/50 to-transparent 
                  dark:from-white/[0.02] dark:to-transparent"></div>
    `;

    try {
      const stats = await Tapp.platform.getStats("steam");

      container.innerHTML = `
        <div class="relative h-full w-full rounded-xl overflow-hidden glass">
          ${glow}
          ${gradient}
          
          <div class="relative h-full flex flex-col p-3" style="padding: ${
            12 * scale
          }px;">
            <!-- 标题 -->
            <div class="flex items-start justify-between mb-2">
              <h3 class="text-xs text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold"
                  style="font-size: ${12 * fontScale}px;">
                我的游戏库
              </h3>
            </div>
            
            <!-- 主数据 -->
            <div class="flex-1 flex items-center">
              <div class="flex items-baseline gap-1">
                <span class="text-3xl font-black text-gray-800 dark:text-gray-100 leading-none"
                      style="font-size: ${30 * fontScale}px;">
                  ${stats.total}
                </span>
                <span class="text-xs text-gray-500 dark:text-gray-400 font-bold">
                  GAMES
                </span>
              </div>
            </div>
            
            <!-- 分类统计 -->
            <div class="grid grid-cols-3 gap-1.5" style="gap: ${6 * scale}px;">
              ${this.renderStatItem(
                "🎮",
                stats.played,
                "已玩",
                scale,
                fontScale,
              )}
              ${this.renderStatItem(
                "📦",
                stats.unplayed,
                "未玩",
                scale,
                fontScale,
              )}
              ${this.renderStatItem(
                "⭐",
                stats.favorite,
                "收藏",
                scale,
                fontScale,
              )}
            </div>
          </div>
          
          ${
            props.isEditMode
              ? `
            <div class="absolute inset-0 border-2 border-dashed border-blue-400 rounded-xl pointer-events-none"></div>
          `
              : ""
          }
        </div>
      `;
    } catch (error) {
      container.innerHTML = `
        <div class="relative h-full w-full rounded-xl overflow-hidden glass">
          ${glow}
          <div class="absolute inset-0 flex flex-col items-center justify-center">
            <span class="text-3xl mb-2">😢</span>
            <span class="text-gray-500 dark:text-gray-400 text-sm">加载失败</span>
          </div>
        </div>
      `;
    }
  },

  renderStatItem(icon, value, label, scale, fontScale) {
    return `
      <div class="flex flex-col items-center justify-center 
                  bg-white/60 dark:bg-white/[0.03] backdrop-blur-sm rounded-md"
           style="padding: ${6 * scale}px;">
        <div class="text-gray-700 dark:text-white/60">${icon}</div>
        <span class="text-base font-black text-gray-800 dark:text-gray-200"
              style="font-size: ${16 * fontScale}px;">${value}</span>
        <span class="text-[7px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold"
              style="font-size: ${7 * fontScale}px;">${label}</span>
      </div>
    `;
  },
};
```

---

## 注册 Widget

在 PAGE_CODE 中注册：

```javascript
Tapp.lifecycle.onReady(async () => {
  await Tapp.widget.register({
    id: "stats",
    name: "数据统计",
    defaultSize: "2x2",
    sizes: ["1x1", "2x2", "4x2"],
  });
});
```

或在 Manifest 中预注册：

```json
{
  "widgets": [
    {
      "id": "stats",
      "name": "数据统计",
      "defaultSize": "2x2",
      "sizes": ["1x1", "2x2", "4x2"]
    }
  ]
}
```

两种注册不是同一生命周期：Manifest Widget 在安装/更新时由后端完整对账，删除声明会删除
注册；`Tapp.widget.register()` 是当前 sandbox 的动态注册，调用必须携带宿主管理的 Runtime
Grant，且不能覆盖或注销 Manifest Widget。静态 Widget 优先写入 Manifest。

---

## 注意事项

### 1. 容器规范

- **不要修改** `container` 的 `position`、`width`、`height`
- 使用 `relative h-full w-full` 或 `absolute inset-0` 填满容器
- 必须添加 `rounded-xl overflow-hidden` 确保圆角正确

### 2. 性能优化

- 一次性构建 HTML，避免频繁 DOM 操作
- 使用 CSS transition 而非 JavaScript 动画
- 缓存颜色、尺寸等计算结果
- 预览模式下简化渲染逻辑

### 3. 主题兼容

- 必须支持亮色/暗色主题
- 使用 `props.theme` 判断当前主题
- 使用 Tailwind 的 `dark:` 前缀处理暗色模式

### 4. 缩放适配

- 所有尺寸值必须乘以 `scale`
- 所有字体大小必须乘以 `fontScale`
- 使用 `style` 属性动态设置尺寸

### 5. 可访问性

- 按钮添加 `title` 或 `aria-label` 属性
- 图标配合文字说明
- 保持足够的颜色对比度
