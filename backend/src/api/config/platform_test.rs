//! Platform connectivity test endpoint.
use axum::{
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};

use super::{form_secret_if_plaintext, is_masked_secret_value};
use crate::api::reports::locale::host_locale_from_headers;
use crate::services::platform_refresh::humanize_platform_fetch_error_for;

/// Map UI platform label → internal platform id for error humanization.
fn test_platform_id(ui_label: &str) -> &'static str {
    match ui_label {
        "GitHub" => "github",
        "Bilibili" => "bilibili",
        "Steam" => "steam",
        "YouTube" => "youtube",
        "Netease Music" | "Netease" => "netease",
        "Bangumi" => "bangumi",
        "Discord" => "discord",
        "X" => "x",
        "MyAnimeList" => "mal",
        "Xbox" => "xbox",
        "PSN" | "PlayStation" => "psn",
        _ => "platform",
    }
}

fn test_fail_message(ui_label: &str, err: impl ToString, locale: &str) -> String {
    humanize_platform_fetch_error_for(test_platform_id(ui_label), &err.to_string(), locale)
}

fn test_reject(code: &'static str, message: &'static str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "success": false,
            "message": message,
            "code": code,
        })),
    )
}

fn test_fetch_fail(ui_label: &str, err: impl ToString, locale: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "success": false,
            "message": test_fail_message(ui_label, err, locale),
            "code": "fetch_failed",
        })),
    )
}

