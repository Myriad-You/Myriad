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
//! acceptable for chat.
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
use crate::services::agent::merope::joining::Why;
use crate::services::agent::types::{AgentProgressEvent, ConversationMessage};
use crate::services::channel_pairing::{ChannelBinding, PairingChannel};
use crate::services::channel_platform::ChannelPlatform;

/// One human line in a group, whatever the platform. Ids are the platform's,
/// as text; the name and text are attacker-controlled and bounded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupLine {
    pub platform: ChannelPlatform,
    pub chat: String,
    pub message_id: String,
    /// A Telegram forum topic, answered in the same topic.
    pub thread: Option<i64>,
    pub from: String,
    pub display_name: String,
    pub text: String,
    /// Whether it speaks to her.
    pub addressed: bool,
    /// The line it replies to, if any.
    pub reply_to: Option<QuotedLine>,
    /// Pictures in it.
    pub images: Vec<GroupImage>,
}

impl GroupLine {
    /// The group, as sessions and memory know it: `<platform>:<chat id>`.
    pub fn venue(&self) -> String {
        format!("{}:{}", self.platform.slug(), self.chat)
    }

    /// What was said, with the line it replies to in front: a reply makes
    /// sense only with what it answers ("说到一半怎么没了").
    pub fn said(&self) -> String {
        match &self.reply_to {
            Some(quoted) if quoted.hers => format!("（回复你说的：{}）{}", quoted.text, self.text),
            Some(quoted) => format!("（回复 {}：{}）{}", quoted.name, quoted.text, self.text),
            None => self.text.clone(),
        }
    }
}

impl From<TelegramGroupMessage> for GroupLine {
    fn from(message: TelegramGroupMessage) -> Self {
        Self {
            platform: ChannelPlatform::Telegram,
            chat: message.chat_id.to_string(),
            message_id: message.message_id.to_string(),
            thread: message.message_thread_id,
            from: message.from_id.to_string(),
            display_name: message.display_name,
            text: message.text,
            addressed: message.addressed,
            reply_to: message.reply_to,
            images: message.images,
        }
    }
}

impl From<myriad_agent_rules::onebot::decode::OneBotGroupLine> for GroupLine {
    fn from(message: myriad_agent_rules::onebot::decode::OneBotGroupLine) -> Self {
        Self {
            platform: ChannelPlatform::OneBot,
            chat: message.group_id,
            message_id: message.message_id,
            thread: None,
            from: message.user_id,
            display_name: message.display_name,
            text: message.text,
            addressed: message.addressed,
            reply_to: message.reply_to,
            images: message.images,
        }
    }
}

impl From<DiscordGroupMessage> for GroupLine {
    fn from(message: DiscordGroupMessage) -> Self {
        Self {
            platform: ChannelPlatform::Discord,
            chat: message.channel_id,
            message_id: message.message_id,
            thread: None,
            from: message.author_id,
            display_name: message.display_name,
            text: message.text,
            addressed: message.addressed,
            reply_to: message.reply_to,
            images: message.images,
        }
    }
}

