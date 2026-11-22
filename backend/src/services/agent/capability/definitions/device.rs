//! What she may ask of the owner's devices from Work (contract `calls`,
//! `services::agent::link::calls`). Only actions enabled on the site are
//! here; a Work run sees one only while a device of the person it runs for
//! is linked and was told it is granted it. Chat turns use no tools.

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    registry.register(Capability {
        id: "device.screen.look".to_string(),
        name: "Look at the screen".to_string(),
        description: "One look at what the owner has open on their computer now: the app, the window and, for a chat window, only who they are talking with and about what. Read-only. Only for the owner, never in a group.".to_string(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "facts": { "type": "object" },
                "summary": { "type": "string" }
            }
        }),
        required_permissions: Vec::new(),
        requires_ai: false,
        estimated_duration_ms: Some(5_000),
        ..Default::default()
    });
    registry.register(Capability {
        id: "device.notify.post".to_string(),
        name: "Notify on the computer".to_string(),
        description: "A notification on the owner's computer, through its own do-not-disturb: held back now, it is not shown. For the owner or her own doing, never in a group.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "A short title" },
                "body": { "type": "string", "description": "What the notification says" }
            },
            "required": ["title", "body"],
            "additionalProperties": false
        }),
        output_schema: json!({
            "type": "object",
            "properties": { "shown": { "type": "boolean" } }
        }),
        required_permissions: Vec::new(),
        requires_ai: false,
        estimated_duration_ms: Some(2_000),
        ..Default::default()
    });
    registry.register(Capability {
        id: "device.voice.speak".to_string(),
        name: "Say aloud on the computer".to_string(),
        description: "She says text aloud on the owner's computer, in her own voice there: delivered says whether it was heard out (a long text may be stopped before its end). For the owner or her own doing, never in a group.".to_string(),
        category: CapabilityCategory::UiControl,
        supported_actions: vec![IntentAction::Execute],
        input_schema: json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "What she says" }
            },
            "required": ["text"],
            "additionalProperties": false
        }),
        output_schema: json!({
            "type": "object",
            "properties": { "delivered": { "type": "boolean" } }
        }),
        required_permissions: Vec::new(),
        requires_ai: false,
        estimated_duration_ms: Some(20_000),
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    fn sources(dir: &Path, found: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                found.push((path.display().to_string(), text));
            }
        }
    }

    /// Every device capability runs where her link to the devices is: a
    /// persona worker of its own process hands each to the web process.
    #[test]
    fn every_device_capability_is_the_web_process_own() {
        let registry = crate::services::agent::capability::CapabilityRegistry::new();
        let mut ids: Vec<&str> = registry
            .get_all()
            .into_iter()
            .map(|cap| cap.id.as_str())
            .filter(|id| id.starts_with("device."))
            .collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            [
                "device.notify.post",
                "device.screen.look",
                "device.voice.speak"
            ]
        );
        for id in ids {
            assert!(crate::persona::web_control::is_web_owned(id), "{id}");
        }
    }

    /// Her chat turns use no tools: only Work calls the model with tools, so
    /// what she may ask of a device is never offered to a chat turn.
    #[test]
    fn only_work_calls_the_model_with_tools() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found = Vec::new();
        sources(&src, &mut found);
        let needle = [".tool", "_turn("].concat();
        let mut callers: Vec<String> = found
            .into_iter()
            .filter(|(_, text)| text.contains(&needle))
            .map(|(path, _)| {
                path.trim_start_matches(&src.display().to_string())
                    .to_string()
            })
            .collect();
        callers.sort();
        // The analyzer that makes the call, Work's own wrapper of it, and the
        // Work loop.
        assert_eq!(
            callers,
            [
                "/services/agent/work_call.rs",
                "/services/agent/work_loop/mod.rs",
                "/services/analyzer/tool_calling.rs",
            ]
        );
    }
}
