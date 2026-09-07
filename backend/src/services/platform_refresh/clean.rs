//! In-place 5W1H cleanup of fetched platform payloads.

use serde_json::{json, Value};

/// 清洗平台数据，只保留核心信息（符合5W1H原则）
/// 优化：原地修改减少内存峰值，添加数据量限制
pub(super) fn clean_platform_data(data: &mut Value) {
    // 内存保护：各平台最大数据量限制
    const MAX_GITHUB_REPOS: usize = 200;
    const MAX_STEAM_GAMES: usize = 500;
    const MAX_BILIBILI_VIDEOS: usize = 100;
    const MAX_BILIBILI_BANGUMI: usize = 100;
    const MAX_SONGS_TO_CLEAN: usize = 5000;
    const MAX_BANGUMI_COLLECTIONS: usize = 1000;
    const MAX_X_TWEETS: usize = 100;

    // 清洗 GitHub 仓库数据 - 原地修改
    if let Some(repos) = data["github"]["repos"].as_array_mut() {
        // 限制仓库数量
        if repos.len() > MAX_GITHUB_REPOS {
            tracing::warn!(
                "⚠️ Truncating GitHub repos from {} to {}",
                repos.len(),
                MAX_GITHUB_REPOS
            );
            repos.truncate(MAX_GITHUB_REPOS);
        }

        for repo in repos.iter_mut() {
            if let Some(obj) = repo.as_object_mut() {
                // 保留的字段
                let name = obj.get("name").cloned();
                let description = obj.get("description").cloned();
                let language = obj.get("language").cloned();
                let stargazers_count = obj.get("stargazers_count").cloned();
                let forks_count = obj.get("forks_count").cloned();
                let created_at = obj.get("created_at").cloned();
                let updated_at = obj.get("updated_at").cloned();
                let topics = obj.get("topics").cloned();
                let html_url = obj.get("html_url").cloned();

                // 清空对象并只保留必要字段
                obj.clear();

                if let Some(v) = name {
                    obj.insert("name".to_string(), v);
                }
                if let Some(v) = description {
                    obj.insert("description".to_string(), v);
                }
                if let Some(v) = language {
                    obj.insert("language".to_string(), v);
                }
                if let Some(v) = stargazers_count {
                    obj.insert("stargazers_count".to_string(), v);
                }
                if let Some(v) = forks_count {
                    obj.insert("forks_count".to_string(), v);
                }
                if let Some(v) = created_at {
                    obj.insert("created_at".to_string(), v);
                }
                if let Some(v) = updated_at {
                    obj.insert("updated_at".to_string(), v);
                }
                if let Some(v) = topics {
                    obj.insert("topics".to_string(), v);
                }
                if let Some(v) = html_url {
                    obj.insert("html_url".to_string(), v);
                }
            }
        }
    }

    // 清洗 GitHub 用户信息 - 原地修改
    if let Some(user) = data["github"]["user"].as_object_mut() {
        let id = user.get("id").cloned();
        let login = user.get("login").cloned();
        let name = user.get("name").cloned();
        let bio = user.get("bio").cloned();
        let avatar_url = user.get("avatar_url").cloned();
        let company = user.get("company").cloned();
        let location = user.get("location").cloned();
        let public_repos = user.get("public_repos").cloned();
        let followers = user.get("followers").cloned();
        let following = user.get("following").cloned();
        let created_at = user.get("created_at").cloned();

        user.clear();

        if let Some(v) = id {
            user.insert("id".to_string(), v);
        }
        if let Some(v) = login {
            user.insert("login".to_string(), v);
        }
        if let Some(v) = name {
            user.insert("name".to_string(), v);
        }
        if let Some(v) = bio {
            user.insert("bio".to_string(), v);
        }
        if let Some(v) = avatar_url {
            user.insert("avatar_url".to_string(), v);
        }
        if let Some(v) = company {
            user.insert("company".to_string(), v);
        }
        if let Some(v) = location {
            user.insert("location".to_string(), v);
        }
        if let Some(v) = public_repos {
            user.insert("public_repos".to_string(), v);
        }
        if let Some(v) = followers {
            user.insert("followers".to_string(), v);
        }
        if let Some(v) = following {
            user.insert("following".to_string(), v);
        }
        if let Some(v) = created_at {
            user.insert("created_at".to_string(), v);
        }
    }

    // 清洗 Steam 游戏数据 - 原地修改
    if let Some(games) = data["steam"]["games"].as_array_mut() {
        // 限制游戏数量
        if games.len() > MAX_STEAM_GAMES {
            tracing::warn!(
                "⚠️ Truncating Steam games from {} to {}",
                games.len(),
                MAX_STEAM_GAMES
            );
            games.truncate(MAX_STEAM_GAMES);
        }

        for game in games.iter_mut() {
            if let Some(obj) = game.as_object_mut() {
                let appid = obj.get("appid").cloned();
                let name = obj.get("name").cloned();
                let playtime_forever = obj.get("playtime_forever").cloned();
                let playtime_2weeks = obj.get("playtime_2weeks").cloned();

                obj.clear();

                if let Some(v) = appid {
                    obj.insert("appid".to_string(), v);
                }
                if let Some(v) = name {
                    obj.insert("name".to_string(), v);
                }
                if let Some(v) = playtime_forever {
                    obj.insert("playtime_forever".to_string(), v);
                }
                if let Some(v) = playtime_2weeks {
                    obj.insert("playtime_2weeks".to_string(), v);
                }
            }
        }
    }

    // 清洗 Steam 用户信息 - 原地修改
    if let Some(user) = data["steam"]["user"].as_object_mut() {
        let steamid = user.get("steamid").cloned();
        let personaname = user.get("personaname").cloned();
        let avatar = user.get("avatar").cloned();
        let avatarfull = user.get("avatarfull").cloned();
        let profileurl = user.get("profileurl").cloned();
        let timecreated = user.get("timecreated").cloned();
        let communityvisibilitystate = user.get("communityvisibilitystate").cloned();
        let personastate = user.get("personastate").cloned();
        let personastate_label = user.get("personastate_label").cloned();
        let lastlogoff = user.get("lastlogoff").cloned();
        let gameid = user.get("gameid").cloned();
        let gameextrainfo = user.get("gameextrainfo").cloned();

        user.clear();

        if let Some(v) = steamid {
            user.insert("steamid".to_string(), v);
        }
        if let Some(v) = personaname {
            user.insert("personaname".to_string(), v);
        }
        if let Some(v) = avatar {
            user.insert("avatar".to_string(), v);
        }
        if let Some(v) = avatarfull {
            user.insert("avatarfull".to_string(), v);
        }
        if let Some(v) = profileurl {
            user.insert("profileurl".to_string(), v);
        }
        if let Some(v) = timecreated {
            user.insert("timecreated".to_string(), v);
        }
        if let Some(v) = communityvisibilitystate {
            user.insert("communityvisibilitystate".to_string(), v);
        }
        if let Some(v) = personastate {
            user.insert("personastate".to_string(), v);
        }
        if let Some(v) = personastate_label {
            user.insert("personastate_label".to_string(), v);
        }
        if let Some(v) = lastlogoff {
            user.insert("lastlogoff".to_string(), v);
        }
        if let Some(v) = gameid {
            user.insert("gameid".to_string(), v);
        }
        if let Some(v) = gameextrainfo {
            user.insert("gameextrainfo".to_string(), v);
        }
    }

    // 清洗 Bilibili 数据 - 原地修改，添加数量限制
    if let Some(bilibili) = data.get_mut("bilibili") {
        // 清洗收藏夹视频
        if let Some(favorites) = bilibili.get_mut("favorites") {
            if let Some(fav_array) = favorites.as_array_mut() {
                for fav in fav_array.iter_mut() {
                    if let Some(videos) = fav.get_mut("videos") {
                        if let Some(videos_array) = videos.as_array_mut() {
                            if videos_array.len() > MAX_BILIBILI_VIDEOS {
                                tracing::debug!(
                                    "⚠️ Truncating Bilibili videos from {} to {}",
                                    videos_array.len(),
                                    MAX_BILIBILI_VIDEOS
                                );
                                videos_array.truncate(MAX_BILIBILI_VIDEOS);
                            }
                        }
                    }
                }
            }
        }

        // 清洗追番数据
        if let Some(bangumi) = bilibili.get_mut("bangumi") {
            if let Some(bangumi_array) = bangumi.as_array_mut() {
                if bangumi_array.len() > MAX_BILIBILI_BANGUMI {
                    tracing::debug!(
                        "⚠️ Truncating Bilibili bangumi from {} to {}",
                        bangumi_array.len(),
                        MAX_BILIBILI_BANGUMI
                    );
                    bangumi_array.truncate(MAX_BILIBILI_BANGUMI);
                }
            }
        }
    }

    // 清洗网易云音乐数据 - 保留核心字段（优化内存使用）
    if let Some(netease) = data.get_mut("netease") {
        // 优化：原地修改而不是创建新数组，减少内存峰值
        if let Some(songs_value) = netease.get_mut("liked_songs") {
            if let Some(songs_array) = songs_value.as_array_mut() {
                let total_songs = songs_array.len();
                tracing::debug!("🧹 Cleaning {} netease songs in-place...", total_songs);

                // 限制歌曲数量，避免处理过多数据
                if songs_array.len() > MAX_SONGS_TO_CLEAN {
                    tracing::warn!(
                        "⚠️ Truncating songs from {} to {} to prevent memory issues",
                        songs_array.len(),
                        MAX_SONGS_TO_CLEAN
                    );
                    songs_array.truncate(MAX_SONGS_TO_CLEAN);
                }

                // 原地清洗每首歌曲，只保留必要字段（含 fee/isVip 供资料库 VIP 角标）
                for song in songs_array.iter_mut() {
                    if let Some(obj) = song.as_object_mut() {
                        // 保留的字段
                        let id = obj.get("id").cloned();
                        let name = obj.get("name").cloned();
                        let ar = obj.get("ar").cloned();
                        let artists = obj.get("artists").cloned();
                        let al = obj.get("al").cloned();
                        let pic_url = obj.get("picUrl").cloned();
                        let dt = obj.get("dt").cloned();
                        let fee = obj
                            .get("fee")
                            .cloned()
                            .or_else(|| obj.get("privilege").and_then(|p| p.get("fee")).cloned());
                        let is_vip = obj.get("isVip").or_else(|| obj.get("is_vip")).cloned();

                        // 清空对象并只保留必要字段
                        obj.clear();

                        if let Some(v) = id {
                            obj.insert("id".to_string(), v);
                        }
                        if let Some(v) = name {
                            obj.insert("name".to_string(), v);
                        }
                        if let Some(v) = ar {
                            obj.insert("ar".to_string(), v);
                        }
                        if let Some(v) = artists {
                            obj.insert("artists".to_string(), v);
                        }
                        if let Some(mut al_val) = al {
                            // 清洗专辑信息 - 原地修改避免额外分配
                            if let Some(al_obj) = al_val.as_object_mut() {
                                let id = al_obj.get("id").cloned();
                                let name = al_obj.get("name").cloned();
                                let pic_url = al_obj.get("picUrl").cloned();

                                al_obj.clear();

                                if let Some(v) = id {
                                    al_obj.insert("id".to_string(), v);
                                }
                                if let Some(v) = name {
                                    al_obj.insert("name".to_string(), v);
                                }
                                if let Some(v) = pic_url {
                                    al_obj.insert("picUrl".to_string(), v);
                                }
                            }
                            obj.insert("al".to_string(), al_val);
                        }
                        if let Some(v) = pic_url {
                            obj.insert("picUrl".to_string(), v);
                        }
                        if let Some(v) = dt {
                            obj.insert("dt".to_string(), v);
                        }
                        if let Some(v) = fee {
                            let fee_n = v.as_i64().unwrap_or(0);
                            obj.insert("fee".to_string(), v);
                            let vip = is_vip
                                .as_ref()
                                .and_then(|b| b.as_bool())
                                .unwrap_or(fee_n == 1 || fee_n == 4);
                            obj.insert("isVip".to_string(), json!(vip));
                        } else if let Some(v) = is_vip {
                            obj.insert("isVip".to_string(), v);
                        }
                    }
                }

                tracing::debug!("✅ Cleaned {} songs in-place", songs_array.len());
            }
        }

        // 清洗 profile 信息
        if let Some(profile) = netease.get("profile").cloned() {
            if let Some(profile_obj) = profile.as_object() {
                let cleaned_profile = json!({
                    "userId": profile_obj.get("userId"),
                    "nickname": profile_obj.get("nickname"),
                    "avatarUrl": profile_obj.get("avatarUrl"),
                    "backgroundUrl": profile_obj.get("backgroundUrl"),
                    "signature": profile_obj.get("signature"),
                    "gender": profile_obj.get("gender"),
                    "birthday": profile_obj.get("birthday"),
                    "province": profile_obj.get("province"),
                    "city": profile_obj.get("city"),
                    "followeds": profile_obj.get("followeds"),
                    "follows": profile_obj.get("follows"),
                    "eventCount": profile_obj.get("eventCount"),
                    "playlistCount": profile_obj.get("playlistCount"),
                    "level": profile_obj.get("level"),
                });
                if let Some(obj) = netease.as_object_mut() {
                    obj.insert("profile".to_string(), cleaned_profile);
                }
            }
        }
    }

    // 清洗 Bangumi 收藏数据 - 保留资料库、报告和分析需要的核心字段
    if let Some(bangumi) = data.get_mut("bangumi") {
        if let Some(collections) = bangumi
            .get_mut("collections")
            .and_then(|value| value.as_array_mut())
        {
            if collections.len() > MAX_BANGUMI_COLLECTIONS {
                tracing::warn!(
                    "⚠️ Truncating Bangumi collections from {} to {}",
                    collections.len(),
                    MAX_BANGUMI_COLLECTIONS
                );
                collections.truncate(MAX_BANGUMI_COLLECTIONS);
            }

            for collection in collections.iter_mut() {
                if let Some(obj) = collection.as_object_mut() {
                    let subject_id = obj.get("subject_id").cloned();
                    let subject_type = obj.get("subject_type").cloned();
                    let rate = obj.get("rate").cloned();
                    let collection_type = obj.get("type").cloned();
                    let comment = obj.get("comment").cloned();
                    let tags = obj.get("tags").cloned();
                    let ep_status = obj.get("ep_status").cloned();
                    let vol_status = obj.get("vol_status").cloned();
                    let updated_at = obj.get("updated_at").cloned();
                    let private = obj.get("private").cloned();
                    let subject = obj.get("subject").cloned();

                    obj.clear();

                    if let Some(v) = subject_id {
                        obj.insert("subject_id".to_string(), v);
                    }
                    if let Some(v) = subject_type {
                        obj.insert("subject_type".to_string(), v);
                    }
                    if let Some(v) = rate {
                        obj.insert("rate".to_string(), v);
                    }
                    if let Some(v) = collection_type {
                        obj.insert("type".to_string(), v);
                    }
                    if let Some(v) = comment {
                        obj.insert("comment".to_string(), v);
                    }
                    if let Some(v) = tags {
                        obj.insert("tags".to_string(), v);
                    }
                    if let Some(v) = ep_status {
                        obj.insert("ep_status".to_string(), v);
                    }
                    if let Some(v) = vol_status {
                        obj.insert("vol_status".to_string(), v);
                    }
                    if let Some(v) = updated_at {
                        obj.insert("updated_at".to_string(), v);
                    }
                    if let Some(v) = private {
                        obj.insert("private".to_string(), v);
                    }
                    if let Some(mut subject_value) = subject {
                        if let Some(subject_obj) = subject_value.as_object_mut() {
                            let id = subject_obj.get("id").cloned();
                            let subject_type = subject_obj.get("type").cloned();
                            let name = subject_obj.get("name").cloned();
                            let name_cn = subject_obj.get("name_cn").cloned();
                            let images = subject_obj.get("images").cloned();
                            let date = subject_obj.get("date").cloned();
                            let platform = subject_obj.get("platform").cloned();
                            let score = subject_obj.get("score").cloned();
                            let rank = subject_obj.get("rank").cloned();
                            let tags = subject_obj.get("tags").cloned();

                            subject_obj.clear();
                            if let Some(v) = id {
                                subject_obj.insert("id".to_string(), v);
                            }
                            if let Some(v) = subject_type {
                                subject_obj.insert("type".to_string(), v);
                            }
                            if let Some(v) = name {
                                subject_obj.insert("name".to_string(), v);
                            }
                            if let Some(v) = name_cn {
                                subject_obj.insert("name_cn".to_string(), v);
                            }
                            if let Some(v) = images {
                                subject_obj.insert("images".to_string(), v);
                            }
                            if let Some(v) = date {
                                subject_obj.insert("date".to_string(), v);
                            }
                            if let Some(v) = platform {
                                subject_obj.insert("platform".to_string(), v);
                            }
                            if let Some(v) = score {
                                subject_obj.insert("score".to_string(), v);
                            }
                            if let Some(v) = rank {
                                subject_obj.insert("rank".to_string(), v);
                            }
                            if let Some(v) = tags {
                                subject_obj.insert("tags".to_string(), v);
                            }
                        }
                        obj.insert("subject".to_string(), subject_value);
                    }
                }
            }
        }
    }

    // 清洗 X (Twitter) 数据
    if let Some(x_data) = data.get_mut("x") {
        // 用户字段精简
        if let Some(user) = x_data.get_mut("user").and_then(|v| v.as_object_mut()) {
            let id = user.get("id").cloned();
            let username = user.get("username").cloned();
            let name = user.get("name").cloned();
            let description = user.get("description").cloned();
            let profile_image_url = user.get("profile_image_url").cloned();
            let public_metrics = user.get("public_metrics").cloned();
            let verified = user.get("verified").cloned();
            let verified_type = user.get("verified_type").cloned();
            let created_at = user.get("created_at").cloned();
            let location = user.get("location").cloned();
            let url = user.get("url").cloned();
            let protected = user.get("protected").cloned();

            user.clear();
            if let Some(v) = id {
                user.insert("id".to_string(), v);
            }
            if let Some(v) = username {
                user.insert("username".to_string(), v);
            }
            if let Some(v) = name {
                user.insert("name".to_string(), v);
            }
            if let Some(v) = description {
                user.insert("description".to_string(), v);
            }
            if let Some(v) = profile_image_url {
                user.insert("profile_image_url".to_string(), v);
            }
            if let Some(v) = public_metrics {
                user.insert("public_metrics".to_string(), v);
            }
            if let Some(v) = verified {
                user.insert("verified".to_string(), v);
            }
            if let Some(v) = verified_type {
                user.insert("verified_type".to_string(), v);
            }
            if let Some(v) = created_at {
                user.insert("created_at".to_string(), v);
            }
            if let Some(v) = location {
                user.insert("location".to_string(), v);
            }
            if let Some(v) = url {
                user.insert("url".to_string(), v);
            }
            if let Some(v) = protected {
                user.insert("protected".to_string(), v);
            }
        }

        // 推文字段精简
        let clean_tweet = |tweet: &mut Value| {
            if let Some(obj) = tweet.as_object_mut() {
                let id = obj.get("id").cloned();
                let text = obj.get("text").cloned();
                let created_at = obj.get("created_at").cloned();
                let public_metrics = obj.get("public_metrics").cloned();
                let lang = obj.get("lang").cloned();
                let author = obj.get("author").cloned();
                let author_id = obj.get("author_id").cloned();

                obj.clear();
                if let Some(v) = id {
                    obj.insert("id".to_string(), v);
                }
                if let Some(v) = text {
                    obj.insert("text".to_string(), v);
                }
                if let Some(v) = created_at {
                    obj.insert("created_at".to_string(), v);
                }
                if let Some(v) = public_metrics {
                    obj.insert("public_metrics".to_string(), v);
                }
                if let Some(v) = lang {
                    obj.insert("lang".to_string(), v);
                }
                if let Some(v) = author {
                    obj.insert("author".to_string(), v);
                }
                if let Some(v) = author_id {
                    obj.insert("author_id".to_string(), v);
                }
            }
        };

        if let Some(tweets) = x_data.get_mut("tweets").and_then(|v| v.as_array_mut()) {
            if tweets.len() > MAX_X_TWEETS {
                tweets.truncate(MAX_X_TWEETS);
            }
            for tweet in tweets.iter_mut() {
                clean_tweet(tweet);
            }
        }

        // 关注列表字段精简：只保留 SmartFilter 消费的字段（entities 等全量字段体积很大）
        if let Some(following) = x_data.get_mut("following").and_then(|v| v.as_array_mut()) {
            for account in following.iter_mut() {
                if let Some(obj) = account.as_object_mut() {
                    obj.retain(|key, _| {
                        matches!(
                            key.as_str(),
                            "id" | "username"
                                | "name"
                                | "description"
                                | "verified"
                                | "public_metrics"
                                | "profile_image_url"
                        )
                    });
                }
            }
        }

        // 不再同步 likes；清理历史字段
        if let Some(obj) = x_data.as_object_mut() {
            obj.remove("liked_tweets");
        }
    }

    tracing::info!("✓ Platform data cleaned (removed unnecessary fields)");
}
