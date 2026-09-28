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
    ConnectFailureKind, DiscordGroupMessage, PairingLookup, QuotedLine, TelegramGroupMessage,
};
use sea_orm::DatabaseConnection;
use tracing::{info, warn};

use crate::services::agent::AgentInteractionMode;
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
        }
    }
}

/// Deliver one chunk of her reply in the group, as a reply to `line`, with
/// her `@name`s as the platform's mentions.
async fn send_reply(
    line: &GroupLine,
    token: &str,
    pieces: &[myriad_agent_rules::mentions::Piece],
) -> Result<(), ConnectFailureKind> {
    use myriad_agent_rules::mentions;
    match line.platform {
        ChannelPlatform::Telegram => {
            let (Ok(chat), Ok(message_id)) = (line.chat.parse(), line.message_id.parse()) else {
                return Err(ConnectFailureKind::Permanent);
            };
            let (text, entities) = mentions::telegram_text_and_entities(pieces);
            crate::services::telegram_bot::send_group_reply(
                token,
                chat,
                &text,
                &entities,
                message_id,
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
                &line.message_id,
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
async fn deliver(line: &GroupLine, token: &str, reply: &str) -> bool {
    let people = people(&line.venue());
    let mut sent = false;
    for chunk in myriad_agent_rules::channel::split_channel_text(reply, text_limit(line.platform)) {
        let pieces = myriad_agent_rules::mentions::split_mentions(&chunk, &people);
        match send_reply(line, token, &pieces).await {
            Ok(()) => sent = true,
            Err(kind) => {
                warn!(?kind, venue = %line.venue(), "[Group] reply not sent");
                break;
            }
        }
    }
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
/// Lines that did not call her by name: she looks once the talk pauses
/// this long, and in talk that never pauses, at least this often.
const SETTLE: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(30);
/// Lines of the talk she looks at.
const CONVERSATION_LINES: usize = 15;
/// Two bots answering each other never stop, and no person talks like that:
/// this many of her replies in a row to the same one, all within the
/// window, and she stops answering them for a while.
const LOOP_ROUNDS: usize = 20;
const LOOP_WINDOW: Duration = Duration::from_secs(10 * 60);
const LOOP_PAUSE: Duration = Duration::from_secs(15 * 60);
/// Replies a day to people outside the community, per group.
const STRANGER_REPLIES_PER_DAY: u32 = 60;

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
    /// Whom she answered lately, in order, and whom she stopped answering
    /// (see `LOOP_ROUNDS`).
    answered: VecDeque<(String, Instant)>,
    paused: HashMap<String, Instant>,
    /// Lines that spoke to her while she was busy, oldest first.
    waiting: VecDeque<GroupLine>,
    /// Replies today to people outside the community: the day, and how many.
    stranger_replies: Option<(chrono::NaiveDate, u32)>,
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
        if with_group(venue, |group| !group.restored).unwrap_or(false) {
            let stored =
                crate::services::runtime_registry::get::<StoredLines>(db, LINES_NAMESPACE, venue)
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
    }
    let lines = with_group(venue, |group| {
        push_line(group, line);
        group.lines.iter().cloned().collect::<Vec<_>>()
    });
    let (Some(db), Some(lines)) = (db, lines) else {
        return;
    };
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
        text: bounded(&message.said()),
        hers: false,
    };
    remember_line(&message.venue(), line).await;
}

async fn record_hers(venue: &str, text: &str) {
    let line = Line {
        at: chrono::Utc::now(),
        message_id: None,
        name: String::new(),
        from: None,
        text: bounded(text),
        hers: true,
    };
    remember_line(venue, line).await;
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
                    format!("{}：{}", line.name, line.text)
                },
                created_at: None,
            })
            .collect()
    })
    .unwrap_or_default()
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

/// Answer one group line that spoke to her: now, or when she is done with
/// the one on hand.
pub async fn handle(mut message: GroupLine, token: String) {
    let venue = message.venue();
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
/// does: when the talk pauses (or, in talk that never pauses, every so
/// often) she looks at what was said and judges, as herself, whether to say
/// something (see `merope::joining`). Nothing is held while she waits.
pub fn notice(message: GroupLine, token: String) {
    let venue = message.venue();
    let id = message.message_id.clone();
    let noticed = with_group(&venue, |group| {
        group.unjudged_since.get_or_insert_with(Instant::now);
        group.pending = Some(message);
    });
    if noticed.is_some() {
        tokio::spawn(async move {
            tokio::time::sleep(SETTLE).await;
            look(venue, id, token).await;
        });
    }
}

enum Look {
    /// A newer line will be looked at instead, or nothing is waiting.
    Done,
    /// She is talking or looking already: again in a moment.
    Later,
    Now(GroupLine),
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
            group.pending.take().map_or(Look::Done, Look::Now)
        })
        .unwrap_or(Look::Done);
        let message = match next {
            Look::Done => return,
            Look::Later => {
                tokio::time::sleep(SETTLE).await;
                continue;
            }
            Look::Now(message) => message,
        };
        let why = judge(&message).await;
        with_group(&venue, |group| group.judging = false);
        if let Some(why) = why
            && matches!(begin_turn(&venue), Turn::Began)
        {
            let replied = answer(&message, &token, Some(why)).await;
            if replied {
                info!(%venue, "[Group] she spoke up");
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
async fn judge(message: &GroupLine) -> Option<String> {
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
    let last_spoke = with_group(&venue, |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| line.hers)
            .map(|line| myriad_merope::doing::ago_text(chrono::Utc::now() - line.at))
    })
    .flatten();
    crate::services::agent::merope::joining::decide(
        &db,
        owner,
        &conversation,
        last_spoke.as_deref(),
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
    let user_id = match lookup(&db, message).await {
        Some(PairingLookup::Paired { user_id }) => user_id,
        // Someone from outside the community: answered lightly.
        Some(_) => return answer_stranger(&db, message, token, chime.as_deref()).await,
        _ => return false,
    };
    let Some(binding) = current_binding(&db, message, user_id).await else {
        return false;
    };
    let Some(reply) = run_turn(&db, message, user_id, token, chime).await else {
        return false;
    };
    // Unpaired or switched off while she was thinking: say nothing.
    if !binding.is_current(&db).await {
        return false;
    }
    let reply = without_reply_mark(&reply);
    let sent = deliver(message, token, &reply).await;
    if sent {
        record_hers(&message.venue(), &reply).await;
        answered(message);
    }
    sent
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
    send_typing(message, token).await;
    let transcript = transcript(&venue, Some(&message.message_id));
    let Ok(Some(reply)) = tokio::time::timeout(
        TURN_DEADLINE,
        crate::services::agent::merope::strangers::reply(
            db,
            owner,
            &venue,
            &stranger,
            &transcript,
            &message.said(),
            why,
        ),
    )
    .await
    else {
        return false;
    };
    let reply = without_reply_mark(&reply);
    let sent = deliver(message, token, &reply).await;
    if sent {
        record_hers(&venue, &reply).await;
        answered(message);
        crate::services::agent::merope::strangers::enqueue_after(
            db,
            owner,
            venue,
            stranger,
            message.said(),
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
) -> Option<String> {
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
            input: message.said(),
            context: Some(crate::services::agent::run::ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: Some(session_id),
                group: Some(crate::services::agent::run::GroupTurn {
                    transcript: transcript(&venue, Some(&message.message_id)),
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
                        return success
                            .then(|| response.get("message").and_then(|value| value.as_str()))
                            .flatten()
                            .map(str::trim)
                            .filter(|text| !text.is_empty())
                            .map(str::to_string);
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
