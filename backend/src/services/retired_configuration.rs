//! Retired `configurations` keys: backup ignore list.
//!
//! Exact names and prefixes stay in one place so export/restore cannot
//! drift. Prefixes are written so `github_client_*` cannot match live
//! `github_token`.

const RETIRED_CONFIGURATION_KEYS: &[&str] = &[
    "ui_wallpaper_parallax",
    "github_redirect_url",
    // Stored and never read.
    "topic_style",
    "openweather_api_key",
    "ui_theme",
    "ui_primary_color",
    "ui_secondary_color",
    // Not delegable: report:write is admin-only, media:control is basic, and
    // the guest ones need a signed-in subject (see `DELEGATIONS`).
    "user_perm_report_write",
    "user_perm_media_control",
    "guest_perm_report_write",
    "guest_perm_media_control",
    "guest_perm_3d_generate",
    "guest_perm_component_theme",
    "guest_perm_shortcut_register",
    "guest_perm_scheduler_register",
    "guest_perm_speech_tts",
    "guest_perm_speech_asr",
    "guest_perm_federation_post",
    "guest_perm_federation_channel",
    "guest_perm_federation_room",
    "guest_perm_phantasi_comment_write",
];

const RETIRED_CONFIGURATION_PREFIXES: &[&str] = &["pet_", "github_client_"];

/// Remove retired rows: nothing reads them, and a backup would carry them.
/// The old AI settings go first through their own upgrade, which reads them.
pub(crate) async fn drop_retired_rows(
    db: &impl sea_orm::ConnectionTrait,
) -> Result<u64, sea_orm::DbErr> {
    let keys: Vec<String> = RETIRED_CONFIGURATION_KEYS
        .iter()
        .map(|key| key.to_string())
        .collect();
    let prefixes: Vec<String> = RETIRED_CONFIGURATION_PREFIXES
        .iter()
        .map(|prefix| format!("{}%", prefix.replace('_', "\\_")))
        .collect();
    let result = db
        .execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "DELETE FROM configurations WHERE key = ANY($1) OR key LIKE ANY($2)",
            [keys.into(), prefixes.into()],
        ))
        .await?;
    Ok(result.rows_affected())
}

pub(crate) fn is_retired_configuration_key(key: &str) -> bool {
    RETIRED_CONFIGURATION_KEYS.contains(&key)
        // Old text-model settings: restored in their new keys instead.
        || crate::services::config_service::legacy_ai_settings::is_legacy_key(key)
        || RETIRED_CONFIGURATION_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::{
        RETIRED_CONFIGURATION_KEYS, RETIRED_CONFIGURATION_PREFIXES, is_retired_configuration_key,
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
        assert!(is_retired_configuration_key("lite_openai_model"));
        assert!(is_retired_configuration_key("openai_api_key"));
        assert!(!is_retired_configuration_key("lite_ai_model"));
        assert!(!is_retired_configuration_key("provider_openai_api_key"));
    }

    #[test]
    fn denylist_tables_are_the_only_match_sources() {
        assert!(RETIRED_CONFIGURATION_KEYS.contains(&"ui_wallpaper_parallax"));
        assert!(RETIRED_CONFIGURATION_KEYS.contains(&"github_redirect_url"));
        // A retired key is not a setting any more.
        let defaults = serde_json::to_value(crate::config::DynamicConfig::default()).unwrap();
        for key in RETIRED_CONFIGURATION_KEYS {
            assert!(defaults.get(*key).is_none(), "{key} is still a setting");
        }
        for (key, _) in defaults.as_object().unwrap() {
            assert!(!is_retired_configuration_key(key), "{key} is live");
        }
        assert_eq!(RETIRED_CONFIGURATION_PREFIXES, &["pet_", "github_client_"]);
    }
}
