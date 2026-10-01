//! Lines that do not call her by name: glancing when the talk pauses and deciding whether to speak up.

use super::*;

/// A line that did not call her by name. She reads a group the way a person
/// does. In talk she is in, she looks when it pauses (or, in talk that never
/// pauses, every so often). Otherwise she glances at the group now and then
/// when she is free, not at every line. Either way she judges, as herself,
/// whether to say something (see `merope::joining`). Nothing is held while
/// she waits.
pub fn notice(message: GroupLine, token: String) {
    let venue = message.venue();
    let id = message.message_id.clone();
    let wake = with_group(&venue, |group| {
        group.unjudged_since.get_or_insert_with(Instant::now);
        group.reach = Some((message.clone(), token.clone()));
        group.pending = Some(message);
        if in_talk(group) {
            Some(SETTLE)
        } else if group.glance_at.is_some() {
            None
        } else {
            let after = glance_after();
            group.glance_at = Some(Instant::now() + after);
            Some(after)
        }
    })
    .flatten();
    if let Some(after) = wake {
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            // A glance takes in whatever is latest by then.
            let id = with_group(&venue, |group| {
                if group.glance_at.is_some_and(|at| at <= Instant::now()) {
                    group.glance_at = None;
                    group.pending.as_ref().map(|line| line.message_id.clone())
                } else {
                    Some(id)
                }
            })
            .flatten();
            if let Some(id) = id {
                look(venue, id, token).await;
            }
        });
    }
}

/// Whether she is in the group's talk: she spoke there, or was called
/// there, lately.
pub(super) fn in_talk(group: &Group) -> bool {
    group.called.is_some_and(|at| at.elapsed() < IN_TALK)
        || group
            .lines
            .iter()
            .rev()
            .find(|line| line.hers)
            .is_some_and(|line| {
                (chrono::Utc::now() - line.at)
                    .to_std()
                    .is_ok_and(|age| age < IN_TALK)
            })
}

/// How long until she glances at a group she is not in: once what she is
/// doing on her own is done (or she is up, if asleep), and then a while, as
/// a person looks at their phone.
pub(super) fn glance_after() -> Duration {
    // Asleep, she looks once she is up.
    if let myriad_merope::timing::Where::Asleep { wakes_in } =
        crate::services::agent::merope::group::timing::where_she_is(false, true)
    {
        return Duration::from_secs_f64(wakes_in)
            + Duration::from_secs(rand::random_range(GLANCE_AFTER_SECONDS));
    }
    let free_in = crate::services::agent::merope::group::doing::current()
        .and_then(|doing| (doing.ends - chrono::Utc::now()).to_std().ok())
        .unwrap_or_default()
        .min(LONGEST_BUSY);
    free_in + Duration::from_secs(rand::random_range(GLANCE_AFTER_SECONDS))
}

pub(super) enum Look {
    /// A newer line will be looked at instead, or nothing is waiting.
    Done,
    /// She is talking or looking already: again in a moment.
    Later,
    Now(Box<GroupLine>),
}

/// Look at the talk once it has settled on line `id`, or once it has gone
/// unlooked at too long; then at whatever came meanwhile.
pub(super) async fn look(venue: String, mut id: String, token: String) {
    loop {
        let next = with_group(&venue, |group| {
            let Some(pending) = group.pending.as_ref() else {
                return Look::Done;
            };
            let latest = pending.message_id == id;
            let overdue = group
                .unjudged_since
                .is_some_and(|since| since.elapsed() >= MAX_WAIT);
            if !latest && !overdue {
                return Look::Done;
            }
            if group.judging || group.busy {
                return Look::Later;
            }
            group.judging = true;
            group.unjudged_since = None;
            group
                .pending
                .take()
                .map_or(Look::Done, |line| Look::Now(Box::new(line)))
        })
        .unwrap_or(Look::Done);
        let message = match next {
            Look::Done => return,
            Look::Later => {
                tokio::time::sleep(SETTLE).await;
                continue;
            }
            Look::Now(message) => *message,
        };
        let decided = judge(&message, &token).await;
        with_group(&venue, |group| {
            group.judging = false;
            // Going on with what she said, without calling her: taken up.
            if decided.as_ref().is_some_and(|(why, _)| *why == Why::Answer) {
                taken_up(group);
            }
        });
        if let Some((why, reason)) = decided
            && matches!(begin_turn(&venue), Turn::Began)
        {
            let replied = answer(&message, &token, Some(reason)).await;
            if replied && why != Why::Answer {
                info!(%venue, "[Group] she spoke up");
                spoke_up_now(&venue);
            }
            let mut next = finish_turn(&venue, replied).await;
            while let Some(message) = next {
                let replied = answer(&message, &token, None).await;
                next = finish_turn(&venue, replied).await;
            }
        }
        match with_group(&venue, |group| {
            group.pending.as_ref().map(|line| line.message_id.clone())
        })
        .flatten()
        {
            Some(newer) => id = newer,
            None => return,
        }
    }
}

