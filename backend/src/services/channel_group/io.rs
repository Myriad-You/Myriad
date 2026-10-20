//! Where group talk reaches past her: the clock, chance, whether she is on
//! the site, the database and the runtime registry, who someone is, and her
//! other parts it hands things to. Every call goes through here, so a test
//! can stand in for all of it at once (see `g0`); nothing here decides
//! anything.

use super::*;
use crate::services::runtime_registry::{RegistryIdentity, RegistryRow};
use sea_orm::DbErr;

/// Now.
pub(super) fn now() -> chrono::DateTime<chrono::Utc> {
    #[cfg(test)]
    if let Some(at) = g0::now() {
        return at;
    }
    chrono::Utc::now()
}

/// She is not on the site now (D3).
pub(super) fn away() -> bool {
    #[cfg(test)]
    if let Some(away) = g0::away() {
        return away;
    }
    crate::services::channel_work::held::away()
}

pub(super) fn db() -> Result<DatabaseConnection, DbErr> {
    #[cfg(test)]
    if g0::on() {
        return Ok(g0::database().unwrap_or_default());
    }
    crate::services::process_db::database()
}

/// The site's owner, who hosts her in groups.
pub(super) async fn owner(db: &DatabaseConnection) -> Option<i32> {
    #[cfg(test)]
    if g0::on() {
        return g0::owner();
    }
    crate::services::site_owner::site_owner_user_id(db)
        .await
        .ok()
}

/// Claim an inbound id once.
pub(super) async fn claim(db: &DatabaseConnection, platform: ChannelPlatform, id: &str) -> bool {
    #[cfg(test)]
    if g0::on() {
        return g0::claim(id);
    }
    crate::services::channel_work::claim_inbound(db, platform, None, id).await
}

/// Who wrote `line`, as pairing knows them.
pub(super) async fn lookup(db: &DatabaseConnection, line: &GroupLine) -> Option<PairingLookup> {
    #[cfg(test)]
    if g0::on() {
        return g0::lookup(&line.from);
    }
    crate::services::channel_pairing::lookup_openid(
        db,
        PairingChannel::of(line.platform),
        &line.from,
    )
    .await
    .ok()
}

/// The binding `user_id` speaks to her through here, while it holds.
pub(super) async fn current_binding(
    db: &DatabaseConnection,
    message: &GroupLine,
    user_id: i32,
) -> Option<ChannelBinding> {
    #[cfg(test)]
    if g0::on() {
        return g0::binding(user_id);
    }
    let binding = ChannelBinding::resolve(db, message.platform, user_id, &message.from)
        .await
        .ok()
        .flatten()?;
    binding.is_current(db).await.then_some(binding)
}

/// Whether `binding` still holds.
pub(super) async fn still_bound(db: &DatabaseConnection, binding: &ChannelBinding) -> bool {
    #[cfg(test)]
    if g0::on() {
        return g0::still_bound(binding.user_id);
    }
    binding.is_current(db).await
}

fn no_identity() -> RegistryIdentity<'static> {
    RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

pub(super) async fn kept<T: serde::de::DeserializeOwned>(
    db: &DatabaseConnection,
    namespace: &str,
    key: &str,
) -> Result<Option<T>, DbErr> {
    #[cfg(test)]
    if g0::on() {
        if g0::fails("read", namespace, key) {
            return Err(DbErr::Custom("the recording's database is down".into()));
        }
        return Ok(g0::kept(namespace, key).and_then(|value| serde_json::from_value(value).ok()));
    }
    crate::services::runtime_registry::get::<T>(db, namespace, key).await
}

pub(super) async fn keep<T: serde::Serialize>(
    db: &DatabaseConnection,
    namespace: &str,
    key: &str,
    value: &T,
    until: i64,
) -> Result<(), DbErr> {
    #[cfg(test)]
    if g0::on() {
        if g0::fails("write", namespace, key) {
            return Err(DbErr::Custom("the recording's database is down".into()));
        }
        g0::keep(
            namespace,
            key,
            serde_json::to_value(value).unwrap_or_default(),
            until,
        );
        return Ok(());
    }
    crate::services::runtime_registry::put(db, namespace, key, no_identity(), value, until).await
}

/// Keep nothing more under `key`.
pub(super) async fn forget_kept(db: &DatabaseConnection, namespace: &str, key: &str) {
    #[cfg(test)]
    if g0::on() {
        return g0::forget_kept(namespace, key);
    }
    let _ = crate::services::runtime_registry::take::<serde_json::Value>(db, namespace, key).await;
}

pub(super) async fn all_kept(db: &DatabaseConnection, namespace: &str) -> Vec<RegistryRow> {
    #[cfg(test)]
    if g0::on() {
        return g0::all_kept(namespace);
    }
    crate::services::runtime_registry::list(db, namespace, None, None)
        .await
        .unwrap_or_default()
}

