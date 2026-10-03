//! Environment variables as first values for settings. A setting that was
//! never stored takes its environment variable once, at startup; from then
//! on the settings page, the runtime and a backup all read the database, and
//! the page is where it changes. Infrastructure (database, server, secrets
//! of the process, `BASE_URL`) stays in the environment and is not here.

use std::collections::HashMap;

use serde_json::{Value as JsonValue, json};

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Bool,
    Int,
}

/// When a stored value counts as never set.
#[derive(Clone, Copy)]
enum Unset {
    /// No row, or a null one. A cleared (empty) value is a choice.
    Missing,
    /// Also an empty string: the runtime fell through blank values.
    Blank,
}

use Kind::{Bool, Int, Text};
use Unset::{Blank, Missing};

/// Setting, the environment variables it was read from (first wins), its
/// type, and when it counts as never set.
const SEEDS: &[(&str, &[&str], Kind, Unset)] = &[
    ("github_username", &["GITHUB_USERNAME"], Text, Missing),
    ("github_token", &["GITHUB_TOKEN"], Text, Missing),
    ("bilibili_uid", &["BILIBILI_UID"], Text, Missing),
    ("steam_api_key", &["STEAM_API_KEY"], Text, Missing),
    ("steam_id", &["STEAM_ID"], Text, Missing),
    ("youtube_api_key", &["YOUTUBE_API_KEY"], Text, Blank),
    ("youtube_channel_id", &["YOUTUBE_CHANNEL_ID"], Text, Missing),
    ("netease_user_id", &["NETEASE_USER_ID"], Text, Missing),
    ("bangumi_username", &["BANGUMI_USERNAME"], Text, Missing),
    (
        "bangumi_access_token",
        &["BANGUMI_ACCESS_TOKEN"],
        Text,
        Missing,
    ),
    ("bangumi_user_agent", &["BANGUMI_USER_AGENT"], Text, Missing),
    ("x_username", &["X_USERNAME"], Text, Missing),
    ("x_bearer_token", &["X_BEARER_TOKEN"], Text, Blank),
    (
        "discord_access_token",
        &["DISCORD_ACCESS_TOKEN"],
        Text,
        Missing,
    ),
    (
        "discord_refresh_token",
        &["DISCORD_REFRESH_TOKEN"],
        Text,
        Missing,
    ),
    ("discord_user_id", &["DISCORD_USER_ID"], Text, Missing),
    ("mal_username", &["MAL_USERNAME"], Text, Missing),
    ("mal_client_id", &["MAL_CLIENT_ID"], Text, Blank),
    ("xbox_gamertag", &["XBOX_GAMERTAG"], Text, Blank),
    (
        "openxbl_api_key",
        &["OPENXBL_API_KEY", "XBL_API_KEY"],
        Text,
        Blank,
    ),
    ("psn_online_id", &["PSN_ONLINE_ID"], Text, Blank),
    ("psn_npsso", &["PSN_NPSSO"], Text, Blank),
    ("ui_wallpaper_url", &["UI_WALLPAPER_URL"], Text, Missing),
    ("site_title", &["SITE_TITLE"], Text, Blank),
    ("site_description", &["SITE_DESCRIPTION"], Text, Blank),
    ("site_favicon", &["SITE_FAVICON"], Text, Blank),
    ("site_keywords", &["SITE_KEYWORDS"], Text, Missing),
    ("site_og_image", &["SITE_OG_IMAGE"], Text, Missing),
    (
        "google_site_verification",
        &["GOOGLE_SITE_VERIFICATION"],
        Text,
        Missing,
    ),
    ("site_ai_intro", &["SITE_AI_INTRO"], Text, Missing),
    (
        "site_visibility_policy",
        &["SITE_VISIBILITY_POLICY"],
        Text,
        Blank,
    ),
    ("ga_measurement_id", &["GA_MEASUREMENT_ID"], Text, Missing),
    ("umami_website_id", &["UMAMI_WEBSITE_ID"], Text, Missing),
    ("umami_script_url", &["UMAMI_SCRIPT_URL"], Text, Missing),
    ("site_footer_custom", &["SITE_FOOTER_CUSTOM"], Text, Missing),
    ("music_enabled", &["MUSIC_ENABLED"], Text, Missing),
    ("music_source", &["MUSIC_SOURCE"], Text, Missing),
    ("music_playlist_id", &["MUSIC_PLAYLIST_ID"], Text, Missing),
    ("tripo_api_key", &["TRIPO_API_KEY"], Text, Blank),
    // Tripo's variables used to override the stored values; a value is
    // stored for every setting, so they only reach a database without one.
    ("tripo_enabled", &["TRIPO_ENABLED"], Bool, Missing),
    ("tripo_base_url", &["TRIPO_BASE_URL"], Text, Missing),
    ("tripo_model", &["TRIPO_MODEL"], Text, Missing),
    ("tripo_face_limit", &["TRIPO_FACE_LIMIT"], Int, Missing),
    (
        "tripo_poll_interval_seconds",
        &["TRIPO_POLL_INTERVAL_SECONDS"],
        Int,
        Missing,
    ),
    (
        "tripo_task_timeout_seconds",
        &["TRIPO_TASK_TIMEOUT_SECONDS"],
        Int,
        Missing,
    ),
    (
        "tripo_max_download_mb",
        &["TRIPO_MAX_DOWNLOAD_MB"],
        Int,
        Missing,
    ),
];

