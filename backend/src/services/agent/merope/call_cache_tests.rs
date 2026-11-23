//! The memory writing steps cache the part of their system prompt that is
//! the same on every call: it must be the same, and long enough to cache.

use super::{REMEMBER_CACHE, SETTLE_CACHE};
use myriad_merope::chat_remember::{Examples, extract_system_prompt, settle_system_prompt};

/// What a provider caches at the least: 512 tokens on Haiku 5.5; a
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

#[test]
fn a_session_name_says_only_what_kind_of_call_it_is() {
    for cached in [REMEMBER_CACHE, SETTLE_CACHE, super::STANDING_CACHE] {
        assert!(cached.session.starts_with("memory-"));
        assert!(
            cached
                .session
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '-')
        );
    }
}