/// Her speaking up unasked there, kept with whether anyone took it up.
pub(super) async fn spoke_up(db: &DatabaseConnection, venue: &str, line: &str, taken: bool) {
    #[cfg(test)]
    if g0::on() {
        return g0::note(
            "spokeUp",
            serde_json::json!({ "venue": venue, "line": line, "taken": taken }),
        );
    }
    crate::services::agent::merope::group::others::spoke_up(db, venue, line, taken).await;
}

pub(super) async fn spoke_up_lately(
    db: &DatabaseConnection,
    venue: &str,
    most: u64,
) -> Vec<(chrono::DateTime<chrono::Utc>, bool)> {
    #[cfg(test)]
    if g0::on() {
        return g0::spoke_up_lately(venue);
    }
    crate::services::agent::merope::group::others::spoke_up_lately(db, venue, most).await
}

/// A picture sent there again: the group's.
pub(super) async fn picture_again(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    key: &str,
    seen: &myriad_merope::seeing::Seen,
) {
    #[cfg(test)]
    if g0::on() {
        return g0::note(
            "pictureAgain",
            serde_json::json!({ "venue": venue, "key": key, "seen": seen }),
        );
    }
    crate::services::agent::merope::group::bits::picture_again(db, owner, venue, key, seen).await;
}

/// A line said there, for the words she watches there.
pub(super) fn jargon_heard(venue: &str, text: &str) {
    #[cfg(test)]
    if g0::on() {
        return g0::note(
            "jargon",
            serde_json::json!({ "venue": venue, "text": text }),
        );
    }
    crate::services::agent::merope::group::jargon::heard(venue, text);
}

/// A stretch of the group's talk, for what she heard in it and what the
/// group was like.
pub(super) async fn take_in_heard(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    stretch: Vec<(chrono::DateTime<chrono::FixedOffset>, String, String, bool)>,
) {
    #[cfg(test)]
    if g0::on() {
        return g0::note(
            "takeIn",
            serde_json::json!({
                "venue": venue,
                "owner": owner,
                "stretch": stretch.iter().map(|(at, name, text, hers)| serde_json::json!({
                    "at": at.to_rfc3339(), "name": name, "text": text, "hers": hers,
                })).collect::<Vec<_>>(),
            }),
        );
    }
    crate::services::agent::merope::group::bits::go_over_stretch(db, owner, venue, stretch).await;
}

/// Her speaking soul.
pub(super) async fn soul() -> String {
    crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default()
}

/// One picture looked at: what she saw, from what she saw of it before,
/// or by fetching and looking now (keeping a sticker she would send).
pub(super) async fn look_at(
    db: &DatabaseConnection,
    owner: i32,
    soul: &str,
    venue: &str,
    token: &str,
    image: &GroupImage,
) -> Option<myriad_merope::seeing::Seen> {
    #[cfg(test)]
    if g0::on() {
        return g0::look(image, !soul.trim().is_empty());
    }
    if let Some(seen) = crate::services::agent::merope::group::seeing::known(db, &image.key).await {
        return Some(seen);
    }
    let bytes = fetch_picture(token, image).await?;
    let sticker = (image.sticker && !soul.trim().is_empty()).then(|| bytes.clone());
    let seen = crate::services::agent::merope::group::seeing::look(
        db,
        owner,
        &image.key,
        bytes,
        image.hint.as_deref(),
        sticker.as_ref().map(|_| soul),
    )
    .await;
    if let (Some(seen), Some(bytes)) = (&seen, sticker) {
        crate::services::agent::merope::group::stickers::keep_theirs(db, owner, venue, seen, bytes)
            .await;
    }
    seen
}

const PICTURE_BYTES: usize = 5 * 1024 * 1024;
const PICTURE_TIMEOUT: Duration = Duration::from_secs(20);

/// A picture's bytes, from where the platform keeps it.
pub(super) async fn fetch_picture(token: &str, image: &GroupImage) -> Option<Vec<u8>> {
    match &image.fetch {
        ImageFetch::TelegramFile { file_id } => {
            crate::services::telegram_bot::download_file_bytes(token, file_id)
                .await
                .ok()
                .map(|(bytes, _)| bytes)
                .filter(|bytes| bytes.len() <= PICTURE_BYTES)
        }
        ImageFetch::Url { url } => {
            let fetched = crate::services::outbound_security::get_public_following_redirects(
                url,
                PICTURE_TIMEOUT,
                None,
            )
            .await
            .ok()?;
            if !fetched.response.status().is_success() {
                return None;
            }
            crate::services::outbound_security::read_limited_body(fetched.response, PICTURE_BYTES)
                .await
                .ok()
        }
    }
}
