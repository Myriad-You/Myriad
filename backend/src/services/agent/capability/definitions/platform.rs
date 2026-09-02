//! 平台数据能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // 平台数据读取
    registry.register(Capability {
        id: "platform.read".to_string(),
        name: "平台数据读取".to_string(),
        description: "读取各平台（Bilibili/Steam/GitHub等）的缓存数据".to_string(),
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
        name: "平台统计数据".to_string(),
        description: "获取平台数据的统计信息".to_string(),
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
        name: "平台数据写入".to_string(),
        description: "向平台缓存写入数据".to_string(),
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
        ..Default::default()
    });

    // 刷新平台数据
    registry.register(Capability {
        id: "platform.refresh".to_string(),
        name: "刷新平台数据".to_string(),
        description: "触发平台数据重新获取".to_string(),
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
        ..Default::default()
    });

    // Bilibili 用户查询
    registry.register(Capability {
        id: "bilibili.user".to_string(),
        name: "Bilibili 用户查询".to_string(),
        description: "获取 Bilibili 用户信息、收藏、追番".to_string(),
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
        name: "Bangumi 用户查询".to_string(),
        description: "获取 Bangumi 用户基本信息".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "username": { "type": "string" },
                "access_token": { "type": "string", "description": "可选，用于访问需要授权的数据" },
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
        name: "Bangumi 收藏查询".to_string(),
        description: "查询用户的 Bangumi 收藏、评分和观看状态".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "username": { "type": "string" },
                "access_token": { "type": "string", "description": "可选，用于访问私有收藏" },
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
        name: "Steam 用户查询".to_string(),
        description: "读取本站已同步的 Steam 缓存（不是按 steamId 实时查询）".to_string(),
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
        name: "GitHub 仓库查询".to_string(),
        description: "查询 GitHub 仓库、贡献和活动".to_string(),
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
                "data": { "description": "repos / contributions / starred 对应的数据" }
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
        name: "网易云歌单".to_string(),
        description: "获取用户网易云歌单和听歌记录".to_string(),
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
                "data": { "description": "playlists / recent / favorites 对应的数据" },
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
        name: "Bilibili 追番查询".to_string(),
        description: "读取本站已同步的 B 站追番缓存（不是按 uid 实时查询）".to_string(),
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
        name: "Bilibili 视频查询".to_string(),
        description: "通过 BV 号或 AV 号查询 Bilibili 视频详情".to_string(),
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
        name: "Steam 愿望单".to_string(),
        description: "读取本站已同步的 Steam 愿望单缓存（不是按 steamId 实时查询）".to_string(),
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
        name: "Steam 游戏详情".to_string(),
        description: "查询 Steam 游戏详细信息".to_string(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "appId": { "type": "integer", "description": "也接受字符串 / app_id" },
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
        name: "平台连接状态".to_string(),
        description: "查询各平台数据连接和同步状态".to_string(),
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
        name: "搜索网易云歌单".to_string(),
        description:
            "搜索网易云音乐歌单，支持关键词搜索（如轻音乐、放松、工作等），返回歌单列表及ID"
                .to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Recommend],
        input_schema: json!({
            "type": "object",
            "properties": {
                "keyword": {
                    "type": "string",
                    "description": "搜索关键词，如：轻音乐、放松、工作、睡眠、纯音乐等"
                },
                "limit": {
                    "type": "integer",
                    "default": 5,
                    "description": "返回结果数量"
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
                            "id": { "type": "string", "description": "歌单ID" },
                            "name": { "type": "string", "description": "歌单名称" },
                            "coverUrl": { "type": "string" },
                            "playCount": { "type": "integer" },
                            "trackCount": { "type": "integer" }
                        }
                    }
                },
                "recommendedPlaylistId": {
                    "type": "string",
                    "description": "推荐的歌单ID（可直接用于播放）"
                }
            }
        }),
        required_permissions: vec!["netease:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });
}
