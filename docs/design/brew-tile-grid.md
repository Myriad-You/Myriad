# Brew 磁贴网格改造 · 实施手册

交接文档。实施时以**本文为源**，旁边的 Canvas demo 只做视觉对照，不要从 demo 里抄网格行数或装箱参数。

配套视觉：

- Demo：[brew-tile-redesign.canvas.tsx](/Users/hitomi/.cursor/projects/Users-hitomi-GitHub-Myriad/canvases/brew-tile-redesign.canvas.tsx)
- 阶段看板：[brew-tile-implementation.canvas.tsx](/Users/hitomi/.cursor/projects/Users-hitomi-GitHub-Myriad/canvases/brew-tile-implementation.canvas.tsx)

Demo 里可以把网格切成 16×8 方便在窄画布里看更多卡。**生产必须是 16×4**，与首页 `WidgetGrid` 同一套格子，否则同一个 4×4 磁贴在两处物理尺寸对不上，复用就是假的。

---

## 0. 一天内怎么开工

1. 打开 demo，切「游客 / 登录 / 管理员」和六种排序，再切「桌面 / 尺寸矩阵 / 空态」。先建立视觉记忆。
2. 读本文 §1–§3（目标、范围、禁令）。范围外的东西一律不碰。
3. 从 PR 1 开始做。PR 2 的四个纯函数必须先有单测再写 UI。
4. 阶段 2 先做 `icon` 型，确认 `WidgetShell` + `useWidgetSize` 接法，再复制到另外五种。
5. 阶段 3 先上首页，再动 `BrewSourceGrid`。不要一上来改 946 行网格。

---

## 1. 目标

把 Brew 订阅源页从「固定三列 CSS Grid + 手工 `card_size`」换成首页同款的**虚拟坐标磁贴墙**：

- 每张卡展示**内容**（封面、标题、节律），不是统计数字墙。
- 构图按源的状态派生（精选 / 列表 / 节律 / 数字 / 图标），不是一种模子刻到底。
- 智能排序用六因子评分；智能的真正产出是**跨源主题聚类**。
- 同一套磁贴组件注册进首页 widget 库，Brew 页与首页共用。
- 游客是多数用户：未读、收藏、失败态全部按角色隐藏或降权。

成功标准：打开 `/brew`，第一眼能判断「现在有什么可看的」，而不是先扫一遍站名。

---

## 2. 范围

### 这次做

- `BrewSourceGrid` 的布局引擎、磁贴形态、排序、横向分页
- 磁贴符合 `WidgetComponentProps`，注册 `brew-source` / `brew-topic` / `brew-featured`
- 文章主题聚类（关键词先上，AI 后换）
- 四个游客态既有缺陷
- 后端：`pulses`（派生数组）+ `brew_items.topic` 列

### 这次不碰

- `BrewReader`（全屏阅读器）
- `BrewFeedList` / `ItemCard`（文章列表，下一独立任务）
- `ControlIsland` 状态机（只加排序项和 `topic-feed` 模式）
- RSSHub 配置、添加订阅、`EditModal`
- Brewlia 注释与播客

### 留到之后

- 文章列表密度（`ItemCard` 的 `p-6` + 瀑布流最短列优先破坏时序）
- 按阅读时长筛选
- 概览「继续阅读」（依赖列表层露出 `read_progress`）

---

## 3. 设计禁令

实施时对照这份清单。违反任何一条都算没做完。

| 禁止 | 改成 |
| --- | --- |
| 左边框高亮、描边分隔、装饰性 `border` | 留白、嵌套表面、字重/颜色反差 |
| 文字压在 RSS 封面上 | 封面独占一块，文字另占一块 |
| 无图时画灰色占位块 | 走纯文本布局，`TileCover` 返回 `null` |
| 标题 > 15px、数字 > 38px 撑满磁贴 | 标题 ≤ 14.5px，数字 ≤ 38px（4×4 数字型），靠封面和版面 |
| 4×2 上下堆封面再堆标题 | 4×2 必须左右拆栏（见 §10） |
| 固定三列 / 长条 `tiny` 卡 | 虚拟坐标，尺寸只有 `2x2` / `4x2` / `4x4` |
| 把 Brew 的 FLIP 搬进首页 | 首页继续用 `WidgetGrid` 自己的 transition |
| 拖拽存 `(x, y)` | 拖拽只改 `sort_order`，位置由装箱决定 |
| 新建「其他」主题桶 | 分不进预定义主题的文章只留在源磁贴里 |
| 阻塞 `list_sources` / `list_items` 做 AI 打标 | 打标离线；读接口只读已有列 |
| 为磁贴新开 `GET /source/:id` | 继续 `getSources()` + `requestCache` |

圆角三级与首页一致：shell 12px（`rounded-xl`）/ nested 8px（`rounded-lg`）/ micro 6px（`rounded-md`）。安全内边距 `WIDGET_SAFE_PADDING`（14）× `scale`。字号乘 `fontScale`，间距乘 `scale`。

---

## 4. 现状：直接复用，不要重写

