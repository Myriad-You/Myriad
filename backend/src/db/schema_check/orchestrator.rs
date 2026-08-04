//! Schema ensure orchestration, drift reporting, advisory lock.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};

use super::ensure_heals::*;
use super::expected_indexes::get_expected_indexes;
use super::expected_schema::get_expected_schema;
use super::introspect::*;
use super::seeds::ensure_default_platforms;

/// Schema 版本号
///
/// 修改此版本号用于记录新的结构基线；schema 安全比对本身会在每次启动执行。
/// 格式建议：YYYY.MM.DD 或语义版本 X.Y.Z
///
/// 变更日志：
///
/// **分工**：数字系列 `migrations/001`–`006` 是新库权威建表，必须完整。
/// 本文件 `ensure_*` 只覆盖**近月新功能**（CREATE 兜底、唯一索引、PK 扩维等）；
/// **普通缺列**一律走 `get_expected_schema` 通用 ADD。
///
/// **Support floor: product ≥ 0.3.10.** 不再为更旧版本维护逐列「字段对齐」
/// heal（approved_permissions / engagement 过渡形态 / rate_* 专用 ALTER 等）。
///
/// - 2026.08.03.2: users 名称/简介文案来源（profile_text_source_kind / profile_text_source_ref）
/// - 2026.08.03.1: users 画像源选择（avatar_source_kind / avatar_source_ref / avatar_resolved_url / avatar_updated_at）
/// - 2026.08.02.2: tapp_storage 凭据字段数据库约束与序列化/查询边界加固
/// - 2026.08.02.1: tapp_storage 加密凭据字段（encrypted_value / binding_fingerprint）
/// - 2026.08.01.1: tapps.visibility（公开安装可见性 all|admin）
/// - 2026.07.31.1: 删除 <0.3.10 字段级对齐；缺列通用 ADD；analytics target 仅保留 PK heal
/// - 2026.07.30.5: analytics_visitor_seen.ordinal（访客到达序号）
/// - 2026.07.30.4: 近月新表——001 analytics / 004 heartbeat / 005 federation 扩展
/// - 2026.07.30.3 … 2026.07.30.1: analytics 基线
/// - 2026.07.27.x: federation FK / last_read_at / comprehensive 清理
/// - 2026.07.21–20: domain_aliases / interactions / heartbeat / policy / filters
/// - ≤0.3.9 字段对齐（已删，见 git）：approved_permissions 专用 ADD、整表 create 兜底等
/// Marker for ops/logs + `_schema_versions`. Bump only with real schema/heal work.
pub const SCHEMA_VERSION: &str = "2026.08.03.2";

