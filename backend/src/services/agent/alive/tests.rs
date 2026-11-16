use super::watch::{GAP_SECS, QUIET_SECS, Said, Seen, WELL_SECS, Watch};
use super::*;
use std::time::Duration;

const LOOK: i64 = LOOK_SECS as i64;

fn seen(now: i64, lived_at: Option<i64>) -> Seen {
    Seen {
        now,
        started_at: 0,
        should_live: true,
        lived_at,
    }
}

/// Looks every [`LOOK`] seconds from `from` to `to`; `lived` says when one
/// of her drivers was last let through as of each look. What was said, and
/// when.
fn looks(
    watch: &mut Watch,
    from: i64,
    to: i64,
    lived: impl Fn(i64) -> Option<i64>,
) -> Vec<(i64, Said)> {
    (from..=to)
        .step_by(LOOK as usize)
        .filter_map(|now| watch.look(seen(now, lived(now))).map(|said| (now, said)))
        .collect()
}

#[test]
fn it_rings_once_after_ten_quiet_minutes() {
    let mut watch = Watch::default();
    // Living until 900, then nothing for an hour.
    let said = looks(&mut watch, 0, 900 + 3600, |now| Some(now.min(900)));
    assert_eq!(said, [(900 + QUIET_SECS, Said::Stopped { from: 900 })]);
    assert_eq!(watch.alarmed_from, Some(900));
}

#[test]
fn a_gap_short_of_ten_minutes_does_not_ring_but_is_counted() {
    let mut watch = Watch::default();
    let stop = 1005;
    let back = stop + QUIET_SECS - LOOK;
    let lived = |now: i64| {
        Some(if now < stop || now >= back {
            now
        } else {
            stop - 5
        })
    };
    assert_eq!(looks(&mut watch, 0, 3000, lived), []);
    assert_eq!(watch.gaps.len(), 1);
    assert_eq!(watch.gaps_of_the_day(3000).0, 1);
}

/// After an alarm, coming back is not said until she has lived an hour
/// without a gap; stopping again within that hour says nothing more. Once
/// she is back, the next stop rings anew.
#[test]
fn it_is_not_said_again_until_she_has_been_well_an_hour() {
    let mut watch = Watch::default();
    // Stops at 900 for twenty minutes, lives twenty, stops for twenty more,
    // then lives on.
    let away = [(900, 2100), (3300, 4500)];
    let lived = |now: i64| {
        Some(
            away.iter()
                .find(|(from, to)| now >= *from && now < *to)
                .map_or(now, |(from, _)| *from),
        )
    };
    let end = 4500 + WELL_SECS;
    let said = looks(&mut watch, 0, end, lived);
    assert_eq!(
        said,
        [
            (900 + QUIET_SECS, Said::Stopped { from: 900 }),
            (
                end,
                Said::Back {
                    from: 900,
                    since: 4500
                }
            ),
        ]
    );
    assert_eq!(watch.alarmed_from, None);
    // Well again: the next stop is a new one.
    let again = end + 600;
    let said = looks(&mut watch, end + LOOK, again + 900, |now| {
        Some(now.min(again))
    });
    assert_eq!(said, [(again + QUIET_SECS, Said::Stopped { from: again })]);
}

/// A restart (a deploy) does not ring: what ran before this process is not
/// counted against it, and its drivers get the same ten minutes.
#[test]
fn a_restart_is_given_ten_minutes() {
    let mut watch = Watch::default();
    let started_at = 100_000;
    // She last lived an hour before the process came up.
    let before = Some(started_at - 3600);
    let look = |watch: &mut Watch, now: i64, lived_at: Option<i64>| {
        watch.look(Seen {
            now,
            started_at,
            should_live: true,
            lived_at,
        })
    };
    for now in (started_at..started_at + QUIET_SECS).step_by(LOOK as usize) {
        assert_eq!(look(&mut watch, now, before), None, "{now}");
    }
    // Her drivers came up within those minutes: nothing to say.
    let mut living = Watch::default();
    for now in (started_at..started_at + 2 * QUIET_SECS).step_by(LOOK as usize) {
        let lived_at = if now < started_at + 90 {
            before
        } else {
            Some(now)
        };
        assert_eq!(look(&mut living, now, lived_at), None, "{now}");
    }
    // They never did: said, ten minutes after the start.
    assert_eq!(
        look(&mut watch, started_at + QUIET_SECS, before),
        Some(Said::Stopped { from: started_at })
    );
}

