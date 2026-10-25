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

/// Most changes a confirmation card lists, and the longest text shown for each side.
const PREVIEW_CHANGES: usize = 12;
const PREVIEW_CHARS: usize = 600;

/// What saving these arguments would change, read against the note as it is now: title and
/// category, and the body as paragraph-level before/after pairs. Edits and whole-body
/// rewrites read the same, so the author confirms changes, never a wall of Markdown.
pub(super) async fn change_preview(
    db: &sea_orm::DatabaseConnection,
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let current = phantasi::load_note_doc(db, doc_id(params)?)
        .await
        .map_err(public_error)?;
    let doc = &current["doc"];
    let mut changes = Vec::new();
    for field in ["title", "topic"] {
        if let Some(next) = params.get(field).filter(|next| **next != doc[field]) {
            changes.push(json!({"field": field, "before": doc[field], "after": next}));
        }
    }
    let body = doc["content_md"].as_str().unwrap_or_default();
    let next = match (params.get("edits"), params.get("content_md")) {
        (Some(edits), _) => Some(apply_edits(body, edits)?),
        (None, Some(Value::String(next))) => Some(next.clone()),
        _ => None,
    };
    if let Some(next) = next {
        for (before, after) in block_changes(body, &next) {
            changes.push(json!({"field": "body", "before": clip(&before), "after": clip(&after)}));
        }
    }
    let total = changes.len();
    changes.truncate(PREVIEW_CHANGES);
    Ok(json!({"title": doc["title"], "changes": changes, "total": total}))
}

fn clip(text: &str) -> String {
    if text.chars().count() <= PREVIEW_CHARS {
        return text.to_string();
    }
    format!("{}…", text.chars().take(PREVIEW_CHARS).collect::<String>())
}

/// Line-level differences as (before, after) pairs, consecutive changed lines grouped; an
/// empty side is an insertion or a deletion. In Markdown a paragraph is one line, and link or
/// footnote definitions sit on single lines right under one, so lines (not blank-line blocks)
/// keep an untouched definition out of a changed paragraph. Longest common subsequence.
fn block_changes(before: &str, after: &str) -> Vec<(String, String)> {
    let split = |text: &str| -> Vec<String> {
        text.replace("\r\n", "\n")
            .lines()
            .map(|line| line.trim_end().to_string())
            .filter(|line| !line.trim().is_empty())
            .collect()
    };
    let (a, b) = (split(before), split(after));
    // A pathological size gets one honest whole-body pair rather than a quadratic table.
    if a.len().saturating_mul(b.len()) > 250_000 {
        return if a == b {
            vec![]
        } else {
            vec![(a.join("\n"), b.join("\n"))]
        };
    }
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut changes = Vec::new();
    let (mut gone, mut came) = (Vec::new(), Vec::new());
    let mut flush = |gone: &mut Vec<String>, came: &mut Vec<String>| {
        if !gone.is_empty() || !came.is_empty() {
            changes.push((gone.join("\n"), came.join("\n")));
            gone.clear();
            came.clear();
        }
    };
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            flush(&mut gone, &mut came);
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            came.push(b[j].clone());
            j += 1;
        } else {
            gone.push(a[i].clone());
            i += 1;
        }
    }
    flush(&mut gone, &mut came);
    changes
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
        // Read before the body: whether it can be rewritten in one reply or needs edits.
        if let Some(chars) = doc
            .get("content_md")
            .and_then(Value::as_str)
            .map(|body| body.chars().count())
        {
            doc.insert("content_chars".into(), json!(chars));
        }
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
    fn body_changes_pair_what_went_with_what_came() {
        let before = "# Title\n\nKeep.\n\nOld one.\nOld two.\n\nTail.\n[ref]: https://example.com";
        let after = "# Title\n\nKeep.\n\nNew one.\n\nTail.\n[ref]: https://example.com\n\nAdded.";
        assert_eq!(
            block_changes(before, after),
            vec![
                ("Old one.\nOld two.".to_string(), "New one.".to_string()),
                (String::new(), "Added.".to_string()),
            ]
        );
        assert!(block_changes(before, before).is_empty());
        // Blank lines between paragraphs are layout, not changes.
        assert!(block_changes("A\n\nB", "A\n\n\n\nB\n").is_empty());
    }

    #[test]
    fn a_save_returns_a_receipt_and_a_read_returns_the_markdown() {
        let doc = json!({"success": true, "doc": {"id": 3, "revision": 9, "content_md": "Body", "authors": [], "image": null}});
        let read = for_agent(doc.clone(), true);
        assert_eq!(read["doc"]["content_md"], "Body");
        assert_eq!(read["doc"]["content_chars"], 4);
        assert!(read["doc"].get("authors").is_none());
        let saved = for_agent(doc, false);
        assert_eq!(saved["doc"]["revision"], 9);
        assert!(saved["doc"].get("content_md").is_none());
    }
}
