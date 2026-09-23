//! Bind business consumers to ready assets. Callers pass an open transaction.

use sea_orm::{ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter, Statement};
use serde_json::Value;
use uuid::Uuid;

use crate::models::entities::{media_assets, media_url_aliases};

use super::assets;
use super::error::MediaError;
use super::references::{NewReference, replace_for_consumer};
use super::types::MediaState;
use super::urls::{
    cite_local_path, compatible_url, content_path, filename_for_mime, media_shaped_path,
};

pub fn extract_registered_paths(text: &str, origins: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for url in myriad_phantasi_notes::markdown_media_urls(text) {
        if let Some(path) = cite_local_path(&url, origins) {
            push_unique(&mut found, path);
        }
    }
    found
}

fn push_unique(found: &mut Vec<String>, path: String) {
    if !found.iter().any(|existing| existing == &path) {
        found.push(path);
    }
}

const STICKER_LAYOUT_KEYS: [&str; 2] = ["standard", "free"];

fn is_sticker_widget(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str) == Some("sticker")
        || item.get("kind").and_then(Value::as_str) == Some("sticker")
}

fn sticker_image_url(item: &Value) -> Option<&str> {
    item.get("config")
        .and_then(Value::as_object)
        .and_then(|config| config.get("imageUrl"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
}

fn walk_sticker_widgets(layout: &Value, mut visit: impl FnMut(&Value)) {
    for key in STICKER_LAYOUT_KEYS {
        let Some(items) = layout.get(key).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            if is_sticker_widget(item) {
                visit(item);
            }
        }
    }
}

pub fn extract_sticker_image_urls(layout: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    walk_sticker_widgets(layout, |item| {
        if let Some(url) = sticker_image_url(item) {
            push_unique(&mut urls, url.to_string());
        }
    });
    urls
}

fn rewrite_sticker_image_urls(layout: &mut Value, rewrite: impl Fn(&str) -> String) {
    for key in STICKER_LAYOUT_KEYS {
        let Some(items) = layout.get_mut(key).and_then(Value::as_array_mut) else {
            continue;
        };
        for item in items {
            if !is_sticker_widget(item) {
                continue;
            }
            let Some(url) = sticker_image_url(item).map(str::to_string) else {
                continue;
            };
            let Some(config) = item.get_mut("config").and_then(Value::as_object_mut) else {
                continue;
            };
            config.insert("imageUrl".into(), Value::String(rewrite(&url)));
        }
    }
}

pub async fn resolve_asset_id(
    db: &impl ConnectionTrait,
    path: &str,
) -> Result<Option<i32>, MediaError> {
    if let Some(id) = parse_content_id(path) {
        if assets::find_by_id(db, id).await?.is_some() {
            return Ok(Some(id));
        }
    }
    if let Some(public_id) = parse_public_id(path) {
        if let Some(row) = assets::find_by_public_id(db, public_id).await? {
            return Ok(Some(row.id));
        }
    }
    if let Some(alias) = media_url_aliases::Entity::find()
        .filter(media_url_aliases::Column::LocalPath.eq(path))
        .one(db)
        .await?
    {
        return Ok(Some(alias.asset_id));
    }
    if let Some(row) = media_assets::Entity::find()
        .filter(media_assets::Column::Url.eq(path))
        .one(db)
        .await?
    {
        return Ok(Some(row.id));
    }
    Ok(None)
}

fn parse_content_id(path: &str) -> Option<i32> {
    let rest = path.strip_prefix("/api/media/")?;
    let id = rest
        .strip_suffix("/content")
        .unwrap_or(rest.split('/').next()?);
    id.parse().ok().filter(|value| *value > 0)
}

fn parse_public_id(path: &str) -> Option<Uuid> {
    let rest = path.strip_prefix("/media/assets/")?;
    Uuid::parse_str(rest.split('/').next()?).ok()
}

pub async fn references_from_fields(
    db: &impl ConnectionTrait,
    origins: &[String],
    cover: Option<&str>,
    body: &str,
    requires_public: bool,
) -> Result<Vec<NewReference>, MediaError> {
    let mut refs = Vec::new();
    if let Some(cover) = cover {
        if let Some(path) = cite_local_path(cover, origins) {
            push_ref(db, &mut refs, &path, "cover", requires_public).await?;
        }
    }
    for (index, path) in extract_registered_paths(body, origins)
        .into_iter()
        .enumerate()
    {
        let slot = format!("body:{index}");
        push_ref(db, &mut refs, &path, &slot, requires_public).await?;
    }
    Ok(refs)
}

pub async fn references_from_urls(
    db: &impl ConnectionTrait,
    origins: &[String],
    urls: &[String],
    slot: impl Fn(usize) -> String,
    requires_public: bool,
) -> Result<Vec<NewReference>, MediaError> {
    let mut refs = Vec::new();
    for (index, url) in urls.iter().enumerate() {
        let Some(path) = cite_local_path(url, origins) else {
            continue;
        };
        let name = slot(index);
        push_ref(db, &mut refs, &path, &name, requires_public).await?;
    }
    Ok(refs)
}

async fn push_ref(
    db: &impl ConnectionTrait,
    refs: &mut Vec<NewReference>,
    path: &str,
    slot: &str,
    requires_public: bool,
) -> Result<(), MediaError> {
    let Some(asset_id) = resolve_asset_id(db, path).await? else {
        // Recognized local URLs must not silently escape deletion protection.
        // Legacy citations become writable after the explicit migration imports
        // them; remote URLs never reach this branch.
        return Err(MediaError::NotReady);
    };
    if refs
        .iter()
        .any(|item| item.asset_id == asset_id && item.slot == slot)
    {
        return Ok(());
    }
    refs.push(NewReference {
        asset_id,
        slot: slot.to_string(),
        requires_public,
        expires_at: None,
    });
    Ok(())
}

pub async fn bind_consumer(
    txn: &impl ConnectionTrait,
    consumer_type: &str,
    consumer_id: impl AsRef<str>,
    refs: &[NewReference],
) -> Result<(), MediaError> {
    replace_for_consumer(txn, consumer_type, consumer_id.as_ref(), refs).await
}

pub async fn bind_note_draft(
    txn: &impl ConnectionTrait,
    doc_id: i32,
    history_since_revision: i64,
    image: Option<&str>,
    content_md: &str,
    origins: &[String],
) -> Result<(), MediaError> {
    let refs = references_from_fields(txn, origins, image, content_md, false).await?;
    bind_consumer(txn, "note_draft", doc_id.to_string(), &refs).await?;
    sync_note_history_refs(txn, doc_id, history_since_revision, origins).await
}

/// RSS downloads remain an evictable cache. Protect any referenced asset that
/// has already entered the durable catalog; do not import every external image.
pub async fn clear_rss_source(
    txn: &impl ConnectionTrait,
    source_id: i32,
) -> Result<(), MediaError> {
    // Block new FK inserts, then serialize with migration's item locks before
    // clearing references and letting the caller cascade-delete the source.
    for sql in [
        "SELECT id FROM phantasi_sources WHERE id = $1 FOR UPDATE",
        "SELECT id FROM phantasi_items WHERE source_id = $1 ORDER BY id FOR UPDATE",
        "DELETE FROM media_references WHERE consumer_type = 'rss_item' AND consumer_id IN (SELECT id::text FROM phantasi_items WHERE source_id = $1)",
    ] {
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [source_id.into()],
        ))
        .await?;
    }
    Ok(())
}

