use sea_orm_migration::prelude::*;

/// Retired coarse permission strings removed from the permission enum.
///
/// On upgrade, strip these names out of the two permission columns of every
/// installed TAPP row (`approved_permissions` and `granted_permissions`) and
/// durably mark the affected installation as needing re-authorization. There
/// is no old-to-new permission mapping: the operator reviews and re-authorizes
/// the installation with current permission names.
pub const RETIRED_PERMISSIONS: [&str; 3] = ["storage", "brew:comment", "federation:write"];

/// Remove every retired permission string from one permission array,
/// preserving the relative order of the remaining elements.
///
/// Explicit contract (mirrors the migration invariants):
/// - Exact string match only — `"storage:read"`, `"media:read"`, … are kept.
/// - Non-string array elements are preserved untouched.
/// - Non-array values (objects, scalars, null, malformed JSON) are returned
///   **unchanged**: the migration never silently replaces malformed data with
///   an empty array. Rows whose values are not arrays are simply left alone
///   and are never marked by this migration.
/// - Idempotent: applying it twice returns the same value.
pub fn clean_legacy_permission_array(value: &serde_json::Value) -> serde_json::Value {
    let Some(elements) = value.as_array() else {
        return value.clone();
    };
    serde_json::Value::Array(
        elements
            .iter()
            .filter(|element| {
                !(element.is_string()
                    && RETIRED_PERMISSIONS.contains(&element.as_str().expect("checked above")))
            })
            .cloned()
            .collect(),
    )
}

/// Clean both permission columns of one install row.
///
/// Returns the cleaned values plus whether anything was removed from either
/// column. Rows with non-array values report `affected == false` and keep the
/// original value (never overwritten with an empty array).
pub fn clean_permission_columns(
    approved_permissions: &serde_json::Value,
    granted_permissions: &serde_json::Value,
) -> (serde_json::Value, serde_json::Value, bool) {
    let clean_approved = clean_legacy_permission_array(approved_permissions);
    let clean_granted = clean_legacy_permission_array(granted_permissions);
    let affected = clean_approved != *approved_permissions || clean_granted != *granted_permissions;
    (clean_approved, clean_granted, affected)
}

fn array_contains_retired(value: &serde_json::Value) -> bool {
    value.as_array().is_some_and(|elements| {
        elements.iter().any(|element| {
            element
                .as_str()
                .is_some_and(|text| RETIRED_PERMISSIONS.contains(&text))
        })
    })
}

/// 候选行判定：任一权限列是 JSON 数组且含有至少一个退役串。
///
/// 与 `clear_legacy_grants` 的 SQL `FOR UPDATE` 锁集谓词语义一致（两边都是
/// 精确集，非超集）：非数组/畸形值永不进入候选集，也就永不被锁、不被写。
pub fn is_legacy_permission_candidate(
    approved_permissions: &serde_json::Value,
    granted_permissions: &serde_json::Value,
) -> bool {
    array_contains_retired(approved_permissions) || array_contains_retired(granted_permissions)
}

/// 校验 UPDATE 恰好命中 1 行。
///
/// 0 行说明候选行在锁定期间被并发删除或改写（锁定后不应发生）；2+ 行说明主键
/// 异常。两种情况都必须显式失败，不允许静默继续。
pub fn ensure_single_row_updated(id: i32, rows_affected: u64) -> Result<(), DbErr> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(DbErr::Custom(format!(
            "legacy permission migration: expected UPDATE of tapps.id={id} to affect exactly 1 row, got {rows_affected}"
        )))
    }
}

/// 退役权限串作为 SQL 绑定参数。
///
/// 三个退役串只存在于 `RETIRED_PERMISSIONS` 一处（单一来源），SQL 谓词
/// 通过 `ARRAY[$1..$3]::text[]` 绑定，不再内联字面量——从根上消除 Rust 常量
/// 与 SQL 字面量的双源漂移。
pub fn retired_permission_params() -> Vec<sea_orm::Value> {
    RETIRED_PERMISSIONS
        .iter()
        .map(|permission| String::from(*permission).into())
        .collect()
}

