//! Looking at the pictures sent in a group.

use super::*;

/// Pictures she looks at when she reads a group's talk, at most at once.
pub(super) const PICTURES_AT_ONCE: usize = 6;
/// Pictures a group's counts are kept for, at most.
pub(super) const PICTURES_KEPT: usize = 512;
pub(super) const PICTURE_BYTES: usize = 5 * 1024 * 1024;
pub(super) const PICTURE_TIMEOUT: Duration = Duration::from_secs(20);

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

/// Look at the pictures in the group's recent talk she has not seen yet,
/// as a person reads back over what was sent. Billed to the site's owner,
/// who hosts her there.
pub(super) async fn see(venue: &str, token: &str) {
    let unseen: Vec<(Option<String>, usize, GroupImage)> = with_group(venue, |group| {
        let lines = group.lines.len();
        group
            .lines
            .iter()
            .skip(lines.saturating_sub(CONVERSATION_LINES))
            .filter(|line| !line.hers)
            .flat_map(|line| {
                line.images
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| line.seen.get(*index).is_none_or(Option::is_none))
                    .map(|(index, image)| (line.message_id.clone(), index, image.clone()))
                    .collect::<Vec<_>>()
            })
            .take(PICTURES_AT_ONCE)
            .collect()
    })
    .unwrap_or_default();
    if unseen.is_empty() {
        return;
    }
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    let Ok(owner) = crate::services::site_owner::site_owner_user_id(&db).await else {
        return;
    };
    let db = &db;
    let looked = futures::future::join_all(unseen.into_iter().map(
        |(message_id, index, image)| async move {
            let seen = match crate::services::agent::merope::group::seeing::known(db, &image.key).await {
                Some(seen) => Some(seen),
                None => match fetch_picture(token, &image).await {
                    Some(bytes) => {
                        crate::services::agent::merope::group::seeing::look(
                            db,
                            owner,
                            &image.key,
                            bytes,
                            image.hint.as_deref(),
                        )
                        .await
                    }
                    None => None,
                },
            };
            (message_id, index, seen)
        },
    ))
    .await;
    let mut again = Vec::new();
    with_group(venue, |group| {
        for (message_id, index, seen) in looked {
            let Some(seen) = seen else {
                continue;
            };
            let key = group
                .lines
                .iter()
                .find(|line| !line.hers && line.message_id == message_id)
                .and_then(|line| line.images.get(index))
                .map(|image| image.key.clone());
            if let Some(key) = key
                && group.pictures.get(&key).is_some_and(|sent| *sent >= 2)
            {
                again.push((key, seen.clone()));
            }
            if let Some(line) = group
                .lines
                .iter_mut()
                .find(|line| !line.hers && line.message_id == message_id)
            {
                if line.seen.len() <= index {
                    line.seen.resize(index + 1, None);
                }
                line.seen[index] = Some(seen);
            }
        }
    });
    for (key, seen) in again {
        crate::services::agent::merope::group::bits::picture_again(db, owner, venue, &key, &seen).await;
    }
}