pub async fn test_platform(
    crate::extract::Db(_db): crate::extract::Db,
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let locale = host_locale_from_headers(&headers);
    let platform = payload["platform"].as_str().unwrap_or("");
    let config = &payload["config"];

    match platform {
        "GitHub" => {
            let username = config["username"].as_str().unwrap_or("");
            if username.is_empty() {
                return test_reject("username_required", "Username is required");
            }

            let token = config["token"].as_str().filter(|s| !s.is_empty());

            // 调用 GitHub API 验证
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_github_user(username, token).await {
                Ok(user_info) => {
                    let name = user_info["name"].as_str().unwrap_or(username);
                    let followers = user_info["followers"].as_i64().unwrap_or(0);
                    let repos = user_info["public_repos"].as_i64().unwrap_or(0);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!("✓ GitHub user '{}' verified. {} followers, {} repos", name, followers, repos)
                        })),
                    )
                }
                Err(e) => test_fetch_fail("GitHub", e, locale),
            }
        }
        "Bilibili" => {
            let uid = config["uid"].as_str().unwrap_or("");
            if uid.is_empty() {
                return test_reject("uid_required", "UID is required");
            }

            // 尝试解析 UID 为数字
            let uid_i64 = match uid.parse::<i64>() {
                Ok(n) => n,
                Err(_) => {
                    return test_reject("invalid_uid", "Invalid UID format");
                }
            };

            // 实际调用 Bilibili API 验证
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_bilibili_user(uid_i64).await {
                Ok(user_info) => (
                    StatusCode::OK,
                    Json(json!({
                        "success": true,
                        "message": format!("✓ Bilibili UID {} is valid. User: {}", uid, user_info.name)
                    })),
                ),
                Err(e) => test_fetch_fail("Bilibili", e, locale),
            }
        }
        "Steam" => {
            let api_key = config["api_key"].as_str().unwrap_or("");
            let steam_id = config["steam_id"].as_str().unwrap_or("");
            if api_key.is_empty() || steam_id.is_empty() {
                return test_reject(
                    "steam_credentials_required",
                    "API Key and Steam ID are required",
                );
            }

            // 调用 Steam API 验证
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_steam_user(api_key, steam_id).await {
                Ok(user_info) => (
                    StatusCode::OK,
                    Json(json!({
                        "success": true,
                        "message": format!("✓ Steam user '{}' verified", user_info.personaname)
                    })),
                ),
                Err(e) => test_fetch_fail("Steam", e, locale),
            }
        }
        "YouTube" => {
            let api_key = config["api_key"].as_str().unwrap_or("").trim();
            let channel_id = config["channel_id"].as_str().unwrap_or("").trim();
            if api_key.is_empty() || channel_id.is_empty() {
                return test_reject(
                    "youtube_credentials_required",
                    "API Key and Channel ID / @handle are required",
                );
            }
            // Masked secrets from form: fall back to stored config
            let cfg = dynamic_config.read().await;
            let resolved_key = if is_masked_secret_value(api_key) {
                cfg.youtube_api_key.clone().unwrap_or_default()
            } else {
                api_key.to_string()
            };
            drop(cfg);
            if resolved_key.trim().is_empty() {
                return test_reject(
                    "youtube_api_key_required",
                    "YouTube API key is required (re-enter after save)",
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher
                .fetch_youtube_channel(resolved_key.trim(), channel_id)
                .await
            {
                Ok(channel) => {
                    let title = channel
                        .pointer("/snippet/title")
                        .and_then(|v| v.as_str())
                        .unwrap_or(channel_id);
                    let subs = channel
                        .pointer("/statistics/subscriberCount")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let vids = channel
                        .pointer("/statistics/videoCount")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ YouTube channel '{}' verified. {} subscribers, {} videos",
                                title, subs, vids
                            )
                        })),
                    )
                }
                Err(e) => test_fetch_fail("YouTube", e, locale),
            }
        }
        "Netease Music" => {
            let user_id = config["user_id"].as_str().unwrap_or("");
            if user_id.is_empty() {
                return test_reject("user_id_required", "User ID is required");
            }

            // 尝试解析 User ID 为数字
            let user_id_i64 = match user_id.parse::<i64>() {
                Ok(n) => n,
                Err(_) => {
                    return test_reject("invalid_user_id", "Invalid User ID format");
                }
            };

            // 调用网易云音乐 API 验证
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_netease_user(user_id_i64).await {
                Ok(user_info) => {
                    let nickname = user_info["profile"]["nickname"]
                        .as_str()
                        .unwrap_or("Unknown");
                    let playlist_count =
                        user_info["profile"]["playlistCount"].as_i64().unwrap_or(0);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!("✓ Netease Music user '{}' verified. {} playlists", nickname, playlist_count)
                        })),
                    )
                }
                Err(e) => test_fetch_fail("Netease Music", e, locale),
            }
        }
        "Bangumi" => {
            let username = config["username"].as_str().unwrap_or("");
            let access_token = config["access_token"]
                .as_str()
                .and_then(|s| form_secret_if_plaintext(Some(s)));
            let user_agent = config["user_agent"].as_str().filter(|s| !s.is_empty());
            if username.is_empty() && access_token.is_none() {
                return test_reject(
                    "bangumi_credentials_required",
                    "Username or access token is required",
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            let result = if username.is_empty() {
                fetcher
                    .fetch_bangumi_me(access_token.as_deref().unwrap_or(""), user_agent)
                    .await
            } else {
                fetcher
                    .fetch_bangumi_user(username, access_token.as_deref(), user_agent)
                    .await
            };

            match result {
                Ok(user_info) => {
                    let display_name = user_info["nickname"]
                        .as_str()
                        .or_else(|| user_info["username"].as_str())
                        .unwrap_or(username);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!("✓ Bangumi user '{}' verified", display_name)
                        })),
                    )
                }
                Err(e) => test_fetch_fail("Bangumi", e, locale),
            }
        }
        "Discord" => {
            let form_token = config["access_token"]
                .as_str()
                .and_then(|s| form_secret_if_plaintext(Some(s)));
            // 一键授权后表单多为掩码：回退到已保存的 token
            let access_token = if let Some(t) = form_token {
                t
            } else {
                let cfg = dynamic_config.read().await;
                cfg.discord_access_token
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default()
            };
            if access_token.is_empty() {
                return test_reject(
                    "discord_token_required",
                    "Access Token is required. Use Connect Discord or paste a token.",
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_discord_me(&access_token).await {
                Ok(user_info) => {
                    let username = user_info["username"].as_str().unwrap_or("unknown");
                    let global_name = user_info["global_name"].as_str().unwrap_or(username);
                    let user_id = user_info["id"].as_str().unwrap_or("");
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ Discord user '{}' verified ({}). id={}",
                                global_name, username, user_id
                            ),
                            "user_id": user_id,
                        })),
                    )
                }
                Err(e) => test_fetch_fail("Discord", e, locale),
            }
        }
        "X" => {
            let username = config["username"]
                .as_str()
                .unwrap_or("")
                .trim()
                .trim_start_matches('@');
            let form_bearer = form_secret_if_plaintext(config["bearer_token"].as_str());
            let cfg = dynamic_config.read().await;
            let bearer_owned = form_bearer.or_else(|| {
                cfg.x_bearer_token
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("X_BEARER_TOKEN").ok())
                    .filter(|s| !s.trim().is_empty())
            });
            drop(cfg);
            let bearer_token = bearer_owned.as_deref().unwrap_or("");
            if username.is_empty() {
                return test_reject("username_required", "Username is required");
            }
            if bearer_token.is_empty() {
                return test_reject(
                    "bearer_token_required",
                    "Bearer Token is required (or re-enter it if the form shows a masked value)",
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher
                .fetch_x_user_by_username(username, bearer_token)
                .await
            {
                Ok(user_info) => {
                    let display = user_info["name"]
                        .as_str()
                        .or_else(|| user_info["username"].as_str())
                        .unwrap_or(username);
                    let followers = user_info
                        .pointer("/public_metrics/followers_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let tweets = user_info
                        .pointer("/public_metrics/tweet_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ X user '@{}' verified ({}). {} followers, {} posts",
                                username, display, followers, tweets
                            )
                        })),
                    )
                }
                Err(e) => test_fetch_fail("X", e, locale),
            }
        }
        "MyAnimeList" => {
            let username = config["username"].as_str().unwrap_or("").trim();
            // Client ID optional: form value if not masked; else fall back to saved config/env
            let form_client_id = config["client_id"]
                .as_str()
                .and_then(|s| form_secret_if_plaintext(Some(s)));
            let cfg = dynamic_config.read().await;
            let client_id = form_client_id.or_else(|| {
                cfg.mal_client_id
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("MAL_CLIENT_ID").ok())
                    .filter(|s| !s.trim().is_empty())
            });
            drop(cfg);
            if username.is_empty() {
                return test_reject("username_required", "Username is required");
            }

            let mode = if client_id.as_ref().is_some_and(|s| !s.trim().is_empty()) {
                "official API"
            } else {
                "public load.json"
            };
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_mal_user(username, client_id.as_deref()).await {
                Ok(user_info) => {
                    let display = user_info["name"].as_str().unwrap_or(username);
                    let anime_completed = user_info
                        .pointer("/anime_statistics/num_items_completed")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let anime_watching = user_info
                        .pointer("/anime_statistics/num_items_watching")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ MAL user '{}' verified ({}) via {}. {} completed, {} watching",
                                username, display, mode, anime_completed, anime_watching
                            )
                        })),
                    )
                }
                Err(e) => test_fetch_fail("MyAnimeList", e, locale),
            }
        }
        "Xbox" => {
            let form_gamertag = config["gamertag"].as_str().unwrap_or("").trim();
            let form_key = config["openxbl_api_key"]
                .as_str()
                .and_then(|s| form_secret_if_plaintext(Some(s)));

            let cfg = dynamic_config.read().await;
            let gamertag = if !form_gamertag.is_empty() {
                form_gamertag.to_string()
            } else {
                cfg.xbox_gamertag
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("XBOX_GAMERTAG").ok())
                    .unwrap_or_default()
            };
            let api_key = form_key.unwrap_or_else(|| {
                cfg.openxbl_api_key
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("OPENXBL_API_KEY").ok())
                    .or_else(|| std::env::var("XBL_API_KEY").ok())
                    .unwrap_or_default()
            });
            drop(cfg);

            if gamertag.trim().is_empty() {
                return test_reject("gamertag_required", "Gamertag is required");
            }
            if api_key.trim().is_empty() {
                return test_reject(
                    "xbox_api_key_required",
                    "OpenXBL API Key is required (or re-enter it if the form shows a masked value)",
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_xbox_profile_bundle(&gamertag, &api_key).await {
                Ok(bundle) => {
                    let titles = bundle
                        .pointer("/achievements/titles")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let display = bundle
                        .get("gamertag")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&gamertag);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ Xbox player '{}' verified. {} titles with achievement data",
                                display, titles
                            )
                        })),
                    )
                }
                Err(e) => test_fetch_fail("Xbox", e, locale),
            }
        }
        "PlayStation" => {
            let form_online_id = config["online_id"].as_str().unwrap_or("").trim();
            let form_npsso = config["npsso"]
                .as_str()
                .and_then(|s| form_secret_if_plaintext(Some(s)));

            let cfg = dynamic_config.read().await;
            let online_id = if !form_online_id.is_empty() {
                form_online_id.to_string()
            } else {
                cfg.psn_online_id
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("PSN_ONLINE_ID").ok())
                    .unwrap_or_default()
            };
            let npsso = form_npsso.unwrap_or_else(|| {
                cfg.psn_npsso
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("PSN_NPSSO").ok())
                    .unwrap_or_default()
            });
            drop(cfg);

            if online_id.trim().is_empty() {
                return test_reject("online_id_required", "Online ID is required");
            }
            if npsso.trim().is_empty() {
                return test_reject(
                    "npsso_required",
                    "NPSSO Token is required (or re-enter it if the form shows a masked value)",
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_psn_profile_bundle(&online_id, &npsso).await {
                Ok(bundle) => {
                    let titles = bundle
                        .get("trophy_titles")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let level = bundle
                        .pointer("/trophy_summary/trophyLevel")
                        .and_then(|v| {
                            v.as_i64()
                                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                        })
                        .unwrap_or(0);
                    let display = bundle
                        .get("online_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&online_id);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ PSN player '{}' verified (Lv.{}). {} trophy titles",
                                display, level, titles
                            )
                        })),
                    )
                }
                Err(e) => test_fetch_fail("PlayStation", e, locale),
            }
        }
        _ => (
            StatusCode::OK,
            Json(json!({
                "success": false,
                "message": "Platform test not implemented yet",
                "code": "platform_test_unimplemented",
            })),
        ),
    }
}
