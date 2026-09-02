use chrono::{DateTime, FixedOffset, Utc};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};

use crate::models::entities::agent_intentions;

use super::{AcceptSource, IntentRecord, IntentStatus, RecentIntent, WorkProposal};

#[derive(Clone)]
pub struct IntentStore {
    db: DatabaseConnection,
}

impl IntentStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn create_proposed(&self, record: IntentRecord) -> Result<IntentRecord, DbErr> {
        if record.status != IntentStatus::Proposed {
            return Err(DbErr::Custom(
                "new consciousness intent must start as proposed".into(),
            ));
        }

        let model = agent_intentions::ActiveModel {
            id: Set(record.id.clone()),
            user_id: Set(record.user_id),
            source_event_id: Set(record.source_event_id.clone()),
            summary: Set(record.summary.clone()),
            reason_code: Set(record.reason_code.clone()),
            status: Set(record.status.as_str().into()),
            proposal: Set(serde_json::to_value(&record.proposal).map_err(json_error)?),
            work_session_id: Set(record.work_session_id.clone()),
            work_run_id: Set(record.work_run_id.clone()),
            result_summary: Set(record.result_summary.clone()),
            expires_at: Set(record.expires_at.map(to_fixed)),
            accept_source: Set(record.accept_source.as_str().into()),
            created_at: Set(to_fixed(record.created_at)),
            updated_at: Set(to_fixed(record.updated_at)),
        };
        agent_intentions::Entity::insert(model)
            .exec(&self.db)
            .await?;
        Ok(record)
    }

    /// Proposed → Accepted with an explicit source. User-click and autonomy
    /// skip-review must not share an unmarked Accepted row.
    pub async fn mark_accepted(
        &self,
        intent_id: &str,
        user_id: i32,
        source: AcceptSource,
    ) -> Result<IntentRecord, DbErr> {
        let current = agent_intentions::Entity::find_by_id(intent_id)
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("consciousness intent not found".into()))?;
        let current_status = parse_status(&current.status)?;
        if current_status == IntentStatus::Accepted {
            let mut record = model_to_record(current)?;
            if record.accept_source == AcceptSource::Autonomy && source == AcceptSource::User {
                let update = agent_intentions::ActiveModel {
                    accept_source: Set(source.as_str().into()),
                    updated_at: Set(to_fixed(Utc::now())),
                    ..Default::default()
                };
                agent_intentions::Entity::update_many()
                    .set(update)
                    .filter(agent_intentions::Column::Id.eq(intent_id))
                    .filter(agent_intentions::Column::UserId.eq(user_id))
                    .exec(&self.db)
                    .await?;
                record.accept_source = source;
            }
            return Ok(record);
        }
        if !current_status.can_transition_to(IntentStatus::Accepted) {
            return Err(DbErr::Custom(format!(
                "invalid consciousness intent transition: {} -> accepted",
                current_status.as_str()
            )));
        }
        let update = agent_intentions::ActiveModel {
            status: Set(IntentStatus::Accepted.as_str().into()),
            accept_source: Set(source.as_str().into()),
            updated_at: Set(to_fixed(Utc::now())),
            ..Default::default()
        };
        let result = agent_intentions::Entity::update_many()
            .set(update)
            .filter(agent_intentions::Column::Id.eq(intent_id))
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .filter(agent_intentions::Column::Status.eq(current_status.as_str()))
            .exec(&self.db)
            .await?;
        if result.rows_affected != 1 {
            return Err(DbErr::Custom(
                "consciousness intent changed concurrently".into(),
            ));
        }
        self.find(intent_id, user_id).await
    }

    pub async fn list_autonomy_accepted(&self, limit: u64) -> Result<Vec<IntentRecord>, DbErr> {
        let models = agent_intentions::Entity::find()
            .filter(agent_intentions::Column::Status.eq(IntentStatus::Accepted.as_str()))
            .filter(agent_intentions::Column::AcceptSource.eq(AcceptSource::Autonomy.as_str()))
            .order_by_asc(agent_intentions::Column::UpdatedAt)
            .limit(limit.min(8))
            .all(&self.db)
            .await?;
        models.into_iter().map(model_to_record).collect()
    }

    pub async fn latest_work_source_event(&self, user_id: i32) -> Result<Option<String>, DbErr> {
        let model = agent_intentions::Entity::find()
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .filter(agent_intentions::Column::Status.is_in([
                IntentStatus::Running.as_str(),
                IntentStatus::Waiting.as_str(),
                IntentStatus::Completed.as_str(),
            ]))
            .order_by_desc(agent_intentions::Column::UpdatedAt)
            .one(&self.db)
            .await?;
        Ok(model.map(|row| row.source_event_id))
    }

    /// Compare-and-set a transition so concurrent event workers cannot advance
    /// the same intention twice.
    pub async fn transition(
        &self,
        intent_id: &str,
        user_id: i32,
        next: IntentStatus,
        work_session_id: Option<String>,
        work_run_id: Option<String>,
        result_summary: Option<String>,
    ) -> Result<IntentRecord, DbErr> {
        let current = agent_intentions::Entity::find_by_id(intent_id)
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("consciousness intent not found".into()))?;
        let current_status = parse_status(&current.status)?;
        if current_status.is_terminal() || !current_status.can_transition_to(next) {
            return Err(DbErr::Custom(format!(
                "invalid consciousness intent transition: {} -> {}",
                current_status.as_str(),
                next.as_str()
            )));
        }

        let update = agent_intentions::ActiveModel {
            status: Set(next.as_str().into()),
            work_session_id: Set(work_session_id.or(current.work_session_id.clone())),
            work_run_id: Set(work_run_id.or(current.work_run_id.clone())),
            result_summary: Set(result_summary.or(current.result_summary.clone())),
            updated_at: Set(to_fixed(Utc::now())),
            ..Default::default()
        };
        let result = agent_intentions::Entity::update_many()
            .set(update)
            .filter(agent_intentions::Column::Id.eq(intent_id))
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .filter(agent_intentions::Column::Status.eq(current_status.as_str()))
            .exec(&self.db)
            .await?;
        if result.rows_affected != 1 {
            return Err(DbErr::Custom(
                "consciousness intent changed concurrently".into(),
            ));
        }

        let updated = agent_intentions::Entity::find_by_id(intent_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| {
                DbErr::RecordNotFound("updated consciousness intent not found".into())
            })?;
        model_to_record(updated)
    }

    pub async fn recent(&self, user_id: i32, limit: u64) -> Result<Vec<RecentIntent>, DbErr> {
        let models = agent_intentions::Entity::find()
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .order_by_desc(agent_intentions::Column::UpdatedAt)
            .limit(limit.min(20))
            .all(&self.db)
            .await?;
        models
            .into_iter()
            .map(|model| {
                Ok(RecentIntent {
                    id: model.id,
                    summary: model.summary,
                    status: parse_status(&model.status)?,
                    updated_at: model.updated_at.with_timezone(&Utc),
                })
            })
            .collect()
    }

    /// Proposals that still need a user action. `Accepted` remains actionable
    /// until Work has actually claimed it, so a reload between the two calls
    /// cannot strand the proposal.
    pub async fn actionable(&self, user_id: i32, limit: u64) -> Result<Vec<IntentRecord>, DbErr> {
        self.expire_stale(user_id).await?;
        let models = agent_intentions::Entity::find()
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .filter(agent_intentions::Column::Status.is_in([
                IntentStatus::Proposed.as_str(),
                IntentStatus::Accepted.as_str(),
            ]))
            .order_by_desc(agent_intentions::Column::UpdatedAt)
            .limit(limit.min(20))
            .all(&self.db)
            .await?;
        models.into_iter().map(model_to_record).collect()
    }

    pub async fn find(&self, intent_id: &str, user_id: i32) -> Result<IntentRecord, DbErr> {
        let model = agent_intentions::Entity::find_by_id(intent_id)
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("consciousness intent not found".into()))?;
        model_to_record(model)
    }

    pub async fn find_by_source_event(
        &self,
        user_id: i32,
        source_event_id: &str,
    ) -> Result<Option<IntentRecord>, DbErr> {
        let model = agent_intentions::Entity::find()
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .filter(agent_intentions::Column::SourceEventId.eq(source_event_id))
            .one(&self.db)
            .await?;
        model.map(model_to_record).transpose()
    }

    pub async fn expire_stale(&self, user_id: i32) -> Result<u64, DbErr> {
        let update = agent_intentions::ActiveModel {
            status: Set(IntentStatus::Expired.as_str().into()),
            updated_at: Set(to_fixed(Utc::now())),
            ..Default::default()
        };
        let mut query = agent_intentions::Entity::update_many()
            .set(update)
            .filter(agent_intentions::Column::Status.is_in([
                IntentStatus::Proposed.as_str(),
                IntentStatus::Accepted.as_str(),
            ]))
            .filter(agent_intentions::Column::ExpiresAt.lte(to_fixed(Utc::now())));
        if user_id != 0 {
            query = query.filter(agent_intentions::Column::UserId.eq(user_id));
        }
        let result = query.exec(&self.db).await?;
        Ok(result.rows_affected)
    }

    /// Expire Proposed and Accepted rows whose `expires_at` has passed.
    pub async fn expire_stale_global(&self) -> Result<u64, DbErr> {
        self.expire_stale(0).await
    }

    pub async fn list_running(&self, limit: u64) -> Result<Vec<IntentRecord>, DbErr> {
        let models = agent_intentions::Entity::find()
            .filter(agent_intentions::Column::Status.eq(IntentStatus::Running.as_str()))
            .order_by_asc(agent_intentions::Column::UpdatedAt)
            .limit(limit.min(32))
            .all(&self.db)
            .await?;
        models.into_iter().map(model_to_record).collect()
    }

    /// Crash recovery: Running with no persisted wait-loop goes back to Accepted
    /// so the autonomy tick can claim it again. User-accepted Running is left
    /// for the proposal card (`accept_source` stays).
    pub async fn reclaim_running_to_accepted(
        &self,
        intent_id: &str,
        user_id: i32,
    ) -> Result<bool, DbErr> {
        let update = agent_intentions::ActiveModel {
            status: Set(IntentStatus::Accepted.as_str().into()),
            work_session_id: Set(None),
            work_run_id: Set(None),
            updated_at: Set(to_fixed(Utc::now())),
            ..Default::default()
        };
        let result = agent_intentions::Entity::update_many()
            .set(update)
            .filter(agent_intentions::Column::Id.eq(intent_id))
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .filter(agent_intentions::Column::Status.eq(IntentStatus::Running.as_str()))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected == 1)
    }

    /// Find the durable proposal attached to a boot-restored Work session.
    pub async fn recoverable_for_session(
        &self,
        user_id: i32,
        session_id: &str,
    ) -> Result<Option<IntentRecord>, DbErr> {
        let model = agent_intentions::Entity::find()
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .filter(agent_intentions::Column::WorkSessionId.eq(session_id))
            .filter(agent_intentions::Column::Status.is_in([
                IntentStatus::Running.as_str(),
                IntentStatus::Waiting.as_str(),
            ]))
            .order_by_desc(agent_intentions::Column::UpdatedAt)
            .one(&self.db)
            .await?;
        model.map(model_to_record).transpose()
    }

    /// Refresh the run identity after process restart without changing the
    /// lifecycle state. Only an already-running/waiting Work item can reattach.
    pub async fn reattach_work(
        &self,
        intent_id: &str,
        user_id: i32,
        session_id: String,
        run_id: String,
    ) -> Result<(), DbErr> {
        let update = agent_intentions::ActiveModel {
            work_session_id: Set(Some(session_id)),
            work_run_id: Set(Some(run_id)),
            updated_at: Set(to_fixed(Utc::now())),
            ..Default::default()
        };
        let result = agent_intentions::Entity::update_many()
            .set(update)
            .filter(agent_intentions::Column::Id.eq(intent_id))
            .filter(agent_intentions::Column::UserId.eq(user_id))
            .filter(agent_intentions::Column::Status.is_in([
                IntentStatus::Running.as_str(),
                IntentStatus::Waiting.as_str(),
            ]))
            .exec(&self.db)
            .await?;
        if result.rows_affected != 1 {
            return Err(DbErr::RecordNotFound(
                "recoverable consciousness intent not found".into(),
            ));
        }
        Ok(())
    }
}

