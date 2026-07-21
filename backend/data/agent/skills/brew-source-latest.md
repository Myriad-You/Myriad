---
name: 某订阅源最新文章
description: 按订阅源名称定位 sourceId，再取该源最新文章；本地 Brew 查询，禁止纯列表场景联网
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

用户指定一个订阅源名称（参数 `${source_name}`），先在本地订阅源中匹配，再列出该源最新文章。

# 硬性约束

1. **必须先 list/match 再取 items**：先从 `brew.sources` 拿到真实 `sourceId`，再调用 `brew.items`。
2. **禁止**在「本地有该源」时使用 `ai.webSearch`。只有本地完全找不到源、且用户明确要查站外信息时，才可考虑联网（本 Skill 默认路径不联网）。
3. 禁止编造 `sourceId`；禁止把源名字符串直接塞进 `sourceId` 字段。
4. `sourceId` 必须是整数，来自上一步 `brew.sources` 返回的 `sources[].id`。

# 推荐步骤

## 步骤 1：按名称匹配订阅源

调用 `brew.sources`：

```json
{
  "action": "list",
  "query": "${source_name}"
}
```

也可用 `name` / `keyword` 代替 `query`（语义相同）。

处理匹配结果：

- `matched: true` 且恰好 1 个高置信结果 → 取该源 `id` 作为 `sourceId`。
- 多个结果 → 优先 exact/contains 命中；若仍歧义，向用户列出候选项名称再确认（不要瞎选）。
- `matched: false` / 空列表 → 告知未找到本地源，可展示 `suggestions`；**不要**自动 webSearch。

## 步骤 2：用 sourceId 取最新文章

调用 `brew.items`，**必须**传入步骤 1 的 `sourceId`：

```json
{
  "sourceId": <from step 1>,
  "limit": 10
}
```

- 用 `xxxFrom` 数据流把步骤 1 的 id 注入（例如依赖 `list_sources` 步骤），不要手写猜测 ID。
- `limit` 默认 10；用户说「多一点/少一点」时调整。
- 可选：`brew.page` 且 `level: "items"` + 同一 `sourceId`，用于页面层级视图。

## 步骤 3：展示

列出标题、发布时间、来源名、链接。不要在本 Skill 内自动 `ai.summarize`，除非用户明确要求总结。

# 参数

| 槽位 | 含义 |
|------|------|
| `${source_name}` | 用户提到的订阅源/站点/友链名称 |

Planner 调用本 Skill 时应传入 `source_name`（从用户话术抽取，如「看看 akiday」→ `akiday`）。
