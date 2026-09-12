//! UI 控制能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // 音乐播放控制
    registry.register(Capability {
        id: "music.control".to_string(),
        name: "Music control".to_string(),
        description: "Play, pause, skip, or change volume.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Control, IntentAction::Navigate],
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": { 
                    "type": "string", 
                    "enum": ["play", "pause", "toggle", "next", "previous", "volume", "mute", "unmute", "seek"],
                    "description": "Player action"
                },
                "volume": { 
                    "type": "number", 
                    "minimum": 0, 
                    "maximum": 100,
                    "description": "Volume (0-100)"
                },
                "position": { 
                    "type": "number",
                    "description": "Playback position in seconds"
                }
            },
            "required": ["action"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "currentState": { "type": "object" },
                "frontendAction": { "type": "object" }
            }
        }),
        required_permissions: vec!["music:control".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 音乐播放状态
    registry.register(Capability {
        id: "music.status".to_string(),
        name: "Music status".to_string(),
        description: "Read the current player status.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "available": { "type": "boolean" },
                "isPlaying": { "type": "boolean" },
                "isEnabled": { "type": "boolean" },
                "currentSong": { "type": "object" },
                "frontendAction": { "type": "object" }
            }
        }),
        required_permissions: vec!["music:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(50),
        ..Default::default()
    });

    // 播放歌单
    registry.register(Capability {
        id: "music.playlist".to_string(),
        name: "Play playlist".to_string(),
        description: "Load and play a playlist.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Navigate, IntentAction::Control],
        input_schema: json!({
            "type": "object",
            "properties": {
                "playlistId": { "type": "string", "description": "Playlist id (playlist_id also accepted)" },
                "playlist_id": { "type": "string", "description": "Alias of playlistId" },
                "source": {
                    "type": "string",
                    "enum": ["netease", "qq"],
                    "description": "Music platform"
                },
                "autoPlay": {
                    "type": "boolean",
                    "default": true,
                    "description": "Start playback automatically"
                }
            },
            "required": ["playlistId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "playlistId": { "type": "string" },
                "source": { "type": "string" },
                "autoPlay": { "type": "boolean" },
                "message": { "type": "string" },
                "frontendAction": { "type": "object" }
            }
        }),
        required_permissions: vec!["music:control".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        ..Default::default()
    });

    // 路由状态
    registry.register(Capability {
        id: "router.state".to_string(),
        name: "Route status".to_string(),
        description: "Read the current route and what is on screen.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "currentPath": { "type": "string", "description": "Current route" },
                "params": { "type": "object", "description": "Route params" },
                "query": { "type": "object", "description": "URL query params" },
                "pageName": { "type": "string", "description": "Page name" },
                "pageType": { 
                    "type": "string", 
                    "enum": ["home", "library", "platform", "brew", "tapp", "report", "settings", "profile", "other"],
                    "description": "Page type"
                },
                "context": {
                    "type": "object",
                    "description": "Page context",
                    "properties": {
                        "platform": { "type": "string", "description": "Current platform (e.g. bilibili/steam)" },
                        "itemId": { "type": "string", "description": "Current item id" },
                        "viewMode": { "type": "string", "description": "View mode (list/grid/detail)" },
                        "filters": { "type": "object", "description": "Current filters" }
                    }
                },
                "breadcrumb": { 
                    "type": "array", 
                    "items": { "type": "string" },
                    "description": "Breadcrumb path"
                },
                "canGoBack": { "type": "boolean", "description": "Whether back is available" },
                "timestamp": { "type": "string", "description": "When this state was read" }
            }
        }),
        required_permissions: vec!["router:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(10),
        ..Default::default()
    });

    // 路由导航
    registry.register(Capability {
        id: "router.navigate".to_string(),
        name: "Navigate".to_string(),
        description: "Go to a page in the site.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Target route, e.g. /brew, /platform/steam, /tapp/multi"
                },
                "params": {
                    "type": "object",
                    "description": "Route params, e.g. { sourceId: 123 }"
                },
                "query": {
                    "type": "object",
                    "description": "URL query, e.g. { filter: 'unread' }"
                },
                "replace": {
                    "type": "boolean",
                    "default": false,
                    "description": "Replace the current history entry instead of pushing"
                },
                "openInNewWindow": {
                    "type": "boolean",
                    "default": false,
                    "description": "Open in a new window/tab"
                }
            },
            "required": ["path"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "currentPath": { "type": "string" },
                "frontendAction": {
                    "type": "object",
                    "description": "Navigation command for the frontend"
                }
            }
        }),
        required_permissions: vec!["router:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        requires_confirmation: false,
        risk_level: RiskLevel::None,
        confirmation_message: None,
    });

    // 页面元素交互
    registry.register(Capability {
        id: "page.interact".to_string(),
        name: "Page actions".to_string(),
        description: "Click or type on page elements.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": { 
                    "type": "string", 
                    "enum": ["click", "hover", "focus", "scroll", "select", "toggle", "expand", "collapse", "type", "input"],
                    "description": "Action type. type/input writes value into an input"
                },
                "target": { 
                    "type": "object",
                    "description": "Target element",
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector" },
                        "testId": { "type": "string", "description": "data-testid value" },
                        "text": { "type": "string", "description": "Element text (loose match)" },
                        "ariaLabel": { "type": "string", "description": "aria-label value" },
                        "role": { "type": "string", "description": "Role, e.g. button, link, tab" },
                        "index": { "type": "integer", "description": "Which match to use when several match (0-based)" }
                    }
                },
                "value": {
                    "description": "Value for select/toggle, or text for type/input"
                },
                "scrollOptions": {
                    "type": "object",
                    "properties": {
                        "direction": { "type": "string", "enum": ["top", "bottom", "left", "right"] },
                        "offset": { "type": "integer" },
                        "smooth": { "type": "boolean", "default": true }
                    }
                },
                "waitFor": {
                    "type": "object",
                    "description": "Wait condition",
                    "properties": {
                        "visible": { "type": "boolean" },
                        "timeout": { "type": "integer", "default": 3000 }
                    }
                }
            },
            "required": ["action", "target"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "queued": { "type": "boolean", "description": "Handed to the frontend; does not mean the element was clicked" },
                "action": { "type": "string" },
                "target": { "type": "object" },
                "frontendAction": {
                    "type": "object",
                    "description": "Interaction command for the frontend"
                }
            }
        }),
        required_permissions: vec!["ui:interact".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        requires_confirmation: true,
        confirmation_message: Some("This will interact with a page element".to_string()),
        risk_level: RiskLevel::Low,
    });

    // 页面 UI 智能理解
    registry.register(Capability {
        id: "page.understand".to_string(),
        name: "Understand page".to_string(),
        description: "Analyze the page UI and plan actions.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Analyze, IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "userIntent": {
                    "type": "string",
                    "description": "What the user wants (query also accepted)"
                },
                "query": {
                    "type": "string",
                    "description": "Alias of userIntent"
                },
                "currentPath": {
                    "type": "string",
                    "description": "Current page path"
                },
                "context": {
                    "type": "object",
                    "description": "Page snapshot (request page_context is injected; pageSnapshot also accepted)"
                },
                "pageSnapshot": {
                    "type": "object",
                    "description": "Page snapshot from the frontend; handler reads context / pageSnapshot",
                    "properties": {
                        "visibleElements": { "type": "array" },
                        "activeElement": { "type": "object" },
                        "scrollPosition": { "type": "object" }
                    }
                },
                "autoExecute": {
                    "type": "boolean",
                    "default": false,
                    "description": "Turn the plan into frontendActions. Click/input still needs granted ui:interact; otherwise only navigate"
                }
            },
            "required": ["userIntent"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "plan": { "type": "object" },
                "understood": { "type": "boolean" },
                "frontendAction": { "type": "object" }
            }
        }),
        required_permissions: vec!["ui:read".to_string(), "ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        requires_confirmation: true,
        confirmation_message: Some("AI will analyze the page and may run actions".to_string()),
        risk_level: RiskLevel::Medium,
    });

    // 页面内容读取
    registry.register(Capability {
        id: "page.content".to_string(),
        name: "Page content".to_string(),
        description: "Read the current page content.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "currentPath": { "type": "string", "description": "Current route" },
                "pageType": { "type": "string", "description": "Page type" },
                "context": {
                    "type": "object",
                    "description": "Page context including sourceId, itemId, etc."
                }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "pageType": { "type": "string" },
                "currentPath": { "type": "string" },
                "content": { "type": "any" },
                "source": { "type": "string" }
            }
        }),
        required_permissions: vec!["page:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 全局搜索
    registry.register(Capability {
        id: "search.global".to_string(),
        name: "Search".to_string(),
        description: "Search across platforms.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "platforms": { "type": "array", "items": { "type": "string" } },
                "limit": { "type": "integer", "default": 20 }
            },
            "required": ["query"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["search:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(1000),
        ..Default::default()
    });

    // 模糊搜索
    registry.register(Capability {
        id: "search.fuzzy".to_string(),
        name: "Fuzzy search".to_string(),
        description: "Fuzzy-search feeds, apps, and content.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search keyword (loose match)"
                },
                "scope": {
                    "type": "string",
                    "enum": ["brew", "tapp", "all"],
                    "description": "Scope: brew=feeds, tapp=apps, all=all",
                    "default": "all"
                },
                "type": {
                    "type": "string",
                    "enum": ["source", "item", "app", "widget"],
                    "description": "Kind: source=feed, item=content, app=app, widget=widget"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results",
                    "default": 10
                }
            },
            "required": ["query"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "results": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "name": { "type": "string" },
                            "type": { "type": "string" },
                            "scope": { "type": "string" },
                            "score": { "type": "number", "description": "Match score 0-1" },
                            "metadata": { "type": "object" }
                        }
                    }
                },
                "total": { "type": "integer" },
                "query": { "type": "string" },
                "notFound": {
                    "type": "boolean",
                    "description": "Whether nothing matched"
                },
                "canDiscover": {
                    "type": "boolean",
                    "description": "Whether discovering a new feed is possible"
                },
                "discoveryHint": {
                    "type": "object",
                    "description": "Hint for discovering a new feed",
                    "properties": {
                        "suggestedUrls": { "type": "array", "items": { "type": "string" } },
                        "searchQuery": { "type": "string" }
                    }
                }
            }
        }),
        required_permissions: vec!["search:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 内容写入
    registry.register(Capability {
        id: "content.write".to_string(),
        name: "Write content".to_string(),
        description: "Write content to an app, note, or clipboard.".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Create, IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "target": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["tapp", "clipboard", "file", "storage"],
                            "description": "Target type"
                        },
                        "id": { "type": "string", "description": "Target id (e.g. Tapp id)" },
                        "name": { "type": "string", "description": "Target name (loose match)" }
                    },
                    "required": ["type"]
                },
                "content": {
                    "type": "string",
                    "description": "Content to write"
                },
                "contentType": {
                    "type": "string",
                    "enum": ["text", "markdown", "html", "json"],
                    "default": "text"
                },
                "append": {
                    "type": "boolean",
                    "description": "Append instead of overwrite",
                    "default": false
                }
            },
            "required": ["target", "content"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "target": { "type": "string" },
                "targetId": { "type": "string" },
                "contentId": { "type": "string" },
                "title": { "type": "string" }
            }
        }),
        required_permissions: vec!["content:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        requires_confirmation: true,
        confirmation_message: Some("This will write content to the target".to_string()),
        risk_level: RiskLevel::Low,
    });

    // 上下文引用
    registry.register(Capability {
        id: "context.reference".to_string(),
        name: "Context reference".to_string(),
        description: "Reuse output from an earlier step.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "stepId": {
                    "type": "string",
                    "description": "Step id to cite"
                },
                "path": {
                    "type": "string",
                    "description": "JSON path, e.g. results[0].content or summary"
                },
                "transform": {
                    "type": "string",
                    "enum": ["none", "stringify", "parse", "join", "first", "last"],
                    "description": "Transform",
                    "default": "none"
                }
            },
            "required": ["stepId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "value": { "description": "Cited value" },
                "type": { "type": "string", "description": "Value type" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(1),
        ..Default::default()
    });
}