| 能力 | 文件 | 怎么用 |
| --- | --- | --- |
| 虚拟网格 / 百分比几何 | `frontend/src/components/WidgetGrid.tsx` | `SIZE_TO_DIMENSIONS`；`left/top/width/height` 百分比 |
| 磁贴契约 | 同上 `WidgetComponentProps` | `{ config, isEditMode, isPreview, onConfigChange }` |
| 尺寸枚举 | `WidgetSize`：生产只用 `2x2` / `4x2` / `4x4` | 枚举里**没有** `8x4` |
| 断点 | `frontend/src/utils/viewportBands.ts` | phone ≤767 → 4 列；tablet 768–1077 → 8 列；desktop ≥1078 → 16 列 |
| 缩放 | `hooks/useWidgetSize.ts` | `scale`、`fontScale`、`containerRef` |
| 外壳 / 光晕 | `widgets/shared/WidgetShell.tsx`、`GlowBackground.tsx` | 根节点必须是 `WidgetShell` |
| 装箱范例 | `ControlPanel/widgetReflow.ts` | first-fit，先填行再换行，不跨页 |
| 装箱单测范例 | `ControlPanel/widgetReflow.test.ts` | `node:test` + `tsx`，`pnpm test:unit` |
| 分页滑轨 | `ControlPanel/ControlPanelWidgets.css` | `width: N00%` + `translateX`，500ms `cubic-bezier(0.25,1,0.5,1)` |
| 入场动画 | `hooks/animation/pages/brew.ts` `useBrewCardStagger` | 只看 index，换布局不用改 |
| FLIP 排序 | `BrewSourceGrid` 现有 `getBoundingClientRect` | 布局无关，**保留** |
| 数据 | `services/brewApi.ts` `getSources` / `getItems` | cache key `brew:sources`，TTL 30s |
| 首页轮询 | `useHomeVisibilityInterval` | FriendLinks 60s，照搬 |
| 封面 URL | `components/brew/constants.ts` `getImageUrl` | 不要另写一套 |
| 自有源 | `BREW_MINE_CATEGORY === '我'`、`isOwnBrewSource` | SEO 依赖，禁止改文案 |
| 友链源 | `BREW_FRIEND_LINK_CATEGORY === '友情链接'` | FriendLinksWidget 已按此过滤 |
| 角色 | `useAuth()` 的 `isAuthenticated` / `isAdmin` | 游客 / 成员 / 管理员 |

首页已有读 Brew 的范例：`FriendLinksWidget` 调 `getSources()`，客户端按 category 过滤。单源磁贴走同一个 cache key，`requestCache` 会合并 inflight。

---

## 5. 数据模型

全部是**加字段**。没有破坏性改动。

### 前端 `frontend/src/types/brew.ts`

```ts
// BrewSource 新增
pulses?: number[]
// 近两年每篇文章距今天数，最多 60 个，已按新→旧。
// 缺失时节律型降级为 feature。

// BrewItem / BrewItemPreview 新增
topic?: string | null
// 预定义主题 key（如 "engineering"），不是展示文案。
// null / 缺失的文章不参与聚类。

// BrewItemsQuery 新增
topic?: string
```

`BrewTopic` **不是**后端类型，由 `clusterTopics()` 在前端派生。

### 后端

| 位置 | 字段 | 说明 |
| --- | --- | --- |
| `list_sources` 响应 | `pulses: Vec<i32>` | 派生，不落库 |
| `brew_items` | `topic TEXT NULL` | 关键词或 AI 写入 |
| `ItemsQuery` / `list_items` | `topic: Option<String>` | 与现有 `category` 同级过滤 |

**不要新建 numbered migration。** 本仓库的政策是：

1. 把列写进 `backend/migrations/003_brew_system.rs`（greenfield）
2. 把列写进 `backend/src/db/schema_check/tables_brew.rs` 的 `brew_items` `TableDef`
3. 写进 `backend/src/models/entities/brew_items.rs` 的 `Model` + `ItemResponse`
4. 启动时 `schema_check` 对旧库执行 `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`

`pulses` 只出现在 JSON 里，不改表。

### 游客数据（现状，不要「修」后端）

`list_sources` 对游客：

- `unread_count` 恒 0（未登录时未读聚合直接跳过）
- `recent_items[].is_read` 恒 `false`（`user_id = -1` 做 LEFT JOIN，匹配不到）

所以源级未读是 0、条级未读点全亮。这是前端 bug，在阶段 0 用 `isAuthenticated` 藏掉。不要改游客 SQL。

---

## 6. 构图决策树

函数：`tileLayout(source, role) → BrewTileLayout`

按顺序，命中即停：

```
source_type === 'link'                         → icon
role !== 'guest' && unread_count >= 20         → numeric
daysSinceLastPublish > 60 && pulses.length ≥ 6 → cadence
展示条目数 ≤ 2                                 → feature
其余                                           → list
```

`daysSinceLastPublish` 用 `last_success_at` 或最新 `recent_items[0].published_at`。没有时间戳时不要走 cadence。

`pulses` 缺失：本该 cadence 的源改走 feature。可用 `recent_items` 的 3 个时间戳画简版节律，但不要对只有 3 根线的图走 cadence 主视觉。

展示条目：首页/列表型若 `recent_items.length < 7`，再补 `getItems({ source_id, per_page: 8 })`。

---

## 7. 尺寸派生

函数：`tileSize(score, source, band, sourceCount) → WidgetSize`

阈值（桌面）：

