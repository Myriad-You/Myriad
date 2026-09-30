//! Her views: what grows out of her own time.
//!
//! At night she goes over what she listened to and read lately, with what
//! stayed with her each time, and the views she already holds. Where things
//! add up to a view of her own (about an artist, a kind of song, a subject)
//! she writes it down in a sentence; where something changed her mind she
//! says so, and the old view is kept as what she used to think. The model
//! judges all of it; nothing counts plays or scores tastes.
//!
//! A view is about public things and grew out of her own time, so every
//! conversation may hear it. When their words touch one she has it at hand
//! and answers from it: the same on Tuesday as on Friday, to one person as to
//! another, unless she changed her mind. Her views also go into what she picks
//! to do next, so a taste can grow.

use std::sync::{LazyLock, Mutex};

use chrono::{NaiveDate, Utc};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde_json::{Value, json};

use crate::models::entities::agent_memories;
use crate::services::agent::memory::lexical;
use crate::services::agent::memory::unified::{self, Concept};
use myriad_merope::views::{
    GREW_FROM, MAX_CHANGES, SCHEMA_NAME, parse, same_subject, schema, spread, system,
};

const LOOK_BACK: chrono::Duration = chrono::Duration::days(14);
/// Read from the whole window (she does up to a few dozen things a day),
/// then spread down to what one going-over can hold.
const WINDOW_ROWS: u64 = 600;
const EXPERIENCES: usize = 60;
/// Views matched against; the most recent of them go into the prompt.
const HELD: u64 = 300;
const HELD_IN_PROMPT: usize = 60;
/// A view not borne out again for this long fades.
const VIEW_FADES_AFTER: chrono::Duration = chrono::Duration::days(60);
/// Her own time older than this fades; the views it grew into stay.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(30);
const PURGE_AFTER: chrono::Duration = chrono::Duration::days(90);

/// The night her views were last gone over, so a night does it once.
static DONE_ON: LazyLock<Mutex<Option<NaiveDate>>> = LazyLock::new(|| Mutex::new(None));

/// A view she holds: what it is about, and what she thinks.
fn view_of(row: &agent_memories::Model) -> Option<(String, String)> {
    let evidence: Value = serde_json::from_str(row.evidence.as_deref()?).ok()?;
    let about = evidence.get("about")?.as_str()?.trim().to_string();
    (!about.is_empty()).then(|| (about, row.content.clone()))
}

/// A view with what it grew out of, and what she thought before if she
/// changed her mind, as she holds it when it comes up.
fn grown_view(row: &agent_memories::Model) -> Option<(String, String)> {
    let (about, view) = view_of(row)?;
    let evidence: Value = row
        .evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or_default();
    let grew_from: Vec<String> = evidence
        .get("grewFrom")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|from| from.get("what")?.as_str().map(str::to_string))
        .collect();
    let mut notes = Vec::new();
    if let Some(was) = evidence
        .get("was")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|was| !was.is_empty())
    {
        notes.push(format!("you used to think: {was}"));
    }
    if !grew_from.is_empty() {
        notes.push(format!("it grew from: {}", grew_from.join("; ")));
    }
    if notes.is_empty() {
        return Some((about, view));
    }
    Some((about, format!("{view} ({})", notes.join("; "))))
}

/// A new persona has not gone over anything yet.
pub(super) fn forget() {
    if let Ok(mut done) = DONE_ON.lock() {
        *done = None;
    }
}