/// Clear retired permission strings from every installed TAPP row and
/// flag the affected rows for re-authorization.
///
/// - Affected rows are determined from the **pre-cleanup** values; both
///   columns are cleaned independently, unrelated permissions and their
///   relative order are preserved, and `needs_reauthorization` is set to
///   `true` for every affected row.
/// - Rows whose permission values are not JSON arrays are left byte-for-byte
///   untouched and are never marked (malformed data is never overwritten).
/// - Candidate rows are locked `FOR UPDATE` inside the migration transaction
///   (SeaORM Migrator wraps each migration in a transaction on PostgreSQL), so
///   a concurrent install/update re-approval cannot be overwritten by stale
///   SELECT values; every UPDATE must affect exactly one row.
/// - Idempotent in effect: a second run finds no retired strings and changes
///   nothing, so an interrupted migration converges on retry.
/// - Never clears the marker — only the explicit install/update/re-approval
///   path may do that.
pub async fn clear_legacy_grants(db: &impl ConnectionTrait) -> Result<u64, DbErr> {
    use sea_orm::Statement;

    // 锁集 = 候选行（含退役串的数组列）。谓词与 is_legacy_permission_candidate
    // 精确一致：jsonb_typeof 守卫排除非数组值，?| 检查数组元素字符串存在性。
    // 注意列类型：approved_permissions 是 jsonb，granted_permissions 在 002 中
    // 真实类型是 json（jsonb_typeof / ?| 只接受 jsonb，故对 granted_permissions
    // 显式 ::jsonb）。schema_check 声称 granted_permissions 为 jsonb 是既有
    // 不一致（L6），本单元不擅自改列类型，仅在谓词内显式转换。
    // FOR UPDATE 持有到 migration 事务提交，串行化并发安装/更新对同一行的写入。
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            r#"SELECT id, approved_permissions, granted_permissions
                 FROM tapps
                WHERE (jsonb_typeof(approved_permissions) = 'array'
                       AND approved_permissions ?| ARRAY[$1,$2,$3]::text[])
                   OR (jsonb_typeof(granted_permissions::jsonb) = 'array'
                       AND granted_permissions::jsonb ?| ARRAY[$1,$2,$3]::text[])
                  FOR UPDATE"#,
            retired_permission_params(),
        ))
        .await?;

    let mut changed = 0u64;
    for row in rows {
        let id: i32 = row.try_get::<i32>("", "id")?;
        let approved: serde_json::Value =
            row.try_get::<serde_json::Value>("", "approved_permissions")?;
        let granted: serde_json::Value =
            row.try_get::<serde_json::Value>("", "granted_permissions")?;
        // 防御性校验（测试/开发构建下真实执行）：SQL FOR UPDATE 锁集谓词必须
        // 与 Rust 候选判定一致——锁集里的每一行都应是候选行。
        debug_assert!(
            is_legacy_permission_candidate(&approved, &granted),
            "legacy permission migration: SELECT ... FOR UPDATE lock-set predicate drifted from the Rust candidate check (tapps.id={id})"
        );
        let (clean_approved, clean_granted, affected) =
            clean_permission_columns(&approved, &granted);
        if !affected {
            continue;
        }
        let result = db
            .execute_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "UPDATE tapps
                SET approved_permissions = $2::jsonb,
                    granted_permissions = $3::json,
                    needs_reauthorization = true
              WHERE id = $1",
                vec![
                    id.into(),
                    clean_approved.to_string().into(),
                    clean_granted.to_string().into(),
                ],
            ))
            .await?;
        ensure_single_row_updated(id, result.rows_affected())?;
        changed += 1;
    }
    Ok(changed)
}

