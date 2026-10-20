//! Her mind in a group chat, as one deterministic machine (D2): every end
//! that hosts her runs this, and gives it only what came (a line, an
//! answer, a timer) with the time, chance and how she is; it says what to do
//! (a timer to set, a model to ask, a message to send, a record to keep).
//! Nothing here waits, reads a clock, rolls dice or touches a store; the end
//! does those and comes back with what happened (see
//! `shared/group_mind_contract.json`).
//!
//! What is here: when she sees a line and takes it up, one turn at a time
//! with those waiting answered in order; glancing at talk that does not
//! call her and the judgment of whether to speak up; reading the group as it
//! is when she gets to a line; how her reply goes out (typed message by
//! message, kept back when the talk moved on, stopped when she is called);
//! the group's ledger of how it types; mutes; not answering one who never
//! stops; and taking up what called her while she was away or before a
//! restart; what she makes of the talk, and whether she speaks up, from what
//! she has that the talk touches (her views, what she heard, the words she
//! knows, read as rows; what she did, would tell and is doing, as facts her
//! host's life gives); looking up the words she is not sure of; her whole
//! answer to someone from outside, put together from what is read and
//! asked of her here; her answer to a member too, the group's own part put
//! together here, what she knows of them and how she is given whole by the
//! end that opens and closes the turn as one of theirs (D2c-8 A, D2c-9 A);
//! the game at the group's table, as a game she hosts (`crate::hosting`):
//! read back, a line judged, one started, one over or left put away and
//! remembered; and a game with seats there, she its referee: sign-up in the
//! group, each seat's own part whispered to it alone, moves whispered back.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use myriad_agent_rules::channel::{GroupImage, QuotedLine};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contrast::Counts;
use crate::seeing::Seen;
use crate::talk_shape::Typed;

mod flows;
pub use flows::seats::{Asking, SeatStage, Seating};
mod ledger;
mod lines;
mod rolls;
#[cfg(test)]
mod tests;

pub use ledger::{Ledger, ledger_who, merge_ledger};
pub use lines::{Line, kept_as_heard, people, transcript};
pub use rolls::Rolls;

/// The version of [`Group`]; one not this is read as a group she has
/// nothing in mind of.
pub const VERSION: u32 = 1;

/// Lines of a group she keeps in mind, and for how long. In a busy group
/// thirty lines was a quarter of an hour: what someone brought up twenty
/// minutes before was gone when they came back to it.
pub const TRANSCRIPT_LINES: usize = 60;
pub const TRANSCRIPT_FOR_SECONDS: i64 = 6 * 3600;
pub const MAX_LINE_CHARS: usize = 500;
/// How long the name she shows as in a group may be.
pub const MAX_SHOWN_AS_CHARS: usize = 40;
/// Lines that spoke to her while she was busy, answered in order after.
pub const WAITING_LINES: usize = 30;
/// Between two of her replies in the same group.
pub const GROUP_PAUSE_SECONDS: f64 = 5.0;
/// Lines that did not call her by name, in talk she is in: she looks once
/// the talk pauses this long, and in talk that never pauses, at least this
/// often.
pub const SETTLE_SECONDS: f64 = 5.0;
pub const MAX_WAIT_SECONDS: f64 = 30.0;
/// She is in a group's talk while she spoke there, or was called there,
/// this recently; otherwise she glances at it now and then, when she is
/// free: this many seconds after a line, give or take, and while she is in
/// the middle of something of her own, once it is done (waiting at most so
/// long for it).
pub const IN_TALK_SECONDS: f64 = 10.0 * 60.0;
pub const GLANCE_AFTER_SECONDS: std::ops::RangeInclusive<u64> = 30..=180;
pub const LONGEST_BUSY_SECONDS: f64 = 30.0 * 60.0;
/// Talk this old when she sees it, she knows she is seeing it late.
pub const LATE_MINUTES: i64 = 3;
/// Her speaking up unasked counts as taken up if someone turns to her this
/// soon after; she keeps this many of them in mind.
pub const TAKEN_UP_WITHIN_SECONDS: f64 = 5.0 * 60.0;
pub const SPOKE_UP_KEPT: usize = 8;
/// Lines of the talk she looks at to decide whether to speak; making sense
/// of it before she answers, she reads all she keeps.
pub const CONVERSATION_LINES: usize = 15;
/// Two bots answering each other never stop: this many of her replies in
/// a row to the same one, all within the window, and she stops answering
/// them for a while.
pub const LOOP_ROUNDS: usize = 20;
pub const LOOP_WINDOW_SECONDS: f64 = 10.0 * 60.0;
pub const LOOP_PAUSE_SECONDS: f64 = 15.0 * 60.0;
/// What she heard about things is taken in every this many lines from
/// others, or, in a slow group, once a few have waited this long.
pub const HEARD_EVERY: usize = 20;
pub const HEARD_AT_LEAST: usize = 4;
pub const HEARD_AFTER_SECONDS: i64 = 2 * 3600;
/// Replies a day to people outside the community, per group.
pub const STRANGER_REPLIES_PER_DAY: u32 = 60;
/// A wait before she sees a line longer than this goes on apart from
/// whoever handed the line in.
pub const HOLD_AT_MOST_SECONDS: f64 = 120.0;
/// How often each picture's count is kept, at most.
pub const PICTURES_KEPT: usize = 512;
/// A picture is the group's once sent this often there, by more than one of
/// them: one person's habit, or twice by chance, is not.
pub const PICTURE_SENT_FOR_THE_GROUP: u32 = 3;
/// Pictures she looks at when she reads the talk, at most at once.
pub const PICTURES_AT_ONCE: usize = 6;
/// How often the group is written down again as one she is in, at most.
pub const REACH_KEPT_EVERY_SECONDS: f64 = 10.0 * 60.0;
/// How long a group counts as one she is in, since she last saw a line.
pub const SEEN_WITHIN_SECONDS: i64 = 3 * 24 * 3600;
/// Lines of each group she looks over when deciding to say something first.
pub const SHARE_LINES: usize = 12;
/// Messages gathered before the ledger is written; her reply writes it too.
pub const LEDGER_EVERY: usize = 10;
/// Who she is in the ledger.
pub const HER: &str = "her";
/// Lines taken up per group she remembers, so one handed in again is not
/// taken up twice.
pub const HANDLED_KEPT: usize = WAITING_LINES * 4;