pub async fn bind_rss_item(
    txn: &impl ConnectionTrait,
    item_id: i32,
    payload: &Value,
    origins: &[String],
) -> Result<(), MediaError> {
    let mut strings = Vec::new();
    for key in [
        "image",
        "content",
        "summary",
        "audio_url",
        "video_url",
        "enclosures",
    ] {
        collect_strings(&payload[key], &mut strings);
    }
    let mut paths = Vec::new();
    for text in strings {
        if let Some(path) = cite_local_path(&text, origins) {
            paths.push(path);
        }
        paths.extend(extract_registered_paths(&text, origins));
    }
    paths.sort();
    paths.dedup();
    let mut refs = Vec::new();
    for path in paths {
        let id = match resolve_asset_id(txn, &path).await? {
            Some(id) => Some(id),
            None => match super::legacy::cache_equivalent_path(&path) {
                Some(other) => resolve_asset_id(txn, &other).await?,
                None => None,
            },
        };
        if let Some(asset_id) = id {
            if !refs.iter().any(|r: &NewReference| r.asset_id == asset_id) {
                refs.push(NewReference {
                    asset_id,
                    slot: format!("media:{}", refs.len()),
                    requires_public: true,
                    expires_at: None,
                });
            }
        }
    }
    bind_consumer(txn, "rss_item", item_id.to_string(), &refs).await
}

