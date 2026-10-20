//! One answer: the gates, who wrote the line, what she reads, the turn that
//! says it, and what went out.

use super::*;

pub(crate) fn answer_start(cx: &mut Cx, answering: &mut Answering) -> Next {
    let now = cx.now();
    // Not on this end, she answers nobody there.
    if cx.at.her.away {
        return Next::Done(false);
    }
    if stopped_answering(cx.group, &answering.message, now)
        || read_already(cx.group, &answering.message)
    {
        return Next::Done(false);
    }
    if muted_now(cx.group, now) {
        return Next::Done(false);
    }
    let inbound_id = format!(
        "group:{}:{}",
        answering.message.chat, answering.message.message_id
    );
    answering.stage = AnswerStage::Claiming;
    Next::Wait(cx.effect(|id| Effect::Claim { id, inbound_id }))
}

pub(crate) fn first_start(cx: &mut Cx, answering: &mut Answering, owner: Option<i32>) -> Next {
    let now = cx.now();
    let Some(seen) = cx.group.reach.clone() else {
        return Next::Done(false);
    };
    let Some(owner) = owner else {
        return Next::Done(false);
    };
    answering.owner = Some(owner);
    answering.message = Message {
        message_id: String::new(),
        from: String::new(),
        display_name: String::new(),
        text: crate::sharing::NOBODY_SAID.to_string(),
        addressed: false,
        reply_to: None,
        images: Vec::new(),
        ..seen
    };
    let inbound_id = format!(
        "group:{}:first:{}",
        answering.message.chat,
        time(now).timestamp_millis()
    );
    answering.stage = AnswerStage::Claiming;
    Next::Wait(cx.effect(|id| Effect::Claim { id, inbound_id }))
}

