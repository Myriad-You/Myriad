//! Brew 阅读系统能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // Brew 内容读取（读 DB brew_items + brew_sources）
    registry.register(Capability {
        id: "brew.read".to_string(),
        name: "Brew 内容读取".to_string(),
        description: "从数据库读取 Brew 订阅文章列表。支持 sourceId / sourceName / source（id 或名称）、limit、since。".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "source": { "type": "string", "description": "订阅源 ID（数字字符串）或名称；与 sourceId/sourceName 对齐" },
                "sourceId": { "type": "integer", "description": "订阅源 ID" },
                "sourceName": { "type": "string", "description": "订阅源名称（模糊包含匹配）" },
                "limit": { "type": "integer", "default": 50 },
                "since": { "type": "string", "format": "date-time", "description": "RFC3339，仅返回此后发布的文章" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "items": { "type": "array" },
                "total": { "type": "integer" },
                "lastUpdated": { "type": "string" },
                "sourceId": { "type": "integer" },
                "sourceName": { "type": "string" }
            }
        }),
        required_permissions: vec!["brew:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(150),
        ..Default::default()
    });

    // 订阅源列表（读 DB，支持宽松名称匹配）
    registry.register(Capability {
        id: "brew.sources".to_string(),
        name: "订阅源列表".to_string(),
        description: "从数据库列出/查找 Brew 订阅源（含友情链接）。支持 category 筛选（友情链接/友链/friends）与 sourceType=link|rss|brewlia；名称宽松匹配（exact/contains/fuzzy）。用户说「友情链接」「友链」时用 category=友情链接；「看看 X」时先用本能力定位 source，再把 sourceId 传给 brew.items；有本地条目时不要改走 ai.webSearch。".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "按名称/URL/分类/描述宽松匹配" },
                "name": { "type": "string", "description": "同 query，按名称查找" },
                "keyword": { "type": "string", "description": "同 query" },
                "category": { "type": "string", "description": "按分类筛选；支持前端别名 friends→友情链接、mine→我；多分类逗号分隔按 token 匹配" },
                "sourceType": { "type": "string", "description": "来源类型: rss|link|brewlia；friendlink/友链/友情链接 视为 link" },
                "action": { "type": "string", "enum": ["list", "add", "refresh"] },
                "url": { "type": "string" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "sources": { "type": "array" },
                "total": { "type": "integer" },
                "totalInSystem": { "type": "integer" },
                "matched": { "type": "boolean" },
                "searchedFor": { "type": "string" },
                "matchKind": { "type": "string" },
                "suggestions": { "type": "array" }
            }
        }),
        required_permissions: vec!["brew:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        ..Default::default()
    });

    // 文章列表
    registry.register(Capability {
        id: "brew.items".to_string(),
        name: "文章列表".to_string(),
        description: "获取订阅源中的文章列表。优先传 sourceId（来自 brew.sources）；也支持 sourceName/name/query 宽松匹配订阅源名。用户说「看看 X」且本地有该源文章时用本能力，不要 webSearch。".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Navigate, IntentAction::Summarize, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "sourceId": { "type": "integer", "description": "订阅源ID（推荐：由 brew.sources 返回后传入）" },
                "sourceName": { "type": "string", "description": "订阅源名称（宽松匹配）" },
                "name": { "type": "string", "description": "同 sourceName/query" },
                "query": { "type": "string", "description": "按源名/作者/URL 筛选" },
                "limit": { "type": "integer", "default": 20, "description": "返回数量" },
                "unreadOnly": { "type": "boolean", "default": false },
                "starred": { "type": "boolean" },
                "keyword": { "type": "string", "description": "按关键词筛选" },
                "selectFirst": { "type": "boolean", "description": "是否选择第一条进行导航" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "items": { "type": "array" },
                "total": { "type": "integer" },
                "navigation": { "type": "object", "description": "导航信息（如果 selectFirst=true）" }
            }
        }),
        required_permissions: vec!["brew:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(300),
        ..Default::default()
    });

    // 阅读统计（聚合 brew_sources / brew_items / brew_user_states）
    registry.register(Capability {
        id: "brew.stats".to_string(),
        name: "阅读统计".to_string(),
        description: "从数据库聚合 Brew 阅读统计：订阅源数、文章数、当前用户未读/收藏数。"
            .to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "totalSources": { "type": "integer" },
                "totalItems": { "type": "integer" },
                "unreadCount": { "type": "integer" },
                "starredCount": { "type": "integer" }
            }
        }),
        required_permissions: vec!["brew:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 文章内容（读 DB brew_items）
    registry.register(Capability {
        id: "brew.article".to_string(),
        name: "文章内容".to_string(),
        description: "获取单篇 Brew 文章完整内容。按 item id（整数或字符串）、guid 或 url/link 从 brew_items 解析。".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![
            IntentAction::Query,
            IntentAction::Summarize,
            IntentAction::Analyze,
        ],
        input_schema: json!({
            "type": "object",
            "properties": {
                "articleId": { "type": ["string", "integer"], "description": "文章 ID（i32）或 guid/link" },
                "itemId": { "type": ["string", "integer"], "description": "同 articleId" },
                "id": { "type": ["string", "integer"], "description": "同 articleId" },
                "url": { "type": "string", "description": "文章原始链接" },
                "link": { "type": "string", "description": "同 url" },
                "sourceId": { "type": "integer", "description": "订阅源 ID（可选，缩小匹配范围）" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "content": { "type": "string" },
                "plainText": { "type": "string" },
                "author": { "type": "string" },
                "publishedAt": { "type": "string" },
                "sourceUrl": { "type": "string" },
                "sourceName": { "type": "string" }
            }
        }),
        required_permissions: vec!["brew:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        ..Default::default()
    });

    // 订阅源发现
    registry.register(Capability {
        id: "brew.discover".to_string(),
        name: "订阅源发现".to_string(),
        description: "通过 URL 或关键词发现 RSS/Atom 订阅源".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "RSS/Atom 源 URL 或网站 URL" },
                "query": { "type": "string", "description": "搜索关键词，用于查找常见源" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "found": { "type": "boolean" },
                "feeds": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "url": { "type": "string" },
                            "title": { "type": "string" },
                            "description": { "type": "string" },
                            "feedType": { "type": "string" },
                            "itemCount": { "type": "integer" }
                        }
                    }
                },
                "suggestions": { "type": "array", "description": "如果未找到精确匹配，提供建议的源" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 添加订阅
    registry.register(Capability {
        id: "brew.subscribe".to_string(),
        name: "添加订阅".to_string(),
        description: "添加新的 RSS/Atom 订阅源".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "description": "订阅源 URL" },
                "name": { "type": "string", "description": "自定义名称（可选，默认从源获取）" },
                "category": { "type": "string", "description": "分类文件夹" },
                "updateInterval": { "type": "integer", "description": "更新间隔（分钟）", "default": 30 }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "sourceId": { "type": "integer" },
                "name": { "type": "string" },
                "itemCount": { "type": "integer" }
            }
        }),
        required_permissions: vec!["brew:manage".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(5000),
        requires_confirmation: true,
        confirmation_message: Some("即将添加新的 RSS/Atom 订阅源".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // 标记文章状态（个人已读/收藏，不修改共享订阅库）
    // 权限用 brew:read：共享库下普通用户只读订阅源，但仍可维护自己的阅读状态
    registry.register(Capability {
        id: "brew.mark".to_string(),
        name: "标记文章状态".to_string(),
        description: "标记 Brew 文章为已读/未读/收藏/稍后阅读（仅当前用户的个人状态）".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "itemId": { "type": "integer", "description": "文章 ID" },
                "action": { "type": "string", "enum": ["read", "unread", "star", "unstar", "later"] }
            },
            "required": ["itemId", "action"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "status": { "type": "string" }
            }
        }),
        required_permissions: vec!["brew:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 生成阅读列表（默认仅本地 brew_items；联网需 allowWebSearch=true）
    registry.register(Capability {
        id: "brew.generateReadingList".to_string(),
        name: "生成阅读列表".to_string(),
        description: "根据用户需求从本地订阅筛选生成阅读列表。本地关键词无匹配时返回诚实空结果与建议，不会自动强制 ai.webSearch；仅当 allowWebSearch=true（或 useWebSearch/webSearch）时才联网补充。".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Recommend],
        input_schema: json!({
            "type": "object",
            "properties": {
                "criteria": { "type": "string", "description": "用户的筛选条件/需求描述" },
                "keyword": { "type": "string", "description": "本地标题/正文关键词" },
                "topic": { "type": "string", "description": "同 keyword" },
                "query": { "type": "string", "description": "同 keyword" },
                "maxItems": { "type": "integer", "default": 10, "description": "列表最大文章数" },
                "sourceName": { "type": "string", "description": "限定特定订阅源" },
                "daysBack": { "type": "integer", "default": 7, "description": "查看最近多少天的文章" },
                "allowWebSearch": { "type": "boolean", "default": false, "description": "显式允许本地无结果时联网搜索；默认 false" },
                "useWebSearch": { "type": "boolean", "description": "同 allowWebSearch" },
                "webSearch": { "type": "boolean", "description": "同 allowWebSearch" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "readingList": { "type": "array" },
                "totalMatched": { "type": "integer" },
                "listName": { "type": "string" },
                "criteria": { "type": "string" },
                "fromWebSearch": { "type": "boolean" },
                "allowWebSearch": { "type": "boolean" },
                "notFound": { "type": "boolean" },
                "suggestions": { "type": "array" },
                "availableSources": { "type": "array" }
            }
        }),
        required_permissions: vec!["brew:read".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(60000),
        ..Default::default()
    });

    // Brew 页面内容
    registry.register(Capability {
        id: "brew.page".to_string(),
        name: "Brew 页面内容".to_string(),
        description: "读取 Brew 信息聚合页面的详细内容。level=sources 返回订阅源列表（含 sourceType/category/siteUrl），可用 category 筛选友情链接等分类。".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "level": { 
                    "type": "string", 
                    "enum": ["sources", "items", "detail"],
                    "description": "页面层级: sources=订阅源列表, items=文章列表, detail=文章详情"
                },
                "sourceId": { "type": "integer", "description": "订阅源 ID（items/detail 层级需要）" },
                "itemId": { "type": "string", "description": "文章 ID（detail 层级需要）" },
                "filter": { 
                    "type": "string", 
                    "enum": ["all", "unread", "starred", "today"],
                    "description": "筛选条件"
                },
                "category": {
                    "type": "string",
                    "description": "level=sources 时按分类筛选；支持 friends/友链→友情链接"
                },
                "limit": { "type": "integer", "default": 20 }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "level": { "type": "string" },
                "hierarchy": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "object", "description": "当前订阅源信息" },
                        "item": { "type": "object", "description": "当前文章信息" }
                    }
                },
                "content": {
                    "type": "object",
                    "properties": {
                        "sources": { "type": "array", "description": "订阅源列表" },
                        "items": { "type": "array", "description": "文章列表" },
                        "detail": { "type": "object", "description": "文章详情" }
                    }
                },
                "stats": {
                    "type": "object",
                    "properties": {
                        "totalSources": { "type": "integer" },
                        "totalItems": { "type": "integer" },
                        "unreadCount": { "type": "integer" },
                        "starredCount": { "type": "integer" }
                    }
                },
                "navigation": {
                    "type": "object",
                    "properties": {
                        "currentFilter": { "type": "string" },
                        "availableFilters": { "type": "array" },
                        "canGoBack": { "type": "boolean" },
                        "parentPath": { "type": "string" }
                    }
                }
            }
        }),
        required_permissions: vec!["brew:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(150),
        ..Default::default()
    });

    // Brew 调度控制
    registry.register(Capability {
        id: "brew.schedule".to_string(),
        name: "Brew 调度控制".to_string(),
        description: "控制 Brew 订阅调度器".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Update, IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["start", "stop", "refresh"] },
                "sourceId": { "type": "integer" }
            },
            "required": ["action"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "message": { "type": "string" }
            }
        }),
        required_permissions: vec!["brew:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(1000),
        requires_confirmation: true,
        confirmation_message: Some("即将控制 Brew 订阅调度器（启动/停止/刷新）".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use crate::services::agent::capability::CapabilityRegistry;

    #[test]
    fn add_subscription_source_requires_brew_manage() {
        // ADR 0013 / handoff：brew.subscribe 对应真实 host 路由
        // POST /api/brew/sources uses privileged brew:manage, not the user-state brew:write.
        let registry = CapabilityRegistry::new();
        let capability = registry
            .get("brew.subscribe")
            .expect("brew.subscribe capability must be registered");
        assert_eq!(capability.required_permissions, vec!["brew:manage"]);
    }
}
