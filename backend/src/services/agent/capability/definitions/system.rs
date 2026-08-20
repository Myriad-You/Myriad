//! 系统操作能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // 数据转换
    registry.register(Capability {
        id: "data.transform".to_string(),
        name: "数据转换".to_string(),
        description: "对数据进行过滤、排序、聚合等操作".to_string(),
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
        name: "创建 Tapp 定时任务".to_string(),
        description: "为已安装的 Tapp 创建真实可执行的定时任务（Tapp 调度器，非 Agent Heartbeat）"
            .to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Monitor, IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "任务所属的已安装 Tapp ID" },
                "taskId": { "type": "string", "description": "Tapp 内唯一任务 ID；省略时自动生成" },
                "name": { "type": "string" },
                "scheduleType": {
                    "type": "string",
                    "enum": ["cron", "interval", "once", "daily"]
                },
                "schedule": {
                    "type": "object",
                    "properties": {
                        "cron": { "type": "string" },
                        "interval": { "type": "integer", "description": "间隔毫秒" },
                        "at": { "type": "integer", "description": "Unix 毫秒时间戳" },
                        "time": { "type": "string", "description": "每日 HH:mm" }
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
        confirmation_message: Some("即将创建 Tapp 定时任务".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // Tapp 定时任务列表
    registry.register(Capability {
        id: "scheduler.list".to_string(),
        name: "Tapp 定时任务列表".to_string(),
        description: "获取当前用户的 Tapp 定时任务列表（非 Agent Heartbeat）".to_string(),
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
        name: "立即执行 Tapp 任务".to_string(),
        description: "立即触发当前用户的指定 Tapp 定时任务".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "tappId": { "type": "string", "description": "任务 ID 不唯一时必须提供" },
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
        confirmation_message: Some("即将立即触发 Tapp 定时任务".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // Agent Heartbeat（HEARTBEAT.md，与 Tapp scheduler 无关）

    registry.register(Capability {
        id: "heartbeat.list".to_string(),
        name: "心跳任务列表".to_string(),
        description: "列出 Agent Heartbeat 定时任务（HEARTBEAT.md，按 cron 主动执行自然语言指令）"
            .to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean", "description": "可选：按启用状态过滤" }
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
        name: "创建心跳任务".to_string(),
        description: "创建 Agent Heartbeat 定时任务。用户说「定时」「每天」「每隔」「心跳」时用这个，而非 scheduler.create。schedule 为 5 字段 cron，action 为自然语言指令".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Create, IntentAction::Monitor],
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "任务显示名称" },
                "schedule": {
                    "type": "string",
                    "description": "标准 5 字段 cron（分 时 日 月 星期），如 0 9 * * * 表示每天 9:00"
                },
                "action": {
                    "type": "string",
                    "description": "到期时 Agent 执行的自然语言指令"
                },
                "enabled": { "type": "boolean", "default": true },
                "id": { "type": "string", "description": "可选自定义 id；省略则从 name 生成" }
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
        name: "更新心跳任务".to_string(),
        description: "按 id 更新 Agent Heartbeat 任务的 name/schedule/action/enabled".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "schedule": { "type": "string", "description": "5 字段 cron" },
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
        name: "删除心跳任务".to_string(),
        description: "按 id 删除 Agent Heartbeat 定时任务".to_string(),
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
        name: "切换心跳任务".to_string(),
        description: "启用或禁用指定的 Agent Heartbeat 任务".to_string(),
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
        name: "系统监控".to_string(),
        description: "获取本进程运行状态：内存、uptime、后台/agent 任务计数（非完整主机监控）"
            .to_string(),
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
        name: "缓存状态".to_string(),
        description: "获取各平台缓存状态".to_string(),
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
        name: "清除缓存".to_string(),
        description: "清除指定平台的缓存数据".to_string(),
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
        name: "图片缓存".to_string(),
        description: "缓存外部图片到本地".to_string(),
        category: CapabilityCategory::SystemOp,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" }
            },
            "required": ["url"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "localPath": { "type": "string" },
                "cached": { "type": "boolean" }
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
        name: "获取配置".to_string(),
        description: "获取系统配置。AI 为 Standard（enabled/provider/model，不含密钥）；platforms 为接通标志；ui 为公开展示字段".to_string(),
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
        name: "系统设置状态".to_string(),
        description: "检查系统初始化状态（库表与管理员；与 HTTP /api/setup/status 一致，不含 AI 钥）".to_string(),
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
        name: "认证状态".to_string(),
        description: "检查用户认证和权限状态".to_string(),
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
        name: "权限检查".to_string(),
        description: "检查当前会话角色的授予权限；带 tappId 时再与该安装的批准权限求交".to_string(),
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
        name: "统计概览".to_string(),
        description: "获取跨平台数据统计概览".to_string(),
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
        name: "存储数据".to_string(),
        description: "保存数据到 Tapp 存储".to_string(),
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
        name: "数据导出".to_string(),
        description: "导出平台数据为指定格式".to_string(),
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
        name: "提交后台任务".to_string(),
        description: "提交平台数据处理任务".to_string(),
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
        confirmation_message: Some("即将提交后台平台数据处理任务".to_string()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });

    // 任务状态查询（agent_tasks / TASK_STORE，需 taskId 或返回最近任务列表）
    registry.register(Capability {
        id: "task.status".to_string(),
        name: "任务状态查询".to_string(),
        description: "查询 agent 任务状态与进度（按 taskId；省略则返回当前用户最近任务）"
            .to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string", "description": "Agent 任务 ID" },
                "limit": { "type": "integer", "description": "未指定 taskId 时返回的最近任务数", "default": 20 }
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

    // 语音服务（与 /api/speech/tts 同一实现：缓存 + 腾讯云合成）
    registry.register(Capability {
        id: "speech.tts".to_string(),
        name: "文字转语音".to_string(),
        description: "将文字转为语音（与产品 /api/speech/tts 相同路径，返回 base64 音频）"
            .to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "voice": { "type": "string", "description": "音色 ID（腾讯云 voice_type）" },
                "speed": { "type": "number", "default": 1.0 }
            },
            "required": ["text"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "audio": { "type": "string", "description": "base64 音频（与 /api/speech/tts 一致）" },
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
