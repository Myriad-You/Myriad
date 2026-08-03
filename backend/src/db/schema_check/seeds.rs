//! Platform seeds and retired migration history reconciliation.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};

use super::introspect::get_existing_tables;

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
            name: "youtube",
            display_name: "YouTube",
            icon: "youtube",
            api_endpoint: "https://www.googleapis.com/youtube/v3",
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

/// Migration versions whose `.rs` files were deleted after folding into base
/// schema (001–006) and/or `schema_check` heals.
///
/// SeaORM refuses to start when `seaql_migrations` still lists a version with no
/// on-disk file. Startup deletes only these **exact** version strings before
/// `Migrator::up`. Matching is by full name — never by numeric prefix alone —
/// so a future real migration such as `007_something_else` is **not** stripped
/// and can apply normally once history is cleaned.
///
/// Includes:
/// - thin ALTER-only migrations retired in the 007–011 consolidation (≤0.3.9 path;
///   those names existed on upgrade paths that reached field installs)
/// - digital_life experiment versions — **local/dev only, never rolled to production**
///   (product: 「007是有的 但是还没上生产」). Safe to DELETE aggressively.
/// - other deleted mid-series names (`008_tapp_runtime_registry`, `009_activity_events`)
///
/// Repo migrator today is only 001–006; no digital_life files remain on mainline.
pub const RETIRED_MIGRATION_VERSIONS: &[&str] = &[
    // Early / mid-series files removed before the current 001–006 base set
    "008_tapp_runtime_registry",
    "009_activity_events",
    // Retired thin ALTER-only migrations (folded into 001/002 + schema_check)
    "007_notification_preferences",
    "008_tapp_approved_permissions",
    "009_user_presence",
    "010_user_owner",
    "011_owner_is_admin",
    // digital_life experiment — local/dev DBs only (never production). Exact names:
    "007_digital_life",
    "008_digital_life_phase_two",
    "009_digital_life_phase_three",
    "010_digital_life_phase_four",
    "011_digital_life_asset_subjects",
];

/// Temporary digital_life experiment tables (from retired `007_digital_life` and
/// phase migrations). **Local/dev only — never on production.** Product confirmed
/// the feature was temporary; safe to `DROP … CASCADE` aggressively.
///
/// Names taken from local `007_digital_life` (`CREATE TABLE IF NOT EXISTS
/// digital_life_*`). Phase 008–011 sources are not on mainline; any extra
/// `digital_life_%` tables are also dropped by prefix scan.
///
/// Deliberately **excludes** generically named tables that the experiment also
/// created (`image_generation_jobs`, `image_assets`) — only the `digital_life_`
/// prefix is considered unambiguous junk.
pub const RETIRED_DIGITAL_LIFE_TABLES: &[&str] = &[
    "digital_life_characters",
    "digital_life_dna_evidence",
    "digital_life_events",
    "digital_life_memories",
    "digital_life_worlds",
    "digital_life_world_objects",
    "digital_life_visual_lineages",
    "digital_life_relationships",
    "digital_life_visits",
    "digital_life_intents",
    "digital_life_growth_log",
    "digital_life_model_calls",
    "digital_life_asset_recipes",
    "digital_life_visit_receipts",
    "digital_life_item_catalog",
    "digital_life_inventory",
    "digital_life_journal_entries",
    "digital_life_discovery_cache",
    "digital_life_relationship_milestones",
    "digital_life_arcs",
    "digital_life_goals",
    "digital_life_timeline_entries",
    "digital_life_world_evolution",
    "digital_life_visual_reviews",
    "digital_life_social_blocks",
    "digital_life_social_proposals",
];