| 条件 | 尺寸 |
| --- | --- |
| `sourceCount < 6` | 一律 `4x4`（源太少就撑满，不要留空格子） |
| `source_type === 'link'` | `4x2` |
| `score ≥ 0.55` | `4x4` |
| `score ≥ 0.30` | `4x2` |
| 其余 | `2x2` |

按 band 降档（**只在 `tileSize` 里做**，组件内禁止再判断窄屏）：

| band | 列 × 行 | 降档 |
| --- | --- | --- |
| desktop ≥1078 | 16 × 4 | 完整三档 |
| tablet 768–1077 | 8 × 4 | `4x4` → `4x2`；主题磁贴也降到 `4x2` |
| phone ≤767 | 4 × 4 | 只剩 `4x2`（满宽）和 `2x2` |

`card_size` 非空视为用户锁定，映射后不再走分数：

`tiny → 2x2`，`mini → 4x2`，`full → 4x4`，然后再套 band 降档。零 migration。

主题磁贴：智能模式前 2 个 `4x4`；主题模式前 4 个 `4x4`，其余 `4x2`。不足 3 篇的主题不成卡。

---

## 8. 评分算法

纯函数，无 IO。`now` 可注入，单测不要读 `Date.now()`。

```ts
export type BrewViewerRole = 'guest' | 'member' | 'admin'

export interface ScoreFactors {
  own: number     // 0 | 1   isOwnBrewSource
  recency: number // 0..1    exp(-days / 21)
  corpus: number  // 0..1    log1p(item_count) / log1p(2000)
  unread: number  // 0..1    log1p(unread_count) / log1p(50)
  pin: number     // 0 | 1   sort_order != null
  fail: number    // 0 | 1   error_count > 0
}

export const SCORE_WEIGHTS: Record<BrewViewerRole, ScoreFactors> = {
  guest:  { own: 0.40, recency: 0.28, corpus: 0.20, unread: 0,    pin: 0.12, fail: -0.60 },
  member: { own: 0.18, recency: 0.22, corpus: 0.10, unread: 0.38, pin: 0.12, fail: -0.60 },
  admin:  { own: 0.15, recency: 0.20, corpus: 0.08, unread: 0.32, pin: 0.10, fail:  0.45 },
}

export function brewScore(s: BrewSource, role: BrewViewerRole, now: number): number {
  const f = scoreFactors(s, now)
  const w = SCORE_WEIGHTS[role]
  return f.own * w.own + f.recency * w.recency + f.corpus * w.corpus
       + f.unread * w.unread + f.pin * w.pin + f.fail * w.fail
}
```

补充规则：

- `source_type === 'link'` 时 `recency = 0`（友链没有更新时间）。
- 游客 `unread` 权重必须为 0。单测锁死。
- 管理员 `fail` 为正，失败源被抬到能看见；游客/成员为负，沉底。
- **分档**：`Math.round(score / 0.05) * 0.05`，档内按 `id` 升序。禁止直接按连续分数排，否则每次刷新微动。
- 会话内冻结：进入页面算一次顺序和尺寸；未读数字可实时变，位置不动。重算时机：切排序、手动刷新、重新进入。

旧排序模式**一行不改依据**，排完再进同一个装箱：

| `SortMode` | 依据 |
| --- | --- |
| `smart`（新，默认） | 分档分数；前 2 个主题卡插到最前 |
| `topic`（新） | 主题卡全部在前，再接源 |
| `update` | 最近更新时间 |
| `custom` | `sort_order` |
| `category` | 分类顺序；`breakOn` 在分类边界强制换页 |
| `pinyin` | 现有拼音 |
| `random` | 现有随机种子 |

类型现在是 `'update' | 'custom' | 'category' | 'random' | 'pinyin'`，在 `components/brew/types.ts` 与 `manager/modes/types.ts` **两处**都要加 `'smart' | 'topic'`。

---

## 9. 装箱与分页

```ts
export type BrewCard =
  | { kind: 'source'; key: string; src: BrewSource; size: WidgetSize }
  | { kind: 'topic'; key: string; topic: BrewTopic; size: WidgetSize }

export type PackedCard = BrewCard & { x: number; y: number }

export function packBrewCards(
  cards: BrewCard[],
  cols: number,
  rows: number,
  opts?: { breakOn?: (prev: BrewCard, next: BrewCard) => boolean },
): PackedCard[][]
```

算法（抄 `packControlPanelWidgets`，页数不封顶）：

1. 按输入顺序 first-fit。
2. 每页 `cols × rows`。先扫 y，再扫 x，放得下就占住。
3. 当前页放不下 → 开新页。禁止一张卡跨页。
4. `breakOn(prev, next) === true` 时强制新页（给 `category` 用）。
5. 不丢卡。断言：不重叠、集合相等、页数 ≥ 1（有卡时）。

几何：

```
left   = x / cols * 100%
top    = y / rows * 100%
width  = w / cols * 100%
height = h / rows * 100%
padding = 4px
```

`rows` 恒为 **4**。`cols` 来自 `homeGridColsForBand`。

分页：外层 `width: ${pages * 100}%`，`transform: translateX(-${page * 100 / pages}%)`。页码进 `sessionStorage`，切排序时重置为 0。游客也要有圆点指示器（控制面板没有，Brew 必须有）。

搜索态：**不装箱**。统一 `2x2` 规整网格，单页可滚动。搜索是「找特定源」，不需要视觉层次。

