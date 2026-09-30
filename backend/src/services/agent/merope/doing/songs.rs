//! Songs she found and would share, and which she offered in a conversation.

use super::*;

/// Songs she could play for someone: ones she listened to on her own
/// lately and liked, most recent first, each with what she wrote then.
/// What she says about a song she plays comes from here, not from nowhere.
pub async fn songs_to_share(db: &DatabaseConnection) -> Vec<(Thing, String)> {
    const WITHIN: chrono::Duration = chrono::Duration::days(14);
    const AT_MOST: usize = 8;
    let now = Utc::now();
    let rows = unified::own_experiences(db, 300).await.unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    rows.iter()
        .filter(|row| now.signed_duration_since(row.created_at) < WITHIN)
        .filter_map(|row| Some((Experience::of(row)?, row)))
        .filter(|(experience, _)| {
            matches!(experience.thing, Thing::Song { .. })
                && matches!(experience.reaction, Some(Reaction::Liked | Reaction::Moved))
        })
        .filter(|(experience, _)| seen.insert(experience.key.clone()))
        .take(AT_MOST)
        .map(|(experience, row)| {
            let line = format!(
                "{} {}, {}: {}",
                experience.thing.describe(),
                experience
                    .reaction
                    .map(|reaction| format!("({})", reaction.felt()))
                    .unwrap_or_default(),
                ago_text(now.signed_duration_since(row.created_at.with_timezone(&Utc))),
                gist(&row.content)
            );
            (experience.thing, line)
        })
        .collect()
}

/// The start of what she wrote, enough to know which song it was to her:
/// the whole of it is in her own time when that comes to mind.
pub(super) fn gist(note: &str) -> String {
    const GIST_CHARS: usize = 40;
    let mut gist: String = note.chars().take(GIST_CHARS).collect();
    if note.chars().count() > GIST_CHARS {
        gist.push('…');
    }
    gist
}

/// The songs last offered in each conversation, so the number she picks
/// is the song she saw, even if she liked another one since.
pub(super) static OFFERED: LazyLock<Mutex<HashMap<(i32, String), Vec<Thing>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn offer_songs(user_id: i32, session_id: &str, songs: Vec<Thing>) {
    if let Ok(mut offered) = OFFERED.lock() {
        if offered.len() > 1024 {
            offered.clear();
        }
        offered.insert((user_id, session_id.to_string()), songs);
    }
}

/// The song numbered `number` (from 1) in what was offered in this
/// conversation.
pub fn offered_song(user_id: i32, session_id: &str, number: u8) -> Option<Thing> {
    let offered = OFFERED.lock().ok()?;
    offered
        .get(&(user_id, session_id.to_string()))?
        .get(usize::from(number).checked_sub(1)?)
        .cloned()
}
