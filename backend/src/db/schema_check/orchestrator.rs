//! Schema ensure orchestration, drift reporting, advisory lock.
use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};
use tokio::time::{Instant, sleep};

use super::ensure_heals::*;
use super::expected_indexes::get_expected_indexes;
use super::expected_schema::get_expected_schema;
use super::introspect::*;
use super::seeds::{ensure_default_config, ensure_default_platforms};

/// Schema 版本号
///
/// 修改此版本号用于记录新的结构基线；schema 安全比对本身会在每次启动执行。
/// 格式建议：YYYY.MM.DD 或语义版本 X.Y.Z
///
/// Marker for ops/logs + `_schema_versions`. Bump only with real schema/heal work.
///
/// 数字系列 `migrations/001`–`006` 是新库权威建表。没有文件的
/// `seaql_migrations` 行在 `Migrator::up` 之前删掉。普通缺列走
/// `get_expected_schema` 通用 ADD。Support floor: product ≥ 0.3.10。
/// Current: drop July CREATE heals; 003 source applications; 006 identities in TableDef。
pub const SCHEMA_VERSION: &str = "2026.09.18.1";

const SCHEMA_LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(120);
const SCHEMA_LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(250);

pub async fn ensure_schema(db: &DatabaseConnection) -> Result<(), DbErr> {
    // 1. 获取 Advisory Lock。其他实例正在检查时等待，绝不能跳过检查后继续提供流量。
    // Advisory lock 是会话级的，必须在同一条连接上加锁和解锁。
    let guard = AdvisoryLockGuard::acquire(db).await?;

    let result = do_schema_check(db).await;

    // 在同一连接上释放；即使失败，连接归还/关闭时会话级锁也会随之释放
    guard.release().await;

    result
}

/// 持有专用连接的 Advisory Lock 守卫
struct AdvisoryLockGuard {
    conn: sea_orm::sqlx::pool::PoolConnection<sea_orm::sqlx::Postgres>,
}

impl AdvisoryLockGuard {
    const LOCK_ID: i64 = 0x4D59524941445343;

    /// 在专用连接上等待加锁。超时是启动失败，不能当成 schema 已就绪。
    async fn acquire(db: &DatabaseConnection) -> Result<Self, DbErr> {
        let pool = db.get_postgres_connection_pool();
        let mut conn = pool
            .acquire()
            .await
            .map_err(|e| DbErr::Custom(format!("acquire lock connection: {}", e)))?;

        let deadline = Instant::now() + SCHEMA_LOCK_WAIT_TIMEOUT;
        loop {
            let acquired: bool = sea_orm::sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
                .bind(Self::LOCK_ID)
                .fetch_one(&mut *conn)
                .await
                .map_err(|e| DbErr::Custom(format!("advisory lock query: {}", e)))?;

            if acquired {
                return Ok(Self { conn });
            }
            if Instant::now() >= deadline {
                return Err(DbErr::Custom(format!(
                    "timed out after {}s waiting for schema advisory lock",
                    SCHEMA_LOCK_WAIT_TIMEOUT.as_secs()
                )));
            }
            sleep(SCHEMA_LOCK_RETRY_INTERVAL).await;
        }
    }

    /// 在加锁的同一连接上释放
    async fn release(mut self) {
        if let Err(e) = sea_orm::sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(Self::LOCK_ID)
            .execute(&mut *self.conn)
            .await
        {
            tracing::warn!("Failed to release advisory lock: {}", e);
        }
    }
}

/// 一条 schema 差异及其修补 DDL。
#[derive(Debug, Clone)]
pub struct DriftItem {
    /// 人类可读标识（`表.列` 或索引名）
    pub label: String,
    /// 补齐它所需的 DDL
    pub ddl: String,
}

