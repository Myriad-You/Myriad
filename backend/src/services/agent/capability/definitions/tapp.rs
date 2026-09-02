//! Tapp 应用系统能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // Tapp 生成
    registry.register(Capability {
        id: "tapp.generate".to_string(),
        name: "Tapp 生成".to_string(),
        description: "根据描述生成 Tapp 应用代码".to_string(),
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
        name: "Tapp 列表".to_string(),
        description: "获取已安装的 Tapp 应用列表".to_string(),
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
        name: "安装 Tapp".to_string(),
        description: "安装新的 Tapp 应用".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "应用名称" },
                "code": { "type": "string", "description": "浏览器可直接运行的 JavaScript" },
                "manifest": { "type": "object", "description": "可选 manifest；id 和 main 由系统规范化" }
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
        ..Default::default()
    });

    // Tapp 页面内容
    registry.register(Capability {
        id: "tapp.page".to_string(),
        name: "Tapp 页面内容".to_string(),
        description: "读取 Tapp 应用页面的详细内容，包括应用列表、应用详情、组件、存储数据、定时任务等".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "level": { 
                    "type": "string", 
                    "enum": ["apps", "detail", "widgets", "storage", "tasks", "executions"],
                    "description": "页面层级: apps=应用列表, detail=应用详情, widgets=组件列表, storage=存储数据, tasks=定时任务, executions=执行记录"
                },
                "tappId": { "type": "string", "description": "Tapp ID（detail/widgets/storage/tasks 层级需要）" },
                "taskId": { "type": "string", "description": "任务 ID（executions 层级需要）" },
                "filter": { 
                    "type": "string", 
                    "enum": ["all", "running", "installed", "error"],
                    "description": "应用状态筛选"
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
                        "tapp": { "type": "object", "description": "当前 Tapp 信息" },
                        "task": { "type": "object", "description": "当前任务信息" }
                    }
                },
                "content": {
                    "type": "object",
                    "properties": {
                        "apps": { "type": "array", "description": "应用列表" },
                        "detail": { "type": "object", "description": "应用详情" },
                        "widgets": { "type": "array", "description": "组件列表" },
                        "storage": { "type": "array", "description": "存储数据" },
                        "tasks": { "type": "array", "description": "定时任务" },
                        "executions": { "type": "array", "description": "执行记录" }
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
                        "available": { "type": "array", "description": "可用操作列表" }
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
        name: "Tapp 页面内容详情".to_string(),
        description: "读取 Tapp 应用页面的详细内容，包括应用列表、详情、组件、存储、任务、执行记录等多层级查询".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "level": {
                    "type": "string",
                    "enum": ["apps", "detail", "widgets", "storage", "tasks", "executions"],
                    "description": "查询层级"
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
        name: "Tapp 组件查询".to_string(),
        description: "查询 Tapp 应用的桌面组件信息".to_string(),
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
        name: "Tapp 存储操作".to_string(),
        description: "读写 Tapp 应用的键值存储数据".to_string(),
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
        name: "Tapp UI 结构".to_string(),
        description: "解析 Tapp 应用的 HTML 结构，识别可交互元素（按钮、输入框、表单等），分析功能和可用操作".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Tapp 应用 ID" },
                "userId": { "type": "integer", "description": "用户 ID" },
                "includeCode": { "type": "boolean", "default": false, "description": "是否包含 JS 代码分析" },
                "elementFilter": {
                    "type": "string",
                    "enum": ["all", "buttons", "inputs", "forms", "interactive"],
                    "default": "interactive",
                    "description": "元素筛选类型"
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
                    "description": "HTML 结构概览",
                    "properties": {
                        "hasBackground": { "type": "boolean" },
                        "hasContent": { "type": "boolean" },
                        "sections": { "type": "array" }
                    }
                },
                "elements": {
                    "type": "object",
                    "properties": {
                        "buttons": { "type": "array", "description": "按钮元素列表" },
                        "inputs": { "type": "array", "description": "输入元素列表" },
                        "forms": { "type": "array", "description": "表单元素列表" },
                        "links": { "type": "array", "description": "链接元素列表" },
                        "interactive": { "type": "array", "description": "其他可交互元素" }
                    }
                },
                "functions": {
                    "type": "array",
                    "description": "从 JS 代码识别的功能列表"
                },
                "events": {
                    "type": "array",
                    "description": "绑定的事件处理器"
                },
                "i18n": {
                    "type": "object",
                    "description": "国际化支持的语言和文本"
                },
                "suggestedActions": {
                    "type": "array",
                    "description": "建议的可执行操作"
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
        name: "Tapp UI 智能理解".to_string(),
        description:
            "使用 AI 分析 Tapp 的 UI 结构，理解每个控件的用途，并根据用户意图生成操作指令序列"
                .to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Analyze, IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Tapp 应用 ID" },
                "userId": { "type": "integer", "description": "用户 ID" },
                "userIntent": {
                    "type": "string",
                    "description": "用户想要执行的操作描述，如'添加一条新任务'、'搜索天气'等（也可用 query）"
                },
                "query": {
                    "type": "string",
                    "description": "userIntent 的别名"
                },
                "uiAnalysis": {
                    "type": "object",
                    "description": "可选：已有的 tapp.ui 分析结果，避免重复分析"
                },
                "windowId": {
                    "type": "string",
                    "description": "目标窗口 ID（多窗口场景）"
                },
                "autoExecute": {
                    "type": "boolean",
                    "default": false,
                    "description": "忽略。Tapp 分析不发出 DOM 指令，执行请用 tapp.interact"
                }
            },
            "required": ["tappId", "userIntent"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "understanding": {
                    "type": "object",
                    "description": "AI 对 UI 的理解",
                    "properties": {
                        "appPurpose": { "type": "string", "description": "应用的主要用途" },
                        "currentState": { "type": "string", "description": "当前 UI 状态描述" },
                        "availableActions": {
                            "type": "array",
                            "description": "可执行的操作列表",
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
                    "description": "根据用户意图生成的操作计划",
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
                            "description": "需要用户提供的输入",
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
                    "description": "当前分析只返回计划，此字段固定为 null；执行必须另建 interaction"
                }
            }
        }),
        required_permissions: vec!["tapp:read".to_string(), "ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        requires_confirmation: true,
        confirmation_message: Some("AI 将分析 Tapp UI 并可能执行操作".to_string()),
        risk_level: RiskLevel::Medium,
    });

    // Tapp UI 交互
    registry.register(Capability {
        id: "tapp.interact".to_string(),
        name: "Tapp UI 交互".to_string(),
        description: "创建 Manifest 声明的 Agent Interaction，由 Tapp 接受并按 schema 返回结果".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Execute, IntentAction::Create, IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Tapp 应用 ID" },
                "interactionType": { "type": "string", "description": "Manifest agent.interactions 中声明的类型" },
                "input": { "description": "按该 interaction inputSchema 校验的输入" }
            },
            "required": ["tappId", "interactionType", "input"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "interaction": { "type": "object", "description": "已创建的 interaction 快照" },
                "frontendAction": { "type": "object", "description": "仅用于打开目标 Tapp，不包含 DOM 命令" }
            }
        }),
        required_permissions: vec!["tapp:write".to_string(), "tapp:interact".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        requires_confirmation: true,
        confirmation_message: Some("即将向 Tapp 发起声明式交互请求".to_string()),
        risk_level: RiskLevel::Low,
    });

    // 窗口状态查询
    registry.register(Capability {
        id: "tapp.windows".to_string(),
        name: "窗口状态查询".to_string(),
        description: "查询当前打开的 Tapp 窗口。不在 /tapp/run 时 available=false，不是空窗口列表".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "userId": { "type": "integer", "description": "用户 ID" },
                "includeUiAnalysis": { 
                    "type": "boolean", 
                    "default": false,
                    "description": "是否同时分析各窗口的 UI 结构"
                }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "windows": {
                    "type": "array",
                    "description": "当前打开的窗口列表",
                    "items": {
                        "type": "object",
                        "properties": {
                            "windowId": { "type": "string", "description": "窗口唯一 ID" },
                            "tappId": { "type": "string", "description": "运行的 Tapp ID" },
                            "tappName": { "type": "string", "description": "Tapp 名称" },
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
                            "zIndex": { "type": "integer", "description": "层级（越大越靠前）" },
                            "isActive": { "type": "boolean", "description": "是否为活跃窗口" },
                            "uiElements": { "type": "object", "description": "UI 元素（如果 includeUiAnalysis=true）" }
                        }
                    }
                },
                "available": { "type": "boolean", "description": "窗口管理器是否挂载；false 时 windows 不能当成空桌面" },
                "activeWindowId": { "type": "string", "description": "当前活跃窗口 ID" },
                "windowCount": { "type": "integer" },
                "maxWindows": { "type": "integer", "description": "最大可打开窗口数" }
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
        name: "打开窗口".to_string(),
        description: "在多窗口模式下打开一个新的 Tapp 应用窗口".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "要打开的 Tapp ID" },
                "tappName": { "type": "string", "description": "Tapp 名称（模糊匹配）" },
                "position": {
                    "type": "object",
                    "description": "窗口位置（可选）",
                    "properties": {
                        "x": { "type": "number" },
                        "y": { "type": "number" }
                    }
                },
                "size": {
                    "type": "object",
                    "description": "窗口尺寸（可选）",
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
                    "description": "前端需要执行的操作"
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
        name: "关闭窗口".to_string(),
        description: "关闭指定的 Tapp 窗口".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Delete],
        input_schema: json!({
            "type": "object",
            "properties": {
                "windowId": { "type": "string", "description": "要关闭的窗口 ID" },
                "tappId": { "type": "string", "description": "通过 Tapp ID 指定（关闭该 Tapp 的窗口）" },
                "position": { 
                    "type": "string", 
                    "enum": ["left", "right", "active", "all"],
                    "description": "通过位置指定：left=最左边, right=最右边, active=当前活跃, all=全部"
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
        name: "聚焦窗口".to_string(),
        description: "将指定窗口置为活跃状态（置顶）".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "windowId": { "type": "string" },
                "tappId": { "type": "string" },
                "tappName": { "type": "string", "description": "通过名称模糊匹配" },
                "position": {
                    "type": "string",
                    "enum": ["left", "right", "next", "previous"],
                    "description": "相对位置：next=下一个, previous=上一个"
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
