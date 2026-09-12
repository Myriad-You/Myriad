//! Local fallback when the report model is missing or returns unusable JSON.
//! Same field contract as the live path; no colon-label insights, no banned hooks.

use serde_json::{json, Value};

use crate::services::smart_filter::{ContentAnalysis, SmartFilteredData};

fn t(locale: &str, key: &str) -> String {
    crate::i18n::reports(locale, key)
}

pub(crate) fn generate_mock_report(
    metadata: &SmartFilteredData,
    _platform: &str,
    locale: &str,
) -> Result<(String, Vec<String>, Value), String> {
    Ok(match &metadata.content_analysis {
        ContentAnalysis::Bilibili(analysis) => {
            let anime = analysis
                .anime_analysis
                .iter()
                .flat_map(|item| item.examples.iter())
                .map(|s| s.as_str())
                .find(|s| !s.is_empty());
            let video = analysis
                .recent_videos
                .iter()
                .map(|v| v.title.as_str())
                .find(|s| !s.is_empty());
            let mix = analysis
                .anime_analysis
                .iter()
                .map(|item| format!("{} {}", item.count, category_label(locale, &item.category)))
                .take(2)
                .collect::<Vec<_>>()
                .join(" / ");
            let danmaku = analysis
                .anime_analysis
                .iter()
                .flat_map(|item| item.examples.iter())
                .chain(analysis.recent_videos.iter().map(|v| &v.title))
                .take(6)
                .map(|title| title.chars().take(12).collect::<String>())
                .collect::<Vec<_>>();
            (
                match (anime, video) {
                    (Some(_), Some(_)) => t(locale, "bili.diverge"),
                    (Some(_), None) => t(locale, "bili.leansAnime"),
                    (None, Some(_)) => t(locale, "bili.watchingUploads"),
                    (None, None) => t(locale, "bili.empty"),
                },
                take_insights([
                    anime.map(|name| format!("{}《{}》", t(locale, "bili.stillListed"), name)),
                    video.map(|title| format!("{}《{}》", t(locale, "bili.watching"), title)),
                    (!mix.is_empty()).then(|| format!("{} {}", t(locale, "bili.mix"), mix)),
                ]),
                json!({ "danmaku": danmaku }),
            )
        }
        ContentAnalysis::Steam(analysis) => {
            let top = analysis.recent_games.iter().max_by_key(|g| g.playtime);
            let genre = analysis.genre_analysis.first().map(|g| g.genre.as_str());
            let concentrated = top.is_some_and(|g| {
                analysis.total_playtime_minutes > 0
                    && g.playtime * 2 >= analysis.total_playtime_minutes
            });
            let player_type = if concentrated {
                "hardcore"
            } else if analysis.games_count > analysis.recent_games.len().saturating_mul(4)
                && analysis.recent_games.len() < 8
            {
                "casual"
            } else {
                "balanced"
            };
            (
                if concentrated {
                    t(locale, "steam.hoursPile")
                } else {
                    t(locale, "steam.libraryOutgrows")
                },
                take_insights([
                    top.map(|g| format!("《{}》{}", g.name, t(locale, "steam.mostMinutes"))),
                    genre.map(|g| format!("{}{}", t(locale, "steam.genreLeans"), g)),
                    Some(format!(
                        "{} {} · {} {}",
                        t(locale, "steam.library"),
                        analysis.games_count,
                        t(locale, "steam.recent"),
                        analysis.recent_games.len()
                    )),
                ]),
                json!({
                    "player_type": player_type,
                    "hardcore_score": if concentrated { 72 } else { 40 },
                }),
            )
        }
        ContentAnalysis::GitHub(analysis) => {
            let top = analysis
                .recent_repos
                .iter()
                .max_by_key(|repo| repo.stars.unwrap_or(0));
            let lang = analysis
                .language_distribution
                .iter()
                .max_by_key(|(_, n)| *n)
                .map(|(name, _)| name.as_str());
            let cal_days = analysis
                .contribution_calendar
                .as_ref()
                .map(|cal| cal.iter().filter(|d| d.count > 0).count())
                .unwrap_or(0);
            (
                t(locale, "github.starsNotCalendar"),
                take_insights([
                    top.and_then(|repo| {
                        repo.stars.map(|n| {
                            format!("{} {} {} star", repo.name, t(locale, "github.top"), n)
                        })
                    }),
                    (cal_days > 0).then(|| {
                        format!(
                            "{} {} {}",
                            t(locale, "github.calendarHas"),
                            cal_days,
                            t(locale, "github.activeDays")
                        )
                    }),
                    lang.map(|name| format!("{} {}", t(locale, "github.topLanguage"), name)),
                ]),
                json!({}),
            )
        }
        ContentAnalysis::YouTube(analysis) => {
            let empty = analysis.video_count == 0 && analysis.recent_videos.is_empty();
            let latest = analysis.recent_videos.first().map(|v| v.title.as_str());
            let avg = if analysis.video_count > 0 {
                analysis.view_count / analysis.video_count
            } else {
                0
            };
            if empty {
                (
                    t(locale, "yt.noPublic"),
                    vec![t(locale, "yt.videoCountZero")],
                    json!({
                        "vibe": t(locale, "yt.coldShell"),
                        "channel_type": t(locale, "yt.coldStart"),
                    }),
                )
            } else {
                let stale = youtube_upload_stale(
                    analysis
                        .recent_videos
                        .first()
                        .and_then(|v| v.published_at.as_deref()),
                );
                (
                    t(locale, "yt.subsSplit"),
                    take_insights([
                        Some(format!(
                            "{} {} · {} {}",
                            t(locale, "yt.avgViews"),
                            avg,
                            t(locale, "yt.subs"),
                            analysis.subscriber_count
                        )),
                        latest.map(|title| format!("{}《{}》", t(locale, "yt.latest"), title)),
                        analysis
                            .recent_videos
                            .first()
                            .and_then(|v| v.published_at.as_deref())
                            .map(|at| format!("{} {}", t(locale, "yt.uploaded"), at)),
                    ]),
                    json!({
                        "vibe": t(locale, "yt.hasVideosVibe"),
                        "channel_type": if stale {
                            t(locale, "yt.dormant")
                        } else {
                            t(locale, "yt.hasVideos")
                        },
                    }),
                )
            }
        }
        ContentAnalysis::Netease(analysis) => {
            let artist = analysis
                .artist_analysis
                .favorite_artists
                .first()
                .map(|s| s.as_str())
                .or_else(|| analysis.recent_songs.first().map(|s| s.artist.as_str()));
            let region = analysis
                .artist_analysis
                .region_distribution
                .iter()
                .max_by_key(|(_, n)| *n)
                .map(|(name, _)| name.as_str());
            let genre = analysis
                .artist_analysis
                .genre_analysis
                .first()
                .map(|g| g.genre.as_str());
            const MOOD_COLORS: [&str; 6] = [
                "#5B6ABF", "#3D4A7A", "#9AA4C2", "#7B68EE", "#FF6B9D", "#4ECDC4",
            ];
            let mut mood_tags: Vec<String> = Vec::new();
            let mut push_tag = |tag: &str| {
                if !tag.is_empty() && !mood_tags.iter().any(|existing| existing == tag) {
                    mood_tags.push(tag.to_string());
                }
            };
            for item in &analysis.artist_analysis.genre_analysis {
                push_tag(item.genre.as_str());
            }
            let mut regions: Vec<_> = analysis
                .artist_analysis
                .region_distribution
                .iter()
                .collect();
            regions.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            for (name, _) in regions {
                push_tag(name.as_str());
            }
            for name in &analysis.artist_analysis.favorite_artists {
                push_tag(name.as_str());
            }
            for song in &analysis.recent_songs {
                push_tag(song.artist.as_str());
            }
            if let Some(tag) = artist {
                push_tag(tag);
            }
            let mood_keywords: Vec<Value> = mood_tags
                .into_iter()
                .take(6)
                .enumerate()
                .map(|(i, tag)| json!({ "tag": tag, "color": MOOD_COLORS[i] }))
                .collect();
            (
                t(locale, "netease.genreHonest"),
                take_insights([
                    artist.map(|name| format!("{} {}", t(locale, "netease.keepsShowing"), name)),
                    region.map(|name| format!("{}{}", t(locale, "netease.regionLeans"), name)),
                    genre.map(|name| format!("{}{}", t(locale, "netease.genreLeans"), name)),
                ]),
                json!({
                    "soul_color": "#5B6ABF",
                    "mood_keywords": mood_keywords,
                    "level": analysis.artist_analysis.artist_count.min(10).max(1) as i64,
                }),
            )
        }
        ContentAnalysis::Bangumi(analysis) => catalog_mock(
            &analysis.collection_type_distribution,
            analysis
                .top_rated_subjects
                .first()
                .map(|s| s.title.as_str()),
            analysis.watching_subjects.first().map(|s| s.title.as_str()),
            false,
            locale,
        ),
        ContentAnalysis::Mal(analysis) => catalog_mock(
            &analysis.collection_type_distribution,
            analysis
                .top_rated_subjects
                .first()
                .map(|s| s.title.as_str()),
            analysis.watching_subjects.first().map(|s| s.title.as_str()),
            true,
            locale,
        ),
        ContentAnalysis::X(analysis) => {
            let follow = analysis.following_sample.first();
            let post = analysis
                .top_posts
                .first()
                .or(analysis.recent_posts.first())
                .map(|p| p.text.chars().take(24).collect::<String>());
            (
                t(locale, "x.followsHonest"),
                take_insights([
                    Some(format!(
                        "{} {} {}",
                        t(locale, "x.posts"),
                        analysis.engagement_stats.total_posts,
                        t(locale, "x.postUnit")
                    )),
                    follow.map(|item| format!("{} @{}", t(locale, "x.follows"), item.username)),
                    post.map(|text| format!("{}「{}」", t(locale, "x.recentPost"), text)),
                ]),
                json!({
                    "vibe": if analysis.engagement_stats.total_posts == 0 {
                        t(locale, "x.watching")
                    } else {
                        t(locale, "x.hasPosts")
                    },
                    "engagement_level": if analysis.engagement_stats.total_posts == 0 {
                        t(locale, "x.observer")
                    } else {
                        t(locale, "x.pulse")
                    },
                    "signature_topics": follow
                        .map(|item| vec![item.name.chars().take(6).collect::<String>()])
                        .unwrap_or_default(),
                    "interest_circles": [],
                    "following_highlights": follow
                        .map(|item| vec![json!({
                            "username": item.username,
                            "name": item.name,
                            "tag": t(locale, "x.followTag")
                        })])
                        .unwrap_or_default(),
                }),
            )
        }
        ContentAnalysis::Discord(analysis) => {
            let owned = analysis.guild_stats.owned_guild_count;
            let admin = analysis.guild_stats.admin_guild_count;
            let guild = analysis.guilds_preview.first();
            let bind = analysis
                .connections
                .iter()
                .find(|c| c.visibility != 0)
                .map(|c| c.name.as_str());
            let role = if owned + admin == 0 {
                t(locale, "discord.lurker")
            } else if owned > 0 {
                t(locale, "discord.host")
            } else {
                t(locale, "discord.regular")
            };
            (
                t(locale, "discord.identityOwned"),
                take_insights([
                    Some(format!(
                        "{} {} · {} {}",
                        t(locale, "discord.owned"),
                        owned,
                        t(locale, "discord.admin"),
                        admin
                    )),
                    guild.map(|g| format!("{} {}", t(locale, "discord.listed"), g.name)),
                    bind.map(|name| format!("{} {}", t(locale, "discord.linked"), name)),
                ]),
                json!({
                    "vibe": role,
                    "role_profile": role,
                    "community_tags": analysis.profile.badges.iter().take(3).cloned().collect::<Vec<_>>(),
                    "guild_takes": analysis.guilds_preview.iter().take(8).map(|g| json!({
                        "name": g.name,
                        "id": g.id,
                        "take": if g.owner {
                            t(locale, "discord.ownServer")
                        } else {
                            t(locale, "discord.joined")
                        },
                    })).collect::<Vec<_>>(),
                }),
            )
        }
        ContentAnalysis::Xbox(analysis) => {
            let title = analysis
                .recent_titles
                .first()
                .or(analysis.top_completed_titles.first())
                .map(|t| t.name.as_str());
            let hunter = analysis.completed_games >= 5;
            (
                t(locale, "xbox.greens"),
                take_insights([
                    Some(format!(
                        "{} {} / {} {:.0}%",
                        t(locale, "xbox.complete"),
                        analysis.completed_games,
                        t(locale, "xbox.avg"),
                        analysis.average_completion
                    )),
                    Some(format!("GS {}", analysis.gamerscore)),
                    title.map(|name| format!("{}《{}》", t(locale, "xbox.recent"), name)),
                ]),
                json!({
                    "gamer_type": if hunter {
                        t(locale, "xbox.hunter")
                    } else {
                        t(locale, "xbox.wideNet")
                    },
                }),
            )
        }
        ContentAnalysis::Psn(analysis) => {
            let title = analysis
                .recent_titles
                .first()
                .or(analysis.top_completed_titles.first())
                .map(|t| t.name.as_str());
            (
                t(locale, "psn.cabinet"),
                take_insights([
                    Some(format!(
                        "{} {}",
                        t(locale, "psn.platinum"),
                        analysis.platinum_count
                    )),
                    Some(format!(
                        "{} {}%",
                        t(locale, "psn.avgProgress"),
                        analysis.average_progress.round()
                    )),
                    title.map(|name| format!("{}《{}》", t(locale, "xbox.recent"), name)),
                ]),
                json!({
                    "hunter_type": if analysis.platinum_count >= 10 {
                        t(locale, "psn.collector")
                    } else {
                        t(locale, "psn.casual")
                    },
                }),
            )
        }
    })
}

