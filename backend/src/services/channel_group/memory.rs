//! Keeping a group's lines across restarts, catching up on missed ones, and recording every line she hears.

use super::*;

/// Keep a line in mind: in memory, and in the runtime registry so a restart
/// does not wipe the group from her mind. The first line after a restart
/// brings back what was kept before it.
pub(super) async fn remember_line(venue: &str, line: Line) {
    let db = crate::services::process_db::database().ok();
    remember_line_on(db.as_ref(), venue, line).await;
}

pub(super) async fn remember_line_on(db: Option<&DatabaseConnection>, venue: &str, line: Line) {
    if let Some(db) = db {
        restore(db, venue).await;
    }
    let lines = with_group(venue, |group| {
        push_line(group, line);
        group.lines.iter().cloned().collect::<Vec<_>>()
    });
    if let (Some(db), Some(lines)) = (db, lines) {
        keep_lines(db, venue, lines).await;
    }
}

/// Bring back the lines kept before a restart, once.
pub(super) async fn restore(db: &DatabaseConnection, venue: &str) {
    if !with_group(venue, |group| !group.restored).unwrap_or(false) {
        return;
    }
    // Not read is not nothing kept: marking it restored now would let the
    // lines since overwrite what was kept before.
    let stored = match crate::services::runtime_registry::get::<StoredLines>(
        db,
        LINES_NAMESPACE,
        venue,
    )
    .await
    {
        Ok(stored) => stored,
        Err(error) => {
            warn!(%error, %venue, "[Group] could not read back the lines kept before a restart");
            return;
        }
    };
    let spoke_up = crate::services::agent::merope::group::others::spoke_up_lately(
        db,
        venue,
        SPOKE_UP_KEPT as u64,
    )
    .await;
    with_group(venue, |group| {
        if !group.restored {
            group.restored = true;
            if let Some(stored) = stored {
                merge_restored(group, stored.lines);
            }
            restore_spoke_up(group, &spoke_up, chrono::Utc::now());
        }
    });
}

/// How her speaking up went there before a restart, oldest first, under
/// what she has said since.
pub(super) fn restore_spoke_up(
    group: &mut Group,
    kept: &[(chrono::DateTime<chrono::Utc>, bool)],
    now: chrono::DateTime<chrono::Utc>,
) {
    let since: Vec<(Instant, bool)> = group.spoke_up.drain(..).collect();
    for (at, taken) in kept {
        let ago = (now - *at).to_std().unwrap_or_default();
        if let Some(at) = Instant::now().checked_sub(ago) {
            group.spoke_up.push_back((at, *taken));
        }
    }
    group.spoke_up.extend(since);
    while group.spoke_up.len() > SPOKE_UP_KEPT {
        group.spoke_up.pop_front();
    }
}

pub(super) async fn keep_lines(db: &DatabaseConnection, venue: &str, lines: Vec<Line>) {
    let keep_until = (chrono::Utc::now()
        + chrono::Duration::from_std(TRANSCRIPT_FOR).unwrap_or_default())
    .timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        db,
        LINES_NAMESPACE,
        venue,
        crate::services::runtime_registry::RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &StoredLines { lines },
        keep_until,
    )
    .await
    {
        warn!(%error, %venue, "[Group] could not keep the group's lines");
    }
}