/// One line in a group as a chat app gives it. Ids are the platform's, as
/// text; the name and text are attacker-controlled and bounded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The platform's slug (`telegram`, `discord`, `onebot`).
    pub platform: String,
    pub chat: String,
    pub message_id: String,
    /// A Telegram forum topic, answered in the same topic.
    #[serde(default)]
    pub thread: Option<i64>,
    pub from: String,
    pub display_name: String,
    pub text: String,
    /// Whether it speaks to her.
    #[serde(default)]
    pub addressed: bool,
    #[serde(default)]
    pub reply_to: Option<QuotedLine>,
    #[serde(default)]
    pub images: Vec<GroupImage>,
    /// How she shows in this group, as the platform names her (her group
    /// card on QQ), when it tells: people call her by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown_as: Option<String>,
}

impl Message {
    /// The group: `<platform>:<chat id>`.
    pub fn venue(&self) -> String {
        format!("{}:{}", self.platform, self.chat)
    }

    /// What was said, with the line it replies to in front. Her own line is
    /// quoted as hers and said before: written 「回复你说的：…」 it read as
    /// though they were saying her words back to her.
    pub fn said(&self) -> String {
        match &self.reply_to {
            Some(quoted) if quoted.hers => {
                format!("（回复你之前说的「{}」）{}", quoted.text, self.text)
            }
            Some(quoted) => format!("（回复 {}：{}）{}", quoted.name, quoted.text, self.text),
            None => self.text.clone(),
        }
    }

    /// Where a send to this line goes.
    fn target(&self) -> Target {
        Target {
            platform: self.platform.clone(),
            chat: self.chat.clone(),
            thread: self.thread,
            message_id: self.message_id.clone(),
        }
    }
}

/// Where something she sends goes: the group, the topic, and the line it
/// answers (empty when it answers none).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub platform: String,
    pub chat: String,
    #[serde(default)]
    pub thread: Option<i64>,
    pub message_id: String,
}

/// How she is as this step happens: the end knows these.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Her {
    /// She is not on this end now (D3): she is not in the group either.
    #[serde(default)]
    pub away: bool,
    /// Asleep, awake in so many seconds.
    #[serde(default)]
    pub asleep_for: Option<f64>,
    /// In the middle of something of her own, free in so many seconds.
    #[serde(default)]
    pub busy_for: Option<f64>,
    /// Her date in her zone (`YYYY-MM-DD`): what "today" is for her.
    pub today: String,
}

