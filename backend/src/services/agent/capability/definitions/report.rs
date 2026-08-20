//! 报告系统能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // 报告生成
    registry.register(Capability {
        id: "report.create".to_string(),
        name: "报告生成".to_string(),
        description: "生成数据分析报告".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "data": { "type": "object" },
                "template": { "type": "string" },
                "format": { "type": "string", "enum": ["markdown", "html", "json"] }
            },
            "required": ["title", "data"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "reportId": { "type": "string" },
                "content": { "type": "string" }
            }
        }),
        required_permissions: vec!["report:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(2000),
        ..Default::default()
    });

    // 创建提醒
    registry.register(Capability {
        id: "reminder.create".to_string(),
        name: "创建提醒".to_string(),
        description: "创建定时提醒".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "datetime": { "type": "string", "format": "date-time" },
                "repeat": { "type": "string", "enum": ["none", "daily", "weekly", "monthly"] }
            },
            "required": ["title", "datetime"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "reminderId": { "type": "string" },
                "success": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["reminder:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 创建笔记
    registry.register(Capability {
        id: "note.create".to_string(),
        name: "创建笔记".to_string(),
        description: "保存文本内容为笔记，支持上游步骤输出作为内容".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "content": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "noteId": { "type": "string" },
                "success": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["note:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    // 保存书签
    registry.register(Capability {
        id: "bookmark.save".to_string(),
        name: "保存书签".to_string(),
        description: "保存 URL 书签，自动获取网页标题".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string" },
                "title": { "type": "string" },
                "description": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["url"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "bookmarkId": { "type": "string" },
                "success": { "type": "boolean" }
            }
        }),
        required_permissions: vec!["bookmark:write".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 报告列表
    registry.register(Capability {
        id: "report.list".to_string(),
        name: "报告列表".to_string(),
        description: "列出站点主人的平台报告；无 platform 时附带当前用户 Agent 创建的报告".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "default": 10 },
                "platform": { "type": "string" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "reports": { "type": "array" },
                "total": { "type": "integer" }
            }
        }),
        required_permissions: vec!["report:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });
}
