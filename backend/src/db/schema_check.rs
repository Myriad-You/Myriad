//! 数据库 Schema 自动补全模块
//!
//! 通过解析迁移文件定义的期望结构，与数据库实际结构比对，
//! 自动补全缺失的字段和索引。
//!
//! 使用版本标记记录已部署的结构基线；为修复不完整升级，每次启动仍会安全比对。
//! 当需要新的 schema 变更时，应同步递增 SCHEMA_VERSION 常量。

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};
use std::collections::HashSet;

/// Schema 版本号
///
/// 修改此版本号用于记录新的结构基线；schema 安全比对本身会在每次启动执行。
/// 格式建议：YYYY.MM.DD 或语义版本 X.Y.Z
///
/// 变更日志：
/// - 2026.07.19.3: 删掉过期升级补齐（approved_permissions heal、整表 create 兜底）；只留权威结构列表 + 持续机制
/// - 2026.07.19.2: 去掉 approved_permissions 专用 ADD COLUMN；缺列走 get_expected_schema
/// - 2026.07.19.1: 退休 007–011 薄 ALTER 迁移；列并入 001/002 CREATE
/// - 2026.07.18.2: owner implies admin（ensure_single_owner）
/// - 2026.07.18.1: users.is_owner 站点 owner 标记
/// - 2026.07.17.1: tapp_ai_cost_ledger 表与索引
/// - 2026.07.11.1: Discord 数据平台种子
/// - 2026.07.10.1: 默认平台种子同步（含 X）
const SCHEMA_VERSION: &str = "2026.07.19.3";

/// 内置平台种子定义（与 migrations/001_initial_schema.rs 中 INSERT 保持同步）
///
/// - **新库**：001 migration 写入这些行
/// - **旧库 / 新增平台**：`ensure_default_platforms` 在启动 schema 检查时补齐缺失行
///
/// 新增平台时请同时改：
/// 1. 本数组
/// 2. `001_initial_schema.rs` 的 INSERT
/// 3. 业务配置层（DynamicConfig / config API / fetcher 等）
#[derive(Debug, Clone, Copy)]
pub struct DefaultPlatformSeed {
    pub name: &'static str,
    pub display_name: &'static str,
    pub icon: &'static str,
    pub api_endpoint: &'static str,
    pub auth_type: &'static str,
    /// 仅种子默认值；用户启用状态由 DynamicConfig / 配置页控制
    pub enabled: bool,
}

/// 返回当前代码期望的默认平台目录（唯一权威列表，供 schema 同步与 API 降级使用）
pub fn default_platform_seeds() -> &'static [DefaultPlatformSeed] {
    &[
        DefaultPlatformSeed {
            name: "github",
            display_name: "GitHub",
            icon: "github",
            api_endpoint: "https://api.github.com",
            auth_type: "token",
            enabled: true,
        },
        DefaultPlatformSeed {
            name: "bilibili",
            display_name: "Bilibili",
            icon: "bilibili",
            api_endpoint: "https://api.bilibili.com",
            auth_type: "uid",
            enabled: false,
        },
        DefaultPlatformSeed {
            name: "steam",
            display_name: "Steam",
            icon: "steam",
            api_endpoint: "https://api.steampowered.com",
            auth_type: "api_key",
            enabled: false,
        },
        DefaultPlatformSeed {
            name: "netease_music",
            display_name: "Netease Music",
            icon: "netease",
            api_endpoint: "https://music.163.com",
            auth_type: "user_id",
            enabled: false,
        },
        DefaultPlatformSeed {
            name: "bangumi",
            display_name: "Bangumi",
            icon: "bangumi",
            api_endpoint: "https://api.bgm.tv",
            auth_type: "access_token",
            enabled: false,
        },
        DefaultPlatformSeed {
            name: "x",
            display_name: "X",
            icon: "x",
            api_endpoint: "https://api.x.com",
            auth_type: "bearer_token",
            enabled: false,
        },
        DefaultPlatformSeed {
            name: "discord",
            display_name: "Discord",
            icon: "discord",
            api_endpoint: "https://discord.com/api/v10",
            auth_type: "oauth",
            enabled: false,
        },
        DefaultPlatformSeed {
            name: "mal",
            display_name: "MyAnimeList",
            icon: "mal",
            api_endpoint: "https://api.myanimelist.net/v2",
            auth_type: "client_id",
            enabled: false,
        },
        DefaultPlatformSeed {
            name: "xbox",
            display_name: "Xbox",
            icon: "xbox",
            api_endpoint: "https://xbl.io/api/v2",
            auth_type: "api_key",
            enabled: false,
        },
        DefaultPlatformSeed {
            name: "psn",
            display_name: "PlayStation",
            icon: "psn",
            api_endpoint: "https://m.np.playstation.com/api",
            auth_type: "npsso",
            enabled: false,
        },
    ]
}

/// 清理已经并入基础结构、代码中不再保留的迁移历史项。
///
/// 必须在 SeaORM 检查迁移状态前调用，否则旧数据库会把已执行但已删除的迁移
/// 判断为历史损坏。这里只删除已由本模块完整接管的迁移名，不改动其他记录。
pub async fn reconcile_retired_migration_history(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF to_regclass('public.seaql_migrations') IS NOT NULL THEN
        DELETE FROM seaql_migrations
         WHERE version IN (
            -- already folded earlier
            '008_tapp_runtime_registry',
            '009_activity_events',
            -- retired thin ALTER-only migrations (covered by 001/002 + schema_check)
            '007_notification_preferences',
            '008_tapp_approved_permissions',
            '009_user_presence',
            '010_user_owner',
            '011_owner_is_admin'
         );
    END IF;
END $$;
"#,
    )
    .await?;

    Ok(())
}

