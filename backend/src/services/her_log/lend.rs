//! What a group or a private chat on a chat app says, lent to her mind when
//! it is on a PC (protocol 2's `group.*` and `chat.*` frames), and what she
//! says there, sent out by the site as her.
//!
//! While she is not on the site and the lease is held by a PC whose link
//! syncs her, the site does not answer anyone itself: a group's new line
//! goes to that PC as `group.lend`, a private message as `chat.lend` (the
//! bots stay here; her mind is there). When her mind there says something
//! (`group.say`, `chat.say`), the site sends it where the lend came from,
//! the way it sends her own lines, and answers with `group.said` or
//! `chat.said`. Only the holder is taken (from another core the relay's
//! frame check refuses it first: `error` `bad_frame`), and only for a place
//! the site lent it (`group_unknown`, `chat_unknown`).
//!
//! A link is registered here ([`open`]) once its welcome said sync on and
//! its hello listed `lend` (its mind thinks with what is lent it): it is a
//! host core that keeps her log, and only one may hold her lease. A link
//! that did not list it is lent nothing: a group's lines stay with the
//! group mind here, a private message is held. What
//! is lent goes only to the link of the device holding the lease
//! ([`mind_on_a_pc`] reads it for every lend), so a device that hands her
//! over is lent nothing more.
//!
//! A lend is her mind's once that link acked it (the envelope's `ack`
//! covers the frame) or once she said something for it: whether she says
//! anything is hers to choose there. A lend not acked when its link closes
//! or her mind leaves that device is the site's again ([`close`],
//! [`let_go`]): a group's line is taken up by the group mind here once she
//! is back ([`untaken_groups`]), a private message is held again as if it
//! had never been lent. Each lend goes once, on the link it was made for:
//! the link does not send it again, and a new link of the device is not
//! sent the old one.
//!
//! What was lent is kept here in memory only: a lend not acked when the
//! process restarts is let go with it (the group mind or the held mail
//! takes up what comes after once she is back).

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use super::relay::{Outgoing, To};
use super::{Error, NODE};

/// A link registered to be lent to: which one it is, and where its frames go.
struct Link {
    token: u64,
    send: UnboundedSender<Outgoing>,
}

/// The links that may be lent to now, by device.
static LINKS: LazyLock<Mutex<HashMap<String, Link>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

static TOKENS: AtomicU64 = AtomicU64::new(1);

/// A link from `device` syncs her (its welcome said sync on): frames lent
/// to it go to `send`. The token names this link, for [`close`]: a newer
/// link of the device takes its place.
pub fn open(device: &str, send: UnboundedSender<Outgoing>) -> u64 {
    let token = TOKENS.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut links) = LINKS.lock() {
        links.insert(device.to_string(), Link { token, send });
    }
    token
}

/// The link `token` of `device` closed: it is lent nothing more, and what
/// was lent on it and not acked is the site's again. The private messages
/// among them, for [`give_back`].
pub fn close(device: &str, token: u64) -> Vec<ChatLent> {
    if let Ok(mut links) = LINKS.lock()
        && links.get(device).is_some_and(|link| link.token == token)
    {
        links.remove(device);
    }
    let_go_where(|one| one.device == device && one.link == token)
}

/// Her mind left `device` (the lease moved, or syncing stopped): what was
/// lent to it and not acked is the site's again. The private messages
/// among them, for [`give_back`].
pub fn let_go(device: &str) -> Vec<ChatLent> {
    let_go_where(|one| one.device == device)
}

fn let_go_where(which: impl Fn(&Lent) -> bool) -> Vec<ChatLent> {
    let mut chats = Vec::new();
    let Ok(mut lent) = LENT.lock() else {
        return chats;
    };
    let ids: Vec<String> = lent
        .iter()
        .filter(|(_, one)| which(one))
        .map(|(id, _)| id.clone())
        .collect();
    for id in &ids {
        let Some(one) = lent.remove(id) else {
            continue;
        };
        if one.taken {
            continue;
        }
        match one.place {
            Place::Group { venue, .. } => {
                if let Ok(mut back) = RETURNED.lock() {
                    back.insert(venue);
                }
            }
            Place::Chat { .. } => chats.extend(one.chat),
        }
    }
    if let Ok(mut latest) = LATEST.lock() {
        latest.retain(|_, id| !ids.contains(id));
    }
    chats
}

