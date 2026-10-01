//! The persona in group chats: the community's shared venues, on any
//! platform whose bot can hear a group (Telegram groups, Discord server
//! channels). Each platform only parses its lines into a [`GroupLine`] and
//! delivers her replies; everything else here is the same everywhere. A
//! group is its venue, `<platform>:<chat id>` (`telegram:-100123`,
//! `discord:123456`).
//!
//! She answers a group line when it speaks to her (an @mention, a mention of
//! her, a command aimed at her, or a reply to her message). Groups get no
//! pairing prompts.
//!
//! For a member of the community (a paired site user) the turn is a Chat
//! turn under their own account (their quota), in a group venue: people
//! outside the community may be reading, so she draws only on what this group
//! has heard, never anyone's private matters (see
//! `memory::unified::Audience::group`). Anyone else she answers lightly, with
//! far less context and only a small note on those she keeps running into
//! (see `merope::strangers`); the site's owner hosts her there and pays, up to
//! a daily number of such replies per group. The group's recent lines, other
//! people's included, are the conversation she answers in; they are untrusted.
//!
//! A group gets one turn at a time and a short pause between replies, so a
//! busy group cannot crowd out everyone else. A line that speaks to her while
//! she is busy waits: when she is done she answers those waiting in order, a
//! few at most (a turtle soup's questions come fast).
//! Delivery is best effort: a restart mid-turn loses that reply, which is
//! acceptable for chat. A line that called her and was still waiting for her
//! (she was asleep or busy) is taken back up after a restart, where the
//! group is read back on reconnecting (OneBot).
//!
//! A line that does not call her by name she reads as a person in the group
//! does: when the talk pauses she looks, and decides as herself whether to
//! say something. Someone going on with what she was talking about gets an
//! answer without having to @ her; otherwise she speaks up only for a reason
//! of her own that means something to them (she knows something about it,
//! something of hers goes with it, or she wants to ask), and most of the time
//! she stays quiet (see `merope::joining`). How often is hers to judge; the
//! only stop is for a sender she answers nonstop, as two bots would.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use myriad_agent_rules::channel::{
    ConnectFailureKind, DiscordGroupMessage, GroupImage, ImageFetch, PairingLookup, QuotedLine,
    TelegramGroupMessage,
};
use sea_orm::DatabaseConnection;
use tracing::{info, warn};

use crate::services::agent::AgentInteractionMode;
use crate::services::agent::merope::group::joining::Why;
use crate::services::agent::types::{AgentProgressEvent, ConversationMessage};
use crate::services::channel_pairing::{ChannelBinding, PairingChannel};
use crate::services::channel_platform::ChannelPlatform;

mod answer;
mod group;
mod ledger;
mod line;
mod memory;
mod pictures;
mod platform;
mod reaching_out;
mod run;
mod speaking_up;
mod transcript;
mod turns;

use answer::*;
use group::*;
use ledger::*;
use memory::*;
use pictures::*;
use platform::*;
use run::*;
use speaking_up::*;
use transcript::*;
use turns::*;

pub use line::GroupLine;
pub use memory::{catch_up, groups_lately, record};
pub use reaching_out::share_first;
pub use speaking_up::notice;
pub use turns::handle;

/// Lines of a group she keeps in mind, and for how long.
const TRANSCRIPT_LINES: usize = 30;
const TRANSCRIPT_FOR: Duration = Duration::from_secs(6 * 3600);
const MAX_GROUPS: usize = 256;
const MAX_LINE_CHARS: usize = 500;
/// Lines that spoke to her while she was busy, answered in order after.
const WAITING_LINES: usize = 5;
/// Between two of her replies in the same group.
const GROUP_PAUSE: Duration = Duration::from_secs(5);
/// Lines that did not call her by name, in talk she is in: she looks once
/// the talk pauses this long, and in talk that never pauses, at least this
/// often.
const SETTLE: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(30);
/// She is in a group's talk while she spoke there, or was called there,
/// this recently; otherwise she glances at it now and then, when she is
/// free: this many seconds after a line, give or take, and while she is in
/// the middle of something of her own, once it is done (waiting at most so
/// long for it).
const IN_TALK: Duration = Duration::from_secs(10 * 60);
const GLANCE_AFTER_SECONDS: std::ops::RangeInclusive<u64> = 30..=180;
const LONGEST_BUSY: Duration = Duration::from_secs(30 * 60);
/// Talk this old when she sees it, she knows she is seeing it late.
const LATE: chrono::Duration = chrono::Duration::minutes(3);
/// Her speaking up unasked counts as taken up if someone turns to her this
/// soon after; she keeps this many of them in mind.
const TAKEN_UP_WITHIN: Duration = Duration::from_secs(5 * 60);
const SPOKE_UP_KEPT: usize = 8;
/// Lines of the talk she looks at.
const CONVERSATION_LINES: usize = 15;
/// Two bots answering each other never stop, and no person talks like that:
/// this many of her replies in a row to the same one, all within the
/// window, and she stops answering them for a while.
const LOOP_ROUNDS: usize = 20;
const LOOP_WINDOW: Duration = Duration::from_secs(10 * 60);
const LOOP_PAUSE: Duration = Duration::from_secs(15 * 60);
/// What she heard about things (see `merope::heard`) is taken in every this
/// many lines from others, or, in a slow group, once a few have waited this
/// long: always while the lines are still in mind.
const HEARD_EVERY: usize = 20;
const HEARD_AT_LEAST: usize = 4;
const HEARD_AFTER: chrono::Duration = chrono::Duration::hours(2);
/// Replies a day to people outside the community, per group.
const STRANGER_REPLIES_PER_DAY: u32 = 60;

/// A wait before she sees a line longer than this goes on apart from the
/// caller, which holds an ingress permit while it waits.
const HOLD_AT_MOST: Duration = Duration::from_secs(120);
/// Longest she takes over one group reply before giving up on it.
const TURN_DEADLINE: Duration = Duration::from_secs(90);
const TYPING_EVERY: Duration = Duration::from_secs(4);

/// Runtime-registry namespace of each group's recent lines: what she keeps
/// in mind of a group outlives a restart, for as long as she would keep it.
const LINES_NAMESPACE: &str = "group_lines";

#[cfg(test)]
mod tests;