---

## 10. 磁贴视觉规格

根节点：`WidgetShell` + 可选 `GlowBackground`（`exlight` / `prefers-reduced-motion` 不渲染光晕）。身份色来自 `normalizeThemeColor(theme_color)`。

### 10.1 字号（再乘 `fontScale`）

| token | px | 用途 |
| --- | --- | --- |
| `T_TITLE` | 14.5 | 站名、头条、主题名 |
| `T_MINOR` | 11 | 次条标题 |
| `T_META` | 8 | 时间、标签、篇数说明 |
| `T_NUM` | 24 | 主题篇数、未读角标 |
| 数字型 4×4 | 38 | 未读主数字 |
| 数字型 4×2 | 30 | 未读主数字 |

### 10.2 封面

- `src = getImageUrl(item.image)`，`object-fit: cover`
- 无图 → 返回 `null`，调用方改纯文本，**禁止灰块**
- 文字不压图
- 2×2 不放封面（面积不够）

分级：

| 尺寸 / 构图 | 封面 |
| --- | --- |
| 4×4 feature | 高约 96×scale，通栏 |
| 4×4 list | 头条 52×52 方图 |
| 4×4 topic | 高约 84×scale，2×2 拼贴 |
| 4×2 feature / cadence | 左侧视觉栏，高度吃满内容区 |
| 4×2 list | 不放封面，单行标题 × 4–5 |
| 2×2 | 无封面 |

### 10.3 4×2 必须左右拆栏

4×2 是扁矩形。上下再堆封面+标题，空间利用率极差，这是上一版被否的点。

| 构图 | 4×2 结构 |
| --- | --- |
| feature | 左封面（约 38–42% 宽）+ 右站名 / 标题两行 / meta |
| cadence | 左节律图 + 右站名 / 最新一篇 / 跨度 |
| numeric | 左数字 + 右 3–4 条标题 |
| icon | 左 50px 字标 + 右站名 / 两行简介（已是拆栏） |
| list | 不拆栏；站名一行 + 4 条单行标题（吃水平宽度） |
| topic | 左 2×2 拼贴 + 右主题名 / 篇数 / 1 条（demo 已是这样） |

### 10.4 五种源构图 + 主题

**feature**：站名行 → 封面 → 标题 clamp 3（4×4）或 2（更小）→ `时间 · N 分钟`。4×4 可再跟 1 条 dim 次条。

**list**：站名行 → 头条（4×4 带方图）→ 次条。4×4 每页 5 条，4×2 每页 4 条单行。超出用 CSS keyframes 整页轮播。

**cadence**：站名 → 节律图 → 轴标签（左「N 个月前」，右「今天」或「失败 N 次」）。4×4 底部再跟最新一篇。

节律图：窗口 730 天；`x = (1 - sqrt(days/730)) * 100%`（右端是今天，近期更疏、远期更密）；高度用 `reading_time`，没有就用稳定伪随机；近处更不透明。不要画横轴以外的装饰线。

**numeric**：仅登录。数字与「条未读」横排，下面 5 条（4×4）或右侧 3–4 条（4×2）。游客永不进入此构图。

**icon**：字标（站名首字 + `theme_color`）+ 站名 + 外链箭头 + 简介 clamp 3 + 标签。点击 `window.open(site_url || url)`。

**topic**：`主题聚合` + 名 + 篇数 + `N 个源` + 4 条。封面拼贴 2×2。不足 4 张封面就少格，不要补灰块。

### 10.5 站名行

左：18px 扁字标 + 站名。右：未读数字（游客不显示）。管理员且 `error_count > 0` 时节律/数字改用红色 token。

### 10.6 次条 `MinorRow`

`[可选 20px 缩略图] 单行标题  时间`

字色三档：正常 `text.secondary` / dim `text.quaternary` / 更 dim。4×4 列表第 3 条起 dim。

---

## 11. 交互

### 点击

| 目标 | 行为 |
| --- | --- |
| 源磁贴（非 icon） | `onSourceClick` → 现有 `items` 视图 |
| icon | `window.open` |
| 次条 / 头条标题 | 直接打开该篇，`stopPropagation` |
| 主题磁贴 | 新 `viewMode: 'topic-feed'`，跨源列表 |
| 节律单根竖线 | 第一版整卡可点即可。若要点线，`pulses` 需改成 `{ days, itemId }[]` |

首页磁贴点击：`navigate('/brew?source=' + id)` 或 `/brew?topic=`。不要在首页打开阅读器。

`ViewMode` 现为 `'sources' | 'items' | 'starred' | 'category-feed'`，加 `'topic-feed'`。`ControlMode` 加 `'topic-feed'`，模式组件抄 `CategoryFeedMode`。

### 键盘（现有一律保留）

| 键 | 现在 | 改造后 |
| --- | --- | --- |
| `j` / `↓`、`k` / `↑` | 一维 | 仍一维，按**装箱顺序** |
| `←` `→` | 未占用 | **切页** |
| `o` / Enter、`m`、`s`、`r`、`a`、`/`、`?` | 不变 | 不变 |

不要改成 vim 二维 `h/j/k/l`。

### 拖拽