/// Whether any link may be lent to now (none until one syncs her).
pub fn any_link() -> bool {
    LINKS.lock().is_ok_and(|links| !links.is_empty())
}

/// Whether anything may be lent now: her log is on and some link syncs
/// her. A cheap check, before [`mind_on_a_pc`] reads the lease.
pub fn may_lend() -> bool {
    super::enabled() && any_link()
}

fn link(device: &str) -> Option<(u64, UnboundedSender<Outgoing>)> {
    LINKS
        .lock()
        .ok()?
        .get(device)
        .map(|link| (link.token, link.send.clone()))
}

/// The PC her mind is on, when it holds the lease and its link syncs her:
/// what she is lent goes there. `None` while her log is off, while no link
/// syncs her, or while the server or another end holds her.
pub async fn mind_on_a_pc(db: &DatabaseConnection) -> Option<String> {
    mind_on_a_pc_if(db, super::enabled()).await
}

/// [`mind_on_a_pc`], with her log on the server `on` or not.
pub(crate) async fn mind_on_a_pc_if(db: &DatabaseConnection, on: bool) -> Option<String> {
    if !on || !any_link() {
        return None;
    }
    let holder = super::lease::standing(db).await.ok()??.holder?;
    (holder != NODE && link(&holder).is_some()).then_some(holder)
}

/// Where a lend came from: what a `say` for it is sent back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// A group, by its venue (`onebot:123`), in a Telegram forum topic.
    Group { venue: String, thread: Option<i64> },
    /// A private chat with a site user on a chat app, by its session key.
    Chat { user: i32, session_key: String },
}

/// A private message as it was lent: what the site keeps of it once her
/// mind takes it (it goes into the chat's history there), or holds again
/// when it is given back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLent {
    /// The chat app, by its slug.
    pub platform: String,
    pub session_key: String,
    pub user: i32,
    /// The platform message it is known by, so it is held once.
    pub inbound: String,
    pub text: String,
    /// The chat's session here, when it has one.
    pub chat_session: Option<String>,
}

#[derive(Debug, Clone)]
struct Lent {
    device: String,
    /// The link it was lent on ([`open`]'s token).
    link: u64,
    audience: String,
    place: Place,
    /// Her mind there took it: its link acked it, or she said something for
    /// it. One not taken when her mind leaves is the site's again.
    taken: bool,
    /// A private message's own, for when it is taken or given back.
    chat: Option<ChatLent>,
}

/// What was lent and not yet let go, by lend id; and, by audience, the
/// latest lend there (a `say` that names no lend goes to it).
static LENT: LazyLock<Mutex<HashMap<String, Lent>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static LATEST: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Groups (by venue) with a line given back untaken: the group mind here
/// takes them up once she is back.
static RETURNED: LazyLock<Mutex<BTreeSet<String>>> = LazyLock::new(|| Mutex::new(BTreeSet::new()));

/// Lends kept at once: past this, the taken ones are let go, then all.
const KEPT: usize = 256;

fn lend_id() -> String {
    format!("lend_{}", &uuid::Uuid::new_v4().simple().to_string()[..20])
}

/// A group's audience: `group:<venue>`.
pub fn group_audience(venue: &str) -> String {
    format!("group:{venue}")
}

/// A private chat's audience: the partner's own (`personal:<partner id>`)
/// when it is the site's owner, else `personal:<site user id>`.
pub async fn chat_audience(db: &DatabaseConnection, user: i32) -> Result<String, Error> {
    let site_owner = crate::services::site_owner::site_owner_user_id(db)
        .await
        .map_err(|_| Error::Refused("no site owner".into()))?;
    if user == site_owner {
        Ok(super::owner(db, user).await?.personal())
    } else {
        Ok(format!("personal:{user}"))
    }
}

/// One message as a lend carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    /// The member's id in the group (groups only).
    pub from: Option<String>,
    /// What they are called there (groups only).
    pub name: Option<String>,
    pub text: String,
    pub at_ms: i64,
    /// Whether it was addressed to her (groups only).
    pub to_her: bool,
    /// A line she said there (a private chat's recent only).
    pub mine: bool,
    /// Who wrote it, as the group mind's `identified` answer has it
    /// (groups only; none when the site could not tell).
    pub who: Option<Value>,
}

/// What the site knows of a lend besides its lines: what the place is
/// called, the site's owner (groups: who hosts her there), who she talks
/// with (private chats), in the `identified` answer's shape, and the
/// highest count of the PC's rows the site holds there ([`through`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct About<'a> {
    pub name: Option<&'a str>,
    pub owner: Option<i32>,
    pub who: Option<Value>,
    pub through: Option<u64>,
}