/// Remove legacy permission strings from installed TAPP rows and flag affected
/// installations as needing re-authorization.
///
/// The column is also part of the greenfield schema (002) and the runtime
/// schema-check source of truth; `ADD COLUMN IF NOT EXISTS` keeps already-migrated
/// databases consistent before the data cleanup runs.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE tapps
                    ADD COLUMN IF NOT EXISTS needs_reauthorization BOOLEAN NOT NULL DEFAULT false",
            )
            .await?;
        let _ = clear_legacy_grants(manager.get_connection()).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 有意 no-op：数据清理不可逆，回滚不得删除持久信号。
        //
        // 被移除的退役权限串无法恢复（不做旧→新映射），而
        // needs_reauthorization 列与已落库的标记是「显式清除 + 标记 + 升级说明」
        // 三件套里唯一能在回滚后继续防止静默降权的持久信号——删除它会让受影响
        // 安装带着已清空的权限列继续运行。列本身同时属于 greenfield schema
        // （002）与 schema_check 源，保留它与两者保持一致。
        let _ = manager;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_retired_name_is_removed() {
        for retired in RETIRED_PERMISSIONS {
            let cleaned = clean_legacy_permission_array(&json!([retired]));
            assert_eq!(cleaned, json!([]), "retired name {retired} must be removed");
        }
    }

    #[test]
    fn unrelated_permissions_are_preserved_in_order() {
        let value = json!(["storage:read", "network:fetch", "ai:generate", "brew:read"]);
        let cleaned = clean_legacy_permission_array(&value);
        assert_eq!(cleaned, value, "unaffected arrays stay byte-for-byte equal");
    }

    #[test]
    fn exact_match_only_prefix_and_suffix_are_kept() {
        let value = json!(["storage", "storage:read", "storage:write", "ui:theme:read"]);
        assert_eq!(
            clean_legacy_permission_array(&value),
            json!(["storage:read", "storage:write", "ui:theme:read"])
        );
    }

    #[test]
    fn mixed_old_and_current_permissions_keep_current_in_order() {
        let value = json!([
            "storage",
            "media:control",
            "storage:read",
            "federation:write",
            "federation:post"
        ]);
        assert_eq!(
            clean_legacy_permission_array(&value),
            json!(["media:control", "storage:read", "federation:post"])
        );
    }

    #[test]
    fn retired_names_are_removed_while_retained_coarse_permissions_stay() {
        let value = json!([
            "brew:write",
            "brew:comment",
            "ui:theme",
            "federation:write",
            "storage",
            "media:control"
        ]);
        assert_eq!(
            clean_legacy_permission_array(&value),
            json!(["brew:write", "ui:theme", "media:control"])
        );
    }

    #[test]
    fn retained_theme_media_and_brew_write_do_not_mark_installation() {
        let retained = json!(["ui:theme", "media:control", "brew:write"]);
        let (clean_approved, clean_granted, affected) =
            clean_permission_columns(&retained, &retained);
        assert_eq!(clean_approved, retained);
        assert_eq!(clean_granted, retained);
        assert!(!affected);
        assert!(!is_legacy_permission_candidate(&retained, &retained));
    }

    #[test]
    fn empty_array_stays_empty() {
        assert_eq!(clean_legacy_permission_array(&json!([])), json!([]));
    }

    #[test]
    fn repeated_execution_is_idempotent() {
        let value = json!(["storage", "storage:read", "media:control"]);
        let once = clean_legacy_permission_array(&value);
        let twice = clean_legacy_permission_array(&once);
        assert_eq!(once, json!(["storage:read", "media:control"]));
        assert_eq!(twice, once);
    }

    #[test]
    fn non_string_elements_are_preserved() {
        let value = json!(["storage", 42, {"name": "storage"}]);
        assert_eq!(
            clean_legacy_permission_array(&value),
            json!([42, {"name": "storage"}])
        );
    }

    #[test]
    fn malformed_non_array_values_are_never_overwritten() {
        for malformed in [
            json!({"storage": true}),
            json!("storage"),
            json!(null),
            json!(7),
        ] {
            let cleaned = clean_legacy_permission_array(&malformed);
            assert_eq!(
                cleaned, malformed,
                "non-array value must stay byte-for-byte unchanged"
            );
        }
    }

    #[test]
    fn row_affected_in_only_one_column_marks_affected() {
        let approved = json!(["storage:read"]);
        let granted = json!(["storage", "storage:read"]);
        let (clean_approved, clean_granted, affected) =
            clean_permission_columns(&approved, &granted);
        assert!(affected);
        assert_eq!(clean_approved, approved, "clean column stays untouched");
        assert_eq!(clean_granted, json!(["storage:read"]));
    }

    #[test]
    fn unaffected_row_is_not_flagged() {
        let value = json!(["storage:read", "media:read"]);
        let (clean_approved, clean_granted, affected) = clean_permission_columns(&value, &value);
        assert!(!affected);
        assert_eq!(clean_approved, value);
        assert_eq!(clean_granted, value);
    }

    #[test]
    fn malformed_row_is_not_flagged() {
        let malformed = json!({"storage": true});
        let (clean_approved, clean_granted, affected) =
            clean_permission_columns(&malformed, &malformed);
        assert!(!affected);
        assert_eq!(clean_approved, malformed);
        assert_eq!(clean_granted, malformed);
    }

    #[test]
    fn re_approved_row_is_not_reflagged_by_cleanup() {
        // Simulate a successful re-approval: marker cleared and permissions
        // already current. The cleanup must not re-flag such a row.
        let current = json!(["storage:read"]);
        let (clean_approved, clean_granted, affected) =
            clean_permission_columns(&current, &current);
        assert!(!affected, "re-approved row has nothing left to clean");
        assert_eq!(clean_approved, current);
        assert_eq!(clean_granted, current);
    }

    #[test]
    fn candidate_predicate_matches_sql_lock_set_semantics() {
        // Array containing a retired string (either column) is a candidate.
        assert!(is_legacy_permission_candidate(
            &json!(["storage"]),
            &json!([])
        ));
        assert!(is_legacy_permission_candidate(
            &json!([]),
            &json!(["brew:comment"])
        ));
        // Current-only permissions never qualify.
        assert!(!is_legacy_permission_candidate(
            &json!(["storage:read", "media:read"]),
            &json!(["storage:read"])
        ));
        // Non-array / malformed values never qualify (jsonb_typeof guard in SQL).
        assert!(!is_legacy_permission_candidate(
            &json!({"storage": true}),
            &json!([])
        ));
        assert!(!is_legacy_permission_candidate(
            &json!("storage"),
            &json!([])
        ));
        assert!(!is_legacy_permission_candidate(&json!(null), &json!([])));
        // Empty arrays never qualify.
        assert!(!is_legacy_permission_candidate(&json!([]), &json!([])));
        // Retained coarse permissions do not qualify.
        assert!(!is_legacy_permission_candidate(
            &json!(["storage:read", "media:control"]),
            &json!([])
        ));
    }

    #[test]
    fn update_must_affect_exactly_one_row() {
        assert!(ensure_single_row_updated(7, 1).is_ok());
        let zero = ensure_single_row_updated(7, 0).unwrap_err().to_string();
        assert!(
            zero.contains("tapps.id=7"),
            "zero rows must fail with the row id: {zero}"
        );
        assert!(
            ensure_single_row_updated(7, 2).is_err(),
            "2+ rows is a primary-key anomaly"
        );
        assert!(ensure_single_row_updated(7, 3).is_err());
    }

    #[test]
    fn sql_bind_params_are_single_sourced_from_the_constant() {
        // The SQL predicate binds exactly the three retired names as text[]
        // elements — no inlined literal can drift from RETIRED_PERMISSIONS.
        let params = retired_permission_params();
        assert_eq!(params.len(), RETIRED_PERMISSIONS.len());
        for (param, name) in params.iter().zip(RETIRED_PERMISSIONS) {
            let sea_orm::Value::String(Some(text)) = param else {
                panic!("param must bind a String, got {param:?}");
            };
            assert_eq!(text, name);
        }
    }
}