/// Go over her own time and let views grow or change. Once a night, and only
/// when she has done something since she last did.
pub async fn go_over(db: &DatabaseConnection, owner: i32) {
    let today = chrono::Local::now().date_naive();
    if DONE_ON.lock().is_ok_and(|done| *done == Some(today)) {
        return;
    }
    let now = Utc::now();
    let Ok(experiences) = unified::own_experiences(db, WINDOW_ROWS).await else {
        return;
    };
    let experiences: Vec<agent_memories::Model> = spread(
        experiences
            .into_iter()
            .filter(|row| now.signed_duration_since(row.created_at) < LOOK_BACK)
            .collect(),
        EXPERIENCES,
    );
    let Ok(held) = unified::own_views(db, HELD).await else {
        return;
    };
    // Done for the night once gone over, or once there is nothing to go
    // over; a call that fails is tried again on the next look.
    let done = || {
        if let Ok(mut done) = DONE_ON.lock() {
            *done = Some(today);
        }
    };
    let newest_view = held.iter().map(|row| row.created_at).max();
    let anything_new = experiences
        .iter()
        .any(|row| newest_view.is_none_or(|at| row.created_at > at));
    if experiences.len() < 2 || !anything_new {
        done();
        return;
    }
    // The views shown, numbered, so she can say which ones hold up.
    let shown: Vec<(String, String, String)> = held
        .iter()
        .take(HELD_IN_PROMPT)
        .filter_map(|row| view_of(row).map(|(about, view)| (row.id.clone(), about, view)))
        .collect();
    let input = json!({
        "experiences": experiences
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                Some(json!({
                    "index": index,
                    "what": super::doing::experience_line(row)?,
                    "stayed": row.content,
                }))
            })
            .collect::<Vec<_>>(),
        "views": shown
            .iter()
            .enumerate()
            .map(|(i, (_, about, view))| json!({ "i": i, "about": about, "view": view }))
            .collect::<Vec<_>>(),
    })
    .to_string();
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let raw = super::call::Ask::new(super::call::Voice::Hers, owner, SCHEMA_NAME)
        .within(std::time::Duration::from_secs(60))
        .json_raw(&system(&soul), &input, SCHEMA_NAME, &schema())
        .await;
    let Some(changes) = raw.ok().and_then(|raw| parse(&raw)) else {
        tracing::info!("[Merope] could not go over her own time");
        return;
    };
    done();
    // Views the experiences bear out again stay fresh; the rest age.
    let borne_out: Vec<String> = changes
        .borne_out
        .iter()
        .filter(|borne| borne.from.iter().any(|index| *index < experiences.len()))
        .filter_map(|borne| shown.get(borne.i).map(|(id, _, _)| id.clone()))
        .collect();
    if !borne_out.is_empty() {
        let now = Utc::now().fixed_offset();
        if let Err(error) = agent_memories::Entity::update_many()
            .col_expr(
                agent_memories::Column::UpdatedAt,
                sea_orm::sea_query::Expr::value(now),
            )
            .filter(agent_memories::Column::Id.is_in(borne_out.clone()))
            .filter(agent_memories::Column::Venue.eq(unified::OWN_VENUE))
            .exec(db)
            .await
        {
            tracing::warn!(%error, "[Merope] could not keep her borne-out views fresh");
        }
    }
    // What she holds, kept current as this pass changes it.
    let mut holding: Vec<(String, String, String)> = held
        .iter()
        .filter_map(|row| view_of(row).map(|(about, view)| (row.id.clone(), about, view)))
        .collect();
    let mut kept = 0;
    for change in changes.views.into_iter().take(MAX_CHANGES) {
        let about: String = change.about.trim().chars().take(40).collect();
        let view = super::ingest::compact_summary(&change.view);
        let sources: Vec<&agent_memories::Model> = change
            .from
            .iter()
            .filter_map(|index| experiences.get(*index))
            .collect();
        // A view comes out of something she did, or it is not hers.
        if about.is_empty() || view.is_empty() || sources.is_empty() {
            continue;
        }
        // What she thought before she changed her mind stays with the new
        // view: she knows she used to think otherwise.
        let mut was = None;
        if let Some(at) = holding
            .iter()
            .position(|(_, subject, _)| same_subject(subject, &about))
        {
            if holding[at].2 == view {
                continue;
            }
            let (id, _, old) = holding.remove(at);
            let _ = unified::retire_own(db, &id, "changed_mind").await;
            was = Some(old);
        }
        let mut concepts = vec![Concept {
            name: about.clone(),
            aliases: Vec::new(),
        }];
        for source in &sources {
            concepts.extend(
                serde_json::from_value::<Vec<Concept>>(source.concepts.clone()).unwrap_or_default(),
            );
        }
        concepts.truncate(5);
        // What it grew out of, so she knows where it came from.
        let grew_from: Vec<Value> = sources
            .iter()
            .take(GREW_FROM)
            .filter_map(|row| {
                Some(json!({ "id": row.id, "what": super::doing::experience_line(row)? }))
            })
            .collect();
        if let Ok(Some(id)) = unified::remember_own(
            db,
            &view,
            &json!({
                "about": about,
                "changed": change.changed,
                "grewFrom": grew_from,
                "was": was,
            })
            .to_string(),
            concepts,
            unified::OWN_VIEW,
        )
        .await
        {
            holding.push((id, about, view));
            kept += 1;
        }
    }
    tracing::info!(kept, "[Merope] went over her own time");
}