/// 期望结构（`expected_schema::get_expected_schema()`）与数据库实际结构的差异。
///
/// # 为什么需要它
///
/// 期望结构在两个地方各写了一遍：`migrations/` 里的 `Table::create`，以及 `schema_check` 的 `TableDef` 列表。
/// 两份定义没有任何机制保证一致。
///
/// 有了这个只读报告，就能在 CI 里断言一件很强的事：
/// **在一个刚跑完 migration 的全新数据库上，drift 必须为空。**
/// 不为空就说明两份定义已经不一致 —— 不需要先做去重，就能立刻止住继续漂移。
#[derive(Debug, Default)]
pub struct SchemaDrift {
    pub missing_tables: Vec<String>,
    pub missing_columns: Vec<DriftItem>,
    pub missing_indexes: Vec<DriftItem>,
}

impl SchemaDrift {
    pub fn is_empty(&self) -> bool {
        self.missing_tables.is_empty()
            && self.missing_columns.is_empty()
            && self.missing_indexes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.missing_tables.len() + self.missing_columns.len() + self.missing_indexes.len()
    }

    /// 缺列/缺索引的 DDL（先列后索引）。缺表不在这里，见 `missing_tables`。
    pub fn ddl_statements(&self) -> Vec<String> {
        self.missing_columns
            .iter()
            .chain(self.missing_indexes.iter())
            .map(|i| i.ddl.clone())
            .collect()
    }

    /// 供 CI 失败信息使用的多行摘要。
    pub fn summary(&self) -> String {
        let mut out = String::new();
        for table in &self.missing_tables {
            out.push_str(&format!("  missing table:  {table}\n"));
        }
        for i in &self.missing_columns {
            out.push_str(&format!("  missing column: {}\n", i.label));
        }
        for i in &self.missing_indexes {
            out.push_str(&format!("  missing index:  {}\n", i.label));
        }
        out
    }
}

/// 只读比对期望结构与实际结构；不执行任何 DDL。
///
/// 缺表记入 `missing_tables`。通用 `ddl_statements` 不建表；部分近期表由后面的
/// `ensure_*` 做 `CREATE TABLE IF NOT EXISTS`。剩余漂移非空则不写 version / 不 ready。
pub async fn report_schema_drift(db: &DatabaseConnection) -> Result<SchemaDrift, DbErr> {
    let mut drift = SchemaDrift::default();

    let existing_tables = get_existing_tables(db).await?;
    let existing_columns = get_all_table_columns(db).await?;

    for table_def in &get_expected_schema() {
        if !existing_tables.contains(&table_def.name) {
            drift.missing_tables.push(table_def.name.clone());
            continue;
        }
        let table_columns = existing_columns.get(&table_def.name);
        for col in &table_def.columns {
            if !table_columns.is_some_and(|columns| columns.contains(&col.name)) {
                drift.missing_columns.push(DriftItem {
                    label: format!("{}.{}", table_def.name, col.name),
                    ddl: generate_add_column_ddl(&table_def.name, col),
                });
            }
        }
    }

    let existing_indexes = get_existing_indexes(db).await?;
    for idx in &get_expected_indexes() {
        if !existing_indexes.contains(&idx.name) && existing_tables.contains(&idx.table) {
            drift.missing_indexes.push(DriftItem {
                label: idx.name.clone(),
                ddl: generate_create_index_ddl(idx),
            });
        }
    }

    Ok(drift)
}

