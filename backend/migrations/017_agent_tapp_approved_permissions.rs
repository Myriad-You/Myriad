use myriad_tapp_contract::permission::TappPermission;
use sea_orm::{DatabaseBackend, Statement};
use sea_orm_migration::prelude::*;
use serde_json::{json, Value};

/// Agent installs implicitly approved all declarations. Skip malformed manifests
/// rather than destroying approvals; unknown names are discarded independently.
fn manifest_approvals(manifest: &Value) -> Option<Value> {
    let manifest = manifest.as_object()?;
    let Some(permissions) = manifest.get("permissions") else {
        return Some(json!([]));
    };
    Some(Value::Array(
        permissions
            .as_array()?
            .iter()
            .filter_map(|value| {
                let name = value.as_str()?;
                TappPermission::from_str(name).map(|_| json!(name))
            })
            .collect(),
    ))
}

async fn restore_agent_approvals(db: &impl ConnectionTrait) -> Result<u64, DbErr> {
    // Migrator holds a PostgreSQL transaction; lock rows until it commits so a
    // concurrent manifest update cannot be overwritten with stale approvals.
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, manifest, approved_permissions FROM tapps
         WHERE tapp_id LIKE 'agent.generated.%' OR tapp_id LIKE 'agent.installed.%'
         FOR UPDATE",
        ))
        .await?;
    let mut changed = 0;
    for row in rows {
        // A corrupt row must not prevent other Agent installations recovering.
        let (Ok(id), Ok(manifest), Ok(approved)) = (
            row.try_get::<i32>("", "id"),
            row.try_get::<Value>("", "manifest"),
            row.try_get::<Value>("", "approved_permissions"),
        ) else {
            continue;
        };
        let Some(target) = manifest_approvals(&manifest) else {
            continue;
        };
        if approved == target {
            continue;
        }
        changed += db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE tapps SET approved_permissions = $2::jsonb WHERE id = $1",
                vec![id.into(), target.to_string().into()],
            ))
            .await?
            .rows_affected();
    }
    Ok(changed)
}

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        restore_agent_approvals(manager.get_connection()).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The original role-filtered snapshot cannot be reconstructed.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_recognized_declarations_including_role_restricted_names() {
        assert_eq!(
            manifest_approvals(&json!({"permissions": [
                "storage:read", "unknown:permission", "platform:write", 42, "storage"
            ]})),
            Some(json!(["storage:read", "platform:write"]))
        );
    }

    #[test]
    fn malformed_manifest_is_skipped_and_absent_permissions_are_empty() {
        for manifest in [Value::Null, json!([]), json!({"permissions": {}})] {
            assert_eq!(manifest_approvals(&manifest), None);
        }
        assert_eq!(manifest_approvals(&json!({})), Some(json!([])));
        assert_eq!(
            manifest_approvals(&json!({"permissions": []})),
            Some(json!([]))
        );
    }
}

#[cfg(test)]
mod integration {
    use super::*;
    use sea_orm::{Database, TransactionTrait};

    #[tokio::test]
    #[ignore = "requires AGENT_APPROVAL_TEST_DATABASE_URL pointing to PostgreSQL"]
    async fn restores_only_agent_approvals_and_is_idempotent() {
        let url = std::env::var("AGENT_APPROVAL_TEST_DATABASE_URL")
            .expect("set AGENT_APPROVAL_TEST_DATABASE_URL to run this test");
        let db = Database::connect(url).await.unwrap();
        let tx = db.begin().await.unwrap();
        // A temporary table shadows any real tapps table, and rollback removes
        // all fixtures. Match the json/jsonb column types of migration 002.
        tx.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "CREATE TEMP TABLE tapps (id serial PRIMARY KEY, tapp_id text,
             manifest json, approved_permissions jsonb, granted_permissions json,
             needs_reauthorization boolean) ON COMMIT DROP",
        ))
        .await
        .unwrap();
        for (id, manifest, marker) in [
            (
                "agent.generated.one",
                json!({"permissions": ["storage:read", "platform:write", "unknown"]}),
                false,
            ),
            (
                "agent.installed.two",
                json!({"permissions": ["platform:write"]}),
                true,
            ),
            (
                "store.app",
                json!({"permissions": ["platform:write"]}),
                false,
            ),
            (
                "direct.app",
                json!({"permissions": ["platform:write"]}),
                false,
            ),
            ("agent.generated.bad", json!({"permissions": {}}), false),
            ("agent.installed.empty", json!({}), false),
        ] {
            tx.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO tapps (tapp_id, manifest, approved_permissions,
                 granted_permissions, needs_reauthorization) VALUES ($1, $2::json,
                 '[\"storage:read\"]'::jsonb, '[\"storage:read\"]'::json, $3)",
                vec![id.into(), manifest.to_string().into(), marker.into()],
            ))
            .await
            .unwrap();
        }
        assert_eq!(restore_agent_approvals(&tx).await.unwrap(), 3);
        assert_eq!(restore_agent_approvals(&tx).await.unwrap(), 0);
        Migration.down(&SchemaManager::new(&tx)).await.unwrap();
        let rows = tx.query_all_raw(Statement::from_string(DatabaseBackend::Postgres,
            "SELECT tapp_id, approved_permissions, granted_permissions, needs_reauthorization FROM tapps ORDER BY id"
        )).await.unwrap();
        let expected = [
            json!(["storage:read", "platform:write"]),
            json!(["platform:write"]),
            json!(["storage:read"]),
            json!(["storage:read"]),
            json!(["storage:read"]),
            json!([]),
        ];
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(
                row.try_get::<Value>("", "approved_permissions").unwrap(),
                expected[index]
            );
            assert_eq!(
                row.try_get::<Value>("", "granted_permissions").unwrap(),
                json!(["storage:read"])
            );
            assert_eq!(
                row.try_get::<bool>("", "needs_reauthorization").unwrap(),
                index == 1
            );
        }
        tx.rollback().await.unwrap();
    }
}
