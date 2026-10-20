//! What she keeps of a group between lines: whether she is in the talk, muted,
//! answering one who never stops, taking her turn, and her speaking up.

use super::*;

pub(crate) fn restored(
    cx: &mut Cx,
    lines: Option<Vec<Line>>,
    kept: Vec<SpokeUp>,
    table: Option<Value>,
) {
    if cx.group.restored {
        return;
    }
    // The game with seats that was on: back at the table, going on.
    if let Some(table) = table.filter(|_| cx.group.seating.is_none())
        && let Ok(seating) = serde_json::from_value::<Seating>(table)
    {
        resume_table(cx, seating);
    }
    cx.group.restored = true;
    let now = cx.now();
    if let Some(lines) = lines {
        // What she heard in the lines kept before the restart was taken in
        // then: taken in again, the same news and bits would be kept twice.
        // A few lines not yet taken in go with them.
        let kept_upto = lines.iter().map(Line::at_micros).max();
        if cx.group.heard_upto.is_none() {
            cx.group.heard_upto = kept_upto;
        }
        merge_restored(cx.group, lines, now);
    }
    let since: Vec<SpokeUp> = cx.group.spoke_up.drain(..).collect();
    cx.group.spoke_up.extend(kept);
    cx.group.spoke_up.extend(since);
    while cx.group.spoke_up.len() > SPOKE_UP_KEPT {
        cx.group.spoke_up.pop_front();
    }
}

pub(crate) fn in_talk(group: &Group, now: i64) -> bool {
    group
        .called
        .is_some_and(|at| now - at < seconds(IN_TALK_SECONDS))
        || group
            .lines
            .iter()
            .rev()
            .find(|line| line.hers)
            .is_some_and(|line| {
                let age = now - line.at_micros();
                age >= 0 && age < seconds(IN_TALK_SECONDS)
            })
}

pub(crate) fn muted_now(group: &Group, now: i64) -> bool {
    [group.muted_until, group.everyone_muted_until]
        .into_iter()
        .flatten()
        .any(|until| until > now)
}

pub(crate) fn stopped_answering(group: &mut Group, message: &Message, now: i64) -> bool {
    group
        .paused
        .retain(|_, since| now - *since < seconds(LOOP_PAUSE_SECONDS));
    group.paused.contains_key(&message.from)
}

pub(crate) fn answered(group: &mut Group, message: &Message, now: i64) {
    group.answered.push_back(Answered {
        from: message.from.clone(),
        at: now,
    });
    while group.answered.len() > LOOP_ROUNDS {
        group.answered.pop_front();
    }
    let looping = group.answered.len() == LOOP_ROUNDS
        && group.answered.iter().all(|one| one.from == message.from)
        && group
            .answered
            .front()
            .is_some_and(|one| now - one.at < seconds(LOOP_WINDOW_SECONDS));
    if looping {
        group.paused.insert(message.from.clone(), now);
        group.answered.clear();
    }
}

pub(crate) fn taken_up(group: &mut Group, now: i64) {
    if let Some(last) = group.spoke_up.back_mut()
        && now - last.at < seconds(TAKEN_UP_WITHIN_SECONDS)
    {
        last.taken = true;
    }
}

/// Count one more of today's, unless `limit` is reached.
pub(crate) fn count_today(slot: &mut Option<DayCount>, today: &str, limit: u32) -> bool {
    let count = match slot {
        Some(day) if day.day == today => day.count,
        _ => 0,
    };
    if count >= limit {
        return false;
    }
    *slot = Some(DayCount {
        day: today.to_string(),
        count: count + 1,
    });
    true
}

pub(crate) fn begin(group: &mut Group, now: i64) -> Result<(), Option<f64>> {
    if group.busy {
        return Err(None);
    }
    if let Some(at) = group.last_reply {
        let rest = seconds(GROUP_PAUSE_SECONDS) - (now - at);
        if rest > 0 {
            return Err(Some(rest as f64 / 1_000_000.0));
        }
    }
    group.busy = true;
    Ok(())
}

pub(crate) fn park(group: &mut Group, message: Message) {
    group.waiting.push_back(message);
    while group.waiting.len() > WAITING_LINES {
        group.waiting.pop_front();
    }
}

pub(crate) fn end_turn(group: &mut Group, replied: bool, now: i64) {
    group.busy = false;
    if replied {
        group.last_reply = Some(now);
    }
}

/// What she does when the talk is quiet enough or she is free: where she
/// is in her day.
pub(crate) fn where_she_is(at: &At, in_talk: bool, may_sleep: bool) -> crate::timing::Where {
    use crate::timing::Where;
    if in_talk {
        return Where::InTalk;
    }
    if may_sleep && let Some(wakes_in) = at.her.asleep_for {
        return Where::Asleep { wakes_in };
    }
    match at
        .her
        .busy_for
        .map(|free| free.trunc())
        .filter(|free| *free > 0.0)
    {
        Some(free_in) => Where::Busy { free_in },
        None => Where::Free,
    }
}

pub(crate) fn spoke_up_now(cx: &mut Cx) {
    let now = cx.now();
    cx.group.spoke_up.push_back(SpokeUp {
        at: now,
        taken: false,
    });
    while cx.group.spoke_up.len() > SPOKE_UP_KEPT {
        cx.group.spoke_up.pop_front();
    }
    let Some(line) = cx
        .group
        .lines
        .iter()
        .rev()
        .find(|line| line.hers)
        .map(|line| line.text.clone())
    else {
        return;
    };
    let id = cx.id();
    let key = cx.wake(TAKEN_UP_WITHIN_SECONDS);
    cx.group.errands.push(Errand {
        id,
        awaiting: key,
        kind: Kind::SpokeUp { at: now, line },
        doing: Doing::Timing,
    });
}