/// When and with what this step happens.
#[derive(Debug, Clone)]
pub struct At {
    /// Microseconds since the Unix epoch.
    pub now: i64,
    pub her: Her,
    pub rolls: Rolls,
}

/// What she has in mind of a group, between steps. The end keeps it as it
/// is and hands it back with the next step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub version: u32,
    #[serde(default)]
    pub lines: VecDeque<Line>,
    /// Whether the lines kept before a restart were brought back.
    #[serde(default)]
    pub restored: bool,
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub last_reply: Option<i64>,
    /// The latest line that did not call her by name and she has not looked
    /// at yet, since when lines have gone unlooked at, and whether she is
    /// looking now.
    #[serde(default)]
    pub pending: Option<Message>,
    #[serde(default)]
    pub unjudged_since: Option<i64>,
    #[serde(default)]
    pub judging: bool,
    /// When she was last called there, and when she means to glance at it.
    #[serde(default)]
    pub called: Option<i64>,
    #[serde(default)]
    pub glance_at: Option<i64>,
    /// Her speaking up unasked there lately, oldest first.
    #[serde(default)]
    pub spoke_up: VecDeque<SpokeUp>,
    /// Whom she answered lately, in order, and whom she stopped answering.
    #[serde(default)]
    pub answered: VecDeque<Answered>,
    #[serde(default)]
    pub paused: BTreeMap<String, i64>,
    /// Lines that spoke to her while she was busy, oldest first.
    #[serde(default)]
    pub waiting: VecDeque<Message>,
    /// Lines someone added after one that called her, answered with it.
    #[serde(default)]
    pub read_with: VecDeque<String>,
    #[serde(default)]
    pub muted_until: Option<i64>,
    #[serde(default)]
    pub everyone_muted_until: Option<i64>,
    /// Replies today to people outside the community.
    #[serde(default)]
    pub stranger_replies: Option<DayCount>,
    /// The stickers last offered to each person from outside (by who they
    /// are), so the number she picks is the sticker she saw.
    #[serde(default)]
    pub offered: BTreeMap<String, Vec<String>>,
    /// The member's turn she shows she is typing for, while it runs.
    #[serde(default)]
    pub typing_for: Option<String>,
    /// A game with seats at the group's table, while it is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seating: Option<flows::Seating>,
    /// The last line she took in for what she heard.
    #[serde(default)]
    pub heard_upto: Option<i64>,
    /// How she shows in the group, as its latest line told (see
    /// [`Message::shown_as`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown_as: Option<String>,
    /// How often each picture was sent there lately (by its key).
    #[serde(default)]
    pub pictures: BTreeMap<String, u32>,
    /// Who sent each picture there lately (by its key).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub picture_senders: BTreeMap<String, BTreeSet<String>>,
    /// The latest line seen there: where she would say something first.
    #[serde(default)]
    pub reach: Option<Message>,
    #[serde(default)]
    pub reach_kept: Option<i64>,
    /// When she last saw a line there, as kept before a restart.
    #[serde(default)]
    pub seen_before: Option<i64>,
    /// Messages, waits and pieces not yet written to the group's ledger.
    #[serde(default)]
    pub ledger: Vec<Typed>,
    #[serde(default)]
    pub ledger_waits: Vec<f64>,
    #[serde(default)]
    pub ledger_theirs: Counts,
    #[serde(default)]
    pub ledger_hers: Counts,
    /// She held back what she wrote because the talk moved on.
    #[serde(default)]
    pub held_back: bool,
    /// The latest line that called her while she was away, and when (D3-2).
    #[serde(default)]
    pub left_while_away: Option<(Message, i64)>,
    /// Lines that called her she took up, and lines taken back up after a
    /// restart, by id: each once.
    #[serde(default)]
    pub handled: VecDeque<String>,
    #[serde(default)]
    pub taken_back: VecDeque<String>,
    /// What she is in the middle of there.
    #[serde(default)]
    pub errands: Vec<flows::Errand>,
    #[serde(default)]
    pub next: u64,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            version: VERSION,
            lines: VecDeque::new(),
            restored: false,
            busy: false,
            last_reply: None,
            pending: None,
            unjudged_since: None,
            judging: false,
            called: None,
            glance_at: None,
            spoke_up: VecDeque::new(),
            answered: VecDeque::new(),
            paused: BTreeMap::new(),
            waiting: VecDeque::new(),
            read_with: VecDeque::new(),
            muted_until: None,
            everyone_muted_until: None,
            stranger_replies: None,
            offered: BTreeMap::new(),
            typing_for: None,
            seating: None,
            heard_upto: None,
            shown_as: None,
            pictures: BTreeMap::new(),
            picture_senders: BTreeMap::new(),
            reach: None,
            reach_kept: None,
            seen_before: None,
            ledger: Vec::new(),
            ledger_waits: Vec::new(),
            ledger_theirs: Counts::default(),
            ledger_hers: Counts::default(),
            held_back: false,
            left_while_away: None,
            handled: VecDeque::new(),
            taken_back: VecDeque::new(),
            errands: Vec::new(),
            next: 0,
        }
    }
}

