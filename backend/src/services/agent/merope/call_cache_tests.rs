//! The memory writing steps cache the part of their system prompt that is
//! the same on every call: it must be the same, and long enough to cache.

use super::{REMEMBER_CACHE, SETTLE_CACHE};
use myriad_merope::chat_remember::{Examples, extract_system_prompt, settle_system_prompt};

/// What a provider caches at the least: 512 tokens on Haiku 5.5 and Sonnet 5.5; a
/// token is about four characters of these English instructions.
const CACHEABLE_CHARS: usize = 512 * 4;

fn same_part<'a>(prompt: &'a str, until: &str) -> &'a str {
    let at = prompt
        .find(until)
        .expect("the cache's end is in the prompt");
    &prompt[..at]
}

#[test]
fn what_she_knows_comes_after_the_cached_part_of_writing_and_going_over() {
    let few = ["likes tea".to_string()];
    let more = [
        "keeps two cats".to_string(),
        "Known:\nnot a heading".to_string(),
        "Known for good:\nnot either".to_string(),
    ];
    for examples in [Examples::Chinese, Examples::English] {
        for (prompt, until) in [
            (
                extract_system_prompt as fn(&[String], Examples) -> String,
                REMEMBER_CACHE.until.unwrap(),
            ),
            (settle_system_prompt, SETTLE_CACHE.until.unwrap()),
        ] {
            let (a, b, none) = (
                prompt(&few, examples),
                prompt(&more, examples),
                prompt(&[], examples),
            );
            let same = same_part(&a, until);
            assert_eq!(same, same_part(&b, until), "{examples:?} {until:?}");
            assert_eq!(same, same_part(&none, until), "{examples:?} {until:?}");
            assert!(
                same.len() >= CACHEABLE_CHARS,
                "{until:?}: {} chars",
                same.len()
            );
            assert!(!same.contains("likes tea") && !same.contains("keeps two cats"));
        }
    }
}

/// How things stand is asked with a schema the provider puts before the
/// prompt, and only with it is there enough to cache: 764 tokens for a step
/// and 745 for a first building, as its ledger said of these prompts
/// (2026-11-23). Asked without one (an endpoint that takes no schema), there
/// is not, and the mark does nothing.
#[test]
fn how_things_stand_is_as_long_as_when_its_cache_was_measured() {
    use myriad_nodes::stands::{BUILD_SYSTEM, STEP_SYSTEM, build_schema, step_schema};
    assert_eq!(super::STANDING_CACHE.until, None);
    for (system, schema, measured) in [
        (STEP_SYSTEM, step_schema(), 1840 + 519),
        (BUILD_SYSTEM, build_schema(), 1483 + 320),
    ] {
        let sent = system.chars().count() + schema.to_string().len();
        assert!(sent >= measured, "{sent} chars: measure it again");
    }
}

/// Going over a note says the same to her every time, long enough to cache
/// by itself. Its schema is the same every time too: a provider held to it
/// may put it before the prompt, where it begins what is cached.
#[test]
fn going_over_a_note_is_asked_the_same_way_every_time_and_long_enough() {
    use crate::services::agent::merope::memory_nodes::upkeep::{schema, system_prompt};
    assert_eq!(super::UPKEEP_CACHE.until, None);
    let (first, again) = (system_prompt(), system_prompt());
    assert_eq!(first, again);
    assert!(
        first.chars().count() >= CACHEABLE_CHARS,
        "{} chars",
        first.chars().count()
    );
    assert_eq!(schema().to_string(), schema().to_string());
}

/// Her look says the same to her every time, whatever she is shown: the
/// whole system prompt is who she is and how she answers, and the schema is
/// the same too. What she is shown is in the prompt, after both.
#[test]
fn a_look_is_asked_the_same_way_whatever_is_shown() {
    use myriad_nodes::session_look::{LookInput, Recalled, Said, Shown, Told, look_input};
    use serde_json::json;
    assert_eq!(super::LOOK_CACHE.until, None);
    let shown = |change: &str, words: &str, memories: &[&str]| Shown {
        told: Told {
            device: "dev_mac".into(),
            session: myriad_her_link::now::read_session(&json!({
                "grant": "g_0123456789", "session": "s-a", "change": change,
                "occurredAt": 1_763_900_000_000_i64, "tool": "claude-code",
                "remote": "github.com/a/b", "fromAgent": words,
            }))
            .unwrap(),
        },
        memories: memories
            .iter()
            .map(|text| Recalled {
                id: "m1".into(),
                text: text.to_string(),
            })
            .collect(),
        before: None,
    };
    let soul = "You are Arael.\n你喜欢安静。".to_string();
    let asked = |changes: Vec<Shown>, said: Vec<Said>, at: i64| {
        look_input(LookInput {
            changes,
            others: Vec::new(),
            said,
            at,
            soul: soul.clone(),
        })
        .unwrap()
    };
    let one = asked(
        vec![shown("finished", "all tests passed", &[])],
        Vec::new(),
        1_763_900_100_000,
    );
    let two = asked(
        vec![
            shown("asked", "push it?", &["pushes are the owner's"]),
            shown("failed", "three tests failed", &[]),
        ],
        vec![Said {
            id: "p1".into(),
            at: 1_763_900_200_000,
            text: "the build is red".into(),
        }],
        1_763_986_500_000,
    );
    assert_ne!(one.prompt, two.prompt);
    assert_eq!(one.system, two.system);
    assert_eq!(one.schema.to_string(), two.schema.to_string());
    // Asked again, the system prompt is still the one sent first.
    assert!(two.again.starts_with(&two.prompt));
    assert!(one.system.starts_with(&soul));
    for shown in ["all tests passed", "push it?", "the build is red"] {
        assert!(!two.system.contains(shown), "{shown}");
    }
    // Long enough with no one in it at all.
    let instructions = one.system.len() - soul.len();
    assert!(instructions >= CACHEABLE_CHARS, "{instructions} chars");
}

#[test]
fn a_session_name_says_only_what_kind_of_call_it_is() {
    for cached in [
        REMEMBER_CACHE,
        SETTLE_CACHE,
        super::STANDING_CACHE,
        super::UPKEEP_CACHE,
        super::LOOK_CACHE,
    ] {
        assert!(cached.session.starts_with("memory-") || cached.session.starts_with("sessions-"));
        assert!(
            cached
                .session
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '-')
        );
    }
}
