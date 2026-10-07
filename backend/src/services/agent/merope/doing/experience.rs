//! What she did, as she recalls it and would tell it.

use super::*;

/// A thing she did and when, read back from her memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Experience {
    pub(super) key: String,
    pub(super) thing: Thing,
    /// How it landed with her.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) reaction: Option<Reaction>,
    /// She would want to tell someone about it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) tell: bool,
    /// What only its kind keeps (see `sources`).
    #[serde(flatten)]
    pub(super) kept: Kept,
}

/// The key of what a row of her own experience was, and the thing.
pub(in crate::services::agent::merope) fn key_of(
    row: &unified_row::Model,
) -> Option<(String, Thing)> {
    Experience::of(row).map(|experience| (experience.key, experience.thing))
}

/// What a row of her own experience was and how it landed, as a line
/// ("listening to … (you liked it)"): her views grow out of these.
pub(in crate::services::agent::merope) fn experience_line(
    row: &unified_row::Model,
) -> Option<String> {
    Experience::of(row).map(|experience| experience.line_felt())
}

/// What a row of her own experience was and how it landed with her, as
/// anyone may see it.
pub(in crate::services::agent::merope) fn thing_and_reaction(
    row: &unified_row::Model,
) -> Option<(Thing, Option<Reaction>)> {
    Experience::of(row).map(|experience| (experience.thing, experience.reaction))
}

/// A row of her own experience for looking back: the line with how it
/// landed and what she wrote, and whether it did not go well (only fine,
/// not for her, a guess that did not hold, a question left unanswered).
pub(in crate::services::agent::merope) fn experience_record(
    row: &unified_row::Model,
) -> Option<(String, bool)> {
    let experience = Experience::of(row)?;
    let (more, went_wrong) = experience.kept.looking_back();
    let missed = went_wrong
        || matches!(
            experience.reaction,
            Some(Reaction::Fine | Reaction::NotForMe)
        );
    Some((
        format!("{}: {}{more}", experience.line_felt(), row.content),
        missed,
    ))
}

/// A row of her own experience as the site's owner looks into it: what it
/// was, how it landed, and what its kind kept (her guess and how it went,
/// what she found out), each as itself rather than run into one line.
pub(in crate::services::agent::merope) fn experience_view(
    row: &unified_row::Model,
) -> Option<Value> {
    let experience = Experience::of(row)?;
    Some(json!({
        "thing": experience.thing,
        "reaction": experience.reaction,
        "kept": experience.kept,
    }))
}

impl Experience {
    pub(super) fn of(row: &unified_row::Model) -> Option<Self> {
        serde_json::from_str(row.evidence.as_deref()?).ok()
    }

    pub(super) fn line(&self) -> String {
        format!("{} {}", self.thing.verb(), self.thing.describe())
    }

    /// The line with how it landed, when she said.
    pub(super) fn line_felt(&self) -> String {
        match self.reaction {
            Some(reaction) => format!("{} ({})", self.line(), reaction.felt()),
            None => self.line(),
        }
    }

    /// "「晴天」 by 周杰伦 (you liked it): what she wrote".
    pub(super) fn noted(&self, content: &str) -> String {
        format!("- {}: {content}", self.line_felt())
    }
}