impl Group {
    /// A group as kept, or a fresh one when it is of another version.
    pub fn read(value: Value) -> Self {
        match serde_json::from_value::<Group>(value) {
            Ok(group) if group.version == VERSION => group,
            _ => Group::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpokeUp {
    pub at: i64,
    pub taken: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answered {
    pub from: String,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCount {
    pub day: String,
    pub count: u32,
}

/// What was kept of a group before a restart.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Restore {
    #[serde(default)]
    pub lines: Option<Vec<Line>>,
    #[serde(default)]
    pub spoke_up: Vec<SpokeUp>,
    /// The game with seats that was on at the group's table, as kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<Value>,
}

/// A line of the group's talk read back after she was not there: the line,
/// when it was said (µs), and whether it was hers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Past {
    pub message: Message,
    pub at: i64,
    #[serde(default)]
    pub hers: bool,
}

/// How a mute changed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Mute {
    For { seconds: u64 },
    UntilLifted,
    Lifted,
}

/// Why she comes back to a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Resume {
    /// This end restarted: the line that called her before it, read back.
    Restart,
    /// She is back on this end: the latest line that called her while away.
    Back,
}

/// What came, for one step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Event {
    /// What was kept of the group before a restart (`lines` none when
    /// nothing was kept), and how her speaking up there went.
    Restored {
        #[serde(default)]
        lines: Option<Vec<Line>>,
        #[serde(default)]
        spoke_up: Vec<SpokeUp>,
        /// The game with seats that was on at the group's table, as kept
        /// (see [`Effect::KeepTable`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        table: Option<Value>,
    },
    /// A group she is in, as kept before a restart: where she would say
    /// something, and when she last saw a line there.
    Reached { message: Message, seen_at: i64 },
    /// A line, whoever wrote it: kept in mind (`fresh` says whether it was
    /// new). `restore` is what was kept before a restart, the first time
    /// after one: brought back once who is here lately is read.
    Heard {
        message: Message,
        #[serde(default)]
        restore: Option<Restore>,
    },
    /// A line that calls her. `waiter` is told when she is done with it.
    Called {
        message: Message,
        #[serde(default)]
        waiter: Option<String>,
    },
    /// A line that does not call her by name.
    Noticed { message: Message },
    /// She was muted there, or it lifted, by `by` (their platform id;
    /// empty if not known).
    Muted {
        by: String,
        mute: Mute,
        everyone: bool,
        #[serde(default)]
        restore: Option<Restore>,
    },
    /// The group read back after she was not there.
    Resumed {
        why: Resume,
        #[serde(default)]
        past: Vec<Past>,
        /// When this end first heard a group (restart only).
        #[serde(default)]
        up_since: Option<i64>,
    },
    /// Say something first there, for `reason`, billed to `owner`.
    SpeakFirst {
        reason: String,
        #[serde(default)]
        owner: Option<i32>,
        waiter: String,
    },
    /// A timer she set.
    Woke { key: String },
    /// The answer to something she asked the end to do.
    Done { id: String, answer: Answer },
    /// A sticker she chose went out. `restore` as for `heard`: her line
    /// about it is kept under what was kept before a restart, if that could
    /// not be read until now.
    StickerSent {
        meaning: String,
        #[serde(default)]
        restore: Option<Restore>,
    },
    /// Someone seated at the group's table whispered to her (`who` as
    /// `<platform>:<id>`): taken when it is a move of theirs in the game,
    /// else it is not the game's (see [`Outcome::taken`]).
    Whispered {
        who: String,
        text: String,
        /// A button pressed: the wait whose whisper it was on (see
        /// [`Effect::Whisper`]); a button of a wait gone by is not taken.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wait: Option<u64>,
    },
}

