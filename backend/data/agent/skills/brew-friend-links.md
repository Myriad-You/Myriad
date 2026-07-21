---
name: Brew 友情链接
description: 列出 Brew 中友情链接分类的订阅源（友链目录），纯本地 brew.sources 查询，不联网
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

展示用户在 Brew 中的**友情链接**订阅源列表（名称、链接、图标、简介等）。这是本地订阅库查询，不是全网搜索。

# 硬性约束

1. **禁止**使用 `ai.webSearch`（以及任何联网搜索）。友链只存在于本地 `brew_sources`。
2. 只使用 gating 中声明的能力 + 必要时的轻量展示说明。
3. 不要编造源；若 `sources` 为空且 `totalInSystem > 0`，说明「当前没有友情链接」；若 `totalInSystem == 0`，说明系统尚无订阅源。

# 推荐步骤

## 步骤 1：按分类列出源

调用 `brew.sources`（真实 DB 列表，支持分类别名归一化）：

```json
{
  "action": "list",
  "category": "友情链接"
}
```

### 分类参数约定（与 #197 API 一致）

- 推荐写 **`category: "友情链接"`**（DB 预置 token）。
- 别名也可：`friends` / `friend` / `friendlink` / `友链` — 服务端会归一化为 `友情链接`。
- 多分类字段按 **token** 匹配（如 `"友情链接, 技术"` 会命中），不是危险子串匹配。
- 旧数据：category 为空且 `sourceType=link` 的纯链接源也会被纳入友情链接列表。
- 可选再加 `"sourceType": "link"` 收窄到链接型源（`friendlink`/`友链` 等同 `link`）。

### 响应字段

- `sources[]`: `id`, `name`, `url`, `siteUrl`, `category`, `sourceType`, `feedType`, `enabled`, `itemCount`, `unreadCount`, `icon`, `description`
- `total` / `totalInSystem` / `matched`

可选补充：`brew.page`（`level: "sources"`）仅作页面级视图；**分类过滤必须以 `brew.sources` + `category` 为准**。

## 步骤 2：整理展示

简洁列表：

- 源名称
- 站点链接（优先 `siteUrl`，否则 `url`）
- `sourceType` / 描述（若有）

若用户还想看某个友链最近更新，引导 skill `brew-source-latest`：先用本结果的 **`id` 作为 `sourceId`** 传给 `brew.items`。

# 参数约定

- 本 Skill 无必填参数。
- 若用户点名某个友链，用 `brew.sources` 的 `query`/`name` 定位，**再**把返回的 `id` 作为 `sourceId` 传给 `brew.items`，切勿跳过 list 猜 ID。
