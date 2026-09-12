//! 外部集成能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // HTTP 请求
    registry.register(Capability {
        id: "http.fetch".to_string(),
        name: "HTTP request".to_string(),
        description: "Make an outbound HTTP request.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "method": { "type": "string", "enum": ["GET", "POST"] },
                "headers": { "type": "object" },
                "body": { "type": "any" }
            },
            "required": ["url"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "status": { "type": "integer" },
                "data": { "type": "any" }
            }
        }),
        required_permissions: vec!["http:fetch".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(2000),
        requires_confirmation: true,
        confirmation_message: Some("This will send an HTTP request to an external URL".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // 一言
    registry.register(Capability {
        id: "hitokoto.get".to_string(),
        name: "Hitokoto".to_string(),
        description: "Get a random quote.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "type": { "type": "string", "description": "Type: anime, manga, games, etc." }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "content": { "type": "string" },
                "from": { "type": "string" },
                "type": { "type": "string" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(1000),
        ..Default::default()
    });

    // RSSHub 实例列表（brew rsshub_instances 表，与 Brew UI 同源）
    registry.register(Capability {
        id: "rsshub.instances".to_string(),
        name: "RSSHub instances".to_string(),
        description: "List RSSHub instances and their health.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "instances": { "type": "array" },
                "healthyCount": { "type": "integer" }
            }
        }),
        required_permissions: vec!["rsshub:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(300),
        ..Default::default()
    });

    // RSSHub 健康检查（对已配置实例探测，非硬编码公共 URL）
    registry.register(Capability {
        id: "rsshub.healthcheck".to_string(),
        name: "RSSHub health".to_string(),
        description: "Check configured RSSHub instances.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Monitor],
        input_schema: json!({
            "type": "object",
            "properties": {
                "instanceId": { "type": "integer", "description": "Optional; omit to check every configured instance" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "instances": { "type": "array" },
                "healthyCount": { "type": "integer" },
                "checkedAt": { "type": "string" }
            }
        }),
        required_permissions: vec!["rsshub:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(5000),
        ..Default::default()
    });

    // Notion 数据查询
    registry.register(Capability {
        id: "notion.query".to_string(),
        name: "Query Notion".to_string(),
        description: "Query a Notion database.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "databaseId": { "type": "string" },
                "filter": { "type": "object" }
            },
            "required": ["databaseId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" },
                "hasMore": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["notion:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 网易云歌曲详情
    registry.register(Capability {
        id: "netease.song".to_string(),
        name: "NetEase song".to_string(),
        description: "Read a NetEase song, optionally with lyrics.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "songId": { "type": "integer" },
                "includeLyrics": { "type": "boolean", "default": false }
            },
            "required": ["songId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "song": { "type": "object" },
                "lyrics": { "type": "string" }
            }
        }),
        required_permissions: vec!["netease:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 网易云歌单详情
    registry.register(Capability {
        id: "netease.playlist.detail".to_string(),
        name: "NetEase playlist".to_string(),
        description: "Read a NetEase playlist and its tracks.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "playlistId": { "type": "integer" }
            },
            "required": ["playlistId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "trackCount": { "type": "integer" },
                "tracks": { "type": "array" }
            }
        }),
        required_permissions: vec!["netease:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 图片代理
    registry.register(Capability {
        id: "proxy.image".to_string(),
        name: "Image proxy".to_string(),
        description: "Fetch an external image through the image proxy.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "platform": { "type": "string" }
            },
            "required": ["url"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "originalUrl": { "type": "string" },
                "proxyUrl": { "type": "string" },
                "platform": { "type": "string" },
                "cached": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["proxy:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        ..Default::default()
    });

    // 天气查询
    registry.register(Capability {
        id: "weather.get".to_string(),
        name: "Weather".to_string(),
        description: "Get weather for a city.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "city": { "type": "string", "description": "City name (location / q also accepted)" },
                "location": { "type": "string", "description": "Alias of city" }
            },
            "required": ["city"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" },
                "temperature": { "type": "string" },
                "weather": { "type": "string" },
                "humidity": { "type": "string" }
            }
        }),
        required_permissions: vec!["weather:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(1000),
        ..Default::default()
    });

    // 时间信息
    registry.register(Capability {
        id: "time.info".to_string(),
        name: "Time".to_string(),
        description: "Convert the current time across time zones.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "timezone": {
                    "type": "string",
                    "description": "IANA (Asia/Shanghai), UTC, local, or +08:00 / UTC+8"
                }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "datetime": { "type": "string" },
                "timestamp": { "type": "integer" },
                "timezone": { "type": "string" },
                "weekday": { "type": "string" },
                "year": { "type": "integer" },
                "month": { "type": "integer" },
                "day": { "type": "integer" },
                "hour": { "type": "integer" },
                "minute": { "type": "integer" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(10),
        ..Default::default()
    });

    // 内容数据库 - 番剧
    registry.register(Capability {
        id: "database.anime".to_string(),
        name: "Anime database".to_string(),
        description: "Query the built-in anime and film database.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "genre": { "type": "string" },
                "category": { "type": "string", "enum": ["anime", "tv", "movie"] }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" },
                "count": { "type": "integer" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 内容数据库 - 游戏
    registry.register(Capability {
        id: "database.game".to_string(),
        name: "Game database".to_string(),
        description: "Query the built-in game database.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "genre": { "type": "string" },
                "platform": { "type": "string" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" },
                "count": { "type": "integer" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 内容数据库 - 艺术家
    registry.register(Capability {
        id: "database.artist".to_string(),
        name: "Artist database".to_string(),
        description: "Query the built-in artist database.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "genre": { "type": "string" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" },
                "count": { "type": "integer" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 元数据历史
    registry.register(Capability {
        id: "metadata.history".to_string(),
        name: "Metadata history".to_string(),
        description: "Read platform metadata history.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "limit": { "type": "integer", "default": 10 }
            },
            "required": ["platform"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "history": { "type": "array" },
                "limit": { "type": "integer" },
                "note": { "type": "string" }
            }
        }),
        required_permissions: vec!["metadata:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        ..Default::default()
    });

    // 用户画像
    registry.register(Capability {
        id: "profile.summary".to_string(),
        name: "Profile summary".to_string(),
        description: "Read a cross-platform profile summary.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platforms": { "type": "array", "items": { "type": "string" } }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "activities": { "type": "array" },
                "platformStats": { "type": "object" }
            }
        }),
        required_permissions: vec!["profile:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        ..Default::default()
    });

    // 随机内容
    registry.register(Capability {
        id: "random.content".to_string(),
        name: "Random content".to_string(),
        description: "Pick random items from platform data.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Recommend],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "count": { "type": "integer", "default": 5 }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "items": { "type": "array" },
                "platform": { "type": "string" }
            }
        }),
        required_permissions: vec!["random:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 网页抓取
    registry.register(Capability {
        id: "web.scrape".to_string(),
        name: "Fetch page".to_string(),
        description: "Fetch a web page and extract readable text.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Page URL to scrape" },
                "selector": { "type": "string", "description": "CSS selector, default body" },
                "max_length": { "type": "integer", "description": "Max characters to return, default 5000" }
            },
            "required": ["url"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "title": { "type": "string" },
                "content": { "type": "string" },
                "length": { "type": "integer" },
                "truncated": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["web:scrape".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(5000),
        ..Default::default()
    });
}
