---
name: 最新订阅文章
description: 跨所有订阅源取最近文章列表（本地 Brew），不使用 webSearch
category: brew
triggers:
  - 最新文章
  - 最近文章
  - 看看订阅
  - 订阅更新
  - latest posts
  - latest articles
  - recent posts
  - recent articles
  - what's new
  - 有什么新的
  - 未读文章
  - unread
  - 今天更新
tier_hint: standard
origin: manual
gating:
  capabilities:
    - brew.items
    - brew.page
    - brew.sources
---

# 目标

汇总用户**所有本地订阅源**中最近的文章，快速回答「有什么新文章 / 订阅更新了什么」。

# 硬性约束

1. **禁止**使用 `ai.webSearch`。这是纯本地阅读列表，不是新闻搜索。
2. 不要按站外关键词去搜互联网；关键词筛选仅用于本地 `brew.items` 的 `keyword`（若用户指定主题）。
3. 跨源列表时**不要**伪造 `sourceId`；无源过滤即可。

# 推荐步骤

## 步骤 1：拉取最近文章

调用 `brew.items`：

```json
{
  "limit": 15
}
```

说明：

- 不传 `sourceId` / `sourceName`，即跨所有源按发布时间倒序。
- 若用户要未读：`"unreadOnly": true`。
- 若用户限定数量（「5 篇」），设置对应 `limit`。
- 若用户说「最新/最近/latest/recent」类泛化词，按通用最新请求处理，不要把这些词当成源名过滤。

可选：`brew.page` + `level: "items"` 仅在已有 `sourceId` 时使用；跨源场景优先 `brew.items`。

## 步骤 2（可选）：需要源目录时

仅当用户同时问「我订了哪些」时，再调 `brew.sources`（`action: "list"`）。平时拉最新文章不必先 list 源。

## 步骤 3：展示

按时间倒序展示：标题、源名称（`_feedTitle` 等字段）、发布时间、链接。保持简洁；用户要总结时再另开 `ai.summarize`（不在本 Skill 默认路径）。

# 与相近 Skill 的区分

| 用户意图 | Skill |
|----------|--------|
| 友链目录 | `brew-friend-links` |
| 某个具体源的最新 | `brew-source-latest`（需 `source_name` → `sourceId`） |
| 全局最近文章 | **本 Skill** |
