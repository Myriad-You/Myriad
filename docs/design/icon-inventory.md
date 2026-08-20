# Myriad 自有图标设计规范

本文档记录 Myriad 自有图标包的设计语言、资产规格和接入规则。它不是路线图；后续是否继续绘制新图标，按具体界面需求单独判断。

不纳入本图标包范围：

- Myriad 品牌 logo / favicon：`frontend/public/logo.webp`、`frontend/public/favicon.webp`
- 数据平台、社交平台、OAuth、AI provider、云服务商等第三方 logo
- 需要保持官方识别的外部品牌图标

## 设计语言

Myriad 自有图标采用接近 emoji 的大图标表达，但不直接复刻系统 emoji。目标是在首页、小组件、控制面板、Toast、Tapp 等高可见界面里形成统一、柔和、可亲近的视觉系统。

核心特征：

- **彩绘 3D 感**：使用柔和体积、圆润倒角、轻微材质高光和自阴影，避免纯扁平贴纸感。
- **语义优先**：图标表达 Myriad 自有功能语义，例如天气、状态、问候、Tapp 包，而不是表达底层组件库名。
- **简洁轮廓**：小尺寸下优先保证主体轮廓可读，少用细碎装饰。复杂物体宁可抽象成 1-2 个强识别元素。
- **无表情化**：除非语义确实需要，太阳、状态、物品都不加五官，避免显得幼稚或与品牌气质冲突。
- **单套适配深浅模式**：不为深色 / 浅色模式维护两套图标。单个 PNG 必须在两种模式下都具备足够明度、边缘和对比。
- **颜色不锁死**：不限定固定品牌色，但每个图标内部要有明确主色、辅色和高光层级，避免整套图标变成单一粉紫、蓝紫或其他单色系。

## 视觉规范

### 画布与尺寸

- 彩绘资源统一使用 `512x512` 透明 WebP。
- 主体建议占画布宽高约 `72%-86%`，避免贴边，也避免实际页面里显得过小。
- 四角必须透明，不能残留 chroma key、底色块或半透明脏边。
- 同一界面内的图标应以“视觉体量”对齐，而不是只按像素尺寸对齐。细长图标可以略放大，厚重图标要适当收敛。

### 光影与材质

- 默认光源来自左上方。
- 高光应柔和，阴影以物体内部自阴影为主，少用重投影。
- 玻璃、陶瓷、塑料、云、水滴等材质可以有不同质感，但边缘处理要统一：圆润、干净、不过度透明。
- 抠图后不要把星光、顶棚、白色主体等高光边缘抠得过薄或发灰。

### 色彩

- 颜色可以自由，但必须考虑深色和浅色背景。
- 高明度图标需要保留可见边缘或轻微暗部，避免浅色模式中消失。
- 深色图标需要足够高光和体积，避免深色模式中糊成一块。
- 避免大面积纯蓝背景作为通用底板。Tapp Store 彩绘图标已改为无蓝底的红色顶棚 + 银白店体。

### 细节密度

- 天气、问候、状态、控制面板图标可以更具象。
- 工具栏、按钮、导航岛等小尺寸入口优先使用线性 SVG 或简化图形。
- 大图标里可以有局部细节，但这些细节不能成为识别核心；缩小到 24-32px 时仍要看得出主语义。

## 已落地资源

当前已落地资源位于 `frontend/public/icons/`。