/// The host asleep is not her stopping. The site's machine sleeps: its
/// drivers' clocks stand still and the wall clock does not, so the first
/// look after it wakes sees an hour with nothing of hers. That look says
/// nothing and keeps the hour apart, as the host's; her ten minutes start
/// from it, and ring if nothing of hers runs in them.
#[test]
fn a_host_that_slept_is_not_her_stopping() {
    let slept = 3600;
    let mut watch = Watch::default();
    assert_eq!(looks(&mut watch, 0, 900, Some), []);
    // The lid closed at 900; an hour on, the look comes before her drivers.
    let woke = 900 + slept;
    assert_eq!(watch.look(seen(woke, Some(900))), None);
    assert_eq!(watch.host_stops, [(900, woke)]);
    assert_eq!(watch.host_stops_of_the_day(), (1, slept));
    assert_eq!(watch.gaps_of_the_day(woke), (0, 0));
    assert_eq!(watch.alarmed_from, None);
    // Her drivers come back with the host: nothing to say, nothing counted.
    let mut living = watch.clone();
    assert_eq!(looks(&mut living, woke + LOOK, woke + 7200, Some), []);
    assert_eq!(living.gaps_of_the_day(woke + 7200), (0, 0));
    // They do not: ten minutes from the look that found the host awake.
    let said = looks(&mut watch, woke + LOOK, woke + 3600, |_| Some(900));
    assert_eq!(said, [(woke + QUIET_SECS, Said::Stopped { from: woke })]);
    assert_eq!(watch.host_stops.len(), 1);
}

/// A stretch of hers that was open when the host stopped ends there, and
/// the hour she has to be well for after an alarm does not run while the
/// host is asleep.
#[test]
fn the_host_asleep_neither_lengthens_her_stop_nor_counts_as_her_being_well() {
    let mut watch = Watch::default();
    // Stopped at 900, said at 1500, back at 2100.
    let lived = |now: i64| Some(if now < 2100 { now.min(900) } else { now });
    let said = looks(&mut watch, 0, 2100 + 1800, lived);
    assert_eq!(said, [(900 + QUIET_SECS, Said::Stopped { from: 900 })]);
    // Half an hour well, then the host sleeps two hours.
    let slept_at = 2100 + 1800;
    let woke = slept_at + 7200;
    assert_eq!(watch.look(seen(woke, Some(slept_at))), None);
    // Back is said once she has been well an hour of the host's waking.
    let said = looks(&mut watch, woke + LOOK, woke + 3600, Some);
    let back_at = woke + WELL_SECS - 1800;
    let back = Said::Back {
        from: 900,
        since: 2100,
    };
    assert_eq!(said, [(back_at, back)]);

    // Stopped four minutes when the lid closed: her stretch is those four.
    let mut watch = Watch::default();
    assert_eq!(looks(&mut watch, 0, 1140, |now| Some(now.min(900))), []);
    let woke = 1140 + 3600;
    assert_eq!(watch.look(seen(woke, Some(900))), None);
    assert_eq!(watch.gaps, [(900, 1140)]);
    assert_eq!(watch.host_stops, [(1140, woke)]);
}

/// Her switch off, or the site not her host: nothing is counted, and the
/// count starts when she should live here again.
#[test]
fn nothing_is_counted_while_she_should_not_live_here() {
    let mut watch = Watch::default();
    let off = |now: i64| Seen {
        now,
        started_at: 0,
        should_live: false,
        lived_at: None,
    };
    for now in (0..7200).step_by(LOOK as usize) {
        assert_eq!(watch.look(off(now)), None);
    }
    // Nothing but when it last looked.
    let looked = Watch {
        looked_at: Some(7185),
        ..Watch::default()
    };
    assert_eq!(watch, looked);
    // Turned on at 7200 with nothing running: ten minutes from then.
    let said = looks(&mut watch, 7200, 7200 + 2 * QUIET_SECS, |_| None);
    assert_eq!(said, [(7200 + QUIET_SECS, Said::Stopped { from: 7200 })]);
    // Turned off while the alarm stands: let go, with nothing said.
    assert_eq!(watch.look(off(9000)), None);
    assert_eq!(watch.alarmed_from, None);
}

