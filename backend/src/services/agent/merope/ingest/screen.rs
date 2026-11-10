//! A look at the owner's screen that a child end told her
//! (`agent.merope.screen`): what it becomes of hers is decided by whether
//! she took it in at that moment, in the decision she already makes about
//! it, never by what it shows. The rules are the shared core's
//! (`myriad_merope::screen`), as on the Mac that sees it.
//!
//! - **Taken in**: her words (none of the window's text she was shown in
//!   them) as a span on the note of the owner's she chose among those
//!   offered her, or else in the day's loose ends (the inbox).
//! - **Only knew**: one fact for its stretch (the same app and window or
//!   file, looked at on end), when and what was in front, nothing of hers.
//! - **Let go**: nothing.
//!
//! A look at a chat window (only who and what about) or a private window is
//! never kept: another person's conversation is not the owner's memory.
//! Only the owner's private talk is written, and only while her mind is
//! here.

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use myriad_merope::screen;
use myriad_nodes::SaidBy;
use sea_orm::DatabaseConnection;
use serde_json::json;

use super::told::Told;
use crate::services::agent::consciousness::{ConsciousnessDecision, ConsciousnessEvent};
use crate::services::agent::merope::memory_nodes::{
    Author, Beside, RowIn, audience, inbox, known, write_with,
};

/// One look as her moment carries it, when it may become memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Look {
    pub app: String,
    /// The file the editor has open, else the window's title.
    pub thing: Option<String>,
    /// The look's own id on the child (else the moment's).
    pub id: String,
    /// The same for every look at one app and window or file on end.
    pub stretch: Option<String>,
    pub at: DateTime<Utc>,
}

impl Look {
    /// The look of `told`, none for what may not become memory: a chat
    /// window (who and what about), a private window, a look with no app.
    pub(super) fn of(told: &Told) -> Option<Self> {
        let facts = &told.safe_facts;
        if facts.contains_key("who") || facts.contains_key("private") {
            return None;
        }
        let app = facts.get("app")?.trim();
        if app.is_empty() {
            return None;
        }
        let thing = facts
            .get("file")
            .or_else(|| facts.get("window_title"))
            .map(|thing| thing.trim().to_string())
            .filter(|thing| !thing.is_empty());
        Some(Self {
            app: app.to_string(),
            thing,
            id: facts
                .get("look_id")
                .filter(|id| !id.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| told.id.clone()),
            stretch: facts
                .get("stretch")
                .filter(|stretch| !stretch.trim().is_empty())
                .cloned(),
            at: told.occurred_at,
        })
    }
}

/// The notes she is offered with a look: label (`n1`…) to node id, in the
/// order she reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Offered {
    pub notes: Vec<(String, String)>,
}

/// The notes of the owner's private talk the look's app and window share
/// most words with (the core's `screen::notes`), and the lines she reads
/// them as under the look: labels, kinds and names, no id.
pub(super) async fn offer(
    db: &DatabaseConnection,
    user_id: i32,
    look: &Look,
) -> (Offered, Option<String>) {
    let audience = audience::private_talk(user_id, Some(user_id));
    let known = match known::known(db, user_id, &audience).await {
        Ok(known) => known,
        Err(error) => {
            tracing::debug!(%error, "[Merope] a look's notes could not be read");
            return (Offered::default(), None);
        }
    };
    let kinds: Vec<String> = known
        .iter()
        .map(|node| node.kind.clone().unwrap_or_else(|| "topic".into()))
        .collect();
    let candidates: Vec<screen::Note> = known
        .iter()
        .zip(&kinds)
        .map(|(node, kind)| screen::Note {
            kind,
            title: &node.name,
            name: None,
            summary: None,
            aliases: &node.aliases,
            threads: &node.holds,
            active: false,
            updated_at: node.latest(),
        })
        .collect();
    let picked = screen::notes(&look.app, look.thing.as_deref(), &candidates);
    if picked.is_empty() {
        return (Offered::default(), None);
    }
    let mut offered = Offered::default();
    let mut lines = Vec::new();
    for (place, index) in picked.into_iter().enumerate() {
        let label = screen::label(place);
        lines.push(format!(
            "- {label}: {} «{}»",
            kinds[index], known[index].name
        ));
        offered.notes.push((label, known[index].id.clone()));
    }
    let section = format!(
        "Your notes this may belong with (labels only, for note):\n{}",
        lines.join("\n")
    );
    (offered, Some(section))
}

