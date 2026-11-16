//! Which of a child's taken moments reach her. A child may tell the same
//! thing over and over (another agent session still waiting, a minute
//! longer each time); each one that reaches her is a judgment of hers, a
//! model call. So the host hands her a moment only when it is news: not
//! the same as one from that device she was handed a little before, and
//! not past how many of that kind a device may hand her for a while.
//!
//! What is held back is still taken (`moment.taken` goes as for any
//! other): the child let it go, and she never hears of it. The first look
//! of a stretch at their screen is never held back for how many came: what
//! is held there is only a repeat.
//!
//! Kept in this process only: after a restart, the first of each comes
//! through once more.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::services::agent::merope::gates::SCREEN_EVENT;

use super::moment::Moment;

/// How long the same thing from a device is a repeat of what she was handed.
const SAME_WITHIN: Duration = Duration::from_secs(30 * 60);
/// How many moments of one kind a device may hand her in `PER`.
const AT_MOST: usize = 6;
const PER: Duration = Duration::from_secs(30 * 60);

/// Whether a taken moment reaches her.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Passing {
    Reaches,
    /// The same as one from that device she was handed within `SAME_WITHIN`.
    Repeat,
    /// That device already handed her `AT_MOST` of its kind within `PER`.
    TooMany,
}

/// What was handed to her lately, by device.
#[derive(Default)]
pub(super) struct Handed {
    /// (device, kind, what it says) → when it was handed.
    same: HashMap<(String, String, u64), Instant>,
    /// (device, kind) → when each was handed, oldest first.
    times: HashMap<(String, String), VecDeque<Instant>>,
    /// (device, stretch) → when its first look was handed.
    stretches: HashMap<(String, String), Instant>,
}

static HANDED: LazyLock<Mutex<Handed>> = LazyLock::new(|| Mutex::new(Handed::default()));

/// Whether `moment`, taken from `device`, reaches her now; when it does it
/// counts as handed.
pub(super) fn pass(device: &str, moment: &Moment) -> Passing {
    HANDED
        .lock()
        .map(|mut handed| handed.pass(device, moment, Instant::now()))
        .unwrap_or(Passing::Reaches)
}

/// Facts that name this one telling, not what it says: a look's own id,
/// and its stretch (a return to the same window is a new stretch).
fn names_the_telling(key: &str) -> bool {
    key == "id" || key.ends_with("_id") || key == "stretch"
}

/// A fact that counts how long or how many: its number changes from one
/// telling to the next while the thing stays the same.
fn counts(key: &str) -> bool {
    key.split('_')
        .any(|part| matches!(part, "seconds" | "minutes" | "hours" | "days" | "count"))
}