/// Who wrote a line, as the end knows them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Who {
    /// A paired member; `bound` whether their binding still holds.
    Member { user_id: i32, bound: bool },
    /// Someone from outside the community.
    Stranger,
    /// Not known (the lookup failed).
    Unknown,
}

/// What the end came back with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Answer {
    Claimed {
        ok: bool,
    },
    Looked {
        seen: Vec<Looked>,
    },
    Identified {
        who: Who,
        /// The site's owner, who hosts her there, if there is one.
        #[serde(default)]
        owner: Option<i32>,
    },
    Prepared {
        ok: bool,
    },
    /// Her speaking soul.
    Soul {
        #[serde(default)]
        text: String,
    },
    /// Memories read (none when they could not be).
    Rows {
        #[serde(default)]
        rows: Vec<MemoryRow>,
    },
    /// The cues kept for memories, one per line, by memory id.
    Cues {
        #[serde(default)]
        cues: std::collections::HashMap<String, String>,
    },
    /// The vectors of memories that have one, by memory id.
    Vectors {
        #[serde(default)]
        vectors: Vec<(String, Vec<f32>)>,
        /// False when there is no embedding model or the vectors could not
        /// be read: what was asked is then not embedded either.
        #[serde(default)]
        known: bool,
    },
    /// The vector of what was asked (none when the model gave none).
    Embedded {
        #[serde(default)]
        vector: Option<Vec<f32>>,
    },
    LedgerRead {
        #[serde(default)]
        stored: Option<Pieces>,
    },
    /// What was kept before a restart (none when it still cannot be read).
    Kept {
        #[serde(default)]
        restore: Option<Restore>,
    },
    /// A member's turn opened (none: it may not be had now).
    TurnBegun {
        #[serde(default)]
        turn: Option<String>,
        #[serde(default)]
        sections: Vec<String>,
        #[serde(default)]
        doing: Option<String>,
    },
    TurnEnded {
        #[serde(default)]
        reply: Option<String>,
        /// Whether whom she answered is still bound to her.
        #[serde(default = "yes")]
        bound: bool,
    },
    /// Her life as facts: what she did that the talk touches (what it was,
    /// what stayed with her), what of her own time she would tell someone,
    /// and what she is doing now.
    Life {
        #[serde(default)]
        did: Vec<(String, String)>,
        #[serde(default)]
        would_tell: Vec<String>,
        #[serde(default)]
        now: Option<String>,
    },
    MayLookUp {
        may: May,
    },
    Whispered {
        ok: bool,
    },
    /// What was asked could not be read (the store is down): what needs it
    /// is left, as when nothing could be asked.
    Unread,
    Now {
        /// RFC 3339, in her zone.
        local: String,
        #[serde(default)]
        doing: Option<String>,
    },
    Stickers {
        #[serde(default)]
        stickers: Vec<StickerRow>,
        #[serde(default)]
        can_make: bool,
        #[serde(default)]
        left: usize,
    },
    Titles {
        #[serde(default)]
        titles: Vec<(String, Option<String>)>,
    },
    Label {
        label: String,
    },
    /// The game on at a table, as kept (none: none is on).
    Game {
        #[serde(default)]
        game: Option<Value>,
    },
    /// Turtle-soup surfaces she used lately, wherever.
    Surfaces {
        #[serde(default)]
        surfaces: Vec<String>,
    },
    /// The dictionaries' entry, if they have one.
    Defined {
        #[serde(default)]
        entry: Option<Entry>,
    },
    /// What the search turned up, and where: none when nothing.
    Found {
        #[serde(default)]
        found: Option<(String, String)>,
    },
    Reacted {
        ok: bool,
    },
    Typed,
    Asked {
        #[serde(default)]
        raw: Option<String>,
        /// Why there is no answer: unavailable, timeout, cut (the stream
        /// broke off), invalid, or failed.
        #[serde(default)]
        failure: Option<String>,
    },
    Sent {
        ok: bool,
    },
}

fn yes() -> bool {
    true
}

/// A picture looked at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Looked {
    #[serde(default)]
    pub message_id: Option<String>,
    pub index: usize,
    pub seen: Seen,
}

/// A picture to look at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    #[serde(default)]
    pub message_id: Option<String>,
    pub index: usize,
    pub image: GroupImage,
}

/// Pieces of their messages and hers, as counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pieces {
    pub theirs: Counts,
    pub hers: Counts,
}

