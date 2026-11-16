//! The raw-chat arm (`MEROPE_MEMORY_BENCH_RAW_ARM=<n>`): each question also
//! answered the wide way with the raw private chat searched every time, by
//! meaning, whatever the cues said: the `n` messages nearest what was asked,
//! whole, beside what the look back found, from the same memories. Only a
//! bench arm: production scrolls back only when the words or cues reach
//! back.
//!
//! Unlike a turn, the latest talk is not left out: the bench shows none of
//! it in front of her.
//!
//! `MEROPE_MEMORY_BENCH_RAW_WHEN` searches it only when recall was weak
//! (see [`weak_enough`]).

use myriad_merope::remembering::Who;

tokio::task_local! {
    /// Inside the raw arm's answer: how many messages to bring.
    pub(super) static RAW: usize;
}

/// The arm asked for: how many raw messages each answer brings.
pub(super) fn raw_arm() -> Option<usize> {
    std::env::var("MEROPE_MEMORY_BENCH_RAW_ARM")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|found| *found > 0)
}

/// Inside the raw arm's answer, how many raw messages it brings; outside
/// it, none.
pub(super) fn raw_way() -> Option<usize> {
    RAW.try_with(|found| *found).ok()
}

/// `MEROPE_MEMORY_BENCH_RAW_WHEN`: when the raw arm searches the raw chat.
/// Unset, every time. `none`: only when recall found nothing by what they
/// said, every line it ranked only filling the room as recent context.
/// `filling`: also when it found too little to fill the room, so recent
/// context filled the rest. Both read what recall itself marks
/// ([`crate::services::agent::merope::store::Recalled::recent`]): no
/// threshold of the bench's own.
#[derive(Clone, Copy, Debug, PartialEq)]
enum When {
    Always,
    FoundNothing,
    Filling,
}

fn when() -> When {
    match std::env::var("MEROPE_MEMORY_BENCH_RAW_WHEN").as_deref() {
        Ok("none") => When::FoundNothing,
        Ok("filling") => When::Filling,
        Ok("") | Err(_) => When::Always,
        Ok(other) => panic!("MEROPE_MEMORY_BENCH_RAW_WHEN is none or filling, not {other}"),
    }
}

/// Whether the raw arm searches the raw chat this time, by how much
/// recall found; said in the log, for how often each way would.
pub(super) fn weak_enough(id: &str, recall: super::answering::Found) -> bool {
    let weak = weak_by(when(), recall);
    eprintln!(
        "raw gate {id}: found {} filling {} | nothing {} | filling {} -> {weak}",
        recall.found,
        recall.filling,
        weak_by(When::FoundNothing, recall),
        weak_by(When::Filling, recall),
    );
    weak
}

fn weak_by(when: When, recall: super::answering::Found) -> bool {
    match when {
        When::Always => true,
        When::FoundNothing => recall.found == 0,
        When::Filling => recall.found == 0 || recall.filling > 0,
    }
}

#[test]
fn the_raw_chat_is_searched_as_weak_recall_says() {
    use super::answering::Found;
    let nothing = Found {
        found: 0,
        filling: 8,
    };
    let some = Found {
        found: 3,
        filling: 5,
    };
    let full = Found {
        found: 8,
        filling: 0,
    };
    let empty = Found::default();
    let by = |when| [nothing, some, full, empty].map(|recall| weak_by(when, recall));
    assert_eq!(by(When::Always), [true, true, true, true]);
    assert_eq!(by(When::FoundNothing), [true, false, false, true]);
    assert_eq!(by(When::Filling), [true, true, false, true]);
}

/// `found` with the `wanted` private messages nearest `asked` by meaning
/// added, each once, oldest first, as a turn of the owner's has them
/// ([`crate::services::agent::merope::chat_near`]): every line is given its
/// vector first, as behind the replies of a talk that long. Nothing is
/// added when what was asked or the chat cannot be embedded.
pub(super) async fn with_raw(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    asked: &str,
    wanted: usize,
    found: Vec<(String, Who, String)>,
) -> Vec<(String, Who, String)> {
    use crate::services::agent::merope::chat_near;
    chat_near::rebuild(db, user_id).await;
    let near = chat_near::nearest(db, user_id, asked, wanted, false).await;
    chat_near::beside(found, near)
}

#[test]
fn a_raw_message_already_found_is_shown_once_and_whole() {
    let found = vec![
        (
            "2023-05-02".to_string(),
            Who::They,
            "which bakery did you say".to_string(),
        ),
        (
            "2023-05-01".to_string(),
            Who::You,
            "… the one on Elm / Street …".to_string(),
        ),
        (
            "2023-05-01".to_string(),
            Who::TheDay,
            "bakeries".to_string(),
        ),
    ];
    let more = vec![
        (
            "2023-05-01".to_string(),
            Who::You,
            "Try the one on Elm\nStreet, open till six".to_string(),
        ),
        (
            "2023-04-30".to_string(),
            Who::They,
            "I love sourdough".to_string(),
        ),
        (
            "2023-05-02".to_string(),
            Who::They,
            "which bakery did you say".to_string(),
        ),
    ];
    let all = crate::services::agent::merope::chat_near::beside(found, more);
    let texts: Vec<(&str, &str)> = all
        .iter()
        .map(|(day, _, text)| (day.as_str(), text.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            ("2023-04-30", "I love sourdough"),
            ("2023-05-01", "Try the one on Elm\nStreet, open till six"),
            ("2023-05-01", "bakeries"),
            ("2023-05-02", "which bakery did you say"),
        ]
    );
}

#[tokio::test]
async fn outside_the_raw_arm_nothing_raw_is_brought() {
    assert_eq!(raw_way(), None);
    assert_eq!(RAW.scope(5, async { raw_way() }).await, Some(5));
}
