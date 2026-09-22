use super::*;
use crate::models::entities::media_references;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter, QueryOrder, Statement,
};
use test_support::Fixture;

async fn refs(db: &impl ConnectionTrait) -> Vec<media_references::Model> {
    media_references::Entity::find()
        .order_by_asc(media_references::Column::Id)
        .all(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn postgres_reference_batch_is_one_insert_and_failures_preserve_old_rows() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    f.db.execute_unprepared(
        "CREATE TABLE ref_insert_statements (n integer);
        CREATE FUNCTION count_ref_insert() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN INSERT INTO ref_insert_statements VALUES (1); RETURN NULL; END $$;
        CREATE TRIGGER count_ref_insert AFTER INSERT ON media_references
        FOR EACH STATEMENT EXECUTE FUNCTION count_ref_insert()",
    )
    .await
    .unwrap();
    let input: Vec<_> = (0..100)
        .map(|n| NewReference {
            asset_id: image.id,
            slot: format!("body:{n}"),
            requires_public: false,
            expires_at: None,
        })
        .collect();
    let txn = f.db.begin().await.unwrap();
    replace_for_consumer(&txn, "note_draft", "1", &input)
        .await
        .unwrap();
    txn.commit().await.unwrap();
    let row =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n FROM ref_insert_statements",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "n").unwrap(), 1);
    let before = refs(&f.db).await;
    assert_eq!(before.len(), 100);

    let mut invalid = input.clone();
    invalid[99].slot = " ".into();
    let txn = f.db.begin().await.unwrap();
    assert!(
        replace_for_consumer(&txn, "note_draft", "1", &invalid)
            .await
            .is_err()
    );
    assert_eq!(
        refs(&txn).await,
        before,
        "validation precedes the destructive replacement"
    );
    txn.commit().await.unwrap();

    let txn = f.db.begin().await.unwrap();
    assert!(
        replace_for_consumer(
            &txn,
            "note_draft",
            "1",
            &[input[0].clone(), input[0].clone()]
        )
        .await
        .is_err()
    );
    txn.rollback().await.unwrap();
    assert_eq!(
        refs(&f.db).await,
        before,
        "SQL failure rolls back the deletion and the entire batch"
    );

    let txn = f.db.begin().await.unwrap();
    replace_for_consumer(&txn, "note_draft", "1", &[])
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert!(refs(&f.db).await.is_empty());
    f.close().await;
}

#[tokio::test]
async fn postgres_reference_counts_and_existence_keep_expiry_and_public_boundaries() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let image = f.image().await;
    let now = chrono::Utc::now();
    let input = [
        NewReference {
            asset_id: image.id,
            slot: "private".into(),
            requires_public: false,
            expires_at: None,
        },
        NewReference {
            asset_id: image.id,
            slot: "public".into(),
            requires_public: true,
            expires_at: Some(now + chrono::Duration::hours(1)),
        },
        NewReference {
            asset_id: image.id,
            slot: "expired".into(),
            requires_public: true,
            expires_at: Some(now - chrono::Duration::hours(1)),
        },
    ];
    let txn = f.db.begin().await.unwrap();
    replace_for_consumer(&txn, "ai_task", "task", &input)
        .await
        .unwrap();
    txn.commit().await.unwrap();
    assert_eq!(active_count(&f.db, image.id).await.unwrap(), 2);
    assert!(
        references::has_active(&f.db, image.id, false)
            .await
            .unwrap()
    );
    assert!(references::has_active(&f.db, image.id, true).await.unwrap());
    assert_eq!(
        f.service.delete(&f.db, image.id).await.unwrap_err(),
        MediaError::InUse
    );
    assert_eq!(
        f.service.unpublish(&f.db, image.id).await.unwrap_err(),
        MediaError::PublicInUse
    );
    media_references::Entity::delete_many()
        .filter(media_references::Column::Slot.eq("public"))
        .exec(&f.db)
        .await
        .unwrap();
    assert_eq!(active_count(&f.db, image.id).await.unwrap(), 1);
    assert!(!references::has_active(&f.db, image.id, true).await.unwrap());
    f.service.unpublish(&f.db, image.id).await.unwrap();
    media_references::Entity::delete_many()
        .filter(media_references::Column::Slot.eq("private"))
        .exec(&f.db)
        .await
        .unwrap();
    assert_eq!(active_count(&f.db, image.id).await.unwrap(), 0);
    assert!(
        !references::has_active(&f.db, image.id, false)
            .await
            .unwrap()
    );
    assert!(!references::has_active(&f.db, -1, false).await.unwrap());
    f.service.delete(&f.db, image.id).await.unwrap();
    f.close().await;
}