/// 将缺失的默认平台行补入 `platforms` 表（ON CONFLICT DO NOTHING，不覆盖用户已有配置）
///
/// 这是「数据表种子同步」入口：结构由列/索引检查负责，默认业务行由本函数负责。
pub async fn ensure_default_platforms(db: &DatabaseConnection) -> Result<usize, DbErr> {
    // 表不存在则跳过（整表由 Migrator 001 创建；此处只补种子行）
    let existing_tables = get_existing_tables(db).await?;
    if !existing_tables.contains("platforms") {
        tracing::debug!("platforms table missing, skip default platform seed");
        return Ok(0);
    }

    let seeds = default_platform_seeds();
    let mut inserted = 0usize;

    for seed in seeds {
        // 使用参数化 Statement，避免字符串拼接注入；ON CONFLICT (name) DO NOTHING
        let sql = r#"
            INSERT INTO platforms (name, display_name, icon, api_endpoint, auth_type, enabled)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (name) DO NOTHING
        "#;

        let result = db
            .execute(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                sql,
                [
                    seed.name.into(),
                    seed.display_name.into(),
                    seed.icon.into(),
                    seed.api_endpoint.into(),
                    seed.auth_type.into(),
                    seed.enabled.into(),
                ],
            ))
            .await?;

        let rows = result.rows_affected();
        if rows > 0 {
            tracing::info!(
                "📦 Seeded default platform row: {} ({})",
                seed.name,
                seed.display_name
            );
            inserted += rows as usize;
        }
    }

    if inserted > 0 {
        tracing::info!(
            "✅ Default platforms seed: inserted {} missing row(s)",
            inserted
        );
    } else {
        tracing::debug!("Default platforms seed: all rows already present");
    }

    Ok(inserted)
}

/// 列定义
#[derive(Debug, Clone)]
struct ColumnDef {
    name: String,
    data_type: String,
    #[allow(dead_code)]
    is_nullable: bool,
    default_value: Option<String>,
}

/// 表定义
#[derive(Debug, Clone)]
struct TableDef {
    name: String,
    columns: Vec<ColumnDef>,
}

/// 索引定义
#[derive(Debug, Clone)]
struct IndexDef {
    name: String,
    table: String,
    columns: Vec<String>,
    is_unique: bool,
}