只改 `sort_order`，1 对 1 交换或按装箱顺序插入。松手后卡片落在装箱结果上，不一定在鼠标下 —— 拖的时候必须画插入位。FLIP 继续用。废弃右下角 resize；改成「锁定当前尺寸」开关，写回 `card_size`。

### 游客四个缺陷（PR 1，先做）

1. `SourceCard`（以及新磁贴）预览未读圆点：`isAuthenticated` 才渲染。
2. `ControlIsland` 的 `withNewItems`：同样加登录判断。现在游客每条 `is_read === false`，「来自 XX 的新文章」永远命中。
3. `Brew.tsx` 二级导航「收藏」对游客隐藏。现在能点，但 `viewMode === 'starred' && isAuthenticated` 让主区空白。
4. `handleItemSelect`：游客不调用 `markRead`。现在每次点开必 401。

---

## 12. 首页 vs Brew 页

同一组件，行为靠 props / 注册配置分叉。

| | 首页 | Brew |
| --- | --- | --- |
| 目的 | 扫一眼 | 浏览全部源 |
| 数量 | 用户放 1–3 张 | 全部源，装箱分页 |
| 尺寸 | 编辑模式手摆 | `tileSize()` |
| 轮播 | 默认关，只第一页 | 开 |
| 点击 | 跳 `/brew?...` | 就地切 `viewMode` |
| 刷新 | `useHomeVisibilityInterval` 60s | 进入拉取；登录走现有 WS |
| 数据 | `getSources()` + `find(sourceId)` | 已有全量列表 |
| 位置动画 | `WidgetGrid` 自带 | Brew FLIP；不要混用 |

`config.config`：

```ts
// brew-source
{ sourceId: number }
// brew-topic
{ topicKey: string }
// brew-featured：不绑源，内部跑 smart 取 top source + top topic
```

设置 UI 用 `WidgetType.settings?: TappSettingItem[]`（`select`）。FriendLinks 没有 settings，这块要新写，参考 `TappShortcutWidget` 的实例配置，不要抄 FriendLinks。

注册（`builtinWidgets.ts`）：

```ts
'brew-source':    { defaultSize: '4x2', supportedSizes: ['2x2','4x2','4x4'], component: ... }
'brew-topic':     { defaultSize: '4x2', supportedSizes: ['2x2','4x2','4x4'], component: ... }
'brew-featured':  { defaultSize: '4x4', supportedSizes: ['4x2','4x4'],       component: ... }
```

同步改：`BUILTIN_WIDGET_ORDER`、`WIDGET_NAME_KEY`、`i18n/en-US.json`（及 zh-CN / ja-JP）的 `widgets`。id 是 kebab，i18n key 是 camelCase（`brewSource` / `brewTopic` / `brewFeatured`）。`getWidgetTranslationKey` 已做这个转换，但 `WIDGET_NAME_KEY` 仍要显式写。

---

## 13. 类型契约

新建 `frontend/src/components/brew/logic/`：

### `score.ts`

见 §8。导出 `scoreFactors`、`brewScore`、`SCORE_WEIGHTS`、`SCORE_BUCKET = 0.05`。

### `layout.ts`

```ts
export type BrewTileLayout = 'feature' | 'list' | 'cadence' | 'numeric' | 'icon'

export function tileLayout(s: BrewSource, role: BrewViewerRole): BrewTileLayout
export function tileSize(
  score: number,
  s: BrewSource,
  band: ViewportBand,
  sourceCount: number,
): WidgetSize
```

### `pack.ts`

见 §9。

### `topics.ts`

```ts
export interface BrewTopic {
  key: string          // "engineering"
  nameKey: string      // i18n key
  hue: string          // 给 Glow / 字标
  items: BrewItem[]
}

export const TOPIC_MIN_ITEMS = 3
export const TOPIC_WINDOW_DAYS = 30
export const PREDEFINED_TOPICS: readonly string[]

export function clusterTopics(items: BrewItem[], now: number): BrewTopic[]
export function inferTopicByKeywords(item: BrewItem): string | null
```

`clusterTopics` 只看 `item.topic`，不管标签是谁填的。关键词版与 AI 版替换点只在写入 `item.topic`。

---

## 14. 文件地图

路径均相对 `frontend/src/`，后端相对 `backend/`。