/// What she did lately, and older things their words touch, for a prompt:
/// (what it was and when, what stayed with her), most recent first.
pub async fn recalled(
    db: &DatabaseConnection,
    words: Option<&str>,
    recent: usize,
    related: usize,
) -> Vec<(String, String)> {
    let Ok(rows) = unified::own_experiences(db, 120).await else {
        return Vec::new();
    };
    let now = Utc::now();
    let mut picked: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| now.signed_duration_since(row.created_at) < chrono::Duration::hours(24))
        .map(|(index, _)| index)
        .take(recent)
        .collect();
    if let Some(words) = words.filter(|words| !words.trim().is_empty()) {
        let concepts: Vec<Vec<Concept>> = rows
            .iter()
            .map(|row| serde_json::from_value(row.concepts.clone()).unwrap_or_default())
            .collect();
        let texts: Vec<String> = rows
            .iter()
            .map(|row| {
                let what = Experience::of(row)
                    .map(|experience| experience.thing.describe())
                    .unwrap_or_default();
                format!("{what} {}", row.content)
            })
            .collect();
        let documents: Vec<crate::services::agent::memory::lexical::Document> = texts
            .iter()
            .zip(&concepts)
            .map(
                |(text, concepts)| crate::services::agent::memory::lexical::Document {
                    text,
                    concepts,
                },
            )
            .collect();
        let scores = crate::services::agent::memory::lexical::score_all(words, &documents);
        let mut touched: Vec<(usize, f64)> = scores
            .iter()
            .enumerate()
            .filter(|(index, score)| score.strong && !picked.contains(index))
            .map(|(index, score)| (index, score.value))
            .collect();
        touched.sort_by(|a, b| b.1.total_cmp(&a.1));
        picked.extend(touched.into_iter().take(related).map(|(index, _)| index));
    }
    // How often each came up among what is read here, so a song she keeps
    // going back to reads as one, and one heard once does not.
    let mut times: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for row in &rows {
        if let Some(experience) = Experience::of(row) {
            *times.entry(experience.key).or_default() += 1;
        }
    }
    picked
        .into_iter()
        .filter_map(|index| {
            let row = &rows[index];
            let experience = Experience::of(row)?;
            let again = match times.get(&experience.key).copied().unwrap_or(1) {
                0 | 1 => String::new(),
                times => format!(", {times} times lately"),
            };
            // What she heard in it is in what she wrote, as a listener says
            // it; the measurements it came from are not talk.
            Some((
                format!(
                    "{} ({}{again})",
                    experience.line_felt(),
                    ago(now, row.created_at.with_timezone(&Utc))
                ),
                row.content.clone(),
            ))
        })
        .collect()
}

/// What she did on her own between `start` and `end`, oldest first, each with
/// what stayed with her: for her diary.
pub async fn during(
    db: &DatabaseConnection,
    start: DateTime<chrono::FixedOffset>,
    end: DateTime<chrono::FixedOffset>,
    limit: usize,
) -> Vec<String> {
    let mut lines: Vec<String> = unified::own_experiences(db, 120)
        .await
        .unwrap_or_default()
        .iter()
        .filter(|row| row.created_at >= start && row.created_at < end)
        .filter_map(|row| {
            Some(format!(
                "{}: {}",
                Experience::of(row)?.line_felt(),
                row.content
            ))
        })
        .take(limit)
        .collect();
    lines.reverse();
    lines
}

pub(super) fn ago(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let minutes = now.signed_duration_since(at).num_minutes().max(0);
    match minutes {
        0..=9 => "just now".into(),
        10..=89 => format!("{minutes} minutes ago"),
        90..=1439 => format!("{} hours ago", minutes / 60),
        1440..=2879 => "yesterday".into(),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// For the player section of a private chat: whether they are already
/// listening with her, or how she can put her song on for them.
/// What she did on her own after `since` (within the last day) that she
/// would want to tell someone, most recent first: what it was, how it
/// landed and what she wrote.
pub async fn would_tell(db: &DatabaseConnection, since: Option<DateTime<Utc>>) -> Vec<String> {
    const WITHIN: chrono::Duration = chrono::Duration::hours(24);
    const AT_MOST: usize = 3;
    let now = Utc::now();
    unified::own_experiences(db, 60)
        .await
        .unwrap_or_default()
        .iter()
        .filter(|row| {
            let at = row.created_at.with_timezone(&Utc);
            now.signed_duration_since(at) < WITHIN && since.is_none_or(|since| at > since)
        })
        .filter_map(|row| Some((Experience::of(row)?, row)))
        .filter(|(experience, _)| experience.tell)
        .take(AT_MOST)
        .map(|(experience, row)| format!("{}: {}", experience.line_felt(), row.content))
        .collect()
}
