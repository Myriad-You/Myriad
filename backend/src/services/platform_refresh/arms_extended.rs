//! Extended platform fetch arms (X / Discord / MAL / Xbox / PSN / YouTube).

use super::errors::note_fetch_error;
use super::fetch::FetchCtx;
use serde_json::json;

pub(super) async fn fetch_x(ctx: &mut FetchCtx<'_>) {
    if let (Some(username), Some(bearer_token)) =
        (&ctx.config.x_username, &ctx.config.x_bearer_token)
    {
        match ctx
            .fetcher
            .fetch_x_profile_bundle(username, bearer_token)
            .await
        {
            Ok(bundle) => {
                ctx.all_data["x"] = bundle;
                let tweet_count = ctx.all_data["x"]["tweets"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                tracing::info!("✓ X data fetched: {} tweets", tweet_count);
            }
            Err(e) => {
                tracing::warn!("X fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "x", "", e);
            }
        }

        if !ctx.all_data["x"].is_null() {
            if let Err(e) = ctx
                .metadata_service
                .save_platform_metadata(ctx.user_id, "x", ctx.all_data["x"].clone())
                .await
            {
                tracing::error!("Failed to save X metadata to database: {}", e);
            }
        }
    } else {
        tracing::warn!("X enabled but username or bearer_token missing");
    }
}

pub(super) async fn fetch_discord(ctx: &mut FetchCtx<'_>) {
    if let Some(access_token_cfg) = ctx.config.discord_access_token.as_deref() {
        let expires_at = ctx
            .config
            .discord_token_expires_at
            .as_deref()
            .and_then(|s| s.parse::<i64>().ok());

        // 若配置了 Discord OAuth App（登录用 provider），可用于 refresh
        let (oauth_client_id, oauth_client_secret) = ctx
            .config
            .oauth_providers
            .iter()
            .find(|p| {
                p.enabled
                    && (p.slug.eq_ignore_ascii_case("discord")
                        || p.display_name.eq_ignore_ascii_case("discord")
                        || p.discovery_url
                            .as_deref()
                            .map(|u| u.contains("discord.com"))
                            .unwrap_or(false))
            })
            .map(|p| (p.client_id.as_str(), p.client_secret.as_str()))
            .unwrap_or(("", ""));

        let (access_token, new_refresh, new_expires, did_refresh) = match ctx
            .fetcher
            .ensure_discord_access_token(
                access_token_cfg,
                ctx.config.discord_refresh_token.as_deref(),
                expires_at,
                if oauth_client_id.is_empty() {
                    None
                } else {
                    Some(oauth_client_id)
                },
                if oauth_client_secret.is_empty() {
                    None
                } else {
                    Some(oauth_client_secret)
                },
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Discord token ensure failed: {}", e);
                (
                    access_token_cfg.to_string(),
                    ctx.config.discord_refresh_token.clone(),
                    expires_at,
                    false,
                )
            }
        };

        if did_refresh {
            let mut token_updates = std::collections::HashMap::new();
            token_updates.insert(
                "discord_access_token".to_string(),
                json!(access_token.clone()),
            );
            if let Some(ref rt) = new_refresh {
                token_updates.insert("discord_refresh_token".to_string(), json!(rt));
            }
            if let Some(exp) = new_expires {
                token_updates.insert(
                    "discord_token_expires_at".to_string(),
                    json!(exp.to_string()),
                );
            }
            if let Err(e) = crate::services::config_service::ConfigService::new(ctx.db.clone())
                .update_configs(token_updates)
                .await
            {
                tracing::warn!("Failed to persist refreshed Discord tokens: {}", e);
            } else {
                tracing::info!("✓ Discord access token refreshed and saved");
            }
        }

        match ctx
            .fetcher
            .fetch_discord_profile_bundle(&access_token)
            .await
        {
            Ok(mut bundle) => {
                // 注入 Myriad 侧配置，供 smart_filter 交叉校验
                if let Some(obj) = bundle.as_object_mut() {
                    obj.insert(
                        "myriad_cross_refs".to_string(),
                        json!({
                            "steam_id": ctx.config.steam_id,
                            "github_username": ctx.config.github_username,
                        }),
                    );
                }

                if let Some(uid) = bundle
                    .pointer("/user/id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    let mut id_update = std::collections::HashMap::new();
                    id_update.insert("discord_user_id".to_string(), json!(uid));
                    let _ = crate::services::config_service::ConfigService::new(ctx.db.clone())
                        .update_configs(id_update)
                        .await;
                }

                ctx.all_data["discord"] = bundle;
                let guild_count = ctx.all_data["discord"]["guilds"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                let conn_count = ctx.all_data["discord"]["connections"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                tracing::info!(
                    "✓ Discord data fetched: {} guilds, {} connections",
                    guild_count,
                    conn_count
                );
            }
            Err(e) => {
                tracing::warn!("Discord fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "discord", "", e);
            }
        }

        if !ctx.all_data["discord"].is_null() {
            if let Err(e) = ctx
                .metadata_service
                .save_platform_metadata(ctx.user_id, "discord", ctx.all_data["discord"].clone())
                .await
            {
                tracing::error!("Failed to save Discord metadata to database: {}", e);
            }
        }
    } else {
        tracing::warn!("Discord enabled but access_token missing");
    }
}

pub(super) async fn fetch_mal(ctx: &mut FetchCtx<'_>) {
    if let Some(username) = ctx
        .config
        .mal_username
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let client_id = ctx
            .config
            .mal_client_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        match ctx
            .fetcher
            .fetch_mal_profile_bundle(username, client_id)
            .await
        {
            Ok(bundle) => {
                ctx.all_data["mal"] = bundle;
                let anime_count = ctx.all_data["mal"]["anime_list"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                let manga_count = ctx.all_data["mal"]["manga_list"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                tracing::info!(
                    "✓ MyAnimeList data fetched: {} anime, {} manga ({})",
                    anime_count,
                    manga_count,
                    if client_id.is_some() {
                        "official API"
                    } else {
                        "load.json"
                    }
                );
            }
            Err(e) => {
                tracing::warn!("MyAnimeList fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "mal", "", e);
            }
        }

        if !ctx.all_data["mal"].is_null() {
            if let Err(e) = ctx
                .metadata_service
                .save_platform_metadata(ctx.user_id, "mal", ctx.all_data["mal"].clone())
                .await
            {
                tracing::error!("Failed to save MyAnimeList metadata to database: {}", e);
            }
        }
    } else {
        tracing::warn!("MyAnimeList enabled but username missing");
    }
}

/// 获取 Xbox 数据（成就向：Gamerscore + 各游戏成就进度）
/// 凭据：DB 优先，env 回退（与 game_presence / 配置页展示一致）
pub(super) async fn fetch_xbox(ctx: &mut FetchCtx<'_>) {
    let gamertag = ctx
        .config
        .xbox_gamertag
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("XBOX_GAMERTAG").ok())
        .unwrap_or_default();
    let api_key = ctx
        .config
        .openxbl_api_key
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("OPENXBL_API_KEY").ok())
        .or_else(|| std::env::var("XBL_API_KEY").ok())
        .unwrap_or_default();

    if !gamertag.trim().is_empty() && !api_key.trim().is_empty() {
        match ctx
            .fetcher
            .fetch_xbox_profile_bundle(&gamertag, &api_key)
            .await
        {
            Ok(bundle) => {
                ctx.all_data["xbox"] = bundle;
                let titles_count = ctx.all_data["xbox"]["achievements"]["titles"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                tracing::info!("✓ Xbox data fetched: {} titles", titles_count);
            }
            Err(e) => {
                tracing::warn!("Xbox fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "xbox", "", e);
            }
        }

        if !ctx.all_data["xbox"].is_null() {
            if let Err(e) = ctx
                .metadata_service
                .save_platform_metadata(ctx.user_id, "xbox", ctx.all_data["xbox"].clone())
                .await
            {
                tracing::error!("Failed to save Xbox metadata to database: {}", e);
            }
        }
    } else {
        tracing::warn!("Xbox enabled but gamertag or OpenXBL API key missing");
    }
}

pub(super) async fn fetch_psn(ctx: &mut FetchCtx<'_>) {
    let online_id = ctx
        .config
        .psn_online_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("PSN_ONLINE_ID").ok())
        .unwrap_or_default();
    let npsso = ctx
        .config
        .psn_npsso
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("PSN_NPSSO").ok())
        .unwrap_or_default();

    if !online_id.trim().is_empty() && !npsso.trim().is_empty() {
        match ctx
            .fetcher
            .fetch_psn_profile_bundle(&online_id, &npsso)
            .await
        {
            Ok(bundle) => {
                ctx.all_data["psn"] = bundle;
                let titles_count = ctx.all_data["psn"]["trophy_titles"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                tracing::info!("✓ PSN data fetched: {} trophy titles", titles_count);
            }
            Err(e) => {
                tracing::warn!("PSN fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "psn", "", e);
            }
        }

        if !ctx.all_data["psn"].is_null() {
            if let Err(e) = ctx
                .metadata_service
                .save_platform_metadata(ctx.user_id, "psn", ctx.all_data["psn"].clone())
                .await
            {
                tracing::error!("Failed to save PSN metadata to database: {}", e);
            }
        }
    } else {
        tracing::warn!("PSN enabled but online_id or NPSSO missing");
    }
}

pub(super) async fn fetch_youtube(ctx: &mut FetchCtx<'_>) {
    if let (Some(api_key), Some(channel_id)) =
        (&ctx.config.youtube_api_key, &ctx.config.youtube_channel_id)
    {
        match ctx
            .fetcher
            .fetch_youtube_channel_bundle(api_key, channel_id)
            .await
        {
            Ok(bundle) => {
                ctx.all_data["youtube"] = bundle;
                let video_n = ctx.all_data["youtube"]["videos"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                tracing::info!("✓ YouTube channel data fetched: {} sample videos", video_n);
            }
            Err(e) => {
                tracing::warn!("YouTube fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "youtube", "", e);
            }
        }

        if !ctx.all_data["youtube"].is_null() {
            if let Err(e) = ctx
                .metadata_service
                .save_platform_metadata(ctx.user_id, "youtube", ctx.all_data["youtube"].clone())
                .await
            {
                tracing::error!("Failed to save YouTube metadata to database: {}", e);
            }
        }
    }
}
