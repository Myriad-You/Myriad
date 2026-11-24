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
    assert_eq!(stalled([(5, 1000)], 0, 1000 + QUIET_SECS - 1), (0, None));
    assert_eq!(stalled([(5, 1000)], 0, 1000 + QUIET_SECS), (1, Some(1000)));
    // Every hour: three hours.
    assert_eq!(stalled([(3600, 1000)], 0, 1000 + 3 * 3600 - 1), (0, None));
    assert_eq!(stalled([(3600, 1000)], 0, 1000 + 3 * 3600), (1, Some(1000)));
    // Asked before the watch could see: from when it could, not before.
    assert_eq!(stalled([(60, 10)], 5000, 5000 + QUIET_SECS - 1), (0, None));
    assert_eq!(
        stalled([(60, 10)], 5000, 5000 + QUIET_SECS),
        (1, Some(5000))
    );
    // Two stuck: both counted, from the earlier.
    assert_eq!(
        stalled([(5, 2000), (60, 1500), (15, 9000)], 0, 9000),
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
        let (_, from) = stalled([(5, ran_at)], watch.watching_from(0), now);
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
        let (_, from) = stalled([(5, now.min(900))], watch.watching_from(0), now);
        said.extend(watch.look_troubles(now, from, None));
    }
    assert_eq!(said, []);
    assert_eq!(watch.alarmed_from, Some(900));
}

/// The drivers that run wherever she is (`persona::start`), as a running
/// site has them: they tick on a clock and never ask whether she is here.
const NOT_HERS: [(&str, u64); 5] = [
    ("reminders", 30),
    ("TAPP interactions", 5),
    ("whether she is living", LOOK_SECS),
    ("heartbeat", 60),
    ("memory and skill pruning", 86_400),
];

/// A process that came up at `started`, with the drivers that are not hers
/// and two of hers let through at `started`.
fn came_up(started: i64) -> State {
    let mut state = State {
        started_at: Some(started),
        ..State::default()
    };
    for (name, every) in NOT_HERS {
        state.drivers.entry(name.into()).or_default().every = Some(every);
    }
    for (name, every) in [("memory jobs", 5), ("her intents", 30)] {
        let beat = state.drivers.entry(name.into()).or_default();
        beat.every = Some(every);
        beat.ran_at = Some(started);
    }
    state
}

/// What the owner is told, look by look from `from` to `to`: her drivers
/// let through at each, but `stuck`, not since its own time.
fn told(
    state: &mut State,
    from: i64,
    to: i64,
    stuck: Option<(&str, i64)>,
) -> Vec<(i64, Trouble)> {
    let mut told = Vec::new();
    for now in (from..=to).step_by(LOOK as usize) {
        for name in ["memory jobs", "her intents"] {
            let since = stuck.filter(|(it, _)| *it == name).map(|(_, since)| since);
            state.drivers.get_mut(name).unwrap().ran_at = Some(since.unwrap_or(now).min(now));
        }
        let looked = look_at(state, now, true);
        assert_eq!(looked.said, None);
        told.extend(looked.troubles.into_iter().map(|trouble| (now, trouble)));
    }
    told
}

/// What the site had written down at 1795240313 (2026-11-21): a trouble
/// said of drivers that are not hers, standing ever since.
fn kept_by_a_process_gone() -> State {
    serde_json::from_value(json!({
        "drivers": { "heartbeat": { "ranAt": null, "runs": 0 } },
        "watch": { "stalled": { "from": 1_795_240_313, "wellFrom": null } },
    }))
    .unwrap()
}

#[test]
fn a_driver_that_runs_wherever_she_is_is_not_hers_to_be_stuck() {
    let mut state = came_up(0);
    let days = 4 * 86_400;
    for name in ["memory jobs", "her intents"] {
        state.drivers.get_mut(name).unwrap().ran_at = Some(days);
    }
    assert_eq!(stuck(&state, 0, days), (0, None));
    assert_eq!(told(&mut state, 0, days, None), []);
}

#[test]
fn one_of_hers_let_through_or_passed_over_long_ago_is_stuck() {
    let mut state = came_up(0);
    // Its last tick let through and never ended: a tick that hangs.
    state.drivers.get_mut("memory jobs").unwrap().ran_at = Some(1000);
    state.drivers.get_mut("her intents").unwrap().ran_at = Some(5000);
    assert_eq!(stuck(&state, 0, 5000), (1, Some(1000)));
    // Passed over a moment ago: it ticks, so it is not stuck.
    state.drivers.get_mut("memory jobs").unwrap().skipped_at = Some(4990);
    assert_eq!(stuck(&state, 0, 5000), (0, None));
}

#[test]
fn one_of_hers_stuck_is_said_though_one_said_by_a_process_gone_stands() {
    let mut state = came_up(1_795_500_000);
    merge(&mut state, kept_by_a_process_gone(), 1_795_500_000);
    assert_eq!(state.watch.stalled.from, Some(1_795_240_313));
    let since = 1_795_500_000 + 1200;
    assert_eq!(
        told(
            &mut state,
            1_795_500_000,
            since + 3 * 3600,
            Some(("memory jobs", since))
        ),
        [(
            since + QUIET_SECS,
            Trouble::Stalled(Turn::Began { from: since })
        )]
    );
    assert_eq!(state.watch.stalled.from, Some(since));
}

#[test]
fn one_said_by_a_process_gone_ends_once_after_an_hour() {
    let up = 1_795_500_000;
    let mut state = came_up(up);
    merge(&mut state, kept_by_a_process_gone(), up);
    assert_eq!(
        told(&mut state, up, up + 4 * 3600, None),
        [(
            up + WELL_SECS,
            Trouble::Stalled(Turn::Ended {
                from: 1_795_240_313,
                since: up
            })
        )]
    );
    assert_eq!(state.watch.stalled, Held::default());
}

#[test]
fn one_said_by_a_process_gone_is_well_for_an_hour_of_this_one() {
    let up = 1_795_500_000;
    let mut kept = kept_by_a_process_gone();
    // The last process saw it gone, and ended; nothing looked since.
    kept.watch.stalled.well_from = Some(up - 10 * 3600);
    let mut state = came_up(up);
    merge(&mut state, kept, up);
    assert_eq!(
        told(&mut state, up, up + 4 * 3600, None),
        [(
            up + WELL_SECS,
            Trouble::Stalled(Turn::Ended {
                from: 1_795_240_313,
                since: up
            })
        )]
    );
}

#[test]
fn one_said_by_a_process_gone_and_again_in_this_one_is_said_once_here() {
    let up = 1_795_500_000;
    let mut state = came_up(up);
    merge(&mut state, kept_by_a_process_gone(), up);
    let since = up + 600;
    let said = told(&mut state, up, since + 1200, Some(("memory jobs", since)));
    assert_eq!(said.len(), 1);
    // Away for less than an hour, then stuck again: said already.
    let back = since + 1215;
    assert_eq!(told(&mut state, back, back + 1200, None), []);
    let again = back + 1215;
    assert_eq!(
        told(&mut state, again, again + 1800, Some(("memory jobs", again))),
        []
    );
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
