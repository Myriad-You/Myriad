//! 系统操作能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // 数据转换
    registry.register(Capability {
        id: "data.transform".to_string(),
        name: "Transform data".to_string(),
        description: "Filter, sort, or aggregate data.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Query, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "input": { "type": "object" },
                "pipeline": { "type": "array" }
            },
            "required": ["input", "pipeline"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "data": { "type": "array" },
                "count": { "type": "integer" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 创建 Tapp 定时任务（tapp_scheduled_tasks，非 Agent Heartbeat）
    registry.register(Capability {
        id: "scheduler.create".to_string(),
        name: "Create scheduled task".to_string(),
        description: "Create a scheduled app task.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Monitor, IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Installed Tapp id that owns the task" },
                "taskId": { "type": "string", "description": "Task id unique inside the Tapp; generated if omitted" },
                "name": { "type": "string" },
                "scheduleType": {
                    "type": "string",
                    "enum": ["cron", "interval", "once", "daily"]
                },
                "schedule": {
                    "type": "object",
                    "properties": {
                        "cron": { "type": "string" },
                        "interval": { "type": "integer", "description": "Interval in milliseconds" },
                        "at": { "type": "integer", "description": "Unix timestamp in milliseconds" },
                        "time": { "type": "string", "description": "Daily HH:mm" }
                    }
                },
                "payload": {},
                "executionTarget": {
                    "type": "string",
                    "enum": ["frontend", "backend", "both"],
                    "default": "frontend"
                },
                "backendActions": {
                    "type": "array",
                    "items": { "type": "object" }
                },
                "missedPolicy": {
                    "type": "string",
                    "enum": ["skip", "run-once", "run-all"],
                    "default": "skip"
                },
                "retry": {
                    "type": "object",
                    "properties": {
                        "maxRetries": { "type": "integer", "minimum": 0 },
                        "retryDelay": { "type": "integer", "minimum": 0 }
                    }
                }
            },
            "required": ["tappId", "name", "scheduleType", "schedule"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string" },
                "tappId": { "type": "string" },
                "nextRun": { "type": "string" }
            }
        }),
        required_permissions: vec!["scheduler:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(300),
        requires_confirmation: true,
        confirmation_message: Some("This will create a scheduled Tapp task".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // Tapp 定时任务列表
    registry.register(Capability {
        id: "scheduler.list".to_string(),
        name: "Scheduled tasks".to_string(),
        description: "List scheduled app tasks.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string" },
                "enabled": { "type": "boolean" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "tasks": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["scheduler:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 立即执行 Tapp 任务
    registry.register(Capability {
        id: "scheduler.trigger".to_string(),
        name: "Run scheduled task".to_string(),
        description: "Run a scheduled app task now.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "Required when task id is not unique" },
                "taskId": { "type": "string" }
            },
            "required": ["taskId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "triggered": { "type": "boolean" },
                "taskId": { "type": "string" },
                "tappId": { "type": "string" }
            }
        }),
        required_permissions: vec!["scheduler:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(1000),
        requires_confirmation: true,
        confirmation_message: Some("This will run a scheduled Tapp task now".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // Agent Heartbeat（HEARTBEAT.md，与 Tapp scheduler 无关）

    registry.register(Capability {
        id: "heartbeat.list".to_string(),
        name: "Heartbeat tasks".to_string(),
        description: "List agent heartbeat tasks.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean", "description": "Optional: filter by enabled" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "tasks": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["system:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    registry.register(Capability {
        id: "heartbeat.create".to_string(),
        name: "Create heartbeat".to_string(),
        description: "Create an agent heartbeat task.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Create, IntentAction::Monitor],
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Display name" },
                "schedule": {
                    "type": "string",
                    "description": "Standard 5-field cron (min hour day month weekday), e.g. 0 9 * * * for 09:00 daily"
                },
                "action": {
                    "type": "string",
                    "description": "Natural-language instruction for Agent when due"
                },
                "enabled": { "type": "boolean", "default": true },
                "id": { "type": "string", "description": "Optional custom id; generated from name if omitted" }
            },
            "required": ["name", "schedule", "action"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "task": { "type": "object" },
                "success": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["system:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    registry.register(Capability {
        id: "heartbeat.update".to_string(),
        name: "Update heartbeat".to_string(),
        description: "Update an agent heartbeat task.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "schedule": { "type": "string", "description": "5-field cron" },
                "action": { "type": "string" },
                "enabled": { "type": "boolean" }
            },
            "required": ["id"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "task": { "type": "object" },
                "success": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["system:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    registry.register(Capability {
        id: "heartbeat.delete".to_string(),
        name: "Delete heartbeat".to_string(),
        description: "Delete an agent heartbeat task.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Delete],
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "deleted": { "type": "boolean" },
                "taskId": { "type": "string" }
            }
        }),
        required_permissions: vec!["system:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(150),
        ..Default::default()
    });

    registry.register(Capability {
        id: "heartbeat.toggle".to_string(),
        name: "Toggle heartbeat".to_string(),
        description: "Enable or disable a heartbeat task.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string" },
                "enabled": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["system:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 系统监控（进程级：内存/uptime/任务计数，非完整主机监控）
    registry.register(Capability {
        id: "system.metrics".to_string(),
        name: "System metrics".to_string(),
        description: "Read process memory, uptime, and task counts.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Query, IntentAction::Monitor],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "memory": { "type": "object" },
                "tasks": { "type": "object" },
                "system": { "type": "object" },
                "scope": { "type": "string" }
            }
        }),
        // Align with HTTP GET /api/metrics (admin_middleware): process metrics are ops-sensitive.
        required_permissions: vec!["system:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 缓存状态
    registry.register(Capability {
        id: "cache.status".to_string(),
        name: "Cache status".to_string(),
        description: "Show cache status for each platform.".to_string(),
        category: CapabilityCategory::SystemOp,
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
                "caches": { "type": "array" },
                "totalSize": { "type": "string" }
            }
        }),
        required_permissions: vec!["cache:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 清除缓存
    registry.register(Capability {
        id: "cache.clear".to_string(),
        name: "Clear cache".to_string(),
        description: "Clear cached data for a platform.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Delete],
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
                "success": { "type": "boolean" },
                "clearedSize": { "type": "string" }
            }
        }),
        required_permissions: vec!["cache:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        ..Default::default()
    });

    // 图片缓存
    registry.register(Capability {
        id: "image.cache".to_string(),
        name: "Image cache".to_string(),
        description: "Cache a remote image, or check or clear the image cache.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Create, IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "External image URL to cache" },
                "action": {
                    "type": "string",
                    "enum": ["status", "clear"],
                    "description": "Without url: list or clear the local image cache"
                }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "localPath": { "type": "string" },
                "cached": { "type": "boolean" },
                "url": { "type": "string" },
                "total_files": { "type": "integer" },
                "cleared_directories": { "type": "integer" }
            }
        }),
        required_permissions: vec!["cache:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 获取配置
    registry.register(Capability {
        id: "config.get".to_string(),
        name: "Read config".to_string(),
        description: "Read public system configuration (no secrets).".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "section": { "type": "string", "enum": ["platforms", "ai", "ui", "all"] }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "config": { "type": "object" }
            }
        }),
        required_permissions: vec!["config:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 系统设置状态
    registry.register(Capability {
        id: "setup.status".to_string(),
        name: "Setup status".to_string(),
        description: "Check setup status (tables and owner; no AI keys).".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "isSetupRequired": { "type": "boolean" },
                "hasDatabase": { "type": "boolean" },
                "hasAdminUser": { "type": "boolean" },
                "missingConfigs": { "type": "array" }
            }
        }),
        required_permissions: vec!["system:admin".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 认证状态
    registry.register(Capability {
        id: "auth.status".to_string(),
        name: "Auth status".to_string(),
        description: "Check sign-in and permission status.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "isAuthenticated": { "type": "boolean" },
                "user": { "type": "object" },
                "linkedPlatforms": { "type": "array" }
            }
        }),
        required_permissions: vec!["auth:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(50),
        ..Default::default()
    });

    // 权限检查
    registry.register(Capability {
        id: "permission.check".to_string(),
        name: "Check permission".to_string(),
        description: "Check granted permissions for this session.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "permission": { "type": "string" },
                "tappId": { "type": "string" }
            },
            "required": ["permission"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "granted": { "type": "boolean" },
                "permission": { "type": "string" },
                "role": { "type": "string" },
                "tappId": { "type": "string" }
            }
        }),
        required_permissions: vec![],
        requires_ai: false,
        estimated_duration_ms: Some(50),
        ..Default::default()
    });

    // 统计概览
    registry.register(Capability {
        id: "stats.overview".to_string(),
        name: "Stats overview".to_string(),
        description: "Read cross-platform stats.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query, IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "platforms": { "type": "object" },
                "totalItems": { "type": "integer" }
            }
        }),
        required_permissions: vec!["platform:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(500),
        ..Default::default()
    });

    // 存储数据
    registry.register(Capability {
        id: "storage.set".to_string(),
        name: "Store data".to_string(),
        description: "Save data to app storage.".to_string(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Create, IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "key": { "type": "string" },
                "value": { "type": "any" },
                "tappId": { "type": "string" }
            },
            "required": ["key", "value"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["storage:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // 数据导出
    registry.register(Capability {
        id: "export.data".to_string(),
        name: "Export data".to_string(),
        description: "Export platform data.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "format": { "type": "string", "enum": ["json", "csv", "markdown"] },
                "dateRange": { "type": "object" }
            },
            "required": ["platform", "format"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "content": { "type": "string" },
                "filename": { "type": "string" }
            }
        }),
        required_permissions: vec!["export:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(2000),
        ..Default::default()
    });

    // 后台任务提交
    registry.register(Capability {
        id: "task.submit".to_string(),
        name: "Submit task".to_string(),
        description: "Submit a background platform-data task.".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "taskType": { "type": "string", "enum": ["fetch", "process", "analyze"] }
            },
            "required": ["platform"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string" },
                "status": { "type": "string" }
            }
        }),
        required_permissions: vec!["task:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        requires_confirmation: true,
        confirmation_message: Some("This will submit a background platform data job".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // 任务状态查询（agent_tasks / TASK_STORE，需 taskId 或返回最近任务列表）
    registry.register(Capability {
        id: "task.status".to_string(),
        name: "Task status".to_string(),
        description: "Read agent task status and progress.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string", "description": "Agent task id" },
                "limit": { "type": "integer", "description": "How many recent tasks to return when taskId is omitted", "default": 20 }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string" },
                "status": { "type": "string" },
                "progress": { "type": "number" },
                "error": { "type": "string" },
                "tasks": { "type": "array" }
            }
        }),
        required_permissions: vec!["task:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });

    // Same synthesize_standalone_tts as POST /api/speech/tts (configured provider; else Tencent).
    registry.register(Capability {
        id: "speech.tts".to_string(),
        name: "Text to speech".to_string(),
        description: "Turn text into speech.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "voice": { "type": "string", "description": "Voice id (Tencent Cloud voice_type)" },
                "speed": { "type": "number", "default": 1.0 }
            },
            "required": ["text"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "audio": { "type": "string", "description": "base64 audio (same as /api/speech/tts)" },
                "duration": { "type": "number" },
                "codec": { "type": "string" },
                "cached": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["speech:tts".to_string()],
        // 走 speech 服务，不依赖 AI analyzer
        requires_ai: false,
        estimated_duration_ms: Some(5000),
        ..Default::default()
    });
}
