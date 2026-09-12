//! Brew 阅读系统能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // Brew 内容读取（读 DB brew_items + brew_sources）
    registry.register(Capability {
        id: "brew.read".to_string(),
        name: "Read feeds".to_string(),
        description: "Read Brew articles from the database.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "source": { "type": "string", "description": "Feed id (numeric string) or name; same as sourceId/sourceName" },
                "sourceId": { "type": "integer", "description": "Feed id" },
                "sourceName": { "type": "string", "description": "Feed name (loose contains match)" },
                "limit": { "type": "integer", "default": 50 },
                "since": { "type": "string", "format": "date-time", "description": "RFC3339; only items published after this" }
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
        name: "Feed list".to_string(),
        description: "List or find Brew feeds.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Loose match on name / URL / category / description" },
                "name": { "type": "string", "description": "Same as query, match by name" },
                "keyword": { "type": "string", "description": "Same as query" },
                "category": { "type": "string", "description": "Filter by category; aliases friends→友情链接, mine→我; comma-separated tokens" },
                "sourceType": { "type": "string", "description": "Source type: rss|link|brewlia; friendlink/友链/友情链接 count as link" },
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
        name: "Article list".to_string(),
        description: "List articles from a feed.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Navigate, IntentAction::Summarize, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "sourceId": { "type": "integer", "description": "Feed id (prefer the id returned by brew.sources)" },
                "sourceName": { "type": "string", "description": "Feed name (loose match)" },
                "name": { "type": "string", "description": "Same as sourceName/query" },
                "query": { "type": "string", "description": "Filter by source name / author / URL" },
                "limit": { "type": "integer", "default": 20, "description": "How many to return" },
                "unreadOnly": { "type": "boolean", "default": false },
                "starred": { "type": "boolean" },
                "keyword": { "type": "string", "description": "Filter by keyword" },
                "selectFirst": { "type": "boolean", "description": "Navigate to the first match" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "items": { "type": "array" },
                "total": { "type": "integer" },
                "navigation": { "type": "object", "description": "Navigation when selectFirst=true" }
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
        name: "Reading stats".to_string(),
        description: "Show Brew reading stats.".to_string(),
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
        name: "Article".to_string(),
        description: "Load one Brew article by id, guid, or URL.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![
            IntentAction::Query,
            IntentAction::Summarize,
            IntentAction::Analyze,
        ],
        input_schema: json!({
            "type": "object",
            "properties": {
                "articleId": { "type": ["string", "integer"], "description": "Article id (i32) or guid/link" },
                "itemId": { "type": ["string", "integer"], "description": "Same as articleId" },
                "id": { "type": ["string", "integer"], "description": "Same as articleId" },
                "url": { "type": "string", "description": "Original article URL" },
                "link": { "type": "string", "description": "Same as url" },
                "sourceId": { "type": "integer", "description": "Optional feed id to narrow the match" }
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
        name: "Discover feeds".to_string(),
        description: "Find RSS/Atom feeds by URL or keyword.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "RSS/Atom feed URL or site URL" },
                "query": { "type": "string", "description": "Keyword to find common feeds" }
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
                "suggestions": { "type": "array", "description": "Suggested feeds when there is no exact match" }
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
        name: "Subscribe".to_string(),
        description: "Add an RSS/Atom feed.".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "description": "Feed URL" },
                "name": { "type": "string", "description": "Optional custom name; default comes from the feed" },
                "category": { "type": "string", "description": "Category folder" },
                "updateInterval": { "type": "integer", "description": "Refresh interval in minutes", "default": 30 }
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
        confirmation_message: Some("This will add a new RSS/Atom feed".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // 标记文章状态（个人已读/收藏，不修改共享订阅库）
    // 权限用 brew:read：共享库下普通用户只读订阅源，但仍可维护自己的阅读状态
    registry.register(Capability {
        id: "brew.mark".to_string(),
        name: "Mark articles".to_string(),
        description: "Mark articles read, unread, saved, or later.".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "itemId": { "type": "integer", "description": "Article id" },
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
        name: "Reading list".to_string(),
        description: "Build a reading list from local feeds.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Recommend],
        input_schema: json!({
            "type": "object",
            "properties": {
                "criteria": { "type": "string", "description": "What the user wants in the list" },
                "keyword": { "type": "string", "description": "Local title/body keyword" },
                "topic": { "type": "string", "description": "Same as keyword" },
                "query": { "type": "string", "description": "Same as keyword" },
                "maxItems": { "type": "integer", "default": 10, "description": "Max articles in the list" },
                "sourceName": { "type": "string", "description": "Limit to one feed" },
                "daysBack": { "type": "integer", "default": 7, "description": "Look back this many days" },
                "allowWebSearch": { "type": "boolean", "default": false, "description": "Allow a web search when local results are empty; default false. Outbound still needs granted ai:search" },
                "useWebSearch": { "type": "boolean", "description": "Same as allowWebSearch" },
                "webSearch": { "type": "boolean", "description": "Same as allowWebSearch" }
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
        name: "Brew page".to_string(),
        description: "Read Brew page content, including feed lists.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "level": { 
                    "type": "string", 
                    "enum": ["sources", "items", "detail"],
                    "description": "Page level: sources=feed list, items=article list, detail=article"
                },
                "sourceId": { "type": "integer", "description": "Feed id (required at items/detail)" },
                "itemId": { "type": "string", "description": "Article id (required at detail)" },
                "filter": { 
                    "type": "string", 
                    "enum": ["all", "unread", "starred", "today"],
                    "description": "Filter"
                },
                "category": {
                    "type": "string",
                    "description": "When level=sources, filter by category; friends/友链→友情链接"
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
                        "source": { "type": "object", "description": "Current feed" },
                        "item": { "type": "object", "description": "Current article" }
                    }
                },
                "content": {
                    "type": "object",
                    "properties": {
                        "sources": { "type": "array", "description": "Feed list" },
                        "items": { "type": "array", "description": "Article list" },
                        "detail": { "type": "object", "description": "Article detail" }
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
        name: "Brew schedule".to_string(),
        description: "Start, stop, or refresh the Brew scheduler.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Update, IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["start", "stop", "refresh", "status"] },
                "sourceId": { "type": "integer" }
            },
            "required": ["action"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "action": { "type": "string" },
                "status": { "type": "string" },
                "attempted": { "type": "integer" },
                "refreshed": { "type": "integer" },
                "failed": { "type": "integer" },
                "newItems": { "type": "integer" },
                "sourceId": { "type": "integer" },
                "running": { "type": "boolean" },
                "available": { "type": "boolean" },
                "message": { "type": "string" }
            }
        }),
        required_permissions: vec!["brew:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(30000),
        requires_confirmation: true,
        confirmation_message: Some(
            "This will start, stop, or refresh the Brew scheduler".to_string(),
        ),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use crate::services::agent::capability::CapabilityRegistry;

    #[test]
    fn add_subscription_source_requires_brew_manage() {
        // brew.subscribe → POST /api/brew/sources：privileged brew:manage，不是 brew:write。
        let registry = CapabilityRegistry::new();
        let capability = registry
            .get("brew.subscribe")
            .expect("brew.subscribe capability must be registered");
        assert_eq!(capability.required_permissions, vec!["brew:manage"]);
    }
}
