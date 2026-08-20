//! UI 控制能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // 音乐播放控制
    registry.register(Capability {
        id: "music.control".to_string(),
        name: "音乐播放控制".to_string(),
        description: "控制音乐播放器：播放、暂停、上一首、下一首、调节音量等".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Control, IntentAction::Navigate],
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": { 
                    "type": "string", 
                    "enum": ["play", "pause", "toggle", "next", "previous", "volume", "mute", "unmute", "seek"],
                    "description": "播放器操作类型"
                },
                "volume": { 
                    "type": "number", 
                    "minimum": 0, 
                    "maximum": 100,
                    "description": "音量值（0-100）"
                },
                "position": { 
                    "type": "number",
                    "description": "播放位置（秒）"
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
        name: "音乐播放状态".to_string(),
        description: "向浏览器请求当前播放器状态（状态只存在于前端，不在后端编造）".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
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
        name: "播放歌单".to_string(),
        description: "加载并播放指定歌单".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Navigate, IntentAction::Control],
        input_schema: json!({
            "type": "object",
            "properties": {
                "playlistId": { "type": "string", "description": "歌单 ID" },
                "source": {
                    "type": "string",
                    "enum": ["netease", "qq"],
                    "description": "音乐平台来源"
                },
                "autoPlay": {
                    "type": "boolean",
                    "default": true,
                    "description": "是否自动开始播放"
                }
            },
            "required": ["playlistId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "playlistName": { "type": "string" },
                "songCount": { "type": "integer" },
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
        name: "路由状态".to_string(),
        description: "获取当前页面路由状态，了解用户正在查看的内容".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "currentPath": { "type": "string", "description": "当前路由路径" },
                "params": { "type": "object", "description": "路由参数" },
                "query": { "type": "object", "description": "URL 查询参数" },
                "pageName": { "type": "string", "description": "页面名称" },
                "pageType": { 
                    "type": "string", 
                    "enum": ["home", "platform", "brew", "tapp", "report", "settings", "profile", "other"],
                    "description": "页面类型"
                },
                "context": {
                    "type": "object",
                    "description": "页面上下文信息",
                    "properties": {
                        "platform": { "type": "string", "description": "当前平台（如 bilibili/steam）" },
                        "itemId": { "type": "string", "description": "当前查看的项目 ID" },
                        "viewMode": { "type": "string", "description": "视图模式（list/grid/detail）" },
                        "filters": { "type": "object", "description": "当前筛选条件" }
                    }
                },
                "breadcrumb": { 
                    "type": "array", 
                    "items": { "type": "string" },
                    "description": "面包屑导航路径"
                },
                "canGoBack": { "type": "boolean", "description": "是否可以返回" },
                "timestamp": { "type": "string", "description": "状态获取时间" }
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
        name: "路由导航".to_string(),
        description: "导航到指定页面，支持主应用的所有路由，包括首页、平台页、Brew、Tapp、设置等"
            .to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "目标路由路径，如 /brew、/platform/steam、/tapp/multi"
                },
                "params": {
                    "type": "object",
                    "description": "路由参数，如 { sourceId: 123 }"
                },
                "query": {
                    "type": "object",
                    "description": "URL 查询参数，如 { filter: 'unread' }"
                },
                "replace": {
                    "type": "boolean",
                    "default": false,
                    "description": "是否替换当前历史记录（而非添加）"
                },
                "openInNewWindow": {
                    "type": "boolean",
                    "default": false,
                    "description": "是否在新窗口/标签页打开"
                }
            },
            "required": ["path"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "previousPath": { "type": "string" },
                "currentPath": { "type": "string" },
                "frontendAction": {
                    "type": "object",
                    "description": "前端执行的导航指令"
                }
            }
        }),
        required_permissions: vec!["router:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        requires_confirmation: false,
        risk_level: RiskLevel::Low,
        confirmation_message: None,
    });

    // 页面元素交互
    registry.register(Capability {
        id: "page.interact".to_string(),
        name: "页面元素交互".to_string(),
        description: "与主应用页面元素交互，支持点击按钮、链接、标签页、菜单项等各种可交互元素".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": { 
                    "type": "string", 
                    "enum": ["click", "hover", "focus", "scroll", "select", "toggle", "expand", "collapse"],
                    "description": "交互动作类型" 
                },
                "target": { 
                    "type": "object",
                    "description": "目标元素",
                    "properties": {
                        "selector": { "type": "string", "description": "CSS 选择器" },
                        "testId": { "type": "string", "description": "data-testid 属性值" },
                        "text": { "type": "string", "description": "元素文本内容（模糊匹配）" },
                        "ariaLabel": { "type": "string", "description": "aria-label 属性值" },
                        "role": { "type": "string", "description": "元素角色，如 button、link、tab" },
                        "index": { "type": "integer", "description": "如果匹配多个元素，选择第几个（0-based）" }
                    }
                },
                "value": { 
                    "type": "string", 
                    "description": "用于 select/toggle 等需要值的操作" 
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
                    "description": "等待条件",
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
                "elementFound": { "type": "boolean" },
                "elementInfo": {
                    "type": "object",
                    "properties": {
                        "tagName": { "type": "string" },
                        "text": { "type": "string" },
                        "classes": { "type": "array" },
                        "rect": { "type": "object" }
                    }
                },
                "frontendAction": {
                    "type": "object",
                    "description": "前端执行的交互指令"
                }
            }
        }),
        required_permissions: vec!["ui:interact".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        requires_confirmation: true,
        confirmation_message: Some("即将与页面元素交互".to_string()),
        risk_level: RiskLevel::Low,
    });

    // 页面 UI 智能理解
    registry.register(Capability {
        id: "page.understand".to_string(),
        name: "页面 UI 智能理解".to_string(),
        description: "使用 AI 分析当前页面的 UI 结构，理解各元素的用途，并根据用户意图生成操作指令"
            .to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Analyze, IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "userIntent": {
                    "type": "string",
                    "description": "用户想要执行的操作描述"
                },
                "currentPath": {
                    "type": "string",
                    "description": "当前页面路径"
                },
                "pageSnapshot": {
                    "type": "object",
                    "description": "页面快照信息（由前端提供）",
                    "properties": {
                        "visibleElements": { "type": "array" },
                        "activeElement": { "type": "object" },
                        "scrollPosition": { "type": "object" }
                    }
                },
                "autoExecute": {
                    "type": "boolean",
                    "default": false,
                    "description": "是否自动执行生成的操作"
                }
            },
            "required": ["userIntent"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "understanding": {
                    "type": "object",
                    "properties": {
                        "pagePurpose": { "type": "string" },
                        "currentState": { "type": "string" },
                        "availableActions": { "type": "array" }
                    }
                },
                "plan": {
                    "type": "object",
                    "properties": {
                        "canFulfill": { "type": "boolean" },
                        "explanation": { "type": "string" },
                        "steps": { "type": "array" }
                    }
                },
                "frontendAction": { "type": "object" }
            }
        }),
        required_permissions: vec!["ui:read".to_string(), "ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        requires_confirmation: true,
        confirmation_message: Some("AI 将分析页面并可能执行操作".to_string()),
        risk_level: RiskLevel::Medium,
    });

    // 页面内容读取
    registry.register(Capability {
        id: "page.content".to_string(),
        name: "页面内容".to_string(),
        description: "读取当前页面内容：有快照用快照，否则转发 brew.page / tapp.page / platform.read / report.list".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "currentPath": { "type": "string", "description": "当前路由路径" },
                "pageType": { "type": "string", "description": "页面类型" },
                "context": {
                    "type": "object",
                    "description": "页面上下文，包含 sourceId、itemId 等"
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
        name: "全局搜索".to_string(),
        description: "跨平台搜索内容".to_string(),
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
        name: "模糊搜索".to_string(),
        description: "在 Brew 订阅源、Tapp 应用、内容等中进行模糊搜索。Brew 源匹配名称、category 与 site_url（如「友情链接」可返回友链分类源）。"
            .to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "搜索关键词，支持模糊匹配"
                },
                "scope": {
                    "type": "string",
                    "enum": ["brew", "tapp", "all"],
                    "description": "搜索范围: brew=订阅源, tapp=应用, all=全部",
                    "default": "all"
                },
                "type": {
                    "type": "string",
                    "enum": ["source", "item", "app", "widget"],
                    "description": "搜索类型: source=订阅源, item=内容, app=应用, widget=组件"
                },
                "limit": {
                    "type": "integer",
                    "description": "返回结果数量限制",
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
                            "score": { "type": "number", "description": "匹配得分 0-1" },
                            "metadata": { "type": "object" }
                        }
                    }
                },
                "total": { "type": "integer" },
                "query": { "type": "string" },
                "notFound": {
                    "type": "boolean",
                    "description": "是否未找到任何匹配"
                },
                "canDiscover": {
                    "type": "boolean",
                    "description": "是否可以尝试发现新源"
                },
                "discoveryHint": {
                    "type": "object",
                    "description": "发现新源的提示信息",
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
        name: "内容写入".to_string(),
        description: "将内容写入到指定目标（Tapp 应用、笔记、剪贴板等）".to_string(),
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
                            "description": "目标类型"
                        },
                        "id": { "type": "string", "description": "目标 ID（如 Tapp ID）" },
                        "name": { "type": "string", "description": "目标名称（用于模糊匹配）" }
                    },
                    "required": ["type"]
                },
                "content": {
                    "type": "string",
                    "description": "要写入的内容"
                },
                "contentType": {
                    "type": "string",
                    "enum": ["text", "markdown", "html", "json"],
                    "default": "text"
                },
                "append": {
                    "type": "boolean",
                    "description": "是否追加而非覆盖",
                    "default": false
                }
            },
            "required": ["target", "content"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "targetId": { "type": "string" },
                "targetName": { "type": "string" }
            }
        }),
        required_permissions: vec!["content:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        requires_confirmation: true,
        confirmation_message: Some("即将写入内容到目标".to_string()),
        risk_level: RiskLevel::Low,
    });

    // 上下文引用
    registry.register(Capability {
        id: "context.reference".to_string(),
        name: "上下文引用".to_string(),
        description: "引用之前步骤的输出结果，支持路径表达式访问嵌套数据".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "stepId": {
                    "type": "string",
                    "description": "要引用的步骤 ID"
                },
                "path": {
                    "type": "string",
                    "description": "JSON 路径表达式，如 'results[0].content' 或 'summary'"
                },
                "transform": {
                    "type": "string",
                    "enum": ["none", "stringify", "parse", "join", "first", "last"],
                    "description": "转换操作",
                    "default": "none"
                }
            },
            "required": ["stepId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "value": { "description": "引用的值" },
                "type": { "type": "string", "description": "值的类型" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(1),
        ..Default::default()
    });
}
