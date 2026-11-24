//! What was told to stay on one try is not asked about on the next.

use sea_orm::{DatabaseBackend, Statement};

use super::*;
use crate::services::agent::merope::call;
use crate::services::agent::merope::memory_jobs::Failure;
use store::memory_jobs::{Claim, Own, claim, finish};

/// The job `id`, taken up again.
async fn taken(db: &DatabaseConnection, id: &str) -> Claim {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE runtime_registry \
         SET payload = jsonb_set(jsonb_set(payload, '{ready}', '0'), '{lease}', '0') \
         WHERE record_id = $1",
        [id.into()],
    ))
    .await
    .unwrap();
    let claimed = claim(db).await.unwrap().expect("due");
    assert_eq!(claimed.id, id);
    claimed
}

fn stayed(claimed: &Claim) -> usize {
    Own::of(claimed).stayed().len()
}

#[tokio::test]
async fn what_was_told_to_stay_is_not_asked_about_again() {
    let Some(schema) = schema("memory_nodes_asked_old_stays").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    asks(db, OTHER, ASK).await;
    for n in 0..60 {
        let text = format!("说的第{n}件");
        makes(db, &[(bits::SOURCE, text.as_str(), None)]).await;
    }
    let drops = |places: &[usize]| Some(serde_json::json!({ "drop": places }).to_string());
    let away = Some("!rejected".to_string());
    waits(db).await;

    // The first part told, one of it to go; the rest not to be reached.
    let first = taken(db, "asked_over").await;
    assert_eq!(stayed(&first), 0);
    let (done, asked) = over_of(db, &[drops(&[3]), away.clone()], Some(Own::of(&first))).await;
    assert!(!done);
    let told: std::collections::BTreeSet<String> = asked[0].iter().cloned().collect();
    assert_eq!(made(db, &[bits::SOURCE]).await.len(), 59);
    let failed = Err(Failure::Model(call::Failure::Rejected));
    finish(db, &first, failed).await.unwrap();

    // Those told to stay are not asked about again; one that says something
    // else since is.
    let again = taken(db, "asked_over").await;
    assert_eq!(stayed(&again), told.len() - 1);
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET content = '改过的' WHERE content = $1",
        [asked[0][0].clone().into()],
    ))
    .await
    .unwrap();
    let answers = [drops(&[]), away];
    let (done, asked) = over_of(db, &answers, Some(Own::of(&again))).await;
    assert!(!done);
    assert!(asked[0].contains(&"改过的".to_string()), "{asked:?}");
    assert!(asked.iter().flatten().all(|text| !told.contains(text)));
    let failed = Err(Failure::Model(call::Failure::Rejected));
    finish(db, &again, failed).await.unwrap();

    // Taken up by another since: nothing is noted by the one before.
    let last = taken(db, "asked_over").await;
    assert_eq!(stayed(&last), 2 * told.len() - 1);
    let late = vec!["late".to_string()];
    Own::of(&again).stays(db, late).await.unwrap();
    let last = taken(db, "asked_over").await;
    assert_eq!(stayed(&last), 2 * told.len() - 1);
    let (done, asked) = over_of(db, &[drops(&[])], Some(Own::of(&last))).await;
    assert!(done);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].len(), 61 - 2 * told.len());

    // Another request of theirs: all of it, from the first.
    let data = Payload::AskedOver {
        asked: crate::clock::now().fixed_offset(),
        venue: None,
        stays: Vec::new(),
    };
    store::memory_jobs::enqueue(db, "asked_anew", OTHER, data, None)
        .await
        .unwrap();
    let anew = taken(db, "asked_anew").await;
    let (done, asked) = over_of(db, &vec![drops(&[]); 3], Some(Own::of(&anew))).await;
    assert!(done);
    assert_eq!(asked.iter().flatten().count(), 59);
    schema.drop().await;
}

/// Refused, and then the rest not to be reached: tried again as what cannot
/// be reached, counted as refused, and never let be for it.
#[tokio::test]
async fn refused_and_then_not_reached_counts_as_refused_and_is_not_let_be() {
    let Some(schema) = schema("memory_nodes_asked_old_unreached").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    asks(db, OTHER, ASK).await;
    makes(
        db,
        &[
            (bits::SOURCE, "话太多", None),
            (bits::SOURCE, "爱熬夜", None),
        ],
    )
    .await;
    waits(db).await;
    let refused = Some("!refused");
    for nth in 1..=5 {
        let claimed = taken(db, "asked_over").await;
        judging(&[refused, refused, Some("!rejected")], &[]);
        let own = Some(Own::of(&claimed));
        let gives_up = nth > 3;
        let done = old::let_go(db, OTHER, soon(), gives_up, 0, own).await;
        g0::leave();
        let failure = done.expect_err("not all of it reached");
        assert!(!failure.untold() && !failure.refused());
        assert_eq!(failure.counted(), Some(call::Failure::Refused));
        finish(db, &claimed, Err(failure)).await.unwrap();
        // Counted from the first, by the days alone; tried as often as ever.
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT (payload->>'untold')::int AS untold, \
                   (payload->>'untold_since') IS NOT NULL AS since, \
                   (payload->>'ready')::bigint - EXTRACT(EPOCH FROM NOW())::bigint AS wait \
                 FROM runtime_registry WHERE record_id = 'asked_over'",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i32>("", "untold").unwrap(), 0);
        assert!(row.try_get::<bool>("", "since").unwrap());
        assert!(row.try_get::<i64>("", "wait").unwrap() <= 600);
        assert_eq!(made(db, &[bits::SOURCE]).await.len(), 2);
    }
    schema.drop().await;
}
