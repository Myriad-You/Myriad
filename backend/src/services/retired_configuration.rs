//! Retired `configurations` keys: backup ignore list.
//!
//! Exact names and prefixes stay in one place so export/restore cannot
//! drift. Prefixes are written so `github_client_*` cannot match live
//! `github_token`.

const RETIRED_CONFIGURATION_KEYS: &[&str] = &["ui_wallpaper_parallax", "github_redirect_url"];

const RETIRED_CONFIGURATION_PREFIXES: &[&str] = &["pet_", "github_client_"];

pub(crate) fn is_retired_configuration_key(key: &str) -> bool {
    RETIRED_CONFIGURATION_KEYS.contains(&key)
        || RETIRED_CONFIGURATION_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
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