/// 实际执行 schema 检查的内部函数
async fn do_schema_check(db: &DatabaseConnection) -> Result<(), DbErr> {
    // 检查版本是否已应用
    // 修改：即使版本已应用也强制检查，确保 schema 完整性
    let version_applied = is_schema_version_applied(db, SCHEMA_VERSION).await?;

    if version_applied {
        tracing::info!(
            "ℹ️ Schema version {} marked as applied, but performing safety check...",
            SCHEMA_VERSION
        );
    } else {
        tracing::info!(
            "🔍 Schema version {} not applied, checking database structure...",
            SCHEMA_VERSION
        );
    }

    let mut changes_made = 0;

    // 1. 同步默认平台种子行（表由 Migrator 创建；此处只补业务目录数据）
    let seeded_platforms = ensure_default_platforms(db).await?;
    if seeded_platforms > 0 {
        changes_made += seeded_platforms;
    }

    // Seed all runtime configuration keys after the explicit default-open
    // entries have had first refusal, preserving any existing administrator
    // choice via ON CONFLICT DO NOTHING.
    let seeded_config = ensure_default_config(db).await?;
    if seeded_config > 0 {
        changes_made += seeded_config;
    }

    // Rebuild `federation_inbox_receipts` before generic ADD COLUMN:
    // `inbox_scope` is NOT NULL without a default and belongs in the PK.
    ensure_federation_inbox_receipts_table(db).await?;

    // 2/3. 比对期望列与索引（整表创建已不再由 schema_check 兜底）
    let drift = report_schema_drift(db).await?;
    if !drift.is_empty() {
        // 正常情况下这里应该是空的 —— migration 就该产出完整结构。
        // 有内容说明要么是从旧版本升级上来的库，要么 migration 与 `get_expected_schema()` 又漂移了。
        let drift_summary = drift.summary();
        tracing::info!(
            "📝 Schema healer will patch {} item(s):\n{}",
            drift.len(),
            drift_summary.trim_end()
        );
    }
    let ddl_statements: Vec<String> = drift.ddl_statements();
    changes_made += ddl_statements.len();

    // 4. 执行所有 DDL
    if !ddl_statements.is_empty() {
        tracing::info!("🔧 Applying {} schema changes...", ddl_statements.len());

        for item in drift.missing_columns.iter().chain(&drift.missing_indexes) {
            let ddl = &item.ddl;
            tracing::debug!("Executing: {}", ddl);
            // These unique indexes must clean historical duplicates before creation.
            let result = match item.label.as_str() {
                "idx_timeline_user_activity" => ensure_timeline_unique(db).await,
                "idx_delivery_queue_activity_target" => ensure_delivery_queue_unique(db).await,
                "idx_channels_active_relationship" => {
                    ensure_channels_active_relationship_unique(db).await
                }
                "idx_platform_metadata_user_platform" => {
                    ensure_platform_metadata_unique(db).await
                }
                _ => db.execute_unprepared(ddl).await.map(|_| ()),
            };
            result.map_err(|e| DbErr::Custom(format!("schema repair DDL failed: {ddl}: {e}")))?;
        }

        tracing::info!("✅ Applied {} schema changes", changes_made);
    } else {
        tracing::info!("✅ Database schema is up to date (no changes needed)");
    }

    // Ongoing object/data heals, plus recent (~1 month) CREATE IF NOT EXISTS.
    ensure_tapp_storage_credential_constraint(db).await?;
    ensure_agent_tasks_status_check(db).await?;
    ensure_tapp_storage_quota(db).await?;
    ensure_timeline_unique(db).await?;
    ensure_delivery_queue_unique(db).await?;
    ensure_channels_active_relationship_unique(db).await?;
    ensure_platform_metadata_unique(db).await?;
    ensure_agent_intentions_table(db).await?;
    ensure_agent_autonomy_grants_table(db).await?;
    ensure_agent_merope_tables(db).await?;
    ensure_phantasi_item_topic_index(db).await?;
    ensure_phantasi_state_revision(db).await?;
    ensure_phantasi_content_revision(db).await?;
    ensure_phantasi_note_docs_table(db).await?;
    ensure_note_editor_history(db).await?;
    ensure_phantasi_note_authors_table(db).await?;
    ensure_phantasi_source_applications_table(db).await?;
    ensure_media_assets_table(db).await?;
    ensure_phantasi_note_source_unique(db).await?;
    ensure_rsshub_global_url_unique(db).await?;
    ensure_phantasi_application_pending_unique(db).await?;
    ensure_tapp_shortcut_chord_unique(db).await?;
    // last_read_at / rate_* / engagement 等字段：TableDef + 通用 drift ADD（无专用 heal）
    ensure_federation_foreign_keys(db).await?;
    ensure_single_owner(db).await?;

    // 只有所有 repair 都成功且最终只读复核无漂移，才能记录版本并开放服务。
    let remaining_drift = report_schema_drift(db).await?;
    if !remaining_drift.is_empty() {
        return Err(DbErr::Custom(format!(
            "schema still has {} drift item(s) after repair:\n{}",
            remaining_drift.len(),
            remaining_drift.summary().trim_end()
        )));
    }

    // 5. 记录版本已应用
    mark_schema_version_applied(db, SCHEMA_VERSION).await?;
    tracing::info!("📌 Schema version {} marked as applied", SCHEMA_VERSION);

    Ok(())
}

