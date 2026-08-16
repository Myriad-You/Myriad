use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::sea_query::OnConflict;

/// 初始数据库结构 - 完整统一版本
///
/// 包含所有必要的表，清晰简洁，无历史包袱
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ==================== 1. PLATFORMS 表 ====================
        // 平台定义和配置
        manager
            .create_table(
                Table::create()
                    .table(Platforms::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Platforms::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Platforms::Name)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Platforms::DisplayName).string().not_null())
                    .col(ColumnDef::new(Platforms::Icon).string())
                    .col(ColumnDef::new(Platforms::ApiEndpoint).string())
                    .col(ColumnDef::new(Platforms::AuthType).string())
                    .col(ColumnDef::new(Platforms::Enabled).boolean().default(false))
                    .col(
                        ColumnDef::new(Platforms::CreatedAt)
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Platforms::UpdatedAt)
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 插入默认平台（与 backend/src/db/schema_check.rs::default_platform_seeds 保持同步）
        // 旧库补种走 ensure_default_platforms，勿为单平台再开 migration。
        manager
            .exec_stmt(
                Query::insert()
                    .into_table(Platforms::Table)
                    .columns([
                        Platforms::Name,
                        Platforms::DisplayName,
                        Platforms::Icon,
                        Platforms::ApiEndpoint,
                        Platforms::AuthType,
                        Platforms::Enabled,
                    ])
                    .values_panic([
                        "github".into(),
                        "GitHub".into(),
                        "github".into(),
                        "https://api.github.com".into(),
                        "token".into(),
                        true.into(),
                    ])
                    .values_panic([
                        "bilibili".into(),
                        "Bilibili".into(),
                        "bilibili".into(),
                        "https://api.bilibili.com".into(),
                        "uid".into(),
                        false.into(),
                    ])
                    .values_panic([
                        "steam".into(),
                        "Steam".into(),
                        "steam".into(),
                        "https://api.steampowered.com".into(),
                        "api_key".into(),
                        false.into(),
                    ])
                    .values_panic([
                        "youtube".into(),
                        "YouTube".into(),
                        "youtube".into(),
                        "https://www.googleapis.com/youtube/v3".into(),
                        "api_key".into(),
                        false.into(),
                    ])
                    .values_panic([
                        "netease_music".into(),
                        "Netease Music".into(),
                        "netease".into(),
                        "https://music.163.com".into(),
                        "user_id".into(),
                        false.into(),
                    ])
                    .values_panic([
                        "bangumi".into(),
                        "Bangumi".into(),
                        "bangumi".into(),
                        "https://api.bgm.tv".into(),
                        "access_token".into(),
                        false.into(),
                    ])
                    .values_panic([
                        "x".into(),
                        "X".into(),
                        "x".into(),
                        "https://api.x.com".into(),
                        "bearer_token".into(),
                        false.into(),
                    ])
                    .values_panic([
                        "discord".into(),
                        "Discord".into(),
                        "discord".into(),
                        "https://discord.com/api/v10".into(),
                        "oauth".into(),
                        false.into(),
                    ])
                    .values_panic([
                        "mal".into(),
                        "MyAnimeList".into(),
                        "mal".into(),
                        "https://api.myanimelist.net/v2".into(),
                        "client_id".into(),
                        false.into(),
                    ])
                    .on_conflict(OnConflict::column(Platforms::Name).do_nothing().to_owned())
                    .to_owned(),
            )
            .await?;

        // ==================== 2. USERS 表 ====================
        // 用户账户（含GitHub OAuth + 本地认证）
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Users::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Users::GithubId).big_integer().unique_key()) // nullable for local users
                    .col(ColumnDef::new(Users::Username).string_len(255).not_null())
                    .col(ColumnDef::new(Users::DisplayName).string_len(255))
                    .col(ColumnDef::new(Users::Email).string_len(255))
                    .col(ColumnDef::new(Users::AvatarUrl).text())
                    .col(ColumnDef::new(Users::GithubProfileUrl).text())
                    .col(ColumnDef::new(Users::Bio).text())
                    .col(ColumnDef::new(Users::Location).string_len(255))
                    .col(ColumnDef::new(Users::Company).string_len(255))
                    .col(
                        ColumnDef::new(Users::IsAdmin)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Users::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Users::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(Users::LastLoginAt).timestamp_with_time_zone())
                    // 本地认证字段
                    .col(ColumnDef::new(Users::PasswordHash).string_len(255)) // nullable for GitHub users
                    .col(
                        ColumnDef::new(Users::AuthProvider)
                            .string_len(20)
                            .not_null()
                            .default("github"),
                    )
                    .col(ColumnDef::new(Users::LinkedGithubId).big_integer())
                    .col(
                        ColumnDef::new(Users::LocalLoginDisabled)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 禁止该用户安装新 Tapp（旧库由 schema_check 补列）
                    .col(
                        ColumnDef::new(Users::TappInstallDisabled)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 每用户通知策略（原 007；旧库由 schema_check 补列）
                    .col(
                        ColumnDef::new(Users::NotificationPreferences)
                            .json_binary()
                            .not_null()
                            .default("{}"),
                    )
                    // 列表页应用卡尺寸（tappId → 1x1|2x1）；旧库由 schema_check 补列
                    .col(
                        ColumnDef::new(Users::TappListCardSizes)
                            .json_binary()
                            .not_null()
                            .default("{}"),
                    )
                    // 在线状态跟踪（原 009；旧库由 schema_check 补列）
                    .col(ColumnDef::new(Users::LastSeenAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(Users::OnlineSeconds)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    // 站点 owner（原 010/011；种子与 owner→admin 由 schema_check::ensure_single_owner）
                    .col(
                        ColumnDef::new(Users::IsOwner)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    // 画像源选择（旧库由 schema_check 补列）：
                    // kind = auto | account | identity | platform，NULL/auto 表示沿用隐式优先级；
                    // ref = identity id 或平台名（bilibili/github/…）。
                    //
                    // 解析结果单独存 avatar_resolved_url，**不回写 avatar_url** —— avatar_url 是
                    // "账号"这一来源本身，被覆盖就再也切不回来了。
                    .col(ColumnDef::new(Users::AvatarSourceKind).string_len(20))
                    .col(ColumnDef::new(Users::AvatarSourceRef).string_len(64))
                    .col(ColumnDef::new(Users::AvatarResolvedUrl).text())
                    .col(ColumnDef::new(Users::AvatarUpdatedAt).timestamp_with_time_zone())
                    // 名称/简介文案来源（与画像源独立；旧库由 schema_check 补列）：
                    // kind = auto | account | identity | platform；ref = identity id 或平台键。
                    .col(ColumnDef::new(Users::ProfileTextSourceKind).string_len(20))
                    .col(ColumnDef::new(Users::ProfileTextSourceRef).string_len(64))
                    // JWT session epoch (MYR-005): bump on logout / password change / admin
                    // force-revoke so long-lived tokens fail closed without shortening TTL.
                    // Old DBs get the column via schema_check ADD COLUMN DEFAULT 0.
                    .col(
                        ColumnDef::new(Users::TokenVersion)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        // 创建索引
        manager
            .create_index(
                Index::create()
                    .name("idx_users_github_id")
                    .table(Users::Table)
                    .col(Users::GithubId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_users_username")
                    .table(Users::Table)
                    .col(Users::Username)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_users_is_admin")
                    .table(Users::Table)
                    .col(Users::IsAdmin)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_users_auth_provider")
                    .table(Users::Table)
                    .col(Users::AuthProvider)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_linked_github_id")
                    .table(Users::Table)
                    .col(Users::LinkedGithubId)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // 约束
        manager.get_connection().execute_unprepared(
            "DO $$ BEGIN
                IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'check_auth_provider') THEN
                    ALTER TABLE users ADD CONSTRAINT check_auth_provider CHECK (auth_provider IN ('local', 'github'));
                END IF;
            END $$;"
        ).await?;
        manager.get_connection().execute_unprepared(
            "DO $$ BEGIN
                IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'check_admin_local_only') THEN
                    ALTER TABLE users ADD CONSTRAINT check_admin_local_only CHECK (NOT is_admin OR auth_provider = 'local');
                END IF;
            END $$;"
        ).await?;
        manager.get_connection().execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_local_admin ON users (auth_provider) WHERE auth_provider = 'local'"
        ).await?;

        // ==================== 3. CONFIGURATIONS 表 ====================
        // 系统配置（含AI、平台、OAuth密钥）
        manager
            .create_table(
                Table::create()
                    .table(Configurations::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Configurations::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Configurations::Key)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Configurations::Value).json().not_null())
                    .col(ColumnDef::new(Configurations::Description).text())
                    .col(
                        ColumnDef::new(Configurations::Category)
                            .string()
                            .default("general"),
                    ) // general/ai/platforms/oauth/ui/features
                    .col(
                        ColumnDef::new(Configurations::IsEncrypted)
                            .boolean()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Configurations::IsPublic)
                            .boolean()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Configurations::CreatedAt)
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Configurations::UpdatedAt)
                            .timestamp_with_time_zone()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_configurations_category")
                    .table(Configurations::Table)
                    .col(Configurations::Category)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 4. PLATFORM_METADATA 表 ====================
        // 平台元数据存储
        manager
            .create_table(
                Table::create()
                    .table(PlatformMetadata::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PlatformMetadata::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PlatformMetadata::UserId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PlatformMetadata::PlatformName)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PlatformMetadata::RawData).json().not_null())
                    .col(ColumnDef::new(PlatformMetadata::FetchedAt).timestamp())
                    .col(
                        ColumnDef::new(PlatformMetadata::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PlatformMetadata::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_platform_metadata_user")
                    .table(PlatformMetadata::Table)
                    .col(PlatformMetadata::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_platform_metadata_platform")
                    .table(PlatformMetadata::Table)
                    .col(PlatformMetadata::PlatformName)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 5. METADATA_HISTORY 表 ====================
        // 数据变更历史记录（纯净版，无人设）
        manager
            .create_table(
                Table::create()
                    .table(MetadataHistory::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MetadataHistory::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(MetadataHistory::MetadataId).integer())
                    .col(ColumnDef::new(MetadataHistory::UserId).integer().not_null())
                    .col(
                        ColumnDef::new(MetadataHistory::PlatformName)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MetadataHistory::ChangedFields)
                            .json()
                            .not_null(),
                    )
                    .col(ColumnDef::new(MetadataHistory::OldData).json())
                    .col(ColumnDef::new(MetadataHistory::NewData).json())
                    .col(
                        ColumnDef::new(MetadataHistory::ChangeDate)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_metadata_history_user")
                    .table(MetadataHistory::Table)
                    .col(MetadataHistory::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_metadata_history_metadata")
                    .table(MetadataHistory::Table)
                    .col(MetadataHistory::MetadataId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 6. ACTIVITY_EVENTS 表 ====================
        // 面向用户的数据平台活动事件；原始字段 diff 仍保留在 metadata_history。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS activity_events (
    id SERIAL PRIMARY KEY,
    metadata_history_id INTEGER NOT NULL UNIQUE,
    metadata_id INTEGER,
    user_id INTEGER NOT NULL,
    platform_name VARCHAR(64) NOT NULL,
    event_type VARCHAR(32) NOT NULL,
    title VARCHAR(255) NOT NULL,
    changes JSONB NOT NULL DEFAULT '[]'::jsonb,
    change_count INTEGER NOT NULL DEFAULT 0,
    importance SMALLINT NOT NULL DEFAULT 0,
    occurred_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_activity_events_history
        FOREIGN KEY (metadata_history_id)
        REFERENCES metadata_history(id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_activity_events_user_date
    ON activity_events (user_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_activity_events_platform_date
    ON activity_events (platform_name, occurred_at DESC);
"#,
            )
            .await?;

        // ==================== 7. PLATFORM_REPORTS 表 ====================
        // 报告存储
        manager
            .create_table(
                Table::create()
                    .table(PlatformReports::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PlatformReports::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PlatformReports::UserId).integer().not_null())
                    .col(
                        ColumnDef::new(PlatformReports::Platform)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PlatformReports::Metadata).json().not_null())
                    .col(ColumnDef::new(PlatformReports::Report).json().not_null())
                    .col(ColumnDef::new(PlatformReports::ReportTitle).string())
                    .col(
                        ColumnDef::new(PlatformReports::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(PlatformReports::ExpiresAt)
                            .timestamp()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_platform_reports_user_id")
                    .table(PlatformReports::Table)
                    .col(PlatformReports::UserId)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_platform_reports_platform")
                    .table(PlatformReports::Table)
                    .col(PlatformReports::Platform)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        // ==================== 8. SITE ANALYTICS 表 ====================
        // 第一方访客统计（page / event / referrer / country）
        // 与 schema_check::get_expected_schema + ensure_analytics_tables 同结构。
        // 已跑过旧 001 的库不会重跑本段，靠 ensure_analytics_tables / 补列对齐。
        manager
            .get_connection()
            .execute_unprepared(
                r#"
CREATE TABLE IF NOT EXISTS analytics_page_daily (
    day DATE NOT NULL,
    path TEXT NOT NULL,
    views BIGINT NOT NULL DEFAULT 0,
    unique_visitors BIGINT NOT NULL DEFAULT 0,
    engagement_ms BIGINT NOT NULL DEFAULT 0,
    engaged_views BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, path)
);
CREATE INDEX IF NOT EXISTS idx_analytics_page_daily_day
    ON analytics_page_daily (day);

CREATE TABLE IF NOT EXISTS analytics_visitor_seen (
    day DATE NOT NULL,
    path TEXT NOT NULL,
    visitor_hash VARCHAR(64) NOT NULL,
    -- 到达序号（"你是今天第 N 位访客"）。仅 path = '__site__' 的行有意义；
    -- 0 = 未知（该功能上线前的历史行）。
    ordinal BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, path, visitor_hash)
);
CREATE INDEX IF NOT EXISTS idx_analytics_visitor_seen_day
    ON analytics_visitor_seen (day);

CREATE TABLE IF NOT EXISTS analytics_event_daily (
    day DATE NOT NULL,
    event_name TEXT NOT NULL,
    path TEXT NOT NULL DEFAULT '',
    -- 事件维度：tapp id / 平台 slug / 音乐源 / Brew 源 id 等；无维度时 ''
    target TEXT NOT NULL DEFAULT '',
    count BIGINT NOT NULL DEFAULT 0,
    unique_visitors BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, event_name, path, target)
);
CREATE INDEX IF NOT EXISTS idx_analytics_event_daily_day
    ON analytics_event_daily (day);

CREATE TABLE IF NOT EXISTS analytics_event_visitor (
    day DATE NOT NULL,
    event_name TEXT NOT NULL,
    path TEXT NOT NULL DEFAULT '',
    target TEXT NOT NULL DEFAULT '',
    visitor_hash VARCHAR(64) NOT NULL,
    PRIMARY KEY (day, event_name, path, target, visitor_hash)
);
CREATE INDEX IF NOT EXISTS idx_analytics_event_visitor_day
    ON analytics_event_visitor (day);

CREATE TABLE IF NOT EXISTS analytics_referrer_daily (
    day DATE NOT NULL,
    host TEXT NOT NULL,
    count BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, host)
);
CREATE INDEX IF NOT EXISTS idx_analytics_referrer_daily_day
    ON analytics_referrer_daily (day);

CREATE TABLE IF NOT EXISTS analytics_country_daily (
    day DATE NOT NULL,
    country_code VARCHAR(8) NOT NULL,
    country_name TEXT NOT NULL DEFAULT '',
    views BIGINT NOT NULL DEFAULT 0,
    unique_visitors BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, country_code)
);
CREATE INDEX IF NOT EXISTS idx_analytics_country_daily_day
    ON analytics_country_daily (day);

CREATE TABLE IF NOT EXISTS analytics_country_visitor (
    day DATE NOT NULL,
    country_code VARCHAR(8) NOT NULL,
    visitor_hash VARCHAR(64) NOT NULL,
    PRIMARY KEY (day, country_code, visitor_hash)
);
CREATE INDEX IF NOT EXISTS idx_analytics_country_visitor_day
    ON analytics_country_visitor (day);
"#,
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 删除所有表（按依赖顺序反向）
        manager
            .get_connection()
            .execute_unprepared(
                r#"
DROP TABLE IF EXISTS analytics_country_visitor;
DROP TABLE IF EXISTS analytics_country_daily;
DROP TABLE IF EXISTS analytics_referrer_daily;
DROP TABLE IF EXISTS analytics_event_visitor;
DROP TABLE IF EXISTS analytics_event_daily;
DROP TABLE IF EXISTS analytics_visitor_seen;
DROP TABLE IF EXISTS analytics_page_daily;
"#,
            )
            .await?;
        manager
            .drop_table(Table::drop().table(PlatformReports::Table).to_owned())
            .await?;
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS activity_events;")
            .await?;
        manager
            .drop_table(Table::drop().table(MetadataHistory::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PlatformMetadata::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Configurations::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Platforms::Table).to_owned())
            .await?;

        Ok(())
    }
}

// ==================== 表定义枚举 ====================

#[derive(DeriveIden)]
enum Platforms {
    Table,
    Id,
    Name,
    DisplayName,
    Icon,
    ApiEndpoint,
    AuthType,
    Enabled,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    GithubId,
    Username,
    DisplayName,
    Email,
    AvatarUrl,
    GithubProfileUrl,
    Bio,
    Location,
    Company,
    IsAdmin,
    CreatedAt,
    UpdatedAt,
    LastLoginAt,
    PasswordHash,
    AuthProvider,
    LinkedGithubId,
    LocalLoginDisabled,
    TappInstallDisabled,
    NotificationPreferences,
    TappListCardSizes,
    LastSeenAt,
    OnlineSeconds,
    IsOwner,
    AvatarSourceKind,
    AvatarSourceRef,
    AvatarResolvedUrl,
    AvatarUpdatedAt,
    ProfileTextSourceKind,
    ProfileTextSourceRef,
    TokenVersion,
}

#[derive(DeriveIden)]
enum Configurations {
    Table,
    Id,
    Key,
    Value,
    Description,
    Category,
    IsEncrypted,
    IsPublic,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum PlatformMetadata {
    Table,
    Id,
    UserId,
    PlatformName,
    RawData,
    FetchedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum MetadataHistory {
    Table,
    Id,
    MetadataId,
    UserId,
    PlatformName,
    ChangedFields,
    OldData,
    NewData,
    ChangeDate,
}

#[derive(DeriveIden)]
enum PlatformReports {
    Table,
    Id,
    UserId,
    Platform,
    Metadata,
    Report,
    ReportTitle,
    CreatedAt,
    ExpiresAt,
}