/// What a group said while she was not connected (see
/// `onebot::decode::decode_group_history`), read back as a person scrolls up
/// on opening a chat: each line she does not have yet goes in at its time,
/// hers as hers. Nothing is answered: it is only read.
pub async fn catch_up(venue: &str, past: Vec<(GroupLine, chrono::DateTime<chrono::Utc>, bool)>) {
    heard_since_up();
    let db = crate::services::process_db::database().ok();
    if let Some(db) = &db {
        restore(db, venue).await;
    }
    // A line that called her before a restart, which she never got to:
    // taken up now, late, once what was said meanwhile is read. Only OneBot
    // reads a group back, and its sends need no token.
    let waiting = left_waiting(venue, &past);
    let people = people(venue);
    let fresh: Vec<Line> = with_group(venue, |group| {
        past.into_iter()
            .filter(|(message, _, _)| {
                !group
                    .lines
                    .iter()
                    .any(|line| line.message_id.as_deref() == Some(message.message_id.as_str()))
            })
            .map(|(message, at, hers)| Line {
                at,
                message_id: Some(message.message_id.clone()),
                name: if hers {
                    String::new()
                } else {
                    message.display_name.clone()
                },
                from: (!hers).then(|| message.from.clone()),
                text: bounded(&by_name(&message.said(), &people)),
                hers,
                addressed: message.addressed,
                images: if hers { Vec::new() } else { message.images },
                seen: Vec::new(),
            })
            .filter(|line| within(line, TRANSCRIPT_FOR))
            .collect()
    })
    .unwrap_or_default();
    if fresh.is_empty() {
        take_back(venue, waiting, String::new());
        return;
    }
    info!(%venue, lines = fresh.len(), "[Group] read back what was said while she was away");
    for line in &fresh {
        let by = if line.hers {
            HER.to_string()
        } else {
            ledger_who(line.from.as_deref().unwrap_or_default())
        };
        let typed = myriad_merope::talk_shape::typed_by(&by, line.at.timestamp(), &line.text);
        note_said(venue, typed, None, Some(&line.text));
    }
    let lines = with_group(venue, |group| {
        merge_restored(group, fresh);
        group.lines.iter().cloned().collect::<Vec<_>>()
    });
    if let (Some(db), Some(lines)) = (&db, lines) {
        keep_lines(db, venue, lines).await;
    }
    keep_ledger(venue).await;
    take_back(venue, waiting, String::new());
}

/// After a restart, on a platform that cannot read a group back (Telegram,
/// Discord): in each group kept lately, the line that called her before it
/// and that she never got to, read from the lines kept (see `left_waiting`),
/// taken up now with the configured token.
pub async fn take_back_kept(platform: ChannelPlatform) {
    heard_since_up();
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    let prefix = format!("{}:", platform.slug());
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let kept = crate::services::runtime_registry::list(&db, LINES_NAMESPACE, None, None)
        .await
        .unwrap_or_default();
    for row in kept {
        let Some(chat) = row.record_id.strip_prefix(&prefix).map(str::to_string) else {
            continue;
        };
        let venue = row.record_id.clone();
        let Some(token) = super::reaching_out::token_for(platform, &chat, &config) else {
            continue;
        };
        let Ok(stored) = serde_json::from_value::<StoredLines>(row.payload) else {
            continue;
        };
        let thread =
            crate::services::runtime_registry::get::<StoredReach>(&db, REACH_NAMESPACE, &venue)
                .await
                .ok()
                .flatten()
                .and_then(|reach| reach.thread);
        let past = kept_as_heard(platform, &chat, thread, stored.lines);
        restore(&db, &venue).await;
        take_back(&venue, left_waiting(&venue, &past), token);
    }
}

/// Lines kept of a group, as the group's talk read back (oldest first, hers
/// marked): what `left_waiting` looks over.
pub(super) fn kept_as_heard(
    platform: ChannelPlatform,
    chat: &str,
    thread: Option<i64>,
    lines: Vec<Line>,
) -> Vec<(GroupLine, chrono::DateTime<chrono::Utc>, bool)> {
    lines
        .into_iter()
        .map(|line| {
            (
                GroupLine {
                    platform,
                    chat: chat.to_string(),
                    message_id: line.message_id.unwrap_or_default(),
                    thread,
                    from: line.from.unwrap_or_default(),
                    display_name: line.name,
                    text: line.text,
                    addressed: line.addressed,
                    reply_to: None,
                    images: line.images,
                },
                line.at,
                line.hers,
            )
        })
        .collect()
}

/// Groups of `platform` she has kept lines of lately.
pub async fn groups_lately(platform: ChannelPlatform) -> Vec<String> {
    let Ok(db) = crate::services::process_db::database() else {
        return Vec::new();
    };
    let prefix = format!("{}:", platform.slug());
    crate::services::runtime_registry::list(&db, LINES_NAMESPACE, None, None)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| row.record_id.strip_prefix(&prefix).map(str::to_string))
        .collect()
}