/// Whether she says something about the talk this line ends, and why. Her
/// judgment is billed to the one who said it if they are of the community,
/// else to the site's owner, who hosts her there.
/// She spoke up unasked there: in mind now, and once it is known whether
/// anyone took it up, kept (see `merope::others`) so a restart does not
/// wipe how it has been going there.
pub(super) fn spoke_up_now(venue: &str) {
    let at = Instant::now();
    let line = with_group(venue, |group| {
        group.spoke_up.push_back((at, false));
        while group.spoke_up.len() > SPOKE_UP_KEPT {
            group.spoke_up.pop_front();
        }
        group
            .lines
            .iter()
            .rev()
            .find(|line| line.hers)
            .map(|line| line.text.clone())
    })
    .flatten();
    let Some(line) = line else {
        return;
    };
    let venue = venue.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(TAKEN_UP_WITHIN).await;
        let taken = with_group(&venue, |group| {
            group
                .spoke_up
                .iter()
                .find(|(when, _)| *when == at)
                .map(|(_, taken)| *taken)
        })
        .flatten()
        .unwrap_or(false);
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        crate::services::agent::merope::group::others::spoke_up(&db, &venue, &line, taken).await;
    });
}

/// Someone turned to her: her latest speaking up there, if recent, was
/// taken up.
pub(super) fn taken_up(group: &mut Group) {
    if let Some((at, taken)) = group.spoke_up.back_mut()
        && at.elapsed() < TAKEN_UP_WITHIN
    {
        *taken = true;
    }
}

pub(super) async fn judge(message: &GroupLine, token: &str) -> Option<(Why, String)> {
    let db = crate::services::process_db::database().ok()?;
    if stopped_answering(message) || is_muted(&message.venue()) {
        return None;
    }
    let owner = match lookup(&db, message).await {
        Some(PairingLookup::Paired { user_id }) => {
            current_binding(&db, message, user_id).await?;
            user_id
        }
        _ => crate::services::site_owner::site_owner_user_id(&db)
            .await
            .ok()?,
    };
    let venue = message.venue();
    see(&venue, token).await;
    let lines = transcript(&venue, None);
    let conversation: Vec<String> = lines
        .iter()
        .skip(lines.len().saturating_sub(CONVERSATION_LINES))
        .map(|line| {
            if line.role == "assistant" {
                format!("you：{}", line.content)
            } else {
                line.content.clone()
            }
        })
        .collect();
    // Talk she sees only a while after it was said.
    let late = with_group(&venue, |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| !line.hers)
            .map(|line| chrono::Utc::now() - line.at)
            .filter(|age| *age >= LATE)
            .map(myriad_merope::doing::ago_text)
    })
    .flatten();
    let last_spoke = with_group(&venue, |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| line.hers)
            .map(|line| myriad_merope::doing::ago_text(chrono::Utc::now() - line.at))
    })
    .flatten();
    let how_it_went = with_group(&venue, |group| {
        myriad_merope::joining::how_it_went(
            group.spoke_up.len(),
            group.spoke_up.iter().filter(|(_, taken)| *taken).count(),
        )
    })
    .flatten();
    crate::services::agent::merope::group::joining::decide(
        &db,
        owner,
        &conversation,
        &crate::services::agent::merope::group::joining::Here {
            last_spoke: last_spoke.as_deref(),
            late: late.as_deref(),
            how_it_went: how_it_went.as_deref(),
        },
    )
    .await
}