fn model_to_record(model: agent_intentions::Model) -> Result<IntentRecord, DbErr> {
    Ok(IntentRecord {
        id: model.id,
        user_id: model.user_id,
        source_event_id: model.source_event_id,
        summary: model.summary,
        reason_code: model.reason_code,
        status: parse_status(&model.status)?,
        proposal: serde_json::from_value::<WorkProposal>(model.proposal).map_err(json_error)?,
        work_session_id: model.work_session_id,
        work_run_id: model.work_run_id,
        result_summary: model.result_summary,
        created_at: model.created_at.with_timezone(&Utc),
        updated_at: model.updated_at.with_timezone(&Utc),
        expires_at: model.expires_at.map(|value| value.with_timezone(&Utc)),
        accept_source: AcceptSource::from_str(&model.accept_source),
    })
}

fn parse_status(value: &str) -> Result<IntentStatus, DbErr> {
    IntentStatus::from_str(value)
        .ok_or_else(|| DbErr::Type(format!("unknown consciousness intent status: {value}")))
}

fn json_error(error: serde_json::Error) -> DbErr {
    DbErr::Json(error.to_string())
}

fn to_fixed(value: DateTime<Utc>) -> DateTime<FixedOffset> {
    value.into()
}

#[cfg(test)]
mod ledger_db_tests {
    use super::*;
    use crate::services::agent::consciousness::{AcceptSource, IntentStatus, WorkProposal};
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};

    async fn connect() -> Option<(DatabaseConnection, i32)> {
        let url = std::env::var("AGENT_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("NOTIFICATION_TEST_DATABASE_URL"))
            .or_else(|_| std::env::var("MYRIAD_SCHEMA_DRIFT_DB"))
            .ok()?;
        let db = Database::connect(url).await.ok()?;
        migration::Migrator::up(&db, None).await.ok()?;
        let _ = crate::db::schema_check::ensure_schema(&db).await;
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ($1) RETURNING id",
                [format!("autonomy-ledger-{}", uuid::Uuid::new_v4().simple()).into()],
            ))
            .await
            .ok()
            .flatten()?;
        let user_id = row.try_get::<i32>("", "id").ok()?;
        Some((db, user_id))
    }

    fn proposal(source: &str) -> WorkProposal {
        WorkProposal {
            title: "整理报告".into(),
            instruction: "整理报告".into(),
            expected_outcome: "一份摘要".into(),
            source_event_id: source.into(),
        }
    }

    fn record(user_id: i32, source: &str, status: IntentStatus) -> IntentRecord {
        let now = Utc::now();
        IntentRecord {
            id: format!("int_test_{}", uuid::Uuid::new_v4().simple()),
            user_id,
            source_event_id: source.into(),
            summary: "整理报告".into(),
            reason_code: "test".into(),
            status,
            proposal: proposal(source),
            work_session_id: None,
            work_run_id: None,
            result_summary: None,
            created_at: now,
            updated_at: now,
            expires_at: Some(now + chrono::Duration::hours(1)),
            accept_source: AcceptSource::Autonomy,
        }
    }

    #[tokio::test]
    async fn unique_source_event_and_cas_claim_and_reclaim() {
        let Some((db, user_id)) = connect().await else {
            return;
        };
        let store = IntentStore::new(db.clone());
        let source = format!("evt_test_{}", uuid::Uuid::new_v4().simple());
        let mut first = record(user_id, &source, IntentStatus::Proposed);
        first = store.create_proposed(first).await.expect("insert proposed");
        let mut dup = record(user_id, &source, IntentStatus::Proposed);
        dup.id = format!("int_test_{}", uuid::Uuid::new_v4().simple());
        assert!(store.create_proposed(dup).await.is_err());

        first = store
            .mark_accepted(&first.id, user_id, AcceptSource::Autonomy)
            .await
            .expect("accept");
        let listed = store
            .list_autonomy_accepted(8)
            .await
            .expect("list autonomy");
        assert!(listed.iter().any(|row| row.id == first.id));

        let store_a = store.clone();
        let store_b = store.clone();
        let first_id = first.id.clone();
        let (left, right) = tokio::join!(
            store_a.transition(&first_id, user_id, IntentStatus::Running, None, None, None),
            store_b.transition(&first_id, user_id, IntentStatus::Running, None, None, None),
        );
        assert!(
            left.is_ok() ^ right.is_ok(),
            "exactly one concurrent claim may enter Running"
        );
        let claimed = left.or(right).expect("winner");
        assert_eq!(claimed.status, IntentStatus::Running);

        store
            .reattach_work(&first.id, user_id, "ses_empty".into(), "run_empty".into())
            .await
            .expect("attach");
        assert!(store
            .reclaim_running_to_accepted(&first.id, user_id)
            .await
            .expect("reclaim"));
        let reclaimed = store.find(&first.id, user_id).await.expect("reload");
        assert_eq!(reclaimed.status, IntentStatus::Accepted);
        assert_eq!(reclaimed.accept_source, AcceptSource::Autonomy);
        assert_eq!(reclaimed.work_session_id, None);
        assert_eq!(reclaimed.work_run_id, None);

        let mut user_accepted = record(
            user_id,
            &format!("evt_user_{}", uuid::Uuid::new_v4().simple()),
            IntentStatus::Proposed,
        );
        user_accepted.accept_source = AcceptSource::User;
        user_accepted = store
            .create_proposed(user_accepted)
            .await
            .expect("user proposed");
        user_accepted = store
            .mark_accepted(&user_accepted.id, user_id, AcceptSource::User)
            .await
            .expect("user accept");
        let autonomy_only = store
            .list_autonomy_accepted(8)
            .await
            .expect("list after user accept");
        assert!(!autonomy_only.iter().any(|row| row.id == user_accepted.id));
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM agent_intentions WHERE id = $1",
                [user_accepted.id.clone().into()],
            ))
            .await;

        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM agent_intentions WHERE id = $1",
                [first.id.clone().into()],
            ))
            .await;
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM users WHERE id = $1",
                [user_id.into()],
            ))
            .await;
    }

    #[tokio::test]
    async fn accepted_rows_expire_by_expires_at() {
        let Some((db, user_id)) = connect().await else {
            return;
        };
        let store = IntentStore::new(db.clone());
        let source = format!("evt_test_{}", uuid::Uuid::new_v4().simple());
        let mut row = record(user_id, &source, IntentStatus::Proposed);
        row.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        row = store.create_proposed(row).await.expect("insert");
        row = store
            .mark_accepted(&row.id, user_id, AcceptSource::Autonomy)
            .await
            .expect("accept");
        let expired = store.expire_stale(user_id).await.expect("expire");
        assert!(expired >= 1);
        let found = store.find(&row.id, user_id).await.expect("reload");
        assert_eq!(found.status, IntentStatus::Expired);
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM agent_intentions WHERE id = $1",
                [row.id.into()],
            ))
            .await;
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM users WHERE id = $1",
                [user_id.into()],
            ))
            .await;
    }
}
