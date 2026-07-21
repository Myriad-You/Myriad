---
name: 平台状态速览
description: 用 platform.read 读取已绑定平台的缓存概览（收藏/动态/库等），本地缓存优先
category: platform
triggers:
  - 平台状态
  - 平台数据
  - platform status
  - 我的 B站
  - 我的 Steam
  - 我的 GitHub
  - 平台缓存
  - 看看平台
  - bound platforms
parameters:
  - platform
tier_hint: standard
origin: manual
gating:
  capabilities:
    - platform.read
---

# 目标

读取用户指定（或上下文中的）已绑定平台的**本地缓存数据**概览。

# 硬性约束

1. 只使用 `platform.read`，参数 `platform` 必须是真实枚举值之一：
   `bilibili` | `bangumi` | `steam` | `github` | `netease` | `x` | `discord` | `mal` | `xbox` | `psn`
2. 本 Skill **不**替代 Brew 订阅查询；订阅/友链/文章请用 brew 系列 Skill。
3. 若用户未指定平台且上下文也无法推断，先澄清，不要瞎选。
4. 缓存为空时如实说明「暂无该平台缓存数据」，可建议用户去绑定/刷新；默认不 webSearch。

# 推荐步骤

## 步骤 1：读取平台数据

```json
{
  "platform": "${platform}",
  "limit": 20
}
```

- `${platform}` 从用户话术映射（「B站」→ `bilibili`，「Steam 游戏库」→ `steam` 等）。
- 可选 `filters` / `offset` 按能力 schema 传递。

## 步骤 2：展示

返回结构化摘要：条目数、代表性字段（标题/名称/状态），避免倾倒原始 JSON 全文。

# 参数

| 槽位 | 含义 |
|------|------|
| `${platform}` | 平台 ID（见上表枚举） |