impl Message {
    fn body(&self, group: bool) -> Value {
        let mut body = json!({ "id": self.id, "text": self.text, "at": self.at_ms });
        if group {
            body["from"] = json!(self.from.clone().unwrap_or_default());
            body["name"] = json!(self.name.clone().unwrap_or_default());
            if self.to_her {
                body["toHer"] = json!(true);
            }
            if let Some(who) = &self.who {
                body["who"] = who.clone();
            }
        } else if self.mine {
            body["mine"] = json!(true);
        }
        body
    }
}

/// The frame a lend goes as, for `audience` and `place`.
fn lend_frame(
    audience: &str,
    place: &Place,
    about: &About<'_>,
    messages: &[Message],
    recent: &[Message],
) -> Outgoing {
    let group = matches!(place, Place::Group { .. });
    let (t, field) = if group {
        ("group.lend", "group")
    } else {
        ("chat.lend", "chat")
    };
    let cap = |all: &[Message]| -> Vec<Value> {
        let skip = all.len().saturating_sub(64);
        all[skip..].iter().map(|one| one.body(group)).collect()
    };
    let mut body = json!({ field: audience, "messages": cap(messages) });
    if let Some(name) = about.name.filter(|name| !name.is_empty()) {
        body["name"] = json!(name);
    }
    if let Some(owner) = about.owner.filter(|_| group) {
        body["owner"] = json!(owner);
    }
    if let Some(who) = about.who.as_ref().filter(|_| !group) {
        body["who"] = who.clone();
    }
    if !recent.is_empty() {
        body["recent"] = json!(cap(recent));
    }
    if let Some(through) = about.through {
        body["through"] = json!(through);
    }
    Outgoing {
        to: To::Sender,
        t,
        id: Some(lend_id()),
        body,
    }
}

/// The highest `dseq` of `device`'s rows the site holds in `audience`,
/// past any gap too, for a lend there: the PC keeps none of an audience it
/// does not hold on disk, so its rows there go on from this without a
/// number used twice (the site keeps one row a place). None for an
/// audience the PC holds (its own count is on its disk) or where the site
/// holds none of its rows.
pub async fn through(db: &DatabaseConnection, device: &str, audience: &str) -> Option<u64> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let site_owner = crate::services::site_owner::site_owner_user_id(db)
        .await
        .ok()?;
    let owner = super::owner(db, site_owner).await.ok()?;
    let pc = myriad_persona_log::Holder {
        core: myriad_persona_log::Core::Pc,
        owner: owner.partner,
    };
    if pc.holds(audience) {
        return None;
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT MAX(dseq) AS through FROM agent_log_rows \
             WHERE user_id = $1 AND origin = $2 AND audience = $3",
            [site_owner.into(), device.into(), audience.into()],
        ))
        .await
        .ok()??;
    let through: Option<i64> = row.try_get("", "through").ok()?;
    through.and_then(|through| u64::try_from(through).ok())
}

/// Lend `messages` of `place` to `device`, her mind; whether it went (the
/// link may have closed meanwhile: then nothing was lent). `chat` is a
/// private message's own, kept for when it is taken or given back.
#[allow(clippy::too_many_arguments)]
pub fn lend(
    device: &str,
    audience: &str,
    place: Place,
    about: About<'_>,
    messages: &[Message],
    recent: &[Message],
    chat: Option<ChatLent>,
) -> bool {
    if messages.is_empty() {
        return false;
    }
    let Some((token, send)) = link(device) else {
        return false;
    };
    let mut frame = lend_frame(audience, &place, &about, messages, recent);
    frame.to = To::Device(device.to_string());
    let id = frame.id.clone().unwrap_or_default();
    if let (Ok(mut lent), Ok(mut latest)) = (LENT.lock(), LATEST.lock()) {
        if lent.len() >= KEPT {
            lent.retain(|_, one| !one.taken);
        }
        if lent.len() >= KEPT {
            lent.clear();
            latest.clear();
        }
        lent.insert(
            id.clone(),
            Lent {
                device: device.to_string(),
                link: token,
                audience: audience.to_string(),
                place,
                taken: false,
                chat,
            },
        );
        latest.insert(audience.to_string(), id.clone());
    }
    if send.send(frame).is_err() {
        // The link went meanwhile: nothing was lent.
        if let Ok(mut lent) = LENT.lock() {
            lent.remove(&id);
        }
        if let Ok(mut links) = LINKS.lock()
            && links.get(device).is_some_and(|link| link.token == token)
        {
            links.remove(device);
        }
        return false;
    }
    tracing::info!(
        device,
        "[her-log] she is on a PC: what came is lent to her mind there"
    );
    true
}

