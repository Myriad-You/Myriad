//! What she is in the middle of in a group, step by step: taking up a line
//! that called her, glancing at talk that did not, saying something first,
//! and the answer and its going out that each of them may come to. Each is
//! an errand that waits for one thing at a time (a timer, or what it asked
//! the end); everything else happens at once, as it would between two
//! awaits.

use serde::{Deserialize, Serialize};

use super::ledger::{differs, keep_ledger, note_said};
use super::lines::*;
use super::*;
use crate::joining::Why;

mod answer;
mod deliver;
mod errands;
mod first;
mod glance;
mod heard;
mod learn;
mod member;
pub mod seats;
mod sense;
mod state;
mod stranger;
mod table;

pub(crate) use answer::*;
pub(crate) use deliver::*;
pub(crate) use errands::*;
pub(crate) use first::*;
pub(crate) use glance::*;
pub(crate) use heard::*;
pub(crate) use learn::*;
pub(crate) use member::*;
pub(crate) use seats::*;
pub(crate) use sense::*;
pub(crate) use state::*;
pub(crate) use stranger::*;
pub(crate) use table::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Errand {
    pub id: String,
    /// The timer key or the id of what it asked: what it waits for.
    pub awaiting: String,
    pub kind: Kind,
    pub doing: Doing,
}

/// What the errand is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Kind {
    /// A line that called her: seen, taken up, answered, then whoever
    /// waited meanwhile.
    TakeUp {
        #[serde(default)]
        waiter: Option<String>,
        message: Message,
        /// Whether a reply held back may still be said again.
        again: bool,
    },
    /// A glance at talk that did not call her, settled on line `id`.
    Look {
        id: String,
        #[serde(default)]
        message: Option<Message>,
        #[serde(default)]
        why: Option<Why>,
        /// The answer it is on is the one she chose to say, not one that
        /// waited.
        #[serde(default)]
        spoke: bool,
    },
    /// Five minutes on, whether anyone took up her speaking up.
    SpokeUp { at: i64, line: String },
    /// Looking up words she is not sure of, beside whatever else she does.
    Learn,
    /// Showing she is typing while a member's turn of hers runs.
    Typer { token: String, to: Target },
    /// The game with seats at the group's table: its timers and whispers.
    Seats,
    /// Saying something first.
    First {
        waiter: String,
        reason: String,
        #[serde(default)]
        owner: Option<i32>,
        #[serde(default)]
        spoke: Option<bool>,
    },
}

/// Where the errand is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "at")]
pub enum Doing {
    /// Until she sees the line.
    Seeing,
    /// The group just had her reply: a moment before the turn.
    Beginning,
    Answering(Box<Answering>),
    /// Done with one: a pause before the next that waited.
    Pausing,
    /// Before the next that waited, still resting.
    Resting,
    Glancing,
    Settling,
    Judging(Box<Judging>),
    Reacting {
        emoji: String,
    },
    /// Her reaction is on the line; before it is kept as hers, what was
    /// kept before a restart is read back.
    ReactedRestoring {
        text: String,
    },
    Learning(Box<Learning>),
    Seats,
    Timing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judging {
    pub message: Message,
    pub stage: JudgeStage,
    #[serde(default)]
    pub owner: Option<i32>,
    /// The recent talk she judges, hers as `you：…`.
    #[serde(default)]
    pub conversation: Vec<String>,
    #[serde(default)]
    pub here: Here,
    /// What she has that the talk touches, so far: (kind, text).
    #[serde(default)]
    pub had: Vec<(String, String)>,
    /// What of her own time she would tell someone, kept for last.
    #[serde(default)]
    pub would_tell: Vec<String>,
    /// What she is doing now.
    #[serde(default)]
    pub own_time: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JudgeStage {
    Identifying,
    Looking,
    /// Her views the talk touches.
    Views,
    /// What she did, would tell, is doing.
    Life,
    /// What she heard elsewhere.
    Heard,
    /// The words and memes she knows.
    Memes,
    Soul,
    Judging,
}

/// One answer: from the gates to what went out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answering {
    pub message: Message,
    #[serde(default)]
    pub chime: Option<String>,
    /// Saying something first: nobody's line behind it.
    #[serde(default)]
    pub first: bool,
    pub stage: AnswerStage,
    #[serde(default)]
    pub user_id: Option<i32>,
    #[serde(default)]
    pub owner: Option<i32>,
    #[serde(default)]
    pub stranger: Option<Stranger>,
    #[serde(default)]
    pub began: i64,
    #[serde(default)]
    pub transcript: Vec<Spoken>,
    #[serde(default)]
    pub said: String,
    #[serde(default)]
    pub making_sense: Option<String>,
    #[serde(default)]
    pub room: Option<crate::talk_shape::Shape>,
    #[serde(default)]
    pub noted: Pieces,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub sticker: Option<Value>,
    #[serde(default)]
    pub deliver: Option<Deliver>,
    /// What went out, while she reads back what was kept before a restart
    /// (it could not be read when the line came) before keeping her words.
    #[serde(default)]
    pub went_out: Option<String>,
    /// The site's owner, who hosts her there.
    #[serde(default)]
    pub site_owner: Option<i32>,
    /// Her reading of the talk, while she does it.
    #[serde(default)]
    pub sensing: Option<Box<Sensing>>,
    /// She meant to read the talk first and could not.
    #[serde(default)]
    pub unread: bool,
    /// Her answer to someone from outside, while she puts it together.
    #[serde(default)]
    pub replying: Option<Box<Replying>>,
    /// Her answer to a member, while she puts it together.
    #[serde(default)]
    pub member: Option<Box<MemberReply>>,
    /// The member's turn her answer closed, kept with what goes out of it.
    #[serde(default)]
    pub turn: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnswerStage {
    Claiming,
    Looking,
    Identifying,
    Preparing,
    Sensing,
    LedgerForTurn,
    Turning,
    StrangerLedger,
    StrangerSensing,
    StrangerTyping,
    StrangerTurning,
    Delivering,
    Restoring,
}

/// Her reply going out, message by message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deliver {
    pub people: Vec<(String, String)>,
    pub messages: Vec<String>,
    pub index: usize,
    #[serde(default)]
    pub chunks: Vec<String>,
    #[serde(default)]
    pub chunk: usize,
    pub sent: bool,
    pub went_out: Vec<String>,
    pub since: i64,
    pub looked: usize,
    /// What was said to her (empty when nobody said anything).
    pub said: String,
    /// Her whole reply, as written, and who the look at it is billed to.
    pub reply: String,
    pub owner: i32,
    #[serde(default)]
    pub came: usize,
    /// How long she still types, once the mark is shown.
    #[serde(default)]
    pub left: f64,
    /// Her words quote no line: she spoke up unasked, and the latest line is
    /// not always the one her words are about (whom they are for, she @-s).
    #[serde(default)]
    pub unquoted: bool,
    pub stage: DeliverStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliverStage {
    TypingMark,
    Typing,
    Fitting,
    Sending,
}

/// What a piece of an errand came to.
pub(crate) enum Next {
    /// Waiting on `awaiting`.
    Wait(String),
    Done(bool),
}

pub(crate) struct Cx<'a> {
    group: &'a mut Group,
    at: &'a mut At,
    out: &'a mut Outcome,
}

impl Cx<'_> {
    fn now(&self) -> i64 {
        self.at.now
    }