/// The stretches whose fact is kept already, newest last: one fact a
/// stretch. Kept in this process only; after a restart a stretch going on
/// may have its fact once more, which fades as any.
static FACTS_KEPT: LazyLock<Mutex<VecDeque<(i32, String)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
const FACTS_REMEMBERED: usize = 256;

/// Whether the fact of `stretch` is new for `user_id` (and now kept).
fn first_of_stretch(user_id: i32, stretch: &str) -> bool {
    let Ok(mut kept) = FACTS_KEPT.lock() else {
        return true;
    };
    let key = (user_id, stretch.to_string());
    if kept.contains(&key) {
        return false;
    }
    if kept.len() >= FACTS_REMEMBERED {
        kept.pop_front();
    }
    kept.push_back(key);
    true
}

/// Where a look went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Kept {
    /// On that node, as a span of hers.
    Node(String),
    /// In the day's loose ends.
    Inbox,
    /// One fact of its stretch.
    Fact,
    Nothing,
}

/// What `decision` makes of `look`, kept: the take without any of `shown`
/// (the window's text she was given), on the note she chose or in the
/// inbox; else, a look that caught her and that she only knew of, its
/// stretch's one fact. `origin` is the device it came from.
pub(super) async fn keep(
    db: &DatabaseConnection,
    user_id: i32,
    event: &ConsciousnessEvent,
    look: &Look,
    offered: &Offered,
    decision: &ConsciousnessDecision,
    shown: &str,
    origin: &str,
) -> Kept {
    let take = screen::unquoted(
        screen::Take {
            took_in: clean(decision.took_in.as_deref(), screen::TAKE_CHARS),
            mood: clean(decision.mood.as_deref(), screen::MOOD_CHARS),
            note: clean(decision.note.as_deref(), screen::NOTE_CHARS),
        },
        Some(shown),
    );
    match take.took_in {
        Some(text) => taken_in(db, user_id, event, look, offered, &text, take.note).await,
        None => only_knew(db, user_id, event, look, origin).await,
    }
}

/// One line of hers: trimmed, a break inside it a space, at most `limit`.
fn clean(text: Option<&str>, limit: usize) -> Option<String> {
    let text = text?.split_whitespace().collect::<Vec<_>>().join(" ");
    let kept: String = text.chars().take(limit).collect();
    (!kept.is_empty()).then_some(kept)
}

async fn taken_in(
    db: &DatabaseConnection,
    user_id: i32,
    event: &ConsciousnessEvent,
    look: &Look,
    offered: &Offered,
    text: &str,
    note: Option<String>,
) -> Kept {
    let Some(pen) = crate::services::her_log::pen::think() else {
        tracing::info!("[Merope] a look not kept: her mind is on another device");
        return Kept::Nothing;
    };
    let audience = audience::private_talk(user_id, Some(user_id));
    let reference = format!("event:{}", event.id);
    let at = look.at.timestamp_micros();
    // Her words about something a program showed: hers, of a third party's.
    let said_by = vec![SaidBy::Her, SaidBy::ThirdParty];
    let node = note.and_then(|label| {
        offered
            .notes
            .iter()
            .find(|(offered, _)| *offered == label)
            .map(|(_, node)| node.clone())
    });
    let mut rows = Vec::new();
    let mut beside = Beside::default();
    let kept = match &node {
        Some(node) => {
            rows.push(RowIn {
                id: myriad_nodes::entries::row_id_from(
                    "node.span",
                    node,
                    &format!("screen-{}", look.id),
                    "",
                ),
                node: node.clone(),
                kind: "node.span".into(),
                body: json!({
                    "version": myriad_nodes::BODY_VERSION, "sync": "shared",
                    "saidBy": said_by, "audience": audience, "text": text,
                    "refs": [reference], "from": at, "to": at,
                }),
                author: Author::Her,
            });
            Kept::Node(node.clone())
        }
        None => {
            beside.inbox.push(inbox::Item {
                day: Some(crate::services::agent::merope::clock::today()),
                id: reference.clone(),
                said_by,
                text: text.to_string(),
                refs: vec![reference],
                from: Some(at),
                to: Some(at),
            });
            Kept::Inbox
        }
    };
    match write_with(db, user_id, &audience, &rows, Some(&pen), &beside).await {
        Ok(_) => kept,
        Err(error) => {
            tracing::warn!(%error, "[Merope] what she took in of a look was not kept");
            Kept::Nothing
        }
    }
}