fn clock(at: &str) -> i64 {
    let mut parts = at.split(':').map(|part| part.parse::<i64>().unwrap());
    parts.next().unwrap() * 3600 + parts.next().unwrap() * 60 + parts.next().unwrap()
}

/// 2026-11-15, UTC, from the site's log (`[her-log] her mind moved`): each
/// time her mind was lent to a device and when it came back. Meanwhile
/// every driver of hers was passed over here and nothing ran there.
const THE_FIFTEENTH: &[(&str, &str)] = &[
    ("06:26:00", "06:33:01"),
    ("06:39:01", "06:46:31"),
    ("06:50:59", "06:56:30"),
    ("07:14:39", "07:20:01"),
    ("07:40:39", "07:51:30"),
    ("08:01:17", "08:06:31"),
    ("08:15:08", "08:26:01"),
    ("08:39:59", "08:46:02"),
    ("09:05:34", "09:14:35"),
    ("09:20:34", "09:25:35"),
    ("09:44:04", "09:49:05"),
    ("09:57:34", "10:29:06"),
    ("10:51:34", "11:14:35"),
    ("11:15:04", "11:57:35"),
    ("12:01:04", "12:06:05"),
    ("12:12:08", "12:20:37"),
    ("12:22:34", "12:29:05"),
    ("12:33:34", "12:48:35"),
    ("12:52:04", "12:57:35"),
    ("12:59:04", "13:04:05"),
    ("13:06:04", "13:26:35"),
    ("13:28:34", "13:34:05"),
    ("13:53:19", "14:39:54"),
    ("23:29:53", "23:32:19"),
    ("23:32:52", "23:37:24"),
    ("23:52:44", "23:55:16"),
];

/// That day played back: the first four times were 5 to 8 minutes and do
/// not ring; it rings ten minutes into the fifth, once; she is back an hour
/// after the last long one; the three short ones late that night do not
/// ring.
#[test]
fn the_fifteenth_rings_once() {
    let away: Vec<(i64, i64)> = THE_FIFTEENTH
        .iter()
        .map(|(from, to)| (clock(from), clock(to)))
        .collect();
    let lived = |now: i64| {
        Some(
            away.iter()
                .find(|(from, to)| now >= *from && now < *to)
                .map_or(now, |(from, _)| *from),
        )
    };
    let mut watch = Watch::default();
    let said = looks(&mut watch, 0, 24 * 3600, lived);
    let stopped = clock("07:40:39");
    assert_eq!(
        said,
        [
            (clock("07:50:45"), Said::Stopped { from: stopped }),
            (
                clock("15:40:00"),
                Said::Back {
                    from: stopped,
                    since: clock("14:40:00"),
                }
            ),
        ]
    );
    // All 26 are on the page as times she stopped, the short ones too.
    assert!(away.iter().all(|(from, to)| to - from >= GAP_SECS));
    assert_eq!(watch.gaps_of_the_day(24 * 3600).0, away.len());
}