/// Write the group down as one she is in, now and then: kept as long as it
/// counts as one (see `reaching_out::SEEN_WITHIN`).
async fn keep_reach(message: &GroupLine) {
    let venue = message.venue();
    let due = with_group(&venue, |group| {
        let due = group
            .reach_kept
            .is_none_or(|kept| kept.elapsed() >= REACH_KEPT_EVERY);
        if due {
            group.reach_kept = Some(Instant::now());
        }
        due
    })
    .unwrap_or(false);
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    if !due {
        return;
    }
    let now = chrono::Utc::now();
    let keep_until = (now
        + chrono::Duration::from_std(super::reaching_out::SEEN_WITHIN).unwrap_or_default())
    .timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        &db,
        REACH_NAMESPACE,
        &venue,
        crate::services::runtime_registry::RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &StoredReach {
            thread: message.thread,
            seen_at: now,
        },
        keep_until,
    )
    .await
    {
        warn!(%error, %venue, "[Group] could not keep that she is in the group");
    }
}

pub(super) fn bounded(text: &str) -> String {
    text.chars().take(MAX_LINE_CHARS).collect()
}

/// Keep a group line in mind, whoever wrote it.
pub async fn record(message: &GroupLine) {
    heard_since_up();
    let line = Line {
        at: chrono::Utc::now(),
        message_id: Some(message.message_id.clone()),
        name: message.display_name.clone(),
        from: Some(message.from.clone()),
        text: bounded(&by_name(&message.said(), &people(&message.venue()))),
        hers: false,
        addressed: message.addressed,
        images: message.images.clone(),
        seen: Vec::new(),
    };
    let venue = message.venue();
    remember_line(&venue, line).await;
    keep_reach(message).await;
    let typed = myriad_merope::talk_shape::typed_by(
        &ledger_who(&message.from),
        chrono::Utc::now().timestamp(),
        &message.text,
    );
    if note_said(&venue, typed, None, Some(&message.text)) {
        keep_ledger(&venue).await;
    }
    take_in(&venue);
    with_group(&venue, |group| {
        if group.pictures.len() > PICTURES_KEPT {
            group.pictures.clear();
        }
        for image in &message.images {
            *group.pictures.entry(image.key.clone()).or_default() += 1;
        }
    });
}

/// Once enough of the group's talk has gone by, take in what she heard in
/// it about things, on the site owner's account: she keeps it as her own,
/// with no name and no group on it.
pub(super) fn take_in(venue: &str) {
    let stretch = with_group(venue, |group| {
        let upto = group.heard_upto;
        let fresh: Vec<&Line> = group
            .lines
            .iter()
            .filter(|line| upto.is_none_or(|upto| line.at > upto))
            .collect();
        let theirs = fresh.iter().filter(|line| !line.hers).count();
        let waited = fresh
            .first()
            .is_some_and(|line| chrono::Utc::now() - line.at >= HEARD_AFTER);
        if theirs < HEARD_EVERY && !(waited && theirs >= HEARD_AT_LEAST) {
            return None;
        }
        group.heard_upto = fresh.last().map(|line| line.at);
        Some(
            fresh
                .into_iter()
                .map(|line| {
                    (
                        line.at.fixed_offset(),
                        line.name.clone(),
                        line.text.clone(),
                        line.hers,
                    )
                })
                .collect::<Vec<_>>(),
        )
    })
    .flatten();
    let Some(stretch) = stretch else {
        return;
    };
    let venue = venue.to_string();
    tokio::spawn(async move {
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        let Ok(owner) = crate::services::site_owner::site_owner_user_id(&db).await else {
            return;
        };
        let heard = stretch
            .iter()
            .map(|(_, name, text, hers)| myriad_merope::heard::Said {
                name: name.clone(),
                text: text.clone(),
                hers: *hers,
            })
            .collect();
        crate::services::agent::merope::group::heard::take_in(&db, owner, heard).await;
        // And what the group was like, while its talk is still at hand.
        crate::services::agent::merope::group::bits::go_over_stretch(&db, owner, &venue, stretch)
            .await;
    });
}

pub(super) async fn record_hers(venue: &str, text: &str) {
    let line = Line {
        at: chrono::Utc::now(),
        message_id: None,
        name: String::new(),
        from: None,
        text: bounded(text),
        hers: true,
        addressed: false,
        images: Vec::new(),
        seen: Vec::new(),
    };
    remember_line(venue, line).await;
}