fn catalog_mock(
    dist: &std::collections::HashMap<String, usize>,
    top: Option<&str>,
    watching: Option<&str>,
    mal: bool,
    locale: &str,
) -> (String, Vec<String>, Value) {
    let wish = dist.get("wish").copied().unwrap_or(0);
    let done = dist.get("done").copied().unwrap_or(0);
    let taste = if wish > done {
        t(locale, "catalog.wishlistOutruns")
    } else if mal {
        t(locale, "catalog.leansFinished")
    } else {
        t(locale, "catalog.doneOutruns")
    };
    (
        taste.clone(),
        take_insights([
            Some(format!("wish {} · done {}", wish, done)),
            top.map(|title| format!("{}《{}》", t(locale, "catalog.highScore"), title)),
            watching.map(|title| format!("{}《{}》", t(locale, "catalog.watching"), title)),
        ]),
        json!({ "taste_profile": taste }),
    )
}

fn youtube_upload_stale(published_at: Option<&str>) -> bool {
    let Some(raw) = published_at else {
        return false;
    };
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).ok().or_else(|| {
        chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%SZ")
            .ok()
            .map(|n| n.and_utc().fixed_offset())
    });
    let Some(dt) = parsed else {
        return false;
    };
    chrono::Utc::now()
        .signed_duration_since(dt.with_timezone(&chrono::Utc))
        .num_days()
        > 365
}