pub(crate) fn answer_step(cx: &mut Cx, a: &mut Answering, answer: Option<Answer>) -> Next {
    let now = cx.now();
    match (a.stage, answer) {
        (AnswerStage::Claiming, Some(Answer::Claimed { ok })) => {
            if !ok {
                return Next::Done(false);
            }
            if a.first {
                // Her own turn on the owner's budget: no one's line to read.
                a.began = now;
                a.stage = AnswerStage::Preparing;
                let user_id = a.owner.unwrap_or_default();
                return Next::Wait(cx.effect(|id| Effect::Prepare { id, user_id }));
            }
            let pictures = unseen(cx.group);
            if pictures.is_empty() {
                return identify(cx, a);
            }
            a.stage = AnswerStage::Looking;
            Next::Wait(cx.effect(|id| Effect::Look { id, pictures }))
        }
        (AnswerStage::Looking, Some(Answer::Looked { seen })) => {
            apply_seen(cx, seen);
            identify(cx, a)
        }
        (AnswerStage::Identifying, Some(Answer::Identified { who, owner })) => {
            a.site_owner = owner;
            match who {
                Who::Member { user_id, bound } => {
                    if !bound {
                        return Next::Done(false);
                    }
                    a.user_id = Some(user_id);
                    a.owner = Some(user_id);
                    a.began = now;
                    a.stage = AnswerStage::Preparing;
                    Next::Wait(cx.effect(|id| Effect::Prepare { id, user_id }))
                }
                Who::Stranger => {
                    let today = cx.at.her.today.clone();
                    if !count_today(
                        &mut cx.group.stranger_replies,
                        &today,
                        STRANGER_REPLIES_PER_DAY,
                    ) {
                        return Next::Done(false);
                    }
                    let Some(owner) = owner else {
                        return Next::Done(false);
                    };
                    a.owner = Some(owner);
                    // Her reading of the talk is part of answering: the
                    // first message is typed while she reads, as a
                    // member's is.
                    a.began = now;
                    a.stranger = Some(Stranger {
                        who: format!("{}:{}", a.message.platform, a.message.from),
                        name: a.message.display_name.chars().take(40).collect(),
                    });
                    let read = reading(cx.group, &a.message, now);
                    a.transcript = read.transcript;
                    a.said = read.said;
                    a.room = room(cx.group, now);
                    a.noted = noted(cx.group);
                    a.stage = AnswerStage::StrangerLedger;
                    Next::Wait(cx.effect(|id| Effect::ReadLedger { id }))
                }
                Who::Unknown => Next::Done(false),
            }
        }
        (AnswerStage::Preparing, Some(Answer::Prepared { ok })) => {
            if !ok {
                return Next::Done(false);
            }
            if a.first {
                a.transcript = transcript(cx.group, None, now);
                a.said = said_now(cx.group, &a.message);
                a.room = room(cx.group, now);
                a.noted = noted(cx.group);
                a.stage = AnswerStage::LedgerForTurn;
                return Next::Wait(cx.effect(|id| Effect::ReadLedger { id }));
            }
            let read = reading(cx.group, &a.message, now);
            a.transcript = read.transcript;
            a.said = read.said;
            a.stage = AnswerStage::Sensing;
            match start_sensing(cx, a) {
                Sensed::Wait(awaiting) => Next::Wait(awaiting),
                Sensed::Done(section) => sensed_member(cx, a, section),
            }
        }
        (AnswerStage::Sensing, answer) => match sensing_step(cx, a, answer) {
            Sensed::Wait(awaiting) => Next::Wait(awaiting),
            Sensed::Done(section) => sensed_member(cx, a, section),
        },
        (AnswerStage::LedgerForTurn, Some(Answer::LedgerRead { stored })) => {
            let differs = differs(cx.group, &a.noted, stored.as_ref());
            let late = if a.first {
                None
            } else {
                seen_late(cx.group, &a.message, now)
            };
            a.stage = AnswerStage::Turning;
            let (member, next) = member_start(cx, a, differs, late);
            if !matches!(next, MemberNext::Wait(_)) {
                a.turn = member.turn.clone();
            }
            a.member = Some(Box::new(member));
            member_replied(cx, a, next)
        }
        (AnswerStage::Turning, answer) => {
            let Some(mut member) = a.member.take() else {
                return Next::Done(false);
            };
            let next = member_step(cx, &mut member, answer);
            if matches!(next, MemberNext::Wait(_)) {
                a.member = Some(member);
            } else {
                a.turn = member.turn.clone();
            }
            member_replied(cx, a, next)
        }
        (AnswerStage::StrangerLedger, Some(Answer::LedgerRead { stored })) => {
            let differs = differs(cx.group, &a.noted, stored.as_ref());
            a.sections = [
                a.room
                    .as_ref()
                    .map(|room| crate::talk_shape::describe(room, "How people type here")),
                differs,
            ]
            .into_iter()
            .flatten()
            .collect();
            a.stage = AnswerStage::StrangerSensing;
            match start_sensing(cx, a) {
                Sensed::Wait(awaiting) => Next::Wait(awaiting),
                Sensed::Done(section) => sensed_stranger(cx, a, section),
            }
        }
        (AnswerStage::StrangerSensing, answer) => match sensing_step(cx, a, answer) {
            Sensed::Wait(awaiting) => Next::Wait(awaiting),
            Sensed::Done(section) => sensed_stranger(cx, a, section),
        },
        (AnswerStage::StrangerTyping, Some(Answer::Typed)) => {
            a.stage = AnswerStage::StrangerTurning;
            let (replying, replied) = reply_start(cx, a);
            a.replying = Some(Box::new(replying));
            stranger_replied(cx, a, replied)
        }
        (AnswerStage::StrangerTurning, answer) => {
            let Some(mut replying) = a.replying.take() else {
                return Next::Done(false);
            };
            let replied = reply_step(cx, &mut replying, answer);
            if matches!(replied, Replied::Wait(_)) {
                a.replying = Some(replying);
            }
            stranger_replied(cx, a, replied)
        }
        (AnswerStage::Restoring, Some(Answer::Kept { restore })) => {
            if let Some(restore) = restore {
                restored(cx, restore.lines, restore.spoke_up, restore.table);
            }
            let went_out = a.went_out.clone().unwrap_or_default();
            after_saying(cx, a, went_out)
        }
        (AnswerStage::Delivering, answer) => {
            let Some(mut deliver) = a.deliver.take() else {
                return Next::Done(false);
            };
            match deliver_step(cx, &a.message, &mut deliver, answer) {
                Next::Wait(awaiting) => {
                    a.deliver = Some(deliver);
                    Next::Wait(awaiting)
                }
                Next::Done(_) => after_saying(cx, a, deliver.went_out.join("\n")),
            }
        }
        _ => Next::Done(false),
    }
}

