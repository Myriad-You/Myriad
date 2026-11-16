//! Handing a channel activity to the other end.
//!
//! A remote inbox gets it through the durable queue. An inbox on this instance
//! is handled in-process once the caller has committed: the delivery worker
//! refuses private addresses, and its process has no live sockets to tell.

use sea_orm::{ConnectionTrait, DatabaseConnection};

use crate::federation::inbox::{deliver_activity_locally, local_username_from_inbox_url};
use crate::federation::types::*;

/// A recorded activity, sent by [`Outbound::send`] after the commit.
pub(super) struct Outbound {
    row: i32,
    inbox: String,
    activity: serde_json::Value,
    local: Option<String>,
}

/// Record `activity`, queueing it unless its inbox is on this instance.
pub(super) async fn record(
    db: &impl ConnectionTrait,
    user_id: i32,
    activity_id: &str,
    activity_type: &str,
    object_type: Option<&str>,
    activity: serde_json::Value,
    inbox: &str,
) -> Result<Outbound, sea_orm::DbErr> {
    let row = insert_local_activity(
        db,
        user_id,
        activity_id,
        activity_type,
        object_type,
        activity.clone(),
    )
    .await?;
    let local = local_username_from_inbox_url(&get_base_url().await, inbox);
    if local.is_none() {
        enqueue_delivery(db, row, inbox, "pending").await?;
    }
    Ok(Outbound {
        row,
        inbox: inbox.to_string(),
        activity,
        local,
    })
}

impl Outbound {
    /// Deliver to this instance's inbox, leaving it to the queue if that fails.
    pub(super) async fn send(self, db: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
        let Some(username) = self.local else {
            return Ok(());
        };
        if let Err(error) = deliver_activity_locally(db, &username, &self.activity).await {
            tracing::warn!(
                target = %username,
                %error,
                "[Channel] Local delivery failed; leaving it to the queue"
            );
            enqueue_delivery(db, self.row, &self.inbox, "pending").await?;
        }
        Ok(())
    }
}
