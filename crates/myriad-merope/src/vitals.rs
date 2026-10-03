//! Her vital signs, the rules of it: a day of her life in numbers, counted in
//! code without asking any model, and what in them is worth raising.
//!
//! Three days of her own time went unreadable before anyone noticed, and her
//! taste narrowed to one axis over a week before anyone looked. What she is
//! for is presence over weeks, so how she is changing has to be seen as it
//! happens: how much she did and how it landed, how much she said and how
//! fast, what she keeps saying, what she cost, and anything that stopped.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A day of her, counted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    pub day: String,
    /// Model calls billed to her, failed ones, and input tokens.
    pub calls: u64,
    pub failed_calls: u64,
    pub input_tokens: u64,
    /// The operations that took the most calls: (operation, calls).
    pub busiest: Vec<(String, u64)>,
    /// Rows kept, by source.
    pub kept: BTreeMap<String, u64>,
    /// Her own records kept that day that cannot be read back.
    pub unreadable: u64,
    /// Things done on her own, minutes in them, and minutes lazing.
    pub things: u64,
    pub own_minutes: f64,
    pub lazed_minutes: f64,
    /// How what she did landed: reaction → count.
    pub landed: BTreeMap<String, u64>,
    /// Replies she gave, and seconds from their message to her reply.
    pub replies: u64,
    pub reply_p50: Option<f64>,
    pub reply_p90: Option<f64>,
    /// Phrases she keeps using in her notes and in her replies, with the
    /// share of them each is in.
    pub notes_lean_on: Vec<(String, f64)>,
    pub replies_lean_on: Vec<(String, f64)>,
    /// The share of her replies lately that ended by asking them something,
    /// when there were enough of them to say.
    #[serde(default)]
    pub replies_asking: Option<f64>,
    /// What she opens with when someone comes back after hours apart,
    /// phrases she keeps opening with lately.
    #[serde(default)]
    pub openers_lean_on: Vec<(String, f64)>,
    /// Messages she wrote first that day, and how many of them the person
    /// answered within `ANSWERED_WITHIN_HOURS`.
    #[serde(default)]
    pub proactive: u64,
    #[serde(default)]
    pub proactive_answered: u64,
    /// New things she learned that day: facts about someone, things looked
    /// up, questions she went into, things heard or put to her. None on days
    /// counted before this was.
    #[serde(default)]
    pub learned: Option<u64>,
    /// Each group that talked that day: how much was said there and how
    /// much of it was hers. Empty on days counted before this was.
    #[serde(default)]
    pub groups: Vec<GroupTalk>,
    /// What is worth raising.
    pub alerts: Vec<Alert>,
}

/// One group's talk on a day, as counts only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupTalk {
    pub venue: String,
    /// Lines said there, hers among them, and how many others spoke.
    pub lines: u64,
    pub hers: u64,
    pub others: u64,
}

/// Something in a day worth raising.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Alert {
    /// Her own records kept that day that cannot be read back.
    Unreadable {
        count: u64,
    },
    FailedCalls {
        failed: u64,
        calls: u64,
    },
    /// A kind of record kept every day before and not that day.
    Stopped {
        source: String,
    },
    NotesLeanOn {
        phrase: String,
        percent: u64,
    },
    RepliesLeanOn {
        phrase: String,
        percent: u64,
    },
    /// Most of her replies ended by asking them something.
    RepliesAsking {
        percent: u64,
    },
    /// She keeps opening with the same words when someone comes back.
    OpenersLeanOn {
        phrase: String,
        percent: u64,
    },
    /// What she wrote first this past week mostly went unanswered.
    ProactiveUnanswered {
        sent: u64,
        answered: u64,
    },
    SlowReplies {
        seconds: u64,
    },
    /// Days on end she learned nothing new: her wanting to know has had
    /// nothing to go on, or nowhere to go.
    NothingLearned {
        days: u64,
    },
    ManyCalls {
        calls: u64,
        usual: u64,
    },
}

