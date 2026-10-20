//! Saying something first in a group she is in: offered as herself, said
//! as a turn of her own (see `myriad_merope::group`).

use myriad_merope::group::{self as core, Event, Message};

use super::*;

/// Something of her own she would want to tell someone: she looks over the
/// groups she is in and, as herself, may bring it up in one of them.
pub async fn share_first(owner: i32, what: String) {
    use crate::services::agent::merope::group::{sharing, timing};
    if timing::asleep_now().is_some() {
        return;
    }
    let Ok(db) = db() else {
        return;
    };
    let offered = offered(&db, None).await;
    if offered.is_empty() {
        return;
    }
    let Some((venue, why)) = sharing::choose(owner, &what, &offered).await else {
        return;
    };
    let reason = myriad_merope::sharing::reason(&what, &why);
    if speak_first(&db, &venue, reason).await {
        info!(%venue, "[Group] she brought something of hers up");
    }
}

/// Something from a group she meant to come back to there, now about due:
/// whether she comes back to it there now. Whether she said it.
pub async fn come_back(owner: i32, venue: &str, what: &str) -> bool {
    use crate::services::agent::merope::group::{sharing, timing};
    if timing::asleep_now().is_some() {
        return false;
    }
    let Ok(db) = db() else {
        return false;
    };
    let Some(group) = offered(&db, Some(venue)).await.into_iter().next() else {
        return false;
    };
    let Some(why) = sharing::come_back(owner, what, &group).await else {
        return false;
    };
    let reason = myriad_merope::sharing::come_back_reason(what, &why);
    let spoke = speak_first(&db, venue, reason).await;
    if spoke {
        info!(%venue, "[Group] she came back to something there");
    }
    spoke
}

/// The groups she is in, as she weighs saying something first: all of
/// them, or only `only`. Not one she is busy in, talking in, or muted in.
pub(super) async fn offered(
    db: &DatabaseConnection,
    only: Option<&str>,
) -> Vec<crate::services::agent::merope::group::sharing::Offered> {
    use crate::services::agent::merope::group::{bits, sharing};
    bring_back(db).await;
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let now = now().timestamp_micros();
    let seen: Vec<(String, core::Offer)> = {
        let Ok(groups) = GROUPS.lock() else {
            return Vec::new();
        };
        let mut venues: Vec<&String> = groups.keys().collect();
        venues.sort();
        venues
            .into_iter()
            .filter(|venue| only.is_none_or(|only| only == venue.as_str()))
            .filter_map(|venue| {
                let group = &groups[venue].group;
                let reach = group.reach.as_ref()?;
                token_for(platform_of(&reach.platform)?, &reach.chat, &config)?;
                Some((venue.clone(), core::offer(group, now)?))
            })
            .collect()
    };
    let mut offered = Vec::with_capacity(seen.len());
    for (venue, offer) in seen {
        // (A recording has no database to read them from.)
        #[cfg(test)]
        let look_up = !g0::on();
        #[cfg(not(test))]
        let look_up = true;
        let shared = if look_up {
            bits::in_group(db, &venue, 5, None)
                .await
                .into_iter()
                .map(|(handle, how)| format!("{handle}: {how}"))
                .collect()
        } else {
            Vec::new()
        };
        offered.push(sharing::Offered {
            id: venue,
            lines: offer.lines,
            quiet_for: offer.quiet_for,
            bits: shared,
            spoke_up_lately: offer.spoke_up_lately,
        });
    }
    offered
}

/// The groups she is in that are not in mind since a restart, brought back
/// from what was kept: their lines, if kept, and where she would say
/// something. The token is read from the configuration.
pub(super) async fn bring_back(db: &DatabaseConnection) {
    let kept = all_kept(db, REACH_NAMESPACE).await;
    if kept.is_empty() {
        return;
    }
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    for row in kept {
        let venue = row.record_id;
        let in_mind = GROUPS.lock().ok().is_some_and(|groups| {
            groups
                .get(&venue)
                .is_some_and(|slot| slot.group.reach.is_some())
        });
        if in_mind {
            continue;
        }
        let Ok(stored) = serde_json::from_value::<StoredReach>(row.payload) else {
            continue;
        };
        let Some((platform, chat)) = venue
            .split_once(':')
            .and_then(|(slug, chat)| Some((platform_of(slug)?, chat.to_string())))
        else {
            continue;
        };
        let Some(token) = token_for(platform, &chat, &config) else {
            continue;
        };
        restore(&venue).await;
        note_token(&venue, &token);
        let seen = Message {
            platform: platform.slug().to_string(),
            chat,
            message_id: String::new(),
            thread: stored.thread,
            from: String::new(),
            display_name: String::new(),
            text: String::new(),
            addressed: false,
            reply_to: None,
            images: Vec::new(),
            shown_as: None,
        };
        step(
            &venue,
            Event::Reached {
                message: seen,
                seen_at: stored.seen_at.timestamp_micros(),
            },
        );
        info!(%venue, "[Group] a group she is in, brought back after a restart");
    }
}

/// Take the group's turn and say it, then answer whoever waited meanwhile.
pub(super) async fn speak_first(db: &DatabaseConnection, venue: &str, reason: String) -> bool {
    let owner = owner(db).await;
    let waiter = format!(
        "{venue}#first{}",
        WAITER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let (sender, done) = tokio::sync::oneshot::channel();
    if let Ok(mut waiters) = WAITERS.lock() {
        waiters.insert(waiter.clone(), sender);
    }
    let out = step(
        venue,
        Event::SpeakFirst {
            reason,
            owner,
            waiter,
        },
    );
    execute(venue.to_string(), out.effects).await;
    done.await.unwrap_or(false)
}

/// What sends into a group with, while it still may: the bot is on, the
/// group is let in (OneBot's allowlist), and it has a token (OneBot needs
/// none). None for a platform she does not send to groups on.
pub(super) fn token_for(
    platform: ChannelPlatform,
    chat: &str,
    config: &crate::config::DynamicConfig,
) -> Option<String> {
    #[cfg(test)]
    if g0::on() {
        return g0::token_for(platform, chat);
    }
    if !platform.enabled(config) {
        return None;
    }
    match platform {
        ChannelPlatform::OneBot => myriad_agent_rules::onebot::rules::onebot_group_allowed(
            &config.onebot_bot_group_ids,
            chat,
        )
        .then(String::new),
        ChannelPlatform::Telegram | ChannelPlatform::Discord => platform
            .credentials(config)
            .1
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_string),
        ChannelPlatform::Qq | ChannelPlatform::Feishu => None,
    }
}