async fn only_knew(
    db: &DatabaseConnection,
    user_id: i32,
    event: &ConsciousnessEvent,
    look: &Look,
    origin: &str,
) -> Kept {
    let stretch = look.stretch.as_deref().unwrap_or(&look.id);
    if !first_of_stretch(user_id, stretch) {
        return Kept::Nothing;
    }
    let language = persona_language(db).await;
    let fact = screen::fact(
        &look.app,
        look.thing.as_deref(),
        look.at,
        &language,
        &crate::services::agent::merope::clock::zone(),
    );
    match crate::services::agent::merope::store::insert_remembered_as(
        db,
        user_id,
        &fact,
        Some(&event.headline),
        origin,
        screen::FACT_SOURCE,
    )
    .await
    {
        Ok(true) => Kept::Fact,
        Ok(false) => Kept::Nothing,
        Err(error) => {
            tracing::warn!(%error, "[Merope] the fact of a look was not kept");
            Kept::Nothing
        }
    }
}

/// Her persona's language (`zh` when it names none).
async fn persona_language(db: &DatabaseConnection) -> String {
    crate::services::agent::merope::store::get_persona(db)
        .await
        .ok()
        .flatten()
        .and_then(|persona| {
            serde_json::from_str::<serde_json::Value>(&persona.personality)
                .ok()?
                .get("language")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| "zh".into())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn told(facts: &[(&str, &str)]) -> Told {
        Told {
            id: "mo_1".into(),
            headline: "h".into(),
            safe_facts: facts
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>(),
            urgency: crate::services::agent::consciousness::EventUrgency::Normal,
            occurred_at: DateTime::from_timestamp(1_793_750_400, 0).unwrap(),
            origin: "dev_mac".into(),
            keeps_memory: false,
        }
    }

    #[test]
    fn only_a_look_at_an_app_may_become_memory() {
        let look = Look::of(&told(&[
            ("app", "Word"),
            ("window_title", "报告.docx - Word"),
            ("file", "报告.docx"),
            ("look_id", "sense.sight-1-2-look"),
            ("stretch", "s1"),
        ]))
        .unwrap();
        assert_eq!(look.thing.as_deref(), Some("报告.docx"));
        assert_eq!(look.id, "sense.sight-1-2-look");
        assert_eq!(look.stretch.as_deref(), Some("s1"));
        // Another person's conversation, a private window, no app: never.
        assert_eq!(Look::of(&told(&[("who", "小林"), ("topic", "周末")])), None);
        assert_eq!(
            Look::of(&told(&[("app", "Safari"), ("private", "true")])),
            None
        );
        assert_eq!(Look::of(&told(&[("window_title", "x")])), None);
        // Without its own id, the moment's.
        assert_eq!(Look::of(&told(&[("app", "Zed")])).unwrap().id, "mo_1");
    }

    #[test]
    fn a_stretch_has_one_fact() {
        assert!(first_of_stretch(-7, "stretch_a"));
        assert!(!first_of_stretch(-7, "stretch_a"));
        assert!(first_of_stretch(-7, "stretch_b"));
        assert!(first_of_stretch(-8, "stretch_a"));
    }

    fn decision(took_in: Option<&str>, note: Option<&str>) -> ConsciousnessDecision {
        ConsciousnessDecision {
            action: crate::services::agent::consciousness::ConsciousnessAction::Ignore,
            reason_code: "r".into(),
            confidence: 0.5,
            memory: None,
            speech: None,
            question: None,
            work_proposal: None,
            took_in: took_in.map(Into::into),
            mood: took_in.map(|_| "开心".into()),
            note: note.map(Into::into),
        }
    }

    fn event(id: &str) -> ConsciousnessEvent {
        ConsciousnessEvent {
            id: id.into(),
            source: "dev_mac".into(),
            kind: crate::services::agent::merope::gates::SCREEN_EVENT.into(),
            headline: "在看报告".into(),
            summary: String::new(),
            addressee_user_id: 7,
            urgency: Default::default(),
            occurred_at: DateTime::from_timestamp(1_793_750_400, 0).unwrap(),
            parent_event_id: None,
            safe_facts: BTreeMap::new(),
            keeps_memory: false,
        }
    }

    /// What she took in of a look goes on the note she chose, or in the
    /// day's loose ends, without the window's words; a look she only knew
    /// of is one fact for its stretch; nothing of a look goes elsewhere.
    #[tokio::test]
    async fn a_look_is_kept_by_whether_she_took_it_in() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "screen_looks").await;
        let db = &schema.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'seven')")
            .await
            .unwrap();
        let talk = audience::private_talk(7, Some(7));
        // A note of the owner's about the report.
        let made = myriad_nodes::entries::fact_rows(
            serde_json::from_value(json!({
                "audience": talk,
                "start": {"name": "季度报告"},
                "facts": [{"memory": "m0", "text": "在写季度报告", "saidBy": ["partner"]}],
            }))
            .unwrap(),
        )
        .unwrap();
        let rows: Vec<RowIn> = made
            .rows
            .into_iter()
            .map(|row| RowIn {
                id: row.id,
                node: made.node.clone(),
                kind: row.kind,
                body: row.body,
                author: Author::Her,
            })
            .collect();
        let pen = crate::services::her_log::pen::think().unwrap();
        crate::services::agent::merope::memory_nodes::write(db, 7, &talk, &rows, Some(&pen))
            .await
            .unwrap();
        drop(pen);
        let look = Look::of(&told(&[
            ("app", "Word"),
            ("file", "季度报告.docx"),
            ("look_id", "look_1"),
            ("stretch", "stretch_report"),
        ]))
        .unwrap();
        let (offered, section) = offer(db, 7, &look).await;
        assert_eq!(offered.notes, vec![("n1".to_string(), made.node.clone())]);
        assert!(section.unwrap().contains("n1: topic «季度报告»"));
        let shown = "本季度营收同比增长百分之十二点五主要来自海外市场";

        // Taken in, with the note she chose: a span there, the copied
        // words taken out.
        let kept = keep(
            db,
            7,
            &event("evt_1"),
            &look,
            &offered,
            &decision(
                Some("本季度营收同比增长百分之十二点五，她写得很认真"),
                Some("n1"),
            ),
            shown,
            "dev_mac",
        )
        .await;
        assert_eq!(kept, Kept::Node(made.node.clone()));
        let node =
            crate::services::agent::merope::memory_nodes::read::node(db, 7, &talk, &made.node)
                .await
                .unwrap()
                .unwrap();
        let spans = node["spans"].as_array().unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0]["text"], "…，她写得很认真");
        assert_eq!(spans[0]["refs"], json!(["event:evt_1"]));
        assert_eq!(spans[0]["saidBy"], json!(["her", "third_party"]));

        // Taken in, no note: the day's loose ends.
        let other = Look {
            id: "look_2".into(),
            ..look.clone()
        };
        let kept = keep(
            db,
            7,
            &event("evt_2"),
            &other,
            &offered,
            &decision(Some("想帮忙看看结构"), Some("n9")),
            shown,
            "dev_mac",
        )
        .await;
        assert_eq!(kept, Kept::Inbox);
        let waiting = inbox::waiting(db, 7, &talk, 10).await.unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].id, "event:evt_2");
        assert_eq!(waiting[0].text, "想帮忙看看结构");

        // Only knew of it: one fact for the stretch, and only one.
        let knew = Look {
            id: "look_3".into(),
            stretch: Some(format!("stretch_{}", std::process::id())),
            ..look.clone()
        };
        let facts = "SELECT COUNT(*) AS n FROM agent_memories WHERE source = 'screen'";
        let count = || async {
            db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, facts))
                .await
                .unwrap()
                .unwrap()
                .try_get::<i64>("", "n")
                .unwrap()
        };
        for at in ["evt_3", "evt_4"] {
            let kept = keep(
                db,
                7,
                &event(at),
                &knew,
                &offered,
                &decision(None, None),
                shown,
                "dev_mac",
            )
            .await;
            assert_eq!(
                kept,
                if at == "evt_3" {
                    Kept::Fact
                } else {
                    Kept::Nothing
                }
            );
        }
        assert_eq!(count().await, 1);
        // What she said was all the window's: she only knew of it.
        let copied = Look {
            id: "look_5".into(),
            at: look.at + chrono::Duration::hours(1),
            stretch: Some(format!("stretch_copied_{}", std::process::id())),
            ..look.clone()
        };
        let kept = keep(
            db,
            7,
            &event("evt_5"),
            &copied,
            &offered,
            &decision(Some("本季度营收同比增长百分之十二点五"), Some("n1")),
            shown,
            "dev_mac",
        )
        .await;
        assert_eq!(kept, Kept::Fact);
        assert_eq!(count().await, 2);
        schema.drop().await;
    }

    #[test]
    fn her_line_is_one_line_within_its_limit() {
        assert_eq!(
            clean(Some("  想\n去  看看 "), 320).as_deref(),
            Some("想 去 看看")
        );
        assert_eq!(clean(Some("   "), 320), None);
        assert_eq!(clean(Some("abcdef"), 3).as_deref(), Some("abc"));
    }
}
