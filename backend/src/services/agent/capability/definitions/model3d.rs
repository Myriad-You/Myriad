//! Tripo 3D capabilities for Agent.

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    registry.register(Capability {
        id: "model3d.status".to_string(),
        name: "3D status".to_string(),
        description: "Check whether 3D generation is configured (no secrets).".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({ "type": "object", "properties": {} }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "enabled": { "type": "boolean" },
                "configured": { "type": "boolean" },
                "capabilities": { "type": "array", "items": { "type": "string" } }
            }
        }),
        required_permissions: vec!["3d:generate".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(200),
        ..Default::default()
    });

    registry.register(Capability {
        id: "model3d.generate".to_string(),
        name: "Generate 3D model".to_string(),
        description: "Generate a GLB model from an image and store it.".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["image_to_model", "multiview_to_model"],
                    "default": "image_to_model"
                },
                "imageUrl": {
                    "type": "string",
                    "description": "Only this site's /api/brew/image-cache/... path; no arbitrary outbound fetch"
                },
                "imageBase64": { "type": "string" },
                "fileName": { "type": "string" },
                "contentType": { "type": "string" },
                "fileToken": { "type": "string" },
                "views": {
                    "type": "object",
                    "description": "multiview direction → imageUrl / imageBase64 / fileToken"
                },
                "payload": { "type": "object" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string" },
                "status": { "type": "string" },
                "assets": { "type": "array" }
            }
        }),
        required_permissions: vec!["3d:generate".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(180_000),
        ..Default::default()
    });

    registry.register(Capability {
        id: "model3d.rig".to_string(),
        name: "Rig 3D model".to_string(),
        description: "Check or apply a rig on a Tripo job.".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["rig_check", "rig"],
                    "default": "rig"
                },
                "input": { "type": "string", "description": "Upstream task id or file token" },
                "payload": { "type": "object" }
            },
            "required": ["input"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string" },
                "status": { "type": "string" },
                "assets": { "type": "array" }
            }
        }),
        required_permissions: vec!["3d:generate".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(120_000),
        ..Default::default()
    });

    registry.register(Capability {
        id: "model3d.retarget".to_string(),
        name: "Retarget animation".to_string(),
        description: "Retarget animation onto a rigged model.".to_string(),
        category: CapabilityCategory::ResourceCreate,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "input": { "type": "string", "description": "Rigged task id" },
                "animation": { "type": "string" },
                "animations": { "type": "array", "items": { "type": "string" } },
                "payload": { "type": "object" }
            },
            "required": ["input"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "taskId": { "type": "string" },
                "status": { "type": "string" },
                "assets": { "type": "array" }
            }
        }),
        required_permissions: vec!["3d:generate".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(120_000),
        ..Default::default()
    });
}