/// A driver that asks while her mind is on a device is written down as
/// passed over, and why, without which device; let through, as run.
#[tokio::test]
async fn a_driver_passed_over_is_written_down() {
    use crate::services::her_log::pen;
    use crate::services::persona_sync::lease;
    const DRIVER: &str = "a driver of hers in a test";
    ticks_every(DRIVER, Duration::from_secs(60));
    assert!(!lease::paused(DRIVER));
    let beat = with(|state| state.drivers[DRIVER].clone());
    assert!(beat.ran_at.is_some());
    assert_eq!((beat.skips, beat.skipped_at), (0, None));
    assert_eq!(with(|state| lived_at(state)), beat.ran_at);

    assert!(pen::quiet("dev_mac", 7, Duration::ZERO).await);
    assert!(lease::paused(DRIVER));
    assert!(lease::paused(DRIVER));
    let beat = with(|state| state.drivers[DRIVER].clone());
    assert_eq!(beat.skips, 2);
    assert!(beat.skipped_at.is_some());
    assert_eq!(beat.skipped_why.as_deref(), Some("mind_elsewhere"));
    assert!(!serde_json::to_string(&beat).unwrap().contains("dev_mac"));

    pen::reset_for_test();
    assert!(!lease::paused(DRIVER));
    assert_eq!(with(|state| state.drivers[DRIVER].skips), 0);

    // The three ways a tick goes are told apart: let through and ended
    // (with or without anything to do), passed over, broken off.
    ended(DRIVER);
    failed(DRIVER).await;
    let beat = with(|state| state.drivers[DRIVER].clone());
    assert_eq!((beat.runs, beat.failures), (2, 1));
    assert!(beat.ended_at.is_some() && beat.failed_at.is_some());
}

#[test]
fn memory_calls_and_ends_are_counted_without_their_words() {
    settled("applied");
    assert_eq!(
        with(|state| state.memory.settled_outcome.clone()).as_deref(),
        Some("applied")
    );
    settled("model_timeout");
    settled("anything else that was said");
    assert_eq!(
        with(|state| state.memory.settled_outcome.clone()).as_deref(),
        Some("failed")
    );
    model_failed("memory_standing_step");
    model_failed("memory_standing_build");
    model_failed("chat_reply");
    assert_eq!(with(|state| state.memory.standing_failed), 2);

    called("act.request", "asked");
    called("act.request", "asked");
    called("act.request", "act_busy");
    let counts: u64 = with(|state| {
        state.calls["act.request"]
            .values()
            .map(|outcomes| outcomes["asked"] + outcomes["act_busy"])
            .sum()
    });
    assert_eq!(counts, 3);

    let declared: Vec<String> = ["log3", "act", "something of its own"]
        .map(String::from)
        .into_iter()
        .chain((0..200).map(|n| format!("extra{n}")))
        .collect();
    let granted = vec!["log3".to_string(), "act".to_string()];
    linked(Hello {
        device_id: "dev_a",
        name: "书房的 Mac",
        protocol: 2,
        app: Some(("MyriadNative", "0.9.1")),
        declared: &declared,
        granted: &granted,
    });
    let ours = BTreeMap::from([("site".to_string(), 40), ("dev_a".to_string(), 7)]);
    let theirs = BTreeMap::from([("site".to_string(), 25), ("dev_a".to_string(), 9)]);
    log_heard("dev_a", "private:someone", &theirs, &ours);
    log_heard("dev_a", "own:", &ours, &ours);
    log_heard("dev_unknown", "own:", &theirs, &ours);
    let state = with(|state| state.clone());
    let end = &state.ends["dev_a"];
    assert_eq!(end.declared.len(), DECLARED_MAX);
    assert_eq!(end.granted, granted);
    assert_eq!(end.behind.values().sum::<u64>(), 15);
    assert!(!state.ends.contains_key("dev_unknown"));
    // Written down without who she talks with.
    let written = serde_json::to_string(&state).unwrap();
    assert!(!written.contains("someone"), "{written}");
    // Linked again: how far behind it was is kept until it says anew.
    linked(Hello {
        device_id: "dev_a",
        name: "书房的 Mac",
        protocol: 2,
        app: None,
        declared: &granted,
        granted: &granted,
    });
    assert_eq!(with(|state| state.ends["dev_a"].behind.len()), 2);
}

async fn fixture() -> Option<crate::federation::test_db::SchemaDb> {
    let fixture = crate::federation::test_db::SchemaDb::new_or_media().await?;
    sea_orm::ConnectionTrait::execute_unprepared(
        &fixture.db,
        "INSERT INTO users (id, username, is_admin, is_owner) VALUES (1, 'owner1', true, true)",
    )
    .await
    .unwrap();
    Some(fixture)
}

