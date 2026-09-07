//! Core platform fetch arms (GitHub / Bilibili / Steam / NetEase / Bangumi).

use super::errors::note_fetch_error;
use super::fetch::FetchCtx;
use serde_json::json;

pub(super) async fn fetch_github(ctx: &mut FetchCtx<'_>) {
    if let Some(github_username) = &ctx.config.github_username {
        let github_token = ctx.config.github_token.as_deref();

        // 获取用户基本信息
        match ctx
            .fetcher
            .fetch_github_user(github_username, github_token)
            .await
        {
            Ok(user_data) => {
                ctx.all_data["github"]["user"] = user_data;
                tracing::info!("✓ GitHub user data fetched");
            }
            Err(e) => {
                tracing::warn!("GitHub user fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "github", "user", e);
            }
        }

        // 获取仓库列表
        match ctx
            .fetcher
            .fetch_github_repos(github_username, github_token)
            .await
        {
            Ok(repos) => {
                ctx.all_data["github"]["repos"] = json!(repos);
                tracing::info!("✓ GitHub repos fetched: {} repositories", repos.len());
            }
            Err(e) => {
                tracing::warn!("GitHub repos fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "github", "repos", e);
            }
        }

        // 获取贡献历史
        match ctx
            .fetcher
            .fetch_github_contributions(github_username, github_token)
            .await
        {
            Ok(contributions) => {
                tracing::info!(
                    "✓ GitHub contributions fetched: {} days",
                    contributions.len()
                );
                if !contributions.is_empty() {
                    tracing::debug!("First contribution: {:?}", contributions.first());
                    tracing::debug!("Last contribution: {:?}", contributions.last());
                }
                ctx.all_data["github"]["contribution_calendar"] = json!(contributions);
            }
            Err(e) => {
                // 贡献图为增强项：失败不阻断刷新成功，仅记日志
                tracing::warn!("⚠ GitHub contributions fetch failed: {}", e);
            }
        }

        // 保存GitHub数据到数据库
        if !ctx.all_data["github"].is_null() {
            if let Err(e) = ctx
                .metadata_service
                .save_platform_metadata(ctx.user_id, "github", ctx.all_data["github"].clone())
                .await
            {
                tracing::error!("Failed to save GitHub metadata to database: {}", e);
            }
        }
    }
}

pub(super) async fn fetch_bilibili(ctx: &mut FetchCtx<'_>) {
    if let Some(uid_str) = &ctx.config.bilibili_uid {
        if let Ok(uid) = uid_str.parse::<i64>() {
            match ctx.fetcher.fetch_bilibili_user(uid).await {
                Ok(user_data) => {
                    // 与 Steam/GitHub 一致用 `user`；smart_filter / get_user_info 都读这个键
                    // （旧版曾写成 user_info，导致过滤与资料页读不到用户信息）
                    ctx.all_data["bilibili"]["user"] = json!(user_data);
                    tracing::info!(
                        "✓ Bilibili user data fetched: {} (mid={}, lv{}, {} followers)",
                        user_data.name,
                        user_data.mid,
                        user_data.level,
                        user_data.follower
                    );
                }
                Err(e) => {
                    tracing::warn!("Bilibili user fetch failed: {}", e);
                    note_fetch_error(&mut ctx.fetch_errors, "bilibili", "user", e);
                }
            }

            // 获取追番/追剧数据
            match ctx.fetcher.fetch_all_bilibili_bangumi(uid).await {
                Ok(bangumi_data) => {
                    ctx.all_data["bilibili"]["bangumi"] = json!(bangumi_data);
                    tracing::info!(
                        "✓ Bilibili bangumi data fetched: {} items",
                        bangumi_data.len()
                    );
                }
                Err(e) => {
                    tracing::warn!("Bilibili bangumi fetch failed: {}", e);
                    note_fetch_error(&mut ctx.fetch_errors, "bilibili", "bangumi", e);
                }
            }

            // 获取收藏夹
            match ctx.fetcher.fetch_bilibili_favorites(uid).await {
                Ok(favorites) => {
                    ctx.all_data["bilibili"]["favorites"] = json!(favorites);
                    tracing::info!("✓ Bilibili favorites fetched: {} items", favorites.len());
                }
                Err(e) => {
                    tracing::warn!("Bilibili favorites fetch failed: {}", e);
                    note_fetch_error(&mut ctx.fetch_errors, "bilibili", "favorites", e);
                }
            }

            // 保存Bilibili数据到数据库
            if !ctx.all_data["bilibili"].is_null() {
                if let Err(e) = ctx
                    .metadata_service
                    .save_platform_metadata(
                        ctx.user_id,
                        "bilibili",
                        ctx.all_data["bilibili"].clone(),
                    )
                    .await
                {
                    tracing::error!("Failed to save Bilibili metadata to database: {}", e);
                }
            }
        }
    }
}

pub(super) async fn fetch_steam(ctx: &mut FetchCtx<'_>) {
    if let (Some(api_key), Some(steam_id)) = (&ctx.config.steam_api_key, &ctx.config.steam_id) {
        match ctx.fetcher.fetch_steam_user(api_key, steam_id).await {
            Ok(user_data) => {
                ctx.all_data["steam"]["user"] = json!(user_data);
                tracing::info!("✓ Steam user data fetched");
            }
            Err(e) => {
                tracing::warn!("Steam user fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "steam", "user", e);
            }
        }

        match ctx.fetcher.fetch_steam_games(api_key, steam_id).await {
            Ok(games_data) => {
                // 过滤：只保留游玩时间>=180分钟(3小时)的游戏
                let filtered_games: Vec<_> = games_data
                    .into_iter()
                    .filter(|game| game.playtime_forever >= 180)
                    .collect();

                let total_count = filtered_games.len();
                ctx.all_data["steam"]["games"] = json!(filtered_games);
                tracing::info!(
                    "✓ Steam games fetched: {} games (filtered >=3h)",
                    total_count
                );
            }
            Err(e) => {
                tracing::warn!("Steam games fetch failed: {}", e);
                note_fetch_error(&mut ctx.fetch_errors, "steam", "games", e);
            }
        }

        // 保存Steam数据到数据库
        if !ctx.all_data["steam"].is_null() {
            if let Err(e) = ctx
                .metadata_service
                .save_platform_metadata(ctx.user_id, "steam", ctx.all_data["steam"].clone())
                .await
            {
                tracing::error!("Failed to save Steam metadata to database: {}", e);
            }
        }
    }
}

pub(super) async fn fetch_netease(ctx: &mut FetchCtx<'_>) {
    tracing::info!("🎵 Should fetch netease: checking config...");
    tracing::info!(
        "🎵 Config netease_user_id: {:?}",
        ctx.config.netease_user_id
    );

    if let Some(user_id_str) = &ctx.config.netease_user_id {
        tracing::info!("🎵 Netease user_id found in config: {}", user_id_str);
        if let Ok(netease_user_id) = user_id_str.parse::<i64>() {
            tracing::info!("🎵 Parsed netease_user_id: {}", netease_user_id);
            // 获取用户信息
            match ctx.fetcher.fetch_netease_user(netease_user_id).await {
                Ok(user_data) => {
                    // 提取 profile 字段（API 返回格式：{ "code": 200, "profile": {...} }）
                    if let Some(profile) = user_data.get("profile") {
                        ctx.all_data["netease"]["profile"] = profile.clone();
                        tracing::info!("✓ Netease user data fetched");
                    } else {
                        // 如果没有 profile 字段，使用整个响应（兼容旧版本）
                        ctx.all_data["netease"]["profile"] = user_data;
                        tracing::warn!(
                            "⚠️ Netease API response missing 'profile' field, using full response"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!("Netease user fetch failed: {}", e);
                    note_fetch_error(&mut ctx.fetch_errors, "netease", "user", e);
                }
            }

            // 获取喜欢的歌曲（分批处理，避免内存占用过大）
            tracing::info!("🎵 Fetching Netease liked songs...");
            match ctx.fetcher.fetch_netease_liked_songs(netease_user_id).await {
                Ok(songs) => {
                    let total_songs = songs.len();
                    tracing::info!("🎵 Total songs fetched: {}", total_songs);

                    // 直接保存完整歌曲列表
                    ctx.all_data["netease"]["liked_songs"] = json!(songs);
                    tracing::info!(
                        "✓ Netease Cloud Music liked songs stored: {} songs",
                        total_songs
                    );
                }
                Err(e) => {
                    tracing::warn!("Netease Cloud Music fetch failed: {}", e);
                    note_fetch_error(&mut ctx.fetch_errors, "netease", "liked_songs", e);
                }
            }

            // 保存网易云音乐数据到数据库
            if !ctx.all_data["netease"].is_null() {
                if let Err(e) = ctx
                    .metadata_service
                    .save_platform_metadata(ctx.user_id, "netease", ctx.all_data["netease"].clone())
                    .await
                {
                    tracing::error!("Failed to save Netease metadata to database: {}", e);
                }
            }
        }
    }
}

pub(super) async fn fetch_bangumi(ctx: &mut FetchCtx<'_>) {
    let access_token = ctx.config.bangumi_access_token.as_deref();
    let user_agent = ctx.config.bangumi_user_agent.as_deref();
    let configured_username = ctx
        .config
        .bangumi_username
        .as_deref()
        .filter(|username| !username.trim().is_empty());

    let user_result = if let Some(username) = configured_username {
        ctx.fetcher
            .fetch_bangumi_user(username, access_token, user_agent)
            .await
    } else if let Some(token) = access_token {
        ctx.fetcher.fetch_bangumi_me(token, user_agent).await
    } else {
        Err(anyhow::anyhow!(
            "Bangumi username or access token is required"
        ))
    };

    match user_result {
        Ok(user_data) => {
            let resolved_username = user_data
                .get("username")
                .and_then(|v| v.as_str())
                .or(configured_username)
                .map(str::to_string);
            ctx.all_data["bangumi"]["user"] = user_data;
            tracing::info!("✓ Bangumi user data fetched");

            if let Some(username) = resolved_username.as_deref() {
                match ctx
                    .fetcher
                    .fetch_bangumi_collections(username, access_token, user_agent)
                    .await
                {
                    Ok(collections) => {
                        let total_count = collections.len();
                        ctx.all_data["bangumi"]["collections"] = json!(collections);
                        tracing::info!("✓ Bangumi collections fetched: {} items", total_count);
                    }
                    Err(e) => {
                        tracing::warn!("Bangumi collections fetch failed: {}", e);
                        note_fetch_error(&mut ctx.fetch_errors, "bangumi", "collections", e);
                    }
                }
            } else {
                tracing::warn!("Bangumi user data did not include username; skipping collections");
                note_fetch_error(
                    &mut ctx.fetch_errors,
                    "bangumi",
                    "collections",
                    "user data missing username",
                );
            }
        }
        Err(e) => {
            tracing::warn!("Bangumi user fetch failed: {}", e);
            note_fetch_error(&mut ctx.fetch_errors, "bangumi", "user", e);
        }
    }

    if !ctx.all_data["bangumi"].is_null() {
        if let Err(e) = ctx
            .metadata_service
            .save_platform_metadata(ctx.user_id, "bangumi", ctx.all_data["bangumi"].clone())
            .await
        {
            tracing::error!("Failed to save Bangumi metadata to database: {}", e);
        }
    }
}