/// Her judgment of whether to speak up: why, and the reason as the turn
/// that speaks is told it (for a reaction, the emoji).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub why: crate::joining::Why,
    pub reason: String,
}

/// Where she stands in the group, beside what was said, for the judgment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Here {
    pub last_spoke: Option<String>,
    pub late: Option<String>,
    pub how_it_went: Option<String>,
    pub share: Option<String>,
}

/// A line of the conversation as a turn reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spoken {
    pub role: String,
    pub content: String,
}

/// A piece of her reply: text, or someone mentioned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Piece {
    Text { text: String },
    Mention { id: String, name: String },
}

/// Someone from outside the community, as her note on them knows them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stranger {
    pub who: String,
    pub name: String,
}

/// A stretch of talk for what she heard in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stretch {
    /// RFC 3339, as the line was kept.
    pub at: String,
    pub name: String,
    pub text: String,
    pub hers: bool,
}

/// What the end is to do. Those with an `id` it answers with
/// [`Event::Done`]; the rest it just does, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Effect {
    /// Step again with [`Event::Woke`] after `after` seconds.
    Wake { key: String, after: f64 },
    /// Keep the group's lines (six hours).
    KeepLines { lines: Vec<Line> },
    /// Keep that she is in the group (three days).
    KeepReach {
        #[serde(default)]
        thread: Option<i64>,
        seen_at: i64,
    },
    /// Add this to the group's ledger (see [`merge_ledger`]).
    Ledger {
        messages: Vec<Typed>,
        waits: Vec<f64>,
        theirs: Counts,
        hers: Counts,
    },
    /// A line someone else said, for the words she watches there.
    Jargon { text: String },
    /// A stretch of the talk, for what she heard in it and what the group
    /// was like (on the site owner's account).
    TakeIn { stretch: Vec<Stretch> },
    /// A picture sent there again: the group's.
    PictureAgain { key: String, seen: Seen },
    /// Her speaking up there, and whether anyone took it up.
    SpokeUp { line: String, taken: bool },
    /// After she answered someone from outside: their exchange.
    StrangerAfter {
        owner: i32,
        stranger: Stranger,
        said: String,
        reply: String,
        message_id: String,
    },
    /// Send the sticker she chose (making it first when new); step with
    /// [`Event::StickerSent`] once it went.
    Sticker { to: Target, chosen: Value },
    /// Tell `waiter` she is done with what it handed in.
    Settled { waiter: String, value: bool },
    /// Claim the inbound id once.
    Claim { id: String, inbound_id: String },
    /// Look at pictures (on the site owner's account).
    Look { id: String, pictures: Vec<Picture> },
    /// Who wrote the line, and the site's owner.
    Identify { id: String, from: String },
    /// Whether `user_id` may talk to her now, and their session here.
    Prepare { id: String, user_id: i32 },
    /// Read something kept: hers (on her host) or others' (on the site).
    Read { id: String, of: Of, what: ToRead },
    /// The vector of `text`, as the embedding model gives it.
    Embed { id: String, text: String },
    /// Keep something: hers or others'.
    Write { of: Of, what: ToWrite },
    /// Whether she may look `term` up now: not tried today, and within the
    /// day's allowance for words and memes (hers: on a host with no
    /// allowance, only whether she tried it today).
    MayLookUp { id: String, term: String },
    /// What the slang and meme dictionaries have for `term`.
    Define { id: String, term: String },
    /// A search for `query` and what its first result says.
    Search { id: String, query: String },
    /// What the group's ledger has kept of its pieces.
    ReadLedger { id: String },
    /// What was kept of the group before a restart, read again: her own
    /// line is about to be kept and it could not be read before.
    ReadKept { id: String },
    /// Open a member's turn, as one of theirs in their session in the group
    /// (their line kept unless she speaks first, their budget reserved, how
    /// she feels moved by it): what she knows of them and how she is, as the
    /// end's whole sections (D2c-8 A), and what she is doing on her own.
    TurnBegin {
        id: String,
        user_id: i32,
        input: String,
        transcript: Vec<Spoken>,
        speaker: String,
        chime: Option<String>,
        first: bool,
    },
    /// Close it: with what she said (and the sticker she chose), ready to
    /// go out; what went out is kept by [`Effect::TurnKept`] once it has
    /// (D2c-9 A); without, left. Answered with her words as they stand, and
    /// whether whom she answered is still hers.
    TurnEnd {
        id: String,
        turn: String,
        user_id: i32,
        said: Option<String>,
        sticker: Option<Value>,
        /// She spoke first: nobody's binding to hold to.
        first: bool,
    },
    /// What of a closed turn went out (nothing, if none did): only that is
    /// kept in their session and taken into her memory. Words that never
    /// reached the group were not said.
    TurnKept {
        turn: String,
        went_out: String,
        /// The sticker she chose went out with it.
        sticker: bool,
    },
    React {
        id: String,
        to: Target,
        emoji: String,
    },
    /// Show her typing.
    Typing { id: String, to: Target },
    /// Ask the model.
    Ask {
        id: String,
        #[serde(default)]
        how: AskKind,
        voice: String,
        owner: i32,
        operation: String,
        timeout_ms: u64,
        system: String,
        input: String,
        schema_name: String,
        schema: Value,
        /// A member's turn it is asked within, when it is one.
        #[serde(default)]
        turn: Option<String>,
    },
    /// Whisper `text` to `user` on `platform` (one in the group `chat`):
    /// answered with whether it reached them.
    Whisper {
        id: String,
        platform: String,
        user: String,
        chat: String,
        text: String,
        /// The choices it offers, as buttons where the platform has them
        /// (pressing one comes back whispered as its number).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        choices: Vec<String>,
        /// With choices: which wait they are for, sent back with a button
        /// pressed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        wait: Option<u64>,
    },
    /// Keep the game with seats at the group's table as it now stands, so
    /// it outlives a restart (none: there is none any more); handed back in
    /// with what else was kept ([`Restore::table`]).
    KeepTable {
        #[serde(default)]
        table: Option<Value>,
    },
    /// `who` sits at the group's table: what they whisper to her comes to
    /// the group's mind first.
    Seat { who: String },
    /// `who` sits there no more.
    Unseat { who: String },
    /// Send one message of hers.
    Send {
        id: String,
        to: Target,
        pieces: Vec<Piece>,
        quote: bool,
    },
}

