//! Saying something first, and the group as she weighs doing so.

use super::*;

pub(crate) fn speak_first(cx: &mut Cx, reason: String, owner: Option<i32>, waiter: String) {
    let now = cx.now();
    if begin(cx.group, now).is_err() {
        cx.settle(&Some(waiter), false);
        return;
    }
    let id = cx.id();
    let errand = Errand {
        id,
        awaiting: String::new(),
        kind: Kind::First {
            waiter,
            reason: reason.clone(),
            owner,
            spoke: None,
        },
        doing: Doing::Timing,
    };
    let message = cx.group.reach.clone().unwrap_or(Message {
        platform: String::new(),
        chat: String::new(),
        message_id: String::new(),
        thread: None,
        from: String::new(),
        display_name: String::new(),
        text: String::new(),
        addressed: false,
        reply_to: None,
        images: Vec::new(),
        shown_as: None,
    });
    start_answer(cx, errand, message, Some(reason), true);
}

pub(crate) fn offer_view(group: &Group, now: i64) -> Option<Offer> {
    if group.busy || in_talk(group, now) || muted_now(group, now) {
        return None;
    }
    group.reach.as_ref()?;
    let quiet = group
        .lines
        .back()
        .map(|line| line.at_micros())
        .or(group.seen_before)
        .map(|at| now - at);
    if quiet.is_some_and(|quiet| quiet > SEEN_WITHIN_SECONDS * 1_000_000) {
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
        Some(quiet) => crate::doing::ago_text(chrono::Duration::microseconds(quiet)),
        None => "a good while (nothing said there lately)".to_string(),
    };
    let spoke_up_lately = group
        .spoke_up
        .iter()
        .map(|one| {
            serde_json::json!({
                "ago": crate::doing::ago_text(chrono::Duration::microseconds((now - one.at).max(0))),
                "takenUp": one.taken,
            })
        })
        .collect();
    Some(Offer {
        lines,
        quiet_for,
        spoke_up_lately,
    })
}
