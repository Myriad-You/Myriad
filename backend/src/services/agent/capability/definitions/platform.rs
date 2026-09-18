//! 平台数据能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // 平台数据读取
    registry.register(Capability {
        id: "platform.read".to_string(),
        name: "Read platform data".to_string(),
        description: "Read cached platform data.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string", "enum": ["bilibili", "bangumi", "steam", "github", "youtube", "netease", "x", "discord", "mal", "xbox", "psn"] },
                "limit": { "type": "integer", "default": 100 },
                "offset": { "type": "integer", "default": 0 },
                "filters": { "type": "object" }
            },
            "required": ["platform"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "items": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["platform:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 平台统计数据
    registry.register(Capability {
        id: "platform.stats".to_string(),
        name: "Platform stats".to_string(),
        description: "Read platform statistics.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" }
            },
            "required": ["platform"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "total": { "type": "integer" },
                "distribution": { "type": "object" }
            }
        }),
        required_permissions: vec!["platform:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 平台数据写入
    registry.register(Capability {
        id: "platform.write".to_string(),
        name: "Write platform data".to_string(),
        description: "Write to platform cache.".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Create, IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "items": { "type": "array" }
            },
            "required": ["platform", "items"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "count": { "type": "integer" }
            }
        }),
        required_permissions: vec!["platform:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        requires_confirmation: true,
        confirmation_message: Some("This will change platform data.".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // 刷新平台数据
    registry.register(Capability {
        id: "platform.refresh".to_string(),
        name: "Refresh platform data".to_string(),
        description: "Refresh platform data.".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string", "enum": ["bilibili", "bangumi", "steam", "github", "youtube", "netease", "x", "discord", "mal", "xbox", "psn", "all"] }
            },
            "required": ["platform"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "message": { "type": "string" }
            }
        }),
        required_permissions: vec!["platform:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(10000),
        requires_confirmation: true,
        confirmation_message: Some(
            "This will refresh platform data and may use API quota.".to_string(),
        ),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // Bilibili 用户查询
    registry.register(Capability {
        id: "bilibili.user".to_string(),
        name: "Bilibili user".to_string(),
        description: "Read Bilibili profile, favorites, and following.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "uid": { "type": "integer" }
            },
            "required": ["uid"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "userInfo": { "type": "object" },
                "uid": { "type": "string" },
                "name": { "type": "string" }
            }
        }),
        required_permissions: vec!["bilibili:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(5000),
        ..Default::default()
    });

    // Bangumi 用户查询
    registry.register(Capability {
        id: "bangumi.user".to_string(),
        name: "Bangumi user".to_string(),
        description: "Read Bangumi profile information.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "username": { "type": "string" },
                "access_token": { "type": "string", "description": "Optional; for data that needs authorization" },
                "user_agent": { "type": "string", "default": "myriad/Myriad" }
            },
            "required": ["username"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "userInfo": { "type": "object" }
            }
        }),
        required_permissions: vec!["bangumi:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // Bangumi 收藏查询
    registry.register(Capability {
        id: "bangumi.collections".to_string(),
        name: "Bangumi collections".to_string(),
        description: "Read Bangumi collections, scores, and watch status.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "username": { "type": "string" },
                "access_token": { "type": "string", "description": "Optional; for private collections" },
                "user_agent": { "type": "string", "default": "myriad/Myriad" }
            },
            "required": ["username"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "items": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["bangumi:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(5000),
        ..Default::default()
    });

    // Steam 用户查询
    registry.register(Capability {
        id: "steam.user".to_string(),
        name: "Steam user".to_string(),
        description: "Read cached Steam data.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "userInfo": { "type": "object" },
                "source": { "type": "string" }
            }
        }),
        required_permissions: vec!["steam:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(5000),
        ..Default::default()
    });

    // GitHub 仓库查询
    registry.register(Capability {
        id: "github.repos".to_string(),
        name: "GitHub repositories".to_string(),
        description: "Read GitHub repositories and activity.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "type": { "type": "string", "enum": ["repos", "contributions", "starred"] }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "type": { "type": "string" },
                "data": { "description": "Data for repos / contributions / starred" }
            }
        }),
        required_permissions: vec!["github:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(2000),
        ..Default::default()
    });

    // 网易云歌单
    registry.register(Capability {
        id: "netease.playlist".to_string(),
        name: "NetEase playlists".to_string(),
        description: "Read NetEase playlists and listening history.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "type": { "type": "string", "enum": ["playlists", "recent", "favorites"] }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "type": { "type": "string" },
                "data": { "description": "Data for playlists / recent / favorites" },
                "playlists": { "type": "array" },
                "songs": { "type": "array" }
            }
        }),
        required_permissions: vec!["netease:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(2000),
        ..Default::default()
    });

    // Bilibili 追番查询
    registry.register(Capability {
        id: "bilibili.bangumi".to_string(),
        name: "Bilibili following".to_string(),
        description: "Read cached Bilibili following data.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "source": { "type": "string" },
                "bangumis": { "type": "array" },
                "items": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["bilibili:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // Bilibili 视频查询
    registry.register(Capability {
        id: "bilibili.video".to_string(),
        name: "Bilibili videos".to_string(),
        description: "Look up a Bilibili video by BV or AV id.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "bvid": { "type": "string" },
                "aid": { "type": "integer" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "desc": { "type": "string" },
                "owner": { "type": "object" },
                "stat": { "type": "object" }
            }
        }),
        required_permissions: vec!["bilibili:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // Steam 愿望单
    registry.register(Capability {
        id: "steam.wishlist".to_string(),
        name: "Steam wishlist".to_string(),
        description: "Read cached Steam wishlist data.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "source": { "type": "string" },
                "wishlist": { "type": "array" },
                "items": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["steam:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // Steam 游戏详情
    registry.register(Capability {
        id: "steam.game".to_string(),
        name: "Steam game details".to_string(),
        description: "Read Steam game details.".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "appId": { "type": "integer", "description": "Also accepts a string / app_id" },
                "app_id": { "type": "integer" }
            },
            "required": ["appId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "description": { "type": "string" },
                "genres": { "type": "array" },
                "price": { "type": "object" }
            }
        }),
        required_permissions: vec!["steam:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 平台连接状态
    registry.register(Capability {
        id: "platform.connection".to_string(),
        name: "Platform connection".to_string(),
        description: "Check platform connection and sync status.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "connections": { "type": "array" },
                "status": { "type": "string" }
            }
        }),
        required_permissions: vec!["platform:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 网易云歌单搜索
    registry.register(Capability {
        id: "netease.searchPlaylist".to_string(),
        name: "Search NetEase playlists".to_string(),
        description: "Search NetEase playlists by keyword.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Recommend],
        input_schema: json!({
            "type": "object",
            "properties": {
                "keyword": {
                    "type": "string",
                    "description": "Search keyword, e.g. relax, work, sleep, light music. Leftover Chinese tags such as 轻音乐 still work."
                },
                "limit": {
                    "type": "integer",
                    "default": 5,
                    "description": "How many results to return"
                }
            },
            "required": ["keyword"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "playlists": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "Playlist id" },
                            "name": { "type": "string", "description": "Playlist name" },
                            "coverUrl": { "type": "string" },
                            "playCount": { "type": "integer" },
                            "trackCount": { "type": "integer" }
                        }
                    }
                },
                "recommendedPlaylistId": {
                    "type": "string",
                    "description": "Recommended playlist id (can be played directly)"
                }
            }
        }),
        required_permissions: vec!["netease:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });
}