fn take_insights(items: [Option<String>; 3]) -> Vec<String> {
    items.into_iter().flatten().collect()
}

fn category_label(
    locale: &str,
    category: &crate::services::content_databases::anime_database::ContentCategory,
) -> String {
    use crate::services::content_databases::anime_database::ContentCategory;
    match category {
        ContentCategory::Anime => t(locale, "bili.catAnime"),
        ContentCategory::TvSeries => t(locale, "bili.catTv"),
        ContentCategory::Movie => t(locale, "bili.catMovie"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banned_fragments() -> &'static [&'static str] {
        &[
            "观看偏好：",
            "收藏概况：",
            "仓库概况：",
            "下次一定",
            "深夜",
            "细腻的 ACG",
            "涉猎广泛",
        ]
    }

    #[test]
    fn mock_bilibili_has_no_label_insights() {
        let data: SmartFilteredData = serde_json::from_value(json!({
            "platform": "bilibili",
            "user_summary": {
                "username": "测试用户",
                "user_id": "1",
                "level": null,
                "stats": { "follower_count": null, "following_count": null, "total_content": 1 }
            },
            "content_analysis": {
                "video_summary": "machine",
                "anime_analysis": [{
                    "category": "Anime",
                    "count": 2,
                    "percentage": 80.0,
                    "genres": { "科幻": 2 },
                    "examples": ["命运石之门"],
                    "summary": "machine"
                }],
                "recent_videos": [{ "title": "杂谈一期" }]
            },
            "raw_unknown_content": []
        }))
        .unwrap();
        let (summary, insights, visuals) =
            generate_mock_report(&data, "bilibili", "zh-CN").unwrap();
        assert!(!summary.contains("测试用户"));
        let blob = format!("{summary}{}", insights.join(""));
        for frag in banned_fragments() {
            assert!(!blob.contains(frag), "banned {frag} in {blob}");
        }
        for word in ["内容丰富", "涉猎广泛", "细腻的收藏家", "硬核大佬"] {
            assert!(!blob.contains(word), "AVOID {word} in {blob}");
        }
        assert!(visuals["danmaku"].as_array().is_some());
        assert!(insights.iter().any(|line| line.contains("命运石之门")));
        assert!(summary.contains("不在同一条线上"));
    }

    #[test]
    fn mock_netease_mood_comes_from_real_tags() {
        let data: SmartFilteredData = serde_json::from_value(json!({
            "platform": "netease",
            "user_summary": {
                "username": "听者",
                "user_id": "1",
                "level": null,
                "stats": { "follower_count": null, "following_count": null, "total_content": 0 }
            },
            "content_analysis": {
                "music_summary": "machine",
                "artist_analysis": {
                    "total_songs": 0,
                    "artist_count": 0,
                    "genre_analysis": [],
                    "region_distribution": {},
                    "summary": "machine",
                    "favorite_artists": [],
                    "unknown_songs": []
                },
                "recent_songs": []
            },
            "raw_unknown_content": []
        }))
        .unwrap();
        let (_, _, visuals) = generate_mock_report(&data, "netease", "zh-CN").unwrap();
        assert!(visuals["mood_keywords"].as_array().unwrap().is_empty());
        let blob = visuals.to_string();
        assert!(!blob.contains("冷感"));
        assert!(!blob.contains("疏离"));
    }

    #[test]
    fn mock_x_does_not_use_colon_labels() {
        let data: SmartFilteredData = serde_json::from_value(json!({
            "platform": "x",
            "user_summary": {
                "username": "demo",
                "user_id": "1",
                "level": null,
                "stats": { "follower_count": 1, "following_count": 1, "total_content": 1 }
            },
            "content_analysis": {
                "post_summary": "machine",
                "user_name": "Demo",
                "engagement_stats": {
                    "total_posts": 1,
                    "total_likes_received": 0,
                    "total_retweets_received": 0,
                    "total_replies_received": 0,
                    "total_impressions": 0,
                    "liked_posts_count": 0
                },
                "following_sample": [{
                    "username": "acc",
                    "name": "Account",
                    "description": "circle",
                    "verified": false
                }],
                "recent_posts": [{
                    "id": "1",
                    "text": "hello world",
                    "like_count": 0,
                    "retweet_count": 0,
                    "reply_count": 0,
                    "impression_count": 0
                }],
                "top_posts": [],
                "language_distribution": {}
            },
            "raw_unknown_content": []
        }))
        .unwrap();
        let (summary, insights, _) = generate_mock_report(&data, "x", "zh-CN").unwrap();
        let blob = format!("{summary}{}", insights.join(""));
        assert!(!blob.contains("近帖："));
        assert!(insights.iter().any(|line| line.contains("近帖写")));
    }

    #[test]
    fn mock_follows_request_locale() {
        let data: SmartFilteredData = serde_json::from_value(json!({
            "platform": "steam",
            "user_summary": {
                "username": "p",
                "user_id": "1",
                "level": null,
                "stats": { "follower_count": null, "following_count": null, "total_content": 1 }
            },
            "content_analysis": {
                "game_summary": "machine",
                "genre_analysis": [],
                "recent_games": [{ "name": "Hades", "playtime": 10 }],
                "games_count": 3,
                "total_playtime_minutes": 20
            },
            "raw_unknown_content": []
        }))
        .unwrap();
        let (en, _, _) = generate_mock_report(&data, "steam", "en-US").unwrap();
        let (ja, _, _) = generate_mock_report(&data, "steam", "ja-JP").unwrap();
        assert!(en.contains("Library") || en.contains("Hours") || en.contains("lifetime"));
        assert!(ja.contains("ライブラリ") || ja.contains("時間") || ja.contains("名簿"));
        assert!(!en.contains("时长"));
    }

    #[test]
    fn mock_youtube_channel_type_stays_on_allow_list() {
        let data: SmartFilteredData = serde_json::from_value(json!({
            "platform": "youtube",
            "user_summary": {
                "username": "ch",
                "user_id": "1",
                "level": null,
                "stats": { "follower_count": 1, "following_count": null, "total_content": 1 }
            },
            "content_analysis": {
                "video_summary": "machine",
                "subscriber_count": 10,
                "view_count": 100,
                "video_count": 1,
                "recent_videos": [{
                    "title": "杂谈",
                    "video_id": "x",
                    "published_at": "2026-08-01T00:00:00Z"
                }]
            },
            "raw_unknown_content": []
        }))
        .unwrap();
        let (_, _, visuals) = generate_mock_report(&data, "youtube", "zh-CN").unwrap();
        assert_eq!(visuals["channel_type"], "有片");
        assert_ne!(visuals["channel_type"], "稳定更新");
        assert_ne!(visuals["channel_type"], "有片可闻");
        assert_eq!(visuals["vibe"], "有片可闻");
    }
}
