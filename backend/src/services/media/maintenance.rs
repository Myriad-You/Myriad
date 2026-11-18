//! Bounded maintenance, owned by the process cleanup loop on every worker.
use super::{MediaError, MediaService};
use crate::models::entities::media_assets;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Statement,
};

pub async fn maintain(db: &DatabaseConnection) -> Result<(), MediaError> {
    let service = MediaService::from_data_paths(crate::services::data_paths::paths());
    let recovery = service.recover_expired(db, 16).await;
    let deletions = retry_deletions(&service, db, 16).await;
    let stickers = cite_sticker_pictures(db).await;
    let refs = prune_references(db, 500).await;
    recovery
        .map(|_| ())
        .and(deletions)
        .and(stickers.map(|_| ()))
        .and(refs.map(|_| ()))
}

/// The consumer her stickers cite their pictures as, one per sticker.
pub(super) const STICKER_CONSUMER: &str = "merope_sticker";

/// The media asset sticker `s` is drawn as, by its public id: only at the
/// canonical address `/media/assets/<public id>/<file>`.
const STICKER_PICTURE: &str = "CASE \
    WHEN btrim(s.asset_id) ~ '^/media/assets/[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/' \
    THEN substring(btrim(s.asset_id) FROM 15 FOR 36)::uuid END";

/// Every sticker of hers (kept from a group, drawn, or brought by a device)
/// cites the picture it is drawn as, so the media library never offers to
/// delete a picture she can still send. Stickers are written from many
/// places; this one statement covers them all and is safe to run again and
/// again: it only adds what is missing. A reference the sticker no longer
/// needs goes with the others in [`prune_references`]. Only a picture at its
/// canonical address and still ready is cited.
pub async fn cite_sticker_pictures(db: &impl ConnectionTrait) -> Result<u64, MediaError> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO media_references
    (asset_id, consumer_type, consumer_id, slot, requires_public, expires_at, created_at)
SELECT m.id, $1, s.id, 'picture', m.exposure = 'public', NULL, $2::timestamptz
FROM merope_stickers s
JOIN media_assets m ON m.public_id = {sticker_picture}
WHERE m.state = 'ready'
ON CONFLICT (asset_id, consumer_type, consumer_id, slot) DO NOTHING
"#
            .replace("{sticker_picture}", STICKER_PICTURE),
            [STICKER_CONSUMER.into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected())
}

/// References whose consumer can no longer show the media: a sticker gone
/// or drawn as another picture now among them. Conversation
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
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
DELETE FROM media_references WHERE id IN (
    SELECT r.id FROM media_references r
    WHERE (r.expires_at IS NOT NULL AND r.expires_at < $2::timestamptz - interval '1 day')
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
       OR (r.consumer_type = 'merope_sticker'
           AND NOT EXISTS (SELECT 1 FROM merope_stickers s
                           JOIN media_assets a ON a.public_id = {sticker_picture}
                           WHERE s.id = r.consumer_id AND a.id = r.asset_id))
       OR (r.consumer_type = 'tapp_storage'
           AND NOT EXISTS (SELECT 1 FROM tapp_storage s
                           WHERE s.id = CASE WHEN r.consumer_id ~ '^[0-9]+$'
                                        THEN r.consumer_id::int END))
       OR (r.consumer_type = 'channel_message'
           AND r.consumer_id LIKE 'run\_%'
           AND r.expires_at IS NULL
           AND r.created_at < $2::timestamptz - interval '1 day')
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
"#
            .replace("{sticker_picture}", STICKER_PICTURE),
            [(limit.clamp(1, 5000) as i64).into(), crate::clock::now().into()],
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
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE media_assets SET updated_at = $2::timestamptz WHERE id = $1 AND state = 'deleting'",
            [row.id.into(), crate::clock::now().into()],
        ))
        .await?;
        if let Err(error) = service.delete(db, row.id).await {
            tracing::warn!(asset_id = row.id, %error, "media deletion retry failed");
        }
    }
    Ok(())
}