pub(crate) fn identify(cx: &mut Cx, a: &mut Answering) -> Next {
    a.stage = AnswerStage::Identifying;
    let from = a.message.from.clone();
    Next::Wait(cx.effect(|id| Effect::Identify { id, from }))
}

/// What was noted for the ledger and not yet written, as she asks for it.
pub(crate) fn noted(group: &Group) -> Pieces {
    Pieces {
        theirs: group.ledger_theirs.clone(),
        hers: group.ledger_hers.clone(),
    }
}

/// Her reply without a （回复 …） mark she copied from the transcript.
pub(crate) fn without_reply_mark(reply: &str) -> String {
    let trimmed = reply.trim_start();
    if let Some(rest) = trimmed.strip_prefix("（回复")
        && let Some(end) = rest
            .find('）')
            .filter(|end| rest[..*end].chars().count() <= 200)
    {
        return rest[end + '）'.len_utf8()..].trim_start().to_string();
    }
    reply.to_string()
}

/// Her words, if any, then the sticker she chose.
pub(crate) fn say(cx: &mut Cx, a: &mut Answering) -> Next {
    if a.reply.trim().is_empty() {
        return after_saying(cx, a, String::new());
    }
    let now = cx.now();
    let people = people(cx.group, now);
    let room = room(cx.group, now);
    let lines = myriad_agent_rules::channel::as_messages(&a.reply);
    let most = crate::talk_shape::messages_this_turn(lines.len(), cx.at.rolls.roll());
    let messages = crate::talk_shape::goes_out_as(&lines, most, room.as_ref());
    let said = if a.message.text == crate::sharing::NOBODY_SAID {
        String::new()
    } else {
        a.message.text.clone()
    };
    let mut deliver = Deliver {
        people,
        messages,
        index: 0,
        chunks: Vec::new(),
        chunk: 0,
        sent: false,
        went_out: Vec::new(),
        since: a.began,
        looked: 0,
        said,
        reply: a.reply.clone(),
        owner: a.owner.unwrap_or_default(),
        came: 0,
        left: 0.0,
        stage: DeliverStage::TypingMark,
    };
    a.stage = AnswerStage::Delivering;
    match next_message(cx, &a.message, &mut deliver, a.began) {
        Next::Wait(awaiting) => {
            a.deliver = Some(deliver);
            Next::Wait(awaiting)
        }
        Next::Done(_) => after_saying(cx, a, deliver.went_out.join("\n")),
    }
}

/// What went out of her words: kept as hers; then the sticker.
pub(crate) fn after_saying(cx: &mut Cx, a: &mut Answering, went_out: String) -> Next {
    let now = cx.now();
    let mut sent = false;
    if !a.reply.trim().is_empty() {
        sent = !went_out.is_empty();
        // What was kept before a restart could not be read when the line
        // came: read again before her words are kept, so they are kept
        // under it (and do not overwrite it).
        if sent && !cx.group.restored && a.went_out.is_none() {
            a.went_out = Some(went_out);
            a.stage = AnswerStage::Restoring;
            return Next::Wait(cx.effect(|id| Effect::ReadKept { id }));
        }
        if sent {
            record_hers(cx.group, &went_out, now, cx.out);
        }
    }
    // Held back, the sticker she chose for it is too.
    let held = !a.reply.trim().is_empty() && !sent;
    let mut sticker = false;
    if let Some(chosen) = a.sticker.take().filter(|_| !held) {
        cx.out.effects.push(Effect::Sticker {
            to: a.message.target(),
            chosen,
        });
        sent = true;
        sticker = true;
    }
    let words = if a.reply.trim().is_empty() || held {
        String::new()
    } else {
        went_out
    };
    keep_turn(cx, a, words, sticker);
    if sent && !a.first {
        answered(cx.group, &a.message, now);
        if let (Some(stranger), Some(owner)) = (&a.stranger, a.owner) {
            cx.out.effects.push(Effect::StrangerAfter {
                owner,
                stranger: stranger.clone(),
                said: a.said.clone(),
                reply: a.reply.clone(),
                message_id: a.message.message_id.clone(),
            });
        }
    }
    Next::Done(sent)
}

