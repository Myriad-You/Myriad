//! What a group or a private chat on a chat app says, lent to her mind when
//! it is on a PC (protocol 2's `group.*` and `chat.*` frames), and what she
//! says there, sent out by the site as her.
//!
//! While she is not on the site and the lease is held by a PC whose link
//! speaks protocol 2, the site does not answer anyone itself: a group's new
//! line goes to that PC as `group.lend`, a private message as `chat.lend`
//! (the bots stay here; her mind is there). When her mind there says
//! something (`group.say`, `chat.say`), the site sends it where the lend came
//! from, the way it sends her own lines, and answers with `group.said` or
//! `chat.said`. Only the holder is taken (`not_holder` otherwise), and only a
//! place the site lent (`group_unknown`, `chat_unknown`).
//!
//! Closed until her link speaks protocol 2: a link is registered here
//! ([`open`]) only once it has negotiated protocol 2, and the site's link
//! still speaks 1, so none is. Until one is, [`mind_on_a_pc`] is `None` and
//! what comes is held (a private chat) or waits for her (a group), as
//! before.
//!
//! What was lent is kept here in memory only: a lend not answered when the
//! process restarts or the lease moves is not sent again (the group mind or
//! the held mail takes it up once she is back).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use super::relay::{Outgoing, To};
use super::{Error, NODE};

/// The links that speak protocol 2 now, by device: where their frames go.
static LINKS: LazyLock<Mutex<HashMap<String, UnboundedSender<Outgoing>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A link from `device` negotiated protocol 2: frames for it go to `send`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn open(device: &str, send: UnboundedSender<Outgoing>) {
    if let Ok(mut links) = LINKS.lock() {
        links.insert(device.to_string(), send);
    }
}

/// The link from `device` closed: what was lent to it is the site's again.
pub fn close(device: &str) {
    if let Ok(mut links) = LINKS.lock() {
        links.remove(device);
    }
    if let Ok(mut lent) = LENT.lock() {
        lent.retain(|_, place| place.device != device);
    }
}

/// Whether any link speaks protocol 2 now (none does until her link
/// negotiates it): nothing can be lent without one.
pub fn any_link() -> bool {
    LINKS.lock().is_ok_and(|links| !links.is_empty())
}

/// Whether anything may be lent now: her log is on and some link speaks
/// protocol 2. A cheap check, before [`mind_on_a_pc`] reads the lease.
pub fn may_lend() -> bool {
    super::enabled() && any_link()
}

fn link(device: &str) -> Option<UnboundedSender<Outgoing>> {
    LINKS.lock().ok()?.get(device).cloned()
}

/// The PC her mind is on, when it holds the lease and its link speaks
/// protocol 2: what she is lent goes there. `None` while her log is off,
/// while no link speaks protocol 2, or while the server or another end
/// holds her.
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

#[derive(Debug, Clone)]
struct Lent {
    device: String,
    audience: String,
    place: Place,
    /// Her mind there took it: she said something for it. One not taken
    /// when her mind comes back is the site's again.
    taken: bool,
}

/// What was lent and not yet let go, by lend id; and, by audience, the
/// latest lend there (a `say` that names no lend goes to it).
static LENT: LazyLock<Mutex<HashMap<String, Lent>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static LATEST: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Lends kept at once: past this, the oldest are let go.
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
/// called, the site's owner (groups: who hosts her there) and who she
/// talks with (private chats), in the `identified` answer's shape.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct About<'a> {
    pub name: Option<&'a str>,
    pub owner: Option<i32>,
    pub who: Option<Value>,
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
    Outgoing {
        to: To::Sender,
        t,
        id: Some(lend_id()),
        body,
    }
}

/// Lend `messages` of `place` to `device`, her mind; whether it went (the
/// link may have closed meanwhile: then nothing was lent).
pub fn lend(
    device: &str,
    audience: &str,
    place: Place,
    about: About<'_>,
    messages: &[Message],
    recent: &[Message],
) -> bool {
    if messages.is_empty() {
        return false;
    }
    let Some(send) = link(device) else {
        return false;
    };
    let mut frame = lend_frame(audience, &place, &about, messages, recent);
    frame.to = To::Device(device.to_string());
    let id = frame.id.clone().unwrap_or_default();
    if let (Ok(mut lent), Ok(mut latest)) = (LENT.lock(), LATEST.lock()) {
        if lent.len() >= KEPT {
            lent.clear();
            latest.clear();
        }
        lent.insert(
            id.clone(),
            Lent {
                device: device.to_string(),
                audience: audience.to_string(),
                place,
                taken: false,
            },
        );
        latest.insert(audience.to_string(), id);
    }
    if send.send(frame).is_err() {
        close(device);
        return false;
    }
    tracing::info!(
        device,
        "[her-log] she is on a PC: what came is lent to her mind there"
    );
    true
}