| 分组       | token / 语义                                                                                                                                        | 资源路径 / 说明                                                                                                   |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| 天气主图标 | `weather.sunny`, `weather.partlyCloudy`, `weather.cloudy`, `weather.fog`, `weather.drizzle`, `weather.rain`, `weather.snow`, `weather.thunderstorm` | `frontend/public/icons/weather/*.webp`                                                                             |
| 天气指标   | `weather.humidity`, `weather.wind`, `weather.airGood`, `weather.airModerate`, `weather.airPoor`                                                     | `frontend/public/icons/weather/humidity.webp`、`wind.webp`、`air-*.webp`                                             |
| 问候       | `greeting.sunrise`, `greeting.sunset`, `greeting.night`                                                                                             | `frontend/public/icons/greeting/*.webp`；晴天、多云可直接复用天气图标                                              |
| 右上动态岛 | `dynamic.quote`, `dynamic.music`, `dynamic.musicPaused`                                                                                             | `frontend/public/icons/dynamic/*.webp`                                                                             |
| 控制面板   | `control.animationStandard`, `control.animationLight`, `control.language`, `control.wallpaper`, `control.config`                                    | `frontend/public/icons/control-panel/*.webp`；外观入口复用晴天 / 月亮语义                                          |
| 设置分类   | `platforms`, `data`, `ai`, `ui`, `music`, `oauth`, `network`, `permissions`, `notifications`, `modules`, `advanced`, `about`                        | `frontend/public/icons/config/*.webp`；`ui` 复用 `control-panel/config.webp`，`music` 复用 `dynamic/music.webp`      |
| 首页小组件 | `widget.welcome`, `widget.library`                                                                                                                  | `frontend/public/icons/widgets/*.webp`                                                                             |
| Tapp       | `tapp.store`, `tapp.package`                                                                                                                        | `frontend/public/icons/tapp/store.webp`、`package.webp`；运行时 token 为 `myriad:tapp.store`、`myriad:tapp.package` |
| 状态反馈   | `status.success`, `status.error`, `status.warning`, `status.info`                                                                                   | `frontend/public/icons/status/*.webp`；Toast 使用显式 `type` 渲染，文案不再输出 `✓` / `✗` / `⚠` / `ℹ` 这类前缀符号 |
| 通知来源   | `agent`, `heartbeat`, `mcp`, `brew`, `tapp`, `updater`, `federation`, `system`                                                                      | `frontend/public/icons/notifications/*.webp`；仅通知中心、通知设置和通知轮播共用，不替换产品导航图标               |

## 接入规范

### 图标入口

| 入口                                                     | 角色                                                                                 |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `frontend/src/lib/icons.tsx`                             | 前端统一图标出口，集中导出库图标和少量 Myriad 自绘 SVG                               |
| `frontend/src/tapp/components/TappIcon.tsx`              | Tapp 图标渲染入口，优先级为 `iconSvg` > Myriad token / URL 图片 > emoji > 名称首字母 |
| `frontend/src/tapp/constants/icons.ts`                   | Tapp 官方 token 到 PNG 资源路径的映射                                                |
| `frontend/src/utils/weather.ts`                          | WMO 天气代码到天气资源图标的映射                                                     |
| `frontend/src/components/GlobalControlPanel.tsx`         | 问候、右上动态岛、控制面板资源图标                                                   |

### 彩绘 PNG 与线性 SVG 的分工

- 彩绘 PNG 用于大尺寸、强语义、需要视觉情绪的入口：天气、小组件、问候、Toast、Tapp fallback、控制面板大图标。
- 线性 SVG 用于按钮、导航、密集工具区和小尺寸操作入口。
- Tapp Store 当前采用混合策略：按钮入口使用纯线条 `MyriadStoreIcon`，页面 / 弹窗标题使用彩绘 `myriad:tapp.store`。
- 加载态保持原有 SVG spinner，不纳入彩绘图标替换，避免旋转高光造成违和。

### Token 命名

- token 使用语义命名，例如 `weather.sunny`、`status.success`、`dynamic.musicPaused`。
- Tapp runtime token 使用 `myriad:tapp.store`、`myriad:tapp.package` 这样的命名空间形式。
- 不用组件库名称作为设计 token。组件库图标只是实现细节，不是产品语义。

### Emoji 与动态内容

- 产品 UI 中的固定 emoji 应逐步改为 Myriad 自有资源或受控 token。
- Tapp manifest 继续兼容 emoji、URL 和内联 SVG，方便第三方 Tapp 自定义。
- AI 生成内容可以保留 emoji fallback，但固定界面不要依赖 AI 随机产出的 emoji 来承担核心视觉。

## 质量检查

图标合入前至少检查：

- `512x512`，PNG RGBA，四角透明。
- 深色和浅色背景下主体都清楚。
- 24px、32px、48px 三档尺寸下主语义可读。
- 没有文字、水印、第三方品牌误识别。
- 图标实际视觉大小与同界面其他图标接近。
- 抠图边缘没有明显绿边、黑边、白边或过度透明。

## 设计边界

- 不重绘第三方品牌标识；需要品牌识别的地方继续使用官方 / 现有图标。
- 不为了统一而把所有小工具按钮都改成彩绘大图标。密集 UI 保持线性、克制和可扫描。
- 不为深浅模式拆两套图标；通过边缘、阴影、明度和材质处理保证单套图标可用。
- 不把图标包当作任务清单维护。新增图标应由具体界面需求驱动，并在落地后补充到“已落地资源”表。
