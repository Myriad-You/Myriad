use sea_orm_migration::prelude::*;

/// Tapp 系统数据库结构
///
/// tapps / widgets / storage / quota / store sources / scheduled tasks /
/// executions / user activities / runtime registry·mailbox / AI cost ledger /
/// storage 8388608 字节触发器。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ==================== 1. TAPPS 表 ====================
        // 存储已安装的 Tapp 应用元数据
        manager
            .create_table(
                Table::create()
                    .table(Tapps::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Tapps::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Tapps::TappId).string_len(255).not_null())
                    .col(ColumnDef::new(Tapps::UserId).integer().not_null())
                    .col(ColumnDef::new(Tapps::Name).string_len(255).not_null())
                    .col(ColumnDef::new(Tapps::Version).string_len(50).not_null())
                    .col(ColumnDef::new(Tapps::Description).text())
                    .col(ColumnDef::new(Tapps::Author).json())
                    .col(ColumnDef::new(Tapps::Icon).text())
                    .col(ColumnDef::new(Tapps::ThemeColor).string_len(20))
                    .col(ColumnDef::new(Tapps::Manifest).json().not_null())
                    .col(
                        ColumnDef::new(Tapps::Status)
                            .string_len(20)
                            .not_null()
                            .default("installed"),
                    )
                    .col(
                        ColumnDef::new(Tapps::GrantedPermissions)
                            .json()
                            .not_null()
                            .default("[]"),
                    )
                    .col(
                        ColumnDef::new(Tapps::ApprovedPermissions)
                            .json_binary()
                            .not_null()
                            .default("[]"),
                    )
                    .col(
                        ColumnDef::new(Tapps::NeedsReauthorization)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(Tapps::FilePath).text().not_null())
                    .col(ColumnDef::new(Tapps::CodePath).text().not_null())
                    .col(
                        ColumnDef::new(Tapps::InstalledAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(Tapps::LastRunAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(Tapps::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(Tapps::ErrorMessage).text())
                    .col(
                        ColumnDef::new(Tapps::Visibility)
                            .string_len(20)
                            .not_null()
                            .default("all"),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一用户不能安装同一 Tapp 两次
        manager
            .create_index(
                Index::create()
                    .name("idx_tapps_user_tapp_id")
                    .table(Tapps::Table)
                    .col(Tapps::UserId)
                    .col(Tapps::TappId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tapps_user_id")
                    .table(Tapps::Table)
                    .col(Tapps::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tapps_status")
                    .table(Tapps::Table)
                    .col(Tapps::Status)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 状态约束（仅 PostgreSQL `check_tapp_status`）
        let db_backend = manager.get_database_backend();
        if matches!(db_backend, sea_orm::DatabaseBackend::Postgres) {
            let _ = manager
                .get_connection()
                .execute_unprepared(
                    "DO $$ BEGIN
                    IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'check_tapp_status') THEN
                        ALTER TABLE tapps ADD CONSTRAINT check_tapp_status 
                        CHECK (status IN ('installed', 'running', 'suspended', 'error'));
                    END IF;
                END $$;",
                )
                .await;
        }

        // ==================== 2. TAPP_WIDGETS 表 ====================
        // 存储 Tapp 注册的小组件
        manager
            .create_table(
                Table::create()
                    .table(TappWidgets::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TappWidgets::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TappWidgets::WidgetId)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TappWidgets::TappId)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TappWidgets::UserId).integer().not_null())
                    .col(ColumnDef::new(TappWidgets::Name).string_len(255).not_null())
                    .col(ColumnDef::new(TappWidgets::Description).text())
                    .col(ColumnDef::new(TappWidgets::Icon).text())
                    .col(
                        ColumnDef::new(TappWidgets::DefaultSize)
                            .string_len(10)
                            .not_null()
                            .default("2x2"),
                    )
                    .col(
                        ColumnDef::new(TappWidgets::Sizes)
                            .json()
                            .not_null()
                            .default("[\"2x2\"]"),
                    )
                    .col(
                        ColumnDef::new(TappWidgets::Category)
                            .string_len(50)
                            .default("custom"),
                    )
                    .col(ColumnDef::new(TappWidgets::Config).json().not_null())
                    .col(
                        ColumnDef::new(TappWidgets::RegisteredAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一用户同一 Tapp 的 Widget ID 唯一
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_widgets_unique")
                    .table(TappWidgets::Table)
                    .col(TappWidgets::UserId)
                    .col(TappWidgets::TappId)
                    .col(TappWidgets::WidgetId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_widgets_user_id")
                    .table(TappWidgets::Table)
                    .col(TappWidgets::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 3. TAPP_STORAGE 表 ====================
        // 存储 Tapp 的键值数据
        manager
            .create_table(
                Table::create()
                    .table(TappStorage::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TappStorage::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TappStorage::TappId)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TappStorage::UserId).integer().not_null())
                    .col(ColumnDef::new(TappStorage::Key).string_len(255).not_null())
                    .col(ColumnDef::new(TappStorage::Value).json().not_null())
                    .col(ColumnDef::new(TappStorage::EncryptedValue).text())
                    .col(ColumnDef::new(TappStorage::BindingFingerprint).string_len(64))
                    .col(
                        ColumnDef::new(TappStorage::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TappStorage::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // encrypted_value / binding_fingerprint 仅允许 key 以 `_credentials.` 开头，且必须同时非空。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
ALTER TABLE tapp_storage
    ADD CONSTRAINT tapp_storage_credential_fields_check
    CHECK (
        (encrypted_value IS NULL AND binding_fingerprint IS NULL)
        OR (
            starts_with(key, '_credentials.')
            AND encrypted_value IS NOT NULL
            AND binding_fingerprint IS NOT NULL
        )
    );
"#,
            )
            .await?;

        // 唯一索引：同一用户同一 Tapp 的 Key 唯一
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_storage_unique")
                    .table(TappStorage::Table)
                    .col(TappStorage::UserId)
                    .col(TappStorage::TappId)
                    .col(TappStorage::Key)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_storage_user_tapp")
                    .table(TappStorage::Table)
                    .col(TappStorage::UserId)
                    .col(TappStorage::TappId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 4. TAPP_QUOTA_USAGE 表 ====================
        // 存储 Tapp 的配额使用情况
        manager
            .create_table(
                Table::create()
                    .table(TappQuotaUsage::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TappQuotaUsage::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TappQuotaUsage::TappId)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TappQuotaUsage::UserId).integer().not_null())
                    .col(
                        ColumnDef::new(TappQuotaUsage::QuotaType)
                            .string_len(50)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TappQuotaUsage::Used)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(TappQuotaUsage::Limit).integer().not_null())
                    .col(
                        ColumnDef::new(TappQuotaUsage::PeriodStart)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TappQuotaUsage::PeriodEnd)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TappQuotaUsage::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_quota_unique")
                    .table(TappQuotaUsage::Table)
                    .col(TappQuotaUsage::UserId)
                    .col(TappQuotaUsage::TappId)
                    .col(TappQuotaUsage::QuotaType)
                    .col(TappQuotaUsage::PeriodStart)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 5. TAPP_STORE_SOURCES 表 ====================
        // 存储远程商店源配置
        manager
            .create_table(
                Table::create()
                    .table(TappStoreSources::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TappStoreSources::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TappStoreSources::Name)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TappStoreSources::Description).text())
                    .col(ColumnDef::new(TappStoreSources::Url).text().not_null())
                    .col(
                        ColumnDef::new(TappStoreSources::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(TappStoreSources::Official)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(TappStoreSources::Icon).string_len(100))
                    .col(
                        ColumnDef::new(TappStoreSources::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TappStoreSources::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：URL 唯一
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_store_sources_url")
                    .table(TappStoreSources::Table)
                    .col(TappStoreSources::Url)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 插入官方商店源
        manager
            .exec_stmt(
                Query::insert()
                    .into_table(TappStoreSources::Table)
                    .columns([
                        TappStoreSources::Name,
                        TappStoreSources::Description,
                        TappStoreSources::Url,
                        TappStoreSources::Enabled,
                        TappStoreSources::Official,
                        TappStoreSources::Icon,
                    ])
                    .values_panic([
                        "Myriad 官方商店".into(),
                        "官方应用商店，提供经过审核的高质量应用".into(),
                        "https://raw.githubusercontent.com/Myriad-You/tapp-store/main/index.json"
                            .into(),
                        true.into(),
                        true.into(),
                        "🏪".into(),
                    ])
                    .to_owned(),
            )
            .await?;

        // ==================== 6. TAPP_SCHEDULED_TASKS 表 ====================
        // 存储 Tapp 注册的定时任务
        manager
            .create_table(
                Table::create()
                    .table(TappScheduledTasks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TappScheduledTasks::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // varchar(255) NOT NULL；唯一性见 idx_tapp_scheduled_tasks_unique
                    .col(
                        ColumnDef::new(TappScheduledTasks::TaskId)
                            .string_len(255)
                            .not_null(),
                    )
                    // 所属 Tapp ID
                    .col(
                        ColumnDef::new(TappScheduledTasks::TappId)
                            .string_len(255)
                            .not_null(),
                    )
                    // 所属用户 ID
                    .col(
                        ColumnDef::new(TappScheduledTasks::UserId)
                            .integer()
                            .not_null(),
                    )
                    // 任务名称（用于显示）
                    .col(
                        ColumnDef::new(TappScheduledTasks::Name)
                            .string_len(255)
                            .not_null(),
                    )
                    // varchar(20) NOT NULL；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(TappScheduledTasks::ScheduleType)
                            .string_len(20)
                            .not_null(),
                    )
                    // 调度配置 (JSON)
                    .col(
                        ColumnDef::new(TappScheduledTasks::ScheduleConfig)
                            .json()
                            .not_null(),
                    )
                    // json，可空
                    .col(ColumnDef::new(TappScheduledTasks::Payload).json())
                    // varchar(20) NOT NULL default frontend；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(TappScheduledTasks::ExecutionTarget)
                            .string_len(20)
                            .not_null()
                            .default("frontend"),
                    )
                    // 后端可执行的操作列表 (JSON)
                    .col(ColumnDef::new(TappScheduledTasks::BackendActions).json())
                    // 是否启用
                    .col(
                        ColumnDef::new(TappScheduledTasks::Enabled)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    // varchar(20) NOT NULL default skip；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(TappScheduledTasks::MissedPolicy)
                            .string_len(20)
                            .not_null()
                            .default("skip"),
                    )
                    // varchar(20) NOT NULL default user；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(TappScheduledTasks::Scope)
                            .string_len(20)
                            .not_null()
                            .default("user"),
                    )
                    // 重试配置 (JSON)
                    .col(ColumnDef::new(TappScheduledTasks::RetryConfig).json())
                    // 下次执行时间
                    .col(ColumnDef::new(TappScheduledTasks::NextRunAt).timestamp_with_time_zone())
                    // 上次执行时间
                    .col(ColumnDef::new(TappScheduledTasks::LastRunAt).timestamp_with_time_zone())
                    // 上次执行结果 (JSON)
                    .col(ColumnDef::new(TappScheduledTasks::LastRunResult).json())
                    // 执行统计 (JSON)
                    // { totalRuns, successRuns, failedRuns, missedRuns }
                    .col(
                        ColumnDef::new(TappScheduledTasks::Stats)
                            .json()
                            .not_null()
                            .default(
                                r#"{"totalRuns":0,"successRuns":0,"failedRuns":0,"missedRuns":0}"#,
                            ),
                    )
                    // 创建时间
                    .col(
                        ColumnDef::new(TappScheduledTasks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 更新时间
                    .col(
                        ColumnDef::new(TappScheduledTasks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一用户同一 Tapp 的任务 ID 唯一
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_scheduled_tasks_unique")
                    .table(TappScheduledTasks::Table)
                    .col(TappScheduledTasks::UserId)
                    .col(TappScheduledTasks::TappId)
                    .col(TappScheduledTasks::TaskId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按用户查询
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_scheduled_tasks_user")
                    .table(TappScheduledTasks::Table)
                    .col(TappScheduledTasks::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按 Tapp 查询
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_scheduled_tasks_tapp")
                    .table(TappScheduledTasks::Table)
                    .col(TappScheduledTasks::TappId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按下次执行时间查询（用于调度器快速查找到期任务）
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_scheduled_tasks_next_run")
                    .table(TappScheduledTasks::Table)
                    .col(TappScheduledTasks::Enabled)
                    .col(TappScheduledTasks::NextRunAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 7. TAPP_TASK_EXECUTIONS 表 ====================
        // 存储任务执行历史
        manager
            .create_table(
                Table::create()
                    .table(TappTaskExecutions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TappTaskExecutions::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 关联的任务表 ID
                    .col(
                        ColumnDef::new(TappTaskExecutions::ScheduledTaskId)
                            .integer()
                            .not_null(),
                    )
                    // 用户 ID（冗余，用于快速查询）
                    .col(
                        ColumnDef::new(TappTaskExecutions::UserId)
                            .integer()
                            .not_null(),
                    )
                    // Tapp ID（冗余，用于快速查询）
                    .col(
                        ColumnDef::new(TappTaskExecutions::TappId)
                            .string_len(255)
                            .not_null(),
                    )
                    // 任务 ID（冗余，用于快速查询）
                    .col(
                        ColumnDef::new(TappTaskExecutions::TaskId)
                            .string_len(255)
                            .not_null(),
                    )
                    // 计划执行时间
                    .col(
                        ColumnDef::new(TappTaskExecutions::ScheduledAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // 实际执行时间
                    .col(
                        ColumnDef::new(TappTaskExecutions::ExecutedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // 完成时间
                    .col(ColumnDef::new(TappTaskExecutions::CompletedAt).timestamp_with_time_zone())
                    // varchar(20) NOT NULL；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(TappTaskExecutions::ExecutionTarget)
                            .string_len(20)
                            .not_null(),
                    )
                    // varchar(20) NOT NULL default pending；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(TappTaskExecutions::Status)
                            .string_len(20)
                            .not_null()
                            .default("pending"),
                    )
                    // 是否为补偿执行（错过任务后的补偿）
                    .col(
                        ColumnDef::new(TappTaskExecutions::IsCompensation)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 执行结果 (JSON)
                    .col(ColumnDef::new(TappTaskExecutions::Result).json())
                    // 错误信息
                    .col(ColumnDef::new(TappTaskExecutions::Error).text())
                    // 执行时长（毫秒）
                    .col(ColumnDef::new(TappTaskExecutions::DurationMs).integer())
                    // 重试次数
                    .col(
                        ColumnDef::new(TappTaskExecutions::RetryCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        // 索引：按任务查询历史
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_task_executions_task")
                    .table(TappTaskExecutions::Table)
                    .col(TappTaskExecutions::ScheduledTaskId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按用户和 Tapp 查询
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_task_executions_user_tapp")
                    .table(TappTaskExecutions::Table)
                    .col(TappTaskExecutions::UserId)
                    .col(TappTaskExecutions::TappId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按执行时间查询（用于清理历史记录）
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_task_executions_executed_at")
                    .table(TappTaskExecutions::Table)
                    .col(TappTaskExecutions::ExecutedAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 8. TAPP_USER_ACTIVITIES 表 ====================
        // 每用户每 Tapp 的活动记录（含 last_run_at）
        manager
            .create_table(
                Table::create()
                    .table(TappUserActivities::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TappUserActivities::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 用户 ID
                    .col(
                        ColumnDef::new(TappUserActivities::UserId)
                            .integer()
                            .not_null(),
                    )
                    // Tapp ID（应用标识符）
                    .col(
                        ColumnDef::new(TappUserActivities::TappId)
                            .string_len(255)
                            .not_null(),
                    )
                    // 最后运行时间
                    .col(
                        ColumnDef::new(TappUserActivities::LastRunAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    // 运行次数
                    .col(
                        ColumnDef::new(TappUserActivities::RunCount)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一索引：同一用户同一 Tapp 只有一条记录
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_user_activities_unique")
                    .table(TappUserActivities::Table)
                    .col(TappUserActivities::UserId)
                    .col(TappUserActivities::TappId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 索引：按用户和最后运行时间查询（用于获取最近使用的 Tapp）
        manager
            .create_index(
                Index::create()
                    .name("idx_tapp_user_activities_user_last_run")
                    .table(TappUserActivities::Table)
                    .col(TappUserActivities::UserId)
                    .col(TappUserActivities::LastRunAt)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 9. TAPP RUNTIME SHARED STATE ====================
        // tapp_runtime_registry、tapp_runtime_mailbox、tapp_ai_cost_ledger，
        // 以及 tapp_storage 8388608 字节 INSERT/UPDATE 触发器。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS tapp_runtime_registry (
    namespace VARCHAR(64) NOT NULL,
    record_id VARCHAR(160) NOT NULL,
    subject_id INTEGER,
    owner_id INTEGER,
    tapp_id VARCHAR(255),
    runtime_id VARCHAR(160),
    payload JSONB NOT NULL,
    expires_at BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (namespace, record_id)
);

CREATE INDEX IF NOT EXISTS idx_tapp_runtime_registry_subject
    ON tapp_runtime_registry (namespace, subject_id, expires_at);
CREATE INDEX IF NOT EXISTS idx_tapp_runtime_registry_tapp
    ON tapp_runtime_registry (namespace, tapp_id, expires_at);
CREATE INDEX IF NOT EXISTS idx_tapp_runtime_registry_runtime
    ON tapp_runtime_registry (namespace, runtime_id, expires_at);

CREATE TABLE IF NOT EXISTS tapp_runtime_mailbox (
    message_id BIGSERIAL PRIMARY KEY,
    channel VARCHAR(64) NOT NULL,
    runtime_id VARCHAR(160) NOT NULL,
    payload JSONB NOT NULL,
    expires_at BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tapp_runtime_mailbox_recipient
    ON tapp_runtime_mailbox (channel, runtime_id, message_id);
CREATE INDEX IF NOT EXISTS idx_tapp_runtime_mailbox_expiry
    ON tapp_runtime_mailbox (expires_at);

-- 独立 AI 费用账本：逐次调用的 append-only 流水，与按日聚合的
-- tapp_quota_usage 配额计数相互独立，不随每日重置。
CREATE TABLE IF NOT EXISTS tapp_ai_cost_ledger (
    id BIGSERIAL PRIMARY KEY,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    subject_id INTEGER NOT NULL,
    owner_id INTEGER NOT NULL,
    tapp_id VARCHAR(255) NOT NULL,
    task_id VARCHAR(160) NOT NULL,
    source VARCHAR(64) NOT NULL,
    operation VARCHAR(32) NOT NULL,
    provider VARCHAR(64) NOT NULL,
    model VARCHAR(255) NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    tokens_estimated BOOLEAN NOT NULL DEFAULT TRUE,
    cost_micro_usd BIGINT,
    status VARCHAR(16) NOT NULL,
    error_code VARCHAR(64)
);

CREATE INDEX IF NOT EXISTS idx_tapp_ai_cost_subject_time
    ON tapp_ai_cost_ledger (subject_id, occurred_at);
CREATE INDEX IF NOT EXISTS idx_tapp_ai_cost_tapp_time
    ON tapp_ai_cost_ledger (tapp_id, occurred_at);

CREATE OR REPLACE FUNCTION enforce_tapp_storage_quota()
RETURNS TRIGGER AS $$
DECLARE
    current_bytes BIGINT;
    projected_bytes BIGINT;
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtextextended('tapp-storage:' || NEW.user_id::text || ':' || NEW.tapp_id, 0)
    );

    IF TG_OP = 'UPDATE' THEN
        SELECT COALESCE(SUM(
                   octet_length(key)
                   + octet_length(value::text)
                   + COALESCE(octet_length(encrypted_value), 0)
               ), 0)::BIGINT
          INTO current_bytes
          FROM tapp_storage
         WHERE user_id = NEW.user_id
           AND tapp_id = NEW.tapp_id
           AND id <> OLD.id;
    ELSE
        SELECT COALESCE(SUM(
                   octet_length(key)
                   + octet_length(value::text)
                   + COALESCE(octet_length(encrypted_value), 0)
               ), 0)::BIGINT
          INTO current_bytes
          FROM tapp_storage
         WHERE user_id = NEW.user_id
           AND tapp_id = NEW.tapp_id;
    END IF;

    projected_bytes := current_bytes
        + octet_length(NEW.key)
        + octet_length(NEW.value::text)
        + COALESCE(octet_length(NEW.encrypted_value), 0);
    IF projected_bytes > 8388608 THEN
        RAISE EXCEPTION 'Tapp storage quota exceeded: % bytes', projected_bytes
            USING ERRCODE = '54000';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_tapp_storage_quota ON tapp_storage;
CREATE TRIGGER trg_tapp_storage_quota
BEFORE INSERT OR UPDATE OF key, value, encrypted_value, user_id, tapp_id ON tapp_storage
FOR EACH ROW EXECUTE FUNCTION enforce_tapp_storage_quota();
"#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TRIGGER IF EXISTS trg_tapp_storage_quota ON tapp_storage; DROP FUNCTION IF EXISTS enforce_tapp_storage_quota(); DROP TABLE IF EXISTS tapp_runtime_mailbox; DROP TABLE IF EXISTS tapp_runtime_registry;",
            )
            .await?;
        manager
            .drop_table(Table::drop().table(TappUserActivities::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TappTaskExecutions::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TappScheduledTasks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TappStoreSources::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TappQuotaUsage::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TappStorage::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TappWidgets::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Tapps::Table).to_owned())
            .await?;

        Ok(())
    }
}

// ==================== 表定义枚举 ====================

#[derive(DeriveIden)]
enum Tapps {
    Table,
    Id,
    TappId,
    UserId,
    Name,
    Version,
    Description,
    Author,
    Icon,
    ThemeColor,
    Manifest,
    Status,
    GrantedPermissions,
    ApprovedPermissions,
    NeedsReauthorization,
    FilePath,
    CodePath,
    InstalledAt,
    LastRunAt,
    UpdatedAt,
    ErrorMessage,
    Visibility,
}

#[derive(DeriveIden)]
enum TappWidgets {
    Table,
    Id,
    WidgetId,
    TappId,
    UserId,
    Name,
    Description,
    Icon,
    DefaultSize,
    Sizes,
    Category,
    Config,
    RegisteredAt,
}

#[derive(DeriveIden)]
enum TappStorage {
    Table,
    Id,
    TappId,
    UserId,
    Key,
    Value,
    EncryptedValue,
    BindingFingerprint,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TappQuotaUsage {
    Table,
    Id,
    TappId,
    UserId,
    QuotaType,
    Used,
    Limit,
    PeriodStart,
    PeriodEnd,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TappStoreSources {
    Table,
    Id,
    Name,
    Description,
    Url,
    Enabled,
    Official,
    Icon,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TappScheduledTasks {
    Table,
    Id,
    TaskId,
    TappId,
    UserId,
    Name,
    ScheduleType,
    ScheduleConfig,
    Payload,
    ExecutionTarget,
    BackendActions,
    Enabled,
    MissedPolicy,
    Scope,
    RetryConfig,
    NextRunAt,
    LastRunAt,
    LastRunResult,
    Stats,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TappTaskExecutions {
    Table,
    Id,
    ScheduledTaskId,
    UserId,
    TappId,
    TaskId,
    ScheduledAt,
    ExecutedAt,
    CompletedAt,
    ExecutionTarget,
    Status,
    IsCompensation,
    Result,
    Error,
    DurationMs,
    RetryCount,
}

#[derive(DeriveIden)]
enum TappUserActivities {
    Table,
    Id,
    UserId,
    TappId,
    LastRunAt,
    RunCount,
}
