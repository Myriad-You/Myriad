//! Shared platform definitions for admin forms and the public five-field summary.
use super::flags::{admin_platform_enabled, nonempty_db, sort_platforms_by_order};
use super::secrets::mask_secret_display_value;
use super::types::{ConfigField, PlatformConfig};
use crate::config::DynamicConfig;
use crate::services::platform_id::PlatformId;
use sea_orm::DatabaseConnection;
use serde::Serialize;

pub(super) fn build_platforms(
    stored: &DynamicConfig,
    reveal_sensitive: bool,
    include_fields: bool,
) -> Vec<PlatformConfig> {
    // Helper to mask sensitive values (passwords, API keys, tokens).
    // Must stay aligned with [`is_masked_secret_value`] (save + platform_test).
    let mask_sensitive = |value: String| -> String {
        if value.trim().is_empty() || reveal_sensitive {
            if value.trim().is_empty() {
                String::new()
            } else {
                value
            }
        } else {
            mask_secret_display_value()
        }
    };

    let has_bangumi_username = nonempty_db(stored.bangumi_username.as_ref());
    let has_bangumi_access_token = nonempty_db(stored.bangumi_access_token.as_ref());
    let has_bangumi_identity = has_bangumi_username || has_bangumi_access_token;
    let has_youtube_key = nonempty_db(stored.youtube_api_key.as_ref());
    let has_x_bearer = nonempty_db(stored.x_bearer_token.as_ref());
    let has_discord_token = nonempty_db(stored.discord_access_token.as_ref());
    let has_mal_username = nonempty_db(stored.mal_username.as_ref());
    let has_openxbl_key = nonempty_db(stored.openxbl_api_key.as_ref());
    let has_psn_npsso = nonempty_db(stored.psn_npsso.as_ref());
    // 开关显示的是「按表单现状保存之后」的启用：同一条 PlatformId 规则，作用在表单
    // 里显示（也会被保存回去）的凭据上。
    let enabled = {
        let admin = admin_platform_enabled(&stored);
        move |id: PlatformId| admin.iter().any(|(p, on)| *p == id && *on)
    };

    let mut platforms = vec![
        PlatformConfig {
            name: "GitHub".to_string(),
            enabled: enabled(PlatformId::Github),
            has_token: nonempty_db(stored.github_token.as_ref()),
            icon: "".to_string(),
            description: "Repos, stars, and contributions".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "GitHub Username".to_string(),
                        field_type: "text".to_string(),
                        value: stored.github_username.clone().unwrap_or_default(),
                        placeholder: "octocat".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "token".to_string(),
                        label: "Personal Access Token (Optional)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.github_token.clone().unwrap_or_default()),
                        placeholder: "ghp_xxxxxxxxxxxx (Increases API rate limit)".to_string(),
                        required: false,
                    },
                ]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "Bilibili".to_string(),
            enabled: enabled(PlatformId::Bilibili),
            has_token: nonempty_db(stored.bilibili_uid.as_ref()),
            icon: "".to_string(),
            description: "Favorites, anime, and viewing history".to_string(),
            config_fields: if include_fields {
                vec![ConfigField {
                    key: "uid".to_string(),
                    label: "User ID (UID)".to_string(),
                    field_type: "number".to_string(),
                    value: stored.bilibili_uid.clone().unwrap_or_default(),
                    placeholder: "123456789".to_string(),
                    required: true,
                }]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "Steam".to_string(),
            enabled: enabled(PlatformId::Steam),
            has_token: nonempty_db(stored.steam_api_key.as_ref()),
            icon: "".to_string(),
            description: "Library, wishlist, and play stats".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "api_key".to_string(),
                        label: "Steam API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.steam_api_key.clone().unwrap_or_default()),
                        placeholder: "Get from steamcommunity.com/dev/apikey".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "steam_id".to_string(),
                        label: "Steam ID".to_string(),
                        field_type: "text".to_string(),
                        value: stored.steam_id.clone().unwrap_or_default(),
                        placeholder: "76561198XXXXXXXXX".to_string(),
                        required: true,
                    },
                ]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "YouTube".to_string(),
            enabled: enabled(PlatformId::Youtube),
            has_token: has_youtube_key,
            icon: "".to_string(),
            description: "Public channel stats and recent uploads".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "api_key".to_string(),
                        label: "YouTube Data API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.youtube_api_key.clone().unwrap_or_default()),
                        placeholder: "Google Cloud → YouTube Data API v3 key".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "channel_id".to_string(),
                        label: "Channel ID or @handle".to_string(),
                        field_type: "text".to_string(),
                        value: stored.youtube_channel_id.clone().unwrap_or_default(),
                        placeholder: "UCxxxxx or @GoogleDevelopers".to_string(),
                        required: true,
                    },
                ]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "Netease Music".to_string(),
            enabled: enabled(PlatformId::Netease),
            has_token: nonempty_db(stored.netease_user_id.as_ref()),
            icon: "".to_string(),
            description: "Liked songs and music taste".to_string(),
            config_fields: if include_fields {
                vec![ConfigField {
                    key: "user_id".to_string(),
                    label: "User ID".to_string(),
                    field_type: "number".to_string(),
                    value: stored.netease_user_id.clone().unwrap_or_default(),
                    placeholder: "Your Netease Cloud Music user ID".to_string(),
                    required: true,
                }]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "Bangumi".to_string(),
            enabled: enabled(PlatformId::Bangumi),
            has_token: has_bangumi_identity,
            icon: "".to_string(),
            description: "Collections, ratings, and watching status".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "Bangumi Username".to_string(),
                        field_type: "text".to_string(),
                        value: stored.bangumi_username.clone().unwrap_or_default(),
                        placeholder: "your Bangumi username".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "access_token".to_string(),
                        label: "Access Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(
                            stored.bangumi_access_token.clone().unwrap_or_default(),
                        ),
                        placeholder: "Bearer token for private collections".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "user_agent".to_string(),
                        label: "User-Agent".to_string(),
                        field_type: "text".to_string(),
                        value: stored.bangumi_user_agent.clone().unwrap_or_default(),
                        placeholder: "myriad/Myriad".to_string(),
                        required: false,
                    },
                ]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "X".to_string(),
            enabled: enabled(PlatformId::X),
            has_token: has_x_bearer,
            icon: "".to_string(),
            description: "Profile and posts, with sharing".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "X Username".to_string(),
                        field_type: "text".to_string(),
                        value: stored.x_username.clone().unwrap_or_default(),
                        placeholder: String::new(),
                        required: true,
                    },
                    ConfigField {
                        key: "bearer_token".to_string(),
                        label: "Bearer Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.x_bearer_token.clone().unwrap_or_default()),
                        placeholder: "From developer.x.com App keys (read-only sync)".to_string(),
                        required: true,
                    },
                ]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "Discord".to_string(),
            enabled: enabled(PlatformId::Discord),
            has_token: has_discord_token,
            icon: "".to_string(),
            description: "Profile, servers, and linked accounts".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "access_token".to_string(),
                        label: "Access Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(
                            stored.discord_access_token.clone().unwrap_or_default(),
                        ),
                        placeholder: "OAuth user token (scopes: identify guilds connections)"
                            .to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "refresh_token".to_string(),
                        label: "Refresh Token (Recommended)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(
                            stored.discord_refresh_token.clone().unwrap_or_default(),
                        ),
                        placeholder: "Optional; enables auto-refresh when access token expires"
                            .to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "user_id".to_string(),
                        label: "User ID (auto-filled after test)".to_string(),
                        field_type: "text".to_string(),
                        value: stored.discord_user_id.clone().unwrap_or_default(),
                        placeholder: "Discord snowflake id".to_string(),
                        required: false,
                    },
                ]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "MyAnimeList".to_string(),
            enabled: enabled(PlatformId::Mal),
            has_token: has_mal_username,
            icon: "".to_string(),
            description: "Anime / manga lists and scores".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "MyAnimeList Username".to_string(),
                        field_type: "text".to_string(),
                        value: stored.mal_username.clone().unwrap_or_default(),
                        placeholder: "your MAL username (required)".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "client_id".to_string(),
                        label: "Client ID (optional)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.mal_client_id.clone().unwrap_or_default()),
                        placeholder:
                            "Optional — leave empty for public list (load.json); fill for official API (myanimelist.net/apiconfig)"
                                .to_string(),
                        required: false,
                    },
                ]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "Xbox".to_string(),
            enabled: enabled(PlatformId::Xbox),
            has_token: has_openxbl_key,
            icon: "".to_string(),
            description: "Achievements, Gamerscore, and recent games".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "gamertag".to_string(),
                        label: "Gamertag".to_string(),
                        field_type: "text".to_string(),
                        value: stored.xbox_gamertag.clone().unwrap_or_default(),
                        placeholder: "Major Nelson or Name#1234".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "openxbl_api_key".to_string(),
                        label: "OpenXBL API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.openxbl_api_key.clone().unwrap_or_default()),
                        placeholder: "From xbl.io profile".to_string(),
                        required: true,
                    },
                ]
            } else {
                Vec::new()
            },
        },
        PlatformConfig {
            name: "PlayStation".to_string(),
            enabled: enabled(PlatformId::Psn),
            has_token: has_psn_npsso,
            icon: "".to_string(),
            description: "Trophies, trophy level, and recent games".to_string(),
            config_fields: if include_fields {
                vec![
                    ConfigField {
                        key: "online_id".to_string(),
                        label: "Online ID".to_string(),
                        field_type: "text".to_string(),
                        value: stored.psn_online_id.clone().unwrap_or_default(),
                        placeholder: "Your PSN Online ID".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "npsso".to_string(),
                        label: "NPSSO Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.psn_npsso.clone().unwrap_or_default()),
                        placeholder: "64-char token from ca.account.sony.com".to_string(),
                        required: true,
                    },
                ]
            } else {
                Vec::new()
            },
        },
    ];
    sort_platforms_by_order(&mut platforms, stored.platform_order.as_ref());
    platforms
}

/// This type can serialize only public fields; configuration values stay behind.
#[derive(Serialize)]
pub(crate) struct PublicPlatformSummary {
    name: String,
    enabled: bool,
    has_token: bool,
    icon: String,
    description: String,
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn public_platform_summaries(stored: &DynamicConfig) -> Vec<PublicPlatformSummary> {
    build_platforms(stored, false, false)
        .into_iter()
        .map(|platform| PublicPlatformSummary {
            name: platform.name,
            enabled: platform.enabled,
            has_token: platform.has_token,
            icon: platform.icon,
            description: platform.description,
        })
        .collect()
}

pub(crate) async fn load_public_platform_summaries(
    db: &DatabaseConnection,
) -> Result<Vec<PublicPlatformSummary>, String> {
    let stored = crate::services::config_service::ConfigService::new(db.clone())
        .load_config()
        .await
        .map_err(|error| {
            tracing::error!(%error, "Stored configuration could not be read");
            "Stored configuration could not be read".to_string()
        })?;
    Ok(public_platform_summaries(&stored))
}

#[cfg(test)]
#[path = "platforms/tests.rs"]
mod tests;
