use sea_orm_migration::prelude::*;

/// Agent 系统数据库结构
///
/// agent_tasks / sessions / messages / task_presets / notifications /
/// heartbeat_claims / persona / addressee_state / diary /
/// proactive_messages / intentions / autonomy_grants。
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ==================== 1. AGENT_TASKS 表 ====================
        // 存储 Agent 任务状态
        manager
            .create_table(
                Table::create()
                    .table(AgentTasks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AgentTasks::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    // 所属用户
                    .col(ColumnDef::new(AgentTasks::UserId).integer().not_null())
                    // 关联的方案 ID
                    .col(
                        ColumnDef::new(AgentTasks::RecipeId)
                            .string_len(64)
                            .not_null(),
                    )
                    // 任务名称
                    .col(ColumnDef::new(AgentTasks::Name).string_len(255))
                    // varchar(32) NOT NULL default pending；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(AgentTasks::Status)
                            .string_len(32)
                            .not_null()
                            .default("pending"),
                    )
                    // 当前步骤索引
                    .col(
                        ColumnDef::new(AgentTasks::CurrentStep)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 总步骤数
                    .col(
                        ColumnDef::new(AgentTasks::TotalSteps)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 步骤执行结果 (JSON)
                    .col(ColumnDef::new(AgentTasks::StepResults).json().not_null())
                    // json，可空
                    .col(ColumnDef::new(AgentTasks::ExecutionContext).json())
                    // jsonb，可空
                    .col(ColumnDef::new(AgentTasks::Recipe).json_binary())
                    // 待回答问题 (JSON)
                    .col(ColumnDef::new(AgentTasks::PendingQuestion).json())
                    // smallint NOT NULL default 0；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(AgentTasks::Progress)
                            .small_integer()
                            .not_null()
                            .default(0),
                    )
                    // 错误信息
                    .col(ColumnDef::new(AgentTasks::Error).text())
                    // 原始用户请求
                    .col(ColumnDef::new(AgentTasks::OriginalRequest).text())
                    // 会话 ID（用于多轮对话）
                    .col(ColumnDef::new(AgentTasks::SessionId).string_len(64))
                    // Lane ID（队列标识）
                    .col(ColumnDef::new(AgentTasks::LaneId).string_len(128))
                    // 开始时间
                    .col(
                        ColumnDef::new(AgentTasks::StartedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // 完成时间
                    .col(ColumnDef::new(AgentTasks::CompletedAt).timestamp_with_time_zone())
                    // 更新时间
                    .col(
                        ColumnDef::new(AgentTasks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // 创建用户索引
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_tasks_user_id")
                    .table(AgentTasks::Table)
                    .col(AgentTasks::UserId)
                    .to_owned(),
            )
            .await?;

        // 创建状态索引
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_tasks_status")
                    .table(AgentTasks::Table)
                    .col(AgentTasks::Status)
                    .to_owned(),
            )
            .await?;

        // 创建会话索引
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_tasks_session_id")
                    .table(AgentTasks::Table)
                    .col(AgentTasks::SessionId)
                    .to_owned(),
            )
            .await?;

        // 创建更新时间索引（用于清理过期任务）
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_tasks_updated_at")
                    .table(AgentTasks::Table)
                    .col(AgentTasks::UpdatedAt)
                    .to_owned(),
            )
            .await?;

        // ==================== 2. AGENT_SESSIONS 表 ====================
        // 存储 Agent 会话（多轮对话）
        manager
            .create_table(
                Table::create()
                    .table(AgentSessions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AgentSessions::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    // 所属用户
                    .col(ColumnDef::new(AgentSessions::UserId).integer().not_null())
                    // varchar(255)，可空
                    .col(ColumnDef::new(AgentSessions::Title).string_len(255))
                    // json，可空
                    .col(ColumnDef::new(AgentSessions::Context).json())
                    // 消息数量
                    .col(
                        ColumnDef::new(AgentSessions::MessageCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    // 是否归档
                    .col(
                        ColumnDef::new(AgentSessions::Archived)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 创建时间
                    .col(
                        ColumnDef::new(AgentSessions::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // 最后活跃时间
                    .col(
                        ColumnDef::new(AgentSessions::LastActiveAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // 创建用户会话索引
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_sessions_user_id")
                    .table(AgentSessions::Table)
                    .col(AgentSessions::UserId)
                    .to_owned(),
            )
            .await?;

        // ==================== 3. AGENT_MESSAGES 表 ====================
        // 存储会话消息历史
        manager
            .create_table(
                Table::create()
                    .table(AgentMessages::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AgentMessages::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 所属会话
                    .col(
                        ColumnDef::new(AgentMessages::SessionId)
                            .string_len(64)
                            .not_null(),
                    )
                    // 关联任务（可选）
                    .col(ColumnDef::new(AgentMessages::TaskId).string_len(64))
                    // varchar(16) NOT NULL；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(AgentMessages::Role)
                            .string_len(16)
                            .not_null(),
                    )
                    // 消息内容
                    .col(ColumnDef::new(AgentMessages::Content).text().not_null())
                    // json，可空
                    .col(ColumnDef::new(AgentMessages::Metadata).json())
                    // 创建时间
                    .col(
                        ColumnDef::new(AgentMessages::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // 创建会话消息索引
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_messages_session_id")
                    .table(AgentMessages::Table)
                    .col(AgentMessages::SessionId)
                    .to_owned(),
            )
            .await?;

        // ==================== 4. AGENT_TASK_PRESETS 表 ====================
        // 存储 agent_task_presets
        manager
            .create_table(
                Table::create()
                    .table(AgentTaskPresets::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AgentTaskPresets::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    // 所属用户
                    .col(
                        ColumnDef::new(AgentTaskPresets::UserId)
                            .integer()
                            .not_null(),
                    )
                    // 原始用户输入
                    .col(ColumnDef::new(AgentTaskPresets::Input).text().not_null())
                    // varchar(16) NOT NULL default history；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(AgentTaskPresets::PresetType)
                            .string_len(16)
                            .not_null()
                            .default("history"),
                    )
                    // json，可空
                    .col(ColumnDef::new(AgentTaskPresets::ParsedSteps).json())
                    // 解析后的意图摘要
                    .col(ColumnDef::new(AgentTaskPresets::IntentSummary).string_len(255))
                    // varchar(255)，可空
                    .col(ColumnDef::new(AgentTaskPresets::Title).string_len(255))
                    // json，可空
                    .col(ColumnDef::new(AgentTaskPresets::ConversationData).json())
                    // 最后使用时间
                    .col(
                        ColumnDef::new(AgentTaskPresets::LastUsedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // 使用次数
                    .col(
                        ColumnDef::new(AgentTaskPresets::UseCount)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    // 创建时间
                    .col(
                        ColumnDef::new(AgentTaskPresets::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // 创建用户+类型索引（用于查询收藏/历史）
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_task_presets_user_type")
                    .table(AgentTaskPresets::Table)
                    .col(AgentTaskPresets::UserId)
                    .col(AgentTaskPresets::PresetType)
                    .to_owned(),
            )
            .await?;

        // 创建用户+输入唯一索引（防止重复）
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_task_presets_user_input")
                    .table(AgentTaskPresets::Table)
                    .col(AgentTaskPresets::UserId)
                    .col(AgentTaskPresets::Input)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // 创建最后使用时间索引（用于排序和清理）
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_task_presets_last_used")
                    .table(AgentTaskPresets::Table)
                    .col(AgentTaskPresets::LastUsedAt)
                    .to_owned(),
            )
            .await?;

        // ==================== agent_notifications 表 ====================
        manager
            .create_table(
                Table::create()
                    .table(AgentNotifications::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AgentNotifications::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    // varchar(32) NOT NULL；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(AgentNotifications::NotificationType)
                            .string_len(32)
                            .not_null(),
                    )
                    // varchar(16) NOT NULL default normal；本 migration 无 CHECK
                    .col(
                        ColumnDef::new(AgentNotifications::Priority)
                            .string_len(16)
                            .not_null()
                            .default("normal"),
                    )
                    .col(ColumnDef::new(AgentNotifications::Title).text().not_null())
                    .col(ColumnDef::new(AgentNotifications::Body).text().not_null())
                    // integer，可空
                    .col(ColumnDef::new(AgentNotifications::UserId).integer())
                    .col(ColumnDef::new(AgentNotifications::Metadata).json())
                    .col(
                        ColumnDef::new(AgentNotifications::Read)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(AgentNotifications::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // 创建时间索引（历史列表按时间倒序 + 过期清理）
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_notifications_created_at")
                    .table(AgentNotifications::Table)
                    .col(AgentNotifications::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 用户索引（按用户过滤历史/未读数）
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_agent_notifications_user_id")
                    .table(AgentNotifications::Table)
                    .col(AgentNotifications::UserId)
                    .to_owned(),
            )
            .await?;

        // ==================== HEARTBEAT_CLAIMS ====================
        // 多副本 heartbeat 分钟桶认领；与 schema_check::ensure_heartbeat_claims_table 同结构
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS heartbeat_claims (
    task_id VARCHAR(128) NOT NULL,
    minute_bucket BIGINT NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'running',
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    PRIMARY KEY (task_id, minute_bucket)
);
CREATE INDEX IF NOT EXISTS idx_heartbeat_claims_claimed_at
    ON heartbeat_claims (claimed_at);
"#,
            )
            .await?;

        // Merope 设定 / 状态 / 日记 / 主动对话。与 schema_check::tables_agent 同结构。
        // 不进 agent_sessions / agent_messages，会话列表才不会露出主动开口。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS agent_persona (
    id VARCHAR(16) PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    personality TEXT NOT NULL DEFAULT '',
    persona_json JSONB,
    visual_profile JSONB,
    portrait_asset_id TEXT,
    portrait_generation JSONB,
    avatar_asset_id TEXT,
    avatar_generation JSONB,
    updated_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_addressee_state (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    mood DOUBLE PRECISION NOT NULL DEFAULT 70,
    arousal DOUBLE PRECISION NOT NULL DEFAULT 50,
    emotion DOUBLE PRECISION NOT NULL DEFAULT 50,
    emotion_arousal DOUBLE PRECISION NOT NULL DEFAULT 50,
    activity VARCHAR(16) NOT NULL DEFAULT 'idle',
    activity_updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    do_not_disturb BOOLEAN NOT NULL DEFAULT false,
    dnd_start_minute INTEGER,
    dnd_end_minute INTEGER,
    last_user_message_at TIMESTAMPTZ,
    last_proactive_at TIMESTAMPTZ,
    music_mood_credited_at TIMESTAMPTZ,
    mood_settled_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    emotion_settled_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_diary (
    id VARCHAR(64) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    source VARCHAR(16) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_diary_user_created
    ON agent_diary (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS agent_proactive_messages (
    id BIGSERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(16) NOT NULL,
    content TEXT NOT NULL,
    event_key VARCHAR(64),
    notified BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_proactive_user_created
    ON agent_proactive_messages (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS agent_intentions (
    id VARCHAR(64) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_event_id VARCHAR(128) NOT NULL,
    summary TEXT NOT NULL,
    reason_code VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'proposed',
    proposal JSONB NOT NULL,
    work_session_id VARCHAR(64),
    work_run_id VARCHAR(64),
    result_summary TEXT,
    expires_at TIMESTAMPTZ,
    accept_source VARCHAR(16) NOT NULL DEFAULT 'user',
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_intentions_user_status
    ON agent_intentions (user_id, status);
CREATE INDEX IF NOT EXISTS idx_agent_intentions_user_updated
    ON agent_intentions (user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_intentions_source_event
    ON agent_intentions (source_event_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_intentions_user_source_event
    ON agent_intentions (user_id, source_event_id);

CREATE TABLE IF NOT EXISTS agent_autonomy_grants (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    allowed_permissions JSONB NOT NULL DEFAULT '[]'::jsonb,
    revoked BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
"#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
DROP TABLE IF EXISTS agent_autonomy_grants;
DROP TABLE IF EXISTS agent_proactive_messages;
DROP TABLE IF EXISTS agent_intentions;
DROP TABLE IF EXISTS agent_diary;
DROP TABLE IF EXISTS agent_addressee_state;
DROP TABLE IF EXISTS agent_persona;
DROP TABLE IF EXISTS heartbeat_claims;
"#,
            )
            .await?;
        manager
            .drop_table(Table::drop().table(AgentNotifications::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(AgentTaskPresets::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(AgentMessages::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(AgentSessions::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(AgentTasks::Table).to_owned())
            .await?;

        Ok(())
    }
}

// ==================== 表定义 ====================

#[derive(Iden)]
pub enum AgentTasks {
    Table,
    Id,
    UserId,
    RecipeId,
    Name,
    Status,
    CurrentStep,
    TotalSteps,
    StepResults,
    ExecutionContext,
    Recipe,
    PendingQuestion,
    Progress,
    Error,
    OriginalRequest,
    SessionId,
    LaneId,
    StartedAt,
    CompletedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum AgentSessions {
    Table,
    Id,
    UserId,
    Title,
    Context,
    MessageCount,
    Archived,
    CreatedAt,
    LastActiveAt,
}

#[derive(Iden)]
pub enum AgentMessages {
    Table,
    Id,
    SessionId,
    TaskId,
    Role,
    Content,
    Metadata,
    CreatedAt,
}

#[derive(Iden)]
pub enum AgentNotifications {
    Table,
    Id,
    NotificationType,
    Priority,
    Title,
    Body,
    UserId,
    Metadata,
    Read,
    CreatedAt,
}

#[derive(Iden)]
pub enum AgentTaskPresets {
    Table,
    Id,
    UserId,
    Input,
    PresetType,
    ParsedSteps,
    IntentSummary,
    Title,
    ConversationData,
    LastUsedAt,
    UseCount,
    CreatedAt,
}
