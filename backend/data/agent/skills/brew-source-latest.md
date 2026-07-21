---
name: 某订阅源最新文章
description: 按订阅源名称 brew.sources 宽松匹配定位 sourceId，再 brew.items 取最新文章；本地查询，禁止纯列表联网
category: brew
triggers:
  - 订阅源
  - 这个源
  - 这个站
  - 看看
  - latest posts
  - latest from
  - posts from
  - 最新文章
  - 最近更新
  - 源的文章
  - source latest
parameters:
  - source_name
tier_hint: standard
origin: manual
gating:
  capabilities:
    - brew.sources
    - brew.items
    - brew.page
---

# 目标

用户指定一个订阅源名称（参数 `${source_name}`），先在本地 `brew.sources` 宽松匹配，再列出该源最新文章。

# 硬性约束

1. **必须先 sources 再 items**：先从 `brew.sources` 拿到真实整数 `id`，再调用 `brew.items` 的 **`sourceId`**。
2. **禁止**在「本地有该源 / 有匹配文章」时使用 `ai.webSearch`。默认路径全程本地。
3. 禁止编造 `sourceId`；禁止把源名字符串塞进 `sourceId`。
4. `brew.items` **优先传 `sourceId`**（来自 sources 步骤）；不要只靠 `sourceName` 当唯一路径。

# 推荐步骤

## 步骤 1：按名称匹配订阅源

调用 `brew.sources`：

```json
{
  "query": "${source_name}"
}
```

也可用 `name` / `keyword`（与 `query` 同义）。`action: "list"` 可选。

### 匹配结果（#197 真实响应）

- `matched: true` + `sources[]`：每项可有 `matchKind`：`exact` | `contains` | `fuzzy`
  - 列表已按匹配强度排序；**优先取 `exact`，其次 `contains`，再 `fuzzy`**
  - 恰好 1 个高置信结果，或 top 为 `exact`/`contains` 且远强于其余 → 用该源 **`id`** 作为 `sourceId`
  - 多个接近结果 → 列出候选项名称让用户确认，不要瞎选
- `matched: false` / `notFound: true`：`sources` 为空；可展示 `suggestions`；看 `totalInSystem`
  - **不要**自动 webSearch
  - 若 `totalInSystem == 0`，说明系统无订阅源，而非名称拼错

## 步骤 2：用 sourceId 取最新文章

调用 `brew.items`，**必须**传步骤 1 的整数 id：

```json
{
  "sourceId": <from step 1 sources[0].id>,
  "limit": 10
}
```

- 用步骤依赖 + `xxxFrom` 注入 id，不要手写猜测 ID。
- `limit` 默认 10；用户说「多一点/少一点」时调整。
- 引擎侧：显式 `sourceId` 优先于 name/author 宽松过滤。
- 可选：`brew.page` 且 `level: "items"` + 同一 `sourceId`。

## 步骤 3：展示

标题、发布时间、来源名、链接。不要默认 `ai.summarize`，除非用户明确要求总结。

# 参数

| 槽位 | 含义 |
|------|------|
| `${source_name}` | 用户提到的订阅源/站点/友链名称 |

Planner 调用时应传入 `source_name`（如「看看 akiday」→ `akiday`）。