| 路径 | 操作 | 做什么 |
| --- | --- | --- |
| `components/brew/logic/score.ts` | 新建 | 评分 |
| `components/brew/logic/layout.ts` | 新建 | 构图 + 尺寸 |
| `components/brew/logic/pack.ts` | 新建 | 装箱 |
| `components/brew/logic/topics.ts` | 新建 | 聚类 + 关键词 |
| `components/brew/logic/*.test.ts` | 新建 | 单测 |
| `components/brew/tileGridFlag.ts` | 新建 | `localStorage['brew_tile_grid'] === '1'` |
| `components/brew/tiles/TileShell.tsx` | 新建 | 薄封装 WidgetShell |
| `components/brew/tiles/TileCover.tsx` | 新建 | 封面，无图返回 null |
| `components/brew/tiles/Cadence.tsx` | 新建 | 节律图 |
| `components/brew/tiles/MinorRow.tsx` | 新建 | 次条 |
| `components/brew/tiles/BrewSourceTile.tsx` | 新建 | 五种构图 × 三档尺寸 |
| `components/brew/tiles/BrewTopicTile.tsx` | 新建 | 主题卡 |
| `components/brew/tiles/BrewFeaturedTile.tsx` | 新建 | 首页综合卡 |
| `components/brew/tiles/rotation.css` | 新建 | keyframes |
| `components/brew/BrewPageDots.tsx` | 新建 | 分页点 |
| `components/brew/manager/modes/TopicFeedMode.tsx` | 新建 | 控制岛 |
| `views/BrewTilePreview.tsx` | 新建 | `import.meta.env.DEV` 预览，24 种组合 |
| `components/brew/BrewSourceGrid.tsx` | 大改 | 虚拟坐标 + 分页 + flag 切旧网格 |
| `components/brew/types.ts` | 改 | SortMode |
| `components/brew/manager/modes/types.ts` | 改 | SortMode + ControlMode |
| `components/brew/manager/modes/DefaultMode.tsx` | 改 | 下拉加 smart / topic |
| `components/brew/manager/ControlIsland.tsx` | 改 | 游客 tip；topic-feed |
| `components/brew/cards/SourceCard.tsx` | 改 | 游客未读点（PR 1）；PR 9 删除 |
| `views/Brew.tsx` | 改 | 收藏 Tab、markRead、topic-feed |
| `types/brew.ts` | 改 | pulses / topic |
| `services/brewApi.ts` | 改 | `getItems` 传 `topic` |
| `components/widgets/builtinWidgets.ts` | 改 | 三条注册 |
| `i18n/*.json` 三语言包 | 改 | widget 名 + 主题名 |
| `App.tsx` | 改 | DEV 路由 `/dev/brew-tiles` |
| `api/brew/feeds_articles.rs` | 改 | pulses；list_items.topic |
| `models/entities/brew_items.rs` | 改 | topic 列 |
| `db/schema_check/tables_brew.rs` | 改 | TableDef |
| `migrations/003_brew_system.rs` | 改 | greenfield 列 |
| 关键词打标 | 新建小模块 | 入库同步 |
| Brewlia 批量打标 | 后做 | 每小时，只处理 30 天内 `topic IS NULL` |

清退（PR 9，观察两周后）见 §20。

---

## 15. 阶段、PR、验收

关键路径：PR2 → PR4 → PR5 → PR6 → PR7 → PR8。PR3 与 PR2 并行。约 16 个前端人日 + 3 个后端人日 + 2 周观察。

| PR | 阶段 | 工作量 | 交付 |
| --- | --- | --- | --- |
| 1 | 0 游客 bug | 半天 / ~80 行 | 游客无假未读、无空白收藏、无 401 |
| 2 | 1 纯逻辑 | 1 天 / ~500 行 | 评分/尺寸/装箱/聚类 + 单测 |
| 3 | 6 后端 | 2–3 天 / ~200 行 | pulses + topic 列 + list 过滤 |
| 4 | 2 磁贴 | 3–4 天 / ~1200 行 | 组件 + `/dev/brew-tiles` |
| 5 | 3 首页 | 半天 / ~60 行 | widget 库可拖 |
| 6 | 4 网格 | 3–4 天 / ~700 行 | `/brew` 分页墙 + flag |
| 7 | 5 排序 | 2 天 / ~150 行 | smart 默认；topic-feed |
| 8 | 7 轮播 | 2 天 / ~250 行 | 错峰 + 可见性 pause |
| 9 | 清退 | 半天 / −1000 行 | 删旧网格与死键 |

### PR 1 验收

- 未登录 `/brew`：源卡预览无未读圆点
- 二级导航只有三项（无收藏）
- 点开文章 Network 无 `/api/brew/...` 401

### PR 2 验收

- `scoreFactors` 六因子单独断言
- 游客 unread 权重为 0
- 同数据 `guest` / `member` / `admin` 头部顺序不同且符合直觉（自有源游客更靠前；失败源只在 admin 抬头）
- `packBrewCards` 不重叠、不丢卡、`breakOn` 换页
- `clusterTopics`：<3 篇不成卡；窗口外的不算

用真实订阅列表打一次分，看 `own = 0.40` 会不会让多个自有源霸屏。会的话在这个 PR 调权重，不要留到 UI 阶段拍脑袋。

### PR 3 验收

- `GET /api/brew/sources` 每源带 `pulses`，长度 ≤ 60，值为距今天数
- 上千篇文章的源被截断，响应没有明显变慢（一次窗口查询，禁止 N+1）
- `GET /api/brew/items?topic=engineering` 只返回该主题
- `topic IS NULL` 的文章不进过滤结果
- 入库后关键词能写出 topic；AI 失败保持 NULL

### PR 4 验收

- `/dev/brew-tiles`（仅 DEV）铺开：6 构图 × 3 尺寸 × 4 空数据
- 无封面 / 无简介 / 无条目 / 抓取失败都不崩、不出现灰占位
- 4×2 是拆栏或单行列表，不是上下瘦条
- `useWidgetSize` 接上，缩到 compact 不溢出

### PR 5 验收

- 首页库出现三个 Brew 磁贴
- resize snap 到声明档位
- 可放多个 `brew-source`，各绑不同 `sourceId`
- 点击跳到 `/brew`

### PR 6 验收