/// Ensure `users.is_owner` exists with exactly one owner row (idempotent).
///
/// Seed priority (when zero owners): id=1 if admin → lowest-id admin → lowest-id user.
/// Multiple owners collapse to the lowest id.
///
/// After exactly one owner is ensured, heals `is_admin = true` on that row.
/// Also creates partial unique index `idx_users_single_owner` (not in
/// `get_expected_indexes` — expression/partial DDL is awkward for the generic
/// index path). Invariant: owner implies admin. Zero users → no-op.
pub async fn ensure_single_owner(db: &DatabaseConnection) -> Result<(), DbErr> {
    // Column may still be missing if DDL failed; that means schema is not ready.
    let col_check = db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT 1 AS ok FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'users' AND column_name = 'is_owner' \
             LIMIT 1",
            vec![],
        ))
        .await?;
    if col_check.is_none() {
        return Err(DbErr::Custom(
            "users.is_owner is missing after schema repair".to_string(),
        ));
    }

    // Collapse multiples before the unique index; otherwise CREATE UNIQUE fails
    // on existing duplicates and the UPDATE never runs.
    db.execute_unprepared(
        "UPDATE users SET is_owner = false \
         WHERE is_owner = true \
           AND id <> (SELECT MIN(id) FROM users WHERE is_owner = true)",
    )
    .await?;

    db.execute_unprepared(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_single_owner \
         ON users ((true)) WHERE is_owner = true",
    )
    .await?;

    // Seed if none — set both is_owner and is_admin so a non-admin pick is still usable.
    let seeded = db
        .execute_unprepared(
            "UPDATE users SET is_owner = true, is_admin = true \
             WHERE id = COALESCE( \
               (SELECT id FROM users WHERE id = 1 AND is_admin = true LIMIT 1), \
               (SELECT id FROM users WHERE is_admin = true ORDER BY id ASC LIMIT 1), \
               (SELECT id FROM users ORDER BY id ASC LIMIT 1) \
             ) \
             AND NOT EXISTS (SELECT 1 FROM users WHERE is_owner = true)",
        )
        .await?;

    if seeded.rows_affected() > 0 {
        tracing::info!("✅ Site owner seeded (users.is_owner + is_admin)");
    }

    // Owner implies admin (heal existing rows that were seeded without is_admin).
    let healed = db
        .execute_unprepared(
            "UPDATE users SET is_admin = true \
             WHERE is_owner = true AND is_admin = false",
        )
        .await?;

    if healed.rows_affected() > 0 {
        tracing::info!(
            "✅ Site owner is_admin healed ({} row(s))",
            healed.rows_affected()
        );
    }

    Ok(())
}

#[cfg(test)]
mod drift_tests {
    use super::SchemaDrift;

    #[test]
    fn missing_table_is_fatal_and_has_no_runtime_create_ddl() {
        let drift = SchemaDrift {
            missing_tables: vec!["users".to_string()],
            ..SchemaDrift::default()
        };

        assert!(!drift.is_empty());
        assert_eq!(drift.len(), 1);
        assert!(drift.ddl_statements().is_empty());
        assert!(drift.summary().contains("missing table:  users"));
    }
}
