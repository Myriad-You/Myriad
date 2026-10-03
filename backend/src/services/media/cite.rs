//! Consumer-shaped wrappers over [`super::binding::bind`]: each names its
//! consumer and how it extracts citations. Callers pass an open transaction.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;
use uuid::Uuid;

use super::assets;
use super::binding::{Authority, Bound, Citations, Consumer, Unresolved, bind};
use super::error::MediaError;
use super::references::replace_for_consumer;
use super::types::{MediaActor, MediaState};
use super::urls::{cite_local_path, compatible_url, filename_for_mime, media_shaped_path};

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
    if let Some(public_id) = super::cache_import::cached_public_id(path) {
        if let Some(row) = assets::find_by_public_id(db, public_id).await? {
            return Ok(Some(row.id));
        }
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

pub async fn bind_note_draft(
    txn: &impl ConnectionTrait,
    doc_id: i32,
    history_since_revision: i64,
    image: Option<&str>,
    content_md: &str,
    origins: &[String],
) -> Result<(), MediaError> {
    let citations = Citations::fields(origins, image, content_md);
    bind(
        txn,
        &Consumer::note_draft(doc_id),
        &citations,
        Authority::Site,
        Unresolved::Reject,
    )
    .await?;
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

/// RSS content is external; only media that already entered the durable
/// catalog as public is protected. Nothing is imported or published here: a
/// feed can name any local URL, including another user's private draft.
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
    let mut citations = Citations::new();
    for text in strings {
        if let Some(path) = cite_local_path(&text, origins) {
            citations.push_path("media", path);
        }
        for path in extract_registered_paths(&text, origins) {
            citations.push_path("media", path);
        }
    }
    bind(
        txn,
        &Consumer::rss_item(item_id),
        &citations,
        Authority::External,
        Unresolved::Skip,
    )
    .await
    .map(|_| ())
}

pub async fn bind_note_published(
    txn: &impl ConnectionTrait,
    item_id: i32,
    image: Option<&str>,
    content_md: &str,
    origins: &[String],
) -> Result<(), MediaError> {
    bind(
        txn,
        &Consumer::note_published(item_id),
        &Citations::fields(origins, image, content_md),
        Authority::Site,
        Unresolved::Reject,
    )
    .await
    .map(|_| ())
}

/// Portrait, avatar and every image the visual profile names are shown on
/// public pages, so all of them are published.
pub async fn bind_persona(
    txn: &impl ConnectionTrait,
    portrait: Option<&str>,
    avatar: Option<&str>,
    visual_profile: Option<&Value>,
    origins: &[String],
) -> Result<(), MediaError> {
    let portraits: Vec<&str> = portrait.into_iter().chain(avatar).collect();
    bind(
        txn,
        &Consumer::persona_portrait(),
        &Citations::urls(origins, &portraits, |i| format!("portrait:{i}")),
        Authority::Site,
        Unresolved::Reject,
    )
    .await?;
    let outfits = visual_profile
        .map(|profile| Citations::strings(origins, profile, |i| format!("outfit:{i}")))
        .unwrap_or_default();
    bind(
        txn,
        &Consumer::persona_outfit(),
        &outfits,
        Authority::Site,
        Unresolved::Reject,
    )
    .await
    .map(|_| ())
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

/// Publish and bind sticker images; `config.imageUrl` is stored as each
/// asset's permanent path.
pub async fn bind_and_publish_dashboard_layout(
    txn: &impl ConnectionTrait,
    layout_json: &str,
    origins: &[String],
) -> Result<String, MediaError> {
    bind_and_publish_dashboard_layout_except(txn, layout_json, origins, &[]).await
}

/// [`bind_and_publish_dashboard_layout`], leaving the `unresolved` local paths
/// unpublished, unbound and unchanged in the layout. Only the media upgrade
/// and settings restore pass any: stored stickers whose media can never exist
/// on this instance. Writers pass none, so saving such a sticker still fails.
pub(crate) async fn bind_and_publish_dashboard_layout_except(
    txn: &impl ConnectionTrait,
    layout_json: &str,
    origins: &[String],
    unresolved: &[String],
) -> Result<String, MediaError> {
    let parsed: Option<Value> = serde_json::from_str(layout_json).ok();
    let urls = parsed
        .as_ref()
        .map(extract_sticker_image_urls)
        .unwrap_or_default();
    let mut citations = Citations::urls(origins, &urls, |i| format!("sticker:{i}"));
    citations.retain(|citation| !unresolved.contains(&citation.path));
    let bound = bind(
        txn,
        &Consumer::stickers(),
        &citations,
        Authority::Site,
        Unresolved::Reject,
    )
    .await?;
    let Some(mut layout) = parsed else {
        return Ok(layout_json.to_string());
    };
    rewrite_sticker_image_urls(&mut layout, |raw| {
        cite_local_path(raw, origins)
            .and_then(|path| permanent_path(&bound, &path))
            .unwrap_or_else(|| raw.to_string())
    });
    Ok(layout.to_string())
}

/// Permanent path of the asset a cited local path resolved to in `bound`.
fn permanent_path(bound: &Bound, path: &str) -> Option<String> {
    bound.permanent_for(path).map(str::to_string)
}

pub async fn bind_ai_task(
    txn: &impl ConnectionTrait,
    task_id: &str,
    result: &Value,
    origins: &[String],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), MediaError> {
    bind(
        txn,
        &Consumer::ai_task(task_id, expires_at),
        &Citations::strings(origins, result, |_| "result".into()),
        Authority::Site,
        Unresolved::Reject,
    )
    .await
    .map(|_| ())
}

/// Media handed to one Agent run (web or channel). `payload` is client input:
/// any string in it may name an asset. Only assets the sender may manage are
/// bound, so a message cannot pin someone else's media against deletion; a
/// sender without an actor (guest) binds nothing. The reference expires after
/// the run; stored messages that show the media protect it from then on.
pub async fn bind_run_input(
    txn: &impl ConnectionTrait,
    consumer_id: &str,
    payload: &Value,
    origins: &[String],
    actor: Option<&MediaActor>,
) -> Result<(), MediaError> {
    let authority = actor.map_or(Authority::Anonymous, Authority::Actor);
    bind(
        txn,
        &Consumer::run_input(consumer_id),
        &Citations::strings(origins, payload, |i| format!("inbound:{i}")),
        authority,
        Unresolved::Reject,
    )
    .await
    .map(|_| ())
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
///
/// Snapshots hold the content *before* each save, so a dead image the author
/// just removed would otherwise fail every later save with MEDIA_NOT_READY.
/// Past versions cannot be edited: protect what is live and skip the rest.
/// Unmigrated assets stay protected by `references_complete`.
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
        bind_note_snapshot(txn, doc_id, revision, &snapshot, origins).await?;
    }
    Ok(())
}

pub(crate) async fn bind_note_snapshot(
    txn: &impl ConnectionTrait,
    doc_id: i32,
    revision: i64,
    snapshot: &Value,
    origins: &[String],
) -> Result<(), MediaError> {
    let md = snapshot
        .get("content_md")
        .and_then(Value::as_str)
        .unwrap_or("");
    let image = snapshot.get("image").and_then(Value::as_str);
    bind(
        txn,
        &Consumer::note_history(doc_id, revision),
        &Citations::fields(origins, image, md),
        Authority::Site,
        Unresolved::Skip,
    )
    .await
    .map(|_| ())
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

/// Rewrite older spellings of cited local media (legacy aliases,
/// `/api/media/{id}/content`) to each asset's permanent address. Read-only:
/// publication happens when the content is bound to its consumer.
pub async fn normalize_cited_media(
    txn: &impl ConnectionTrait,
    origins: &[String],
    cover: Option<&str>,
    body: &str,
) -> Result<(Option<String>, String), MediaError> {
    let citations = Citations::fields(origins, cover, body);
    let mut resolved = Bound::default();
    for citation in citations.iter() {
        if let Some(id) = resolve_asset_id(txn, &citation.path).await? {
            if let Some(row) = assets::find_by_id(txn, id).await? {
                if let Ok(asset) = assets::to_domain(row, 0) {
                    resolved.insert(citation.path.clone(), asset.id, asset.url);
                }
            }
        }
    }
    let body = resolved.rewrite(body);
    let cover = cover
        .map(|cover| resolved.rewrite(cover))
        .filter(|value| !value.trim().is_empty());
    Ok((cover, body))
}

/// [`normalize_cited_media`] for one URL; anything that is not local media
/// comes back unchanged.
pub async fn normalize_local_url(
    txn: &impl ConnectionTrait,
    url: &str,
    origins: &[String],
) -> Result<String, MediaError> {
    let (cover, _) = normalize_cited_media(txn, origins, Some(url), "").await?;
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

/// Saving a site image setting is publication; bind it in the same
/// transaction as config. Returns the value to store: local media as its
/// path-only permanent URL (which drops any stale origin), anything else
/// unchanged.
pub async fn bind_and_publish_wallpaper(
    txn: &impl ConnectionTrait,
    url: &str,
    origins: &[String],
) -> Result<String, MediaError> {
    bind_and_publish_site_image(txn, "ui_wallpaper_url", url, origins).await
}

pub async fn bind_and_publish_site_image(
    txn: &impl ConnectionTrait,
    key: &str,
    url: &str,
    origins: &[String],
) -> Result<String, MediaError> {
    let consumer =
        Consumer::site_image(key).ok_or_else(|| MediaError::invalid("Not a site image setting"))?;
    let local = site_media_local_path(txn, url, origins).await?;
    let mut citations = Citations::new();
    if let Some(path) = local.clone() {
        citations.push_path("image", path);
    }
    let bound = bind(
        txn,
        &consumer,
        &citations,
        Authority::Site,
        Unresolved::Reject,
    )
    .await?;
    Ok(local
        .and_then(|path| permanent_path(&bound, &path))
        .unwrap_or_else(|| url.to_string()))
}

/// Restore a site image, retaining missing local URLs without binding them.
/// Returns the stored value and any dead URLs.
pub(crate) async fn bind_restored_site_image(
    txn: &impl ConnectionTrait,
    key: &str,
    url: &str,
    origins: &[String],
) -> Result<(String, Vec<String>), MediaError> {
    let consumer =
        Consumer::site_image(key).ok_or_else(|| MediaError::invalid("Not a site image setting"))?;
    if let Some(path) = cite_local_path(url, origins) {
        if is_dead_local_path(txn, &path).await? {
            bind(
                txn,
                &consumer,
                &Citations::new(),
                Authority::Site,
                Unresolved::Skip,
            )
            .await?;
            return Ok((url.to_string(), vec![url.to_string()]));
        }
    }
    Ok((
        bind_and_publish_site_image(txn, key, url, origins).await?,
        Vec::new(),
    ))
}

/// Settings restore of the dashboard layout; dead sticker media is handled as
/// in [`bind_restored_site_image`] while live stickers are published and bound.
pub(crate) async fn bind_restored_dashboard_layout(
    txn: &impl ConnectionTrait,
    layout_json: &str,
    origins: &[String],
) -> Result<(String, Vec<String>), MediaError> {
    let layout: Value = serde_json::from_str(layout_json).unwrap_or(Value::Null);
    let mut dead_urls = Vec::new();
    let mut dead_paths = Vec::new();
    for url in extract_sticker_image_urls(&layout) {
        let Some(path) = cite_local_path(&url, origins) else {
            continue;
        };
        if is_dead_local_path(txn, &path).await? {
            dead_urls.push(url);
            push_unique(&mut dead_paths, path);
        }
    }
    let stored =
        bind_and_publish_dashboard_layout_except(txn, layout_json, origins, &dead_paths).await?;
    Ok((stored, dead_urls))
}

/// A local path that definitely has no media on this instance: its asset was
/// deleted, or nothing is catalogued for it and no cached file exists to
/// import. A catalogued asset that is not ready (its file is missing) is not
/// dead: its bytes may come back. Database and filesystem errors propagate.
async fn is_dead_local_path(db: &impl ConnectionTrait, path: &str) -> Result<bool, MediaError> {
    if let Some(id) = resolve_asset_id(db, path).await? {
        let row = assets::find_by_id(db, id).await?;
        return Ok(row.is_none_or(|row| row.state.as_deref() == Some("deleted")));
    }
    let cache = crate::services::image_cache::ImageCacheService::new();
    match cache.local_path_for_public_url(path) {
        Some(disk) if path.starts_with("/api/phantasi/image-cache/") => {
            Ok(!tokio::fs::try_exists(&disk).await?)
        }
        // Other platform media paths are only ever created by a catalogued upload.
        _ => Ok(true),
    }
}

/// Mark assets public. Only [`super::binding::bind`] calls this, after it has
/// authorized every id for the consumer being bound.
pub(super) async fn publish_asset_ids(
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