/// End-to-end migration test against a real PostgreSQL database.
///
/// 显式 `#[ignore]`：默认 `cargo test -p migration` 只报告 ignored，不会静默
/// early-return。显式运行（`-- --ignored`）时缺 `TAPP_LEGACY_GRANT_TEST_DATABASE_URL`
/// 必须失败并给出命令。测试应用完整迁移套件、种子升级前 fixture、
/// 重跑清理证明幂等、验证 down() 不回滚持久信号、并验证模拟重新授权不被重新标记。
#[cfg(test)]
mod integration {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};

    const TEST_OWNER_USER_ID: i32 = 999999;

    async fn insert_fixture(
        db: &sea_orm::DatabaseConnection,
        tapp_id: &str,
        approved: &str,
        granted: &str,
    ) {
        db.execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            r#"INSERT INTO tapps
                (tapp_id, user_id, name, version, manifest, status,
                 granted_permissions, approved_permissions, needs_reauthorization,
                 file_path, code_path, installed_at, updated_at, visibility)
               VALUES ($1, $2, 'legacy-permission-fixture', '1.0.0', '{}'::jsonb, 'installed',
                       $3::jsonb, $4::jsonb, false,
                       '/tmp/manifest.json', '/tmp/main.js', NOW(), NOW(), 'all')"#,
            vec![
                tapp_id.into(),
                TEST_OWNER_USER_ID.into(),
                granted.into(),
                approved.into(),
            ],
        ))
        .await
        .unwrap();
    }

    async fn read_row(
        db: &sea_orm::DatabaseConnection,
        tapp_id: &str,
    ) -> (serde_json::Value, serde_json::Value, bool) {
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT approved_permissions, granted_permissions, needs_reauthorization
                   FROM tapps WHERE tapp_id = $1",
                vec![tapp_id.into()],
            ))
            .await
            .unwrap();
        let row = rows.first().expect("fixture row exists");
        (
            row.try_get::<serde_json::Value>("", "approved_permissions")
                .unwrap(),
            row.try_get::<serde_json::Value>("", "granted_permissions")
                .unwrap(),
            row.try_get::<bool>("", "needs_reauthorization").unwrap(),
        )
    }

    /// 显式运行要求环境变量；缺失即失败并给出命令（不静默跳过）。
    #[tokio::test]
    #[ignore = "requires PostgreSQL: run with TAPP_LEGACY_GRANT_TEST_DATABASE_URL set and -- --ignored"]
    async fn migration_cleans_legacy_grants_and_flags_affected_installs() {
        let database_url = std::env::var("TAPP_LEGACY_GRANT_TEST_DATABASE_URL").expect(
            "TAPP_LEGACY_GRANT_TEST_DATABASE_URL must be set to run this integration test. \
             Example: TAPP_LEGACY_GRANT_TEST_DATABASE_URL=postgres://myriad:CHANGE_ME_STRONG_PASSWORD@localhost:5432/myriad_test \
             cargo test -p migration -- --ignored",
        );
        let db = Database::connect(&database_url).await.unwrap();

        // Clean slate: full greenfield + upgrade path, then drop prior fixtures.
        crate::Migrator::up(&db, None).await.unwrap();
        db.execute_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "DELETE FROM tapps WHERE user_id = 999999",
        ))
        .await
        .unwrap();

        // Pre-upgrade fixture rows (retired strings still in the columns).
        insert_fixture(
            &db,
            "legacy-unaffected",
            "[\"storage:read\"]",
            "[\"storage:read\"]",
        )
        .await;
        insert_fixture(
            &db,
            "legacy-approved-only",
            "[\"storage\",\"ui:theme\",\"storage:read\"]",
            "[]",
        )
        .await;
        insert_fixture(&db, "legacy-retained-only", "[]", "[\"media:control\"]").await;
        insert_fixture(
            &db,
            "legacy-mixed",
            "[\"brew:write\",\"brew:comment\",\"federation:write\",\"federation:post\"]",
            "[\"storage\",\"media:control\",\"media:read\"]",
        )
        .await;
        insert_fixture(&db, "legacy-all-three", "[\"storage\",\"ui:theme\",\"media:control\",\"brew:write\",\"brew:comment\",\"federation:write\"]", "[\"storage\"]").await;
        // Malformed value: must be left byte-for-byte untouched and unmarked.
        insert_fixture(
            &db,
            "legacy-malformed",
            "{\"storage\": true}",
            "[\"storage:read\"]",
        )
        .await;

        // Run the cleanup (the migration already applied; re-run the
        // data step the way an interrupted upgrade would on retry).
        let changed = clear_legacy_grants(&db).await.unwrap();
        assert_eq!(
            changed, 3,
            "unaffected, retained-only, and malformed rows must not be touched"
        );

        // Unaffected row: byte-for-byte equal values, marker stays false.
        let (approved, granted, flagged) = read_row(&db, "legacy-unaffected").await;
        assert_eq!(approved, serde_json::json!(["storage:read"]));
        assert_eq!(granted, serde_json::json!(["storage:read"]));
        assert!(!flagged);

        // Affected in the approved column only.
        let (approved, granted, flagged) = read_row(&db, "legacy-approved-only").await;
        assert_eq!(approved, serde_json::json!(["ui:theme", "storage:read"]));
        assert_eq!(granted, serde_json::json!([]));
        assert!(flagged);

        // Retained coarse permissions are neither removed nor marked.
        let (approved, granted, flagged) = read_row(&db, "legacy-retained-only").await;
        assert_eq!(approved, serde_json::json!([]));
        assert_eq!(granted, serde_json::json!(["media:control"]));
        assert!(!flagged);

        // Mixed old/current in both columns: current permissions retained in order.
        let (approved, granted, flagged) = read_row(&db, "legacy-mixed").await;
        assert_eq!(
            approved,
            serde_json::json!(["brew:write", "federation:post"])
        );
        assert_eq!(granted, serde_json::json!(["media:control", "media:read"]));
        assert!(flagged);

        // All three retired names are removed; retained coarse names stay.
        let (approved, granted, flagged) = read_row(&db, "legacy-all-three").await;
        assert_eq!(
            approved,
            serde_json::json!(["ui:theme", "media:control", "brew:write"])
        );
        assert_eq!(granted, serde_json::json!([]));
        assert!(flagged);

        // Malformed value: untouched, never replaced with an empty array, unmarked.
        let (approved, granted, flagged) = read_row(&db, "legacy-malformed").await;
        assert_eq!(approved, serde_json::json!({"storage": true}));
        assert_eq!(granted, serde_json::json!(["storage:read"]));
        assert!(!flagged);

        // Repeated execution effect: a second run changes nothing.
        let changed = clear_legacy_grants(&db).await.unwrap();
        assert_eq!(changed, 0, "cleanup must be idempotent in effect");
        let (_, _, flagged) = read_row(&db, "legacy-mixed").await;
        assert!(flagged, "marker must survive repeated execution");

        // Successful re-approval clears the marker (update path writes marker
        // false + current permissions); the cleanup must not re-flag it.
        db.execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "UPDATE tapps SET needs_reauthorization = false, approved_permissions = $2::jsonb
              WHERE tapp_id = $1",
            vec![
                "legacy-approved-only".into(),
                serde_json::json!(["storage:read"]).to_string().into(),
            ],
        ))
        .await
        .unwrap();
        let changed = clear_legacy_grants(&db).await.unwrap();
        assert_eq!(changed, 0);
        let (_, _, flagged) = read_row(&db, "legacy-approved-only").await;
        assert!(
            !flagged,
            "cleanup must never re-flag a re-authorized installation"
        );

        // M1: down() 是 no-op —— 列与已落库标记在回滚后必须保留。
        let manager = sea_orm_migration::SchemaManager::new(&db);
        Migration.down(&manager).await.unwrap();
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT column_name FROM information_schema.columns
                  WHERE table_name = 'tapps' AND column_name = 'needs_reauthorization'",
                vec![],
            ))
            .await
            .unwrap();
        assert_eq!(
            rows.len(),
            1,
            "down() must not drop the needs_reauthorization column"
        );
        let (_, _, flagged) = read_row(&db, "legacy-mixed").await;
        assert!(
            flagged,
            "down() must preserve the durable re-authorization marker"
        );

        // L6（记录，不改变）：granted_permissions 在 002 中真实类型是 json
        // （schema_check 声称 jsonb 是既有不一致，本单元不擅自改列类型）。
        // B1 修复依赖该事实：谓词对 json 列显式 ::jsonb 才能通过 PostgreSQL 16。
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT data_type FROM information_schema.columns
                  WHERE table_name = 'tapps' AND column_name = 'granted_permissions'",
                vec![],
            ))
            .await
            .unwrap();
        assert_eq!(
            rows.first()
                .unwrap()
                .try_get::<String>("", "data_type")
                .unwrap(),
            "json",
            "002 defines granted_permissions as json; the predicate's ::jsonb cast depends on it"
        );

        db.execute_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "DELETE FROM tapps WHERE user_id = 999999",
        ))
        .await
        .unwrap();
    }
}
