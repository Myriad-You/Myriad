use chrono::{DateTime, FixedOffset, Utc};
use sea_orm::{ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter};

use crate::models::entities::agent_autonomy_grants;

use super::grant::AutonomyGrantView;

#[derive(Clone)]
pub struct AutonomyGrantStore {
    db: DatabaseConnection,
}

impl AutonomyGrantStore {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn find(&self, user_id: i32) -> Result<Option<AutonomyGrantView>, DbErr> {
        let Some(model) = agent_autonomy_grants::Entity::find()
            .filter(agent_autonomy_grants::Column::UserId.eq(user_id))
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(model_to_view(model)))
    }

    pub async fn upsert(&self, grant: &AutonomyGrantView) -> Result<AutonomyGrantView, DbErr> {
        let now = to_fixed(Utc::now());
        let permissions = serde_json::json!(grant.allowed_permissions);
        if self.find(grant.user_id).await?.is_some() {
            let update = agent_autonomy_grants::ActiveModel {
                allowed_permissions: Set(permissions),
                revoked: Set(grant.revoked),
                updated_at: Set(now),
                ..Default::default()
            };
            agent_autonomy_grants::Entity::update_many()
                .set(update)
                .filter(agent_autonomy_grants::Column::UserId.eq(grant.user_id))
                .exec(&self.db)
                .await?;
        } else {
            let insert = agent_autonomy_grants::ActiveModel {
                user_id: Set(grant.user_id),
                allowed_permissions: Set(permissions),
                revoked: Set(grant.revoked),
                created_at: Set(now),
                updated_at: Set(now),
            };
            agent_autonomy_grants::Entity::insert(insert)
                .exec(&self.db)
                .await?;
        }
        self.find(grant.user_id)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("personal autonomy grant not found".into()))
    }
}

fn model_to_view(model: agent_autonomy_grants::Model) -> AutonomyGrantView {
    let allowed_permissions = match model.allowed_permissions {
        serde_json::Value::Array(values) => values
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    AutonomyGrantView {
        user_id: model.user_id,
        allowed_permissions,
        revoked: model.revoked,
    }
}

fn to_fixed(value: DateTime<Utc>) -> DateTime<FixedOffset> {
    value.into()
}
