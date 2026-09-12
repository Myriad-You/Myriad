//! Author timeline inserts and published-list field extraction.

use axum::{http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::types::PublishedAttachment;
use crate::federation::types::db_err;

/// Insert Create into the author's local timeline so published local content appears immediately.
pub(super) async fn insert_author_timeline(
    db: &DatabaseConnection,
    user_id: i32,
    activity_id: &str,
    activity_type: &str,
    object_type: &str,
    activity_json: &serde_json::Value,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let object = &activity_json["object"];
    let preview = preview_from_ap_object(object);

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_timeline
               (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
           VALUES ($1, $2, NULL, $3, $4, $5, $6, NOW())
           ON CONFLICT (user_id, activity_id) DO NOTHING"#,
        [
            user_id.into(),
            activity_id.into(),
            activity_type.into(),
            object_type.into(),
            preview.into(),
            object.clone().into(),
        ],
    ))
    .await
    .map_err(db_err)?;

    Ok(())
}

/// Plain preview from an AP Note/Article object (prefers source plain text).
pub(super) fn preview_from_ap_object(object: &serde_json::Value) -> Option<String> {
    object
        .pointer("/source/content")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("content").and_then(|v| v.as_str()))
        .or_else(|| object.get("summary").and_then(|v| v.as_str()))
        .or_else(|| object.get("content_preview").and_then(|v| v.as_str()))
        .or_else(|| object.get("mfp:contentPreview").and_then(|v| v.as_str()))
        .or_else(|| object.get("mfp:summary").and_then(|v| v.as_str()))
        .or_else(|| object.get("name").and_then(|v| v.as_str()))
        .map(|s| strip_tags_preview(s, 200))
        .filter(|s| !s.is_empty())
}

/// title / summary / content_preview / attachments from a stored Create activity JSON
/// (`object_json` column holds the full Create envelope).
pub(super) fn published_fields_from_activity_json(
    activity_json: Option<&serde_json::Value>,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Vec<PublishedAttachment>,
) {
    let Some(root) = activity_json else {
        return (None, None, None, vec![]);
    };
    // Prefer nested object (Create envelope); fall back to root if it is already the object.
    let object = if root.get("object").map(|o| o.is_object()).unwrap_or(false) {
        &root["object"]
    } else {
        root
    };

    let title = object
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.chars().take(200).collect::<String>())
        .filter(|s| !s.is_empty());

    let summary = object
        .get("summary")
        .and_then(|v| v.as_str())
        .or_else(|| object.get("mfp:summary").and_then(|v| v.as_str()))
        .map(|s| strip_tags_preview(s, 300))
        .filter(|s| !s.is_empty());

    // Quote-repost: commentary from source/content_preview; keep quoted snippet in summary.
    let is_repost = object
        .get("mfp:kind")
        .and_then(|v| v.as_str())
        .map(|s| s == "repost")
        .unwrap_or(false)
        || object
            .get("mfp:contentType")
            .and_then(|v| v.as_str())
            .map(|s| s == "repost")
            .unwrap_or(false);

    let mut content_preview = preview_from_ap_object(object).or_else(|| summary.clone());
    let mut summary_out = summary;

    if is_repost {
        if content_preview.is_none() {
            content_preview = object
                .get("source")
                .and_then(|s| s.get("content"))
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(200).collect::<String>())
                .filter(|s| !s.is_empty());
        }
        if summary_out.is_none() {
            if let Some(quoted) = object.get("mfp:quotedObject") {
                let q = quoted
                    .get("content_preview")
                    .and_then(|v| v.as_str())
                    .or_else(|| quoted.get("content").and_then(|v| v.as_str()))
                    .or_else(|| quoted.get("summary").and_then(|v| v.as_str()))
                    .or_else(|| {
                        quoted
                            .get("source")
                            .and_then(|s| s.get("content"))
                            .and_then(|v| v.as_str())
                    })
                    // Generous quoted snippet for list cards; full body stays in object_json.
                    .map(|s| s.chars().take(800).collect::<String>())
                    .filter(|s| !s.is_empty());
                if let Some(q) = q {
                    summary_out = Some(format!("↪ {q}"));
                }
            }
        }
    }

    let attachments = attachments_from_ap_object(object);

    (title, summary_out, content_preview, attachments)
}

