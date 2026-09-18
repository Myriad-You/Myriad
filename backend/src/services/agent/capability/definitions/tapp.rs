//! Tapp 应用系统能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // Tapp 生成
    registry.register(Capability {
        id: "tapp.generate".to_string(),
        name: "Generate app".to_string(),
        description: "Generate app code from a description.".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "description": { "type": "string" },
                "features": { "type": "array", "items": { "type": "string" } },
                "template": { "type": "string" }
            },
            "required": ["description"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "tappId": { "type": "string" },
                "name": { "type": "string" },
                "tapp": { "type": "object" },
                "frontendAction": { "type": "object" }
            }
        }),
        required_permissions: vec!["ai:generate".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(10000),
        ..Default::default()
    });

    // Tapp 列表
    registry.register(Capability {
        id: "tapp.list".to_string(),
        name: "App list".to_string(),
        description: "List installed apps.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": ["ai", "data", "developer", "game", "media", "productivity", "social", "utility"]
                },
                "enabled": { "type": "boolean" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "tapps": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["tapp:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 安装 Tapp
    registry.register(Capability {
        id: "tapp.install".to_string(),
        name: "Install app".to_string(),
        description: "Install an app.".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "App name" },
                "code": { "type": "string", "description": "Browser-runnable JavaScript" },
                "manifest": { "type": "object", "description": "Optional manifest; id and main are normalized by the host" }
            },
            "required": ["code"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "tappId": { "type": "string" }
            }
        }),
        required_permissions: vec!["tapp:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(2000),
        requires_confirmation: true,
        confirmation_message: Some("This will install a third-party app.".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // Tapp 页面内容
    registry.register(Capability {
        id: "tapp.page".to_string(),
        name: "App page".to_string(),
        description: "Read app page content.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "level": { 
                    "type": "string", 
                    "enum": ["apps", "detail", "widgets", "storage", "tasks", "executions"],
                    "description": "Page level: apps=list, detail=app, widgets=widgets, storage=storage, tasks=scheduled tasks, executions=runs"
                },
                "tappId": { "type": "string", "description": "Tapp id (required at detail/widgets/storage/tasks)" },
                "taskId": { "type": "string", "description": "Task id (required at executions)" },
                "filter": { 
                    "type": "string", 
                    "enum": ["all", "running", "installed", "error"],
                    "description": "App status filter"
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
                        "tapp": { "type": "object", "description": "Current Tapp" },
                        "task": { "type": "object", "description": "Current task" }
                    }
                },
                "content": {
                    "type": "object",
                    "properties": {
                        "apps": { "type": "array", "description": "App list" },
                        "detail": { "type": "object", "description": "App detail" },
                        "widgets": { "type": "array", "description": "Widget list" },
                        "storage": { "type": "array", "description": "Storage entries" },
                        "tasks": { "type": "array", "description": "Scheduled tasks" },
                        "executions": { "type": "array", "description": "Executions" }
                    }
                },
                "stats": {
                    "type": "object",
                    "properties": {
                        "totalApps": { "type": "integer" },
                        "runningApps": { "type": "integer" },
                        "totalWidgets": { "type": "integer" },
                        "totalTasks": { "type": "integer" }
                    }
                },
                "actions": {
                    "type": "object",
                    "properties": {
                        "available": { "type": "array", "description": "Available actions" }
                    }
                }
            }
        }),
        required_permissions: vec!["tapp:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(150),
        ..Default::default()
    });

    // Tapp 页面内容（详细）
    registry.register(Capability {
        id: "tapp.pageContent".to_string(),
        name: "App page content".to_string(),
        description: "Read detailed app page content.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "level": {
                    "type": "string",
                    "enum": ["apps", "detail", "widgets", "storage", "tasks", "executions"],
                    "description": "Query level"
                },
                "tappId": { "type": "string" },
                "taskId": { "type": "string" },
                "filter": { "type": "string", "enum": ["all", "running", "installed", "error"] },
                "limit": { "type": "integer", "default": 20 }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "level": { "type": "string" },
                "content": { "type": "object" },
                "stats": { "type": "object" }
            }
        }),
        required_permissions: vec!["tapp:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // Tapp 组件查询
    registry.register(Capability {
        id: "tapp.widget".to_string(),
        name: "App widgets".to_string(),
        description: "Read app widget information.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string" }
            },
            "required": ["tappId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "widgets": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["tapp:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // Tapp 存储操作
    registry.register(Capability {
        id: "tapp.storage".to_string(),
        name: "App storage".to_string(),
        description: "Read or write app key-value storage.".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![
            IntentAction::Query,
            IntentAction::Create,
            IntentAction::Update,
            IntentAction::Delete,
        ],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string" },
                "action": { "type": "string", "enum": ["get", "set", "delete"] },
                "key": { "type": "string" },
                "value": {}
            },
            "required": ["tappId", "action"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "data": {}
            }
        }),
        required_permissions: vec!["tapp:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // Tapp UI 结构
    registry.register(Capability {
        id: "tapp.ui".to_string(),
        name: "App UI".to_string(),
        description: "Parse app HTML and find interactive elements.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Tapp id" },
                "userId": { "type": "integer", "description": "User id" },
                "includeCode": { "type": "boolean", "default": false, "description": "Include JS analysis" },
                "elementFilter": {
                    "type": "string",
                    "enum": ["all", "buttons", "inputs", "forms", "interactive"],
                    "default": "interactive",
                    "description": "Element filter"
                }
            },
            "required": ["tappId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string" },
                "tappName": { "type": "string" },
                "structure": {
                    "type": "object",
                    "description": "HTML structure overview",
                    "properties": {
                        "hasBackground": { "type": "boolean" },
                        "hasContent": { "type": "boolean" },
                        "sections": { "type": "array" }
                    }
                },
                "elements": {
                    "type": "object",
                    "properties": {
                        "buttons": { "type": "array", "description": "Buttons" },
                        "inputs": { "type": "array", "description": "Inputs" },
                        "forms": { "type": "array", "description": "Forms" },
                        "links": { "type": "array", "description": "Links" },
                        "interactive": { "type": "array", "description": "Other interactive elements" }
                    }
                },
                "functions": {
                    "type": "array",
                    "description": "Functions found in JS"
                },
                "events": {
                    "type": "array",
                    "description": "Bound event handlers"
                },
                "i18n": {
                    "type": "object",
                    "description": "Supported languages and copy"
                },
                "suggestedActions": {
                    "type": "array",
                    "description": "Suggested actions"
                }
            }
        }),
        required_permissions: vec!["tapp:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // Tapp UI 智能理解
    registry.register(Capability {
        id: "tapp.understand".to_string(),
        name: "Understand app UI".to_string(),
        description: "Analyze app UI and plan actions.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Analyze, IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Tapp id" },
                "userId": { "type": "integer", "description": "User id" },
                "userIntent": {
                    "type": "string",
                    "description": "What the user wants, e.g. add a new task / search the weather (query also accepted)"
                },
                "query": {
                    "type": "string",
                    "description": "Alias of userIntent"
                },
                "uiAnalysis": {
                    "type": "object",
                    "description": "Optional existing tapp.ui analysis to skip a repeat"
                },
                "windowId": {
                    "type": "string",
                    "description": "Target window id (multi-window)"
                },
                "autoExecute": {
                    "type": "boolean",
                    "default": false,
                    "description": "Ignored. Tapp analysis does not emit DOM commands; use tapp.interact to run"
                }
            },
            "required": ["tappId", "userIntent"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "understanding": {
                    "type": "object",
                    "description": "Model understanding of the UI",
                    "properties": {
                        "appPurpose": { "type": "string", "description": "Main purpose of the app" },
                        "currentState": { "type": "string", "description": "Current UI state" },
                        "availableActions": {
                            "type": "array",
                            "description": "Actions that can be run",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": { "type": "string" },
                                    "description": { "type": "string" },
                                    "element": { "type": "string" },
                                    "confidence": { "type": "number" }
                                }
                            }
                        }
                    }
                },
                "plan": {
                    "type": "object",
                    "description": "Action plan for the user intent",
                    "properties": {
                        "canFulfill": { "type": "boolean" },
                        "explanation": { "type": "string" },
                        "steps": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "step": { "type": "integer" },
                                    "action": { "type": "string" },
                                    "target": { "type": "string" },
                                    "value": { "type": "string" },
                                    "reason": { "type": "string" }
                                }
                            }
                        },
                        "requiredInputs": {
                            "type": "array",
                            "description": "Inputs the user must provide",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "field": { "type": "string" },
                                    "description": { "type": "string" },
                                    "required": { "type": "boolean" }
                                }
                            }
                        }
                    }
                },
                "frontendAction": {
                    "type": ["object", "null"],
                    "description": "Analysis returns a plan only; this field is always null. Running needs a separate interaction"
                }
            }
        }),
        required_permissions: vec!["tapp:read".to_string(), "ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        requires_confirmation: true,
        confirmation_message: Some("AI will analyze the Tapp UI and may run actions".to_string()),
        risk_level: RiskLevel::Medium,
    });

    // Tapp UI 交互
    registry.register(Capability {
        id: "tapp.interact".to_string(),
        name: "App UI actions".to_string(),
        description: "Send a declared app interaction and get its result.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Execute, IntentAction::Create, IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Tapp id" },
                "interactionType": { "type": "string", "description": "Type declared in manifest agent.interactions" },
                "input": { "description": "Input validated against that interaction inputSchema" }
            },
            "required": ["tappId", "interactionType", "input"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "interaction": { "type": "object", "description": "Created interaction snapshot" },
                "frontendAction": { "type": "object", "description": "Opens the target Tapp only; no DOM commands" }
            }
        }),
        required_permissions: vec!["tapp:write".to_string(), "tapp:interact".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        requires_confirmation: true,
        confirmation_message: Some("This will send a declared interaction to the Tapp".to_string()),
        risk_level: RiskLevel::Low,
    });

    // 窗口状态查询
    registry.register(Capability {
        id: "tapp.windows".to_string(),
        name: "Window status".to_string(),
        description: "List open app windows.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "userId": { "type": "integer", "description": "User id" },
                "includeUiAnalysis": { 
                    "type": "boolean", 
                    "default": false,
                    "description": "Also analyze each window UI"
                }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "windows": {
                    "type": "array",
                    "description": "Open windows",
                    "items": {
                        "type": "object",
                        "properties": {
                            "windowId": { "type": "string", "description": "Window id" },
                            "tappId": { "type": "string", "description": "Running Tapp id" },
                            "tappName": { "type": "string", "description": "Tapp name" },
                            "position": { 
                                "type": "object",
                                "properties": {
                                    "x": { "type": "number" },
                                    "y": { "type": "number" }
                                }
                            },
                            "size": {
                                "type": "object",
                                "properties": {
                                    "width": { "type": "number" },
                                    "height": { "type": "number" }
                                }
                            },
                            "zIndex": { "type": "integer", "description": "Z-order (higher is in front)" },
                            "isActive": { "type": "boolean", "description": "Whether this window is active" },
                            "uiElements": { "type": "object", "description": "UI elements when includeUiAnalysis=true" }
                        }
                    }
                },
                "available": { "type": "boolean", "description": "Whether the window manager is mounted; if false, windows is not an empty desktop" },
                "activeWindowId": { "type": "string", "description": "Active window id" },
                "windowCount": { "type": "integer" },
                "maxWindows": { "type": "integer", "description": "Max open windows" }
            }
        }),
        required_permissions: vec!["tapp:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(50),
        ..Default::default()
    });

    // 打开窗口
    registry.register(Capability {
        id: "tapp.window.open".to_string(),
        name: "Open window".to_string(),
        description: "Open an app window.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Tapp id to open" },
                "tappName": { "type": "string", "description": "Tapp name (loose match)" },
                "position": {
                    "type": "object",
                    "description": "Window position (optional)",
                    "properties": {
                        "x": { "type": "number" },
                        "y": { "type": "number" }
                    }
                },
                "size": {
                    "type": "object",
                    "description": "Window size (optional)",
                    "properties": {
                        "width": { "type": "number" },
                        "height": { "type": "number" }
                    }
                }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "windowId": { "type": "string" },
                "tappId": { "type": "string" },
                "frontendAction": {
                    "type": "object",
                    "description": "Action the frontend should run"
                }
            }
        }),
        required_permissions: vec!["tapp:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 关闭窗口
    registry.register(Capability {
        id: "tapp.window.close".to_string(),
        name: "Close window".to_string(),
        description: "Close an app window.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Delete],
        input_schema: json!({
            "type": "object",
            "properties": {
                "windowId": { "type": "string", "description": "Window id to close" },
                "tappId": { "type": "string", "description": "Close windows of this Tapp id" },
                "position": {
                    "type": "string",
                    "enum": ["left", "right", "active", "all"],
                    "description": "By position: left, right, active, all"
                }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "closedWindows": { "type": "array", "items": { "type": "string" } },
                "frontendAction": { "type": "object" }
            }
        }),
        required_permissions: vec!["tapp:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(50),
        ..Default::default()
    });

    // 聚焦窗口
    registry.register(Capability {
        id: "tapp.window.focus".to_string(),
        name: "Focus window".to_string(),
        description: "Focus an app window.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "windowId": { "type": "string" },
                "tappId": { "type": "string" },
                "tappName": { "type": "string", "description": "Loose match by name" },
                "position": {
                    "type": "string",
                    "enum": ["left", "right", "next", "previous"],
                    "description": "Relative: next, previous"
                }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "focusedWindowId": { "type": "string" },
                "frontendAction": { "type": "object" }
            }
        }),
        required_permissions: vec!["tapp:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(20),
        ..Default::default()
    });
}