- flag 开：虚拟坐标 + 分页点 + ←→ 切页
- flag 关：旧网格像素级可用
- 搜索 = 统一 2×2，可滚动
- 拖拽只改顺序，有插入位
- `j/k` 按装箱顺序
- 窄屏无 4×4
- 二次进入 100ms 内出现 localStorage 骨架，无整屏重排
- 页码进 sessionStorage

### PR 7 验收

- 默认 smart
- custom 拖拽保存仍工作
- category 按分类换页，标题是页标题不是网格内横条
- 主题卡进入 topic-feed
- 主题 < 3 个时下拉里 topic 禁用

### PR 8 验收

- 非当前页 / 滚出视口 / 隐藏标签页：`animation-play-state: paused`
- `exlight` 与 `prefers-reduced-motion`：停在第一条，不渲染 Glow
- 一屏十多张卡滚动无明显掉帧

### 阶段 2 推荐顺序

1. TileShell
2. TileCover
3. Cadence（可先用 3 个 `recent_items` 时间戳）
4. MinorRow
5. BrewSourceTile · icon
6. feature
7. list + 轮播
8. numeric
9. cadence 构图
10. BrewTopicTile
11. 三档尺寸分支（约占阶段 2 的四成）
12. 空数据
13. 布局缓存 / 会话冻结

---

## 16. 后端细节

### pulses：一次查完全部源

```sql
SELECT source_id, published_at FROM (
  SELECT source_id, published_at,
         ROW_NUMBER() OVER (
           PARTITION BY source_id ORDER BY published_at DESC
         ) AS rn
  FROM brew_items
  WHERE source_id = ANY($1)
    AND published_at > $2          -- now() - 730 days
) t
WHERE rn <= 60
```

在 `list_sources` 里与现有 unread / recent_items 一起 `tokio::join!`。后端算成 `i32` 距今天数再返回。不要给前端时间戳。

### topic 列

- `ALTER` 由 schema_check 自动补；同时改 `003` 和 SeaORM model
- 建议索引：`CREATE INDEX IF NOT EXISTS idx_brew_items_topic ON brew_items (topic) WHERE topic IS NOT NULL`
- 写进 `schema_check/expected_indexes.rs`

### 打标

1. **入库同步关键词**（`inferTopicByKeywords` 的 Rust 版或共享词表）。成本接近零。
2. **每小时 AI 批**：近 30 天、`topic IS NULL`、每批 20–50 篇。复用 Brewlia 调用链。
3. `UPDATE ... WHERE topic IS NULL`，重跑安全。
4. 失败保持 NULL。禁止写入「未知」。
5. Prompt：标题 + 摘要前 200 字；只许从预定义 key 里选一个或 null；JSON 数组按 index 对齐；不要传正文。

### 预定义主题（10 个，key 稳定，文案走 i18n）

| key | zh-CN | 关键词种子（标题/摘要，大小写不敏感） |
| --- | --- | --- |
| `engineering` | 工程实践 | refactor, 重构, 微服务, 单体, code review, 代码审查, rust, typescript |
| `systems` | 系统与性能 | linux, kernel, tcp, dns, sqlite, postgres, 性能, perf, jit |
| `ai` | AI 与 LLM | llm, gpt, 模型, transformer, agent, embedding, 提示词 |
| `product` | 产品与设计 | 设计, ux, ui, 独立开发, 产品, 交互 |
| `writing` | 中文写作与排版 | 中文, 排版, 写作, 播客, 字体 |
| `tools` | 效率与工具 | 效率, 笔记, 工作流, 周刊, 工具 |
| `culture` | 生活与文化 | 生活, 文化, 旅行, 城市 |
| `security` | 安全与隐私 | 安全, 漏洞, cve, 加密, privacy |
| `oss` | 开源与社区 | 开源, github, license, 社区 |
| `hardware` | 硬件与制造 | 芯片, 硬件, pcb, 制造, risc-v |

词表放 `topics.ts` 与后端各一份也可以，但 **key 必须一致**。宁可漏标（null），不要标错硬塞。每季度看一次误分类再改词，不要让 AI 自创主题名。

聚类只看近 30 天。一篇文章单标签。不要「其他」。

---

## 17. 测试

框架：`node:test` + `tsx`。在 `frontend/` 跑 `pnpm test:unit -- src/components/brew/logic`。

没有 Testing Library，没有视觉回归，没有 E2E。组件靠 `/dev/brew-tiles` 人工扫。

后端：inline `#[cfg(test)]`。Brew 已有 DB-gated smoke（`BREW_TEST_DATABASE_URL`），pulses SQL 加进去。

---

## 18. 动画

| 现有 | 处置 | 原因 |
| --- | --- | --- |
| `useBrewCardStagger` | 保留 | 只看 index |
| FLIP 排序 | 保留 | `getBoundingClientRect`，与 CSS Grid 无关 |
| `SourceCard` 的 `gridRow: span N` | 重写 | 改 `WidgetSize` + 百分比高 |
| 分类 `col-span-3` 横条 | 重写 | 改成分页标题 |
| 首页位置动画 | 不要搬 FLIP | 用 WidgetGrid 的 0.45s / 跨档淡入淡出 |

轮播：纯 CSS `@keyframes` 步进 `translateY`，零定时器。每张卡不同的负 `animation-delay` 错峰。非当前页、滚出视口、`visibilitychange` hidden → `paused`。`exlight` 停在第一条。

