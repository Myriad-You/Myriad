//! Lines coming in: kept, taken up when they call her, glanced at when they
//! do not, mutes, and the talk read back after she was not there.

use super::*;

pub(crate) fn record(cx: &mut Cx, message: &Message, restore: Option<Restore>) -> bool {
    let now = cx.now();
    let people = people(cx.group, now);
    if let Some(restore) = restore {
        restored(cx, restore.lines, restore.spoke_up, restore.table);
    }
    let line = Line {
        at: time(now),
        message_id: Some(message.message_id.clone()),
        name: message.display_name.clone(),
        from: Some(message.from.clone()),
        text: bounded(&by_name(&message.said(), &people)),
        hers: false,
        addressed: message.addressed,
        images: message.images.clone(),
        seen: Vec::new(),
    };
    if !remember_line(cx.group, line, now, cx.out) {
        return false;
    }
    if let Some(shown_as) = message
        .shown_as
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        cx.group.shown_as = Some(shown_as.chars().take(MAX_SHOWN_AS_CHARS).collect());
    }
    heard_at_table(cx, message);
    // Written down as a group she is in, now and then.
    let due = cx
        .group
        .reach_kept
        .is_none_or(|kept| now - kept >= seconds(REACH_KEPT_EVERY_SECONDS));
    if due {
        cx.group.reach_kept = Some(now);
        cx.out.effects.push(Effect::KeepReach {
            thread: message.thread,
            seen_at: now,
        });
    }
    let typed = crate::talk_shape::typed_by(
        &super::ledger_who(&message.from),
        lines::time(now).timestamp(),
        &message.text,
    );
    if note_said(cx.group, typed, None, Some(&message.text)) {
        keep_ledger(cx.group, cx.out);
    }
    // Not on this end, she hears nothing there (D3-2).
    let away = cx.at.her.away;
    if !away {
        cx.out.effects.push(Effect::Jargon {
            text: message.text.clone(),
        });
    }
    take_in(cx);
    if cx.group.pictures.len() > PICTURES_KEPT {
        cx.group.pictures.clear();
    }
    for image in &message.images {
        *cx.group.pictures.entry(image.key.clone()).or_default() += 1;
    }
    true
}

/// Once enough of the talk has gone by, what she heard in it.
pub(crate) fn take_in(cx: &mut Cx) {
    let now = cx.now();
    if cx.at.her.away {
        cx.group.heard_upto = cx
            .group
            .lines
            .back()
            .map(|line| line.at_micros())
            .or(cx.group.heard_upto);
        return;
    }
    let upto = cx.group.heard_upto;
    let fresh: Vec<&Line> = cx
        .group
        .lines
        .iter()
        .filter(|line| upto.is_none_or(|upto| line.at_micros() > upto))
        .collect();
    let theirs = fresh.iter().filter(|line| !line.hers).count();
    let waited = fresh
        .first()
        .is_some_and(|line| now - line.at_micros() >= HEARD_AFTER_SECONDS * 1_000_000);
    if theirs < HEARD_EVERY && !(waited && theirs >= HEARD_AT_LEAST) {
        return;
    }
    let stretch: Vec<Stretch> = fresh
        .iter()
        .map(|line| Stretch {
            at: line.at.fixed_offset().to_rfc3339(),
            name: line.name.clone(),
            text: line.said(),
            hers: line.hers,
        })
        .collect();
    cx.group.heard_upto = fresh.last().map(|line| line.at_micros());
    cx.out.effects.push(Effect::TakeIn { stretch });
}

pub(crate) fn first_time(group: &mut Group, message_id: &str) -> bool {
    if message_id.is_empty() {
        return true;
    }
    if group.handled.iter().any(|id| id == message_id) {
        return false;
    }
    group.handled.push_back(message_id.to_string());
    while group.handled.len() > HANDLED_KEPT {
        group.handled.pop_front();
    }
    true
}

