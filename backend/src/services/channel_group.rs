//! The persona in group chats: the community's shared venues, on any
//! platform whose bot can hear a group (Telegram groups, Discord server
//! channels). Each platform only parses its lines into a [`GroupLine`] and
//! delivers her replies; everything else here is the same everywhere. A
//! group is its venue, `<platform>:<chat id>` (`telegram:-100123`,
//! `discord:123456`).
//!
//! She answers a group line when it calls her (an @mention, a mention of
//! her, a command aimed at her). A reply to her message she reads with the
//! rest of the talk, the way a line that does not call her is read: it is
//! often meant for her and she answers it then, but it may be meant for
//! someone else. Groups get no pairing prompts.
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
//! she is busy waits: when she is done she answers those waiting in order (a
//! turtle soup's questions come fast).
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
//!
//! All of that is her group mind, which lives in the shared core
//! (`myriad_merope::group`, D2) so any end that hosts her runs the same; this
//! module is the site as that end (`mind`): it keeps each group's state,
//! steps it with what comes, and does what it says against the platforms,
//! the database and her other parts.

use std::time::Duration;

use tokio::time::Instant;

use myriad_agent_rules::channel::{
    ConnectFailureKind, DiscordGroupMessage, GroupImage, ImageFetch, PairingLookup, QuotedLine,
    TelegramGroupMessage,
};
use sea_orm::DatabaseConnection;
use tracing::{info, warn};

use crate::services::agent::AgentInteractionMode;
use crate::services::agent::types::ConversationMessage;
use crate::services::channel_pairing::{ChannelBinding, PairingChannel};
use crate::services::channel_platform::ChannelPlatform;

mod doing;
mod first;
#[cfg(test)]
pub(crate) mod g0;
mod io;
mod ledger;
mod line;
mod mind;
mod platform;
mod reads;
mod title;
mod turns;

use doing::*;
use first::*;
use io::*;
use ledger::*;
use mind::*;
use platform::*;
use reads::*;
use turns::*;

pub use first::{come_back, share_first};
pub(crate) use ledger::talk_between;
pub use line::GroupLine;
pub(crate) use mind::shown_as;
pub use mind::{
    back_from_away, catch_up, groups_lately, handle, muted, names_here, notice, record,
    pressed, seated, take_back_kept, take_back_tables, whispered,
};
pub use title::title;

/// Groups she keeps in mind at once.
const MAX_GROUPS: usize = 256;

/// Runtime-registry namespace of each group's recent lines: what she keeps
/// in mind of a group outlives a restart, for as long as she would keep it.
const LINES_NAMESPACE: &str = "group_lines";
/// Runtime-registry namespace of the groups she is in (see `StoredReach`):
/// after a restart she can still say something first in one that has gone
/// quiet, for as long as it counts as one she is in.
const REACH_NAMESPACE: &str = "group_reach";
/// Runtime-registry namespace of the game with seats on at each group's
/// table, as her group mind keeps it: it outlives a restart (D2-3, D2-9).
const TABLE_NAMESPACE: &str = "group_table";
/// How long a game left at a table is kept: longer than any game goes on.
const TABLE_FOR_SECONDS: i64 = 86_400;

/// A group's lines as kept across a restart.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredLines {
    lines: Vec<myriad_merope::group::Line>,
}

/// A group she is in, as kept across a restart: where in it she would say
/// something (a Telegram forum topic), and when she last saw a line there.
/// Never the token: that is read from the configuration when she sends.
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredReach {
    #[serde(default)]
    thread: Option<i64>,
    seen_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
// 场景用一把 std 锁跨 await 串行占住整段回合（见 WHOLE_TURNS），是有意的。
#[allow(clippy::await_holding_lock)]
mod g0_scenes;
#[cfg(test)]
mod tests;
