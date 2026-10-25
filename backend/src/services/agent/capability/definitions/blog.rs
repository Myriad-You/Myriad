//! Studio documents are the source of Hugo posts, distinct from personal note.create.
use super::super::CapabilityRegistry;
use crate::services::agent::types::*;
use serde_json::json;

pub fn register(registry: &mut CapabilityRegistry) {
    registry.register(Capability {
        id: "blog.notes".into(),
        name: "Find blog notes".into(),
        description: "Find Hugo blog / 创作台笔记 documents, including drafts, scheduled and published notes. Returns document ids and revisions, not full Markdown. Use blog.note to read before editing; note.create saves personal notes instead.".into(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({"type":"object","properties":{
            "query":{"type":"string","maxLength":200,"description":"Match title or category"},
            "status":{"type":"string","enum":["draft","scheduled","published"]},
            "limit":{"type":"integer","minimum":1,"maximum":100,"default":20}
        },"additionalProperties":false}),
        output_schema: json!({"type":"object","properties":{"docs":{"type":"array"},"total":{"type":"integer"}}}),
        required_permissions: vec!["phantasi:admin".into()],
        ..Default::default()
    });
    registry.register(Capability {
        id: "blog.note".into(),
        name: "Read blog note".into(),
        description: "Read the complete editable Markdown and current revision of one Hugo blog / studio document. docId is the document id (page metadata noteDocId), NOT the published article itemId. Always read this before blog.note.update.".into(),
        category: CapabilityCategory::DataRead,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({"type":"object","properties":{
            "docId":{"type":"integer","minimum":1,"maximum":2147483647}
        },"required":["docId"],"additionalProperties":false}),
        output_schema: json!({"type":"object","properties":{"doc":{"type":"object"}}}),
        required_permissions: vec!["phantasi:admin".into()],
        ..Default::default()
    });
    registry.register(Capability {
        id: "blog.note.update".into(),
        name: "Edit blog note".into(),
        description: "Revise a Hugo blog / 创作台笔记 document's title, Markdown or category. Read blog.note first and pass its revision. Change the Markdown with edits (exact find/replace pairs, each find copied verbatim from the current Markdown and matching exactly once) for typesetting, fixes and any partial change; send content_md only to replace the whole body. Never both. Omitted fields stay unchanged; topic=null clears the category. A revision conflict requires reading again and reconciling, never blindly retrying. Saves the editable document and syncs open editors; does not publish, unschedule or change the live post. Publishing is a separate editor action. Returns the new revision and fields, not the Markdown just sent.".into(),
        category: CapabilityCategory::DataWrite,
        supported_actions: vec![IntentAction::Update],
        input_schema: json!({"type":"object","properties":{
            "docId":{"type":"integer","minimum":1,"maximum":2147483647},
            "revision":{"type":"integer","minimum":1},
            "title":{"type":"string","maxLength":200},
            "content_md":{"type":"string","maxLength":200000,"description":"The complete replacement Markdown, preserving unrelated content. Only for whole-body rewrites; prefer edits"},
            "edits":{"type":"array","minItems":1,"maxItems":100,"description":"Applied in order to the current Markdown. Each find must appear exactly once; include surrounding text to make it unique","items":{"type":"object","properties":{
                "find":{"type":"string","minLength":1,"maxLength":20000},
                "replace":{"type":"string","maxLength":20000}
            },"required":["find","replace"],"additionalProperties":false}},
            "topic":{"type":["string","null"],"maxLength":200}
        },"required":["docId","revision"],"additionalProperties":false}),
        output_schema: json!({"type":"object","properties":{"success":{"type":"boolean"},"doc":{"type":"object"}}}),
        required_permissions: vec!["phantasi:admin".into()],
        requires_confirmation: true,
        confirmation_message: Some("Save changes to this blog note's editable document".into()),
        risk_level: RiskLevel::Medium,
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blog_documents_keep_admin_grants_and_confirm_mutations() {
        let registry = CapabilityRegistry::new();
        for id in ["blog.notes", "blog.note", "blog.note.update"] {
            let cap = registry.get(id).unwrap();
            assert_eq!(cap.required_permissions, vec!["phantasi:admin"]);
            assert_eq!(cap.requires_confirmation, id == "blog.note.update");
            assert!(crate::persona::web_control::is_web_owned(id));
        }
    }
}
