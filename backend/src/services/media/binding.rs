//! The one path by which business content cites media:
//! resolve → authorize → publish (public consumers) → bind.
//!
//! Every consumer states once, in its constructor, whether its content is
//! served publicly. Writers only supply citations and who is writing; they
//! cannot bind a public reference to a private asset, skip authorization for
//! request-supplied ids, or forget to publish what a public page will show.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sea_orm::ConnectionTrait;
use serde_json::Value;

use super::access::can_manage;
use super::assets;
use super::cite::{collect_strings, extract_registered_paths, publish_asset_ids, resolve_asset_id};
use super::error::MediaError;
use super::references::{NewReference, replace_for_consumer};
use super::types::{MediaActor, MediaExposure, MediaState};
use super::urls::{cite_local_path, content_path};

/// Where cited media ends up being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// Served to anonymous visitors, crawlers or remote servers: cited assets
    /// are published and the references require them to stay public.
    Public,
    /// Read only by people who can already read the assets.
    Private,
}

/// One business record that cites media. Construct through the named
/// constructors so the kind and its visibility always agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Consumer {
    kind: &'static str,
    id: String,
    visibility: Visibility,
    expires_at: Option<DateTime<Utc>>,
}

impl Consumer {
    fn new(kind: &'static str, id: impl Into<String>, visibility: Visibility) -> Self {
        Self {
            kind,
            id: id.into(),
            visibility,
            expires_at: None,
        }
    }

    pub fn note_draft(doc_id: i32) -> Self {
        Self::new("note_draft", doc_id.to_string(), Visibility::Private)
    }

    pub fn note_history(doc_id: i32, revision: i64) -> Self {
        Self::new(
            "note_history",
            format!("{doc_id}:{revision}"),
            Visibility::Private,
        )
    }

    pub fn note_published(item_id: i32) -> Self {
        Self::new("note_published", item_id.to_string(), Visibility::Public)
    }

    pub fn rss_item(item_id: i32) -> Self {
        Self::new("rss_item", item_id.to_string(), Visibility::Public)
    }

    pub fn persona_portrait() -> Self {
        Self::new("persona_portrait", "persona", Visibility::Public)
    }

    pub fn persona_outfit() -> Self {
        Self::new("persona_outfit", "persona", Visibility::Public)
    }

    pub fn stickers() -> Self {
        Self::new("sticker", "dashboard", Visibility::Public)
    }

    /// Publicly rendered site image settings; `None` for any other key.
    pub fn site_image(key: &str) -> Option<Self> {
        match key {
            "ui_wallpaper_url" => Some(Self::new("site_wallpaper", "site", Visibility::Public)),
            "site_og_image" | "site_favicon" => {
                Some(Self::new("site_setting", key, Visibility::Public))
            }
            _ => None,
        }
    }

    /// Results live only as long as the task record they belong to.
    pub fn ai_task(task_id: &str, expires_at: Option<DateTime<Utc>>) -> Self {
        Self {
            expires_at,
            ..Self::new("ai_task", task_id, Visibility::Private)
        }
    }

    pub fn channel_message(id: impl Into<String>) -> Self {
        Self::new("channel_message", id, Visibility::Private)
    }

    /// A stored Agent conversation message; same identity the upgrade uses.
    pub fn agent_message(message_id: i32) -> Self {
        Self::channel_message(format!("agent_messages:{message_id}"))
    }

    /// Media handed to one Agent run, protected while the run can still read
    /// it. Afterwards only stored messages that show the media keep it
    /// referenced (attachments are not part of the stored message), so this
    /// expires instead of pinning uploads forever.
    pub fn run_input(run_id: &str) -> Self {
        Self {
            expires_at: Some(Utc::now() + chrono::Duration::hours(24)),
            ..Self::channel_message(run_id)
        }
    }

    /// One `tapp_storage` row; the row id is stable across overwrites of a key.
    pub fn tapp_storage(row_id: i32) -> Self {
        Self::new("tapp_storage", row_id.to_string(), Visibility::Private)
    }

    pub fn federation_activity(activity_id: impl Into<String>) -> Self {
        Self::new("federation_activity", activity_id, Visibility::Public)
    }

    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }
}

/// Who wrote the citing content.
#[derive(Clone, Copy, Debug)]
pub enum Authority<'a> {
    /// Server-produced content or an admin-only writer: every cited asset is
    /// the site's to publish.
    Site,
    /// Content carrying request-supplied URLs. A public consumer rejects
    /// private assets the actor cannot manage (as missing, so ids cannot be
    /// probed); a private consumer binds only assets the actor manages, so it
    /// can never pin someone else's media against deletion.
    Actor(&'a MediaActor),
    /// Request-supplied content from someone without an identity (guest):
    /// nothing is bound.
    Anonymous,
    /// Content nobody here authored (a subscribed feed). It may name any local
    /// URL, including someone's private draft by sequential id, so it never
    /// publishes, never imports, and binds only assets that are already public.
    External,
}