/// Deliver one chunk of her reply in the group, as a reply to `line`, with
/// her `@name`s as the platform's mentions.
async fn send_reply(
    line: &GroupLine,
    token: &str,
    pieces: &[myriad_agent_rules::mentions::Piece],
    quoting: bool,
) -> Result<(), ConnectFailureKind> {
    use myriad_agent_rules::mentions;
    match line.platform {
        ChannelPlatform::Telegram => {
            let Ok(chat) = line.chat.parse() else {
                return Err(ConnectFailureKind::Permanent);
            };
            let quoted = match quoting.then(|| line.message_id.parse()) {
                Some(Ok(message_id)) => Some(message_id),
                Some(Err(_)) => return Err(ConnectFailureKind::Permanent),
                None => None,
            };
            let (text, entities) = mentions::telegram_text_and_entities(pieces);
            crate::services::telegram_bot::send_group_reply(
                token,
                chat,
                &text,
                &entities,
                quoted,
                line.thread,
            )
            .await
        }
        ChannelPlatform::Discord => {
            let (content, users) = mentions::discord_content_and_users(pieces);
            crate::services::discord_bot::send_group_reply(
                token,
                &line.chat,
                &content,
                &users,
                quoting.then_some(line.message_id.as_str()),
            )
            .await
        }
        ChannelPlatform::OneBot => {
            let Some(action) = myriad_agent_rules::onebot::encode::encode_group_message(
                &line.chat,
                &mentions::onebot_segments(pieces),
            ) else {
                return Err(ConnectFailureKind::Permanent);
            };
            match crate::services::onebot_send::send_action(action).await {
                Ok(None) => Ok(()),
                Ok(Some(kind)) => Err(kind),
                Err(_) => Err(ConnectFailureKind::Transient),
            }
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => Err(ConnectFailureKind::Permanent),
    }
}

async fn send_typing(line: &GroupLine, token: &str) {
    match line.platform {
        ChannelPlatform::Telegram => {
            let _ = crate::services::telegram_bot::send_typing(token, &line.chat).await;
        }
        ChannelPlatform::Discord => {
            let _ = crate::services::discord_bot::send_typing(token, &line.chat).await;
        }
        ChannelPlatform::OneBot => {
            // Group typing is not a documented NapCat action. Private typing
            // uses `set_input_status` with a user id, which a group line has
            // no reason to poke.
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => {}
    }
}

fn text_limit(platform: ChannelPlatform) -> usize {
    match platform {
        ChannelPlatform::Discord => myriad_agent_rules::channel::DISCORD_TEXT_LIMIT,
        _ => myriad_agent_rules::channel::TELEGRAM_TEXT_LIMIT,
    }
}

/// Her reply without a （回复 …） mark she copied from the transcript: the
/// platform already shows what she replies to.
fn without_reply_mark(reply: &str) -> String {
    let trimmed = reply.trim_start();
    if let Some(rest) = trimmed.strip_prefix("（回复") {
        if let Some(end) = rest
            .find('）')
            .filter(|end| rest[..*end].chars().count() <= 200)
        {
            return rest[end + '）'.len_utf8()..].trim_start().to_string();
        }
    }
    reply.to_string()
}

/// Her reply, chunk by chunk; whether any of it reached the group.
async fn deliver(line: &GroupLine, token: &str, reply: &str, began: Instant) -> bool {
    use crate::services::agent::merope::timing::typing;
    use myriad_agent_rules::channel::{as_messages, as_messages_at_most, split_channel_text};
    let venue = line.venue();
    let people = people(&venue);
    let room = room(&venue);
    let mut sent = false;
    // Most turns go as one message, and a few in a row when something grabs
    // her: typing each before it goes, typed the way people there type, and
    // only the first quoting the line she answers.
    let most = myriad_merope::talk_shape::messages_this_turn(
        as_messages(reply).len(),
        rand::random::<f64>(),
    );
    for (index, message) in as_messages_at_most(reply, most).into_iter().enumerate() {
        let message = match &room {
            Some(room) => myriad_merope::talk_shape::typed_like(&message, room),
            None => message,
        };
        // Typing it takes as long as it takes; the first she was already
        // at while she thought.
        let left = if index == 0 {
            typing(&message).saturating_sub(began.elapsed())
        } else {
            typing(&message)
        };
        if !left.is_zero() {
            send_typing(line, token).await;
            tokio::time::sleep(left).await;
        }
        for chunk in split_channel_text(&message, text_limit(line.platform)) {
            let pieces = myriad_agent_rules::mentions::split_mentions(&chunk, &people);
            // A line with no message behind it (her saying something first)
            // quotes nothing.
            match send_reply(line, token, &pieces, !sent && !line.message_id.is_empty()).await {
                Ok(()) => {
                    // How long whoever called her waited for her first words.
                    let waited = (!sent && line.addressed)
                        .then(|| said_ago(line))
                        .flatten()
                        .map(|age| age.num_milliseconds() as f64 / 1000.0);
                    let typed = myriad_merope::talk_shape::typed_by(
                        HER,
                        chrono::Utc::now().timestamp(),
                        &chunk,
                    );
                    note_said(&venue, typed, waited, Some(&chunk));
                    sent = true;
                }
                Err(kind) => {
                    warn!(?kind, venue = %line.venue(), "[Group] reply not sent");
                    keep_ledger(&venue).await;
                    return sent;
                }
            }
        }
    }
    keep_ledger(&venue).await;
    sent
}

async fn lookup(db: &DatabaseConnection, line: &GroupLine) -> Option<PairingLookup> {
    crate::services::channel_pairing::lookup_openid(
        db,
        PairingChannel::of(line.platform),
        &line.from,
    )
    .await
    .ok()
}

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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Line {
    at: chrono::DateTime<chrono::Utc>,
    message_id: Option<String>,
    name: String,
    /// Who said it, as the platform knows them: how she can mention them.
    #[serde(default)]
    from: Option<String>,
    text: String,
    hers: bool,
    /// Pictures in it, and what she saw of each once she looked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<GroupImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    seen: Vec<Option<myriad_merope::seeing::Seen>>,
}

impl Line {
    /// What it says, with its pictures as she saw them.
    fn said(&self) -> String {
        let pictures: Vec<String> = self
            .images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                myriad_merope::seeing::as_said(
                    self.seen.get(index).and_then(Option::as_ref),
                    image.hint.as_deref(),
                    image.sticker,
                )
            })
            .collect();
        [self.text.clone(), pictures.join("")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct StoredLines {
    lines: Vec<Line>,
}

/// Whether a line is still one she keeps in mind of the group.
fn within(line: &Line, window: Duration) -> bool {
    (chrono::Utc::now() - line.at)
        .to_std()
        .map_or(true, |age| age < window)
}

#[derive(Default)]
struct Group {
    lines: VecDeque<Line>,
    /// Whether the lines kept before a restart were brought back.
    restored: bool,
    busy: bool,
    last_reply: Option<Instant>,
    touched: Option<Instant>,
    /// The latest line that did not call her by name and she has not
    /// looked at yet, since when lines have gone unlooked at, and whether
    /// she is looking now.
    pending: Option<GroupLine>,
    unjudged_since: Option<Instant>,
    judging: bool,
    /// When she was last called there, and when she means to glance at it.
    called: Option<Instant>,
    glance_at: Option<Instant>,
    /// Her speaking up unasked there lately, oldest first: when, and whether
    /// anyone took it up.
    spoke_up: VecDeque<(Instant, bool)>,
    /// Whom she answered lately, in order, and whom she stopped answering
    /// (see `LOOP_ROUNDS`).
    answered: VecDeque<(String, Instant)>,
    paused: HashMap<String, Instant>,
    /// Lines that spoke to her while she was busy, oldest first.
    waiting: VecDeque<GroupLine>,
    /// Replies today to people outside the community: the day, and how many.
    stranger_replies: Option<(chrono::NaiveDate, u32)>,
    /// The last line she took in for what she heard (see `take_in`).
    heard_upto: Option<chrono::DateTime<chrono::Utc>>,
    /// How often each picture was sent there lately (by its key): one sent
    /// again is the group's (see `bits::picture_again`).
    pictures: HashMap<String, u32>,
    /// The latest line seen there and the token it came with: where she
    /// would say something first (see `share_first`).
    reach: Option<(GroupLine, String)>,
    /// Messages and waits not yet written to the group's ledger.
    ledger: Vec<myriad_merope::talk_shape::Typed>,
    ledger_waits: Vec<f64>,
    /// Pieces of their messages and hers not yet written to it.
    ledger_theirs: myriad_merope::contrast::Counts,
    ledger_hers: myriad_merope::contrast::Counts,
}

enum Turn {
    Began,
    Busy,
    Resting(Duration),
}

/// Count one more of today's, unless `limit` is reached.
fn count_today(slot: &mut Option<(chrono::NaiveDate, u32)>, limit: u32) -> bool {
    let today = chrono::Local::now().date_naive();
    let count = match *slot {
        Some((day, count)) if day == today => count,
        _ => 0,
    };
    if count >= limit {
        return false;
    }
    *slot = Some((today, count + 1));
    true
}

static GROUPS: LazyLock<Mutex<HashMap<String, Group>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The Chat session each sender has in each group, so their turns in that
/// group supersede only each other.
static SESSIONS: LazyLock<Mutex<HashMap<(String, i32), String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn with_group<T>(venue: &str, act: impl FnOnce(&mut Group) -> T) -> Option<T> {
    let mut groups = GROUPS.lock().ok()?;
    if !groups.contains_key(venue) && groups.len() >= MAX_GROUPS {
        if let Some(stalest) = groups
            .iter()
            .filter(|(_, group)| !group.busy)
            .min_by_key(|(_, group)| group.touched)
            .map(|(id, _)| id.clone())
        {
            groups.remove(&stalest);
        }
    }
    let group = groups.entry(venue.to_string()).or_default();
    group.touched = Some(Instant::now());
    Some(act(group))
}

fn push_line(group: &mut Group, line: Line) {
    group.lines.retain(|line| within(line, TRANSCRIPT_FOR));
    group.lines.push_back(line);
    while group.lines.len() > TRANSCRIPT_LINES {
        group.lines.pop_front();
    }
}

/// Lines kept from before a restart, merged under the ones heard since.
fn merge_restored(group: &mut Group, stored: Vec<Line>) {
    let mut lines: Vec<Line> = stored;
    for line in group.lines.drain(..) {
        let known = line.message_id.is_some()
            && lines.iter().any(|kept| kept.message_id == line.message_id);
        if !known {
            lines.push(line);
        }
    }
    lines.sort_by_key(|line| line.at);
    for line in lines {
        push_line(group, line);
    }
}

/// Keep a line in mind: in memory, and in the runtime registry so a restart
/// does not wipe the group from her mind. The first line after a restart
/// brings back what was kept before it.
async fn remember_line(venue: &str, line: Line) {
    let db = crate::services::process_db::database().ok();
    remember_line_on(db.as_ref(), venue, line).await;
}

async fn remember_line_on(db: Option<&DatabaseConnection>, venue: &str, line: Line) {
    if let Some(db) = db {
        restore(db, venue).await;
    }
    let lines = with_group(venue, |group| {
        push_line(group, line);
        group.lines.iter().cloned().collect::<Vec<_>>()
    });
    if let (Some(db), Some(lines)) = (db, lines) {
        keep_lines(db, venue, lines).await;
    }
}

/// Bring back the lines kept before a restart, once.
async fn restore(db: &DatabaseConnection, venue: &str) {
    if !with_group(venue, |group| !group.restored).unwrap_or(false) {
        return;
    }
    let stored = crate::services::runtime_registry::get::<StoredLines>(db, LINES_NAMESPACE, venue)
        .await
        .ok()
        .flatten();
    with_group(venue, |group| {
        if !group.restored {
            group.restored = true;
            if let Some(stored) = stored {
                merge_restored(group, stored.lines);
            }
        }
    });
}

async fn keep_lines(db: &DatabaseConnection, venue: &str, lines: Vec<Line>) {
    let keep_until = (chrono::Utc::now()
        + chrono::Duration::from_std(TRANSCRIPT_FOR).unwrap_or_default())
    .timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        db,
        LINES_NAMESPACE,
        venue,
        crate::services::runtime_registry::RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &StoredLines { lines },
        keep_until,
    )
    .await
    {
        warn!(%error, %venue, "[Group] could not keep the group's lines");
    }
}

/// What a group said while she was not connected (see
/// `onebot::decode::decode_group_history`), read back as a person scrolls up
/// on opening a chat: each line she does not have yet goes in at its time,
/// hers as hers. Nothing is answered: it is only read.
pub async fn catch_up(venue: &str, past: Vec<(GroupLine, chrono::DateTime<chrono::Utc>, bool)>) {
    let db = crate::services::process_db::database().ok();
    if let Some(db) = &db {
        restore(db, venue).await;
    }
    let people = people(venue);
    let fresh: Vec<Line> = with_group(venue, |group| {
        past.into_iter()
            .filter(|(message, _, _)| {
                !group
                    .lines
                    .iter()
                    .any(|line| line.message_id.as_deref() == Some(message.message_id.as_str()))
            })
            .map(|(message, at, hers)| Line {
                at,
                message_id: Some(message.message_id.clone()),
                name: if hers {
                    String::new()
                } else {
                    message.display_name.clone()
                },
                from: (!hers).then(|| message.from.clone()),
                text: bounded(&by_name(&message.said(), &people)),
                hers,
                images: if hers { Vec::new() } else { message.images },
                seen: Vec::new(),
            })
            .filter(|line| within(line, TRANSCRIPT_FOR))
            .collect()
    })
    .unwrap_or_default();
    if fresh.is_empty() {
        return;
    }
    info!(%venue, lines = fresh.len(), "[Group] read back what was said while she was away");
    for line in &fresh {
        let by = if line.hers {
            HER.to_string()
        } else {
            ledger_who(line.from.as_deref().unwrap_or_default())
        };
        let typed = myriad_merope::talk_shape::typed_by(&by, line.at.timestamp(), &line.text);
        note_said(venue, typed, None, Some(&line.text));
    }
    let lines = with_group(venue, |group| {
        merge_restored(group, fresh);
        group.lines.iter().cloned().collect::<Vec<_>>()
    });
    if let (Some(db), Some(lines)) = (&db, lines) {
        keep_lines(db, venue, lines).await;
    }
    keep_ledger(venue).await;
}

/// Groups of `platform` she has kept lines of lately.
pub async fn groups_lately(platform: ChannelPlatform) -> Vec<String> {
    let Ok(db) = crate::services::process_db::database() else {
        return Vec::new();
    };
    let prefix = format!("{}:", platform.slug());
    crate::services::runtime_registry::list(&db, LINES_NAMESPACE, None, None)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|row| row.record_id.strip_prefix(&prefix).map(str::to_string))
        .collect()
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_LINE_CHARS).collect()
}

/// Keep a group line in mind, whoever wrote it.
pub async fn record(message: &GroupLine) {
    let line = Line {
        at: chrono::Utc::now(),
        message_id: Some(message.message_id.clone()),
        name: message.display_name.clone(),
        from: Some(message.from.clone()),
        text: bounded(&by_name(&message.said(), &people(&message.venue()))),
        hers: false,
        images: message.images.clone(),
        seen: Vec::new(),
    };
    let venue = message.venue();
    remember_line(&venue, line).await;
    let typed = myriad_merope::talk_shape::typed_by(
        &ledger_who(&message.from),
        chrono::Utc::now().timestamp(),
        &message.text,
    );
    if note_said(&venue, typed, None, Some(&message.text)) {
        keep_ledger(&venue).await;
    }
    take_in(&venue);
    with_group(&venue, |group| {
        if group.pictures.len() > PICTURES_KEPT {
            group.pictures.clear();
        }
        for image in &message.images {
            *group.pictures.entry(image.key.clone()).or_default() += 1;
        }
    });
}