pub async fn bind_note_published(
    txn: &impl ConnectionTrait,
    item_id: i32,
    image: Option<&str>,
    content_md: &str,
    origins: &[String],
) -> Result<(), MediaError> {
    let refs = references_from_fields(txn, origins, image, content_md, true).await?;
    bind_consumer(txn, "note_published", item_id.to_string(), &refs).await
}

pub async fn bind_persona(
    txn: &impl ConnectionTrait,
    portrait: Option<&str>,
    avatar: Option<&str>,
    visual_profile: Option<&Value>,
    origins: &[String],
) -> Result<(), MediaError> {
    let mut portrait_urls = Vec::new();
    if let Some(portrait) = portrait {
        portrait_urls.push(portrait.to_string());
    }
    if let Some(avatar) = avatar {
        portrait_urls.push(avatar.to_string());
    }
    let portrait_refs = references_from_urls(
        txn,
        origins,
        &portrait_urls,
        |i| format!("portrait:{i}"),
        true,
    )
    .await?;
    bind_consumer(txn, "persona_portrait", "persona", &portrait_refs).await?;
    let mut outfit_urls = Vec::new();
    if let Some(profile) = visual_profile {
        collect_strings(profile, &mut outfit_urls);
    }
    let outfit_refs =
        references_from_urls(txn, origins, &outfit_urls, |i| format!("outfit:{i}"), true).await?;
    bind_consumer(txn, "persona_outfit", "persona", &outfit_refs).await
}

pub(super) fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_strings(item, out);
            }
        }
        _ => {}
    }
}

pub async fn bind_stickers(
    txn: &impl ConnectionTrait,
    layout: &str,
    origins: &[String],
) -> Result<(), MediaError> {
    let layout: Value = serde_json::from_str(layout).unwrap_or(Value::Null);
    let urls = extract_sticker_image_urls(&layout);
    let refs = references_from_urls(txn, origins, &urls, |i| format!("sticker:{i}"), true).await?;
    bind_consumer(txn, "sticker", "dashboard", &refs).await
}

/// Publish sticker images and rewrite `config.imageUrl` to public paths.
pub async fn bind_and_publish_dashboard_layout(
    txn: &impl ConnectionTrait,
    layout_json: &str,
    origins: &[String],
) -> Result<String, MediaError> {
    let rewritten = publish_dashboard_layout(txn, layout_json, origins).await?;
    bind_stickers(txn, &rewritten, origins).await?;
    Ok(rewritten)
}

async fn publish_dashboard_layout(
    txn: &impl ConnectionTrait,
    layout_json: &str,
    origins: &[String],
) -> Result<String, MediaError> {
    let Ok(mut layout) = serde_json::from_str::<Value>(layout_json) else {
        return Ok(layout_json.to_string());
    };
    let urls = extract_sticker_image_urls(&layout);
    let mut ids = Vec::new();
    let mut aliases = Vec::new();
    for url in urls {
        let Some(path) = cite_local_path(&url, origins) else {
            continue;
        };
        if let Some(id) = resolve_asset_id(txn, &path).await? {
            aliases.push((path, id));
            ids.push(id);
        }
    }
    let map = publish_asset_ids(txn, &ids).await?;
    rewrite_sticker_image_urls(&mut layout, |raw| {
        let Some(path) = cite_local_path(raw, origins) else {
            return raw.to_string();
        };
        if let Some((_, id)) = aliases.iter().find(|(from, _)| from == &path) {
            if let Some(to) = map.get(id) {
                return to.clone();
            }
        }
        if let Some(id) = parse_content_id(&path) {
            if let Some(to) = map.get(&id) {
                return to.clone();
            }
        }
        raw.to_string()
    });
    Ok(layout.to_string())
}