/// Her own time older than a month fades once its views are drawn; faded
/// rows go for good later. Runs each night, apart from going over.
pub async fn let_fade(db: &DatabaseConnection) {
    // A view nothing has borne out for this long is a taste no longer acted
    // on: it fades.
    match unified::fade_source(db, unified::OWN_VIEW, VIEW_FADES_AFTER).await {
        Ok(faded) if faded > 0 => tracing::info!(faded, "[Merope] views no longer borne out faded"),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not let old views fade"),
    }
    match unified::fade_own_experiences(db, FADE_AFTER, PURGE_AFTER).await {
        Ok((faded, purged)) if faded + purged > 0 => {
            tracing::info!(faded, purged, "[Merope] older own time faded")
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not let her older own time fade"),
    }
}

/// The views their words touch, for a prompt: (about, view).
pub async fn touched(db: &DatabaseConnection, words: &str, limit: usize) -> Vec<(String, String)> {
    if words.trim().is_empty() {
        return Vec::new();
    }
    let Ok(rows) = unified::own_views(db, HELD).await else {
        return Vec::new();
    };
    let views: Vec<(String, String)> = rows.iter().filter_map(view_of).collect();
    let grown: Vec<(String, String)> = rows.iter().filter_map(grown_view).collect();
    let texts: Vec<String> = views
        .iter()
        .map(|(about, view)| format!("{about} {view}"))
        .collect();
    let concepts: Vec<Vec<Concept>> = rows
        .iter()
        .filter(|row| view_of(row).is_some())
        .map(|row| serde_json::from_value(row.concepts.clone()).unwrap_or_default())
        .collect();
    let documents: Vec<lexical::Document> = texts
        .iter()
        .zip(&concepts)
        .map(|(text, concepts)| lexical::Document { text, concepts })
        .collect();
    let mut scored: Vec<(usize, f64)> = lexical::score_all(words, &documents)
        .into_iter()
        .enumerate()
        .filter(|(_, score)| score.strong)
        .map(|(index, score)| (index, score.value))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored
        .into_iter()
        .take(limit)
        .map(|(index, _)| grown[index].clone())
        .collect()
}

/// The views she holds, most recent first, as "about: view".
pub async fn held(db: &DatabaseConnection, limit: u64) -> Vec<String> {
    unified::own_views(db, limit)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(grown_view)
        .map(|(about, view)| format!("{about}: {view}"))
        .collect()
}

/// The going-over call as production sends it, for the semantic suite.
#[cfg(test)]
pub(crate) fn probe_contract(soul: &str) -> (String, Value) {
    (system(soul), schema())
}

/// The views a going-over wrote, if it honors the contract.
#[cfg(test)]
pub(crate) fn parse_views(raw: &str) -> Option<Vec<(String, String)>> {
    parse(raw).map(|changes| {
        changes
            .views
            .into_iter()
            .map(|change| (change.about, change.view))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view_row(evidence: Value) -> agent_memories::Model {
        let now = Utc::now().fixed_offset();
        agent_memories::Model {
            id: "v".into(),
            user_id: None,
            kind: "knowledge".into(),
            content: "ヨルシカ的歌越往后越有劲".into(),
            evidence: Some(evidence.to_string()),
            speaker: "agent".into(),
            source: unified::OWN_VIEW.into(),
            venue: unified::OWN_VENUE.into(),
            audience: json!([]),
            concepts: json!([]),
            importance: 0.5,
            access_count: 0,
            last_accessed_at: None,
            valid_from: now,
            invalid_at: None,
            invalid_reason: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn a_changed_mind_remembers_what_it_was() {
        let changed = view_row(json!({
            "about": "ヨルシカ",
            "was": "ヨルシカ的歌都太闷",
            "grewFrom": [{ "id": "e", "what": "listening to 「花に亡霊」" }],
        }));
        assert_eq!(
            grown_view(&changed).unwrap().1,
            "ヨルシカ的歌越往后越有劲 (you used to think: ヨルシカ的歌都太闷; it grew from: listening to 「花に亡霊」)"
        );
        let first = view_row(json!({ "about": "ヨルシカ", "was": null }));
        assert_eq!(grown_view(&first).unwrap().1, "ヨルシカ的歌越往后越有劲");
    }

    #[test]
    fn a_going_over_sees_the_whole_window() {
        let rows: Vec<agent_memories::Model> = (0..300)
            .map(|index| agent_memories::Model {
                id: format!("own_{index}"),
                user_id: None,
                kind: "knowledge".into(),
                content: String::new(),
                evidence: None,
                speaker: "agent".into(),
                source: unified::OWN_EXPERIENCE.into(),
                venue: unified::OWN_VENUE.into(),
                audience: json!([]),
                concepts: json!([]),
                importance: 0.4,
                access_count: 0,
                last_accessed_at: None,
                valid_from: Utc::now().fixed_offset(),
                invalid_at: None,
                invalid_reason: None,
                created_at: Utc::now().fixed_offset(),
                updated_at: Utc::now().fixed_offset(),
            })
            .collect();
        // Newest first in: own_0 is the newest, own_299 the oldest.
        let spread = spread(rows, 60);
        assert_eq!(spread.len(), 60);
        assert_eq!(
            spread[0].id, "own_299",
            "oldest first, from the start of the window"
        );
        assert_eq!(spread.last().unwrap().id, "own_4", "and on to its end");
    }
    #[test]
    fn views_grow_out_of_what_she_did_and_may_change() {
        let prompt = system("你是小灯。");
        assert!(prompt.contains("changed true"));
        assert!(prompt.contains("Only what these experiences support"));
        assert!(prompt.contains("nothing about any person you talk with"));
        assert!(prompt.contains("If nothing adds up, views is empty"));
        let parsed = parse_views(
            r#"{"views":[{"about":"amazarashi","view":"他们的歌冲得狠，但我总等不到收尾。","changed":false,"from":[0,2]}]}"#,
        )
        .unwrap();
        assert_eq!(parsed[0].0, "amazarashi");
        assert!(parse_views(r#"{"views":[{"about":"x","view":"y"}]}"#).is_none());
        assert_eq!(parse_views(r#"{"views":[]}"#), Some(Vec::new()));
        // A view the night's experiences bear out again is said so, by its i.
        assert!(prompt.contains("a view nothing bears out for a long while fades"));
        let changes = parse(r#"{"views":[],"borneOut":[{"i":1,"from":[0,3]}]}"#).expect("changes");
        assert_eq!(changes.borne_out[0].i, 1);
        assert_eq!(changes.borne_out[0].from, vec![0, 3]);
    }

    #[test]
    fn a_view_is_read_back_by_its_subject() {
        let row = agent_memories::Model {
            id: "own_1".into(),
            user_id: None,
            kind: "knowledge".into(),
            content: "他们的歌冲得狠。".into(),
            evidence: Some(r#"{"about":"amazarashi","changed":false}"#.into()),
            speaker: "agent".into(),
            source: unified::OWN_VIEW.into(),
            venue: unified::OWN_VENUE.into(),
            audience: json!([]),
            concepts: json!([]),
            importance: 0.4,
            access_count: 0,
            last_accessed_at: None,
            valid_from: Utc::now().fixed_offset(),
            invalid_at: None,
            invalid_reason: None,
            created_at: Utc::now().fixed_offset(),
            updated_at: Utc::now().fixed_offset(),
        };
        assert_eq!(
            view_of(&row),
            Some(("amazarashi".into(), "他们的歌冲得狠。".into()))
        );
        assert!(same_subject(" Amazarashi", "amazarashi "));
        assert!(same_subject("灰色公路", "灰色公路的歌"));
        assert!(!same_subject("歌", "灰色公路的歌"));
        assert!(!same_subject("amazarashi", "Mili"));
        // Read back with what it grew out of, when that was kept.
        assert_eq!(grown_view(&row), view_of(&row));
        let grown = agent_memories::Model {
            evidence: Some(
                r#"{"about":"amazarashi","changed":false,"grewFrom":[{"id":"doing_1","what":"listened to the song 「スピードと摩擦」 by amazarashi (you liked it)"}]}"#
                    .into(),
            ),
            ..row
        };
        assert_eq!(
            grown_view(&grown).unwrap().1,
            "他们的歌冲得狠。 (it grew from: listened to the song 「スピードと摩擦」 by amazarashi (you liked it))"
        );
    }
}