/// On a real database (an isolated schema; skipped without a test
/// database): a stop is one notification of the owner's, and her coming
/// back changes that one, it does not add another.
#[tokio::test]
async fn a_stop_and_its_end_are_one_notification() {
    let Some(fixture) = fixture().await else {
        return;
    };
    let db = &fixture.db;
    let manager =
        crate::services::agent::notifications::NotificationManager::new_with_db(200, db.clone())
            .await;
    let rows = || async {
        db.query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, priority, metadata->>'status' AS status, read FROM agent_notifications \
             WHERE user_id = 1",
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| {
            (
                row.try_get::<String>("", "id").unwrap(),
                row.try_get::<String>("", "priority").unwrap(),
                row.try_get::<Option<String>>("", "status").unwrap(),
                row.try_get::<bool>("", "read").unwrap(),
            )
        })
        .collect::<Vec<_>>()
    };
    assert_eq!(rows().await, []);
    manager
        .notify_her_life(
            1,
            Said::Stopped {
                from: 1_794_000_000,
            },
        )
        .await;
    assert_eq!(
        rows().await,
        [(
            "her_life_u1".to_string(),
            "high".to_string(),
            Some("her_life_stopped".to_string()),
            false
        )]
    );
    manager
        .notify_her_life(
            1,
            Said::Back {
                from: 1_794_000_000,
                since: 1_794_030_000,
            },
        )
        .await;
    assert_eq!(
        rows().await,
        [(
            "her_life_u1".to_string(),
            "normal".to_string(),
            Some("her_life_back".to_string()),
            false
        )]
    );
    fixture.close().await;
}

/// What is kept comes back after a restart: when each driver last ran, the
/// alarm that stands (so it is not said twice), the day's counts. What the
/// new process already saw is not overwritten.
#[tokio::test]
async fn what_is_kept_comes_back_after_a_restart() {
    let Some(fixture) = fixture().await else {
        return;
    };
    let db = &fixture.db;
    ticks_every("her own time", Duration::from_secs(60));
    ran("her own time");
    ran("persona nights");
    settled("applied");
    called("act.request", "completed");
    with(|state| {
        state.watch.alarmed_from = Some(123);
        state.watch.should_since = Some(100);
    });
    let before = with(|state| state.clone());
    keep(db).await;

    // A new process: its drivers are named again, one has already run.
    with(|state| *state = State::default());
    ticks_every("her own time", Duration::from_secs(60));
    ticks_every("persona nights", Duration::from_secs(1800));
    ran("persona nights");
    let fresh = with(|state| state.drivers["persona nights"].ran_at);
    recall(db).await;
    let after = with(|state| state.clone());
    assert_eq!(
        after.drivers["her own time"].ran_at,
        before.drivers["her own time"].ran_at
    );
    assert_eq!(after.drivers["her own time"].every, Some(60));
    assert_eq!(after.drivers["persona nights"].ran_at, fresh);
    assert_eq!(after.memory, before.memory);
    assert_eq!(after.calls, before.calls);
    assert_eq!(after.watch.alarmed_from, Some(123));
    assert_eq!(after.watch.should_since, None);
    assert!(after.started_at.is_some());

    // What the owner's page reads: the three layers apart, the middle one
    // not there yet; nothing but times, counts, names and versions.
    let report = report(db, 1).await;
    assert_eq!(report["host"]["holder"], "site");
    assert_eq!(report["host"]["temporary"], false);
    assert_eq!(report["memory"]["settledOutcome"], "applied");
    // The memory line's own counts, by kind.
    for kind in ["asked_again", "unanswered", "unreadable", "not_written"] {
        assert!(
            report["memory"]["standingFailures"][kind].is_u64(),
            "{kind}"
        );
    }
    assert_eq!(report["calls"][0]["kind"], "act.request");
    assert_eq!(report["calls"][0]["outcomes"]["completed"], 1);
    assert_eq!(report["ends"], json!([]));
    let names: Vec<&str> = report["life"]["drivers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|driver| driver["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["her own time", "persona nights"]);
    fixture.close().await;
}
