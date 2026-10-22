//! Errands moving on: a timer or an answer comes, and the errand goes on
//! to its next wait; one answer done, the next that waited.

use super::*;

pub(crate) fn resume(cx: &mut Cx, key: &str, answer: Option<Answer>) {
    let Some(index) = cx
        .group
        .errands
        .iter()
        .position(|errand| errand.awaiting == key)
    else {
        return;
    };
    let errand = cx.group.errands.remove(index);
    advance(cx, errand, answer);
}

/// Keep `errand` waiting on what `next` says, or let it go.
pub(crate) fn park_errand(cx: &mut Cx, mut errand: Errand, awaiting: String) {
    errand.awaiting = awaiting;
    cx.group.errands.push(errand);
}

pub(crate) fn advance(cx: &mut Cx, mut errand: Errand, answer: Option<Answer>) {
    let doing = std::mem::replace(&mut errand.doing, Doing::Timing);
    match doing {
        Doing::Seeing => {
            // She sees the line: called there now.
            cx.group.called = Some(cx.now());
            take_up_begin(cx, errand);
        }
        Doing::Beginning => take_up_begin(cx, errand),
        Doing::Answering(mut answering) => match answer_step(cx, &mut answering, answer) {
            Next::Wait(awaiting) => {
                errand.doing = Doing::Answering(answering);
                park_errand(cx, errand, awaiting);
            }
            Next::Done(replied) => answered_then(cx, errand, replied),
        },
        Doing::Pausing | Doing::Resting => finish_begin(cx, errand),
        Doing::Glancing => {
            let now = cx.now();
            let Kind::Look { id, .. } = &mut errand.kind else {
                return;
            };
            // A glance takes in whatever is latest by then.
            if cx.group.glance_at.is_some_and(|at| at <= now) {
                cx.group.glance_at = None;
                match cx
                    .group
                    .pending
                    .as_ref()
                    .map(|line| line.message_id.clone())
                {
                    Some(latest) => *id = latest,
                    None => return,
                }
            }
            look_loop(cx, errand);
        }
        Doing::Settling => look_loop(cx, errand),
        Doing::Judging(mut judging) => match judge_step(cx, &mut judging, answer) {
            Ok(awaiting) => {
                errand.doing = Doing::Judging(judging);
                park_errand(cx, errand, awaiting);
            }
            Err(decided) => judged(cx, errand, decided),
        },
        Doing::Reacting { emoji } => {
            if let (
                Some(Answer::Reacted { ok: true }),
                Kind::Look {
                    message: Some(message),
                    ..
                },
            ) = (answer, &errand.kind)
            {
                let text = format!("（给 {} 那句贴了 {emoji}）", message.display_name);
                // Kept under what was kept before a restart, read again if
                // it could not be read when the line came.
                if !cx.group.restored {
                    let awaiting = cx.effect(|id| Effect::ReadKept { id });
                    errand.doing = Doing::ReactedRestoring { text };
                    park_errand(cx, errand, awaiting);
                    return;
                }
                let now = cx.now();
                record_hers(cx.group, &text, now, cx.out);
            }
            chain_end(cx, errand);
        }
        Doing::ReactedRestoring { text } => {
            if let Some(Answer::Kept {
                restore: Some(restore),
            }) = answer
            {
                restored(cx, restore.lines, restore.spoke_up, restore.table);
            }
            let now = cx.now();
            record_hers(cx.group, &text, now, cx.out);
            chain_end(cx, errand);
        }
        Doing::Learning(mut learning) => {
            if let Some(awaiting) = learn_step(cx, &mut learning, answer) {
                errand.doing = Doing::Learning(learning);
                park_errand(cx, errand, awaiting);
            }
        }
        Doing::Timing if matches!(errand.kind, Kind::Typer { .. }) => typing_tick(cx, errand),
        Doing::Seats => seats_step(cx, errand, answer),
        Doing::Timing => {
            if let Kind::SpokeUp { at, line } = errand.kind {
                let taken = cx
                    .group
                    .spoke_up
                    .iter()
                    .find(|one| one.at == at)
                    .is_some_and(|one| one.taken);
                cx.out.effects.push(Effect::SpokeUp { line, taken });
            }
        }
    }
}

pub(crate) fn take_up_begin(cx: &mut Cx, mut errand: Errand) {
    let now = cx.now();
    let Kind::TakeUp {
        message, waiter, ..
    } = &errand.kind
    else {
        return;
    };
    match begin(cx.group, now) {
        Ok(()) => {
            let message = message.clone();
            cx.group.held_back = false;
            start_answer(cx, errand, message, None, false);
        }
        Err(Some(rest)) => {
            let key = cx.wake(rest);
            errand.doing = Doing::Beginning;
            park_errand(cx, errand, key);
        }
        Err(None) => {
            let (message, waiter) = (message.clone(), waiter.clone());
            park(cx.group, message);
            cx.settle(&waiter, true);
        }
    }
}