async fn save_draft(f: &Fixture, revision: i64, body: &str) -> Result<(), MediaError> {
    let txn = f.db.begin().await?;
    txn.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "UPDATE phantasi_note_docs SET content_md = $1, title = $2, revision = revision + 1 WHERE id = 1",
        [body.into(), format!("revision {revision}").into()],
    )).await?;
    bind_note_draft(&txn, 1, revision, None, body, &[]).await?;
    txn.commit().await?;
    Ok(())
}

#[tokio::test]
async fn postgres_history_keeps_retained_references_prunes_evictions_and_rolls_back() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let a = f.image().await;
    let b = f.image().await;
    let body_a = format!("![a]({})", a.content_path);
    let body_b = format!("![b]({})", b.content_path);
    f.db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO phantasi_note_docs (id, user_id, content_md, revision) VALUES (1, 1, $1, 1)",
        [body_a.clone().into()],
    ))
    .await
    .unwrap();
    let txn = f.db.begin().await.unwrap();
    bind_note_draft(&txn, 1, 1, None, &body_a, &[])
        .await
        .unwrap();
    txn.commit().await.unwrap();
    save_draft(&f, 1, &body_b).await.unwrap();
    let first_history = refs(&f.db)
        .await
        .into_iter()
        .find(|r| r.consumer_type == "note_history")
        .unwrap();
    assert_eq!(first_history.asset_id, a.id);

    // Classification also captures history; it must bind that revision before the next edit.
    crate::services::note_publish::update_note_doc_topic(&f.db, 1, 2, Some("topic".into()))
        .await
        .unwrap();
    let second_history = refs(&f.db)
        .await
        .into_iter()
        .find(|r| r.consumer_id == "1:2")
        .unwrap();
    assert_eq!(second_history.asset_id, b.id);
    save_draft(&f, 3, "no media").await.unwrap();
    assert!(
        refs(&f.db).await.contains(&first_history),
        "unchanged history is not deleted/reinserted"
    );
    assert!(refs(&f.db).await.contains(&second_history));
    assert_eq!(
        f.service.delete(&f.db, a.id).await.unwrap_err(),
        MediaError::InUse
    );

    // Failed binding rolls back the document, captured version and reference pruning.
    let before = refs(&f.db).await;
    assert!(
        save_draft(&f, 4, "![missing](/api/media/2147483647/content)")
            .await
            .is_err()
    );
    assert_eq!(refs(&f.db).await, before);
    let row =
        f.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT revision FROM phantasi_note_docs WHERE id = 1",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<i64>("", "revision").unwrap(), 4);

    for revision in 4..=12 {
        save_draft(&f, revision, "no media").await.unwrap();
    }
    assert!(
        !refs(&f.db)
            .await
            .iter()
            .any(|r| r.consumer_id == "1:1" || r.consumer_id == "1:2")
    );
    assert_eq!(active_count(&f.db, a.id).await.unwrap(), 0);

    // Restore preserves unsaved input first, then restores: both captured versions belong to one write.
    let txn = f.db.begin().await.unwrap();
    for body in [&body_a, &body_b] {
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE phantasi_note_docs SET content_md = $1, revision = revision + 1 WHERE id = 1",
            [body.clone().into()],
        ))
        .await
        .unwrap();
    }
    bind_note_draft(&txn, 1, 13, None, &body_b, &[])
        .await
        .unwrap();
    txn.commit().await.unwrap();
    let saved = refs(&f.db).await;
    assert!(
        saved
            .iter()
            .any(|r| r.consumer_id == "1:14" && r.asset_id == a.id)
    );
    assert!(
        saved
            .iter()
            .any(|r| r.consumer_type == "note_draft" && r.asset_id == b.id)
    );
    assert_eq!(
        f.service.delete(&f.db, a.id).await.unwrap_err(),
        MediaError::InUse
    );
    f.close().await;
}