pub(crate) fn handle(cx: &mut Cx, message: Message, waiter: Option<String>) {
    let now = cx.now();
    // She is not on this end: not in the group either (D3-2).
    if cx.at.her.away {
        cx.group.reach = Some(message.clone());
        cx.group.left_while_away = Some((message, now));
        cx.settle(&waiter, false);
        return;
    }
    if !first_time(cx.group, &message.message_id) {
        cx.settle(&waiter, false);
        return;
    }
    taken_up(cx.group, now);
    cx.group.reach = Some(message.clone());
    let talking = in_talk(cx.group, now);
    let at = where_she_is(cx.at, talking, true);
    let wait = crate::timing::seen_after(at, cx.at.rolls.roll(), cx.at.rolls.roll())
        + crate::timing::reading(message.text.chars().count());
    // A long wait goes on apart from whoever handed the line in.
    let waiter = if wait > HOLD_AT_MOST_SECONDS {
        cx.settle(&waiter, true);
        None
    } else {
        waiter
    };
    let id = cx.id();
    let key = cx.wake(wait);
    cx.group.errands.push(Errand {
        id,
        awaiting: key,
        kind: Kind::TakeUp {
            waiter,
            message,
            again: true,
        },
        doing: Doing::Seeing,
    });
}

pub(crate) fn glance_after(at: &mut At) -> f64 {
    if let crate::timing::Where::Asleep { wakes_in } = where_she_is(at, false, true) {
        return wakes_in + at.rolls.range(GLANCE_AFTER_SECONDS) as f64;
    }
    let free_in = at
        .her
        .busy_for
        .filter(|free| *free > 0.0)
        .unwrap_or_default()
        .min(LONGEST_BUSY_SECONDS);
    free_in + at.rolls.range(GLANCE_AFTER_SECONDS) as f64
}

pub(crate) fn notice(cx: &mut Cx, message: Message) {
    // Away; or hosting a game there, when she does not speak up unasked.
    if cx.at.her.away || hosting_now(cx.group) {
        return;
    }
    let now = cx.now();
    let id = message.message_id.clone();
    cx.group.unjudged_since.get_or_insert(now);
    cx.group.reach = Some(message.clone());
    cx.group.pending = Some(message);
    let after = if in_talk(cx.group, now) {
        SETTLE_SECONDS
    } else if cx.group.glance_at.is_some() {
        return;
    } else {
        let after = glance_after(cx.at);
        cx.group.glance_at = Some(now + seconds(after));
        after
    };
    let errand = cx.id();
    let key = cx.wake(after);
    cx.group.errands.push(Errand {
        id: errand,
        awaiting: key,
        kind: Kind::Look {
            id,
            message: None,
            why: None,
            spoke: false,
        },
        doing: Doing::Glancing,
    });
}

pub(crate) fn told(mute: Mute, everyone: bool) -> String {
    fn how_long(seconds: u64) -> String {
        let minutes = seconds.div_ceil(60);
        match minutes {
            0..=89 => format!(" {minutes} 分钟"),
            90..=2159 => format!(" {} 小时", (minutes + 30) / 60),
            _ => format!(" {} 天", (minutes + 720) / 1440),
        }
    }
    match (mute, everyone) {
        (Mute::For { seconds }, false) => format!("（把你禁言了{}）", how_long(seconds)),
        (Mute::For { seconds }, true) => format!("（全员禁言{}）", how_long(seconds)),
        (Mute::UntilLifted, false) => "（把你禁言了）".to_string(),
        (Mute::UntilLifted, true) => "（开了全员禁言）".to_string(),
        (Mute::Lifted, false) => "（解除了你的禁言）".to_string(),
        (Mute::Lifted, true) => "（解除了全员禁言）".to_string(),
    }
}

/// Muted with no end given: as long as a group can mute anyone.
pub(crate) const UNTIL_LIFTED_DAYS: i64 = 30;