/// The groups (by venue) with lines lent that her mind never took: when
/// she is back on the site, these are hers to take up here; the others she
/// answered (or chose not to) there. What was lent is let go.
pub fn untaken_groups() -> Vec<String> {
    let Ok(mut lent) = LENT.lock() else {
        return Vec::new();
    };
    let mut venues: Vec<String> = lent
        .values()
        .filter(|one| !one.taken)
        .filter_map(|one| match &one.place {
            Place::Group { venue, .. } => Some(venue.clone()),
            Place::Chat { .. } => None,
        })
        .collect();
    venues.sort();
    venues.dedup();
    lent.clear();
    if let Ok(mut latest) = LATEST.lock() {
        latest.clear();
    }
    venues
}

/// Where a `say` from `device` for `audience` (naming `lend`, maybe) goes:
/// the lend it names, else the latest one there; only one lent to it.
fn place_for(device: &str, audience: &str, lend: Option<&str>) -> Option<Place> {
    let mut lent = LENT.lock().ok()?;
    let id = match lend {
        Some(id) => id.to_string(),
        None => LATEST.lock().ok()?.get(audience)?.clone(),
    };
    lent.get_mut(&id)
        .filter(|one| one.device == device && one.audience == audience)
        .map(|one| {
            one.taken = true;
            one.place.clone()
        })
}

/// Sends her line to a place the site lent: how it failed, if it did.
/// `Sync`, so a link's task may hold it across a send.
pub trait Sender: Sync {
    fn send<'a>(
        &'a self,
        db: &'a DatabaseConnection,
        place: &'a Place,
        text: &'a str,
        reply_to: Option<&'a str>,
    ) -> futures::future::BoxFuture<'a, Result<(), &'static str>>;
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
    ) -> futures::future::BoxFuture<'a, Result<(), &'static str>> {
        Box::pin(async move {
            match place {
                Place::Group { venue, thread } => {
                    crate::services::channel_group::say_lent(venue, *thread, reply_to, text).await
                }
                Place::Chat { user, session_key } => {
                    crate::services::channel_work::held::say_lent(db, *user, session_key, text)
                        .await
                }
            }
        })
    }
}

/// Takes a `group.say` or `chat.say` from `device`: her line goes out where
/// it was lent from, answered with `group.said` or `chat.said`. `mine` is
/// whether `device` holds the lease (the relay checked the frame as hers).
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
    let Some(place) = place_for(device, audience, body["lend"].as_str()) else {
        return answer(false, Some(unknown));
    };
    let text = body["text"].as_str().unwrap_or_default().trim();
    if text.is_empty() {
        return answer(false, Some("unavailable"));
    }
    match sender
        .send(db, &place, text, body["replyTo"].as_str())
        .await
    {
        Ok(()) => answer(true, None),
        Err(why) => {
            tracing::warn!(device, why, "[her-log] her line from a PC was not sent");
            answer(false, Some("unavailable"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for the chat apps: what it was asked to send.
    #[derive(Default)]
    struct Kept(Mutex<Vec<(Place, String, Option<String>)>>);

    impl Sender for Kept {
        fn send<'a>(
            &'a self,
            _db: &'a DatabaseConnection,
            place: &'a Place,
            text: &'a str,
            reply_to: Option<&'a str>,
        ) -> futures::future::BoxFuture<'a, Result<(), &'static str>> {
            self.0.lock().unwrap().push((
                place.clone(),
                text.to_string(),
                reply_to.map(str::to_string),
            ));
            Box::pin(async { Ok(()) })
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
        let db = DatabaseConnection::default();
        let (send, mut lent) = tokio::sync::mpsc::unbounded_channel();
        open("dev_lend_unit", send);
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
            &[]
        ));
        assert!(lend(
            "dev_lend_unit",
            "personal:7",
            place.clone(),
            About {
                name: Some("小林"),
                owner: Some(1),
                who: Some(json!({ "kind": "member", "user_id": 7, "bound": true })),
            },
            &[message("m_1", "在吗")],
            &recent,
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
        // The link closed: what was lent to it is the site's again.
        close("dev_lend_unit");
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
        ));
    }

    /// A group's lend is a frame the contract takes, and her line in the
    /// group goes back to it, in its topic.
    #[tokio::test]
    async fn a_lent_group_is_a_contract_frame_and_answered_there() {
        let db = DatabaseConnection::default();
        let (send, mut lent) = tokio::sync::mpsc::unbounded_channel();
        open("dev_lend_group", send);
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
            },
            &[line],
            &[before],
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
        close("dev_lend_group");
    }
}
