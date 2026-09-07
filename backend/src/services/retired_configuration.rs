//! Retired `configurations` keys: backup ignore list and startup purge.
//!
//! Exact names and prefixes stay in one place so export/restore and the
//! startup DELETE cannot drift. Prefixes are written so `github_client_*`
//! cannot match live `github_token`.

use anyhow::Context;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

const RETIRED_CONFIGURATION_KEYS: &[&str] = &["ui_wallpaper_parallax", "github_redirect_url"];

const RETIRED_CONFIGURATION_PREFIXES: &[&str] = &["pet_", "github_client_"];

pub(crate) fn is_retired_configuration_key(key: &str) -> bool {
    RETIRED_CONFIGURATION_KEYS.contains(&key)
        || RETIRED_CONFIGURATION_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

/// 启动时删掉已下线配置行。幂等：没有行就是 0。
///
/// `parse_config` 已经不读这些键。留下 `github_client_secret` 只是把旧 OAuth
/// 凭据继续放在库里；宠物和视差行则是死数据。
pub async fn purge_retired_configuration_keys(db: &DatabaseConnection) -> anyhow::Result<usize> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT key FROM configurations".to_string(),
        ))
        .await
        .context("Failed to list configurations for retired-key purge")?;

    let mut deleted = 0usize;
    for row in rows {
        let Ok(key) = row.try_get::<String>("", "key") else {
            continue;
        };
        if !is_retired_configuration_key(&key) {
            continue;
        }
        match db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM configurations WHERE key = $1",
                vec![key.clone().into()],
            ))
            .await
        {
            Ok(result) => deleted += result.rows_affected() as usize,
            Err(error) => {
                tracing::error!(key, %error, "Failed to purge retired configuration row")
            }
        }
    }

    if deleted > 0 {
        tracing::info!("Purged {deleted} retired configuration row(s)");
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::{
        is_retired_configuration_key, RETIRED_CONFIGURATION_KEYS, RETIRED_CONFIGURATION_PREFIXES,
    };

    #[test]
    fn denylist_covers_known_retired_keys_and_spares_live_github() {
        for key in [
            "ui_wallpaper_parallax",
            "github_redirect_url",
            "pet_enabled",
            "pet_image_url",
            "github_client_id",
            "github_client_secret",
        ] {
            assert!(is_retired_configuration_key(key), "{key}");
        }
        assert!(!is_retired_configuration_key("github_token"));
        assert!(!is_retired_configuration_key("github_enabled"));
        assert!(!is_retired_configuration_key("github_username"));
        assert!(!is_retired_configuration_key("github_api_base_url"));
        assert!(!is_retired_configuration_key("island_show_tapp"));
    }

    #[test]
    fn denylist_tables_are_the_only_match_sources() {
        assert_eq!(
            RETIRED_CONFIGURATION_KEYS,
            &["ui_wallpaper_parallax", "github_redirect_url"]
        );
        assert_eq!(RETIRED_CONFIGURATION_PREFIXES, &["pet_", "github_client_"]);
    }
}
