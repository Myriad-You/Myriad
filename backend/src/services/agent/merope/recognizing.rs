//! Recognizing someone from elsewhere (see `myriad_merope::recognizing`):
//! after she has talked with someone from outside the community in a group,
//! whether they might be a person she already knows, from the rare things
//! they said that only that person would, and how they type.
//!
//! The guess is kept with the group, apart from ordinary memory, and heard
//! only when she answers them there. It opens nothing up: who may know
//! what stays with pairing, and pairing their account is how a guess
//! becomes knowing (`strangers::adopt`).

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::call::{self, Voice};
use super::strangers::Stranger;
use crate::services::agent::memory::unified::{self, Audience, MemoryKind};
use myriad_merope::recognizing::{
    Known, RARE_NEEDED, SCHEMA_NAME, parse, rare_shared, schema, section, system,
};
use myriad_merope::strangers::evidence_marker;

pub const SOURCE: &str = "maybe_is";
/// A guess not looked at again for this long fades, as the note it was a
/// guess about does (see `strangers`).
const FADE_AFTER: chrono::Duration = chrono::Duration::days(60);
/// People she knows that a stranger is held against, and memories of each.
const PEOPLE: i64 = 50;
const MEMORIES: usize = 300;
/// Typing this close to someone's own counts as typing like them.
const TYPES_LIKE: f64 = 1.5;
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn group_venue(venue: &str) -> String {
    Audience::group(venue, 0).venue()
}

/// Whom she takes this stranger in this group for, if anyone: (who, how
/// sure, why).
pub async fn guess(
    db: &DatabaseConnection,
    venue: &str,
    stranger: &Stranger,
) -> Option<(i32, String, String)> {
    let row = unified::unowned_with_evidence(
        db,
        &group_venue(venue),
        SOURCE,
        &evidence_marker(&stranger.who),
    )
    .await
    .ok()??;
    let evidence: Value = serde_json::from_str(row.evidence.as_deref()?).ok()?;
    Some((
        i32::try_from(evidence.get("userId")?.as_i64()?).ok()?,
        evidence.get("sure")?.as_str()?.to_string(),
        row.content,
    ))
}

/// What she wonders about this stranger, for her reply to them there.
pub async fn guess_section(
    db: &DatabaseConnection,
    venue: &str,
    stranger: &Stranger,
) -> Option<String> {
    let (user_id, sure, why) = guess(db, venue, stranger).await?;
    let candidate = super::resolve_addressee_label(db, user_id).await;
    Some(section(&stranger.name, &candidate, &sure, &why))
}

/// Having talked with them (their lines `said`), whether they might be
/// someone she knows; the judgment is billed to `owner`.
pub async fn consider(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    stranger: &Stranger,
    said: &[String],
) {
    let text = said.join("\n");
    if text.trim().is_empty() {
        return;
    }
    let people = unified::people_remembered(db, PEOPLE)
        .await
        .unwrap_or_default();
    let mut memories: Vec<(i32, Vec<String>)> = Vec::with_capacity(people.len());
    for user_id in people {
        let rows = unified::active_in(
            db,
            user_id,
            &Audience::private(user_id),
            &MemoryKind::ABOUT_PERSON,
        )
        .await
        .unwrap_or_default();
        memories.push((
            user_id,
            rows.into_iter()
                .take(MEMORIES)
                .map(|row| row.content)
                .collect(),
        ));
    }
    let known: Vec<Known> = memories
        .iter()
        .map(|(user_id, memories)| Known {
            user_id: *user_id,
            memories,
        })
        .collect();
    let mut chosen = None;
    for (user_id, rare, touched) in rare_shared(&text, &known).into_iter().take(2) {
        let history: Vec<String> = super::store::their_lines(db, user_id, 1500)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(_, line)| line)
            .collect();
        let typing = myriad_merope::style::unlike(&history, said);
        let types_like = typing.is_some_and(|far| far < TYPES_LIKE);
        if rare.len() >= RARE_NEEDED || (!rare.is_empty() && types_like) {
            chosen = Some((user_id, touched, typing));
            break;
        }
    }
    let marker = evidence_marker(&stranger.who);
    let group = group_venue(venue);
    let held = unified::unowned_with_evidence(db, &group, SOURCE, &marker)
        .await
        .ok()
        .flatten();
    let Some((user_id, touched, typing)) = chosen else {
        return;
    };
    let candidate = super::resolve_addressee_label(db, user_id).await;
    let typing = match typing {
        Some(far) if far < TYPES_LIKE => "they type much the way the candidate does",
        Some(far) if far >= myriad_merope::style::NOTICED_AT => {
            "they type quite unlike the candidate"
        }
        Some(_) => "their typing neither matches nor rules out the candidate",
        None => "too little to compare their typing",
    };
    let input = json!({
        "theirLines": said,
        "candidate": candidate,
        "touches": touched,
        "typing": typing,
    })
    .to_string();
    let Ok(raw) = call::Ask::new(Voice::Judge, owner, "recognizing")
        .within(CALL_TIMEOUT)
        .json_raw(&system(), &input, SCHEMA_NAME, &schema())
        .await
    else {
        return;
    };
    let Some(judged) = parse(&raw) else {
        return;
    };
    if let Some(row) = &held {
        let _ = unified::retire_unowned(db, &group, &row.id, "superseded").await;
    }
    if let Some((sure, why)) = judged {
        // Kept in the group and read back there: never a word of what she
        // knows of the candidate privately, whatever the judgment wrote.
        if myriad_merope::recognizing::gives_away(&why, &touched, said) {
            tracing::warn!(%venue, "[Merope] a guess about who someone is cited what she knows privately; not kept");
            return;
        }
        let evidence = json!({
            "who": stranger.who,
            "name": stranger.name,
            "userId": user_id,
            "sure": sure,
        })
        .to_string();
        let _ = unified::remember_in_venue(db, &group, &why, &evidence, SOURCE).await;
        tracing::info!(%venue, sure, "[Merope] she wonders who someone in a group is");
    }
}

/// Guesses about who an outsider is, untouched for long, fade.
pub async fn let_fade(db: &DatabaseConnection) {
    match unified::fade_source(db, SOURCE, FADE_AFTER).await {
        Ok(faded) if faded > 0 => tracing::info!(faded, "[Merope] guesses about outsiders faded"),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not let guesses about outsiders fade"),
    }
}
