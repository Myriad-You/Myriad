//! One turn at a time per group: taking up a line that calls her, and the lines waiting while she is busy.

use super::*;

/// A group's venue as memories store it (`group:telegram:-100123`).
pub(super) fn unified_venue(venue: &str) -> String {
    crate::services::agent::memory::unified::Audience::group(venue, 0).venue()
}

/// Take the group's single turn, if it is free and not just replied in.
pub(super) fn begin_turn(venue: &str) -> Turn {
    with_group(venue, begin).unwrap_or(Turn::Busy)
}

pub(super) fn begin(group: &mut Group) -> Turn {
    if group.busy {
        return Turn::Busy;
    }
    if let Some(rest) = group
        .last_reply
        .and_then(|at| GROUP_PAUSE.checked_sub(at.elapsed()))
        .filter(|rest| !rest.is_zero())
    {
        return Turn::Resting(rest);
    }
    group.busy = true;
    Turn::Began
}

/// A line that spoke to her while she was busy: answered after, in order;
/// past a few, the oldest goes.
pub(super) fn park(group: &mut Group, message: GroupLine) {
    group.waiting.push_back(message);
    while group.waiting.len() > WAITING_LINES {
        group.waiting.pop_front();
    }
}

pub(super) fn end_turn(venue: &str, replied: bool) {
    with_group(venue, |group| {
        group.busy = false;
        if replied {
            group.last_reply = Some(Instant::now());
        }
    });
}

/// Answer one group line that spoke to her, once she sees it (see
/// `merope::timing`): at once in talk she is in, otherwise when she next
/// looks, and after she wakes if she is asleep; then now, or when she is
/// done with the one on hand. A long wait does not hold the caller.
pub async fn handle(message: GroupLine, token: String) {
    use crate::services::agent::merope::group::timing;
    let venue = message.venue();
    let talking = with_group(&venue, |group| {
        taken_up(group);
        group.reach = Some((message.clone(), token.clone()));
        in_talk(group)
    })
    .unwrap_or(false);
    let at = timing::where_she_is(talking, true);
    let wait = timing::until_read(at, &message.text);
    info!(%venue, ?at, seconds = wait.as_secs(), "[Group] she will see the line");
    if wait > HOLD_AT_MOST {
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            take_up(message, token).await;
        });
        return;
    }
    tokio::time::sleep(wait).await;
    take_up(message, token).await;
}

pub(super) async fn take_up(mut message: GroupLine, token: String) {
    let venue = message.venue();
    with_group(&venue, |group| group.called = Some(Instant::now()));
    loop {
        // Busy or not is decided under the same lock that parks the line, so
        // the turn on hand cannot end without seeing it.
        let turn = with_group(&venue, |group| {
            let turn = begin(group);
            if matches!(turn, Turn::Busy) {
                park(group, message.clone());
            }
            turn
        })
        .unwrap_or(Turn::Busy);
        match turn {
            Turn::Began => break,
            Turn::Resting(rest) => tokio::time::sleep(rest).await,
            Turn::Busy => {
                info!(%venue, "[Group] busy; the line waits for her");
                return;
            }
        }
    }
    loop {
        let replied = answer(&message, &token, None).await;
        match finish_turn(&venue, replied).await {
            Some(next) => message = next,
            None => return,
        }
    }
}

/// End the turn; if a line waited meanwhile, take the turn again for it
/// after the pause.
pub(super) async fn finish_turn(venue: &str, replied: bool) -> Option<GroupLine> {
    end_turn(venue, replied);
    with_group(venue, |group| !group.waiting.is_empty()).filter(|waiting| *waiting)?;
    if replied {
        tokio::time::sleep(GROUP_PAUSE).await;
    }
    loop {
        match begin_turn(venue) {
            Turn::Began => break,
            Turn::Resting(rest) => tokio::time::sleep(rest).await,
            // Someone else took the turn; they will find the line waiting.
            Turn::Busy => return None,
        }
    }
    let next = with_group(venue, |group| group.waiting.pop_front()).flatten();
    if next.is_none() {
        end_turn(venue, false);
    }
    next
}
