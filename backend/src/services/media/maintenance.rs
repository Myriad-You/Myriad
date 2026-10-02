//! Bounded maintenance, owned by the process cleanup loop on every worker.
use super::{MediaError, MediaService};
use crate::models::entities::media_assets;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

pub async fn maintain(db: &DatabaseConnection) -> Result<(), MediaError> {
    let service = MediaService::from_data_paths(crate::services::data_paths::paths());
    let recovery = service.recover_expired(db, 16).await;
    let deletions = retry_deletions(&service, db, 16).await;
    let refs = prune_references(db, 500).await;
    recovery.map(|_| ()).and(deletions).and(refs.map(|_| ()))
}

/// References whose consumer can no longer show the media. Conversation
/// messages go away by cascade with their session, so their references are
/// pruned here rather than at every delete site. Expired references are kept
/// a day for diagnosis. Run inputs bound before they expired on their own are
/// dropped once the run is long over. Bounded per tick.
///
/// 已撤回的联邦发布：撤回现在在同一事务里释放以原 Create 为消费者的引用，
/// 但更早撤回的帖子没有。判定只看
/// 两件确定的事：已发布行已经不在，且同一用户有一条以该 Create 为对象的本地
/// Delete（只有撤回会写）。还在已发布列表里的、从没撤回过的都不碰。
pub async fn prune_references(db: &DatabaseConnection, limit: u64) -> Result<u64, MediaError> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
DELETE FROM media_references WHERE id IN (
    SELECT r.id FROM media_references r
    WHERE (r.expires_at IS NOT NULL AND r.expires_at < NOW() - interval '1 day')
       OR (r.consumer_type = 'channel_message'
           AND r.consumer_id ~ '^agent_messages:[0-9]+$'
           AND NOT EXISTS (SELECT 1 FROM agent_messages m
                           WHERE m.id = CASE WHEN r.consumer_id ~ '^agent_messages:[0-9]+$'
                                        THEN substring(r.consumer_id FROM 16)::int END))
       OR (r.consumer_type = 'channel_message'
           AND r.consumer_id ~ '^federation_channel_messages:[0-9]+$'
           AND NOT EXISTS (SELECT 1 FROM federation_channel_messages m
                           WHERE m.id = CASE WHEN r.consumer_id ~ '^federation_channel_messages:[0-9]+$'
                                        THEN substring(r.consumer_id FROM 29)::int END))
       OR (r.consumer_type = 'tapp_storage'
           AND NOT EXISTS (SELECT 1 FROM tapp_storage s
                           WHERE s.id = CASE WHEN r.consumer_id ~ '^[0-9]+$'
                                        THEN r.consumer_id::int END))
       OR (r.consumer_type = 'channel_message'
           AND r.consumer_id LIKE 'run\_%'
           AND r.expires_at IS NULL
           AND r.created_at < NOW() - interval '1 day')
       OR (r.consumer_type = 'federation_activity'
           AND NOT EXISTS (SELECT 1 FROM federation_published_content p
                           WHERE p.activity_id = r.consumer_id)
           AND EXISTS (SELECT 1 FROM federation_activities c
                       WHERE c.activity_id = r.consumer_id
                         AND c.is_local AND c.activity_type = 'Create'
                         AND (c.user_id, c.activity_id) IN (
                             SELECT d.user_id,
                                    COALESCE(d.object_json #>> '{object,id}',
                                             d.object_json ->> 'object')
                             FROM federation_activities d
                             WHERE d.is_local AND d.activity_type = 'Delete')))
    LIMIT $1
)
"#,
            [(limit.clamp(1, 5000) as i64).into()],
        ))
        .await?;
    Ok(result.rows_affected())
}

pub(super) async fn retry_deletions(
    service: &MediaService,
    db: &DatabaseConnection,
    limit: u64,
) -> Result<(), MediaError> {
    let rows = media_assets::Entity::find()
        .filter(media_assets::Column::State.eq("deleting"))
        .order_by_asc(media_assets::Column::UpdatedAt)
        .limit(limit.clamp(1, 32))
        .all(db)
        .await?;
    for row in rows {
        // Touch before retry so a permanently failing file cannot starve others.
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE media_assets SET updated_at = NOW() WHERE id = $1 AND state = 'deleting'",
            [row.id.into()],
        ))
        .await?;
        if let Err(error) = service.delete(db, row.id).await {
            tracing::warn!(asset_id = row.id, %error, "media deletion retry failed");
        }
    }
    Ok(())
}