/// 清理已经并入基础结构、代码中不再保留的迁移历史项，并丢弃临时 digital_life
/// 实验表（产品确认可 DROP，不单删 history）。
///
/// 必须在 SeaORM 检查迁移状态前调用，否则旧数据库会把已执行但已删除的迁移
/// 判断为历史损坏。
///
/// 列表见 [`RETIRED_MIGRATION_VERSIONS`] / [`RETIRED_DIGITAL_LIFE_TABLES`]。
/// ≥0.3.10 连续升级通常已无这些行；保留删除以兼容跳版本与本地 digital_life 实验库。
pub async fn reconcile_retired_migration_history(db: &DatabaseConnection) -> Result<(), DbErr> {
    // Build the IN (...) list from the single const so SQL and tests cannot drift.
    let versions_sql = RETIRED_MIGRATION_VERSIONS
        .iter()
        .map(|v| format!("'{v}'"))
        .collect::<Vec<_>>()
        .join(",\n            ");
    let history_sql = format!(
        r#"
DO $$
BEGIN
    IF to_regclass('public.seaql_migrations') IS NOT NULL THEN
        DELETE FROM seaql_migrations
         WHERE version IN (
            {versions_sql}
         );
    END IF;
    -- schema_check marks for the experiment (if any)
    IF to_regclass('public._schema_versions') IS NOT NULL THEN
        DELETE FROM _schema_versions
         WHERE version LIKE 'digital_life%'
            OR version LIKE '%digital_life%';
    END IF;
END $$;
"#
    );

    db.execute_unprepared(&history_sql).await?;

    drop_retired_digital_life_tables(db).await?;

    Ok(())
}

/// Drop leftover temporary digital_life experiment tables and matching types.
///
/// 1. Explicit list from [`RETIRED_DIGITAL_LIFE_TABLES`] (`DROP IF EXISTS … CASCADE`)
/// 2. Prefix scan: any remaining `public.digital_life_%` table
/// 3. Prefix scan: `public` enum/domain/composite types named `digital_life_%`
///
/// Never touches tables outside the `digital_life_` prefix.
async fn drop_retired_digital_life_tables(db: &DatabaseConnection) -> Result<(), DbErr> {
    // Discover first so we can log what we remove (ops visibility on local DBs).
    let existing = discover_digital_life_tables(db).await?;
    if !existing.is_empty() {
        tracing::info!(
            tables = ?existing,
            "Dropping temporary digital_life experiment tables (retired feature cleanup)"
        );
    }

    let explicit_drops = RETIRED_DIGITAL_LIFE_TABLES
        .iter()
        .map(|t| format!("DROP TABLE IF EXISTS public.{t} CASCADE;"))
        .collect::<Vec<_>>()
        .join("\n");

    // Known names + any leftover digital_life_* from phase migrations not in the const.
    // %I quotes identifiers; LIKE escape keeps `_` literal in the prefix filter.
    let sql = format!(
        r#"
DO $$
DECLARE
    r RECORD;
BEGIN
    {explicit_drops}

    FOR r IN
        SELECT tablename
          FROM pg_tables
         WHERE schemaname = 'public'
           AND tablename LIKE 'digital_life\_%' ESCAPE '\'
    LOOP
        EXECUTE format('DROP TABLE IF EXISTS public.%I CASCADE', r.tablename);
    END LOOP;

    -- Freestanding enums/domains/composites left after CASCADE table drops
    FOR r IN
        SELECT t.typname
          FROM pg_type t
          JOIN pg_namespace n ON n.oid = t.typnamespace
         WHERE n.nspname = 'public'
           AND t.typname LIKE 'digital_life\_%' ESCAPE '\'
           AND t.typtype IN ('e', 'd', 'c')
    LOOP
        EXECUTE format('DROP TYPE IF EXISTS public.%I CASCADE', r.typname);
    END LOOP;
END $$;
"#
    );

    db.execute_unprepared(&sql).await?;
    Ok(())
}

/// List `public` tables whose names start with `digital_life_`.
async fn discover_digital_life_tables(db: &DatabaseConnection) -> Result<Vec<String>, DbErr> {
    use sea_orm::{DatabaseBackend, Statement};

    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
            SELECT tablename
              FROM pg_tables
             WHERE schemaname = 'public'
               AND tablename LIKE 'digital_life\_%' ESCAPE '\'
             ORDER BY tablename
            "#
            .to_string(),
        ))
        .await?;

    let mut names = Vec::with_capacity(rows.len());
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "tablename") {
            names.push(name);
        }
    }
    Ok(names)
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