pub async fn bind_ai_task(
    txn: &impl ConnectionTrait,
    task_id: &str,
    result: &Value,
    origins: &[String],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), MediaError> {
    let mut urls = Vec::new();
    collect_strings(result, &mut urls);
    let mut refs = references_from_urls(txn, origins, &urls, |_| "result".into(), false).await?;
    for item in &mut refs {
        item.expires_at = expires_at;
    }
    bind_consumer(txn, "ai_task", task_id, &refs).await
}

pub async fn bind_channel_message(
    txn: &impl ConnectionTrait,
    consumer_id: &str,
    payload: &Value,
    origins: &[String],
) -> Result<(), MediaError> {
    let mut urls = Vec::new();
    collect_strings(payload, &mut urls);
    let refs = references_from_urls(txn, origins, &urls, |i| format!("inbound:{i}"), false).await?;
    bind_consumer(txn, "channel_message", consumer_id, &refs).await
}

pub async fn clear_note_doc(
    txn: &impl ConnectionTrait,
    doc_id: i32,
    item_id: Option<i32>,
) -> Result<(), MediaError> {
    replace_for_consumer(txn, "note_draft", &doc_id.to_string(), &[]).await?;
    clear_history_prefix(txn, doc_id).await?;
    if let Some(item_id) = item_id {
        replace_for_consumer(txn, "note_published", &item_id.to_string(), &[]).await?;
    }
    Ok(())
}

/// Bind snapshots captured by this write; retained revisions are immutable.
/// Migration passes zero to rebuild all retained history.
pub(crate) async fn sync_note_history_refs(
    txn: &impl ConnectionTrait,
    doc_id: i32,
    since_revision: i64,
    origins: &[String],
) -> Result<(), MediaError> {
    let rows = txn
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT revision, snapshot FROM phantasi_note_history WHERE doc_id = $1 AND revision >= $2",
            [doc_id.into(), since_revision.into()],
        ))
        .await?;
    // The history trigger prunes old versions in this same document transaction.
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM media_references AS r
         WHERE r.consumer_type = 'note_history' AND r.consumer_id LIKE $1
           AND NOT EXISTS (SELECT 1 FROM phantasi_note_history AS h
             WHERE h.doc_id = $2 AND r.consumer_id = h.doc_id::text || ':' || h.revision::text)",
        [format!("{doc_id}:%").into(), doc_id.into()],
    ))
    .await?;
    for row in rows {
        let revision: i64 = row.try_get("", "revision")?;
        let snapshot: Value = row.try_get("", "snapshot")?;
        let md = snapshot
            .get("content_md")
            .and_then(Value::as_str)
            .unwrap_or("");
        let image = snapshot.get("image").and_then(Value::as_str);
        let refs = references_from_fields(txn, origins, image, md, false).await?;
        replace_for_consumer(txn, "note_history", &format!("{doc_id}:{revision}"), &refs).await?;
    }
    Ok(())
}

async fn clear_history_prefix(txn: &impl ConnectionTrait, doc_id: i32) -> Result<(), MediaError> {
    let prefix = format!("{doc_id}:%");
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM media_references WHERE consumer_type = 'note_history' AND consumer_id LIKE $1",
        [prefix.into()],
    ))
    .await?;
    Ok(())
}

/// Mark cited assets public and rewrite private content paths to public paths.
pub async fn publish_cited_media(
    txn: &impl ConnectionTrait,
    origins: &[String],
    cover: Option<&str>,
    body: &str,
) -> Result<(Option<String>, String), MediaError> {
    let mut paths = extract_registered_paths(body, origins);
    if let Some(cover) = cover {
        if let Some(path) = cite_local_path(cover, origins) {
            paths.push(path);
        }
    }
    let mut ids = Vec::new();
    let mut aliases = Vec::new();
    for path in paths {
        if let Some(id) = resolve_asset_id(txn, &path).await? {
            aliases.push((path, id));
            ids.push(id);
        }
    }
    let map = publish_asset_ids(txn, &ids).await?;
    let rewrite = |raw: &str| -> String {
        let mut out = raw.to_string();
        for (from, id) in &aliases {
            if let Some(to) = map.get(id) {
                out = out.replace(from, to);
            }
        }
        for (id, to) in &map {
            out = out.replace(&content_path(*id), to);
        }
        out
    };
    let body = rewrite(body);
    let cover = cover.map(rewrite).filter(|value| !value.trim().is_empty());
    Ok((cover, body))
}