/// Once enough of the group's talk has gone by, take in what she heard in
/// it about things, on the site owner's account: she keeps it as her own,
/// with no name and no group on it.
fn take_in(venue: &str) {
    let stretch = with_group(venue, |group| {
        let upto = group.heard_upto;
        let fresh: Vec<&Line> = group
            .lines
            .iter()
            .filter(|line| upto.is_none_or(|upto| line.at > upto))
            .collect();
        let theirs = fresh.iter().filter(|line| !line.hers).count();
        let waited = fresh
            .first()
            .is_some_and(|line| chrono::Utc::now() - line.at >= HEARD_AFTER);
        if theirs < HEARD_EVERY && !(waited && theirs >= HEARD_AT_LEAST) {
            return None;
        }
        group.heard_upto = fresh.last().map(|line| line.at);
        Some(
            fresh
                .into_iter()
                .map(|line| myriad_merope::heard::Said {
                    name: line.name.clone(),
                    text: line.text.clone(),
                    hers: line.hers,
                })
                .collect::<Vec<_>>(),
        )
    })
    .flatten();
    let Some(stretch) = stretch else {
        return;
    };
    tokio::spawn(async move {
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        let Ok(owner) = crate::services::site_owner::site_owner_user_id(&db).await else {
            return;
        };
        crate::services::agent::merope::heard::take_in(&db, owner, stretch).await;
    });
}

async fn record_hers(venue: &str, text: &str) {
    let line = Line {
        at: chrono::Utc::now(),
        message_id: None,
        name: String::new(),
        from: None,
        text: bounded(text),
        hers: true,
        images: Vec::new(),
        seen: Vec::new(),
    };
    remember_line(venue, line).await;
}

/// Pictures she looks at when she reads a group's talk, at most at once.
const PICTURES_AT_ONCE: usize = 6;
/// Pictures a group's counts are kept for, at most.
const PICTURES_KEPT: usize = 512;
const PICTURE_BYTES: usize = 5 * 1024 * 1024;
const PICTURE_TIMEOUT: Duration = Duration::from_secs(20);

/// A picture's bytes, from where the platform keeps it.
async fn fetch_picture(token: &str, image: &GroupImage) -> Option<Vec<u8>> {
    match &image.fetch {
        ImageFetch::TelegramFile { file_id } => {
            crate::services::telegram_bot::download_file_bytes(token, file_id)
                .await
                .ok()
                .map(|(bytes, _)| bytes)
                .filter(|bytes| bytes.len() <= PICTURE_BYTES)
        }
        ImageFetch::Url { url } => {
            let fetched = crate::services::outbound_security::get_public_following_redirects(
                url,
                PICTURE_TIMEOUT,
                None,
            )
            .await
            .ok()?;
            if !fetched.response.status().is_success() {
                return None;
            }
            crate::services::outbound_security::read_limited_body(fetched.response, PICTURE_BYTES)
                .await
                .ok()
        }
    }
}

/// Look at the pictures in the group's recent talk she has not seen yet,
/// as a person reads back over what was sent. Billed to the site's owner,
/// who hosts her there.
async fn see(venue: &str, token: &str) {
    let unseen: Vec<(Option<String>, usize, GroupImage)> = with_group(venue, |group| {
        let lines = group.lines.len();
        group
            .lines
            .iter()
            .skip(lines.saturating_sub(CONVERSATION_LINES))
            .filter(|line| !line.hers)
            .flat_map(|line| {
                line.images
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| line.seen.get(*index).is_none_or(Option::is_none))
                    .map(|(index, image)| (line.message_id.clone(), index, image.clone()))
                    .collect::<Vec<_>>()
            })
            .take(PICTURES_AT_ONCE)
            .collect()
    })
    .unwrap_or_default();
    if unseen.is_empty() {
        return;
    }
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    let Ok(owner) = crate::services::site_owner::site_owner_user_id(&db).await else {
        return;
    };
    let db = &db;
    let looked = futures::future::join_all(unseen.into_iter().map(
        |(message_id, index, image)| async move {
            let seen = match crate::services::agent::merope::seeing::known(db, &image.key).await {
                Some(seen) => Some(seen),
                None => match fetch_picture(token, &image).await {
                    Some(bytes) => {
                        crate::services::agent::merope::seeing::look(
                            db,
                            owner,
                            &image.key,
                            bytes,
                            image.hint.as_deref(),
                        )
                        .await
                    }
                    None => None,
                },
            };
            (message_id, index, seen)
        },
    ))
    .await;
    let mut again = Vec::new();
    with_group(venue, |group| {
        for (message_id, index, seen) in looked {
            let Some(seen) = seen else {
                continue;
            };
            let key = group
                .lines
                .iter()
                .find(|line| !line.hers && line.message_id == message_id)
                .and_then(|line| line.images.get(index))
                .map(|image| image.key.clone());
            if let Some(key) = key
                && group.pictures.get(&key).is_some_and(|sent| *sent >= 2)
            {
                again.push((key, seen.clone()));
            }
            if let Some(line) = group
                .lines
                .iter_mut()
                .find(|line| !line.hers && line.message_id == message_id)
            {
                if line.seen.len() <= index {
                    line.seen.resize(index + 1, None);
                }
                line.seen[index] = Some(seen);
            }
        }
    });
    for (key, seen) in again {
        crate::services::agent::merope::bits::picture_again(db, owner, venue, &key, &seen).await;
    }
}

/// What this line says as she has it in mind, pictures as she saw them.
fn said_now(message: &GroupLine) -> String {
    with_group(&message.venue(), |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| !line.hers && line.message_id.as_deref() == Some(&message.message_id))
            .map(Line::said)
    })
    .flatten()
    .unwrap_or_else(|| message.said())
}

/// How long ago `message` was said, when that is long enough ago that she
/// knows she is only now seeing it.
fn seen_late(message: &GroupLine) -> Option<String> {
    said_ago(message)
        .filter(|age| *age >= LATE)
        .map(myriad_merope::doing::ago_text)
}

/// How long ago `message` was said in its group, while its line is kept.
fn said_ago(message: &GroupLine) -> Option<chrono::Duration> {
    with_group(&message.venue(), |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| !line.hers && line.message_id.as_deref() == Some(&message.message_id))
            .map(|line| chrono::Utc::now() - line.at)
    })
    .flatten()
}

/// `@123` (someone @-ed by their platform id, as QQ gives it without a
/// name) as `@name`, for whoever spoke in the group lately.
fn by_name(text: &str, people: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at + 1]);
        rest = &rest[at + 1..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let id = &rest[..digits];
        match people.iter().find(|(_, from)| from == id) {
            Some((name, _)) if !name.is_empty() => out.push_str(name),
            _ => out.push_str(id),
        }
        rest = &rest[digits..];
    }
    out.push_str(rest);
    out
}

/// Who she can mention in the group: whoever spoke there lately, by the
/// name they showed last.
fn people(venue: &str) -> Vec<(String, String)> {
    with_group(venue, |group| {
        let mut people: Vec<(String, String)> = Vec::new();
        for line in group
            .lines
            .iter()
            .rev()
            .filter(|line| within(line, TRANSCRIPT_FOR))
        {
            let Some(from) = line.from.as_ref().filter(|_| !line.hers) else {
                continue;
            };
            if !people.iter().any(|(_, id)| id == from) {
                people.push((line.name.clone(), from.clone()));
            }
        }
        people
    })
    .unwrap_or_default()
}

/// How the group's people type there lately (their lines, not hers), once
/// there is enough of it.
fn room(venue: &str) -> Option<myriad_merope::talk_shape::Shape> {
    with_group(venue, |group| {
        let lines: Vec<(&str, i64, &str)> = group
            .lines
            .iter()
            .filter(|line| !line.hers && within(line, TRANSCRIPT_FOR))
            .map(|line| {
                (
                    line.from.as_deref().unwrap_or(line.name.as_str()),
                    line.at.timestamp(),
                    line.text.as_str(),
                )
            })
            .collect();
        myriad_merope::talk_shape::room_of(&lines)
    })
    .flatten()
}