/// Runs of letters in a text (Han, kana, Latin), where phrases live.
fn runs(text: &str) -> Vec<Vec<char>> {
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for ch in text.chars() {
        let letter = ch.is_alphabetic();
        if letter {
            run.push(ch);
        } else if !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

/// Phrases (3 to 6 letters) in at least `share` of `texts` and in at least
/// three of them, longest meaning first, none inside another: what she
/// leans on. With the share of texts each is in.
pub fn leaned_on<S: AsRef<str>>(texts: &[S], share: f64, most: usize) -> Vec<(String, f64)> {
    if texts.len() < 3 {
        return Vec::new();
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for text in texts {
        let mut seen = HashSet::new();
        for run in runs(text.as_ref()) {
            for width in 3..=6 {
                for gram in run.windows(width) {
                    let gram: String = gram.iter().collect();
                    if seen.insert(gram.clone()) {
                        *counts.entry(gram).or_default() += 1;
                    }
                }
            }
        }
    }
    let need = ((share * texts.len() as f64).ceil() as usize).max(3);
    let mut found: Vec<(String, usize)> = counts
        .into_iter()
        .filter(|(_, count)| *count >= need)
        .collect();
    found.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(b.0.chars().count().cmp(&a.0.chars().count()))
            .then(a.0.cmp(&b.0))
    });
    let mut kept: Vec<(String, usize)> = Vec::new();
    for (gram, count) in found {
        if kept
            .iter()
            .any(|(other, _)| other.contains(&gram) || gram.contains(other.as_str()))
        {
            continue;
        }
        kept.push((gram, count));
        if kept.len() == most {
            break;
        }
    }
    kept.into_iter()
        .map(|(gram, count)| (gram, count as f64 / texts.len() as f64))
        .collect()
}

/// The value at `quantile` (0..1) of `values`.
pub fn quantile(values: &[f64], quantile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = ((sorted.len() - 1) as f64 * quantile.clamp(0.0, 1.0)).round() as usize;
    Some(sorted[at])
}

/// A share of her notes or replies a phrase is in before it is worth raising.
pub const LEANS_TOO_MUCH: f64 = 0.3;

/// A share of her replies ending by asking before it is worth raising.
pub const ASKS_TOO_MUCH: f64 = 0.6;

/// A message she wrote first counts as answered if they said anything to
/// her within this long: the same as she herself goes by when she thinks of
/// writing to them again (see `others`).
pub const ANSWERED_WITHIN_HOURS: i64 = crate::others::ANSWERED_WITHIN_HOURS;
/// Over a week, this many first messages and fewer than a fifth answered is
/// worth raising: she is writing into silence.
const PROACTIVE_AT_LEAST: u64 = 5;

/// Whether a reply ends by asking them something: its last line of words
/// (not a stage direction) ends with a question mark.
pub fn ends_asking(reply: &str) -> bool {
    reply
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty() && !line.starts_with("[["))
        .is_some_and(|last| last.ends_with(['?', '？']))
}

/// What in `today` is worth raising, against the days before it (oldest
/// first): things that stopped, went wrong, or went odd.
pub fn alerts(today: &Day, before: &[Day]) -> Vec<Alert> {
    let mut alerts = Vec::new();
    if today.unreadable > 0 {
        alerts.push(Alert::Unreadable {
            count: today.unreadable,
        });
    }
    if today.calls > 0 && today.failed_calls * 10 > today.calls {
        alerts.push(Alert::FailedCalls {
            failed: today.failed_calls,
            calls: today.calls,
        });
    }
    // A kind of record she kept every one of the days before, and not today.
    if before.len() >= 3 {
        let recent = &before[before.len() - 3..];
        let mut sources: Vec<&String> = recent[0].kept.keys().collect();
        sources.sort();
        for source in sources {
            if recent
                .iter()
                .all(|day| day.kept.get(source).copied().unwrap_or(0) > 0)
                && today.kept.get(source).copied().unwrap_or(0) == 0
            {
                alerts.push(Alert::Stopped {
                    source: source.clone(),
                });
            }
        }
    }
    let percent = |share: f64| (share * 100.0).round() as u64;
    for (phrase, share) in &today.notes_lean_on {
        if *share >= LEANS_TOO_MUCH {
            alerts.push(Alert::NotesLeanOn {
                phrase: phrase.clone(),
                percent: percent(*share),
            });
        }
    }
    for (phrase, share) in &today.replies_lean_on {
        if *share >= LEANS_TOO_MUCH {
            alerts.push(Alert::RepliesLeanOn {
                phrase: phrase.clone(),
                percent: percent(*share),
            });
        }
    }
    for (phrase, share) in &today.openers_lean_on {
        if *share >= LEANS_TOO_MUCH {
            alerts.push(Alert::OpenersLeanOn {
                phrase: phrase.clone(),
                percent: percent(*share),
            });
        }
    }
    // The past week, today with the six days before it.
    let week = before.iter().rev().take(6).chain(std::iter::once(today));
    let (sent, answered) = week.fold((0, 0), |(sent, answered), day| {
        (sent + day.proactive, answered + day.proactive_answered)
    });
    if sent >= PROACTIVE_AT_LEAST && answered * 5 < sent {
        alerts.push(Alert::ProactiveUnanswered { sent, answered });
    }
    if let Some(share) = today.replies_asking.filter(|share| *share >= ASKS_TOO_MUCH) {
        alerts.push(Alert::RepliesAsking {
            percent: percent(share),
        });
    }
    if let Some(seconds) = today.reply_p90.filter(|seconds| *seconds > 30.0) {
        alerts.push(Alert::SlowReplies {
            seconds: seconds.round() as u64,
        });
    }
    // Nothing new learned today and the day before (both counted).
    let unlearned = std::iter::once(today)
        .chain(before.iter().rev())
        .take_while(|day| day.learned == Some(0))
        .count() as u64;
    if unlearned >= NOTHING_LEARNED_DAYS {
        alerts.push(Alert::NothingLearned { days: unlearned });
    }
    let usual_calls = quantile(
        &before
            .iter()
            .map(|day| day.calls as f64)
            .collect::<Vec<_>>(),
        0.5,
    );
    if let Some(usual) = usual_calls.filter(|usual| *usual >= 20.0)
        && today.calls as f64 > usual * 2.0
    {
        alerts.push(Alert::ManyCalls {
            calls: today.calls,
            usual: usual.round() as u64,
        });
    }
    alerts
}