/// How a model is asked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AskKind {
    /// For a JSON answer to `schema`, with `system`.
    #[default]
    Json,
    /// What she says to a whole prompt (`input`), streamed.
    Say,
    /// What she makes of pictures.
    Look,
}

/// Whose a thing read or kept is: hers lives on her host, others' on the
/// site (H4-g2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Of {
    Hers,
    Others,
}

/// Which memories come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Newest {
    Updated,
    Created,
}

/// What to read. The reads are plain: which rows, newest first, how many;
/// what comes to mind of them is worked out here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ToRead {
    Soul,
    /// Memories of one source kept in one venue, still standing.
    VenueRows {
        venue: String,
        source: String,
        newest: Newest,
        limit: u64,
    },
    Cues {
        ids: Vec<String>,
    },
    /// Her own memories of one source (her views, what she heard, the words
    /// and memes she knows), still standing, newest first.
    OwnRows {
        source: String,
        limit: u64,
    },
    /// Her life as the host lives it, as facts: what she did that `talk`
    /// touches (at most `related`), what of her own time she would tell
    /// someone, and what she is doing now.
    Life {
        talk: String,
        related: usize,
    },
    /// Her clock in her zone (RFC 3339), and what she is doing now.
    Now,
    /// The stickers she may send in the group `venue`: hers still of her,
    /// and those she kept that she still uses, belonging there or anywhere;
    /// most sent first. With whether she can make one, and how many are
    /// left this month.
    Stickers {
        venue: String,
    },
    /// The row of `source` kept in `venue` with no owner whose evidence
    /// carries `marker`, if any (her note on someone from outside, her guess
    /// at who they are), most recently touched first.
    Unowned {
        venue: String,
        source: String,
        marker: String,
    },
    /// Those kept in groups other than `here`, at most `limit`.
    UnownedElsewhere {
        here: String,
        source: String,
        marker: String,
        limit: u64,
    },
    /// Rows of `sources` kept in groups other than `here` (empty: any
    /// group), freshest first, short notes that faded left out.
    OtherGroups {
        here: String,
        sources: Vec<String>,
        limit: u64,
    },
    /// What these groups (`<platform>:<chat>`) call themselves, as their
    /// platforms say.
    Titles {
        venues: Vec<String>,
    },
    /// How someone from the community goes by, by their account.
    Label {
        user_id: i32,
    },
    /// The game on at a table (`g:<venue>`), as the group keeps it.
    Game {
        table: String,
    },
    /// Turtle-soup surfaces she used lately, at any table: hers.
    RecentSurfaces,
    /// What a game with seats of `game` was dealt lately (the pairs of
    /// words of 谁是卧底), answered as surfaces: hers.
    RecentDealt {
        game: String,
    },
    /// What she is called where people see her (her seat's name at a game
    /// she plays), answered as a label: hers.
    HerName,
    /// The vectors kept for these memories (as read, with their cues), by
    /// the model in use; up to `fill` of those without one are given one
    /// behind the reply.
    Vectors {
        rows: Vec<(String, String)>,
        fill: usize,
    },
}