/// The group's recent lines before `message_id` (all of them without one),
/// oldest first. Others' lines carry their name; hers are her own turns.
fn transcript(venue: &str, message_id: Option<&str>) -> Vec<ConversationMessage> {
    with_group(venue, |group| {
        group
            .lines
            .iter()
            .filter(|line| within(line, TRANSCRIPT_FOR))
            .take_while(|line| message_id.is_none() || line.message_id.as_deref() != message_id)
            .map(|line| ConversationMessage {
                role: if line.hers { "assistant" } else { "user" }.into(),
                content: if line.hers {
                    line.text.clone()
                } else {
                    format!("{}：{}", line.name, line.said())
                },
                created_at: None,
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Runtime-registry namespace of each group's talk ledger (see `Ledger`).
const LEDGER_NAMESPACE: &str = "group_talk_ledger";
const LEDGER_MESSAGES: usize = 2000;
const LEDGER_WAITS: usize = 500;
const LEDGER_DAYS: i64 = 30;
/// Messages gathered before the ledger is written; her reply writes it too.
const LEDGER_EVERY: usize = 10;
/// Who she is in the ledger.
const HER: &str = "her";

/// How a group types, and how long she took there to answer whoever
/// called her, as numbers only: what the process layer is checked against
/// (see `myriad_merope::talk_shape::Typed`). Who is a hash, never an id;
/// nobody's words are kept.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct Ledger {
    messages: Vec<myriad_merope::talk_shape::Typed>,
    her_waits: Vec<f64>,
    /// Counts of the pieces their messages and hers are made of (see
    /// `myriad_merope::contrast`), never the messages.
    #[serde(default)]
    theirs: myriad_merope::contrast::Counts,
    #[serde(default)]
    hers: myriad_merope::contrast::Counts,
}

/// Messages of each side the piece counts stay within (older ones weigh
/// less once past it).
const LEDGER_COUNTED: u32 = 2000;

/// A member as the ledger knows them: the same token for the same person,
/// never their id.
fn ledger_who(from: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    from.hash(&mut hasher);
    format!("{:012x}", hasher.finish() & 0xffff_ffff_ffff)
}

/// Note a message typed there (counted into its pieces for whoever said
/// it), and how long she took if it was her first answer to someone who
/// called her; whether the ledger is due a write.
fn note_said(
    venue: &str,
    typed: Option<myriad_merope::talk_shape::Typed>,
    waited: Option<f64>,
    said: Option<&str>,
) -> bool {
    with_group(venue, |group| {
        if let (Some(typed), Some(said)) = (&typed, said) {
            if typed.by == HER {
                group.ledger_hers.add(said);
            } else {
                group.ledger_theirs.add(said);
            }
        }
        group.ledger.extend(typed);
        group.ledger_waits.extend(waited);
        group.ledger.len() >= LEDGER_EVERY || !group.ledger_waits.is_empty()
    })
    .unwrap_or(false)
}

/// Write what was noted to the group's ledger, keeping the latest.
async fn keep_ledger(venue: &str) {
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    let Some((messages, waits, theirs, hers)) = with_group(venue, |group| {
        (
            std::mem::take(&mut group.ledger),
            std::mem::take(&mut group.ledger_waits),
            std::mem::take(&mut group.ledger_theirs),
            std::mem::take(&mut group.ledger_hers),
        )
    }) else {
        return;
    };
    if messages.is_empty() && waits.is_empty() {
        return;
    }
    let mut ledger = crate::services::runtime_registry::get::<Ledger>(&db, LEDGER_NAMESPACE, venue)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    ledger.messages.extend(messages);
    let over = ledger.messages.len().saturating_sub(LEDGER_MESSAGES);
    ledger.messages.drain(..over);
    ledger.her_waits.extend(waits);
    let over = ledger.her_waits.len().saturating_sub(LEDGER_WAITS);
    ledger.her_waits.drain(..over);
    ledger.theirs.merge(&theirs);
    ledger.theirs.keep_within(LEDGER_COUNTED);
    ledger.hers.merge(&hers);
    ledger.hers.keep_within(LEDGER_COUNTED);
    let keep_until = (chrono::Utc::now() + chrono::Duration::days(LEDGER_DAYS)).timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        &db,
        LEDGER_NAMESPACE,
        venue,
        crate::services::runtime_registry::RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &ledger,
        keep_until,
    )
    .await
    {
        warn!(%error, %venue, "[Group] could not keep the talk ledger");
    }
}

/// How her lines differ from the people's here (see
/// `myriad_merope::contrast`): from the ledger and what is not yet written
/// to it; while the ledger has too little of theirs, from the lines kept.
async fn how_she_differs(venue: &str) -> Option<String> {
    use myriad_merope::contrast::{Counts, describe, overused};
    let (mut theirs, mut hers) = with_group(venue, |group| {
        (group.ledger_theirs.clone(), group.ledger_hers.clone())
    })?;
    if let Ok(db) = crate::services::process_db::database()
        && let Ok(Some(ledger)) =
            crate::services::runtime_registry::get::<Ledger>(&db, LEDGER_NAMESPACE, venue).await
    {
        theirs.merge(&ledger.theirs);
        hers.merge(&ledger.hers);
    }
    if theirs.messages < 20 {
        (theirs, hers) = with_group(venue, |group| {
            let kept = |hers: bool| {
                Counts::of(
                    group
                        .lines
                        .iter()
                        .filter(|line| line.hers == hers)
                        .flat_map(|line| line.text.lines()),
                )
            };
            (kept(false), kept(true))
        })?;
    }
    describe(&overused(&hers, &theirs))
}

/// Before answering `message`, she reads the talk: what is going on, what
/// it means, what people are telling her about herself (kept, see
/// `merope::making_sense`). With what the group told her before, as the
/// sections her reply starts from.
async fn make_sense(db: &DatabaseConnection, message: &GroupLine) -> Option<String> {
    use crate::services::agent::merope::making_sense;
    let venue = message.venue();
    let stored = unified_venue(&venue);
    let owner = crate::services::site_owner::site_owner_user_id(db)
        .await
        .ok()?;
    let lines = transcript(&venue, Some(&message.message_id));
    let said = said_now(message);
    let mut conversation: Vec<String> = lines
        .iter()
        .skip(lines.len().saturating_sub(CONVERSATION_LINES))
        .map(|line| {
            if line.role == "assistant" {
                format!("you：{}", line.content)
            } else {
                line.content.clone()
            }
        })
        .collect();
    conversation.push(format!("{}：{said}", message.display_name));
    let sense = making_sense::read(owner, &conversation).await;
    if let Some(told) = sense.as_ref().and_then(|sense| sense.about_you.as_deref()) {
        making_sense::remember_told(db, &stored, told, &said).await;
    }
    let sections: Vec<String> = [
        making_sense::told_section(db, &stored).await,
        sense.as_ref().map(myriad_merope::making_sense::section),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

/// A group's venue as memories store it (`group:telegram:-100123`).
fn unified_venue(venue: &str) -> String {
    crate::services::agent::memory::unified::Audience::group(venue, 0).venue()
}

/// Take the group's single turn, if it is free and not just replied in.
fn begin_turn(venue: &str) -> Turn {
    with_group(venue, begin).unwrap_or(Turn::Busy)
}

fn begin(group: &mut Group) -> Turn {
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
fn park(group: &mut Group, message: GroupLine) {
    group.waiting.push_back(message);
    while group.waiting.len() > WAITING_LINES {
        group.waiting.pop_front();
    }
}

fn end_turn(venue: &str, replied: bool) {
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
    use crate::services::agent::merope::timing;
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

async fn take_up(mut message: GroupLine, token: String) {
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
async fn finish_turn(venue: &str, replied: bool) -> Option<GroupLine> {
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

/// A line that did not call her by name. She reads a group the way a person
/// does. In talk she is in, she looks when it pauses (or, in talk that never
/// pauses, every so often). Otherwise she glances at the group now and then
/// when she is free, not at every line. Either way she judges, as herself,
/// whether to say something (see `merope::joining`). Nothing is held while
/// she waits.
pub fn notice(message: GroupLine, token: String) {
    let venue = message.venue();
    let id = message.message_id.clone();
    let wake = with_group(&venue, |group| {
        group.unjudged_since.get_or_insert_with(Instant::now);
        group.reach = Some((message.clone(), token.clone()));
        group.pending = Some(message);
        if in_talk(group) {
            Some(SETTLE)
        } else if group.glance_at.is_some() {
            None
        } else {
            let after = glance_after();
            group.glance_at = Some(Instant::now() + after);
            Some(after)
        }
    })
    .flatten();
    if let Some(after) = wake {
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            // A glance takes in whatever is latest by then.
            let id = with_group(&venue, |group| {
                if group.glance_at.is_some_and(|at| at <= Instant::now()) {
                    group.glance_at = None;
                    group.pending.as_ref().map(|line| line.message_id.clone())
                } else {
                    Some(id)
                }
            })
            .flatten();
            if let Some(id) = id {
                look(venue, id, token).await;
            }
        });
    }
}

/// Whether she is in the group's talk: she spoke there, or was called
/// there, lately.
fn in_talk(group: &Group) -> bool {
    group.called.is_some_and(|at| at.elapsed() < IN_TALK)
        || group
            .lines
            .iter()
            .rev()
            .find(|line| line.hers)
            .is_some_and(|line| {
                (chrono::Utc::now() - line.at)
                    .to_std()
                    .is_ok_and(|age| age < IN_TALK)
            })
}

/// How long until she glances at a group she is not in: once what she is
/// doing on her own is done (or she is up, if asleep), and then a while, as
/// a person looks at their phone.
fn glance_after() -> Duration {
    // Asleep, she looks once she is up.
    if let myriad_merope::timing::Where::Asleep { wakes_in } =
        crate::services::agent::merope::timing::where_she_is(false, true)
    {
        return Duration::from_secs_f64(wakes_in)
            + Duration::from_secs(rand::random_range(GLANCE_AFTER_SECONDS));
    }
    let free_in = crate::services::agent::merope::doing::current()
        .and_then(|doing| (doing.ends - chrono::Utc::now()).to_std().ok())
        .unwrap_or_default()
        .min(LONGEST_BUSY);
    free_in + Duration::from_secs(rand::random_range(GLANCE_AFTER_SECONDS))
}

enum Look {
    /// A newer line will be looked at instead, or nothing is waiting.
    Done,
    /// She is talking or looking already: again in a moment.
    Later,
    Now(Box<GroupLine>),
}

/// Look at the talk once it has settled on line `id`, or once it has gone
/// unlooked at too long; then at whatever came meanwhile.
async fn look(venue: String, mut id: String, token: String) {
    loop {
        let next = with_group(&venue, |group| {
            let Some(pending) = group.pending.as_ref() else {
                return Look::Done;
            };
            let latest = pending.message_id == id;
            let overdue = group
                .unjudged_since
                .is_some_and(|since| since.elapsed() >= MAX_WAIT);
            if !latest && !overdue {
                return Look::Done;
            }
            if group.judging || group.busy {
                return Look::Later;
            }
            group.judging = true;
            group.unjudged_since = None;
            group
                .pending
                .take()
                .map_or(Look::Done, |line| Look::Now(Box::new(line)))
        })
        .unwrap_or(Look::Done);
        let message = match next {
            Look::Done => return,
            Look::Later => {
                tokio::time::sleep(SETTLE).await;
                continue;
            }
            Look::Now(message) => *message,
        };
        let decided = judge(&message, &token).await;
        with_group(&venue, |group| {
            group.judging = false;
            // Going on with what she said, without calling her: taken up.
            if decided.as_ref().is_some_and(|(why, _)| *why == Why::Answer) {
                taken_up(group);
            }
        });
        if let Some((why, reason)) = decided
            && matches!(begin_turn(&venue), Turn::Began)
        {
            let replied = answer(&message, &token, Some(reason)).await;
            if replied && why != Why::Answer {
                info!(%venue, "[Group] she spoke up");
                with_group(&venue, |group| {
                    group.spoke_up.push_back((Instant::now(), false));
                    while group.spoke_up.len() > SPOKE_UP_KEPT {
                        group.spoke_up.pop_front();
                    }
                });
            }
            let mut next = finish_turn(&venue, replied).await;
            while let Some(message) = next {
                let replied = answer(&message, &token, None).await;
                next = finish_turn(&venue, replied).await;
            }
        }
        match with_group(&venue, |group| {
            group.pending.as_ref().map(|line| line.message_id.clone())
        })
        .flatten()
        {
            Some(newer) => id = newer,
            None => return,
        }
    }
}

/// Whether she says something about the talk this line ends, and why. Her
/// judgment is billed to the one who said it if they are of the community,
/// else to the site's owner, who hosts her there.
/// Someone turned to her: her latest speaking up there, if recent, was
/// taken up.
fn taken_up(group: &mut Group) {
    if let Some((at, taken)) = group.spoke_up.back_mut()
        && at.elapsed() < TAKEN_UP_WITHIN
    {
        *taken = true;
    }
}

async fn judge(message: &GroupLine, token: &str) -> Option<(Why, String)> {
    let db = crate::services::process_db::database().ok()?;
    if stopped_answering(message) {
        return None;
    }
    let owner = match lookup(&db, message).await {
        Some(PairingLookup::Paired { user_id }) => {
            current_binding(&db, message, user_id).await?;
            user_id
        }
        _ => crate::services::site_owner::site_owner_user_id(&db)
            .await
            .ok()?,
    };
    let venue = message.venue();
    see(&venue, token).await;
    let lines = transcript(&venue, None);
    let conversation: Vec<String> = lines
        .iter()
        .skip(lines.len().saturating_sub(CONVERSATION_LINES))
        .map(|line| {
            if line.role == "assistant" {
                format!("you：{}", line.content)
            } else {
                line.content.clone()
            }
        })
        .collect();
    // Talk she sees only a while after it was said.
    let late = with_group(&venue, |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| !line.hers)
            .map(|line| chrono::Utc::now() - line.at)
            .filter(|age| *age >= LATE)
            .map(myriad_merope::doing::ago_text)
    })
    .flatten();
    let last_spoke = with_group(&venue, |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| line.hers)
            .map(|line| myriad_merope::doing::ago_text(chrono::Utc::now() - line.at))
    })
    .flatten();
    let how_it_went = with_group(&venue, |group| {
        myriad_merope::joining::how_it_went(
            group.spoke_up.len(),
            group.spoke_up.iter().filter(|(_, taken)| *taken).count(),
        )
    })
    .flatten();
    crate::services::agent::merope::joining::decide(
        &db,
        owner,
        &conversation,
        &crate::services::agent::merope::joining::Here {
            last_spoke: last_spoke.as_deref(),
            late: late.as_deref(),
            how_it_went: how_it_went.as_deref(),
        },
    )
    .await
}

/// Whether she stopped answering whoever wrote this line (see `LOOP_ROUNDS`).
fn stopped_answering(message: &GroupLine) -> bool {
    with_group(&message.venue(), |group| {
        group.paused.retain(|_, since| since.elapsed() < LOOP_PAUSE);
        group.paused.contains_key(&message.from)
    })
    .unwrap_or(false)
}

/// She answered whoever wrote this line; after too many rounds with them
/// too fast, she stops answering them for a while.
fn answered(message: &GroupLine) {
    let venue = message.venue();
    with_group(&venue, |group| {
        group
            .answered
            .push_back((message.from.clone(), Instant::now()));
        while group.answered.len() > LOOP_ROUNDS {
            group.answered.pop_front();
        }
        let looping = group.answered.len() == LOOP_ROUNDS
            && group.answered.iter().all(|(from, _)| *from == message.from)
            && group
                .answered
                .front()
                .is_some_and(|(_, at)| at.elapsed() < LOOP_WINDOW);
        if looping {
            warn!(%venue, "[Group] answering one sender nonstop; she stops for a while");
            group.paused.insert(message.from.clone(), Instant::now());
            group.answered.clear();
        }
    });
}

async fn answer(message: &GroupLine, token: &str, chime: Option<String>) -> bool {
    let Ok(db) = crate::services::process_db::database() else {
        return false;
    };
    if stopped_answering(message) {
        return false;
    }
    let inbound_id = format!("group:{}:{}", message.chat, message.message_id);
    if !crate::services::channel_work::claim_inbound(&db, message.platform, None, &inbound_id).await
    {
        return false;
    }
    see(&message.venue(), token).await;
    let user_id = match lookup(&db, message).await {
        Some(PairingLookup::Paired { user_id }) => user_id,
        // Someone from outside the community: answered lightly.
        Some(_) => return answer_stranger(&db, message, token, chime.as_deref()).await,
        _ => return false,
    };
    let Some(binding) = current_binding(&db, message, user_id).await else {
        return false;
    };
    let began = Instant::now();
    let Some((reply, sticker)) = run_turn(&db, message, user_id, token, chime, false).await else {
        return false;
    };
    // Unpaired or switched off while she was thinking: say nothing.
    if !binding.is_current(&db).await {
        return false;
    }
    let reply = without_reply_mark(&reply);
    let sent = say_and_send(message, token, &reply, sticker, began).await;
    if sent {
        answered(message);
    }
    sent
}

/// Her words, if any, and the sticker she chose, if any: the sticker after
/// the words; one she is making, once it is made. Whether anything went.
async fn say_and_send(
    message: &GroupLine,
    token: &str,
    reply: &str,
    sticker: Option<serde_json::Value>,
    began: Instant,
) -> bool {
    let venue = message.venue();
    let mut sent = false;
    if !reply.trim().is_empty() {
        sent = deliver(message, token, reply, began).await;
        if sent {
            record_hers(&venue, reply).await;
        }
    }
    if let Some(chosen) = sticker {
        let (message, token) = (message.clone(), token.to_string());
        tokio::spawn(async move {
            send_sticker(&message, &token, &chosen).await;
        });
        sent = true;
    }
    sent
}

/// Send the sticker she chose into the group, making it first if it is new.
async fn send_sticker(message: &GroupLine, token: &str, chosen: &serde_json::Value) {
    use crate::services::agent::merope::stickers;
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    if chosen.get("make").is_some() {
        send_typing(message, token).await;
    }
    let Some(sticker) = stickers::resolve(&db, chosen, None).await else {
        return;
    };
    let Some((png, _)) = stickers::picture(&sticker).await else {
        return;
    };
    let Some(prepared) = crate::services::sticker_send::prepare(png, message.platform).await else {
        return;
    };
    let sent = match message.platform {
        ChannelPlatform::Telegram => crate::services::telegram_bot::send_sticker(
            token,
            &message.chat,
            &prepared.bytes,
            message.thread,
        )
        .await
        .is_ok(),
        ChannelPlatform::Discord => crate::services::discord_bot::send_photo(
            token,
            &message.chat,
            &prepared.bytes,
            prepared.mime,
            None,
        )
        .await
        .is_ok(),
        ChannelPlatform::OneBot => {
            use base64::Engine as _;
            let inline = format!(
                "base64://{}",
                base64::engine::general_purpose::STANDARD.encode(&prepared.bytes)
            );
            match myriad_agent_rules::onebot::encode::encode_group_message(
                &message.chat,
                &[myriad_agent_rules::onebot::encode::encode_image_segment(
                    &inline,
                )],
            ) {
                Some(action) => matches!(
                    crate::services::onebot_send::send_action(action).await,
                    Ok(None)
                ),
                None => false,
            }
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => false,
    };
    if sent {
        stickers::sent(&db, &sticker).await;
        // The group sees she sent it, and so does she.
        record_hers(
            &message.venue(),
            &format!("（表情包：{}）", sticker.meaning),
        )
        .await;
    } else {
        warn!(venue = %message.venue(), "[Group] sticker not sent");
    }
}

/// How long a group counts as one she is in, since she last saw a line
/// there.
const SEEN_WITHIN: Duration = Duration::from_secs(3 * 24 * 3600);
/// Lines of each group she looks over when deciding.
const SHARE_LINES: usize = 12;

/// Something of her own she would want to tell someone (`what`, as she took
/// it in): she looks over the groups she is in and, as herself, may bring
/// it up in one of them (see `merope::sharing`). Asleep she does not; a
/// group she is already talking in hears it in the talk. Billed to `owner`.
pub async fn share_first(owner: i32, what: String) {
    use crate::services::agent::merope::{bits, sharing, timing};
    if timing::asleep_now().is_some() {
        return;
    }
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    let now = chrono::Utc::now();
    let seen: Vec<(String, Vec<String>, String, Vec<serde_json::Value>)> = {
        let Ok(groups) = GROUPS.lock() else {
            return;
        };
        groups
            .iter()
            .filter(|(_, group)| !group.busy && !in_talk(group))
            .filter_map(|(venue, group)| {
                group.reach.as_ref()?;
                let last = group.lines.back();
                let quiet = last.map(|line| now - line.at);
                if quiet.is_some_and(|quiet| quiet.to_std().is_ok_and(|quiet| quiet > SEEN_WITHIN))
                {
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
                    Some(quiet) => myriad_merope::doing::ago_text(quiet),
                    None => "a good while (nothing said there lately)".to_string(),
                };
                let spoke_up = group
                    .spoke_up
                    .iter()
                    .map(|(at, taken)| {
                        let ago = chrono::Duration::from_std(at.elapsed()).unwrap_or_default();
                        serde_json::json!({
                            "ago": myriad_merope::doing::ago_text(ago),
                            "takenUp": taken,
                        })
                    })
                    .collect();
                Some((venue.clone(), lines, quiet_for, spoke_up))
            })
            .collect()
    };
    if seen.is_empty() {
        return;
    }
    let mut offered = Vec::with_capacity(seen.len());
    for (venue, lines, quiet_for, spoke_up_lately) in seen {
        let shared = bits::in_group(&db, &venue, 5)
            .await
            .into_iter()
            .map(|(handle, how)| format!("{handle}: {how}"))
            .collect();
        offered.push(sharing::Offered {
            id: venue,
            lines,
            quiet_for,
            bits: shared,
            spoke_up_lately,
        });
    }
    let Some((venue, why)) = sharing::choose(owner, &what, &offered).await else {
        return;
    };
    if !matches!(begin_turn(&venue), Turn::Began) {
        return;
    }
    let spoke = share_turn(&db, &venue, &what, &why).await;
    if spoke {
        info!(%venue, "[Group] she brought something of hers up");
        with_group(&venue, |group| {
            group.spoke_up.push_back((Instant::now(), false));
            while group.spoke_up.len() > SPOKE_UP_KEPT {
                group.spoke_up.pop_front();
            }
        });
    }
    let mut next = finish_turn(&venue, spoke).await;
    while let Some(message) = next {
        let token = with_group(&venue, |group| {
            group.reach.as_ref().map(|(_, token)| token.clone())
        })
        .flatten()
        .unwrap_or_default();
        let replied = answer(&message, &token, None).await;
        next = finish_turn(&venue, replied).await;
    }
}

/// Say it in the group, as a turn of her own on the site owner's budget: no
/// one's line behind it, so nothing quoted and nothing to answer.
async fn share_turn(db: &DatabaseConnection, venue: &str, what: &str, why: &str) -> bool {
    let Some((seen, token)) = with_group(venue, |group| group.reach.clone()).flatten() else {
        return false;
    };
    let Ok(owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return false;
    };
    let inbound_id = format!(
        "group:{}:first:{}",
        seen.chat,
        chrono::Utc::now().timestamp_millis()
    );
    if !crate::services::channel_work::claim_inbound(db, seen.platform, None, &inbound_id).await {
        return false;
    }
    let line = GroupLine {
        message_id: String::new(),
        from: String::new(),
        display_name: String::new(),
        text: myriad_merope::sharing::NOBODY_SAID.to_string(),
        addressed: false,
        reply_to: None,
        images: Vec::new(),
        ..seen
    };
    let began = Instant::now();
    let reason = myriad_merope::sharing::reason(what, why);
    let Some((reply, sticker)) = run_turn(db, &line, owner, &token, Some(reason), true).await
    else {
        return false;
    };
    let reply = without_reply_mark(&reply);
    say_and_send(&line, &token, &reply, sticker, began).await
}

/// Answer someone from outside the community, with little context, on the
/// site owner's budget. `why` is why she speaks when they did not call her.
async fn answer_stranger(
    db: &DatabaseConnection,
    message: &GroupLine,
    token: &str,
    why: Option<&str>,
) -> bool {
    let venue = message.venue();
    let within = with_group(&venue, |group| {
        count_today(&mut group.stranger_replies, STRANGER_REPLIES_PER_DAY)
    })
    .unwrap_or(false);
    if !within {
        info!(%venue, "[Group] enough replies to outsiders today");
        return false;
    }
    let Ok(owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return false;
    };
    let stranger = crate::services::agent::merope::strangers::Stranger {
        who: format!("{}:{}", message.platform.slug(), message.from),
        name: message.display_name.chars().take(40).collect(),
    };
    let reading: Vec<String> = [
        room(&venue).map(|room| myriad_merope::talk_shape::describe(&room, "How people type here")),
        how_she_differs(&venue).await,
        make_sense(db, message).await,
    ]
    .into_iter()
    .flatten()
    .collect();
    send_typing(message, token).await;
    let began = Instant::now();
    let transcript = transcript(&venue, Some(&message.message_id));
    let Ok(Some((reply, sticker))) = tokio::time::timeout(
        TURN_DEADLINE,
        crate::services::agent::merope::strangers::reply(
            db,
            owner,
            &venue,
            &stranger,
            &transcript,
            &said_now(message),
            why,
            &reading,
        ),
    )
    .await
    else {
        return false;
    };
    let reply = without_reply_mark(&reply);
    let sent = say_and_send(message, token, &reply, sticker, began).await;
    if sent {
        answered(message);
        crate::services::agent::merope::strangers::enqueue_after(
            db,
            owner,
            venue,
            stranger,
            said_now(message),
            reply,
            &message.message_id,
        )
        .await;
    }
    sent
}

async fn current_binding(
    db: &DatabaseConnection,
    message: &GroupLine,
    user_id: i32,
) -> Option<ChannelBinding> {
    let binding = ChannelBinding::resolve(db, message.platform, user_id, &message.from)
        .await
        .ok()
        .flatten()?;
    binding.is_current(db).await.then_some(binding)
}

async fn run_turn(
    db: &DatabaseConnection,
    message: &GroupLine,
    user_id: i32,
    token: &str,
    chime: Option<String>,
    first: bool,
) -> Option<(String, Option<serde_json::Value>)> {
    crate::services::principal::current_roles(db, user_id)
        .await
        .ok()??;
    let venue = message.venue();
    let key = (venue.clone(), user_id);
    let known = SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&key).cloned());
    // Made as the group's from the start: never read back as a private one.
    let session_id = crate::services::agent::sessions::ensure_session_in(
        db,
        known.as_deref(),
        user_id,
        AgentInteractionMode::Chat,
        Some(&venue),
    )
    .await
    .ok()?;
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.insert(key, session_id.clone());
    }
    let run = crate::services::agent::run::start_for_user(
        db.clone(),
        user_id,
        crate::services::agent::run::ProcessRequest {
            input: said_now(message),
            context: Some(crate::services::agent::run::ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: Some(session_id),
                group: Some(crate::services::agent::run::GroupTurn {
                    transcript: transcript(&venue, (!first).then_some(message.message_id.as_str())),
                    room: room(&venue),
                    differs: how_she_differs(&venue).await,
                    // Saying something first, there is no line of theirs
                    // to make sense of or to be late for.
                    making_sense: if first {
                        None
                    } else {
                        make_sense(db, message).await
                    },
                    late: if first { None } else { seen_late(message) },
                    venue,
                    chime,
                    speaker: message.display_name.clone(),
                }),
                ..Default::default()
            }),
        },
    )
    .await
    .ok()?;
    let mut events = Box::pin(crate::services::agent::run::agent_run_envelopes(run));
    let mut typing = tokio::time::interval(TYPING_EVERY);
    let deadline = tokio::time::sleep(TURN_DEADLINE);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return None,
            _ = typing.tick() => send_typing(message, token).await,
            envelope = events.next() => {
                match envelope?.event {
                    AgentProgressEvent::TaskCompleted { success, response, .. } => {
                        // A superseded or failed turn says nothing in the group.
                        if !success {
                            return None;
                        }
                        let text = response
                            .get("message")
                            .and_then(|value| value.as_str())
                            .map(str::trim)
                            .unwrap_or_default()
                            .to_string();
                        let sticker = response.pointer("/data/sticker").cloned();
                        return (!text.is_empty() || sticker.is_some()).then_some((text, sticker));
                    }
                    AgentProgressEvent::Error { .. } => return None,
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(chat_id: i64, message_id: i64, name: &str, text: &str) -> GroupLine {
        GroupLine::from(TelegramGroupMessage {
            update_id: message_id,
            message_id,
            chat_id,
            message_thread_id: None,
            from_id: 1,
            display_name: name.into(),
            text: text.into(),
            addressed: false,
            reply_to: None,
            images: Vec::new(),
        })
    }

    fn venue(chat_id: i64) -> String {
        format!("telegram:{chat_id}")
    }

    #[tokio::test]
    async fn the_group_transcript_is_the_lines_before_the_one_she_answers() {
        let chat = -9_001;
        record(&line(chat, 1, "阿明", "周五聚餐吗")).await;
        record(&line(chat, 2, "小红", "我可以")).await;
        record_hers(&venue(chat), "我在屏幕里，就不去了，你们吃好").await;
        record(&line(chat, 3, "阿明", "@bot 你推荐哪家")).await;
        let lines: Vec<(String, String)> = transcript(&venue(chat), Some("3"))
            .into_iter()
            .map(|message| (message.role, message.content))
            .collect();
        assert_eq!(
            lines,
            vec![
                ("user".into(), "阿明：周五聚餐吗".into()),
                ("user".into(), "小红：我可以".into()),
                ("assistant".into(), "我在屏幕里，就不去了，你们吃好".into()),
            ]
        );
    }

    #[tokio::test]
    async fn she_can_mention_whoever_spoke_there_lately_by_their_last_name() {
        let chat = -9_471;
        let said = |id: i64, from: i64, name: &str| {
            GroupLine::from(TelegramGroupMessage {
                update_id: id,
                message_id: id,
                chat_id: chat,
                message_thread_id: None,
                from_id: from,
                display_name: name.into(),
                text: "嗯嗯".into(),
                addressed: false,
                reply_to: None,
                images: Vec::new(),
            })
        };
        record(&said(1, 11, "阿明")).await;
        record(&said(2, 12, "小红")).await;
        record_hers(&venue(chat), "@阿明 你好").await;
        record(&said(3, 11, "阿明同学")).await;
        assert_eq!(
            people(&venue(chat)),
            vec![
                ("阿明同学".to_string(), "11".to_string()),
                ("小红".to_string(), "12".to_string())
            ]
        );
    }

    #[test]
    fn a_group_gets_one_turn_at_a_time_and_a_pause_after_replying() {
        let chat = -9_002;
        assert!(matches!(begin_turn(&venue(chat)), Turn::Began));
        assert!(
            matches!(begin_turn(&venue(chat)), Turn::Busy),
            "one turn at a time"
        );
        end_turn(&venue(chat), true);
        assert!(
            matches!(begin_turn(&venue(chat)), Turn::Resting(_)),
            "a short pause after a reply"
        );
        let other = -9_003;
        assert!(matches!(begin_turn(&venue(other)), Turn::Began));
        end_turn(&venue(other), false);
        assert!(
            matches!(begin_turn(&venue(other)), Turn::Began),
            "no reply, no pause"
        );
        end_turn(&venue(other), false);
    }

    #[tokio::test]
    async fn she_looks_once_the_talk_settles_on_its_latest_line() {
        let chat = -9_472;
        let later = line(chat, 2, "阿明", "你们说呢");
        notice(
            line(chat, 1, "阿明", "有人听过 amazarashi 吗"),
            String::new(),
        );
        notice(later, String::new());
        with_group(&venue(chat), |group| {
            assert_eq!(
                group.pending.as_ref().map(|line| line.message_id.as_str()),
                Some("2")
            );
            assert!(group.unjudged_since.is_some());
        });
    }

    #[tokio::test]
    async fn she_follows_talk_she_is_in_and_glances_at_the_rest_now_and_then() {
        let chat = -9_474;
        record(&line(chat, 1, "阿明", "周五聚餐吗")).await;
        with_group(&venue(chat), |group| assert!(!in_talk(group)));
        // Not in the talk: one glance is planned, not one per line.
        notice(line(chat, 1, "阿明", "周五聚餐吗"), String::new());
        let planned = with_group(&venue(chat), |group| group.glance_at).flatten();
        // Busy at most so long first, or asleep until she is up.
        let first = match crate::services::agent::merope::timing::where_she_is(false, true) {
            myriad_merope::timing::Where::Asleep { wakes_in } => {
                Duration::from_secs_f64(wakes_in + 60.0)
            }
            _ => LONGEST_BUSY,
        };
        assert!(planned.is_some_and(|at| {
            let wait = at - Instant::now();
            wait <= Duration::from_secs(*GLANCE_AFTER_SECONDS.end()) + first
        }));
        notice(line(chat, 2, "小红", "可以"), String::new());
        assert_eq!(
            with_group(&venue(chat), |group| group.glance_at).flatten(),
            planned
        );
        // She said something there: she is in the talk now.
        record_hers(&venue(chat), "我在屏幕里，你们吃好").await;
        with_group(&venue(chat), |group| assert!(in_talk(group)));
        let other = -9_475;
        with_group(&venue(other), |group| group.called = Some(Instant::now()));
        with_group(&venue(other), |group| assert!(in_talk(group)));
    }

    #[tokio::test]
    async fn pictures_read_as_she_saw_them_and_one_sent_again_is_counted() {
        let chat = -9_476;
        let picture = |id: i64| {
            let mut line = line(chat, id, "阿明", "");
            line.images = vec![GroupImage {
                key: "telegram:cat".into(),
                fetch: ImageFetch::TelegramFile {
                    file_id: "f".into(),
                },
                hint: Some("😂".into()),
                sticker: true,
            }];
            line
        };
        record(&picture(1)).await;
        record(&line(chat, 2, "小红", "哈哈哈")).await;
        record(&picture(3)).await;
        let lines: Vec<String> = transcript(&venue(chat), None)
            .into_iter()
            .map(|line| line.content)
            .collect();
        assert_eq!(
            lines[0], "阿明：[表情：😂]",
            "not looked at yet: what the app calls it"
        );
        with_group(&venue(chat), |group| {
            assert_eq!(group.pictures.get("telegram:cat"), Some(&2));
            group.lines[0].seen = vec![Some(myriad_merope::seeing::Seen {
                what: "一只翻白眼的猫".into(),
                says: Some("无语".into()),
            })];
        });
        assert_eq!(
            transcript(&venue(chat), None)[0].content,
            "阿明：[表情：一只翻白眼的猫（无语）]"
        );
        assert_eq!(said_now(&picture(3)), "[表情：😂]");
    }

    /// Replayed from the QQ group's own traffic on 2026-09-28 (NapCat's log of
    /// it): a member @-ing another, by number only and with a name.
    #[tokio::test]
    async fn a_real_qq_line_at_someone_else_reads_as_who() {
        let wire = |at: &str| {
            format!(
                r#"{{"post_type":"message","message_type":"group","group_id":1076198,"user_id":3059342645,"self_id":3264977935,"message_id":7,
                "sender":{{"card":"梦想成为猪侯王的leaphy"}},
                "message":[{{"type":"text","data":{{"text":"你去看ave mujika "}}}},{at}]}}"#
            )
        };
        let spoke = GroupLine::from(
            myriad_agent_rules::onebot::decode::decode_group_inbound(
                r#"{"post_type":"message","message_type":"group","group_id":1076198,"user_id":798494815,"self_id":3264977935,"message_id":6,
                "sender":{"card":"染川 瞳"},"message":[{"type":"text","data":{"text":"所以乐奈是什么意思"}}]}"#,
                3264977935,
            )
            .unwrap(),
        );
        let venue = spoke.venue();
        record(&spoke).await;
        for at in [
            r#"{"type":"at","data":{"qq":"798494815"}}"#,
            r#"{"type":"at","data":{"qq":"798494815","name":"染川 瞳"}}"#,
        ] {
            let line = GroupLine::from(
                myriad_agent_rules::onebot::decode::decode_group_inbound(&wire(at), 3264977935)
                    .unwrap(),
            );
            assert!(!line.addressed, "someone else @-ed, not her");
            record(&line).await;
            let last = transcript(&venue, None).last().unwrap().content.clone();
            assert_eq!(last, "梦想成为猪侯王的leaphy：你去看ave mujika @染川 瞳");
        }
    }

    #[test]
    fn someone_at_by_their_id_reads_as_their_name_when_known() {
        let people = vec![("小红".to_string(), "111".to_string())];
        assert_eq!(
            by_name("@111 你看 @1112 @222", &people),
            "@小红 你看 @1112 @222"
        );
        assert_eq!(by_name("邮箱 a@b.c", &people), "邮箱 a@b.c");
    }

    #[test]
    fn her_speaking_up_counts_as_taken_up_when_someone_turns_to_her_soon() {
        let mut group = Group::default();
        taken_up(&mut group);
        assert!(group.spoke_up.is_empty());
        group
            .spoke_up
            .push_back((Instant::now() - TAKEN_UP_WITHIN, false));
        taken_up(&mut group);
        assert_eq!(group.spoke_up.back().map(|(_, taken)| *taken), Some(false));
        group.spoke_up.push_back((Instant::now(), false));
        taken_up(&mut group);
        assert_eq!(
            group
                .spoke_up
                .iter()
                .map(|(_, taken)| *taken)
                .collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn she_stops_answering_one_who_never_stops_but_not_a_person() {
        let chat = -9_473;
        let from = |id: i64, from: i64, name: &str| {
            GroupLine::from(TelegramGroupMessage {
                update_id: id,
                message_id: id,
                chat_id: chat,
                message_thread_id: None,
                from_id: from,
                display_name: name.into(),
                text: "在吗".into(),
                addressed: false,
                reply_to: None,
                images: Vec::new(),
            })
        };
        let bot = from(1, 77, "复读机");
        for _ in 0..LOOP_ROUNDS - 1 {
            answered(&bot);
        }
        assert!(!stopped_answering(&bot));
        answered(&from(2, 11, "阿明"));
        for _ in 0..LOOP_ROUNDS - 1 {
            answered(&bot);
        }
        assert!(!stopped_answering(&bot), "someone else came between");
        answered(&bot);
        assert!(stopped_answering(&bot));
    }

    #[test]
    fn a_line_that_comes_while_she_is_busy_waits_for_her() {
        let chat = -9_007;
        assert!(matches!(begin_turn(&venue(chat)), Turn::Began));
        assert!(matches!(begin_turn(&venue(chat)), Turn::Busy));
        with_group(&venue(chat), |group| {
            for id in 1..=7 {
                park(group, line(chat, id, "阿明", &format!("第{id}问")));
            }
        });
        end_turn(&venue(chat), true);
        assert!(matches!(begin_turn(&venue(chat)), Turn::Resting(_)));
        // Questions that came while she was busy are answered in order; past
        // a few, the oldest go.
        let waiting: Vec<String> = with_group(&venue(chat), |group| {
            group.waiting.iter().map(|line| line.text.clone()).collect()
        })
        .unwrap();
        assert_eq!(waiting, ["第3问", "第4问", "第5问", "第6问", "第7问"]);
        let mut today = None;
        for _ in 0..3 {
            assert!(count_today(&mut today, 3));
        }
        assert!(!count_today(&mut today, 3));
    }

    #[test]
    fn a_group_is_its_venue_on_every_platform() {
        let telegram = line(-100123, 7, "阿明", "在吗");
        assert_eq!(telegram.venue(), "telegram:-100123");
        assert_eq!(telegram.message_id, "7");
        let discord = GroupLine::from(DiscordGroupMessage {
            message_id: "11".into(),
            channel_id: "22".into(),
            guild_id: "33".into(),
            author_id: "44".into(),
            display_name: "阿明".into(),
            text: "说到一半怎么没了".into(),
            addressed: true,
            reply_to: Some(QuotedLine {
                name: "若泉".into(),
                text: "听完要是".into(),
                hers: true,
            }),
            images: Vec::new(),
        });
        assert_eq!(discord.said(), "（回复你说的：听完要是）说到一半怎么没了");
        assert_eq!(discord.venue(), "discord:22");
        assert_eq!(text_limit(ChannelPlatform::Discord), 2000);
    }

    /// A restart does not wipe the group from her mind: what was kept comes
    /// back from the runtime registry with the first line after it.
    #[tokio::test]
    async fn a_restart_keeps_what_the_group_said() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "group_lines").await;
        let db = &schema.db;
        let chat = -9_300;
        let venue = venue(chat);
        let said = |id: i64, text: &str| Line {
            at: chrono::Utc::now(),
            message_id: Some(id.to_string()),
            name: "阿明".into(),
            from: Some("1".into()),
            text: text.into(),
            hers: false,
            images: Vec::new(),
            seen: Vec::new(),
        };
        remember_line_on(Some(db), &venue, said(1, "来点歌")).await;
        remember_line_on(Some(db), &venue, said(2, "放一首")).await;
        // The process restarts: nothing of the group is left in memory.
        if let Ok(mut groups) = GROUPS.lock() {
            groups.remove(&venue);
        }
        remember_line_on(Some(db), &venue, said(3, "说到一半怎么没了")).await;
        let lines: Vec<String> = transcript(&venue, None)
            .into_iter()
            .map(|line| line.content)
            .collect();
        assert_eq!(
            lines,
            ["阿明：来点歌", "阿明：放一首", "阿明：说到一半怎么没了"]
        );
        schema.drop().await;
    }

    /// After a restart the group comes back as it was, under what was heard
    /// since, each line once.
    #[test]
    fn lines_kept_before_a_restart_come_back_under_newer_ones() {
        let at = |minutes: i64| chrono::Utc::now() - chrono::Duration::minutes(minutes);
        let kept = |id: &str, minutes: i64, text: &str| Line {
            at: at(minutes),
            message_id: Some(id.into()),
            name: "阿明".into(),
            from: Some("1".into()),
            text: text.into(),
            hers: false,
            images: Vec::new(),
            seen: Vec::new(),
        };
        let mut group = Group::default();
        push_line(&mut group, kept("9", 1, "说到一半怎么没了"));
        merge_restored(
            &mut group,
            vec![
                kept("7", 30, "来点歌"),
                Line {
                    at: at(29),
                    message_id: None,
                    name: String::new(),
                    from: None,
                    text: "放就放，听完要是".into(),
                    hers: true,
                    images: Vec::new(),
                    seen: Vec::new(),
                },
                kept("9", 1, "说到一半怎么没了"),
                kept("1", 7 * 60, "太久以前的话"),
            ],
        );
        let texts: Vec<&str> = group.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["来点歌", "放就放，听完要是", "说到一半怎么没了"]);
        let stored = serde_json::to_string(&StoredLines {
            lines: group.lines.iter().cloned().collect(),
        })
        .unwrap();
        let back: StoredLines = serde_json::from_str(&stored).unwrap();
        assert_eq!(back.lines.len(), 3);
    }

    #[test]
    fn she_does_not_echo_the_reply_mark() {
        assert_eq!(
            without_reply_mark("（回复 阿明）谁是你宝宝！"),
            "谁是你宝宝！"
        );
        assert_eq!(without_reply_mark("（回复你说的：听完要是）断了"), "断了");
        assert_eq!(
            without_reply_mark("谁暴躁了（回复一下）"),
            "谁暴躁了（回复一下）"
        );
    }

    #[tokio::test]
    async fn a_line_she_sees_hours_later_she_knows_she_sees_late() {
        let chat = -9_005;
        let asked = line(chat, 1, "阿明", "@bot 在吗");
        record(&asked).await;
        assert_eq!(seen_late(&asked), None);
        with_group(&venue(chat), |group| {
            group.lines[0].at = chrono::Utc::now() - chrono::Duration::hours(7);
        });
        assert_eq!(seen_late(&asked).as_deref(), Some("7 hours ago"));
    }

    #[tokio::test]
    async fn the_room_is_how_the_others_type_not_her() {
        let chat = -9_006;
        for index in 0..myriad_merope::talk_shape::ROOM_AT_LEAST as i64 {
            record(&line(chat, index, "阿明", "哈哈哈")).await;
            record_hers(&venue(chat), "这句话很长很长很长很长很长很长！").await;
        }
        let room = room(&venue(chat)).unwrap();
        assert_eq!(room.messages, myriad_merope::talk_shape::ROOM_AT_LEAST);
        assert_eq!(room.chars_median, 3);
        assert_eq!(room.bang, 0.0);
    }

    #[tokio::test]
    async fn the_ledger_keeps_numbers_not_words_or_ids() {
        let chat = -9_007;
        let venue = venue(chat);
        for index in 0..(LEDGER_EVERY as i64 - 1) {
            record(&line(chat, index, "阿明", "周五聚餐吗")).await;
        }
        record(&line(chat, 99, "阿明", "[图片]")).await;
        let kept = with_group(&venue, |group| group.ledger.clone()).unwrap();
        assert_eq!(kept.len(), LEDGER_EVERY - 1);
        let first = &kept[0];
        assert_eq!(first.chars, 5);
        assert!(!first.mark && !first.bang);
        // The member is a token, the same each time, and not their id.
        assert_eq!(first.by, ledger_who("1"));
        assert_ne!(first.by, "1");
        assert!(kept.iter().all(|typed| typed.by == first.by));
        let json = serde_json::to_string(&kept).unwrap();
        assert!(!json.contains("聚餐"));
        // Her first words to someone who called her are due a write at once.
        assert!(note_said(
            &venue,
            myriad_merope::talk_shape::typed_by(HER, 0, "在"),
            Some(12.0),
            Some("在")
        ));
        let hers = with_group(&venue, |group| group.ledger_hers.clone()).unwrap();
        let theirs = with_group(&venue, |group| group.ledger_theirs.clone()).unwrap();
        assert_eq!(
            (hers.messages, theirs.messages),
            (1, LEDGER_EVERY as u32 - 1)
        );
        assert!(theirs.pieces.contains_key("end:餐吗"));
    }

    /// How she talks in each group against its members, from the ledgers in
    /// the site database: `MEROPE_TALK_REPORT=1 DATABASE_URL=… cargo test
    /// -p myriad-backend --bin myriad-backend -- --ignored
    /// how_she_talks_against_the_members --nocapture`.
    #[tokio::test]
    #[ignore = "reads the site database; MEROPE_TALK_REPORT=1 with DATABASE_URL"]
    async fn how_she_talks_against_the_members() {
        use myriad_merope::talk_shape::{out_of_line, shape_of, waits};
        assert_eq!(std::env::var("MEROPE_TALK_REPORT").as_deref(), Ok("1"));
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let db = sea_orm::Database::connect(url).await.expect("database");
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/merope/talk-reference.json"))
                .unwrap();
        let rows = crate::services::runtime_registry::list(&db, LEDGER_NAMESPACE, None, None)
            .await
            .expect("ledgers");
        println!("ledgers: {}", rows.len());
        for row in rows {
            let Ok(ledger) = serde_json::from_value::<Ledger>(row.payload) else {
                continue;
            };
            let (hers, members): (Vec<_>, Vec<_>) = ledger
                .messages
                .into_iter()
                .partition(|typed| typed.by == HER);
            let members = shape_of(&members);
            let hers = shape_of(&hers);
            println!("\n{}", row.record_id);
            println!("  members: {members:?}");
            println!("  hers:    {hers:?}");
            if let (Some(hers), Some(members)) = (&hers, &members) {
                println!("  out of line: {:?}", out_of_line(hers, members));
            }
            println!(
                "  she answered a call after (s): {:?}; members in the reference group: {}",
                waits(&ledger.her_waits),
                reference["chatApp"]["answerToAt"]
            );
        }
    }

    #[tokio::test]
    async fn what_was_said_while_she_was_away_is_read_back_in_its_place() {
        let chat = -9_008;
        let venue = venue(chat);
        record(&line(chat, 5, "瞳", "@bot 宝宝")).await;
        let now = chrono::Utc::now();
        let ago = |minutes: i64| now - chrono::Duration::minutes(minutes);
        catch_up(
            &venue,
            vec![
                (line(chat, 3, "leaphy", "宝宝，晚安喵"), ago(3), false),
                (line(chat, 4, "", "晚安"), ago(2), true),
                // Already had, and long gone: neither is taken again.
                (line(chat, 5, "瞳", "@bot 宝宝"), ago(1), false),
                (line(chat, 1, "某人", "昨天的事"), ago(60 * 24), false),
            ],
        )
        .await;
        let lines: Vec<(String, String)> = transcript(&venue, None)
            .into_iter()
            .map(|message| (message.role, message.content))
            .collect();
        assert_eq!(
            lines,
            vec![
                ("user".into(), "leaphy：宝宝，晚安喵".into()),
                ("assistant".into(), "晚安".into()),
                ("user".into(), "瞳：@bot 宝宝".into()),
            ]
        );
        // Read back, not answered: nothing is waiting for her.
        assert!(
            with_group(&venue, |group| group.waiting.is_empty()
                && group.pending.is_none())
            .unwrap()
        );
    }

    #[tokio::test]
    async fn a_group_keeps_only_its_recent_lines() {
        let chat = -9_004;
        for index in 0..(TRANSCRIPT_LINES as i64 + 5) {
            record(&line(chat, index, "某人", &format!("第{index}句"))).await;
        }
        let lines = transcript(&venue(chat), None);
        assert_eq!(lines.len(), TRANSCRIPT_LINES);
        assert_eq!(lines[0].content, "某人：第5句");
    }
}