pub(crate) fn start_answer(
    cx: &mut Cx,
    mut errand: Errand,
    message: Message,
    chime: Option<String>,
    first: bool,
) {
    let mut answering = Box::new(Answering {
        message,
        chime,
        first,
        stage: AnswerStage::Claiming,
        user_id: None,
        owner: None,
        stranger: None,
        began: 0,
        transcript: Vec::new(),
        said: String::new(),
        making_sense: None,
        room: None,
        noted: Pieces::default(),
        sections: Vec::new(),
        reply: String::new(),
        sticker: None,
        deliver: None,
        went_out: None,
        site_owner: None,
        sensing: None,
        unread: false,
        replying: None,
        member: None,
        turn: None,
    });
    let owner = match &errand.kind {
        Kind::First { owner, .. } => *owner,
        _ => None,
    };
    let next = if first {
        first_start(cx, &mut answering, owner)
    } else {
        answer_start(cx, &mut answering)
    };
    match next {
        Next::Wait(awaiting) => {
            errand.doing = Doing::Answering(answering);
            park_errand(cx, errand, awaiting);
        }
        Next::Done(replied) => answered_then(cx, errand, replied),
    }
}

/// After one answer: say it again from the newest line if it was held
/// back, then whoever waited.
pub(crate) fn answered_then(cx: &mut Cx, mut errand: Errand, replied: bool) {
    match &mut errand.kind {
        Kind::TakeUp { message, again, .. } => {
            let held = std::mem::take(&mut cx.group.held_back);
            if held
                && *again
                && let Some(newest) = newest_theirs(cx.group, message)
            {
                *again = false;
                *message = newest.clone();
                cx.group.held_back = false;
                start_answer(cx, errand, newest, None, false);
                return;
            }
            *again = true;
        }
        Kind::Look { why, spoke, .. } => {
            if std::mem::take(spoke) && replied && why.is_some_and(|why| why != Why::Answer) {
                spoke_up_now(cx);
            }
        }
        Kind::First { spoke, .. } => {
            if spoke.is_none() {
                *spoke = Some(replied);
                if replied {
                    spoke_up_now(cx);
                }
            }
        }
        Kind::SpokeUp { .. } | Kind::Learn | Kind::Typer { .. } | Kind::Seats => return,
    }
    finish(cx, errand, replied);
}

/// End the turn; if a line waited meanwhile, take the turn again for it
/// after the pause.
pub(crate) fn finish(cx: &mut Cx, mut errand: Errand, replied: bool) {
    let now = cx.now();
    end_turn(cx.group, replied, now);
    if cx.group.waiting.is_empty() {
        chain_end(cx, errand);
        return;
    }
    if replied {
        let key = cx.wake(GROUP_PAUSE_SECONDS);
        errand.doing = Doing::Pausing;
        park_errand(cx, errand, key);
        return;
    }
    finish_begin(cx, errand);
}

pub(crate) fn finish_begin(cx: &mut Cx, mut errand: Errand) {
    let now = cx.now();
    match begin(cx.group, now) {
        Ok(()) => {}
        Err(Some(rest)) => {
            let key = cx.wake(rest);
            errand.doing = Doing::Resting;
            park_errand(cx, errand, key);
            return;
        }
        // Someone else took the turn; they will find the line waiting.
        Err(None) => {
            chain_end(cx, errand);
            return;
        }
    }
    let Some(next) = cx.group.waiting.pop_front() else {
        end_turn(cx.group, false, now);
        chain_end(cx, errand);
        return;
    };
    if let Kind::TakeUp { message, .. } = &mut errand.kind {
        *message = next.clone();
        cx.group.held_back = false;
    }
    start_answer(cx, errand, next, None, false);
}

pub(crate) fn chain_end(cx: &mut Cx, mut errand: Errand) {
    match &mut errand.kind {
        Kind::TakeUp { waiter, .. } => {
            let waiter = waiter.clone();
            cx.settle(&waiter, true);
        }
        Kind::Look {
            id, message, why, ..
        } => {
            *message = None;
            *why = None;
            if let Some(newer) = cx
                .group
                .pending
                .as_ref()
                .map(|line| line.message_id.clone())
            {
                *id = newer;
                look_loop(cx, errand);
            }
        }
        Kind::First { waiter, spoke, .. } => {
            let (waiter, spoke) = (Some(waiter.clone()), spoke.unwrap_or(false));
            cx.settle(&waiter, spoke);
        }
        Kind::SpokeUp { .. } | Kind::Learn | Kind::Typer { .. } | Kind::Seats => {}
    }
}
