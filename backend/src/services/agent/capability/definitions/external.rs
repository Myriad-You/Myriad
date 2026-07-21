//! 外部集成能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // HTTP 请求
    registry.register(Capability {
        id: "http.fetch".to_string(),
        name: "HTTP 请求".to_string(),
        description: "发起外部 HTTP 请求".to_string(),
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
        ..Default::default()
    });

    // 一言
    registry.register(Capability {
        id: "hitokoto.get".to_string(),
        name: "获取一言".to_string(),
        description: "获取随机一言/语录".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "type": { "type": "string", "description": "类型：动画、漫画、游戏等" }
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
        name: "RSSHub 实例列表".to_string(),
        description: "获取 RSSHub 实例列表及健康状态（与 Brew 设置同源）".to_string(),
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
        name: "RSSHub 健康检查".to_string(),
        description: "对已配置的 RSSHub 实例做实时健康检查（可选 instanceId）".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Monitor],
        input_schema: json!({
            "type": "object",
            "properties": {
                "instanceId": { "type": "integer", "description": "可选；省略则检查全部已配置实例" }
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
        name: "Notion 数据查询".to_string(),
        description: "查询 Notion 数据库内容".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "databaseId": { "type": "string" },
                "filter": { "type": "object" },
                "sort": { "type": "object" }
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
        name: "网易云歌曲详情".to_string(),
        description: "查询网易云音乐歌曲详细信息，可包含歌词".to_string(),
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
        name: "网易云歌单详情".to_string(),
        description: "获取网易云歌单的详细信息和歌曲列表".to_string(),
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
        name: "图片代理".to_string(),
        description: "代理获取外链图片（绕过防盗链）".to_string(),
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
                "proxyUrl": { "type": "string" },
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
        name: "天气查询".to_string(),
        description: "获取指定城市的天气信息".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "temperature": { "type": "number" },
                "weather": { "type": "string" },
                "humidity": { "type": "number" }
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
        name: "时间信息".to_string(),
        description: "获取当前时间、节假日等信息".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "timezone": { "type": "string" },
                "format": { "type": "string" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "datetime": { "type": "string" },
                "timezone": { "type": "string" },
                "weekday": { "type": "string" },
                "isHoliday": { "type": "boolean" }
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
        name: "番剧数据库查询".to_string(),
        description: "查询预置番剧/电视剧/电影数据库".to_string(),
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
        name: "游戏数据库查询".to_string(),
        description: "查询预置游戏数据库".to_string(),
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
        name: "艺术家数据库查询".to_string(),
        description: "查询预置歌手/艺术家数据库".to_string(),
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
        name: "元数据历史".to_string(),
        description: "查询平台元数据变化历史".to_string(),
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
                "history": { "type": "array" },
                "changes": { "type": "array" }
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
        name: "用户画像".to_string(),
        description: "获取用户跨平台综合画像".to_string(),
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
                "interests": { "type": "array" },
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
        name: "随机内容".to_string(),
        description: "从平台数据中随机推荐内容".to_string(),
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
        name: "网页内容抓取".to_string(),
        description: "抓取外部网页并提取可读文本，支持 CSS 选择器。可与 ai.summarize 联用实现「读这个网页并总结」".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "要抓取的网页 URL" },
                "selector": { "type": "string", "description": "CSS 选择器，默认 body" },
                "max_length": { "type": "integer", "description": "最大返回字符数，默认 5000" }
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
