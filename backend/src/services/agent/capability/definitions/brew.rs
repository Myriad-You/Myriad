//! Brew 阅读系统能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // Brew 内容读取
    registry.register(Capability {
        id: "brew.read".to_string(),
        name: "Brew 内容读取".to_string(),
        description: "读取 Brew RSS 订阅内容".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "source": { "type": "string" },
                "limit": { "type": "integer", "default": 50 },
                "since": { "type": "string", "format": "date-time" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "items": { "type": "array" },
                "lastUpdated": { "type": "string" }
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
        description: "从数据库列出/查找 Brew 订阅源（含友情链接）。支持按名称宽松匹配（exact/contains/fuzzy）。用户说「看看 X」「X 是什么订阅」时先用本能力定位 source，再把 sourceId 传给 brew.items；有本地条目时不要改走 ai.webSearch。".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "按名称/URL/分类/描述宽松匹配" },
                "name": { "type": "string", "description": "同 query，按名称查找" },
                "keyword": { "type": "string", "description": "同 query" },
                "category": { "type": "string", "description": "按分类筛选" },
                "sourceType": { "type": "string", "enum": ["rss", "link", "brewlia"], "description": "来源类型" },
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

    // 阅读统计
    registry.register(Capability {
        id: "brew.stats".to_string(),
        name: "阅读统计".to_string(),
        description: "获取 Brew 阅读统计数据".to_string(),
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

    // 文章内容
    registry.register(Capability {
        id: "brew.article".to_string(),
        name: "文章内容".to_string(),
        description: "获取单篇 Brew 文章的完整内容（仅在已知ID或URL的情况下使用）".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![
            IntentAction::Query,
            IntentAction::Summarize,
            IntentAction::Analyze,
        ],
        input_schema: json!({
            "type": "object",
            "properties": {
                "articleId": { "type": "string", "description": "文章 ID" },
                "url": { "type": "string", "description": "文章原始链接" },
                "sourceId": { "type": "integer", "description": "订阅源 ID" }
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
        required_permissions: vec!["brew:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(5000),
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

    // 生成阅读列表
    registry.register(Capability {
        id: "brew.generateReadingList".to_string(),
        name: "生成阅读列表".to_string(),
        description: "根据用户需求由 AI 筛选并生成符合条件的阅读列表。支持按主题、关键词、时间范围等条件筛选。".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Recommend],
        input_schema: json!({
            "type": "object",
            "properties": {
                "criteria": { "type": "string", "description": "用户的筛选条件/需求描述" },
                "maxItems": { "type": "integer", "default": 10, "description": "列表最大文章数" },
                "sourceName": { "type": "string", "description": "限定特定订阅源" },
                "daysBack": { "type": "integer", "default": 7, "description": "查看最近多少天的文章" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "readingList": { "type": "array" },
                "totalMatched": { "type": "integer" },
                "listName": { "type": "string" },
                "criteria": { "type": "string" }
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
        description: "读取 Brew 信息聚合页面的详细内容，包括订阅源列表、文章列表、文章详情等层级".to_string(),
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
        ..Default::default()
    });
}