---

## 19. 性能与稳定性

| 预算 | 值 |
| --- | --- |
| 单页卡数 | 16×4 最多 8 张 4×4 或 32 张 2×2，超出即分页 |
| 合成层 | 单页 ≤16 个提升层；exlight 关 Glow；非当前页 pause |
| 首屏 | localStorage 骨架 ≤100ms |
| 切页 | 只做 `translateX`，目标 60fps |

稳定性：

1. 上次装箱结果进 `localStorage`，当下次骨架。
2. 分数 0.05 分档。
3. 会话内冻布局。
4. 页码进 `sessionStorage`。

`getSources` 失败：用缓存布局+数据渲染，顶上一行重试。首页 widget 已有 `failed` 态。

---

## 20. 清退（PR 9）

观察期看：新报错、位置是否每次进入都变、首屏是否整屏重排、主题是否离谱、节律图是否空白、自己会不会想切回旧网格。

确认不回退后删除：

| 资产 | 约行数 | 备注 |
| --- | --- | --- |
| `cards/SourceCard.tsx` | 781 | 先 grep 引用 |
| `constants.ts` `SIZE_TO_ROWS` | 5 | 只被 SourceCard 用 |
| `types.ts` `SourceCardProps` | ~28 | |
| `BrewSourceGrid` resize | ~120 | `handleResizeStart`、拉伸条 |
| `BrewSourceGrid` CSS Grid 容器 | ~80 | `grid-cols-1/2/3`、`gridAutoRows` |
| i18n `feedTypeRss/Atom/Json` | 9 键 × 3 | 只被 SourceCard 用；`feedTypeLink` / `feedTypeBrewliaDesc` 可能 EditModal 还在用，逐键 grep |
| `tileGridFlag` | ~15 | 最容易忘，变成永久债 |

保留：`CardSize` 与 `brew_sources.card_size`（改语义为锁定）、`sort_order`、`getIconUrl` / `getImageUrl`、`getPlainText`、`normalizeThemeColor`、`BREW_MINE_CATEGORY` / `isOwnBrewSource`、整个 FeedList/ItemCard。

---

## 21. Feature flag

仓库**没有**现成 feature flag 框架。不要为这个功能引入远程配置。

```ts
// frontend/src/components/brew/tileGridFlag.ts
const KEY = 'brew_tile_grid'

export function isBrewTileGridEnabled(): boolean {
  try {
    return globalThis.localStorage?.getItem(KEY) === '1'
  } catch {
    return false
  }
}
```

打开：控制台 `localStorage.setItem('brew_tile_grid','1')` 后刷新。`BrewSourceGrid` 顶部分支。PR 9 删除模块与分支。

---

## 22. Dev 预览路由

```tsx
// App.tsx，与 PerformanceMonitor 一样包在 import.meta.env.DEV 里
<Route path="/dev/brew-tiles" element={<SuspensePage><BrewTilePreview /></SuspensePage>} />
```

生产构建不得打进这个页面。用 fixture 源覆盖：有图/无图、有条目/无条目、失败、link、未读 47、沉寂 210 天。每种构图 × `2x2`/`4x2`/`4x4` 铺成一页。

---

## 23. 已拍板，不要再讨论

| 问题 | 决定 |
| --- | --- |
| 拖拽 / resize | 留拖拽；废 resize，改「锁定尺寸」写回 `card_size` |
| Brew 用 4 行还是 8 行 | **4 行**，与首页一致。Demo 16×8 只是画布预览 |
| 新旧并存 | 一次替换 + localStorage flag，观察两周再删 |
| 主题先关键词还是等 AI | 先关键词 |
| 主题自由生成？ | 预定义 10 个 key，AI 只做分类 |
| 主题名语言 | i18n，跟界面 |
| 聚类窗口 | 近 30 天 |
| 多标签？ | 单标签 |
| 「其他」？ | 不要 |
| 未读变化是否挪卡 | 数字动、位置会话内冻 |

接受的代价：代码净增约 700 行（清退后约 −300）；一屏看不到全部源；主题前期会有误分类；松手后卡不在鼠标下。换来的是零维护排版、内容可读、网格有节奏。

---

## 24. 给实施者的检查清单

开始写代码前：

- [ ] 读过 demo 的「桌面 / 尺寸矩阵 / 空态」三页
- [ ] 知道生产是 16×4，不是 demo 默认的 16×8
- [ ] 知道 `WidgetSize` 没有 `8x4`
- [ ] 知道游客 `unread_count=0` 且 `is_read=false`，前端藏，不改 SQL

每个 PR 合并前：

- [ ] 没有新的装饰性 border / 左边框 / 文字压图 / 灰色封面占位
- [ ] 4×2 不是上下瘦条
- [ ] 字号乘 `fontScale`，间距乘 `scale`
- [ ] 游客路径没有未读点、没有 markRead、没有收藏 Tab
- [ ] 纯函数有单测（PR 2+）
- [ ] 没有为磁贴新增 REST 资源

不要做：

- 重写 `BrewReader` 或 `ItemCard`
- 把坐标写入数据库
- 新建 numbered migration（改 `003` + `tables_brew.rs`）
- 引入 Redux/Zustand
- 把主题名写死在组件里（走 i18n）
- 在组件里写第二套窄屏判断
