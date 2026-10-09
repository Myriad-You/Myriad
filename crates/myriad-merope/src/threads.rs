//! What she has on her mind about someone, the rules of it: a thread, when it
//! is due, and how it is put before her. A thread is something of theirs to
//! come back to (an exam, a move), or something of hers toward them (a song
//! she wants them to hear, something she wants to ask them).

use chrono::{DateTime, TimeZone, Utc};
/// Open threads kept per person; the oldest go first.
pub const MAX_OPEN: usize = 8;

pub const MAX_ABOUT_CHARS: usize = 40;

pub const MAX_THEN_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq)]
pub struct Thread {
    pub id: String,
    /// What it is about, a few words.
    pub about: String,
    /// What she would come back with, in her words.
    pub then: String,
    /// When it would be natural to bring it up; none for "whenever".
    pub due: Option<DateTime<Utc>>,
    /// Hers toward them (something she wants to do with them, show or ask
    /// them), not something of theirs to come back to.
    pub hers: bool,
}

impl Thread {
    pub fn is_due(&self, now: DateTime<Utc>) -> bool {
        self.due.is_some_and(|due| due <= now)
    }
}

/// What is on her mind about them, for a conversation with them. Times
/// are told on the clock of `zone` (hers).
pub fn section<Z: TimeZone>(threads: &[Thread], now: DateTime<Utc>, zone: &Z) -> Option<String>
where
    Z::Offset: std::fmt::Display,
{
    listed(
        threads,
        now,
        zone,
        "## On your mind about them\nThings you meant to come back to with them.",
    )
}

/// What is on her mind in a group, for its talk: things someone there was
/// about to do or face, or left unfinished there.
pub fn group_section<Z: TimeZone>(
    threads: &[Thread],
    now: DateTime<Utc>,
    zone: &Z,
) -> Option<String>
where
    Z::Offset: std::fmt::Display,
{
    listed(
        threads,
        now,
        zone,
        "## On your mind in this group\nThings from this group you meant to come back to here.",
    )
}

fn listed<Z: TimeZone>(
    threads: &[Thread],
    now: DateTime<Utc>,
    zone: &Z,
    heading: &str,
) -> Option<String>
where
    Z::Offset: std::fmt::Display,
{
    if threads.is_empty() {
        return None;
    }
    let lines: Vec<String> = threads
        .iter()
        .map(|thread| {
            let when = match thread.due {
                Some(due) if due <= now => "now".to_string(),
                Some(due) => format!(
                    "later, around {}",
                    due.with_timezone(zone).format("%m-%d %H:%M")
                ),
                None => "whenever it fits".to_string(),
            };
            let whose = if thread.hers {
                "something you wanted to do with them; "
            } else {
                ""
            };
            format!("- {}: {} ({whose}{when})", thread.about, thread.then)
        })
        .collect();
    Some(format!(
        "{heading} Bring one up when it fits, as yourself; one at a time, and never as a list.\n{}",
        myriad_agent_rules::untrusted_block("on_your_mind", &lines.join("\n"))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_says_what_and_when() {
        let now: DateTime<Utc> = "2026-09-26T12:00:00Z".parse().unwrap();
        let due = Thread {
            id: "a".into(),
            about: "考试".into(),
            then: "问他考得怎么样".into(),
            due: Some(now - chrono::Duration::hours(1)),
            hers: false,
        };
        let later = Thread {
            due: Some(now + chrono::Duration::hours(20)),
            about: "搬家".into(),
            ..due.clone()
        };
        let whenever = Thread {
            due: None,
            about: "那首歌".into(),
            then: "想让他听听 Reol".into(),
            hers: true,
            ..due.clone()
        };
        assert!(due.is_due(now) && !later.is_due(now) && !whenever.is_due(now));
        let shanghai = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let section = section(&[due, later.clone(), whenever], now, &shanghai).unwrap();
        assert!(section.contains("考试: 问他考得怎么样 (now)"));
        // Due 2026-09-27 08:00 UTC, told on her clock.
        assert!(section.contains("搬家") && section.contains("later, around 09-27 16:00"));
        assert!(section.contains("(something you wanted to do with them; whenever it fits)"));
        assert!(section.contains("never as a list"));
        assert!(super::section(&[], now, &shanghai).is_none());
        let group = group_section(&[later], now, &Utc).unwrap();
        assert!(group.contains("later, around 09-27 08:00"));
        assert!(group.starts_with("## On your mind in this group"));
        assert!(group.contains("搬家") && group.contains("never as a list"));
    }
}
