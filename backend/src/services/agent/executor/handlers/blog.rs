//! Uses the editor's save path, including revision checks and the web-owned collab hub.
use super::HandlerContext;
use crate::api::phantasi::{self, NoteDocWriteRequest};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(super) async fn execute(
    capability_id: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    // The editor APIs are administrator-only, even if phantasi:admin is delegated.
    if !crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await? {
        return Err("Blog note management requires a current administrator".into());
    }
    let actor = if ctx.user_id == crate::services::agent::SYSTEM_USER_ID {
        crate::services::agent::agent_footer::heartbeat_delegate(ctx.db)
            .await?
            .ok_or("Blog note management requires a site owner")?
    } else {
        ctx.user_id
    };
    let result = match capability_id {
        "blog.notes" => {
            let mut result = phantasi::load_note_docs(ctx.db)
                .await
                .map_err(public_error)?;
            let query = params
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_lowercase();
            let status = params.get("status").and_then(Value::as_str);
            let limit = params
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(20)
                .clamp(1, 100) as usize;
            let docs = result["docs"]
                .as_array_mut()
                .ok_or("Could not read blog notes")?;
            docs.retain(|doc| {
                status.is_none_or(|status| doc["status"].as_str() == Some(status))
                    && (query.is_empty()
                        || ["title", "topic"].iter().any(|key| {
                            doc[*key]
                                .as_str()
                                .is_some_and(|text| text.to_lowercase().contains(&query))
                        }))
            });
            let total = docs.len();
            docs.truncate(limit);
            result["total"] = json!(total);
            return Ok(result);
        }
        "blog.note" => phantasi::load_note_doc(ctx.db, doc_id(params)?)
            .await
            .map(|result| for_agent(result, true)),
        "blog.note.update" => {
            if !["title", "content_md", "edits", "topic"]
                .iter()
                .any(|key| params.contains_key(*key))
            {
                return Err("Provide at least one of title, edits, content_md or topic".into());
            }
            // Allow only the advertised draft fields, never publishing or changing a slug.
            let mut patch = serde_json::Map::new();
            for key in ["revision", "title", "content_md", "topic"] {
                if let Some(value) = params.get(key) {
                    patch.insert(key.into(), value.clone());
                }
            }
            if let Some(edits) = params.get("edits") {
                if params.contains_key("content_md") {
                    return Err("Send either edits or content_md, not both".into());
                }
                let current = phantasi::load_note_doc(ctx.db, doc_id(params)?)
                    .await
                    .map_err(public_error)?;
                // Edits are written against one revision; a newer one means the text moved.
                if params.get("revision") != Some(&current["doc"]["revision"]) {
                    return Err(format!(
                        "The note changed since it was read (now revision {}); read blog.note again and redo the edits",
                        current["doc"]["revision"]
                    ));
                }
                let source = current["doc"]["content_md"].as_str().unwrap_or_default();
                patch.insert("content_md".into(), json!(apply_edits(source, edits)?));
            }
            patch.insert(
                "client_request_id".into(),
                json!(uuid::Uuid::new_v4().to_string()),
            );
            let request: NoteDocWriteRequest = serde_json::from_value(Value::Object(patch))
                .map_err(|_| "Invalid blog note update")?;
            phantasi::save_note_doc(ctx.db, actor, doc_id(params)?, request)
                .await
                .map(|result| for_agent(result, false))
        }
        _ => return Err("Unknown blog capability".into()),
    };
    result.map_err(public_error)
}

/// Applies find/replace pairs in order. Each find must match exactly once in the text as it
/// stands after the previous edits, so an edit can never land somewhere the Agent did not mean.
fn apply_edits(source: &str, edits: &Value) -> Result<String, String> {
    let edits = edits.as_array().ok_or("edits must be a list")?;
    let mut text = source.to_string();
    for (index, edit) in edits.iter().enumerate() {
        let number = index + 1;
        let (Some(find), Some(replace)) = (edit["find"].as_str(), edit["replace"].as_str()) else {
            return Err(format!("Edit {number} needs find and replace text"));
        };
        if find.is_empty() {
            return Err(format!("Edit {number} has an empty find"));
        }
        match text.matches(find).count() {
            1 => text = text.replacen(find, replace, 1),
            0 => {
                return Err(format!(
                    "Edit {number}: the find text is not in the note (after earlier edits). Copy it exactly from blog.note; no edit was saved"
                ));
            }
            count => {
                return Err(format!(
                    "Edit {number}: the find text appears {count} times. Include more surrounding text so it matches once; no edit was saved"
                ));
            }
        }
    }
    Ok(text)
}

/// Every tool result is resent on each later model turn. Keep what editing needs, and after
/// a save only its receipt: echoing the Markdown just written would carry the whole note in
/// the conversation a second time.
fn for_agent(mut result: Value, with_content: bool) -> Value {
    const FIELDS: [&str; 9] = [
        "id",
        "revision",
        "title",
        "topic",
        "status",
        "slug",
        "published_at",
        "scheduled_at",
        "updated_at",
    ];
    if let Some(doc) = result.get_mut("doc").and_then(Value::as_object_mut) {
        doc.retain(|key, _| {
            FIELDS.contains(&key.as_str()) || (with_content && key == "content_md")
        });
    }
    result
}

fn doc_id(params: &HashMap<String, Value>) -> Result<i32, String> {
    params
        .get("docId")
        .and_then(Value::as_i64)
        .and_then(|id| i32::try_from(id).ok())
        .filter(|id| *id > 0)
        .ok_or_else(|| "A valid blog document docId is required".into())
}

fn public_error(error: crate::error::HttpError) -> String {
    error.0.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_apply_in_order_and_each_must_match_once() {
        let source = "# Title\n\nOne. Two.\n\nOne.";
        let edits = json!([
            {"find": "# Title", "replace": "## Title"},
            {"find": "Two.", "replace": "Two!"},
            {"find": "Two!\n\nOne.", "replace": "Two!\n\nOnly one."}
        ]);
        assert_eq!(
            apply_edits(source, &edits).unwrap(),
            "## Title\n\nOne. Two!\n\nOnly one."
        );
        let ambiguous =
            apply_edits(source, &json!([{"find": "One.", "replace": "1"}])).unwrap_err();
        assert!(ambiguous.contains("appears 2 times"));
        let missing = apply_edits(source, &json!([{"find": "Three", "replace": "3"}])).unwrap_err();
        assert!(missing.contains("not in the note"));
        // A later edit sees the text after the earlier ones, not the original.
        let stale = apply_edits(
            source,
            &json!([{"find": "# Title", "replace": "Heading"}, {"find": "# Title", "replace": "x"}]),
        )
        .unwrap_err();
        assert!(stale.starts_with("Edit 2"));
    }

    #[test]
    fn a_save_returns_a_receipt_and_a_read_returns_the_markdown() {
        let doc = json!({"success": true, "doc": {"id": 3, "revision": 9, "content_md": "Body", "authors": [], "image": null}});
        let read = for_agent(doc.clone(), true);
        assert_eq!(read["doc"]["content_md"], "Body");
        assert!(read["doc"].get("authors").is_none());
        let saved = for_agent(doc, false);
        assert_eq!(saved["doc"]["revision"], 9);
        assert!(saved["doc"].get("content_md").is_none());
    }
}
