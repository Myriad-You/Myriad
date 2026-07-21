---
name: Brew 友情链接
description: 列出 Brew 中 category 为「友情链接」的订阅源（友链目录），纯本地查询，不联网
category: brew
triggers:
  - 友情链接
  - 友链
  - friend links
  - friend link
  - 我的友链
  - list friends
  - 看看友链
  - 友链列表
  - 订阅源里的友链
tier_hint: standard
origin: manual
gating:
  capabilities:
    - brew.sources
    - brew.page
---

# 目标

展示用户在 Brew 中标记为**友情链接**分类的订阅源列表（名称、链接、图标、简介等）。这是本地订阅库查询，不是全网搜索。

# 硬性约束

1. **禁止**使用 `ai.webSearch`（以及任何联网搜索）。友链只存在于本地 `brew_sources`。
2. 只使用 gating 中声明的能力 + 必要时的轻量展示说明。
3. 不要编造源；若列表为空，如实说明「当前没有友情链接分类下的订阅源」。

# 推荐步骤

## 步骤 1：按分类列出源

调用 `brew.sources`：

```json
{
  "action": "list",
  "category": "友情链接"
}
```

- 数据库预置分类值是 **`友情链接`**（不要用英文 `friends` 作为 category 参数）。
- 返回的 `sources[]` 中每项含 `id`、`name`、`url`/`siteUrl`、`category`、`icon`、`description` 等。

可选补充：`brew.page`（`level: "sources"`）用于页面级视图，但仍应以 `brew.sources` + `category` 过滤为准。

## 步骤 2：整理展示

将结果整理为简洁列表，例如：

- 源名称
- 站点链接（优先 `siteUrl`，否则 `url`）
- 一句话描述（若有）

若用户还想「看看某个友链最近更新了什么」，引导使用 skill `brew-source-latest`（需先拿到该源的 `sourceId`）。

# 参数约定

- 本 Skill 无必填参数。
- 若用户提到具体友链名称，仍先用本 Skill 或 `brew.sources` 的 `query`/`name` 定位，**再**把返回的 `id` 作为 `sourceId` 传给 `brew.items`，切勿跳过 list 直接猜 ID。