/// The first values `env` gives settings `stored` never had.
pub(crate) fn seeds(
    stored: &HashMap<String, JsonValue>,
    env: impl Fn(&str) -> Option<String>,
) -> HashMap<String, JsonValue> {
    let mut out = HashMap::new();
    for &(key, names, kind, unset) in SEEDS {
        let never_set = match stored.get(key) {
            None | Some(JsonValue::Null) => true,
            Some(JsonValue::String(value)) => matches!(unset, Blank) && value.trim().is_empty(),
            Some(_) => false,
        };
        if !never_set {
            continue;
        }
        if let Some((_, value)) = env_value(names, kind, &env) {
            out.insert(key.to_string(), value);
        }
    }
    out
}

/// Variables still set to something other than their stored setting: the
/// stored one is used. Names only; a value may be a secret.
pub(crate) fn shadowed(
    stored: &HashMap<String, JsonValue>,
    env: impl Fn(&str) -> Option<String>,
) -> Vec<&'static str> {
    SEEDS
        .iter()
        .filter_map(|&(key, names, kind, _)| {
            let stored = stored.get(key).filter(|value| !value.is_null())?;
            let (name, value) = env_value(names, kind, &env)?;
            (stored != &value).then_some(name)
        })
        .collect()
}

/// The first of `names` set to something, read as `kind`.
fn env_value(
    names: &'static [&'static str],
    kind: Kind,
    env: &impl Fn(&str) -> Option<String>,
) -> Option<(&'static str, JsonValue)> {
    let (name, raw) = names.iter().find_map(|name| {
        env(name)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| (*name, value))
    })?;
    let value = match kind {
        Text => json!(raw),
        Bool => json!(matches!(
            raw.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )),
        Int => json!(raw.parse::<i64>().ok()?),
    };
    Some((name, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn a_setting_never_stored_takes_its_variable_once() {
        let stored = HashMap::from([
            ("github_token".to_string(), JsonValue::Null),
            ("site_title".to_string(), json!("")),
            ("site_keywords".to_string(), json!("")),
            ("site_favicon".to_string(), json!(" ")),
            ("steam_id".to_string(), json!("kept")),
        ]);
        let out = seeds(
            &stored,
            env(&[
                ("GITHUB_TOKEN", " ghp_x "),
                ("SITE_TITLE", "From env"),
                ("SITE_KEYWORDS", "env, words"),
                ("SITE_FAVICON", "/icon.png"),
                ("STEAM_ID", "env-id"),
                ("XBL_API_KEY", "xbl"),
                ("TRIPO_ENABLED", "true"),
                ("TRIPO_FACE_LIMIT", "8000"),
                ("TRIPO_MODEL", ""),
            ]),
        );
        assert_eq!(out["github_token"], json!("ghp_x"));
        // Branding fell through a blank title at runtime: so does the seed.
        assert_eq!(out["site_title"], json!("From env"));
        // The runtime fell through a blank favicon: so does the seed.
        assert_eq!(out["site_favicon"], json!("/icon.png"));
        // Cleared on purpose: stays cleared.
        assert!(!out.contains_key("site_keywords"));
        assert!(!out.contains_key("steam_id"));
        // The second name of a setting counts too.
        assert_eq!(out["openxbl_api_key"], json!("xbl"));
        assert_eq!(out["tripo_enabled"], json!(true));
        assert_eq!(out["tripo_face_limit"], json!(8000));
        assert!(
            !out.contains_key("tripo_model"),
            "blank variables seed nothing"
        );
    }

    #[test]
    fn a_variable_the_stored_setting_overrules_is_named() {
        let stored = HashMap::from([
            ("tripo_enabled".to_string(), json!(false)),
            ("tripo_face_limit".to_string(), json!(8000)),
            ("github_token".to_string(), JsonValue::Null),
        ]);
        let names = shadowed(
            &stored,
            env(&[
                ("TRIPO_ENABLED", "true"),
                ("TRIPO_FACE_LIMIT", "8000"),
                ("GITHUB_TOKEN", "ghp_x"),
            ]),
        );
        // Same value: nothing to say. Never stored: it is seeded instead.
        assert_eq!(names, ["TRIPO_ENABLED"]);
    }

    #[test]
    fn infrastructure_is_not_a_setting() {
        for (key, names, ..) in SEEDS {
            for name in *names {
                assert!(
                    !matches!(
                        *name,
                        "DATABASE_URL" | "JWT_SECRET" | "BASE_URL" | "CORS_ORIGINS"
                    ),
                    "{key}"
                );
            }
            assert!(
                serde_json::to_value(crate::config::DynamicConfig::default())
                    .unwrap()
                    .get(*key)
                    .is_some(),
                "{key} is not a setting"
            );
        }
    }
}
