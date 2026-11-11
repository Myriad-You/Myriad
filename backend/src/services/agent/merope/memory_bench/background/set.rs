//! The background and the questions asked after it, checked against what
//! the run needs.

use super::*;

const BACKGROUND: &str =
    include_str!("../../../../../../../tests/merope/memory-long-background-zh.json");
const LONG: &str = include_str!("../../../../../../../tests/merope/memory-long-zh.json");
const HELD_OUT: &str =
    include_str!("../../../../../../../tests/merope/holdout/memory-long-zh.json");

/// The background and the questions asked after it read as the run needs:
/// the background in order, each session theirs first and taking turns;
/// each question resting on sessions the background has, its own sessions
/// after the background and before it is asked, graded by a rule of its
/// type (a detail asked plainly by the default one); both sides of a pair
/// on the same talk; the held-out questions none of the others.
#[test]
fn the_long_background_reads_as_the_bench_needs() {
    let background: Background = serde_json::from_str(BACKGROUND).unwrap();
    let dates: Vec<_> = background
        .sessions
        .iter()
        .map(|day| date_of(&day.date))
        .collect();
    assert!(background.sessions.len() >= 250);
    assert!(dates.windows(2).all(|pair| pair[0] < pair[1]));
    for (index, day) in background.sessions.iter().enumerate() {
        assert!(
            day.venue() == "private" || day.venue().starts_with("group:"),
            "{index}"
        );
        assert!((4..=8).contains(&day.turns.len()), "{index}");
        for (at, turn) in day.turns.iter().enumerate() {
            let theirs = at % 2 == 0;
            assert_eq!(turn.role == "user", theirs, "{index}");
        }
    }
    let end = *dates.last().unwrap();
    let read = |text: &str| -> Vec<Question> { serde_json::from_str(text).unwrap() };
    let (long, held_out) = (read(LONG), read(HELD_OUT));
    for questions in [&long, &held_out] {
        let mut ids = std::collections::BTreeSet::new();
        for question in questions.iter() {
            let id = question.question_id.as_str();
            assert!(ids.insert(id), "{id}");
            assert_eq!(rule_key(question), question.question_type, "{id}");
            let categories: &[&str] = match question.question_type.as_str() {
                "implicit-state" | "stale-premise" | "implicit-use" => &["type-1", "type-2"],
                "associated" => &super::super::chinese::ASSOCIATED,
                "single-session-user" => &["needle"],
                "feeling-cause" | "still-true" => &[],
                other => panic!("{id}: {other}"),
            };
            match &question.category {
                Some(category) => assert!(categories.contains(&category.as_str()), "{id}"),
                None => assert!(categories.is_empty(), "{id}"),
            }
            assert!(
                question.question_type == "single-session-user"
                    || prompts()["grade"]["rules"][rule_key(question)].is_string(),
                "{id}"
            );
            assert!(!question.background_sessions.is_empty(), "{id}");
            assert!(
                question
                    .background_sessions
                    .iter()
                    .all(|index| *index < background.sessions.len()),
                "{id}"
            );
            assert_eq!(
                question.haystack_dates.len(),
                question.haystack_sessions.len(),
                "{id}"
            );
            let asked = date_of(&question.question_date);
            assert!(asked > end, "{id}");
            for date in &question.haystack_dates {
                let at = date_of(date);
                assert!(at > end && at < asked, "{id}");
            }
            assert_eq!(
                question.pair.is_some(),
                question.category.as_deref() == Some("paired"),
                "{id}"
            );
            if let Some(pair) = &question.pair {
                let sides: Vec<&Question> = questions
                    .iter()
                    .filter(|other| other.pair.as_ref() == Some(pair))
                    .collect();
                assert_eq!(sides.len(), 2, "{id}");
                assert_eq!(
                    sides[0].background_sessions, sides[1].background_sessions,
                    "{id}"
                );
            }
            // At the small size it is its own sessions after those it rests on.
            let small = question.small(&background);
            assert_eq!(
                small.haystack_sessions.len(),
                question.background_sessions.len() + question.haystack_sessions.len(),
                "{id}"
            );
        }
    }
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for question in &long {
        // What it needs is named, but for a question where nothing should
        // come up; each fact by words a line of it holds. (The held-out
        // questions are left as they are.)
        let id = &question.question_id;
        assert_eq!(question.needs.is_empty(), id.ends_with("-hold"), "{id}");
        assert!(
            question.needs.iter().all(|words| {
                !words.is_empty()
                    && words
                        .iter()
                        .all(|word| word.split('&').all(|part| !part.is_empty()))
            }),
            "{id}"
        );
        if question.question_id.ends_with("-hold") {
            continue;
        }
        let kind = match &question.category {
            Some(category) => format!("{}/{category}", question.question_type),
            None => question.question_type.clone(),
        };
        *kinds.entry(kind).or_default() += 1;
    }
    let mut expected: BTreeMap<String, usize> = [
        ("feeling-cause".to_string(), 6),
        ("still-true".to_string(), 20),
        ("single-session-user/needle".to_string(), 10),
    ]
    .into_iter()
    .collect();
    for kind in ["implicit-state", "stale-premise", "implicit-use"] {
        for category in ["type-1", "type-2"] {
            expected.insert(format!("{kind}/{category}"), 10);
        }
    }
    for category in super::super::chinese::ASSOCIATED {
        expected.insert(format!("associated/{category}"), 3);
    }
    assert_eq!(kinds, expected);
    for question in &held_out {
        assert!(
            !long
                .iter()
                .any(|other| other.question_id == question.question_id
                    || other.question == question.question),
            "{}",
            question.question_id
        );
    }
}