/// What to do with a recognized local URL that is not a ready asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unresolved {
    /// Refuse the write (`MEDIA_NOT_READY`): content must not silently cite
    /// media that escapes deletion protection.
    Reject,
    /// Leave it unbound. Only for content that cannot be edited any more
    /// (history snapshots) or media known to be gone (restores).
    Skip,
}

/// A local media path cited in one slot of the consumer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Citation {
    pub slot: String,
    pub path: String,
}

/// Citations extracted from business content. Only local media paths are
/// kept; remote URLs never become citations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Citations(Vec<Citation>);

impl Citations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, slot: impl Into<String>, url: &str, origins: &[String]) {
        if let Some(path) = cite_local_path(url, origins) {
            self.push_path(slot, path);
        }
    }

    /// A path already known to be local (e.g. an older site origin resolved
    /// by the caller).
    pub fn push_path(&mut self, slot: impl Into<String>, path: String) {
        let slot = slot.into();
        if !self
            .0
            .iter()
            .any(|item| item.slot == slot && item.path == path)
        {
            self.0.push(Citation { slot, path });
        }
    }

    /// A cover field plus the media a markdown body embeds.
    pub fn fields(origins: &[String], cover: Option<&str>, body: &str) -> Self {
        let mut citations = Self::new();
        if let Some(cover) = cover {
            citations.push("cover", cover, origins);
        }
        for (index, path) in extract_registered_paths(body, origins)
            .into_iter()
            .enumerate()
        {
            citations.push_path(format!("body:{index}"), path);
        }
        citations
    }

    pub fn urls<S: AsRef<str>>(
        origins: &[String],
        urls: &[S],
        slot: impl Fn(usize) -> String,
    ) -> Self {
        let mut citations = Self::new();
        for (index, url) in urls.iter().enumerate() {
            citations.push(slot(index), url.as_ref(), origins);
        }
        citations
    }

    /// Every string anywhere in a JSON value (results, payloads, profiles).
    pub fn strings(origins: &[String], value: &Value, slot: impl Fn(usize) -> String) -> Self {
        let mut strings = Vec::new();
        collect_strings(value, &mut strings);
        Self::urls(origins, &strings, slot)
    }

    pub fn retain(&mut self, keep: impl Fn(&Citation) -> bool) {
        self.0.retain(keep);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Citation> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// What a bind resolved, for normalizing stored text to permanent addresses.
#[derive(Clone, Debug, Default)]
pub struct Bound {
    /// Cited path → asset id, for every citation that was bound.
    resolved: Vec<(String, i32)>,
    /// Asset id → permanent address.
    urls: HashMap<i32, String>,
}

impl Bound {
    pub(super) fn insert(&mut self, path: String, asset_id: i32, url: String) {
        self.resolved.push((path, asset_id));
        self.urls.insert(asset_id, url);
    }

    /// Permanent address of the asset a cited local path resolved to.
    pub fn permanent_for(&self, path: &str) -> Option<&str> {
        let (_, id) = self.resolved.iter().find(|(from, _)| from == path)?;
        self.urls.get(id).map(String::as_str)
    }

    /// Permanent address of each bound asset.
    pub fn urls(&self) -> &HashMap<i32, String> {
        &self.urls
    }

    /// Replace legacy aliases and `/api/media/{id}/content` with each bound
    /// asset's permanent address. Addresses are stable, so this only
    /// normalizes older spellings; it never changes which asset is cited.
    pub fn rewrite(&self, raw: &str) -> String {
        let mut out = raw.to_string();
        for (from, id) in &self.resolved {
            if let Some(to) = self.urls.get(id) {
                if from != to {
                    out = out.replace(from, to);
                }
            }
        }
        for (id, to) in &self.urls {
            out = out.replace(&content_path(*id), to);
        }
        out
    }
}

/// Bind `consumer` to exactly `citations`, replacing what it cited before.
pub async fn bind(
    txn: &impl ConnectionTrait,
    consumer: &Consumer,
    citations: &Citations,
    authority: Authority<'_>,
    unresolved: Unresolved,
) -> Result<Bound, MediaError> {
    let mut bound = Bound::default();
    let mut refs: Vec<NewReference> = Vec::new();
    let mut to_publish = Vec::new();
    for citation in citations.iter() {
        if matches!(authority, Authority::Anonymous) {
            break;
        }
        let mut asset_id = resolve_asset_id(txn, &citation.path).await?;
        if asset_id.is_none()
            && unresolved == Unresolved::Reject
            && matches!(authority, Authority::Site)
        {
            // Site-authored content citing a cached image (an RSS picture, a
            // proxied download) makes it durable instead of failing the save.
            // Only the site imports: request-supplied content must not turn
            // evictable cache into permanent storage. Evictable caches
            // themselves (RSS items) bind with Skip and are never imported.
            asset_id = import_cached(txn, &citation.path).await?;
        }
        let Some(asset_id) = asset_id else {
            if unresolved == Unresolved::Reject {
                return Err(MediaError::NotReady);
            }
            continue;
        };
        let Some(row) = assets::find_by_id(txn, asset_id).await? else {
            if unresolved == Unresolved::Reject {
                return Err(MediaError::NotReady);
            }
            continue;
        };
        if MediaState::parse(row.state.as_deref().unwrap_or("")).ok() != Some(MediaState::Ready) {
            if unresolved == Unresolved::Reject {
                return Err(MediaError::NotReady);
            }
            continue;
        }
        let asset = assets::to_domain(row, 0)?;
        if matches!(authority, Authority::External) && asset.exposure != MediaExposure::Public {
            continue;
        }
        if let Authority::Actor(actor) = authority {
            if !can_manage(actor, &asset) {
                match consumer.visibility {
                    Visibility::Public if asset.exposure != MediaExposure::Public => {
                        return Err(MediaError::Missing);
                    }
                    Visibility::Public => {}
                    Visibility::Private => continue,
                }
            }
        }
        if consumer.visibility == Visibility::Public && asset.exposure != MediaExposure::Public {
            to_publish.push(asset.id);
        }
        bound.insert(citation.path.clone(), asset.id, asset.url.clone());
        if !refs
            .iter()
            .any(|item| item.asset_id == asset.id && item.slot == citation.slot)
        {
            refs.push(NewReference {
                asset_id: asset.id,
                slot: citation.slot.clone(),
                requires_public: consumer.visibility == Visibility::Public,
                expires_at: consumer.expires_at,
            });
        }
    }
    to_publish.sort_unstable();
    to_publish.dedup();
    publish_asset_ids(txn, &to_publish).await?;
    replace_for_consumer(txn, consumer.kind, &consumer.id, &refs).await?;
    Ok(bound)
}

async fn import_cached(txn: &impl ConnectionTrait, path: &str) -> Result<Option<i32>, MediaError> {
    let data = crate::services::data_paths::paths();
    super::cache_import::import_cached_citation(
        txn,
        &super::MediaStore::new(data.media.clone()),
        &crate::services::image_cache::ImageCacheService::new(),
        path,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_follows_the_consumer_not_the_caller() {
        assert_eq!(Consumer::note_draft(1).visibility(), Visibility::Private);
        assert_eq!(Consumer::note_published(1).visibility(), Visibility::Public);
        assert_eq!(Consumer::note_history(1, 2).id(), "1:2");
        assert_eq!(
            Consumer::site_image("site_og_image").map(|c| (c.kind(), c.visibility())),
            Some(("site_setting", Visibility::Public))
        );
        assert!(Consumer::site_image("ui_title").is_none());
        assert_eq!(
            Consumer::channel_message("r").visibility(),
            Visibility::Private
        );
    }

    #[test]
    fn citations_keep_only_local_paths_once_per_slot() {
        let origins = vec!["https://site.example".to_string()];
        let mut citations = Citations::fields(
            &origins,
            Some("https://site.example/api/media/3/content"),
            "![a](/api/media/4/content) ![b](https://cdn.example/x.png) ![c](/api/media/4/content)",
        );
        citations.push(
            "cover",
            "https://site.example/api/media/3/content",
            &origins,
        );
        let paths: Vec<_> = citations
            .iter()
            .map(|c| (c.slot.as_str(), c.path.as_str()))
            .collect();
        assert_eq!(
            paths,
            vec![
                ("cover", "/api/media/3/content"),
                ("body:0", "/api/media/4/content")
            ]
        );
    }

    #[test]
    fn rewrite_normalizes_older_spellings_to_permanent_addresses() {
        let url = "/media/assets/11111111-1111-1111-1111-111111111111/a.png".to_string();
        let bound = Bound {
            resolved: vec![("/api/phantasi/image-cache/ab/old.png".to_string(), 7)],
            urls: HashMap::from([(7, url.clone())]),
        };
        assert_eq!(
            bound.rewrite("![](/api/media/7/content) ![](/api/phantasi/image-cache/ab/old.png)"),
            format!("![]({url}) ![]({url})")
        );
    }
}