/// What to keep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ToWrite {
    /// Something kept in one group, about no account.
    RememberInVenue {
        venue: String,
        content: String,
        evidence: String,
        source: String,
    },
    /// Something of hers, about no account (a word she learned).
    RememberOwn {
        content: String,
        evidence: String,
        concepts: Vec<myriad_agent_rules::Concept>,
        source: String,
    },
    /// A word in the group `venue` she could not learn: watched as maybe
    /// the group's own, with the lines of the talk that used it.
    Watch {
        venue: String,
        term: String,
        used: Vec<String>,
    },
    /// Keep the game on at a table (as the game is kept).
    Game { table: String, game: Value },
    /// Take the game off a table.
    TakeGame { table: String },
    /// A turtle-soup surface she used: hers.
    UsedSurface { surface: String },
    /// What a game with seats of `game` was dealt (kept among the latest
    /// [`crate::undercover::RECENT_KEPT`]): hers.
    Dealt { game: String, dealt: String },
    /// How a puzzle of hers went at a table, kept with it: hers.
    Tried {
        id: String,
        tried: crate::making::Tried,
    },
}

/// Whether she may look a word up now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum May {
    Yes,
    /// Tried already today, found or not: left.
    Tried,
    /// Nothing left of the day's allowance: watched instead.
    NoneLeft,
}

/// A sticker she may send, as it is read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickerRow {
    pub id: String,
    pub meaning: String,
    /// The group it belongs to; none: anywhere.
    #[serde(default)]
    pub venue: Option<String>,
    #[serde(default)]
    pub sent: i32,
    /// µs.
    #[serde(default)]
    pub last_sent_at: Option<i64>,
    /// The group she kept it from, when it is not of her.
    #[serde(default)]
    pub kept_from: Option<String>,
}

/// A dictionary entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub title: String,
    pub text: String,
    pub url: String,
}

/// A memory as it is read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRow {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub concepts: Vec<myriad_agent_rules::Concept>,
    pub source: String,
    #[serde(default)]
    pub evidence: Option<String>,
    /// Where it is kept (`group:telegram:-100123`, `own`).
    #[serde(default)]
    pub venue: Option<String>,
    /// µs.
    pub created_at: i64,
    #[serde(default)]
    pub access_count: i32,
    #[serde(default)]
    pub last_accessed_at: Option<i64>,
}

impl MemoryRow {
    pub(crate) fn view(&self) -> myriad_agent_rules::memory::rank::Row<'_> {
        let at = |micros: i64| lines::time(micros).fixed_offset();
        myriad_agent_rules::memory::rank::Row {
            id: &self.id,
            content: &self.content,
            concepts: &self.concepts,
            source: &self.source,
            created_at: at(self.created_at),
            access_count: self.access_count,
            last_accessed_at: self.last_accessed_at.map(at),
        }
    }
}

/// What one step came to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub effects: Vec<Effect>,
    /// For [`Event::Heard`]: whether the line was new.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh: Option<bool>,
    /// For [`Event::Whispered`]: whether the game took it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken: Option<bool>,
}

/// One step of the group's mind.
pub fn step(group: &mut Group, at: &mut At, event: Event) -> Outcome {
    let mut out = Outcome::default();
    flows::step(group, at, event, &mut out);
    out
}

/// The group as she weighs saying something first in it: its latest
/// lines, how long it has been quiet, and how her speaking up went there.
/// None when she is busy, talking, or muted there, or it has gone too long
/// unseen to count as one she is in, or she has no line there to say it by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub lines: Vec<String>,
    pub quiet_for: String,
    pub spoke_up_lately: Vec<Value>,
}

pub fn offer(group: &Group, now: i64) -> Option<Offer> {
    flows::offer_view(group, now)
}