/// 获取迁移文件定义的期望表结构
///
/// 这里硬编码了迁移文件中定义的所有表结构
/// 当迁移文件更新时，需要同步更新这里
fn get_expected_schema() -> Vec<TableDef> {
    vec![
        // ==================== platforms 表 ====================
        TableDef {
            name: "platforms".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "display_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "icon".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "api_endpoint".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "auth_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "enabled".into(),
                    data_type: "boolean".into(),
                    is_nullable: true,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== users 表 ====================
        TableDef {
            name: "users".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "github_id".into(),
                    data_type: "bigint".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "username".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "display_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "email".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "avatar_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "github_profile_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "bio".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "location".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "company".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "is_admin".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "last_login_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                // 本地认证字段
                ColumnDef {
                    name: "password_hash".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "auth_provider".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'github'".into()),
                },
                ColumnDef {
                    name: "linked_github_id".into(),
                    data_type: "bigint".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "local_login_disabled".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "notification_preferences".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some("'{}'::jsonb".into()),
                },
                // 在线状态跟踪（schema_check / base 001）
                ColumnDef {
                    name: "last_seen_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "online_seconds".into(),
                    data_type: "bigint".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                // 站点 owner（schema_check / base 001）；was: privilege gates used id=1
                ColumnDef {
                    name: "is_owner".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
            ],
        },
        // ==================== configurations 表 ====================
        TableDef {
            name: "configurations".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "key".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "value".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "description".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "category".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: Some("'general'".into()),
                },
                ColumnDef {
                    name: "is_encrypted".into(),
                    data_type: "boolean".into(),
                    is_nullable: true,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "is_public".into(),
                    data_type: "boolean".into(),
                    is_nullable: true,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== platform_metadata 表 ====================
        TableDef {
            name: "platform_metadata".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "platform_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "raw_data".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "fetched_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== metadata_history 表 ====================
        TableDef {
            name: "metadata_history".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "platform_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "changed_fields".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "old_data".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "new_data".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "change_date".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== activity_events 表 ====================
        TableDef {
            name: "activity_events".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata_history_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "platform_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "event_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "title".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "changes".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some("'[]'::jsonb".into()),
                },
                ColumnDef {
                    name: "change_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "importance".into(),
                    data_type: "smallint".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "occurred_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== platform_reports 表 ====================
        TableDef {
            name: "platform_reports".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "platform".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "report".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "report_title".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "expires_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        // ==================== 002_tapp_system.rs 表 ====================
        // ==================== tapps 表 ====================
        TableDef {
            name: "tapps".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "version".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "description".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "author".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "icon".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "theme_color".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "manifest".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'installed'".into()),
                },
                ColumnDef {
                    name: "granted_permissions".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some("'[]'".into()),
                },
                ColumnDef {
                    name: "approved_permissions".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some("'[]'".into()),
                },
                ColumnDef {
                    name: "file_path".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "code_path".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "installed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "last_run_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "error_message".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== tapp_widgets 表 ====================
        TableDef {
            name: "tapp_widgets".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "widget_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "description".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "icon".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "default_size".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'2x2'".into()),
                },
                ColumnDef {
                    name: "sizes".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some("'[\"2x2\"]'".into()),
                },
                ColumnDef {
                    name: "category".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: Some("'custom'".into()),
                },
                ColumnDef {
                    name: "config".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "registered_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== tapp_storage 表 ====================
        TableDef {
            name: "tapp_storage".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "key".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "value".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== tapp_runtime_registry 表 ====================
        TableDef {
            name: "tapp_runtime_registry".to_string(),
            columns: vec![
                ColumnDef {
                    name: "namespace".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "record_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "subject_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "owner_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "runtime_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "payload".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "expires_at".into(),
                    data_type: "bigint".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
            ],
        },
        // ==================== tapp_runtime_mailbox 表 ====================
        TableDef {
            name: "tapp_runtime_mailbox".to_string(),
            columns: vec![
                ColumnDef {
                    name: "message_id".into(),
                    data_type: "bigint".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "channel".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "runtime_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "payload".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "expires_at".into(),
                    data_type: "bigint".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
            ],
        },
        // ==================== tapp_quota_usage 表 ====================
        TableDef {
            name: "tapp_quota_usage".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "quota_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "used".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "limit".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "period_start".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "period_end".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== tapp_ai_cost_ledger 表 ====================
        TableDef {
            name: "tapp_ai_cost_ledger".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "bigint".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "occurred_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "subject_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "owner_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "task_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "source".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "operation".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "provider".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "model".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "input_tokens".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "output_tokens".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "tokens_estimated".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("true".into()),
                },
                ColumnDef {
                    name: "cost_micro_usd".into(),
                    data_type: "bigint".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "error_code".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== tapp_store_sources 表 ====================
        TableDef {
            name: "tapp_store_sources".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "description".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "url".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "enabled".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("true".into()),
                },
                ColumnDef {
                    name: "official".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "icon".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== tapp_scheduled_tasks 表 ====================
        TableDef {
            name: "tapp_scheduled_tasks".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "task_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "schedule_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "schedule_config".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "payload".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "execution_target".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'frontend'".into()),
                },
                ColumnDef {
                    name: "backend_actions".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "enabled".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("true".into()),
                },
                ColumnDef {
                    name: "missed_policy".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'skip'".into()),
                },
                ColumnDef {
                    name: "scope".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'user'".into()),
                },
                ColumnDef {
                    name: "retry_config".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "next_run_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_run_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_run_result".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "stats".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some(
                        r#"'{"totalRuns":0,"successRuns":0,"failedRuns":0,"missedRuns":0}'"#.into(),
                    ),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== tapp_task_executions 表 ====================
        TableDef {
            name: "tapp_task_executions".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "scheduled_task_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "task_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "scheduled_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "executed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "completed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "execution_target".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'pending'".into()),
                },
                ColumnDef {
                    name: "is_compensation".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "result".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "error".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "duration_ms".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "retry_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
            ],
        },
        // ==================== tapp_user_activities 表 ====================
        TableDef {
            name: "tapp_user_activities".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_run_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "run_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("1".into()),
                },
            ],
        },
        // ==================== 003_brew_system.rs 表 ====================
        // ==================== brew_sources 表 ====================
        TableDef {
            name: "brew_sources".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "url".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "feed_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'rss'".into()),
                },
                ColumnDef {
                    name: "source_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'rss'".into()),
                },
                ColumnDef {
                    name: "category".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "icon".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "description".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "site_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "update_interval".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("30".into()),
                },
                ColumnDef {
                    name: "last_fetched_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_success_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_error".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "error_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "enabled".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("true".into()),
                },
                ColumnDef {
                    name: "item_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "unread_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "card_size".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "theme_color".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "sort_order".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "ai_style_tags".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "extra_config".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "rsshub_route".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "admin_only".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== brew_items 表 ====================
        TableDef {
            name: "brew_items".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "source_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "guid".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "title".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "link".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "summary".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "content".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "author".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "image".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "audio_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "video_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "enclosures".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "categories".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "published_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "fetched_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "word_count".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "reading_time".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "fulltext_fetched".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
            ],
        },
        // ==================== brew_user_states 表 ====================
        TableDef {
            name: "brew_user_states".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "item_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "is_read".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "is_starred".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "read_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "read_progress".into(),
                    data_type: "real".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "starred_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "notes".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== brew_categories 表 ====================
        TableDef {
            name: "brew_categories".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "icon".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "color".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "sort_order".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== brew_annotations 表 ====================
        TableDef {
            name: "brew_annotations".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "item_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "annotation_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'term'".into()),
                },
                ColumnDef {
                    name: "term".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "explanation".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "position".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "context_hint".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== brew_podcasts 表 ====================
        TableDef {
            name: "brew_podcasts".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "item_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "title".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "language".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "dialogues".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "estimated_duration".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== brew_comments 表 ====================
        TableDef {
            name: "brew_comments".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "item_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "selected_text".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "comment".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "start_offset".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "end_offset".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "context_before".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "context_after".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "color".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "is_public".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "parent_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== rsshub_instances 表 ====================
        TableDef {
            name: "rsshub_instances".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "url".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "access_key".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "priority".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("100".into()),
                },
                ColumnDef {
                    name: "enabled".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("true".into()),
                },
                ColumnDef {
                    name: "health_status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'unknown'".into()),
                },
                ColumnDef {
                    name: "last_health_check".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_response_time_ms".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "consecutive_failures".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "total_requests".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "success_requests".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        // ==================== agent_tasks 表 ====================
        TableDef {
            name: "agent_tasks".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "recipe_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'pending'".into()),
                },
                ColumnDef {
                    name: "current_step".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "total_steps".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "step_results".into(),
                    data_type: "json".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "execution_context".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "recipe".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "pending_question".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "progress".into(),
                    data_type: "smallint".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "error".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "original_request".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "session_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "lane_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "started_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "completed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        // ==================== agent_sessions 表 ====================
        TableDef {
            name: "agent_sessions".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "title".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "context".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "message_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "archived".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_active_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        // ==================== agent_messages 表 ====================
        TableDef {
            name: "agent_messages".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "session_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "task_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "role".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "content".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        // ==================== agent_notifications 表 ====================
        TableDef {
            name: "agent_notifications".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "notification_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "priority".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'normal'".into()),
                },
                ColumnDef {
                    name: "title".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "body".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "read".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        // ==================== agent_task_presets 表 ====================
        TableDef {
            name: "agent_task_presets".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "input".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "preset_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'history'".into()),
                },
                ColumnDef {
                    name: "parsed_steps".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "intent_summary".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                // 对话标题（用于继续对话时显示）
                ColumnDef {
                    name: "title".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                // 完整对话记录 (JSON) - 支持「继续对话」模式
                ColumnDef {
                    name: "conversation_data".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_used_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "use_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("1".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_keys 表 ====================
        TableDef {
            name: "federation_keys".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "public_key_pem".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "private_key_encrypted".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "key_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "algorithm".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'RSA-SHA256'".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "rotated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_remote_actors 表 ====================
        TableDef {
            name: "federation_remote_actors".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "actor_url".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "username".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "domain".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "display_name".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "avatar_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "summary".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "inbox_url".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "outbox_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "shared_inbox_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "public_key_pem".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "public_key_id".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "software".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "mfp_version".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_capabilities".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_fetched_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_instances 表 ====================
        TableDef {
            name: "federation_instances".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "domain".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "software".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "software_version".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "mfp_version".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "nodeinfo_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "shared_inbox_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "trust_level".into(),
                    data_type: "smallint".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "is_blocked".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "block_reason".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "total_users".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "active_users_monthly".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "open_registrations".into(),
                    data_type: "boolean".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_capabilities".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_seen_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_success_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "failure_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_follows 表 ====================
        TableDef {
            name: "federation_follows".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "remote_actor_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "direction".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'pending'".into()),
                },
                ColumnDef {
                    name: "activity_id".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "accepted_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_activities 表 ====================
        TableDef {
            name: "federation_activities".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "activity_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "remote_actor_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "activity_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "object_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "object_json".into(),
                    data_type: "json".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "is_local".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("true".into()),
                },
                ColumnDef {
                    name: "published_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "received_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_delivery_queue 表 ====================
        TableDef {
            name: "federation_delivery_queue".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "activity_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "target_inbox".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "target_domain".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'pending'".into()),
                },
                ColumnDef {
                    name: "attempts".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "max_attempts".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("12".into()),
                },
                ColumnDef {
                    name: "last_attempt_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "next_retry_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "error_message".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
            ],
        },
        // ==================== federation_channels 表 ====================
        TableDef {
            name: "federation_channels".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "channel_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "remote_actor_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "channel_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "tapp_id".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'pending'".into()),
                },
                ColumnDef {
                    name: "transport".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'http'".into()),
                },
                ColumnDef {
                    name: "properties".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "initiated_by".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_activity_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "closed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_channel_messages 表 ====================
        TableDef {
            name: "federation_channel_messages".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "channel_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "message_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "sender_actor".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "message_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "payload".into(),
                    data_type: "json".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "reply_to".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "is_encrypted".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
            ],
        },
        // ==================== federation_rooms 表 ====================
        TableDef {
            name: "federation_rooms".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "room_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "description".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "avatar_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "owner_actor".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "home_server".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "governance_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'owner'".into()),
                },
                ColumnDef {
                    name: "governance_config".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "enabled_tapps".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "shared_data_config".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "distribution_strategy".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'fan-out'".into()),
                },
                ColumnDef {
                    name: "max_members".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("50".into()),
                },
                ColumnDef {
                    name: "is_public".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "invite_policy".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'admin-only'".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_room_members 表 ====================
        TableDef {
            name: "federation_room_members".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "room_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "actor_url".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "is_local".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "local_user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "role".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'member'".into()),
                },
                ColumnDef {
                    name: "custom_permissions".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "joined_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "invited_by".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_room_messages 表 ====================
        TableDef {
            name: "federation_room_messages".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "room_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "message_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "sender_actor".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "message_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "payload".into(),
                    data_type: "json".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "thread_id".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "reply_to".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "reactions".into(),
                    data_type: "json".into(),
                    is_nullable: false,
                    default_value: Some("'{}'".into()),
                },
                ColumnDef {
                    name: "is_pinned".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "is_encrypted".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
            ],
        },
        // ==================== federation_ring_memberships 表 ====================
        TableDef {
            name: "federation_ring_memberships".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "ring_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "ring_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "ring_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "gossip_config".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                // Plain json (not jsonb). SQL ops that need jsonb (@>, ||, jsonb_array_elements)
                // must cast: COALESCE(known_peers, '[]'::json)::jsonb — see federation/ring.rs.
                ColumnDef {
                    name: "known_peers".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_sync_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "joined_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
            ],
        },
        // ==================== federation_published_content 表 ====================
        TableDef {
            name: "federation_published_content".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "content_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "content_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "activity_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "visibility".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'public'".into()),
                },
                ColumnDef {
                    name: "published_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
        // ==================== federation_timeline 表 ====================
        TableDef {
            name: "federation_timeline".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "activity_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "remote_actor_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "activity_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "object_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "content_preview".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "content_json".into(),
                    data_type: "json".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "is_read".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "is_bookmarked".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "received_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
            ],
        },
        // ==================== federation_file_transfers 表 ====================
        TableDef {
            name: "federation_file_transfers".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "channel_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "transfer_id".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "filename".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "file_size".into(),
                    data_type: "bigint".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "mime_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "checksum_sha256".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "direction".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'pending'".into()),
                },
                ColumnDef {
                    name: "chunks_total".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "chunks_completed".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "local_path".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("now()".into()),
                },
                ColumnDef {
                    name: "completed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
            ],
        },
    ]
}

/// 获取期望的索引定义
fn get_expected_indexes() -> Vec<IndexDef> {
    vec![
        IndexDef {
            name: "idx_users_github_id".into(),
            table: "users".into(),
            columns: vec!["github_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_users_username".into(),
            table: "users".into(),
            columns: vec!["username".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_users_is_admin".into(),
            table: "users".into(),
            columns: vec!["is_admin".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_linked_github_id".into(),
            table: "users".into(),
            columns: vec!["linked_github_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_users_auth_provider".into(),
            table: "users".into(),
            columns: vec!["auth_provider".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_platform_metadata_user".into(),
            table: "platform_metadata".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_platform_metadata_platform".into(),
            table: "platform_metadata".into(),
            columns: vec!["platform_name".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_metadata_history_user".into(),
            table: "metadata_history".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_metadata_history_metadata".into(),
            table: "metadata_history".into(),
            columns: vec!["metadata_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "activity_events_metadata_history_id_key".into(),
            table: "activity_events".into(),
            columns: vec!["metadata_history_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_activity_events_user_date".into(),
            table: "activity_events".into(),
            columns: vec!["user_id".into(), "occurred_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_activity_events_platform_date".into(),
            table: "activity_events".into(),
            columns: vec!["platform_name".into(), "occurred_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_platform_reports_user_id".into(),
            table: "platform_reports".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_platform_reports_platform".into(),
            table: "platform_reports".into(),
            columns: vec!["platform".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_configurations_category".into(),
            table: "configurations".into(),
            columns: vec!["category".into()],
            is_unique: false,
        },
        // ==================== 002_tapp_system.rs 索引 ====================
        // tapps 索引
        IndexDef {
            name: "idx_tapps_user_tapp_id".into(),
            table: "tapps".into(),
            columns: vec!["user_id".into(), "tapp_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapps_user_id".into(),
            table: "tapps".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapps_status".into(),
            table: "tapps".into(),
            columns: vec!["status".into()],
            is_unique: false,
        },
        // tapp_widgets 索引
        IndexDef {
            name: "idx_tapp_widgets_unique".into(),
            table: "tapp_widgets".into(),
            columns: vec!["user_id".into(), "tapp_id".into(), "widget_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapp_widgets_user_id".into(),
            table: "tapp_widgets".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        // tapp_storage 索引
        IndexDef {
            name: "idx_tapp_storage_unique".into(),
            table: "tapp_storage".into(),
            columns: vec!["user_id".into(), "tapp_id".into(), "key".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapp_storage_user_tapp".into(),
            table: "tapp_storage".into(),
            columns: vec!["user_id".into(), "tapp_id".into()],
            is_unique: false,
        },
        // shared runtime state 索引
        IndexDef {
            name: "idx_tapp_runtime_registry_subject".into(),
            table: "tapp_runtime_registry".into(),
            columns: vec!["namespace".into(), "subject_id".into(), "expires_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_runtime_registry_tapp".into(),
            table: "tapp_runtime_registry".into(),
            columns: vec!["namespace".into(), "tapp_id".into(), "expires_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_runtime_registry_runtime".into(),
            table: "tapp_runtime_registry".into(),
            columns: vec!["namespace".into(), "runtime_id".into(), "expires_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_runtime_mailbox_recipient".into(),
            table: "tapp_runtime_mailbox".into(),
            columns: vec!["channel".into(), "runtime_id".into(), "message_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_runtime_mailbox_expiry".into(),
            table: "tapp_runtime_mailbox".into(),
            columns: vec!["expires_at".into()],
            is_unique: false,
        },
        // tapp_quota_usage 索引
        IndexDef {
            name: "idx_tapp_quota_unique".into(),
            table: "tapp_quota_usage".into(),
            columns: vec![
                "user_id".into(),
                "tapp_id".into(),
                "quota_type".into(),
                "period_start".into(),
            ],
            is_unique: true,
        },
        // tapp_ai_cost_ledger 索引
        IndexDef {
            name: "idx_tapp_ai_cost_subject_time".into(),
            table: "tapp_ai_cost_ledger".into(),
            columns: vec!["subject_id".into(), "occurred_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_ai_cost_tapp_time".into(),
            table: "tapp_ai_cost_ledger".into(),
            columns: vec!["tapp_id".into(), "occurred_at".into()],
            is_unique: false,
        },
        // tapp_store_sources 索引
        IndexDef {
            name: "idx_tapp_store_sources_url".into(),
            table: "tapp_store_sources".into(),
            columns: vec!["url".into()],
            is_unique: true,
        },
        // tapp_scheduled_tasks 索引
        IndexDef {
            name: "idx_tapp_scheduled_tasks_unique".into(),
            table: "tapp_scheduled_tasks".into(),
            columns: vec!["user_id".into(), "tapp_id".into(), "task_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapp_scheduled_tasks_user".into(),
            table: "tapp_scheduled_tasks".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_scheduled_tasks_tapp".into(),
            table: "tapp_scheduled_tasks".into(),
            columns: vec!["tapp_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_scheduled_tasks_next_run".into(),
            table: "tapp_scheduled_tasks".into(),
            columns: vec!["enabled".into(), "next_run_at".into()],
            is_unique: false,
        },
        // tapp_task_executions 索引
        IndexDef {
            name: "idx_tapp_task_executions_task".into(),
            table: "tapp_task_executions".into(),
            columns: vec!["scheduled_task_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_task_executions_user_tapp".into(),
            table: "tapp_task_executions".into(),
            columns: vec!["user_id".into(), "tapp_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_task_executions_executed_at".into(),
            table: "tapp_task_executions".into(),
            columns: vec!["executed_at".into()],
            is_unique: false,
        },
        // tapp_user_activities 索引
        IndexDef {
            name: "idx_tapp_user_activities_unique".into(),
            table: "tapp_user_activities".into(),
            columns: vec!["user_id".into(), "tapp_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapp_user_activities_user_last_run".into(),
            table: "tapp_user_activities".into(),
            columns: vec!["user_id".into(), "last_run_at".into()],
            is_unique: false,
        },
        // ==================== 003_brew_system.rs 索引 ====================
        // brew_sources 索引
        IndexDef {
            name: "idx_brew_sources_user_id".into(),
            table: "brew_sources".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_sources_user_url".into(),
            table: "brew_sources".into(),
            columns: vec!["user_id".into(), "url".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_brew_sources_category".into(),
            table: "brew_sources".into(),
            columns: vec!["user_id".into(), "category".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_sources_schedule".into(),
            table: "brew_sources".into(),
            columns: vec!["enabled".into(), "last_fetched_at".into()],
            is_unique: false,
        },
        // brew_items 索引
        IndexDef {
            name: "idx_brew_items_source_guid".into(),
            table: "brew_items".into(),
            columns: vec!["source_id".into(), "guid".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_brew_items_published".into(),
            table: "brew_items".into(),
            columns: vec!["source_id".into(), "published_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_items_timeline".into(),
            table: "brew_items".into(),
            columns: vec!["published_at".into()],
            is_unique: false,
        },
        // brew_user_states 索引
        IndexDef {
            name: "idx_brew_user_states_unique".into(),
            table: "brew_user_states".into(),
            columns: vec!["user_id".into(), "item_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_brew_user_states_unread".into(),
            table: "brew_user_states".into(),
            columns: vec!["user_id".into(), "is_read".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_user_states_starred".into(),
            table: "brew_user_states".into(),
            columns: vec!["user_id".into(), "is_starred".into()],
            is_unique: false,
        },
        // brew_categories 索引
        IndexDef {
            name: "idx_brew_categories_unique".into(),
            table: "brew_categories".into(),
            columns: vec!["user_id".into(), "name".into()],
            is_unique: true,
        },
        // brew_annotations 索引
        IndexDef {
            name: "idx_brew_annotations_item".into(),
            table: "brew_annotations".into(),
            columns: vec!["item_id".into()],
            is_unique: false,
        },
        // brew_podcasts 索引
        IndexDef {
            name: "idx_brew_podcasts_item".into(),
            table: "brew_podcasts".into(),
            columns: vec!["item_id".into()],
            is_unique: true, // 每篇文章只有一个播客
        },
        // brew_comments 索引
        IndexDef {
            name: "idx_brew_comments_item".into(),
            table: "brew_comments".into(),
            columns: vec!["item_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_comments_user".into(),
            table: "brew_comments".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_comments_item_user".into(),
            table: "brew_comments".into(),
            columns: vec!["item_id".into(), "user_id".into()],
            is_unique: false,
        },
        // rsshub_instances 索引
        IndexDef {
            name: "idx_rsshub_instances_user_url".into(),
            table: "rsshub_instances".into(),
            columns: vec!["user_id".into(), "url".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_rsshub_instances_priority".into(),
            table: "rsshub_instances".into(),
            columns: vec!["user_id".into(), "enabled".into(), "priority".into()],
            is_unique: false,
        },
        // agent_tasks 索引
        IndexDef {
            name: "idx_agent_tasks_user_id".into(),
            table: "agent_tasks".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_tasks_status".into(),
            table: "agent_tasks".into(),
            columns: vec!["status".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_tasks_session_id".into(),
            table: "agent_tasks".into(),
            columns: vec!["session_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_tasks_updated_at".into(),
            table: "agent_tasks".into(),
            columns: vec!["updated_at".into()],
            is_unique: false,
        },
        // agent_sessions 索引
        IndexDef {
            name: "idx_agent_sessions_user_id".into(),
            table: "agent_sessions".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        // agent_messages 索引
        IndexDef {
            name: "idx_agent_messages_session_id".into(),
            table: "agent_messages".into(),
            columns: vec!["session_id".into()],
            is_unique: false,
        },
        // agent_notifications 索引
        IndexDef {
            name: "idx_agent_notifications_created_at".into(),
            table: "agent_notifications".into(),
            columns: vec!["created_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_notifications_user_id".into(),
            table: "agent_notifications".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        // agent_task_presets 索引
        IndexDef {
            name: "idx_agent_task_presets_user_type".into(),
            table: "agent_task_presets".into(),
            columns: vec!["user_id".into(), "preset_type".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_task_presets_user_input".into(),
            table: "agent_task_presets".into(),
            columns: vec!["user_id".into(), "input".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_agent_task_presets_last_used".into(),
            table: "agent_task_presets".into(),
            columns: vec!["last_used_at".into()],
            is_unique: false,
        },
        // ==================== federation 索引 ====================
        IndexDef {
            name: "idx_remote_actors_domain".into(),
            table: "federation_remote_actors".into(),
            columns: vec!["domain".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_follows_user".into(),
            table: "federation_follows".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_follows_direction_status".into(),
            table: "federation_follows".into(),
            columns: vec!["direction".into(), "status".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_follows_unique".into(),
            table: "federation_follows".into(),
            columns: vec![
                "user_id".into(),
                "remote_actor_id".into(),
                "direction".into(),
            ],
            is_unique: true,
        },
        IndexDef {
            name: "idx_activities_user".into(),
            table: "federation_activities".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_activities_type".into(),
            table: "federation_activities".into(),
            columns: vec!["activity_type".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_activities_published".into(),
            table: "federation_activities".into(),
            columns: vec!["published_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_delivery_pending".into(),
            table: "federation_delivery_queue".into(),
            columns: vec!["status".into(), "next_retry_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_channels_user".into(),
            table: "federation_channels".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_channels_status".into(),
            table: "federation_channels".into(),
            columns: vec!["status".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_channel_msgs_channel".into(),
            table: "federation_channel_messages".into(),
            columns: vec!["channel_id".into(), "created_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_room_members_room".into(),
            table: "federation_room_members".into(),
            columns: vec!["room_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_room_members_unique".into(),
            table: "federation_room_members".into(),
            columns: vec!["room_id".into(), "actor_url".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_room_msgs_room".into(),
            table: "federation_room_messages".into(),
            columns: vec!["room_id".into(), "created_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_room_msgs_thread".into(),
            table: "federation_room_messages".into(),
            columns: vec!["thread_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_published_user_type".into(),
            table: "federation_published_content".into(),
            columns: vec!["user_id".into(), "content_type".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_published_content_unique".into(),
            table: "federation_published_content".into(),
            columns: vec!["content_type".into(), "content_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_timeline_user_received".into(),
            table: "federation_timeline".into(),
            columns: vec!["user_id".into(), "received_at".into()],
            is_unique: false,
        },
    ]
}

/// Whole-table CREATE fallbacks were removed (2026.07.19.3).
/// Tables come from Migrator 001–006; schema_check only reconciles missing
/// columns/indexes on tables that already exist, plus ongoing data/object heals.

/// 确保存储配额函数和触发器存在。
///
/// 函数使用 `CREATE OR REPLACE` 保持逻辑最新；触发器仅在缺失时创建，
/// 避免每次启动都重建对象。
async fn ensure_tapp_storage_quota(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
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
        SELECT COALESCE(SUM(octet_length(key) + octet_length(value::text)), 0)::BIGINT
          INTO current_bytes
          FROM tapp_storage
         WHERE user_id = NEW.user_id
           AND tapp_id = NEW.tapp_id
           AND id <> OLD.id;
    ELSE
        SELECT COALESCE(SUM(octet_length(key) + octet_length(value::text)), 0)::BIGINT
          INTO current_bytes
          FROM tapp_storage
         WHERE user_id = NEW.user_id
           AND tapp_id = NEW.tapp_id;
    END IF;

    projected_bytes := current_bytes
        + octet_length(NEW.key)
        + octet_length(NEW.value::text);
    IF projected_bytes > 5242880 THEN
        RAISE EXCEPTION 'Tapp storage quota exceeded: % bytes', projected_bytes
            USING ERRCODE = '54000';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM pg_trigger
         WHERE tgname = 'trg_tapp_storage_quota'
           AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER trg_tapp_storage_quota
        BEFORE INSERT OR UPDATE OF key, value, user_id, tapp_id ON tapp_storage
        FOR EACH ROW EXECUTE FUNCTION enforce_tapp_storage_quota();
    END IF;
END $$;
"#,
    )
    .await?;

    Ok(())
}

/// 从数据库获取表的实际列
async fn get_table_columns(
    db: &DatabaseConnection,
    table_name: &str,
) -> Result<HashSet<String>, DbErr> {
    // 安全检查：表名只允许字母、数字、下划线
    if !table_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(DbErr::Custom(format!("Invalid table name: {}", table_name)));
    }

    let sql = format!(
        r#"
        SELECT column_name
        FROM information_schema.columns
        WHERE table_schema = 'public' AND table_name = '{}'
        "#,
        table_name
    );

    let rows = db
        .query_all(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql,
        ))
        .await?;

    let mut columns = HashSet::new();
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "column_name") {
            columns.insert(name);
        }
    }

    Ok(columns)
}

/// 从数据库获取所有表名
async fn get_existing_tables(db: &DatabaseConnection) -> Result<HashSet<String>, DbErr> {
    let sql = r#"
        SELECT table_name
        FROM information_schema.tables
        WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
    "#;

    let rows = db
        .query_all(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await?;

    let mut tables = HashSet::new();
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "table_name") {
            tables.insert(name);
        }
    }

    Ok(tables)
}

/// 从数据库获取现有索引
async fn get_existing_indexes(db: &DatabaseConnection) -> Result<HashSet<String>, DbErr> {
    let sql = r#"
        SELECT indexname
        FROM pg_indexes
        WHERE schemaname = 'public'
    "#;

    let rows = db
        .query_all(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await?;

    let mut indexes = HashSet::new();
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "indexname") {
            indexes.insert(name);
        }
    }

    Ok(indexes)
}

/// 生成 ADD COLUMN DDL
fn generate_add_column_ddl(table: &str, col: &ColumnDef) -> String {
    let mut ddl = format!(
        "ALTER TABLE {} ADD COLUMN IF NOT EXISTS {} {}",
        table, col.name, col.data_type
    );

    if let Some(ref default) = col.default_value {
        ddl.push_str(&format!(" DEFAULT {}", default));
    }

    ddl
}

/// 生成 CREATE INDEX DDL
fn generate_create_index_ddl(idx: &IndexDef) -> String {
    let unique = if idx.is_unique { "UNIQUE " } else { "" };
    let columns = idx.columns.join(", ");
    format!(
        "CREATE {}INDEX IF NOT EXISTS {} ON {}({})",
        unique, idx.name, idx.table, columns
    )
}

/// 检查 schema 版本是否已应用
async fn is_schema_version_applied(db: &DatabaseConnection, version: &str) -> Result<bool, DbErr> {
    // 安全检查：版本号只允许字母、数字、点、下划线、连字符
    if !version
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(DbErr::Custom(format!(
            "Invalid version format: {}",
            version
        )));
    }

    // 先确保版本表存在
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS _schema_versions (
            version VARCHAR(50) PRIMARY KEY,
            applied_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .await?;

    let result = db
        .query_one(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                "SELECT 1 FROM _schema_versions WHERE version = '{}'",
                version
            ),
        ))
        .await?;

    Ok(result.is_some())
}

/// 记录 schema 版本已应用
async fn mark_schema_version_applied(db: &DatabaseConnection, version: &str) -> Result<(), DbErr> {
    // 安全检查：版本号只允许字母、数字、点、下划线、连字符
    if !version
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(DbErr::Custom(format!(
            "Invalid version format: {}",
            version
        )));
    }

    db.execute_unprepared(&format!(
        "INSERT INTO _schema_versions (version) VALUES ('{}') ON CONFLICT (version) DO NOTHING",
        version
    ))
    .await?;

    Ok(())
}

/// 确保数据库 schema 是最新的
///
/// 工作流程：
/// 1. 读取版本标记（仅日志；已标记也会继续做安全比对）
/// 2. 比对期望列/索引并补齐（整表由 Migrator 负责，不再 create 兜底）
/// 3. 运行持续机制：平台种子、owner heal、配额触发器等
/// 4. 记录版本标记
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

    let mut ddl_statements: Vec<String> = Vec::new();
    let mut changes_made = 0;

    // 1. 同步默认平台种子行（表由 Migrator 创建；此处只补业务目录数据）
    match ensure_default_platforms(db).await {
        Ok(n) if n > 0 => changes_made += n,
        Ok(_) => {}
        Err(e) => tracing::warn!("Default platforms seed warning: {}", e),
    }

    // 2. 比对期望列/索引（整表创建已不再由 schema_check 兜底）
    let existing_tables = get_existing_tables(db).await?;
    let expected_tables = get_expected_schema();

    for table_def in &expected_tables {
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
                let ddl = generate_add_column_ddl(&table_def.name, col);
                tracing::info!("📝 Missing column: {}.{}", table_def.name, col.name);
                ddl_statements.push(ddl);
                changes_made += 1;
            }
        }
    }

    // 3. 检查缺失的索引
    let existing_indexes = get_existing_indexes(db).await?;
    let expected_indexes = get_expected_indexes();

    for idx in &expected_indexes {
        if !existing_indexes.contains(&idx.name) {
            if existing_tables.contains(&idx.table) {
                let ddl = generate_create_index_ddl(idx);
                tracing::info!("📝 Missing index: {}", idx.name);
                ddl_statements.push(ddl);
                changes_made += 1;
            }
        }
    }

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
    ensure_tapp_storage_quota(db).await?;
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
        .query_one(sea_orm::Statement::from_sql_and_values(
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

    ensure_tapp_storage_quota(db).await?;
    if let Err(e) = ensure_single_owner(db).await {
        tracing::warn!("Force check: site owner seed warning: {}", e);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expected_schema_tables() {
        let tables = get_expected_schema();
        assert!(!tables.is_empty());

        // 验证关键表存在
        let table_names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        assert!(table_names.contains(&"users"));
        assert!(table_names.contains(&"configurations"));
        assert!(table_names.contains(&"platforms"));
    }

    #[test]
    fn test_users_schema_includes_is_owner() {
        let tables = get_expected_schema();
        let users = tables
            .iter()
            .find(|t| t.name == "users")
            .expect("users table");
        assert!(
            users.columns.iter().any(|c| c.name == "is_owner"),
            "users must define is_owner (schema_check / base 001)"
        );
    }

    #[test]
    fn test_default_platform_seeds_include_x_and_core() {
        let seeds = default_platform_seeds();
        let names: Vec<&str> = seeds.iter().map(|s| s.name).collect();
        for required in [
            "github",
            "bilibili",
            "steam",
            "netease_music",
            "bangumi",
            "x",
            "discord",
            "mal",
        ] {
            assert!(
                names.contains(&required),
                "missing default platform seed: {}",
                required
            );
        }
        // name 唯一
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }

    #[test]
    fn test_tapps_schema_includes_approved_permissions() {
        let tables = get_expected_schema();
        let tapps = tables
            .iter()
            .find(|t| t.name == "tapps")
            .expect("tapps table");
        let col = tapps
            .columns
            .iter()
            .find(|c| c.name == "approved_permissions")
            .expect("tapps.approved_permissions must be in expected schema (002 + generic ADD)");
        assert_eq!(col.data_type, "jsonb");
        assert!(!col.is_nullable);
        assert_eq!(col.default_value.as_deref(), Some("'[]'"));
    }

    #[test]
    fn test_generate_add_column_ddl() {
        let col = ColumnDef {
            name: "test_col".into(),
            data_type: "VARCHAR(255)".into(),
            is_nullable: true,
            default_value: Some("'default'".into()),
        };

        let ddl = generate_add_column_ddl("users", &col);
        assert!(ddl.contains("ALTER TABLE users"));
        assert!(ddl.contains("ADD COLUMN IF NOT EXISTS"));
        assert!(ddl.contains("test_col"));
        assert!(ddl.contains("DEFAULT 'default'"));
    }

    #[test]
    fn test_generate_create_index_ddl() {
        let idx = IndexDef {
            name: "idx_test".into(),
            table: "users".into(),
            columns: vec!["col1".into(), "col2".into()],
            is_unique: true,
        };

        let ddl = generate_create_index_ddl(&idx);
        assert!(ddl.contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
        assert!(ddl.contains("idx_test"));
        assert!(ddl.contains("col1, col2"));
    }
}
