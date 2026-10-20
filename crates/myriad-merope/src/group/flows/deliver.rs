//! Her reply going out message by message: typed, looked over, sent, kept
//! back when the talk moved on, stopped when she is called.

use super::*;

/// Looking her draft over against what came meanwhile. At 8 seconds a
/// third of these were cut off, and an unchecked draft goes out as it was,
/// however far the talk had moved.
const STILL_WITHIN_MS: u64 = 20_000;

pub(crate) fn text_limit(platform: &str) -> usize {
    match platform {
        "discord" => myriad_agent_rules::channel::DISCORD_TEXT_LIMIT,
        _ => myriad_agent_rules::channel::TELEGRAM_TEXT_LIMIT,
    }
}

/// The next message of her reply: typed, looked over, sent.
pub(crate) fn next_message(cx: &mut Cx, to: &Message, d: &mut Deliver, began: i64) -> Next {
    let now = cx.now();
    if d.index >= d.messages.len() {
        keep_ledger(cx.group, cx.out);
        return Next::Done(true);
    }
    let message = &d.messages[d.index];
    let typing = crate::timing::typing(message.chars().count(), cx.at.rolls.roll());
    // Typing it takes as long as it takes; the first she was already at
    // while she thought.
    let left = if d.index == 0 {
        (typing - (now - began) as f64 / 1_000_000.0).max(0.0)
    } else {
        typing
    };
    if left > 0.0 {
        d.stage = DeliverStage::TypingMark;
        d.left = left;
        let to = to.target();
        return Next::Wait(cx.effect(|id| Effect::Typing { id, to }));
    }
    look_over(cx, to, d)
}

/// Before a message goes: what came meanwhile.
pub(crate) fn look_over(cx: &mut Cx, to: &Message, d: &mut Deliver) -> Next {
    let came = came_since(cx.group, d.since, &to.from);
    if came.len() > d.looked {
        let fresh = &came[d.looked..];
        if d.sent {
            // Under way, the one she answers speaking again, or someone
            // calling her, stops her.
            if fresh.iter().any(|(_, _, stops)| *stops) {
                keep_ledger(cx.group, cx.out);
                return Next::Done(true);
            }
        } else {
            let fresh: Vec<(String, String)> = fresh
                .iter()
                .map(|(who, what, _)| (who.clone(), what.clone()))
                .collect();
            d.came = came.len();
            d.stage = DeliverStage::Fitting;
            let effect = |id| Effect::Ask {
                id,
                how: AskKind::Json,
                voice: "judge".into(),
                owner: d.owner,
                operation: "draft_still".into(),
                timeout_ms: STILL_WITHIN_MS,
                system: crate::draft::system(),
                input: crate::draft::input(&d.said, &d.reply, &fresh),
                schema_name: crate::draft::SCHEMA_NAME.into(),
                schema: crate::draft::schema(),
                turn: None,
            };
            return Next::Wait(cx.effect(effect));
        }
        d.looked = came.len();
    }
    chunks(cx, to, d)
}

pub(crate) fn chunks(cx: &mut Cx, to: &Message, d: &mut Deliver) -> Next {
    d.chunks = myriad_agent_rules::channel::split_channel_text(
        &d.messages[d.index],
        text_limit(&to.platform),
    );
    d.chunk = 0;
    send_chunk(cx, to, d)
}

pub(crate) fn send_chunk(cx: &mut Cx, to: &Message, d: &mut Deliver) -> Next {
    if d.chunk >= d.chunks.len() {
        d.index += 1;
        return next_message(cx, to, d, d.since);
    }
    let chunk = &d.chunks[d.chunk];
    let pieces = myriad_agent_rules::mentions::split_mentions(chunk, &d.people)
        .into_iter()
        .map(|piece| match piece {
            myriad_agent_rules::mentions::Piece::Text(text) => Piece::Text { text },
            myriad_agent_rules::mentions::Piece::Mention { id, name } => {
                Piece::Mention { id, name }
            }
        })
        .collect();
    // A line with no message behind it (her saying something first) quotes
    // nothing, and neither do words she said unasked.
    let quote = !d.sent && !d.unquoted && !to.message_id.is_empty();
    d.stage = DeliverStage::Sending;
    let target = to.target();
    Next::Wait(cx.effect(|id| Effect::Send {
        id,
        to: target,
        pieces,
        quote,
    }))
}

pub(crate) fn deliver_step(
    cx: &mut Cx,
    to: &Message,
    d: &mut Deliver,
    answer: Option<Answer>,
) -> Next {
    let now = cx.now();
    match (d.stage, answer) {
        (DeliverStage::TypingMark, Some(Answer::Typed)) => {
            d.stage = DeliverStage::Typing;
            Next::Wait(cx.wake(d.left))
        }
        (DeliverStage::Typing, None) => look_over(cx, to, d),
        (DeliverStage::Fitting, Some(Answer::Asked { raw, .. })) => {
            let send = raw.as_deref().and_then(crate::draft::parse).unwrap_or(true);
            if !send {
                cx.group.held_back = true;
                keep_ledger(cx.group, cx.out);
                return Next::Done(true);
            }
            d.looked = d.came;
            chunks(cx, to, d)
        }
        (DeliverStage::Sending, Some(Answer::Sent { ok })) => {
            if !ok {
                keep_ledger(cx.group, cx.out);
                return Next::Done(false);
            }
            let chunk = d.chunks[d.chunk].clone();
            // How long whoever called her waited for her first words.
            let waited = (!d.sent && to.addressed)
                .then(|| said_ago(cx.group, to, now))
                .flatten()
                .map(|age| age.num_milliseconds() as f64 / 1000.0);
            let typed = crate::talk_shape::typed_by(HER, time(now).timestamp(), &chunk);
            note_said(cx.group, typed, waited, Some(&chunk));
            d.sent = true;
            d.went_out.push(chunk);
            d.chunk += 1;
            send_chunk(cx, to, d)
        }
        _ => {
            keep_ledger(cx.group, cx.out);
            Next::Done(false)
        }
    }
}