/// The link of `device` acked the reliable frame `id`: when it is a lend,
/// her mind there took it. A private message taken now, for [`keep`].
pub fn acked(device: &str, id: &str) -> Option<ChatLent> {
    let mut lent = LENT.lock().ok()?;
    let one = lent
        .get_mut(id)
        .filter(|one| one.device == device && !one.taken)?;
    one.taken = true;
    one.chat.clone()
}

/// Whether the lend `id` to `device` waits for its link's ack.
pub fn waiting(device: &str, id: &str) -> bool {
    LENT.lock().is_ok_and(|lent| {
        lent.get(id)
            .is_some_and(|one| one.device == device && !one.taken)
    })
}

/// The groups (by venue) with lines lent that her mind never took: when
/// she is back on the site, these are hers to take up here; the others she
/// took there (and answered, or chose not to). What was lent is let go.
pub fn untaken_groups() -> Vec<String> {
    let Ok(mut lent) = LENT.lock() else {
        return Vec::new();
    };
    let mut venues: BTreeSet<String> = lent
        .values()
        .filter(|one| !one.taken)
        .filter_map(|one| match &one.place {
            Place::Group { venue, .. } => Some(venue.clone()),
            Place::Chat { .. } => None,
        })
        .collect();
    if let Ok(mut back) = RETURNED.lock() {
        venues.append(&mut back);
    }
    lent.clear();
    if let Ok(mut latest) = LATEST.lock() {
        latest.clear();
    }
    venues.into_iter().collect()
}

/// Where a `say` from `device` for `audience` (naming `lend`, maybe) goes:
/// the lend it names, else the latest one there; only one lent to it. With
/// it, a private message taken only now (no ack had come for it), for
/// [`keep`].
fn place_for(
    device: &str,
    audience: &str,
    lend: Option<&str>,
) -> Option<(Place, Option<ChatLent>)> {
    let mut lent = LENT.lock().ok()?;
    let id = match lend {
        Some(id) => id.to_string(),
        None => LATEST.lock().ok()?.get(audience)?.clone(),
    };
    lent.get_mut(&id)
        .filter(|one| one.device == device && one.audience == audience)
        .map(|one| {
            let newly = (!one.taken).then(|| one.chat.clone()).flatten();
            one.taken = true;
            (one.place.clone(), newly)
        })
}

/// Sends her line to a place the site lent, how it failed if it did; and
/// keeps what a lent private message leaves. `Sync`, so a link's task may
/// hold it across a send.
pub trait Sender: Sync {
    /// `line` is the say's own name, `<device>/<say id>`: the `speech.said`
    /// row the mind writes of it names the same say (its `line`).
    fn send<'a>(
        &'a self,
        db: &'a DatabaseConnection,
        place: &'a Place,
        text: &'a str,
        reply_to: Option<&'a str>,
        line: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<(), &'static str>>;

    /// Her mind took `chat`: their message goes into the chat's history.
    fn keep<'a>(
        &'a self,
        db: &'a DatabaseConnection,
        chat: &'a ChatLent,
    ) -> futures::future::BoxFuture<'a, ()>;

    /// `chat` was given back untaken: it is held until she is back.
    fn hold<'a>(
        &'a self,
        db: &'a DatabaseConnection,
        chat: &'a ChatLent,
    ) -> futures::future::BoxFuture<'a, ()>;
}

/// The chat apps, as the site reaches them.
pub struct Platforms;

impl Sender for Platforms {
    fn send<'a>(
        &'a self,
        db: &'a DatabaseConnection,
        place: &'a Place,
        text: &'a str,
        reply_to: Option<&'a str>,
        line: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<(), &'static str>> {
        Box::pin(async move {
            match place {
                Place::Group { venue, thread } => {
                    crate::services::channel_group::say_lent(venue, *thread, reply_to, text).await
                }
                Place::Chat { user, session_key } => {
                    crate::services::channel_work::held::say_lent(
                        db,
                        *user,
                        session_key,
                        text,
                        line,
                    )
                    .await
                }
            }
        })
    }

    fn keep<'a>(
        &'a self,
        db: &'a DatabaseConnection,
        chat: &'a ChatLent,
    ) -> futures::future::BoxFuture<'a, ()> {
        Box::pin(crate::services::channel_work::held::keep_lent(db, chat))
    }

    fn hold<'a>(
        &'a self,
        db: &'a DatabaseConnection,
        chat: &'a ChatLent,
    ) -> futures::future::BoxFuture<'a, ()> {
        Box::pin(crate::services::channel_work::held::hold_lent(db, chat))
    }
}