/// The member's turn closed with this answer, kept with what went out of
/// it: nothing, when nothing did.
fn keep_turn(cx: &mut Cx, a: &mut Answering, went_out: String, sticker: bool) {
    if let Some(turn) = a.turn.take() {
        cx.out.effects.push(Effect::TurnKept {
            turn,
            went_out,
            sticker,
        });
    }
}

/// Begin reading the talk before answering it (see `sense`).
fn start_sensing(cx: &mut Cx, a: &mut Answering) -> Sensed {
    let conversation = conversation(&a.transcript, TRANSCRIPT_LINES);
    let (sensing, sensed) = sense_start(
        cx,
        a.message.venue(),
        conversation,
        a.said.clone(),
        a.site_owner,
    );
    a.unread = unread(&sensing, &sensed);
    a.sensing = Some(Box::new(sensing));
    sensed
}

/// Whether a reading she meant to do came to nothing.
fn unread(sensing: &Sensing, sensed: &Sensed) -> bool {
    matches!(sensed, Sensed::Done(_)) && sensing.owner.is_some() && sensing.sense.is_none()
}

fn sensing_step(cx: &mut Cx, a: &mut Answering, answer: Option<Answer>) -> Sensed {
    let Some(mut sensing) = a.sensing.take() else {
        return Sensed::Done(None);
    };
    let sensed = sense_step(cx, &mut sensing, answer);
    a.unread = unread(&sensing, &sensed);
    if matches!(sensed, Sensed::Wait(_)) {
        a.sensing = Some(sensing);
    }
    sensed
}

/// Nobody asked her and she could not make sense of the talk: she keeps
/// quiet rather than chime in on what she has not read (an answer unread
/// was most of what went wrong when she joined in).
fn unread_chime(a: &Answering) -> bool {
    a.chime.is_some() && a.unread
}

/// Her reading done, a member's turn goes on: how people type there and
/// how her lines differ, from the ledger.
fn sensed_member(cx: &mut Cx, a: &mut Answering, section: Option<String>) -> Next {
    if unread_chime(a) {
        return Next::Done(false);
    }
    let now = cx.now();
    a.making_sense = section;
    a.room = room(cx.group, now);
    a.noted = noted(cx.group);
    a.stage = AnswerStage::LedgerForTurn;
    Next::Wait(cx.effect(|id| Effect::ReadLedger { id }))
}

/// Her reading done, the answer to someone from outside goes on: typing.
fn sensed_stranger(cx: &mut Cx, a: &mut Answering, section: Option<String>) -> Next {
    if unread_chime(a) {
        return Next::Done(false);
    }
    a.sections.extend(section);
    a.stage = AnswerStage::StrangerTyping;
    let to = a.message.target();
    Next::Wait(cx.effect(|id| Effect::Typing { id, to }))
}

/// Her answer to someone from outside, as far as it got.
fn stranger_replied(cx: &mut Cx, a: &mut Answering, replied: Replied) -> Next {
    match replied {
        Replied::Wait(awaiting) => Next::Wait(awaiting),
        Replied::Done(None) => Next::Done(false),
        Replied::Done(Some((text, sticker))) => {
            let text = text.trim().to_string();
            if text.is_empty() && sticker.is_none() {
                return Next::Done(false);
            }
            a.reply = without_reply_mark(&text);
            a.sticker = sticker;
            say(cx, a)
        }
    }
}

/// Her answer to a member, as far as it got.
fn member_replied(cx: &mut Cx, a: &mut Answering, next: MemberNext) -> Next {
    match next {
        MemberNext::Wait(awaiting) => Next::Wait(awaiting),
        MemberNext::Done(said, bound) => {
            let (text, sticker) = said.unwrap_or_default();
            let text = text.trim().to_string();
            if text.is_empty() && sticker.is_none() {
                keep_turn(cx, a, String::new(), false);
                return Next::Done(false);
            }
            // Unpaired or switched off while she was thinking.
            if !bound {
                keep_turn(cx, a, String::new(), false);
                return Next::Done(false);
            }
            a.reply = without_reply_mark(&text);
            a.sticker = sticker;
            say(cx, a)
        }
    }
}
