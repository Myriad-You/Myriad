//! Default platform seed catalog and synchronization.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, Statement};
use serde_json::Value;

use crate::config::DynamicConfig;

/// Runtime permissions whose default delegation is open; seeded with `true`
/// independently of `DynamicConfig::default()`.
const EXPLICIT_OPEN_CONFIG_KEYS: [&str; 2] =
    ["user_perm_component_theme", "user_perm_shortcut_register"];

/// Build `($1, $2, …), ($n, …)` placeholders for a multi-row VALUES list.
/// `trailing` is appended verbatim inside each row after the bound columns.
fn values_placeholders(rows: usize, columns: usize, trailing: &str) -> String {
    let mut sql = String::with_capacity(rows * (columns * 5 + trailing.len() + 4));
    for row in 0..rows {
        if row > 0 {
            sql.push_str(", ");
        }
        sql.push('(');
        for column in 0..columns {
            if column > 0 {
                sql.push_str(", ");
            }
            sql.push('$');
            sql.push_str(&(row * columns + column + 1).to_string());
        }
        sql.push_str(trailing);
        sql.push(')');
    }
    sql
}

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
/// 整表由 Migrator 001 创建；缺表时 INSERT 直接返回数据库错误并阻断 readiness。
pub async fn ensure_default_platforms(db: &DatabaseConnection) -> Result<usize, DbErr> {
    let seeds = default_platform_seeds();
    let mut values = Vec::with_capacity(seeds.len() * 6);
    for seed in seeds {
        values.extend([
            seed.name.into(),
            seed.display_name.into(),
            seed.icon.into(),
            seed.api_endpoint.into(),
            seed.auth_type.into(),
            seed.enabled.into(),
        ]);
    }
    // 单条参数化 multi-row INSERT；ON CONFLICT (name) DO NOTHING，RETURNING 只含真实插入的行
    let sql = format!(
        "INSERT INTO platforms (name, display_name, icon, api_endpoint, auth_type, enabled) \
         VALUES {} ON CONFLICT (name) DO NOTHING RETURNING name, display_name",
        values_placeholders(seeds.len(), 6, "")
    );
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            sql,
            values,
        ))
        .await?;
    let inserted = rows.len();
    for row in &rows {
        let name: String = row.try_get("", "name")?;
        let display_name: String = row.try_get("", "display_name")?;
        tracing::info!("📦 Seeded default platform row: {} ({})", name, display_name);
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
    let Value::Object(defaults) = serde_json::to_value(DynamicConfig::default())
        .expect("DynamicConfig defaults must remain JSON serializable")
    else {
        panic!("DynamicConfig must serialize as an object");
    };
    defaults
        .into_iter()
        .filter(|(key, _)| !EXPLICIT_OPEN_CONFIG_KEYS.contains(&key.as_str()))
        .collect()
}

/// Seed missing runtime configuration rows without overwriting administrator
/// choices. The two explicitly default-open permissions carry a fixed `true`
/// value and are excluded from `default_config_seeds()`, so every key appears
/// exactly once in the single multi-row INSERT.
pub async fn ensure_default_config(db: &DatabaseConnection) -> Result<usize, DbErr> {
    let seeds = default_config_seeds();
    let row_count = EXPLICIT_OPEN_CONFIG_KEYS.len() + seeds.len();
    let mut values = Vec::with_capacity(row_count * 2);
    for key in EXPLICIT_OPEN_CONFIG_KEYS {
        values.extend([key.into(), Value::Bool(true).into()]);
    }
    for (key, value) in seeds {
        values.extend([key.into(), value.into()]);
    }
    // 缺表时 INSERT 直接返回数据库错误；已有管理员设置由 DO NOTHING 保留
    let sql = format!(
        "INSERT INTO configurations (key, value, updated_at) VALUES {} \
         ON CONFLICT (key) DO NOTHING RETURNING key",
        values_placeholders(row_count, 2, ", CURRENT_TIMESTAMP")
    );
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            sql,
            values,
        ))
        .await?;
    let inserted = rows.len();
    let mut explicit_open_inserted = false;
    for row in &rows {
        let key: String = row.try_get("", "key")?;
        explicit_open_inserted |= EXPLICIT_OPEN_CONFIG_KEYS.contains(&key.as_str());
    }

    if inserted > 0 {
        tracing::info!(
            "✅ Runtime configuration seed: inserted {} missing row(s)",
            inserted
        );
    } else {
        tracing::debug!("Runtime configuration seed: all rows already present");
    }
    if explicit_open_inserted {
        tracing::info!(
            "Upgrade notice: ordinary users now have default granted permissions for component:theme and shortcut:register"
        );
    }

    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::values_placeholders;

    #[test]
    fn values_placeholders_number_rows_sequentially() {
        assert_eq!(values_placeholders(1, 2, ""), "($1, $2)");
        assert_eq!(
            values_placeholders(2, 2, ", CURRENT_TIMESTAMP"),
            "($1, $2, CURRENT_TIMESTAMP), ($3, $4, CURRENT_TIMESTAMP)"
        );
    }
}
