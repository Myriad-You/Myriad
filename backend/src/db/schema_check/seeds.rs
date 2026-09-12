//! Default platform seed catalog and synchronization.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement};
use serde_json::Value;

use super::introspect::get_existing_tables;
use crate::config::DynamicConfig;

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

/// 将缺失的默认平台行补入 `platforms` 表（ON CONFLICT DO NOTHING，不覆盖用户已有配置）
///
/// 这是「数据表种子同步」入口：结构由列/索引检查负责，默认业务行由本函数负责。
pub async fn ensure_default_platforms(db: &DatabaseConnection) -> Result<usize, DbErr> {
    // 整表由 Migrator 001 创建；缺表说明迁移历史/结构不一致，必须阻断 readiness。
    let existing_tables = get_existing_tables(db).await?;
    if !existing_tables.contains("platforms") {
        return Err(DbErr::Custom(
            "platforms table missing after migrations".to_string(),
        ));
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
            .execute_raw(sea_orm::Statement::from_sql_and_values(
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

/// Return runtime configuration seeds except the two default-open permission keys.
///
/// `DynamicConfig::default()` is the value source for keys this function returns.
/// Optional values serialize as JSON null. `user_perm_component_theme` and
/// `user_perm_shortcut_register` are inserted separately in `ensure_default_config`.
pub fn default_config_seeds() -> Vec<(String, Value)> {
    let defaults = serde_json::to_value(DynamicConfig::default())
        .expect("DynamicConfig defaults must remain JSON serializable");
    let explicit_open = ["user_perm_component_theme", "user_perm_shortcut_register"];
    defaults
        .as_object()
        .expect("DynamicConfig must serialize as an object")
        .iter()
        .filter(|(key, _)| !explicit_open.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Seed missing runtime configuration rows without overwriting administrator
/// choices. The two explicitly default-open permissions are inserted first so
/// their upgrade semantics are independent from the general seed pass.
pub async fn ensure_default_config(db: &DatabaseConnection) -> Result<usize, DbErr> {
    let existing_tables = get_existing_tables(db).await?;
    if !existing_tables.contains("configurations") {
        return Err(DbErr::Custom(
            "configurations table missing after migrations".to_string(),
        ));
    }

    let explicit_open = ["user_perm_component_theme", "user_perm_shortcut_register"];
    let mut inserted = 0usize;

    let mut explicit_open_inserted = Vec::new();
    for key in explicit_open {
        let result = db
            .execute_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO configurations (key, value, updated_at) VALUES ($1, $2, CURRENT_TIMESTAMP) ON CONFLICT (key) DO NOTHING",
                vec![key.into(), Value::Bool(true).into()],
            ))
            .await?;
        let rows = result.rows_affected() as usize;
        inserted += rows;
        if rows > 0 {
            explicit_open_inserted.push(key);
        }
    }

    for (key, value) in default_config_seeds() {
        let result = db
            .execute_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO configurations (key, value, updated_at) VALUES ($1, $2, CURRENT_TIMESTAMP) ON CONFLICT (key) DO NOTHING",
                vec![key.into(), value.into()],
            ))
            .await?;
        inserted += result.rows_affected() as usize;
    }

    if inserted > 0 {
        tracing::info!(
            "✅ Runtime configuration seed: inserted {} missing row(s)",
            inserted
        );
    } else {
        tracing::debug!("Runtime configuration seed: all rows already present");
    }
    if !explicit_open_inserted.is_empty() {
        tracing::info!(
            "Upgrade notice: ordinary users now have default granted permissions for component:theme and shortcut:register"
        );
    }

    Ok(inserted)
}