/// Publish one local URL and return the public path, or the original if it is already public.
pub async fn publish_local_url(
    txn: &impl ConnectionTrait,
    url: &str,
    origins: &[String],
) -> Result<String, MediaError> {
    let (cover, _) = publish_cited_media(txn, origins, Some(url), "").await?;
    Ok(cover
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| url.to_string()))
}

/// Local path of a site-level media setting. Path-only and allowlisted-origin
/// values follow [`cite_local_path`]. A URL under any other origin (typically a
/// previous site domain) counts as local only when its media-shaped path resolves
/// to an asset catalogued here; otherwise it stays an external URL.
pub async fn site_media_local_path(
    db: &impl ConnectionTrait,
    url: &str,
    origins: &[String],
) -> Result<Option<String>, MediaError> {
    if let Some(path) = cite_local_path(url, origins) {
        return Ok(Some(path));
    }
    let Some(path) = media_shaped_path(url) else {
        return Ok(None);
    };
    Ok(resolve_asset_id(db, &path).await?.map(|_| path))
}

/// Saving a wallpaper is publication; bind it in the same transaction as config.
/// Returns the value to store: local media as its path-only public URL (which
/// drops any stale origin), anything else unchanged.
pub async fn bind_and_publish_wallpaper(
    txn: &impl ConnectionTrait,
    url: &str,
    origins: &[String],
) -> Result<String, MediaError> {
    let local = site_media_local_path(txn, url, origins).await?;
    let published = publish_local_url(txn, local.as_deref().unwrap_or(url), origins).await?;
    let refs = references_from_urls(
        txn,
        origins,
        std::slice::from_ref(&published),
        |_| "image".into(),
        true,
    )
    .await?;
    bind_consumer(txn, "site_wallpaper", "site", &refs).await?;
    Ok(published)
}

pub async fn publish_asset_ids(
    txn: &impl ConnectionTrait,
    ids: &[i32],
) -> Result<std::collections::HashMap<i32, String>, MediaError> {
    let locked = assets::lock_by_ids_sorted(txn, ids.to_vec()).await?;
    let mut map = std::collections::HashMap::new();
    for row in locked {
        if MediaState::parse(row.state.as_deref().unwrap_or("")).ok() != Some(MediaState::Ready) {
            return Err(MediaError::NotReady);
        }
        let public_id = row.public_id.ok_or(MediaError::NotReady)?;
        let filename = filename_for_mime(&row.name, &row.mime, public_id)?;
        let public = compatible_url(public_id, &filename);
        assets::mark_public(txn, row.id, &public).await?;
        map.insert(row.id, public);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_only_delimited_registered_paths() {
        let md = "see ![cover](/media/assets/11111111-1111-1111-1111-111111111111/a.png) and <img src=\"/api/phantasi/image-cache/aa/abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd.png\"> plus /media/assets/11111111-1111-1111-1111-111111111111/ignored.png in prose";
        let paths = extract_registered_paths(md, &[]);
        assert_eq!(
            paths,
            vec![
                "/media/assets/11111111-1111-1111-1111-111111111111/a.png".to_string(),
                "/api/phantasi/image-cache/aa/abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd.png"
                    .to_string()
            ]
        );
        assert!(
            extract_registered_paths(
                "![x](https://evil.example/media/assets/11111111-1111-1111-1111-111111111111/a.png)",
                &["https://site.example".into()]
            )
            .is_empty()
        );
    }

    #[test]
    fn extract_reference_style_markdown_images() {
        let md = "![cover][pic]\n\n[pic]: /api/media/9/content \"alt\"\n";
        assert_eq!(
            extract_registered_paths(md, &[]),
            vec!["/api/media/9/content".to_string()]
        );
    }

    #[test]
    fn extract_sticker_urls_from_layout_json_not_markdown() {
        let layout = serde_json::json!({
            "standard": [{
                "type": "clock",
                "config": { "imageUrl": "/api/media/1/content" }
            }],
            "free": [{
                "type": "sticker",
                "kind": "sticker",
                "config": { "imageUrl": "/api/media/4/content" }
            }]
        });
        assert_eq!(
            extract_sticker_image_urls(&layout),
            vec!["/api/media/4/content".to_string()]
        );
        assert!(extract_registered_paths(&layout.to_string(), &[]).is_empty());
    }
}