/// Days in a row of learning nothing new before it is raised.
pub const NOTHING_LEARNED_DAYS: u64 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_of_learning_nothing_new_are_raised() {
        let learned = |learned: Option<u64>| Day {
            learned,
            ..Day::default()
        };
        let quiet = alerts(&learned(Some(0)), &[learned(Some(3)), learned(Some(0))]);
        assert!(quiet.contains(&Alert::NothingLearned { days: 2 }));
        // One quiet day, or days counted before this was: nothing raised.
        assert!(alerts(&learned(Some(0)), &[learned(Some(4))]).is_empty());
        assert!(alerts(&learned(Some(0)), &[learned(None)]).is_empty());
    }

    #[test]
    fn what_she_leans_on_is_found_and_only_that() {
        let notes = [
            "跑得挺快，不是这个，笼子继续空着。",
            "冲得挺急，不是这个。",
            "这首真好听，想再听一遍。",
            "不是这个，提着笼子接着走。",
            "有点酸酸的，像发了会儿呆。",
        ];
        let leaned = leaned_on(&notes, LEANS_TOO_MUCH, 5);
        assert_eq!(leaned[0].0, "不是这个");
        assert!((leaned[0].1 - 0.6).abs() < 1e-9);
        // Too few to say.
        assert!(leaned_on(&notes[..2], 0.3, 5).is_empty());
        assert_eq!(quantile(&[3.0, 1.0, 2.0], 0.5), Some(2.0));
        assert_eq!(quantile(&[], 0.5), None);
        assert!(ends_asking("还好吧\n你呢？"));
        assert!(ends_asking("你呢?\n[[motion:nod]]"));
        assert!(!ends_asking("是吗？我觉得还行。"));
    }

    #[test]
    fn what_stopped_or_went_odd_is_raised() {
        let day = |calls: u64, doing: u64| Day {
            calls,
            kept: [("doing".to_string(), doing)].into_iter().collect(),
            ..Day::default()
        };
        let before = vec![
            day(200, 50),
            day(220, 60),
            Day {
                proactive: 2,
                ..day(210, 55)
            },
        ];
        let today = Day {
            unreadable: 3,
            failed_calls: 80,
            reply_p90: Some(42.0),
            replies_asking: Some(0.72),
            openers_lean_on: vec![("怎么这个点".into(), 0.6)],
            proactive: 4,
            proactive_answered: 0,
            notes_lean_on: vec![("不是这个".into(), 0.36)],
            ..day(500, 0)
        };
        let raised = alerts(&today, &before);
        assert!(raised.contains(&Alert::Unreadable { count: 3 }));
        assert!(raised.contains(&Alert::FailedCalls {
            failed: 80,
            calls: 500
        }));
        assert!(raised.contains(&Alert::Stopped {
            source: "doing".into()
        }));
        assert!(raised.contains(&Alert::NotesLeanOn {
            phrase: "不是这个".into(),
            percent: 36
        }));
        assert!(raised.contains(&Alert::RepliesAsking { percent: 72 }));
        assert!(raised.contains(&Alert::OpenersLeanOn {
            phrase: "怎么这个点".into(),
            percent: 60
        }));
        // Four unanswered today and two the day before: six into silence.
        assert!(raised.contains(&Alert::ProactiveUnanswered {
            sent: 6,
            answered: 0
        }));
        assert!(raised.contains(&Alert::SlowReplies { seconds: 42 }));
        assert!(raised.contains(&Alert::ManyCalls {
            calls: 500,
            usual: 210
        }));
        assert_eq!(
            serde_json::to_value(&raised[0]).unwrap(),
            serde_json::json!({ "kind": "unreadable", "count": 3 })
        );
        // An ordinary day raises nothing.
        assert!(alerts(&day(205, 52), &before).is_empty());
    }
}