/// What her mind took of the private messages lent: each goes into its
/// chat's history.
pub async fn keep(db: &DatabaseConnection, sender: &dyn Sender, chats: Vec<ChatLent>) {
    for chat in &chats {
        sender.keep(db, chat).await;
    }
}

/// Private messages given back untaken: each is held until she is back.
pub async fn give_back(db: &DatabaseConnection, sender: &dyn Sender, chats: Vec<ChatLent>) {
    for chat in &chats {
        sender.hold(db, chat).await;
    }
    if !chats.is_empty() {
        tracing::info!(
            count = chats.len(),
            "[her-log] private messages lent and not taken are held again"
        );
    }
}

/// Takes a `group.say` or `chat.say` from `device`: her line goes out where
/// it was lent from, answered with `group.said` or `chat.said`. `mine` is
/// whether `device` holds the lease (the relay checked the frame as hers, so
/// from the relay it always does; `not_holder` stays for a caller that has
/// not).
pub async fn say(
    db: &DatabaseConnection,
    sender: &dyn Sender,
    device: &str,
    mine: bool,
    frame: &Value,
) -> Vec<Outgoing> {
    let t = frame["t"].as_str().unwrap_or_default();
    let (said, field, unknown) = if t == "group.say" {
        ("group.said", "group", "group_unknown")
    } else {
        ("chat.said", "chat", "chat_unknown")
    };
    let say_id = frame["id"].as_str().unwrap_or_default().to_string();
    let body = &frame["body"];
    let answer = |sent: bool, code: Option<&str>| {
        let mut body = json!({ "sayId": say_id, "sent": sent });
        if let Some(code) = code {
            body["code"] = json!(code);
        }
        vec![Outgoing {
            to: To::Sender,
            t: said,
            id: None,
            body,
        }]
    };
    if !mine {
        return answer(false, Some("not_holder"));
    }
    let audience = body[field].as_str().unwrap_or_default();
    let Some((place, newly)) = place_for(device, audience, body["lend"].as_str()) else {
        return answer(false, Some(unknown));
    };
    // Their message goes into the chat's history before her line.
    if let Some(chat) = &newly {
        sender.keep(db, chat).await;
    }
    let text = body["text"].as_str().unwrap_or_default().trim();
    if text.is_empty() {
        return answer(false, Some("unavailable"));
    }
    let line = format!("{device}/{say_id}");
    match sender
        .send(db, &place, text, body["replyTo"].as_str(), &line)
        .await
    {
        Ok(()) => answer(true, None),
        Err(why) => {
            tracing::warn!(device, why, "[her-log] her line from a PC was not sent");
            answer(false, Some("unavailable"))
        }
    }
}

/// Whether the lend `id` is still kept, and taken (tests).
#[cfg(test)]
pub(crate) fn taken(id: &str) -> Option<bool> {
    LENT.lock().ok()?.get(id).map(|one| one.taken)
}

