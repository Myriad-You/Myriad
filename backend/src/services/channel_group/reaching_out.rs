//! Saying something first in a group she was seen in lately.

use super::*;

/// How long a group counts as one she is in, since she last saw a line
/// there.
pub(super) const SEEN_WITHIN: Duration = Duration::from_secs(3 * 24 * 3600);
/// Lines of each group she looks over when deciding.
pub(super) const SHARE_LINES: usize = 12;

/// Something of her own she would want to tell someone (`what`, as she took
/// it in): she looks over the groups she is in and, as herself, may bring
/// it up in one of them (see `merope::sharing`). Asleep she does not; a
/// group she is already talking in hears it in the talk. Billed to `owner`.
pub async fn share_first(owner: i32, what: String) {
    use crate::services::agent::merope::group::{sharing, timing};
    if timing::asleep_now().is_some() {
        return;
    }
    let Ok(db) = crate::services::process_db::database() else {
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
    speak_first(
        &db,
        &venue,
        &reason,
        "[Group] she brought something of hers up",
    )
    .await;
}

/// Something from a group she meant to come back to there (see
/// `merope::threads`), now about due: she decides, as herself, whether to
/// come back to it there now. Not while she is asleep, nor in a group she
/// is already talking in (it is in her mind as she talks there). Billed to
/// `owner`. Whether she said it.
pub async fn come_back(owner: i32, venue: &str, what: &str) -> bool {
    use crate::services::agent::merope::group::{sharing, timing};
    if timing::asleep_now().is_some() {
        return false;
    }
    let Ok(db) = crate::services::process_db::database() else {
        return false;
    };
    let Some(group) = offered(&db, Some(venue)).await.into_iter().next() else {
        return false;
    };
    let Some(why) = sharing::come_back(owner, what, &group).await else {
        return false;
    };
    let reason = myriad_merope::sharing::come_back_reason(what, &why);
    speak_first(
        &db,
        venue,
        &reason,
        "[Group] she came back to something there",
    )
    .await
}

/// The groups she is in, as she weighs saying something first: all of
/// them, or only `only`. Not one she is busy in, talking in, or muted in.
async fn offered(
    db: &DatabaseConnection,
    only: Option<&str>,
) -> Vec<crate::services::agent::merope::group::sharing::Offered> {
    use crate::services::agent::merope::group::{bits, sharing};
    let now = chrono::Utc::now();
    let seen: Vec<(String, Vec<String>, String, Vec<serde_json::Value>)> = {
        let Ok(groups) = GROUPS.lock() else {
            return Vec::new();
        };
        groups
            .iter()
            .filter(|(venue, _)| only.is_none_or(|only| only == venue.as_str()))
            .filter(|(_, group)| !group.busy && !in_talk(group) && !muted_now(group, now))
            .filter_map(|(venue, group)| {
                group.reach.as_ref()?;
                let last = group.lines.back();
                let quiet = last.map(|line| now - line.at);
                if quiet.is_some_and(|quiet| quiet.to_std().is_ok_and(|quiet| quiet > SEEN_WITHIN))
                {
                    return None;
                }
                let lines: Vec<String> = group
                    .lines
                    .iter()
                    .rev()
                    .take(SHARE_LINES)
                    .rev()
                    .map(|line| {
                        if line.hers {
                            format!("you：{}", line.text)
                        } else {
                            format!("{}：{}", line.name, line.said())
                        }
                    })
                    .collect();
                let quiet_for = match quiet {
                    Some(quiet) => myriad_merope::doing::ago_text(quiet),
                    None => "a good while (nothing said there lately)".to_string(),
                };
                let spoke_up = group
                    .spoke_up
                    .iter()
                    .map(|(at, taken)| {
                        let ago = chrono::Duration::from_std(at.elapsed()).unwrap_or_default();
                        serde_json::json!({
                            "ago": myriad_merope::doing::ago_text(ago),
                            "takenUp": taken,
                        })
                    })
                    .collect();
                Some((venue.clone(), lines, quiet_for, spoke_up))
            })
            .collect()
    };
    let mut offered = Vec::with_capacity(seen.len());
    for (venue, lines, quiet_for, spoke_up_lately) in seen {
        let shared = bits::in_group(db, &venue, 5)
            .await
            .into_iter()
            .map(|(handle, how)| format!("{handle}: {how}"))
            .collect();
        offered.push(sharing::Offered {
            id: venue,
            lines,
            quiet_for,
            bits: shared,
            spoke_up_lately,
        });
    }
    offered
}

/// Take the group's turn and say it, then answer whoever waited meanwhile.
async fn speak_first(db: &DatabaseConnection, venue: &str, reason: &str, said: &str) -> bool {
    if !matches!(begin_turn(venue), Turn::Began) {
        return false;
    }
    let spoke = share_turn(db, venue, reason).await;
    if spoke {
        info!(%venue, "{said}");
        spoke_up_now(venue);
    }
    let mut next = finish_turn(venue, spoke).await;
    while let Some(message) = next {
        let token = with_group(venue, |group| {
            group.reach.as_ref().map(|(_, token)| token.clone())
        })
        .flatten()
        .unwrap_or_default();
        let replied = answer(&message, &token, None).await;
        next = finish_turn(venue, replied).await;
    }
    spoke
}

/// Say it in the group, as a turn of her own on the site owner's budget: no
/// one's line behind it, so nothing quoted and nothing to answer.
pub(super) async fn share_turn(db: &DatabaseConnection, venue: &str, reason: &str) -> bool {
    let Some((seen, token)) = with_group(venue, |group| group.reach.clone()).flatten() else {
        return false;
    };
    let Ok(owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return false;
    };
    let inbound_id = format!(
        "group:{}:first:{}",
        seen.chat,
        chrono::Utc::now().timestamp_millis()
    );
    if !crate::services::channel_work::claim_inbound(db, seen.platform, None, &inbound_id).await {
        return false;
    }
    let line = GroupLine {
        message_id: String::new(),
        from: String::new(),
        display_name: String::new(),
        text: myriad_merope::sharing::NOBODY_SAID.to_string(),
        addressed: false,
        reply_to: None,
        images: Vec::new(),
        ..seen
    };
    let began = Instant::now();
    let Some((reply, sticker)) =
        run_turn(db, &line, owner, &token, Some(reason.to_string()), true).await
    else {
        return false;
    };
    let reply = without_reply_mark(&reply);
    say_and_send(&line, &token, &reply, sticker, began).await
}
