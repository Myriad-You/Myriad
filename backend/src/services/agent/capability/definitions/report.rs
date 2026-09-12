//! Agent 报告与相关资源 capability 注册

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // 报告生成
    registry.register(Capability {
        id: "report.create".to_string(),
        name: "Create report".to_string(),
        description: "Generate an analysis report.".to_string(),
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
        name: "Create reminder".to_string(),
        description: "Create a reminder.".to_string(),
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
        name: "Create note".to_string(),
        description: "Save text as a note.".to_string(),
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
        name: "Save bookmark".to_string(),
        description: "Save a URL bookmark and fetch its title.".to_string(),
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
        name: "Report list".to_string(),
        description: "List platform reports.".to_string(),
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