pub(crate) fn muted(cx: &mut Cx, by: &str, mute: Mute, everyone: bool, restore: Option<Restore>) {
    let now = cx.now();
    let until = match mute {
        Mute::For { seconds } => {
            Some(now + (seconds.min(i64::MAX as u64 / 1_000_000) as i64) * 1_000_000)
        }
        Mute::UntilLifted => Some(now + UNTIL_LIFTED_DAYS * 86_400 * 1_000_000),
        Mute::Lifted => None,
    };
    if everyone {
        cx.group.everyone_muted_until = until;
    } else {
        cx.group.muted_until = until;
    }
    let name = (!by.is_empty())
        .then(|| {
            people(cx.group, now)
                .into_iter()
                .find(|(_, from)| from == by)
                .map(|(name, _)| name)
        })
        .flatten()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "群管理".to_string());
    if let Some(restore) = restore {
        restored(cx, restore.lines, restore.spoke_up, restore.table);
    }
    let line = Line {
        at: time(now),
        message_id: None,
        name,
        from: (!by.is_empty()).then(|| by.to_string()),
        text: told(mute, everyone),
        hers: false,
        addressed: false,
        images: Vec::new(),
        seen: Vec::new(),
    };
    remember_line(cx.group, line, now, cx.out);
}

/// The line that called her before `up_since` and that she never answered,
/// out of what the group said meanwhile: the latest, if she has not spoken
/// there since and it is recent enough to still be talk. Each line once.
pub(crate) fn left_waiting(
    group: &mut Group,
    past: &[Past],
    up_since: i64,
    now: i64,
) -> Option<Message> {
    let before = up_since - 5 * 1_000_000;
    let mut past: Vec<&Past> = past.iter().collect();
    past.sort_by_key(|line| line.at);
    let after_her = past
        .iter()
        .rposition(|line| line.hers)
        .map_or(0, |at| at + 1);
    let line = past[after_her..].iter().rev().find(|line| {
        !line.hers
            && line.message.addressed
            && line.at < before
            && now - line.at < TRANSCRIPT_FOR_SECONDS * 1_000_000
    })?;
    let key = line.message.message_id.clone();
    if group.taken_back.contains(&key) {
        return None;
    }
    group.taken_back.push_back(key);
    while group.taken_back.len() > WAITING_LINES {
        group.taken_back.pop_front();
    }
    Some(line.message.clone())
}

pub(crate) fn resumed(cx: &mut Cx, why: Resume, past: Vec<Past>, up_since: Option<i64>) {
    let now = cx.now();
    match why {
        Resume::Back => {
            let left = cx
                .group
                .left_while_away
                .take()
                .filter(|(_, at)| now - *at < TRANSCRIPT_FOR_SECONDS * 1_000_000);
            if let Some((line, _)) = left {
                handle(cx, line, None);
            }
        }
        Resume::Restart => {
            let waiting = left_waiting(cx.group, &past, up_since.unwrap_or(now), now);
            let people = people(cx.group, now);
            let fresh: Vec<Line> = past
                .into_iter()
                .filter(|past| {
                    !cx.group.lines.iter().any(|line| {
                        line.message_id.as_deref() == Some(past.message.message_id.as_str())
                    })
                })
                .map(|past| Line {
                    at: time(past.at),
                    message_id: Some(past.message.message_id.clone()),
                    name: if past.hers {
                        String::new()
                    } else {
                        past.message.display_name.clone()
                    },
                    from: (!past.hers).then(|| past.message.from.clone()),
                    text: bounded(&by_name(&past.message.said(), &people)),
                    hers: past.hers,
                    addressed: past.message.addressed,
                    images: if past.hers {
                        Vec::new()
                    } else {
                        past.message.images
                    },
                    seen: Vec::new(),
                })
                .filter(|line| within(line, now))
                .collect();
            if !fresh.is_empty() {
                for line in &fresh {
                    let by = if line.hers {
                        HER.to_string()
                    } else {
                        super::ledger_who(line.from.as_deref().unwrap_or_default())
                    };
                    let typed = crate::talk_shape::typed_by(&by, line.at.timestamp(), &line.text);
                    note_said(cx.group, typed, None, Some(&line.text));
                }
                merge_restored(cx.group, fresh, now);
                cx.out.effects.push(Effect::KeepLines {
                    lines: cx.group.lines.iter().cloned().collect(),
                });
                keep_ledger(cx.group, cx.out);
            }
            if let Some(line) = waiting {
                handle(cx, line, None);
            }
        }
    }
}