/// The lend tests share these statics (and `untaken_groups` lets go of
/// all of them): they take turns.
#[cfg(test)]
pub(crate) static SERIAL: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for the chat apps: what it was asked to send, and the lent
    /// private messages it was asked to keep and to hold again.
    #[derive(Default)]
    struct Kept(
        Mutex<Vec<(Place, String, Option<String>)>>,
        Mutex<Vec<String>>,
        Mutex<Vec<String>>,
    );

    impl Sender for Kept {
        fn send<'a>(
            &'a self,
            _db: &'a DatabaseConnection,
            place: &'a Place,
            text: &'a str,
            reply_to: Option<&'a str>,
            _line: &'a str,
        ) -> futures::future::BoxFuture<'a, Result<(), &'static str>> {
            self.0.lock().unwrap().push((
                place.clone(),
                text.to_string(),
                reply_to.map(str::to_string),
            ));
            Box::pin(async { Ok(()) })
        }

        fn keep<'a>(
            &'a self,
            _db: &'a DatabaseConnection,
            chat: &'a ChatLent,
        ) -> futures::future::BoxFuture<'a, ()> {
            self.1.lock().unwrap().push(chat.inbound.clone());
            Box::pin(async {})
        }

        fn hold<'a>(
            &'a self,
            _db: &'a DatabaseConnection,
            chat: &'a ChatLent,
        ) -> futures::future::BoxFuture<'a, ()> {
            self.2.lock().unwrap().push(chat.inbound.clone());
            Box::pin(async {})
        }
    }

    fn chat_lent(inbound: &str, user: i32) -> ChatLent {
        ChatLent {
            platform: "onebot".into(),
            session_key: format!("onebot:{user}"),
            user,
            inbound: inbound.into(),
            text: "在吗".into(),
            chat_session: None,
        }
    }

    fn message(id: &str, text: &str) -> Message {
        Message {
            id: id.into(),
            from: None,
            name: None,
            text: text.into(),
            at_ms: 1_760_140_800_000,
            to_her: true,
            mine: false,
            who: None,
        }
    }

    fn say_frame(t: &str, field: &str, audience: &str, lend: Option<&str>) -> Value {
        let mut body = json!({ field: audience, "text": "嗯，在的。", "replyTo": "m_1" });
        if let Some(lend) = lend {
            body["lend"] = json!(lend);
        }
        json!({ "v": 1, "t": t, "seq": 3, "ack": 0, "at": 1, "id": "say_1", "body": body })
    }

    /// A private chat is lent to the PC her mind is on; what she says back
    /// goes to that chat, only from that PC, only for a chat it was lent.
    #[tokio::test]
    async fn a_lent_chat_is_answered_where_it_came_from() {
        let _turn = SERIAL.lock().await;
        let db = DatabaseConnection::default();
        let (send, mut lent) = tokio::sync::mpsc::unbounded_channel();
        let link = open("dev_lend_unit", send);
        let place = Place::Chat {
            user: 7,
            session_key: "onebot:20002".into(),
        };
        let recent = [Message {
            mine: true,
            to_her: false,
            ..message("h_1", "刚读完。")
        }];
        assert!(!lend(
            "dev_lend_unit",
            "personal:7",
            place.clone(),
            About::default(),
            &[],
            &[],
            None,
        ));
        assert!(lend(
            "dev_lend_unit",
            "personal:7",
            place.clone(),
            About {
                name: Some("小林"),
                owner: Some(1),
                who: Some(json!({ "kind": "member", "user_id": 7, "bound": true })),
                through: Some(4),
            },
            &[message("m_1", "在吗")],
            &recent,
            Some(chat_lent("m_1", 7)),
        ));
        let frame = lent.try_recv().unwrap();
        assert_eq!(frame.t, "chat.lend");
        assert_eq!(frame.to, To::Device("dev_lend_unit".into()));
        assert_eq!(frame.body["chat"], "personal:7");
        assert_eq!(frame.body["name"], "小林");
        assert_eq!(frame.body["messages"][0]["text"], "在吗");
        assert!(frame.body["messages"][0].get("from").is_none());
        assert_eq!(frame.body["recent"][0]["mine"], true);
        // Who she talks with goes on the lend; the owner is a group's.
        assert_eq!(frame.body["who"]["user_id"], 7);
        assert!(frame.body.get("owner").is_none());
        assert_eq!(frame.body["through"], 4);
        let lend_id = frame.id.unwrap();

        let kept = Kept::default();
        // From another device: not one it was lent.
        let said = say(
            &db,
            &kept,
            "dev_other",
            true,
            &say_frame("chat.say", "chat", "personal:7", Some(&lend_id)),
        )
        .await;
        assert_eq!(said[0].t, "chat.said");
        assert_eq!(said[0].body["code"], "chat_unknown");
        // Not from her mind.
        let said = say(
            &db,
            &kept,
            "dev_lend_unit",
            false,
            &say_frame("chat.say", "chat", "personal:7", Some(&lend_id)),
        )
        .await;
        assert_eq!(said[0].body["code"], "not_holder");
        // Another chat than the one lent.
        let said = say(
            &db,
            &kept,
            "dev_lend_unit",
            true,
            &say_frame("chat.say", "chat", "personal:8", None),
        )
        .await;
        assert_eq!(said[0].body["code"], "chat_unknown");
        assert!(kept.0.lock().unwrap().is_empty());
        // Hers, for the chat lent (by its lend, then by the latest there).
        for lend in [Some(lend_id.as_str()), None] {
            let said = say(
                &db,
                &kept,
                "dev_lend_unit",
                true,
                &say_frame("chat.say", "chat", "personal:7", lend),
            )
            .await;
            assert_eq!(said[0].body, json!({ "sayId": "say_1", "sent": true }));
        }
        assert_eq!(
            kept.0.lock().unwrap()[0],
            (place.clone(), "嗯，在的。".into(), Some("m_1".into()))
        );
        // Their message went into the chat once, before her first line (no
        // ack had come for it).
        assert_eq!(*kept.1.lock().unwrap(), vec!["m_1".to_string()]);
        // The link closed: it is lent nothing more, and what she took is
        // not given back.
        assert!(close("dev_lend_unit", link).is_empty());
        let said = say(
            &db,
            &kept,
            "dev_lend_unit",
            true,
            &say_frame("chat.say", "chat", "personal:7", Some(&lend_id)),
        )
        .await;
        assert_eq!(said[0].body["code"], "chat_unknown");
        assert!(!lend(
            "dev_lend_unit",
            "personal:7",
            place,
            About::default(),
            &[message("m_2", "人呢")],
            &[],
            None,
        ));
    }

    /// A group's lend is a frame the contract takes, and her line in the
    /// group goes back to it, in its topic.
    #[tokio::test]
    async fn a_lent_group_is_a_contract_frame_and_answered_there() {
        let _turn = SERIAL.lock().await;
        let db = DatabaseConnection::default();
        let (send, mut lent) = tokio::sync::mpsc::unbounded_channel();
        let link = open("dev_lend_group", send);
        let place = Place::Group {
            venue: "telegram:-100".into(),
            thread: Some(9),
        };
        let audience = group_audience("telegram:-100");
        let line = Message {
            from: Some("m7".into()),
            name: Some("小林".into()),
            who: Some(json!({ "kind": "member", "user_id": 101, "bound": true })),
            ..message("m_903", "这周读到哪了？")
        };
        let before = Message {
            to_her: false,
            who: Some(json!({ "kind": "stranger" })),
            ..message("m_902", "我还在第三章")
        };
        assert!(lend(
            "dev_lend_group",
            &audience,
            place.clone(),
            About {
                name: Some("读书会"),
                owner: Some(1),
                who: Some(json!({ "kind": "stranger" })),
                through: None,
            },
            &[line],
            &[before],
            None,
        ));
        let frame = lent.try_recv().unwrap();
        assert_eq!(frame.t, "group.lend");
        assert_eq!(frame.body["group"], "group:telegram:-100");
        assert_eq!(frame.body["messages"][0]["toHer"], true);
        assert_eq!(frame.body["messages"][0]["from"], "m7");
        // Who wrote each line, and who hosts her there.
        assert_eq!(frame.body["messages"][0]["who"]["kind"], "member");
        assert_eq!(frame.body["recent"][0]["who"]["kind"], "stranger");
        assert_eq!(frame.body["owner"], 1);
        assert!(frame.body.get("who").is_none());
        super::super::relay::encode(&frame, 1, 0, 1_760_140_800_000).unwrap();
        let kept = Kept::default();
        let said = say(
            &db,
            &kept,
            "dev_lend_group",
            true,
            &say_frame("group.say", "group", &audience, frame.id.as_deref()),
        )
        .await;
        assert_eq!(said[0].t, "group.said");
        assert_eq!(said[0].body["sent"], true);
        super::super::relay::encode(&said[0], 2, 0, 1_760_140_800_000).unwrap();
        assert_eq!(kept.0.lock().unwrap()[0].0, place);
        close("dev_lend_group", link);
    }

    fn lend_chat(device: &str, user: i32, inbound: &str) -> bool {
        lend(
            device,
            &format!("personal:{user}"),
            Place::Chat {
                user,
                session_key: format!("onebot:{user}"),
            },
            About::default(),
            &[message(inbound, "在吗")],
            &[],
            Some(chat_lent(inbound, user)),
        )
    }

    fn lend_group(device: &str, venue: &str, id: &str) -> bool {
        lend(
            device,
            &group_audience(venue),
            Place::Group {
                venue: venue.into(),
                thread: None,
            },
            About::default(),
            &[message(id, "有人吗")],
            &[],
            None,
        )
    }

    /// A lend its link acked is her mind's: whether she says anything is
    /// hers, and nothing of it comes back to the site. One not acked when
    /// the link closes is the site's again: the group's line for the group
    /// mind once she is back, the private message held again. Each lend
    /// goes once.
    #[tokio::test]
    async fn an_acked_lend_is_hers_and_one_not_acked_comes_back() {
        let _turn = SERIAL.lock().await;
        let db = DatabaseConnection::default();
        untaken_groups();
        let (send, mut lent) = tokio::sync::mpsc::unbounded_channel();
        let link = open("dev_lend_ack", send);
        assert!(lend_group("dev_lend_ack", "onebot:701", "g_1"));
        assert!(lend_chat("dev_lend_ack", 71, "c_1"));
        assert!(lend_group("dev_lend_ack", "onebot:702", "g_2"));
        assert!(lend_chat("dev_lend_ack", 72, "c_2"));
        let ids: Vec<String> = (0..4)
            .map(|_| lent.try_recv().unwrap().id.unwrap())
            .collect();
        // Each went once: nothing more is waiting on the link.
        assert!(lent.try_recv().is_err());
        assert!(ids.iter().all(|id| waiting("dev_lend_ack", id)));

        // The first two acked: hers. The private one goes into its chat.
        assert_eq!(acked("dev_lend_ack", &ids[0]), None);
        assert_eq!(acked("dev_lend_ack", &ids[1]), Some(chat_lent("c_1", 71)));
        // Acked again, or by another device: nothing more.
        assert_eq!(acked("dev_lend_ack", &ids[1]), None);
        assert_eq!(acked("dev_other", &ids[3]), None);
        assert_eq!(taken(&ids[0]), Some(true));
        assert_eq!(taken(&ids[3]), Some(false));
        assert!(!waiting("dev_lend_ack", &ids[1]));

        // The link closed with the last two not acked: they come back.
        let back = close("dev_lend_ack", link);
        assert_eq!(back, vec![chat_lent("c_2", 72)]);
        let kept = Kept::default();
        give_back(&db, &kept, back).await;
        assert_eq!(*kept.2.lock().unwrap(), vec!["c_2".to_string()]);
        assert_eq!(taken(&ids[3]), None);
        // A say for a lend given back finds nothing.
        let said = say(
            &db,
            &kept,
            "dev_lend_ack",
            true,
            &say_frame("chat.say", "chat", "personal:72", Some(&ids[3])),
        )
        .await;
        assert_eq!(said[0].body["code"], "chat_unknown");
        // She is back: only the group whose line she never took is hers to
        // take up here.
        assert_eq!(untaken_groups(), vec!["onebot:702".to_string()]);
        assert!(untaken_groups().is_empty());
    }

    /// A newer link of the device takes the old one's place: what was lent
    /// on the old one and not acked comes back when it closes, what is lent
    /// on the new one stays its own; and when her mind leaves the device,
    /// what is left there comes back too.
    #[tokio::test]
    async fn each_link_gives_back_only_its_own_lends() {
        let _turn = SERIAL.lock().await;
        untaken_groups();
        let (old_send, mut old) = tokio::sync::mpsc::unbounded_channel();
        let old_link = open("dev_lend_two", old_send);
        assert!(lend_chat("dev_lend_two", 81, "c_old"));
        let old_id = old.try_recv().unwrap().id.unwrap();
        let (new_send, mut new) = tokio::sync::mpsc::unbounded_channel();
        let new_link = open("dev_lend_two", new_send);
        assert!(lend_chat("dev_lend_two", 82, "c_new"));
        // The new lend went on the new link only.
        assert!(old.try_recv().is_err());
        let new_id = new.try_recv().unwrap().id.unwrap();
        assert_eq!(
            close("dev_lend_two", old_link),
            vec![chat_lent("c_old", 81)]
        );
        assert_eq!(taken(&old_id), None);
        assert_eq!(taken(&new_id), Some(false));
        // The newer link is still lent to.
        assert!(lend_group("dev_lend_two", "onebot:803", "g_3"));
        assert!(new.try_recv().is_ok());
        // Her mind left the device: what is left there comes back.
        assert_eq!(let_go("dev_lend_two"), vec![chat_lent("c_new", 82)]);
        assert_eq!(untaken_groups(), vec!["onebot:803".to_string()]);
        assert!(close("dev_lend_two", new_link).is_empty());
        assert!(!lend_chat("dev_lend_two", 82, "c_late"));
    }
}