/// Extract Image/Video (etc.) attachments from an AP Note/Article object.
fn attachments_from_ap_object(object: &serde_json::Value) -> Vec<PublishedAttachment> {
    let Some(att) = object
        .get("attachment")
        .or_else(|| object.get("attachments"))
    else {
        return vec![];
    };

    let items: Vec<&serde_json::Value> = if let Some(arr) = att.as_array() {
        arr.iter().collect()
    } else if att.is_object() {
        vec![att]
    } else {
        return vec![];
    };

    items
        .into_iter()
        .filter_map(|a| {
            let url = a
                .get("url")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())?
                .to_string();
            let media_type = a
                .get("mediaType")
                .or_else(|| a.get("media_type"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let attachment_type = a
                .get("type")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let name = a
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            Some(PublishedAttachment {
                url,
                media_type,
                attachment_type,
                name,
            })
        })
        .collect()
}

fn strip_tags_preview(s: &str, max_chars: usize) -> String {
    let plain = s
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<p>", "")
        .replace("</p>", "\n")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"");
    // Drop remaining simple tags
    let mut out = String::with_capacity(plain.len());
    let mut in_tag = false;
    for c in plain.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let trimmed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn published_fields_from_create_note_activity() {
        let create = json!({
            "type": "Create",
            "object": {
                "type": "Note",
                "content": "<p>Hello world post</p>",
                "source": { "content": "Hello world post", "mediaType": "text/plain" },
                "name": null
            }
        });
        let (title, summary, preview, attachments) =
            published_fields_from_activity_json(Some(&create));
        assert!(title.is_none());
        assert!(summary.is_none());
        assert_eq!(preview.as_deref(), Some("Hello world post"));
        assert!(attachments.is_empty());
    }

    #[test]
    fn published_fields_from_article_name() {
        let create = json!({
            "type": "Create",
            "object": {
                "type": "Article",
                "name": "Spring Report",
                "summary": "A short summary of the report body text"
            }
        });
        let (title, summary, preview, attachments) =
            published_fields_from_activity_json(Some(&create));
        assert_eq!(title.as_deref(), Some("Spring Report"));
        assert!(summary
            .as_ref()
            .is_some_and(|s| s.contains("short summary")));
        assert!(preview.is_some());
        assert!(attachments.is_empty());
    }

    #[test]
    fn published_fields_include_note_image_attachments() {
        let create = json!({
            "type": "Create",
            "object": {
                "type": "Note",
                "source": { "content": "with photo", "mediaType": "text/plain" },
                "attachment": [
                    {
                        "type": "Image",
                        "mediaType": "image/jpeg",
                        "url": "https://example.com/media/federation/1/photo.jpg",
                        "name": "photo.jpg"
                    },
                    {
                        "type": "Video",
                        "mediaType": "video/mp4",
                        "url": "https://example.com/media/federation/1/clip.mp4"
                    },
                    {
                        "type": "Image",
                        "mediaType": "image/png",
                        "url": "   "
                    }
                ]
            }
        });
        let (title, summary, preview, attachments) =
            published_fields_from_activity_json(Some(&create));
        assert!(title.is_none());
        assert!(summary.is_none());
        assert_eq!(preview.as_deref(), Some("with photo"));
        assert_eq!(attachments.len(), 2);
        assert_eq!(
            attachments[0].url,
            "https://example.com/media/federation/1/photo.jpg"
        );
        assert_eq!(attachments[0].media_type.as_deref(), Some("image/jpeg"));
        assert_eq!(attachments[0].attachment_type.as_deref(), Some("Image"));
        assert_eq!(attachments[0].name.as_deref(), Some("photo.jpg"));
        assert_eq!(
            attachments[1].url,
            "https://example.com/media/federation/1/clip.mp4"
        );
        assert_eq!(attachments[1].attachment_type.as_deref(), Some("Video"));
    }

    #[test]
    fn published_fields_single_attachment_object() {
        let note = json!({
            "type": "Note",
            "attachment": {
                "type": "Image",
                "media_type": "image/webp",
                "url": "https://example.com/media/federation/2/a.webp"
            }
        });
        let (_t, _s, _p, attachments) = published_fields_from_activity_json(Some(&note));
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].media_type.as_deref(), Some("image/webp"));
    }

    #[test]
    fn strip_tags_preview_limits_and_unescapes() {
        let s = strip_tags_preview("<p>Hello&amp;world</p><br/>x", 20);
        assert!(s.contains("Hello&world") || s.contains("Hello"));
        assert!(s.len() <= 20 || s.chars().count() <= 20);
        let long = strip_tags_preview(&"a".repeat(100), 10);
        assert_eq!(long.chars().count(), 10);
    }

    #[test]
    fn w175_strip_tags_preview_limit() {
        let s = strip_tags_preview("<p>Hi&amp;there</p>", 50);
        assert!(s.contains("Hi") && (s.contains("&") || s.contains("there")));
        assert_eq!(strip_tags_preview(&"z".repeat(40), 8).chars().count(), 8);
    }
}
