//! Syncing her with other platforms (see `myriad_persona_sync` for the
//! format): the devices that do it, and what the site has of her.
//!
//! Only what is hers or the site owner's crosses: her own memories, and the
//! owner's private ones whose audience is the owner alone. Nobody else's
//! memories, and nothing learned in a group.

pub mod assets;
pub mod devices;
pub mod fields;
mod outside;
pub mod snapshot;

use myriad_persona_sync::{Holdings, entity::EmbeddingModel};
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};

/// Most entities in one pull page, the default, and most bytes.
pub const PULL_MAX_ENTITIES: usize = 2000;
pub const PULL_DEFAULT_ENTITIES: usize = 500;
pub const PULL_MAX_BYTES: usize = 4 * 1024 * 1024;
/// What a push will take, once pushing exists.
pub const PUSH_MAX_ENTITIES: usize = 500;
pub const PUSH_MAX_BYTES: usize = 1024 * 1024;
/// Largest asset served or taken.
pub const ASSET_MAX_BYTES: u64 = 24 * 1024 * 1024;

/// The memories that cross, for the owner `$1`: her own, and the owner's
/// private ones heard by the owner alone.
pub(crate) const SYNCED_MEMORY: &str = "(venue = 'own' OR (venue = 'private' AND user_id = $1 \
     AND audience <@ jsonb_build_array($1::int)))";

/// What the site holds of her, for deciding whether it is empty.
pub async fn holdings<C: ConnectionTrait>(db: &C, owner: i32) -> Result<Holdings, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT \
                 EXISTS (SELECT 1 FROM agent_persona WHERE id = 'site') AS persona, \
                 (SELECT COUNT(*) FROM agent_memories WHERE {SYNCED_MEMORY})::BIGINT AS memories, \
                 (SELECT COUNT(*) FROM agent_diary WHERE user_id = $1)::BIGINT AS diary, \
                 (SELECT COUNT(*) FROM agent_affect_events WHERE user_id = $1)::BIGINT \
                 AS affect_events"
            ),
            [owner.into()],
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("holdings query returned no row".into()))?;
    Ok(Holdings {
        persona: row.try_get("", "persona")?,
        memories: row.try_get::<i64>("", "memories")?.max(0) as u64,
        diary: row.try_get::<i64>("", "diary")?.max(0) as u64,
        affect_events: row.try_get::<i64>("", "affect_events")?.max(0) as u64,
    })
}

/// The site's embedding model, as two sides compare it: the configured
/// name, trimmed, and the length of the vectors it gave. None when no
/// model is set or no vector is kept yet.
pub async fn embedding_model<C: ConnectionTrait>(db: &C) -> Result<Option<EmbeddingModel>, DbErr> {
    let model = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .aux_embedding_model
        .trim()
        .to_string();
    if model.is_empty() {
        return Ok(None);
    }
    let dims = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT (octet_length(vector) / 4)::INTEGER AS dims FROM agent_memory_embeddings \
             WHERE model = $1 LIMIT 1",
            [model.clone().into()],
        ))
        .await?
        .map(|row| row.try_get::<i32>("", "dims"))
        .transpose()?;
    Ok(dims.filter(|dims| *dims > 0).map(|dims| EmbeddingModel {
        model,
        dims: dims as u32,
    }))
}

/// Her name on the site, trimmed; none when she has no persona row.
pub async fn persona_name<C: ConnectionTrait>(db: &C) -> Result<Option<String>, DbErr> {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT name FROM agent_persona WHERE id = 'site'".to_string(),
    ))
    .await?
    .map(|row| {
        row.try_get::<String>("", "name")
            .map(|name| name.trim().to_string())
    })
    .transpose()
}
