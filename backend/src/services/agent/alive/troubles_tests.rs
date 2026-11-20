//! The troubles short of her stopping: one of her drivers stuck while the
//! others run, and her model not answering her.

use super::watch::{
    Held, QUIET_SECS, Seen, Trouble, Turn, UNANSWERED_IN_A_ROW, WELL_SECS, Watch, stalled,
};
use super::*;

const LOOK: i64 = LOOK_SECS as i64;

/// The first look at or after `at`.
fn next_look(at: i64) -> i64 {
    at + (LOOK - at % LOOK) % LOOK
}

fn seen(now: i64, lived_at: Option<i64>) -> Seen {
    Seen {
        now,
        started_at: 0,
        should_live: true,
        lived_at,
    }
}

#[test]
fn a_held_trouble_is_said_once_and_once_more_after_an_hour_gone() {
    let mut held = Held::default();
    assert_eq!(held.look(100, None), None);
    assert_eq!(held.look(200, Some(150)), Some(Turn::Began { from: 150 }));
    // Still holding, from another time or the same: said already.
    assert_eq!(held.look(300, Some(150)), None);
    assert_eq!(held.look(400, Some(390)), None);
    // Gone for less than an hour, back again: still the one said.
    assert_eq!(held.look(500, None), None);
    assert_eq!(held.look(500 + WELL_SECS - 1, Some(600)), None);
    assert_eq!(held.from, Some(150));
    // Gone for an hour: ended, once.
    let gone = 600 + WELL_SECS;
    assert_eq!(held.look(gone, None), None);
    assert_eq!(
        held.look(gone + WELL_SECS, None),
        Some(Turn::Ended {
            from: 150,
            since: gone
        })
    );
    assert_eq!(held, Held::default());
    assert_eq!(held.look(gone + WELL_SECS + 1, None), None);
}

#[test]
fn a_driver_is_stuck_after_three_of_its_rounds_and_never_under_ten_minutes() {
    // Every 5 s: ten minutes, not 15 s.
    assert_eq!(stalled([(5, Some(1000))], 0, 1000 + QUIET_SECS - 1), (0, None));
    assert_eq!(stalled([(5, Some(1000))], 0, 1000 + QUIET_SECS), (1, Some(1000)));
    // Every hour: three hours.
    assert_eq!(stalled([(3600, Some(1000))], 0, 1000 + 3 * 3600 - 1), (0, None));
    assert_eq!(stalled([(3600, Some(1000))], 0, 1000 + 3 * 3600), (1, Some(1000)));
    // Never let through: counted from when the watch could first see it.
    assert_eq!(stalled([(60, None)], 5000, 5000 + QUIET_SECS), (1, Some(5000)));
    // Let through before the watch could see: from then, not before.
    assert_eq!(stalled([(60, Some(10))], 5000, 5000 + QUIET_SECS - 1), (0, None));
    // Two stuck: both counted, from the earlier.
    assert_eq!(
        stalled(
            [(5, Some(2000)), (60, Some(1500)), (15, Some(9000))],
            0,
            9000
        ),
        (2, Some(1500))
    );
}

#[test]
fn a_stuck_driver_is_said_while_the_rest_of_her_life_runs() {
    let mut watch = Watch::default();
    let mut said = Vec::new();
    for now in (0..=3 * 3600).step_by(LOOK as usize) {
        assert_eq!(watch.look(seen(now, Some(now))), None);
        // Her memory jobs stuck from 1000, until 5000.
        let ran_at = if (1000..5000).contains(&now) { 1000 } else { now };
        let (_, from) = stalled([(5, Some(ran_at))], watch.watching_from(0), now);
        for trouble in watch.look_troubles(now, from, None) {
            said.push((now, trouble));
        }
    }
    let back = next_look(5000);
    assert_eq!(
        said,
        [
            (
                next_look(1000 + QUIET_SECS),
                Trouble::Stalled(Turn::Began { from: 1000 })
            ),
            (
                back + WELL_SECS,
                Trouble::Stalled(Turn::Ended {
                    from: 1000,
                    since: back
                })
            ),
        ]
    );
}

#[test]
fn while_nothing_of_hers_runs_a_stuck_driver_is_that_alarm_not_its_own() {
    let mut watch = Watch::default();
    let mut said = Vec::new();
    for now in (0..=900 + 3600).step_by(LOOK as usize) {
        // Everything stops at 900: the one alarm says so.
        watch.look(seen(now, Some(now.min(900))));
        let (_, from) = stalled([(5, Some(now.min(900)))], watch.watching_from(0), now);
        said.extend(watch.look_troubles(now, from, None));
    }
    assert_eq!(said, []);
    assert_eq!(watch.alarmed_from, Some(900));
}