/// What a moment says, as one number: its headline, summary and facts,
/// spacing and case aside, with every run of digits in the headline, the
/// summary and a fact that counts taken as one (a session waiting 3 minutes
/// and the same one waiting 7 say the same). Which session, file or window
/// it is stays in the other facts as written.
pub(super) fn what_it_says(moment: &Moment) -> u64 {
    fn fold(text: &str, numbers: bool, out: &mut String) {
        let mut digits = false;
        let mut space = false;
        for c in text.chars() {
            if numbers && c.is_ascii_digit() {
                if !digits {
                    out.push('#');
                }
                digits = true;
                space = false;
                continue;
            }
            digits = false;
            if c.is_whitespace() {
                if !space {
                    out.push(' ');
                }
                space = true;
                continue;
            }
            space = false;
            out.extend(c.to_lowercase());
        }
        out.push('\u{1f}');
    }
    let mut text = String::new();
    fold(&moment.kind, false, &mut text);
    fold(&moment.headline, true, &mut text);
    fold(&moment.summary, true, &mut text);
    for (key, value) in &moment.safe_facts {
        if !names_the_telling(key) {
            fold(key, false, &mut text);
            fold(value, counts(key), &mut text);
        }
    }
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

impl Handed {
    pub(super) fn pass(&mut self, device: &str, moment: &Moment, now: Instant) -> Passing {
        self.forget_before(now);
        let kind = moment.kind.clone();
        let same = (device.to_string(), kind.clone(), what_it_says(moment));
        if self.same.contains_key(&same) {
            return Passing::Repeat;
        }
        // A look at their screen: the first of its stretch always reaches her.
        let stretch = (moment.kind == SCREEN_EVENT).then(|| {
            let stretch = moment
                .safe_facts
                .get("stretch")
                .filter(|stretch| !stretch.trim().is_empty());
            stretch.map(|stretch| (device.to_string(), stretch.clone()))
        });
        let first_look = match &stretch {
            Some(Some(stretch)) => !self.stretches.contains_key(stretch),
            // A look without a stretch is a stretch of its own.
            Some(None) => true,
            None => false,
        };
        let times = self.times.entry((device.to_string(), kind)).or_default();
        if !first_look && times.len() >= AT_MOST {
            return Passing::TooMany;
        }
        times.push_back(now);
        self.same.insert(same, now);
        if let Some(Some(stretch)) = stretch {
            self.stretches.entry(stretch).or_insert(now);
        }
        Passing::Reaches
    }

    fn forget_before(&mut self, now: Instant) {
        let recent = |at: &Instant, within: Duration| now.saturating_duration_since(*at) < within;
        self.same.retain(|_, at| recent(at, SAME_WITHIN));
        self.stretches.retain(|_, at| recent(at, SAME_WITHIN));
        self.times.retain(|_, times| {
            while times.front().is_some_and(|at| !recent(at, PER)) {
                times.pop_front();
            }
            !times.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn waiting(session: usize, minutes: usize) -> Moment {
        let facts: BTreeMap<String, String> = [
            ("happened", "waiting".to_string()),
            ("agent", format!("agent-{session}")),
            ("repo", "myriad".to_string()),
            ("turn", "ended".to_string()),
            ("waited_minutes", minutes.to_string()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        Moment {
            kind: "agent.merope.peer_agent".into(),
            occurred_at: 1,
            headline: format!("agent-{session} · myriad · waiting"),
            summary: format!(
                "agent-{session}, in their repository “myriad”, stopped {minutes} minutes ago: its turn ended and the next move is theirs."
            ),
            safe_facts: facts,
            urgency: Default::default(),
            keeps_memory: false,
        }
    }

    fn look(look_id: &str, stretch: &str, window: &str) -> Moment {
        Moment {
            kind: SCREEN_EVENT.into(),
            occurred_at: 1,
            headline: format!("Editor · {window}"),
            summary: format!("{window}: fn main() {{}}"),
            safe_facts: [
                ("app", "Editor"),
                ("window_title", window),
                ("look_id", look_id),
                ("stretch", stretch),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
            urgency: Default::default(),
            keeps_memory: false,
        }
    }

    /// The same five sessions told a hundred times over, a minute longer
    /// waiting each time: she is handed each session once.
    #[test]
    fn a_hundred_tellings_of_five_waiting_sessions_reach_her_five_times() {
        let mut handed = Handed::default();
        let start = Instant::now();
        let mut reached = 0;
        for batch in 0..100 {
            let now = start + Duration::from_secs(batch * 10);
            for session in 0..5 {
                if handed.pass("mac", &waiting(session, batch as usize), now) == Passing::Reaches {
                    reached += 1;
                }
            }
        }
        assert_eq!(reached, 5);
        // Another device's are its own.
        assert_eq!(
            handed.pass("other-mac", &waiting(0, 3), start),
            Passing::Reaches
        );
        // Once the repeat window is past, the same thing is news again.
        let later = start + SAME_WITHIN + Duration::from_secs(1000);
        assert_eq!(handed.pass("mac", &waiting(0, 50), later), Passing::Reaches);
    }

    /// Different things from one device: at most `AT_MOST` of a kind in `PER`.
    #[test]
    fn a_device_hands_her_at_most_a_few_of_a_kind_for_a_while() {
        let mut handed = Handed::default();
        let start = Instant::now();
        let passed: Vec<_> = (0..10)
            .map(|session| handed.pass("mac", &waiting(session, 1), start))
            .collect();
        assert_eq!(
            passed
                .iter()
                .filter(|pass| **pass == Passing::Reaches)
                .count(),
            AT_MOST
        );
        assert_eq!(passed[AT_MOST], Passing::TooMany);
        // Another kind from the same device is counted apart.
        let mut chat = waiting(0, 1);
        chat.kind = "agent.merope.ai_chat".into();
        assert_eq!(handed.pass("mac", &chat, start), Passing::Reaches);
        // And once `PER` is past, the kind may hand her more.
        assert_eq!(
            handed.pass("mac", &waiting(20, 1), start + PER + Duration::from_secs(1)),
            Passing::Reaches
        );
    }

    /// A look at their screen: the first of each stretch reaches her however
    /// many came; within a stretch, a look showing the same is a repeat, and
    /// one showing something else counts toward how many.
    #[test]
    fn the_first_look_of_a_stretch_is_never_held_for_how_many_came() {
        let mut handed = Handed::default();
        let now = Instant::now();
        for n in 0..20 {
            let first = look(
                &format!("look-{n}"),
                &format!("s{n}"),
                &format!("file{n}.rs"),
            );
            assert_eq!(handed.pass("mac", &first, now), Passing::Reaches, "{n}");
        }
        // The same window again, a look later in that stretch: a repeat.
        assert_eq!(
            handed.pass("mac", &look("look-99", "s3", "file3.rs"), now),
            Passing::Repeat
        );
        // Something else in an old stretch, past how many: held.
        assert_eq!(
            handed.pass("mac", &look("look-100", "s3", "other.rs"), now),
            Passing::TooMany
        );
    }

    #[test]
    fn what_it_says_leaves_out_numbers_ids_and_spacing() {
        let a = look("look-1", "s1", "main.rs");
        let b = look("look-2", "s2", "main.rs");
        assert_eq!(what_it_says(&a), what_it_says(&b));
        assert_ne!(
            what_it_says(&a),
            what_it_says(&look("look-1", "s1", "lib.rs"))
        );
        assert_eq!(what_it_says(&waiting(1, 3)), what_it_says(&waiting(1, 45)));
        assert_ne!(what_it_says(&waiting(1, 3)), what_it_says(&waiting(2, 3)));
    }
}