pub async fn ensure_schema(db: &DatabaseConnection) -> Result<(), DbErr> {
    // 1. 尝试获取 Advisory Lock（非阻塞）
    // Advisory lock 是会话级的，必须在同一条连接上加锁和解锁。
    // 之前直接在连接池上执行，加锁和解锁常常落在不同连接：解锁永远失败，
    // 锁被池中连接持有直到进程退出——其他实例从此永久跳过 schema 自愈。
    let Some(guard) = AdvisoryLockGuard::acquire(db).await? else {
        tracing::info!("🔒 Another instance is running schema check, skipping...");
        return Ok(());
    };

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

    /// 在专用连接上尝试加锁。返回 Ok(Some(guard)) = 已持锁；Ok(None) = 被他人持有
    async fn acquire(db: &DatabaseConnection) -> Result<Option<Self>, DbErr> {
        let pool = db.get_postgres_connection_pool();
        let mut conn = pool
            .acquire()
            .await
            .map_err(|e| DbErr::Custom(format!("acquire lock connection: {}", e)))?;

        let acquired: bool = sea_orm::sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(Self::LOCK_ID)
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| DbErr::Custom(format!("advisory lock query: {}", e)))?;

        Ok(if acquired { Some(Self { conn }) } else { None })
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

/// 期望结构（本文件里的权威列表）与数据库实际结构的差异。
///
/// # 为什么需要它
///
/// 期望结构在两个地方各写了一遍：`migrations/` 里的 `Table::create`，以及本文件
/// 里 49 个 `TableDef` / 554 个 `ColumnDef`。两份定义没有任何机制保证一致，
/// 审计已经观察到索引语义漂移。
///
/// 有了这个只读报告，就能在 CI 里断言一件很强的事：
/// **在一个刚跑完 migration 的全新数据库上，drift 必须为空。**
/// 不为空就说明两份定义已经不一致 —— 不需要先做去重，就能立刻止住继续漂移。
#[derive(Debug, Default)]
pub struct SchemaDrift {
    pub missing_columns: Vec<DriftItem>,
    pub missing_indexes: Vec<DriftItem>,
}

impl SchemaDrift {
    pub fn is_empty(&self) -> bool {
        self.missing_columns.is_empty() && self.missing_indexes.is_empty()
    }

    pub fn len(&self) -> usize {
        self.missing_columns.len() + self.missing_indexes.len()
    }

    /// 补齐全部差异所需的 DDL，顺序为先列后索引（索引可能依赖新列）。
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
/// 表本身不存在时跳过该表的列检查 —— 建表是 Migrator 的职责，
/// schema_check 不再兜底整表创建。
pub async fn report_schema_drift(db: &DatabaseConnection) -> Result<SchemaDrift, DbErr> {
    let mut drift = SchemaDrift::default();

    let existing_tables = get_existing_tables(db).await?;

    for table_def in &get_expected_schema() {
        if !existing_tables.contains(&table_def.name) {
            tracing::debug!(
                "Table '{}' does not exist, skipping column check (rely on Migrator)",
                table_def.name
            );
            continue;
        }
        let existing_columns = get_table_columns(db, &table_def.name).await?;
        for col in &table_def.columns {
            if !existing_columns.contains(&col.name) {
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
    match ensure_default_platforms(db).await {
        Ok(n) if n > 0 => changes_made += n,
        Ok(_) => {}
        Err(e) => tracing::warn!("Default platforms seed warning: {}", e),
    }

    // 2/3. 比对期望列与索引（整表创建已不再由 schema_check 兜底）
    let drift = report_schema_drift(db).await?;
    if !drift.is_empty() {
        // 正常情况下这里应该是空的 —— migration 就该产出完整结构。
        // 有内容说明要么是从旧版本升级上来的库，要么 migration 与本文件的
        // 权威结构列表又漂移了（CI 的 migrations_leave_no_schema_drift 守这条）。
        tracing::info!(
            "📝 Schema healer will patch {} item(s):\n{}",
            drift.len(),
            drift.summary().trim_end()
        );
    }
    let ddl_statements: Vec<String> = drift.ddl_statements();
    changes_made += ddl_statements.len();

    // 4. 执行所有 DDL
    if !ddl_statements.is_empty() {
        tracing::info!("🔧 Applying {} schema changes...", ddl_statements.len());

        for ddl in &ddl_statements {
            tracing::debug!("Executing: {}", ddl);
            if let Err(e) = db.execute_unprepared(ddl).await {
                tracing::warn!("DDL execution warning: {} - {}", ddl, e);
            }
        }

        tracing::info!("✅ Applied {} schema changes", changes_made);
    } else {
        tracing::info!("✅ Database schema is up to date (no changes needed)");
    }

    // Ongoing object/data heals (not historical one-shot upgrade paths).
    ensure_tapp_storage_credential_constraint(db).await?;
    ensure_tapp_storage_quota(db).await?;
    if let Err(e) = ensure_federation_content_filters_table(db).await {
        tracing::warn!("federation_content_filters table ensure warning: {}", e);
    }
    if let Err(e) = ensure_federation_policy_settings_table(db).await {
        tracing::warn!("federation_policy_settings table ensure warning: {}", e);
    }
    if let Err(e) = ensure_timeline_unique(db).await {
        tracing::warn!("timeline unique index ensure warning: {}", e);
    }
    if let Err(e) = ensure_delivery_queue_unique(db).await {
        tracing::warn!("delivery queue unique index ensure warning: {}", e);
    }
    if let Err(e) = ensure_heartbeat_claims_table(db).await {
        tracing::warn!("heartbeat_claims table ensure warning: {}", e);
    }
    if let Err(e) = ensure_analytics_tables(db).await {
        tracing::warn!("analytics tables ensure warning: {}", e);
    }
    if let Err(e) = ensure_federation_domain_aliases_table(db).await {
        tracing::warn!("federation_domain_aliases table ensure warning: {}", e);
    }
    if let Err(e) = ensure_federation_object_interactions_table(db).await {
        tracing::warn!("federation_object_interactions table ensure warning: {}", e);
    }
    // last_read_at / rate_* / engagement 等字段：TableDef + 通用 drift ADD（无专用 heal）
    if let Err(e) = ensure_federation_foreign_keys(db).await {
        tracing::warn!("federation foreign keys ensure warning: {}", e);
    }
    if let Err(e) = cleanup_retired_comprehensive_reports(db).await {
        tracing::warn!("retired comprehensive reports cleanup warning: {}", e);
    }
    if let Err(e) = ensure_single_owner(db).await {
        tracing::warn!("Site owner seed warning: {}", e);
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
    // Column may still be missing if DDL failed; skip quietly.
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
        tracing::debug!("users.is_owner missing, skip owner seed");
        return Ok(());
    }

    // Partial unique index: at most one owner.
    if let Err(e) = db
        .execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_single_owner \
             ON users ((true)) WHERE is_owner = true",
        )
        .await
    {
        tracing::warn!("idx_users_single_owner create warning: {}", e);
    }

    // Collapse multiples → keep lowest id.
    db.execute_unprepared(
        "UPDATE users SET is_owner = false \
         WHERE is_owner = true \
           AND id <> (SELECT MIN(id) FROM users WHERE is_owner = true)",
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

/// 强制重新执行 schema 检查
#[allow(dead_code)]
pub async fn force_schema_check(db: &DatabaseConnection) -> Result<(), DbErr> {
    tracing::warn!("⚠️ Force schema check");

    // 尝试拿锁（重试一次）；强制模式下即使拿不到也继续执行
    let guard = match AdvisoryLockGuard::acquire(db).await? {
        Some(g) => Some(g),
        None => {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            AdvisoryLockGuard::acquire(db).await?
        }
    };

    let result = do_force_schema_check(db).await;

    if let Some(g) = guard {
        g.release().await;
    }

    result
}

/// 强制 schema 检查的内部实现
async fn do_force_schema_check(db: &DatabaseConnection) -> Result<(), DbErr> {
    if let Err(e) = ensure_default_platforms(db).await {
        tracing::warn!("Force check: default platforms seed warning: {}", e);
    }

    let existing_tables = get_existing_tables(db).await?;
    let expected_tables = get_expected_schema();

    for table_def in &expected_tables {
        if !existing_tables.contains(&table_def.name) {
            continue;
        }

        let existing_columns = get_table_columns(db, &table_def.name).await?;

        for col in &table_def.columns {
            if !existing_columns.contains(&col.name) {
                let ddl = generate_add_column_ddl(&table_def.name, col);
                let _ = db.execute_unprepared(&ddl).await;
            }
        }
    }

    ensure_tapp_storage_credential_constraint(db).await?;
    ensure_tapp_storage_quota(db).await?;
    if let Err(e) = ensure_federation_content_filters_table(db).await {
        tracing::warn!(
            "Force check: federation_content_filters table ensure warning: {}",
            e
        );
    }
    if let Err(e) = ensure_federation_policy_settings_table(db).await {
        tracing::warn!(
            "Force check: federation_policy_settings table ensure warning: {}",
            e
        );
    }
    if let Err(e) = ensure_timeline_unique(db).await {
        tracing::warn!("timeline unique index ensure warning: {}", e);
    }
    if let Err(e) = ensure_delivery_queue_unique(db).await {
        tracing::warn!("delivery queue unique index ensure warning: {}", e);
    }
    if let Err(e) = ensure_heartbeat_claims_table(db).await {
        tracing::warn!("Force check: heartbeat_claims table ensure warning: {}", e);
    }
    if let Err(e) = ensure_analytics_tables(db).await {
        tracing::warn!("Force check: analytics tables ensure warning: {}", e);
    }
    if let Err(e) = ensure_federation_domain_aliases_table(db).await {
        tracing::warn!(
            "Force check: federation_domain_aliases table ensure warning: {}",
            e
        );
    }
    if let Err(e) = ensure_federation_object_interactions_table(db).await {
        tracing::warn!(
            "Force check: federation_object_interactions table ensure warning: {}",
            e
        );
    }
    if let Err(e) = cleanup_retired_comprehensive_reports(db).await {
        tracing::warn!(
            "Force check: retired comprehensive reports cleanup warning: {}",
            e
        );
    }
    if let Err(e) = ensure_single_owner(db).await {
        tracing::warn!("Force check: site owner seed warning: {}", e);
    }

    Ok(())
}