#[test]
fn her_model_not_answering_is_said_after_ten_in_a_row_for_ten_minutes() {
    let mut answers = Answers {
        unanswered_in_a_row: UNANSWERED_IN_A_ROW - 1,
        unanswered_since: Some(1000),
        ..Answers::default()
    };
    // Nine in a row, an hour long: not yet.
    assert_eq!(unanswered_from(&answers, 1000 + 3600), None);
    // Ten in a row, within ten minutes: not yet.
    answers.unanswered_in_a_row = UNANSWERED_IN_A_ROW;
    assert_eq!(unanswered_from(&answers, 1000 + QUIET_SECS - 1), None);
    assert_eq!(unanswered_from(&answers, 1000 + QUIET_SECS), Some(1000));

    let mut watch = Watch::default();
    watch.look(seen(0, Some(0)));
    assert_eq!(
        watch.look_troubles(2000, None, Some(1000)),
        [Trouble::Unanswered(Turn::Began { from: 1000 })]
    );
}

#[test]
fn an_answer_that_cannot_be_read_is_an_answer() {
    with(|state| state.answers = Answers::default());
    for _ in 0..12 {
        model_unanswered();
    }
    let (in_a_row, since) =
        with(|state| (state.answers.unanswered_in_a_row, state.answers.unanswered_since));
    assert_eq!(in_a_row, 12);
    assert!(since.is_some());
    // What `merope::call` says of an answer it could not read.
    model_answered();
    let answers = with(|state| state.answers.clone());
    assert_eq!(
        (answers.unanswered_in_a_row, answers.unanswered_since),
        (0, None)
    );
    assert!(answers.answered_at.is_some());
}

#[test]
fn troubles_are_let_go_while_she_should_not_live_here() {
    let mut watch = Watch::default();
    watch.look(seen(0, Some(0)));
    assert_eq!(
        watch.look_troubles(1000, None, Some(100)),
        [Trouble::Unanswered(Turn::Began { from: 100 })]
    );
    let off = Seen {
        should_live: false,
        ..seen(1015, Some(1015))
    };
    assert_eq!(watch.look(off), None);
    assert_eq!(watch.look_troubles(1015, Some(10), Some(100)), []);
    assert_eq!(watch.unanswered, Held::default());
}

/// On a real database (an isolated schema; skipped without a test
/// database): each trouble is one notification of the owner's, apart from
/// the one alarm's, and its end changes that one.
#[tokio::test]
async fn each_trouble_and_its_end_are_one_notification() {
    let Some(fixture) = super::tests::fixture().await else {
        return;
    };
    let db = &fixture.db;
    let manager =
        crate::services::agent::notifications::NotificationManager::new_with_db(200, db.clone())
            .await;
    let rows = || async {
        db.query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, priority, metadata->>'status' AS status FROM agent_notifications \
             WHERE user_id = 1 ORDER BY id",
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| {
            (
                row.try_get::<String>("", "id").unwrap(),
                row.try_get::<String>("", "priority").unwrap(),
                row.try_get::<Option<String>>("", "status").unwrap(),
            )
        })
        .collect::<Vec<_>>()
    };
    let began = Turn::Began {
        from: 1_794_000_000,
    };
    let ended = Turn::Ended {
        from: 1_794_000_000,
        since: 1_794_030_000,
    };
    manager.notify_her_trouble(1, Trouble::Stalled(began), 2).await;
    manager.notify_her_trouble(1, Trouble::Unanswered(began), 2).await;
    let owned = |id: &str, priority: &str, status: &str| {
        (id.to_string(), priority.to_string(), Some(status.to_string()))
    };
    assert_eq!(
        rows().await,
        [
            owned("her_drivers_stalled_u1", "high", "her_drivers_stalled"),
            owned("her_model_unanswered_u1", "high", "her_model_unanswered"),
        ]
    );
    manager.notify_her_trouble(1, Trouble::Stalled(ended), 0).await;
    manager.notify_her_trouble(1, Trouble::Unanswered(ended), 0).await;
    assert_eq!(
        rows().await,
        [
            owned("her_drivers_stalled_u1", "normal", "her_drivers_back"),
            owned("her_model_unanswered_u1", "normal", "her_model_back"),
        ]
    );
    fixture.close().await;
}
