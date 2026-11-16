//! A memory job goes on for the her it was taken up for, however she is
//! edited meanwhile; another her, or her cleared, lets it go.

use super::*;

/// Her persona on the site, no copy of her yet.
async fn persona(db: &DatabaseConnection) {
    db.execute_unprepared(
        "INSERT INTO agent_persona (id, name, updated_at) VALUES ('site', 'Merope', NOW())",
    )
    .await
    .unwrap();
}

/// Her copy, born.
async fn born(db: &DatabaseConnection) {
    crate::services::persona_sync::lease::read(db)
        .await
        .unwrap()
        .expect("born");
}

/// Her persona on the site, and her copy, born.
async fn her(db: &DatabaseConnection) {
    persona(db).await;
    born(db).await;
}

/// Her copy born while a line is being gone over, nothing else of her
/// changed: the same her. What they say after is noted as hers, so her
/// edited later lets nothing go either.
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn her_copy_born_meanwhile_lets_no_memory_job_go() {
    let schema = fixture().await;
    let db = &schema.db;
    persona(db).await;
    chat(db, "line", Audience::private(7)).await;
    let first = claim(db).await.unwrap().unwrap();
    born(db).await;
    chat(db, "line", Audience::private(7)).await;
    finish(db, &first, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 1, "the line's note is kept");
    assert!(
        loaded(db, "line")
            .await
            .her
            .is_some_and(|her| her.lineage.is_some()),
        "noted as hers"
    );
    let second = claim(db).await.unwrap().unwrap();
    let tx = db.begin().await.unwrap();
    crate::services::agent::merope::upsert_persona_on(
        &tx,
        "小满".into(),
        "New personality".into(),
        crate::services::agent::merope::PortraitUpdate::Keep,
        Default::default(),
        7,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    finish(db, &second, Ok(fact_of("养了一只狗叫豆豆")))
        .await
        .unwrap();
    assert_eq!(facts(db).await, 2, "kept although she was edited");
    schema.drop().await;
}

/// Edited while a line is being gone over (her name, then a portrait being
/// drawn noting how it goes): she is the same her, and what the line left
/// is kept.
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn editing_her_lets_no_memory_job_go() {
    let schema = fixture().await;
    let db = &schema.db;
    her(db).await;
    chat(db, "line", Audience::private(7)).await;
    let line = claim(db).await.unwrap().unwrap();
    db.execute_unprepared(
        "UPDATE agent_persona SET name = '小满', updated_at = NOW() + INTERVAL '1 second' \
         WHERE id = 'site'",
    )
    .await
    .unwrap();
    db.execute_unprepared(
        "UPDATE agent_persona SET portrait_generation = '{\"pending\": {}}'::jsonb, \
         updated_at = NOW() + INTERVAL '2 seconds' WHERE id = 'site'",
    )
    .await
    .unwrap();
    finish(db, &line, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 1, "the line's note is kept");
    assert_eq!(loaded(db, "line").await.outcome.as_deref(), Some("applied"));
    schema.drop().await;
}

/// Another her (born again, or brought back from another end's backup)
/// while a line is being gone over: it is let go. Her cleared: every job
/// is.
#[tokio::test]
#[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL; isolated migrated schema"]
async fn another_her_or_her_cleared_lets_the_jobs_go() {
    let schema = fixture().await;
    let db = &schema.db;
    her(db).await;
    chat(db, "line", Audience::private(7)).await;
    let line = claim(db).await.unwrap().unwrap();
    use crate::services::persona_sync::lease;
    lease::seed(db, lease::SITE, "another", 1, lease::State::Live)
        .await
        .unwrap();
    finish(db, &line, Ok(fact())).await.unwrap();
    assert_eq!(facts(db).await, 0, "not kept for another her");
    assert_eq!(loaded(db, "line").await.outcome.as_deref(), Some("stale"));

    chat(db, "again", Audience::private(7)).await;
    crate::services::agent::merope::clear_persona_on(db)
        .await
        .unwrap();
    assert!(
        registry::get::<Job>(db, NAMESPACE, "again")
            .await
            .unwrap()
            .is_none(),
        "cleared with her"
    );
    schema.drop().await;
}
