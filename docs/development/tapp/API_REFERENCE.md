# Tapp API 参考

本文档详细说明通用稳定 API，并在文末列出完整版与 Widget SDK 的能力边界。SDK 方法
只有在当前沙箱注册了对应 handler 且权限已授予时才可用；不能仅以 `window.Tapp` 上存在
某个方法判断能力可调用。

## 目录

- [存储 API](#存储-api)
- [国际化 API](#国际化-api)
- [跨 Tapp Data Exchange API](#跨-tapp-data-exchange-api)
- [设置 API](#设置-api)
- [UI API](#ui-api)
- [动画 API](#动画-api)
- [平台 API](#平台-api)
- [AI API](#ai-api)
- [Agent Interaction API](#agent-interaction-api)
- [小组件 API](#小组件-api)
- [报告 API](#报告-api)
- [DOM 安全 API](#dom-安全-api)
- [数据处理 API](#数据处理-api)
- [媒体控制 API](#媒体控制-api)
- [上下文 API](#上下文-api)
- [用户角色 API](#用户角色-api)
- [组件注册 API](#组件注册-api)
- [快捷键 API](#快捷键-api)
- [事件 API](#事件-api)
- [后台需求 API](#后台需求-api)
- [动态内容 API](#动态内容-api)
- [定时任务 API](#定时任务-api)
- [声明式网络 API](#声明式网络-api)
- [文件与语音 API](#文件与语音-api)
- [包内资源 Assets API](#包内资源-assets-api)
- [能力边界与完整命名空间](#能力边界与完整命名空间)

---

## 国际化 API

**权限**：无（Basic 读取能力）

安装资源中的 `i18n/<locale>.json` 会以只读数据注入沙箱。Page、Widget 和 headless core
统一通过 `Tapp.i18n` 读取，不要自行猜测 `Tapp.i18n` 以外的接口，也不要直接依赖
`window._TAPP_I18N` 内部变量。

```javascript
const title = Tapp.i18n.t("title");
const progress = Tapp.i18n.t("progress", { done: 3, total: 5 });
const locale = Tapp.i18n.getLocale();
const allTranslations = Tapp.i18n.getAll(); // 返回只读数据的深拷贝
```

`t()` 先匹配语言表中的完整 key，因此 `{"app.title": "..."}` 可直接使用；未命中时再把
点号作为嵌套路径读取。locale 按当前完整 locale、语言前缀、`en-US`、`zh-CN` 的顺序
回退；缺失时返回 key，避免把 `undefined` 写进 DOM。语言切换后 `getLocale()` 和 `t()`
立即使用新 locale，已有 DOM 文本仍应在 `Tapp.ui.onLocaleChange()` 回调中重新渲染。

---

## 存储 API

**权限**: `storage`

该能力仅向已登录用户的 Runtime Grant 签发。访客运行公开 Tapp 时没有持久 storage。

```javascript
// 获取数据
const value = await Tapp.storage.get("key");

// 设置数据
await Tapp.storage.set("key", { any: "value" });

// 删除数据
await Tapp.storage.remove("key");

// 获取所有键
const keys = await Tapp.storage.keys();

// 一次获取所有键值（不要自行 keys() 后逐项 get）
const entries = await Tapp.storage.getAll();
// 返回: { key: value, ... }

// 清空存储
await Tapp.storage.clear();

// 获取存储使用情况
const usage = await Tapp.storage.usage();
// 返回: { used: 1024, quota: 5242880 } // 字节；quota 是服务端硬上限

// 同一 Tapp 的其他 Page、Widget 或 headless core 修改 storage 时触发
const unsubscribe = Tapp.storage.onChanged(({ key, operation }) => {
  console.log(key, operation); // operation: set | remove | clear
});
```

存储 key 和单值由后端校验，单值最大 1 MiB，总量最大 5 MiB。替换投影与写入位于同一数据库
事务并使用 subject/Tapp advisory lock，因此并发写入也不能越过配额。公开 Tapp 仍按当前
登录用户隔离存储；`_settings.`、`_component:`、`_shortcut:`、`_report:` 为宿主保留前缀。

---

## 跨 Tapp Data Exchange API

该 API 不使用可长期授予的静态权限。安全边界由双方 Manifest 声明、双方 Runtime Grant、
同一当前用户、provider 安装 owner、宿主可见授权弹窗和服务端原子消费的一次性 Data Access
Grant 共同组成。

提供方通常在 `core` 注册 handler；需要离开页面后仍可提供数据时，还应声明真实的
`backgroundRequirements`，让 headless core 保持在线：

```javascript
const removeProvider = await Tapp.dataExchange.provide(
  "playlist.current",
  async (params, context) => {
    console.log("本次用途", context.purpose);
    return getCurrentPlaylist(params);
  },
);

removeProvider(); // 不再提供时调用
```

调用方必须在 Manifest 的 `imports` 中声明目标与 export：

```javascript
const playlist = await Tapp.dataExchange.request({
  targetTappId: "com.example.player",
  exportId: "playlist.current",
  params: { fields: ["title", "artist"] },
  purpose: "把当前播放列表加入周报",
});
```

每次 `request()` 都会进入宿主授权队列，并显示一张结构化弹窗，列出调用方、提供方、数据
名称、请求参数/字段范围、用途、返回上限和自动过期倒计时。弹窗默认聚焦“拒绝”，Escape、
遮罩和关闭按钮都视为拒绝；多个请求按顺序逐项确认。首版不提供“始终允许”。用户拒绝、
弹窗关闭、调用方销毁、提供方离线、30 秒未响应、响应超限或 schema 不匹配都会拒绝调用；
不会返回部分结果。Runtime Grant 与一次性 token 都只存在于宿主，不会进入 iframe 或
`postMessage`。当前仅从已在线并注册 handler 的 Page、Widget 或 headless runtime 中选择
提供方，不会为没有后台实例的 Tapp 隐式启动完整 Page。

---

## 设置 API

**权限**: `storage`

访客不会获得 `storage`，因此只使用 Manifest 中的默认设置；以下读写 API 仅在已登录
用户的 Tapp runtime 中可用。

```javascript
// 获取设置项
const refreshInterval = await Tapp.settings.get("refreshInterval");

// 设置设置项
await Tapp.settings.set("refreshInterval", 60);

// 获取所有设置
const allSettings = await Tapp.settings.getAll();
// 返回: { refreshInterval: 60, showDetails: true, ... }
```

设置是 Manifest 声明的安装级配置。安装 owner 或管理员可修改；运行该安装的其他已登录用户
只读。`getAll()` 使用独立 settings 端点一次读取全部已保存值，不会枚举当前用户的私有
`Tapp.storage`。

---

## UI API

**权限**: `ui:notification`, `ui:theme`, `ui:confirm`, `ui:fullscreen`

### 基础 UI

```javascript
// 设置页面标题
await Tapp.ui.setTitle("我的页面");

// 显示通知（Toast）
await Tapp.ui.showNotification({
  title: "操作成功", // 可选：通知标题
  message: "数据已保存", // 必填：通知消息
  type: "success", // 可选：success | error | warning | info
  duration: 3000, // 可选：显示时长（毫秒）
});

// 确认对话框
const confirmed = await Tapp.ui.confirm("确定要执行吗？");
// 返回: true（确定）或 false（取消）
```

### 主题

```javascript
// 获取当前主题
const theme = await Tapp.ui.getTheme();
// 返回: 'light' | 'dark'

// 监听主题变化
const unsubscribe = Tapp.ui.onThemeChange((theme) => {
  console.log("主题切换为:", theme);
});

// 获取全局主色调（壁纸色）
const primaryColor = await Tapp.ui.getPrimaryColor();
// 返回: '#6366f1' (十六进制颜色值)

// 监听主色调变化
Tapp.ui.onPrimaryColorChange((color) => {
  console.log("主色调变化:", color);
});
```

### 语言

```javascript
// 获取当前语言
const locale = await Tapp.ui.getLocale();
// 返回: 'zh-CN' | 'en-US' | ...

// 监听语言变化
Tapp.ui.onLocaleChange((locale) => {
  console.log("语言切换为:", locale);
});
```

### 全屏

```javascript
// 请求全屏
await Tapp.ui.fullscreen.request();

// 退出全屏
await Tapp.ui.fullscreen.exit();

// 切换全屏
await Tapp.ui.fullscreen.toggle();

// 查询状态
const isFs = await Tapp.ui.fullscreen.isFullscreen();
```

---

## 动画 API

**权限**: 无需特殊权限

获取系统动画配置，根据用户偏好调整 UI 行为。

```javascript
// 获取当前动画级别
const level = await Tapp.animation.getLevel();
// 返回: 'none' | 'light' | 'standard'

// 检查是否应该显示动画
const shouldAnimate = await Tapp.animation.shouldAnimate();
// 返回: boolean

// 获取完整动画配置
const config = await Tapp.animation.getConfig();
// 返回: {
//   level: 'standard',
//   loop: true,
//   spring: { tension: 280, friction: 20 },
//   durationScale: 1
// }

// 获取推荐的交错延迟（用于列表动画）
const delay = await Tapp.animation.getStaggerDelay(index, baseDelay);
// index: 元素索引
// baseDelay: 基础延迟（毫秒），默认 50ms

// 监听动画级别变化
Tapp.animation.onLevelChange((level) => {
  console.log("动画级别变化:", level);
});
```

### 使用示例

```javascript
async function animateListItems(items) {
  const shouldAnimate = await Tapp.animation.shouldAnimate();
  const config = await Tapp.animation.getConfig();

  for (let i = 0; i < items.length; i++) {
    const delay = await Tapp.animation.getStaggerDelay(i);

    if (shouldAnimate) {
      setTimeout(() => {
        items[i].style.transition = `all ${200 * config.durationScale}ms`;
        items[i].classList.add("visible");
      }, delay);
    } else {
      items[i].classList.add("visible");
    }
  }
}
```

---

## 平台 API

**权限**: `platform:read`, `platform:write`

平台数据端点要求持久登录主体，匿名访客不会获得 `platform:read` Runtime Grant。

```javascript
// 获取已启用平台列表
const platforms = await Tapp.platform.listEnabled();
// 返回: [{ id: 'steam', name: 'Steam', enabled: true, ... }]

// 获取平台数据
const data = await Tapp.platform.getData("steam", {
  limit: 100,
  offset: 0,
});

// 获取平台统计
const stats = await Tapp.platform.getStats("steam");
// 返回: { total: 100, distribution: {...} }

// 获取数据分布
const dist = await Tapp.platform.getDistribution("steam", "genre");

// 添加数据条目（需要 platform:write 权限）
const result = await Tapp.platform.addItem({
  platform: "custom",
  type: "game",
  title: "我的游戏",
  description: "描述",
  metadata: { rating: 5 },
});

// 批量添加数据条目
await Tapp.platform.addItems([
  { platform: "custom", title: "游戏1" },
  { platform: "custom", title: "游戏2" },
]);
```

---

## AI API

**权限**: `ai:generate`, `ai:analyze`, `ai:chat`, `ai:image`

AI 只提供服务端治理的 Task API。Manifest 必须通过 `ai` 声明 operation、model tier、context
source 与 output format，并同时申请 operation 对应的 `ai:*` 权限。

```javascript
let task = await Tapp.ai.tasks.create({
  version: 2,
  operation: "generate",
  input: { prompt: "生成一段摘要" },
  context: [{ type: "report", reportId: 42 }],
  output: { format: "json", schema: { type: "object" } },
  delivery: "stream",
  idempotencyKey: "summary-42-v1",
});

const stop = await Tapp.ai.tasks.subscribe(task.taskId, ({ event, data }) => {
  if (event === "delta") renderDelta(data.text);
  if (event === "result") renderResult(data.result);
});

task = await Tapp.ai.tasks.get(task.taskId);
await Tapp.ai.tasks.cancel(task.taskId); // 仅非终态任务
const usage = await Tapp.ai.tasks.usage();
stop();
```

任务绑定当前 Tapp/subject/owner，最多并发 4 个，125 秒执行上限，终态保留 15 分钟。
并发/保留上限和 `idempotencyKey` 在跨副本事务中原子判定；同一身份重复提交相同请求只返回
原任务，不会重复调用模型或重复计费。
`platform`、`report` 上下文还需对应读取权限；跨 Tapp 上下文不能由 AI 接口静默获取，必须
先走 One-shot Data Exchange。结构化 JSON 输出会在后端解析并验证 inline schema。

---

## Agent Interaction API

```javascript
const off = Tapp.agent.onInteraction("report.compose", async (interaction) => {
  await interaction.accept();
  const report = await buildReport(interaction.input);
  await interaction.submitResult({ data: report, summary: "报告已生成" });
});
```

只有 `accept()` 成功的 runtime 能提交结果；输入/结果按 Manifest schema 校验，5 分钟到期，
结果提交默认使用基于 interactionId 的幂等键，提交后恢复原 Agent task。`requestIntent()`
经后端授权后由 `ui.open`、`report.create` 或 `dataExchange.request` 宿主 adapter 执行；跨
Tapp 数据读取只显示 Data Exchange 自己的一张明细化一次性授权弹窗。
到期不是简单删除：共享过期 worker 会以 CAS 转为 `expired`，并让原 Executor 从持久化任务
恢复，避免任务永久停在等待状态。

---

## 小组件 API

**权限**: `widget:register`（privileged，仅当前管理员）

```javascript
// 注册小组件
await Tapp.widget.register({
  id: "my-widget",
  name: "我的小组件",
  defaultSize: "2x2",
  sizes: ["1x1", "2x2", "4x2"],
  category: "utility",
});

// 注销小组件
await Tapp.widget.unregister("my-widget");

// 获取已注册小组件
const widgets = await Tapp.widget.listRegistered();

// 替换小组件注册元数据（不会保存用户设置）
await Tapp.widget.updateConfig("my-widget", {
  name: "我的小组件",
  description: "更新后的说明",
  icon: "🧊",
  defaultSize: "2x2",
  sizes: ["1x1", "2x2", "4x2"],
  category: "utility",
});
```

在 Widget 沙箱内还提供当前 Dashboard 实例专用 API（无需 `widget:register`）：

```javascript
const settings = Tapp.widget.getInstanceSettings();
await Tapp.widget.updateInstanceSettings({ compact: true });
await Tapp.widget.invalidate("data-ready");
```

`updateInstanceSettings()` 只能更新当前 Widget 的 `widgets[].settings` 已声明字段，宿主会
按类型、select 选项和数值范围校验，然后写入 Dashboard 布局。顶层 `settings` 仍是整个
Tapp 共享的全局设置。可见刷新采用 `widgets[].refreshPolicy`；后台周期任务继续使用
scheduler/headless core，不依赖 Widget iframe 常驻。

---

## 报告 API

**权限**: `report:read`, `report:write`

```javascript
// 获取报告列表
const reports = await Tapp.report.list();
// 或: await Tapp.report.listReports();

// 获取报告详情
const report = await Tapp.report.get(reportId);
// 或: await Tapp.report.getReport(reportId);

// 获取特定平台的报告
const steamReport = await Tapp.report.getPlatformReport("steam");

// 创建报告（需要 report:write）
const newReport = await Tapp.report.create(
  "我的报告", // title
  "summary", // reportType
  { summary: "..." }, // content
  { tags: ["test"] }, // metadata (可选)
);

// 更新报告
await Tapp.report.update(reportId, "新标题", { summary: "新内容" });

// 删除报告
await Tapp.report.delete(reportId);
```

---

## DOM 安全 API

**无需权限** - 防止 XSS 攻击的安全工具

```javascript
// HTML 转义
const safe = Tapp.dom.escapeHtml('<script>alert("xss")</script>');
// 返回: '&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;'

// 安全设置文本内容
Tapp.dom.setText(element, userInput);

// 安全设置 HTML（自动转义）
Tapp.dom.setSafeHtml(element, userInput);

// 创建文本节点
const textNode = Tapp.dom.createTextNode(userInput);

// 安全设置属性
Tapp.dom.setAttribute(element, "href", url);

// 创建安全元素
const div = Tapp.dom.createElement("div", {
  text: "安全文本",
  className: "my-class",
  attributes: { "data-id": "123" },
});

// 安全渲染列表
Tapp.dom.renderList(container, items, (item, index) => {
  return Tapp.dom.createElement("div", {
    text: item.name,
    className: "item",
  });
});
```

> ⚠️ **安全警告**：永远不要直接使用 `innerHTML` 渲染用户输入！

---

## 数据处理 API

权限按数据流动态计算：inline 输入且无输出不需要静态权限；platform 输入需要
`platform:read`，platform 输出需要 `platform:write`，storage 输入/输出需要 `storage`。
纯 inline 输入与返回值不需要额外权限，访客也可使用；一旦请求 platform 或 storage，
服务端仍按当前 Runtime Grant 拒绝未获授权的访问。
后端同时校验 Runtime Grant 与安装授权。

```javascript
const result = await Tapp.data.transform({
  input: { source: "platform", platform: "steam" },
  pipeline: [
    { type: "filter", field: "status", operator: "eq", value: "active" },
    { type: "sort", field: "createdAt", order: "desc" },
    { type: "limit", count: 10 },
    { type: "select", fields: ["id", "title", "date"] },
    {
      type: "map",
      operations: [
        { op: "rename", from: "title", to: "name" },
        { op: "default", field: "date", value: null },
      ],
    },
  ],
  output: { target: "storage", key: "my-data" },
});
```

### 输入源类型

| 类型       | 参数               | 说明         |
| ---------- | ------------------ | ------------ |
| `platform` | `platform: string` | 从平台读取   |
| `storage`  | `key: string`      | 从存储读取   |
| `inline`   | `data: unknown`    | 直接传入数据 |

### 管道操作

| 操作        | 参数                         | 说明                         |
| ----------- | ---------------------------- | ---------------------------- |
| `filter`    | `field`, `operator`, `value` | 过滤数据                     |
| `sort`      | `field`, `order`             | 排序                         |
| `limit`     | `count`                      | 限制数量                     |
| `offset`    | `count`                      | 跳过数量                     |
| `select`    | `fields`                     | 选择字段                     |
| `group`     | `by`                         | 分组                         |
| `aggregate` | `operation`, `field`         | 聚合统计                     |
| `dedupe`    | `key`                        | 去重                         |
| `map`       | `operations`                 | 声明式字段映射；不执行表达式 |

---

## 媒体控制 API

**权限**: `media:control`, `media:read`

```javascript
// 播放控制（需要 media:control）
await Tapp.media.play();
await Tapp.media.pause();
await Tapp.media.next();
await Tapp.media.prev();
await Tapp.media.seek(120); // 秒

// 音量控制
await Tapp.media.setVolume(0.8); // 0-1
await Tapp.media.mute();
await Tapp.media.unmute();

// 播放模式
await Tapp.media.setMode("repeat"); // repeat | shuffle | normal

// 播放指定曲目
await Tapp.media.playTrack(trackId, trackIndex);

// 获取播放状态（需要 media:read）
const status = await Tapp.media.getStatus();
// 返回: { isPlaying, currentTrack, position, volume, mode, muted }

// 获取播放列表
const playlist = await Tapp.media.getPlaylist();

// 监听状态变化
const unsubscribe = Tapp.media.onStateChange((state) => {
  console.log("播放状态:", state.isPlaying);
});

// 监听轻量播放进度；进度 tick 不重复发送完整状态
const stopProgress = Tapp.media.onProgress((progress) => {
  console.log(progress.current, progress.duration, progress.percentage);
});

// 实时频谱分析（需要 media:read）
// 返回归一化 0-1 的频段数据，播放任意音乐时均可用（无需首页频谱组件在场）
const s = await Tapp.media.getSpectrum();
// 返回: {
//   spectrum: number[4],  // 为 4 柱视觉重排的数据（低-高-高-低对称，适合简单柱状）
//   bands: number[8],     // 原始 8 频段（bass→high 自然顺序，适合频谱可视化）
//   energy, bass, mid, high
// }
// 建议架构：~15fps 轮询数据 + 本地攻击/释放包络 60fps 渲染（postMessage 有成本）

// 获取歌词：逐字 + 逐行兜底（需要 media:read）
// 多源逐字：网易云 yrc（按 id）→ 酷狗 KRC（按 歌名+歌手+时长）→ 逐行
// 不传参数默认取当前播放曲目；也可指定 { songId, source }
const ly = await Tapp.media.getLyrics();
// 返回: {
//   lines:    [{ time, text, translation? }],           // 逐行 LRC（始终尝试提供）
//   verbatim: [{ time, duration, text, translation?,
//               words: [{ time, duration, text }] }],   // 逐字（word.time 为绝对秒）
//   hasVerbatim: boolean,                               // 是否含逐字数据
//   source: "netease" | "qq",                           // 曲目来源
//   verbatimSource: "netease" | "kugou" | "",           // 逐字实际命中源
//   hasTranslation: boolean,                            // 是否含逐行翻译
//   translationLang: "zh" | ""                          // 翻译语言（当前仅网易中文翻译源）
// }
// verbatim 为空时消费方应回退到 lines 做逐行高亮
// 说明：网易云 yrc 覆盖较少，酷狗 KRC 覆盖最广（尤其日系/番剧），故作为回退源
// 翻译：整行级（无逐字翻译），已按时间就近对齐挂在各行 translation 字段；
//      建议仅当 translationLang 与用户界面语言一致时展示翻译开关

// 节拍网格：预载全曲离线分析（需要 media:read）
// 主应用对当前歌曲做一次性节拍跟踪（Ellis 2007 风格：谱通量 + 自相关 + comb 相位），
// 返回精确拍点时间戳——可视化可按网格预测踩拍，消除实时检测的固有滞后
const grid = await Tapp.media.getBeatGrid();
// 返回: { available: boolean, bpm, beats: number[]（秒）, confidence: 0-1 }
// available=false 或 confidence 低时应回退到实时频谱检测
// 注意：首次调用会触发全曲下载+分析（约 1-3s），结果按歌缓存

// VIP 歌曲开关（读 media:read / 写 media:control）
const { skipVip } = await Tapp.media.getSkipVip();
await Tapp.media.setSkipVip(true); // true=跳过VIP歌曲
```

---

## 上下文 API

**无需权限** - 获取应用上下文信息

```javascript
// 获取应用信息
const app = await Tapp.context.getApp();
// 返回: { version, name, environment }

// 获取用户信息
const user = await Tapp.context.getUser();
// 返回: { id, username, avatar, preferences }

// 获取播放器信息
const player = await Tapp.context.getPlayer();
// 返回: { isPlaying, currentTrack, volume }

// 获取导航信息
const nav = await Tapp.context.getNavigation();
// 返回: { currentPath, params }

// 获取系统信息
const system = await Tapp.context.getSystem();
// 返回: { theme, language, timezone }
```

---

## 用户角色 API

**无需权限** - 获取当前用户的角色信息

```javascript
// 获取当前用户角色
const role = await Tapp.user.getRole();
// 返回: "guest" | "user" | "admin"

// 检查是否为管理员
const isAdmin = await Tapp.user.isAdmin();

// 检查是否为游客
const isGuest = await Tapp.user.isGuest();

// 检查是否已登录
const isLoggedIn = await Tapp.user.isLoggedIn();

// 获取可用的权限等级
const levels = await Tapp.user.getAllowedPermissionLevels();
// admin -> ['public', 'basic', 'elevated', 'privileged']
// user / guest -> ['public', 'basic']，存在任一动态下放权限时还包含 'elevated'

// 检查是否可以使用指定权限等级
const canUse = await Tapp.user.canUsePermissionLevel("elevated");
```

这两个等级 API 读取后端当前的权限下放配置，表示该角色在系统层面是否可以使用此
等级，并不表示当前 Tapp 已取得该等级下的每项权限。实际调用前仍应以
`Tapp.permissions.includes("具体权限")` 为准，后端也会再次校验。
尤其对游客，`basic` 只表示该等级存在；要求持久登录主体的 basic 能力不会出现在
Runtime Grant 中。

---

## Federation Feed API

**权限**: `federation:read`

```javascript
const role = await Tapp.user.getRole();
const feed = await Tapp.federation.getFeed();

// 游客：feed.audience === "public"
// 普通用户/管理员：feed.audience === "public+personal"
// feed.items 中的 scope 为 "public" 或 "personal"
```

`getFeed()` 是面向 Tapp 的角色感知入口：游客只能读取公开 Activity，已登录用户读取
公开 Activity 与自己的 Federation Timeline。游客不会取得 `federation:write`、
`federation:message` 或 `federation:files`。Tapp 应使用 `Tapp.user.getRole()` 调整界面，
不要向游客展示关注、发布、私聊、Room 或文件传输操作。

`getTimeline()` 保留为已登录用户的原始个人 Timeline 接口；需要同时展示公开内容时应
优先使用 `getFeed()`。

---

## 组件注册 API

**权限**: `component:theme`, `component:agent`

```javascript
// 注册自定义主题
await Tapp.component.registerTheme({
  id: "my-theme",
  name: "我的主题",
  surface: "glass", // glass | solid | flat | outline
  glow: "primary", // identity | primary | none
});

// 注册 AI Agent
await Tapp.component.registerAgent({
  id: "my-agent",
  name: "我的助手",
  description: "一个自定义 AI 助手",
  capabilities: ["chat", "analyze"],
});

// 注销组件
await Tapp.component.unregister("theme", "my-theme");

// 列出已注册组件（theme | agent）
const themes = await Tapp.component.list("theme");
```

---

## 快捷键 API

**权限**: `shortcut:register`

```javascript
// 注册快捷键
await Tapp.shortcut.register({
  id: "my-shortcut",
  keys: "Ctrl+Shift+M",
  description: "打开我的 Tapp",
  action: "open-tapp",
  scope: "global", // global | tapp | editor
});

// 监听快捷键触发
Tapp.event.on("shortcut:triggered", (data) => {
  if (data.shortcutId === "my-shortcut") {
    console.log("快捷键已触发:", data.action);
  }
});

// 注销快捷键
await Tapp.shortcut.unregister("my-shortcut");

// 列出已注册快捷键
const shortcuts = await Tapp.shortcut.list();
```

---

## 事件 API

**权限**:
- `event:publish` — `Tapp.event.publish`（经宿主桥接到服务端 Broker）
- `event:subscribe` — 服务端 SSE 流 / Broker 订阅路径（`GET /api/tapp/events/stream` 等）；
  也用于 `Tapp.background.require` / `release`（见下方后台需求 API）

```javascript
// 本地 iframe 内监听：Tapp.event.on 只在沙箱内注册回调，不单独占用 bridge 权限条目。
// 订阅范围仍来自 Manifest events.subscribe；跨 runtime 投递走宿主/SSE 的 event:subscribe。
const unsubscribe = Tapp.event.on(
  "tapp.com.example.player.track.changed",
  (event) => console.log(event.payload),
);

await Tapp.event.publish({
  topic: "tapp.com.example.my-tapp.status.changed",
  scope: "owner",
  payload: { status: "changed", revision: 3 },
  dedupeKey: "status-revision-3",
});
unsubscribe();
```

Event Broker 是在线 at-most-once Broker：`instance` 只发给当前 runtime，`owner` 发给同一
subject 数据空间内、Manifest 明确订阅 topic 的在线 Page/Widget/headless。无 ACK、重试或
离线积压；慢消费者队列满时事件会丢弃。`owner` payload 最大 8 KiB 且仅允许浅层状态元数据，
`data/content/items/records/body/blob/bytes` 等正文键会被拒绝；跨 Tapp 数据必须走一次性授权。
`system.*` 只能由宿主发布。当前可订阅 `system.theme.changed`、`system.network.changed`、
`system.locale.changed`、`system.visibility.changed` 与 `system.navigation.changed`。

游客 subject 由 HttpOnly HMAC 签名的浏览器 guest session 派生，不共享出口 IP；当前权限策略
仍只允许游客发布 `instance` scope，`owner` 返回 `GUEST_OWNER_EVENT_UNAVAILABLE`。

---

## 后台需求 API

**权限**: `event:subscribe` — `require` / `release` 经 bridge 映射到该权限（见
`permissionConfig.ts`）。`list` / `has` 为本地查询，不额外申请权限。

```javascript
// 声明后台运行需求（需 event:subscribe）
await Tapp.background.require("sync", "每5分钟同步数据");

// 释放后台运行需求（需 event:subscribe）
await Tapp.background.release("sync");

// 获取当前所有后台需求
const requirements = await Tapp.background.list();
// 返回: ['scheduler', 'sync']

// 检查是否有特定后台需求
const hasSync = await Tapp.background.has("sync");
```

### 需求类型

| 类型             | 说明             |
| ---------------- | ---------------- |
| `media`          | 媒体控制功能     |
| `sync`           | 后台数据同步     |
| `notification`   | 定时通知功能     |
| `scheduler`      | 定时任务执行     |
| `event-listener` | 跨 Tapp 事件监听 |
| `realtime`       | 实时数据更新     |

---

## 动态内容 API

**权限**: `ui:notification`

在控制岛显示动态内容（如歌词、天气、统计等）。

```javascript
// 设置动态内容
await Tapp.dynamicContent.set({
  icon: "📊",
  text: "今日活跃: 128",
  subtext: "较昨日 +15%",
  priority: 10,
  expiresAt: Date.now() + 3600000, // 1小时后过期
  i18n: {
    text: {
      "zh-CN": "今日活跃: 128",
      "en-US": "Active today: 128",
    },
  },
});

// 快速更新文本
await Tapp.dynamicContent.update({
  text: "今日活跃: 156",
  subtext: "较昨日 +22%",
});

// 获取当前动态内容
const content = await Tapp.dynamicContent.get();

// 移除动态内容
await Tapp.dynamicContent.remove();
```

---

## 定时任务 API

**权限**: `scheduler:register`

```javascript
// 注册定时任务
await Tapp.scheduler.register({
  taskId: "daily-sync",
  name: "每日数据同步",
  scheduleType: "daily", // cron | interval | once | daily
  schedule: { time: "09:00" },
  executionTarget: "backend", // backend | frontend | both
  backendActions: [{ type: "platform.sync", platform: "steam" }],
  missedPolicy: "run-once", // skip | run-once | run-all
});

// 注册间隔任务
await Tapp.scheduler.register({
  taskId: "refresh",
  name: "刷新数据",
  scheduleType: "interval",
  schedule: { interval: 5 * 60 * 1000 }, // 5分钟
  executionTarget: "frontend",
});

// 注销任务
await Tapp.scheduler.unregister("daily-sync");

// 获取任务列表
const tasks = await Tapp.scheduler.list();

// 获取单个任务
const task = await Tapp.scheduler.get("daily-sync");

// 启用/禁用任务
await Tapp.scheduler.enable("daily-sync");
await Tapp.scheduler.disable("daily-sync");

// 手动触发任务
await Tapp.scheduler.trigger("daily-sync");

// 监听任务执行（前端任务）
const unsubscribe = Tapp.scheduler.onTask("refresh", async (payload) => {
  await refreshData();
});
```

### 调度类型

| 类型       | 配置参数   | 示例                            |
| ---------- | ---------- | ------------------------------- |
| `cron`     | `cron`     | `'0 9 * * 1'` - 每周一上午 9 点 |
| `interval` | `interval` | `300000` - 每 5 分钟            |
| `once`     | `at`       | 时间戳（毫秒）                  |
| `daily`    | `time`     | `'09:00'` - 每天上午 9 点       |

### 后端操作类型

```javascript
// 平台数据同步
{ type: 'platform.sync', platform: 'steam' }

// 存储操作
{ type: 'storage.set', key: 'key', value: { data: 1 } }
{ type: 'storage.delete', key: 'key' }
{ type: 'storage.get', key: 'key' }

// AI 生成
{ type: 'ai.generate', prompt: '生成摘要' }

// HTTP 请求
{ type: 'fetch', url: '...', method: 'GET', headers: {...} }

// 队列通知
{ type: 'notification.queue', title: '提醒', message: '...' }

// 数据转换
{ type: 'transform', input: 'varName', extract: '$.data' }
```

`ai.generate` 后端操作要求当前 Runtime Grant、安装授权和 `manifest.ai` 同时包含
`ai:generate` / V2 `generate` / `text` output。注册时检查一次，每次延迟执行前再次检查；执行
进入与 `Tapp.ai.tasks` 相同的共享并发、速率、calls、tokens 和 cooldown 账本。Declared API 的
`ai:generate`、`ai:chat` builtin 也遵守相同规则。

`fetch` 与声明式 HTTP API 仅连接解析后全部为公网的目标，DNS 在客户端中钉扎且不自动跟随
重定向；Host/Connection 等路由或 hop-by-hop 头会被拒绝，响应体上限为 2 MiB。

---

## 声明式网络 API

沙箱禁止 `fetch`、XHR 和 WebSocket。外部 HTTP 或受控 builtin 能力必须先写入
`manifest.apis`，然后按名称调用：

```javascript
const result = await Tapp.api("profile", {
  id: "42",
  query: { include: "summary" },
});

// 查看 Manifest 当前声明且宿主可识别的 API
const declaredApis = await Tapp.api.list();
```

```json
{
  "permissions": ["network:fetch"],
  "apis": {
    "profile": {
      "type": "http",
      "endpoint": "https://api.example.com/users/{{params.id}}",
      "method": "GET",
      "access": "protected",
      "description": "读取用户资料"
    }
  }
}
```

- 所有 `type: "http"` API 都要求 `network:fetch`；`public`/`protected` 只控制调用者范围。
- 后端按当前用户与 Tapp owner 重新加载 Manifest，并执行模板参数、频率和出站安全校验。
- Tapp 不能传入任意 URL，也不能使用历史文档中的 `Tapp.http.request()`。
- 详细 Manifest 字段和 REST 链路见 [Manifest](MANIFEST.md#api-声明-apis) 与
  [REST API](REST_API.md#manifest-声明-api)。

## 文件与语音 API

文件下载由宿主创建 Blob 并触发下载，不依赖 iframe 的 download sandbox 权限：

```javascript
await Tapp.file.download("hello\n", "hello.txt", "text/plain;charset=utf-8");
```

语音能力需要对应权限：

```javascript
const voices = await Tapp.speech.getVoices();
const status = await Tapp.speech.getStatus();
const audio = await Tapp.speech.tts({ text: "你好" }); // speech:tts
const text = await Tapp.speech.asr({ audio }); // speech:asr
```

语音服务统一要求登录（涉及付费供应商调用），因此不会向匿名访客下放 `speech:*`。

## 包内资源 Assets API

**权限**: public（仅可读本安装 `manifest.assets` 声明路径）

用于游戏贴图、音频、wasm、关卡 JSON 等包内静态文件。不走 `Tapp.storage`。

```javascript
const paths = await Tapp.assets.list();

// 在沙箱内创建 blob URL（可赋给 Image / Audio）
const { url, mimeType, size } = await Tapp.assets.getUrl("assets/sprite.png");

// 需要二进制时
const { buffer, mimeType: mt } = await Tapp.assets.getArrayBuffer("assets/level.json");

Tapp.assets.revoke(url);
Tapp.assets.revokeAll(); // 也会在 onDestroy 时自动调用
```

后端入口：`GET /api/tapps/{tappId}/asset?path=assets/...`（返回 base64）。
约定与配额见 [图形与轻量游戏](GRAPHICS.md)。

播放包内音频还需申请 `media:audio`，以便 CSP `media-src` 允许 `blob:` / `data:`。

## 能力边界与完整命名空间

`generateFullSDK()` 用于 Page 和 headless core；`generateWidgetSDK()` 是缩小能力面的 Widget 版本。
完整版当前包含以下命名空间：

| 命名空间                                   | 主要能力                                            | 权限族                             |
| ------------------------------------------ | --------------------------------------------------- | ---------------------------------- |
| `storage`, `settings`                      | Tapp 私有键值存储与设置                             | `storage`                          |
| `dataExchange`                             | 逐次授权的跨 Tapp 具名数据交换                      | Manifest + one-shot consent        |
| `ui`, `animation`, `dynamicContent`, `dom` | 宿主 UI、主题、动画和安全 DOM helper                | `ui:*` 或 public                   |
| `platform`, `data`                         | 平台数据读取、写入、转换和注册                      | `platform:*`                       |
| `ai`, `report`                             | 服务端治理的 AI Task 与报告读写                     | `ai:*`, `report:*`                 |
| `widget`                                   | 管理员动态注册与配置 Widget                         | `widget:register`                  |
| `media`                                    | 播放器读取和控制                                    | `media:*`                          |
| `context`, `user`                          | 应用、用户、导航、系统和地理上下文                  | public                             |
| `component`, `shortcut`                    | 主题/Agent 组件和快捷键注册                         | `component:*`, `shortcut:register` |
| `event`, `background`, `scheduler`         | 在线 Event Broker、常驻需求和持久化任务             | `event:*`（含 background.require/release→`event:subscribe`）、`scheduler:register` |
| `agent`                                    | schema 约束的 Agent Interaction                     | Manifest + Runtime Grant           |
| `api`                                      | Manifest 声明的 HTTP/builtin 能力                   | HTTP 需 `network:fetch`；`access` 仅控制调用者范围 |
| `file`, `speech`                           | 文件下载、TTS 和 ASR                                | `storage`, `speech:*`              |
| `assets`                                   | 包内静态资源 list/get/blob URL                      | public（限 manifest.assets）       |
| `tappList`                                 | Tapp 查询、安装、启停、卸载与导出                   | `tappList:*`                       |
| `brewList`                                 | Brew 列表、订阅源、分类、评论和 OPML                | `brew:*`                           |
| `federation`                               | 身份、时间线、关注、Channel、Room、Ring、信任和传输 | `federation:*`                     |

Widget SDK 只保留 Widget 渲染需要的生命周期、UI/主题、用户角色、存储、AI Task、平台读取、报告
读取、媒体、背景需求、调度、声明式 API、上下文、DOM 和文件等子集。它不会自动拥有
完整版的写入/管理能力。新增或调用 API 时必须核对：

1. SDK 生成器是否暴露方法；
2. `permissionConfig.ts` 是否声明 action → 权限；
3. 当前 Page 或 Widget 宿主是否注册 handler；
4. 后端路由是否执行身份、owner 和权限复核。

`Tapp.context.getGeo()` 也是公开上下文方法；返回结果由后端地理信息服务决定。专业能力
的请求/响应结构以对应前端服务类型和后端路由结构为准，不能从方法名猜测参数。
