//! The site's reminders come due (H2-5). `reminder.create` keeps a reminder
//! under the `agent_reminders` Tapp's storage; this loop reads the active
//! ones and, when one is due by the shared rule
//! (`myriad_agent_rules::reminder`, the same a Mac runs for its own), tells
//! the owner through the notification center (`agent.reminder_due`). A Mac
//! the owner is using her on hears it from there as a `reminder` notice on
//! her link (`link::notices`).
//!
//! How far each reminder was reminded of is kept in it
//! (`remindedThrough`, milliseconds), written before the notification goes
//! out and only over the value it was read with, so a time is reminded of at
//! most once, and times missed while nothing ran fold into one. A reminder
//! with no offset in its `datetime` is read in her zone (the site's own when
//! her persona names none).

use chrono::{DateTime, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::notification_preferences::{ACTION_OPEN_AGENT, NotificationEventKey};
use super::notifications::{
    Notification, NotificationPriority, NotificationType, get_notification_manager,
};
use super::resource_create_pure::AGENT_REMINDERS_TAPP_ID;

/// Where a reminder keeps how far it was reminded of.
pub const REMINDED_THROUGH: &str = "remindedThrough";
/// At most this many reminders are read each pass.
const PASS: i64 = 500;

/// One active reminder as kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub id: i32,
    pub user_id: i32,
    pub key: String,
    pub title: String,
    pub datetime: String,
    pub repeat: String,
    pub reminded_through: Option<i64>,
}

impl Kept {
    fn of(id: i32, user_id: i32, key: String, value: &Value) -> Option<Self> {
        Some(Self {
            id,
            user_id,
            key,
            title: value.get("title")?.as_str()?.to_string(),
            datetime: value.get("datetime")?.as_str()?.to_string(),
            repeat: value
                .get("repeat")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_string(),
            reminded_through: value.get(REMINDED_THROUGH).and_then(Value::as_i64),
        })
    }

    /// The time to remind of now, if any.
    pub fn due<Tz: TimeZone>(&self, now: DateTime<Utc>, zone: &Tz) -> Option<DateTime<Utc>> {
        let through = self
            .reminded_through
            .and_then(|millis| Utc.timestamp_millis_opt(millis).single());
        myriad_agent_rules::reminder::due(&self.datetime, &self.repeat, through, now, zone)
    }
}

/// The active reminders kept on the site.
pub async fn active(db: &DatabaseConnection) -> Result<Vec<Kept>, sea_orm::DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, user_id, key, value FROM tapp_storage \
             WHERE tapp_id = $1 AND value->>'status' = 'active' ORDER BY id LIMIT $2",
            [AGENT_REMINDERS_TAPP_ID.into(), PASS.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let value: Value = row.try_get("", "value").ok()?;
            Kept::of(
                row.try_get("", "id").ok()?,
                row.try_get("", "user_id").ok()?,
                row.try_get("", "key").ok()?,
                &value,
            )
        })
        .collect())
}

/// Marks the reminder reminded of through `at`, unless another pass did it
/// meanwhile: true when this pass did.
async fn mark(
    db: &DatabaseConnection,
    kept: &Kept,
    at: DateTime<Utc>,
) -> Result<bool, sea_orm::DbErr> {
    let written = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE tapp_storage SET value = jsonb_set(value::jsonb, '{remindedThrough}', to_jsonb($2::bigint))::json, updated_at = $4::timestamptz \
             WHERE id = $1 AND (value->>'remindedThrough') IS NOT DISTINCT FROM $3::text",
            [
                kept.id.into(),
                at.timestamp_millis().into(),
                kept.reminded_through.map(|millis| millis.to_string()).into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    Ok(written.rows_affected() == 1)
}

/// The notification for one time of the reminder.
pub fn notification<Tz: TimeZone>(kept: &Kept, at: DateTime<Utc>, zone: &Tz) -> Notification
where
    Tz::Offset: std::fmt::Display,
{
    let when = at.with_timezone(zone).format("%Y-%m-%d %H:%M").to_string();
    let mut notification = Notification::new(
        kept.user_id,
        NotificationType::SystemInfo,
        NotificationPriority::High,
        kept.title.clone(),
        when,
    )
    .with_event(
        NotificationEventKey::ReminderDue,
        json!({ "action": ACTION_OPEN_AGENT, "reminderId": kept.key, "at": at.timestamp_millis() }),
    );
    // The same time of the same reminder is one notification.
    notification.id = format!("reminder_{}_{}", kept.id, at.timestamp());
    notification
}

/// One pass: every reminder due now is marked, then told.
pub async fn tick(db: DatabaseConnection) {
    let reminders = match active(&db).await {
        Ok(reminders) => reminders,
        Err(error) => {
            tracing::warn!(%error, "[reminders] could not read the reminders");
            return;
        }
    };
    if reminders.is_empty() {
        return;
    }
    let zone = super::merope::clock::zone();
    let now = crate::clock::now();
    for kept in reminders {
        let Some(at) = kept.due(now, &zone) else {
            continue;
        };
        match mark(&db, &kept, at).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                tracing::warn!(%error, reminder = kept.id, "[reminders] could not mark a reminder");
                continue;
            }
        }
        let told = match get_notification_manager() {
            Some(manager) => manager.notify(notification(&kept, at, &zone)).await,
            None => false,
        };
        tracing::info!(
            reminder = kept.id,
            user = kept.user_id,
            told,
            "[reminders] a reminder came due"
        );
    }
}