    fn id(&mut self) -> String {
        self.group.next += 1;
        format!("g{}", self.group.next)
    }

    fn wake(&mut self, after: f64) -> String {
        let key = self.id();
        self.out.effects.push(Effect::Wake {
            key: key.clone(),
            after,
        });
        key
    }

    fn effect(&mut self, make: impl FnOnce(String) -> Effect) -> String {
        let id = self.id();
        self.out.effects.push(make(id.clone()));
        id
    }

    fn settle(&mut self, waiter: &Option<String>, value: bool) {
        if let Some(waiter) = waiter {
            self.out.effects.push(Effect::Settled {
                waiter: waiter.clone(),
                value,
            });
        }
    }
}

pub(crate) fn step(group: &mut Group, at: &mut At, event: Event, out: &mut Outcome) {
    let table = group.seating.clone();
    step_on(group, at, event, out);
    // The table changed: kept as it now stands, to outlive a restart.
    if group.seating != table {
        out.effects.push(Effect::KeepTable {
            table: group
                .seating
                .as_ref()
                .and_then(|seating| serde_json::to_value(seating).ok()),
        });
    }
}

fn step_on(group: &mut Group, at: &mut At, event: Event, out: &mut Outcome) {
    let mut cx = Cx { group, at, out };
    match event {
        Event::Restored {
            lines,
            spoke_up,
            table,
        } => restored(&mut cx, lines, spoke_up, table),
        Event::Reached { message, seen_at } => {
            if cx.group.reach.is_none() {
                cx.group.reach = Some(message);
                cx.group.seen_before = Some(seen_at);
            }
        }
        Event::Heard { message, restore } => {
            let fresh = record(&mut cx, &message, restore);
            cx.out.fresh = Some(fresh);
        }
        Event::Called { message, waiter } => handle(&mut cx, message, waiter),
        Event::Noticed { message } => notice(&mut cx, message),
        Event::Muted {
            by,
            mute,
            everyone,
            restore,
        } => muted(&mut cx, &by, mute, everyone, restore),
        Event::Resumed {
            why,
            past,
            up_since,
        } => resumed(&mut cx, why, past, up_since),
        Event::SpeakFirst {
            reason,
            owner,
            waiter,
        } => speak_first(&mut cx, reason, owner, waiter),
        Event::StickerSent { meaning, restore } => {
            if let Some(restore) = restore {
                restored(&mut cx, restore.lines, restore.spoke_up, restore.table);
            }
            let now = cx.now();
            record_hers(cx.group, &format!("（表情包：{meaning}）"), now, cx.out);
        }
        Event::Whispered { who, text, wait } => {
            let taken = whispered(&mut cx, &who, &text, wait);
            cx.out.taken = Some(taken);
        }
        Event::Woke { key } => resume(&mut cx, &key, None),
        Event::Done { id, answer } => resume(&mut cx, &id, Some(answer)),
    }
}
