//! How many calls an account's upkeep made today, kept in the runtime registry
//! so a restart does not reset it.

use sea_orm::ConnectionTrait;

use super::Error;
use crate::services::runtime_registry as registry;

/// Where the calls an account's upkeep made today are counted.
pub(super) const DAY_NAMESPACE: &str = "merope_memory_node_upkeep";

fn day_key(user_id: i32, audience: &str) -> String {
    format!(
        "{user_id}:{audience}:{}",
        crate::services::agent::merope::clock::today()
    )
}

/// Calls the upkeep of `user_id`'s `audience` made today.
pub(crate) async fn calls_today<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<u32, Error> {
    Ok(
        registry::get::<u32>(db, DAY_NAMESPACE, &day_key(user_id, audience))
            .await?
            .unwrap_or(0),
    )
}

pub(crate) async fn count_calls<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    calls: u32,
) -> Result<(), Error> {
    let total = calls_today(db, user_id, audience).await? + calls;
    registry::put(
        db,
        DAY_NAMESPACE,
        &day_key(user_id, audience),
        registry::RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &total,
        crate::clock::now().timestamp() + 2 * 86_400,
    )
    .await?;
    Ok(())
}
