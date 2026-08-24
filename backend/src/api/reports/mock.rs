//! Local fallback when the report model is missing or returns unusable JSON.
//! Same field contract as the live path; no colon-label insights, no banned hooks.

use serde_json::{json, Value};

use super::locale::pick;
use crate::services::smart_filter::{ContentAnalysis, SmartFilteredData};

fn t<'a>(locale: &str, zh: &'a str, ja: &'a str, en: &'a str) -> &'a str {
    pick(locale, zh, ja, en)
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
                .map(|item| format!("{} {}", item.count, category_label(&item.category)))
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
                    (Some(_), Some(_)) => t(
                        locale,
                        "追番和投稿不在同一条线上",
                        "追番と投稿は別線",
                        "Watching and uploads diverge",
                    ),
                    (Some(_), None) => {
                        t(locale, "名单偏追番", "リストは追番寄り", "List leans anime")
                    }
                    (None, Some(_)) => t(
                        locale,
                        "最近在看投稿",
                        "最近は投稿を見ている",
                        "Watching uploads lately",
                    ),
                    (None, None) => t(
                        locale,
                        "公开区几乎是空的",
                        "公開区はほぼ空",
                        "Public shelf is almost empty",
                    ),
                }
                .into(),
                take_insights([
                    anime.map(|name| {
                        format!(
                            "{}《{}》",
                            t(locale, "名单上还挂着", "リストに残る", "Still listed"),
                            name
                        )
                    }),
                    video.map(|title| {
                        format!(
                            "{}《{}》",
                            t(locale, "最近在看", "最近見ている", "Watching"),
                            title
                        )
                    }),
                    (!mix.is_empty())
                        .then(|| format!("{} {}", t(locale, "类型占比", "ジャンル比", "Mix"), mix)),
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
                    t(
                        locale,
                        "时长堆在少数作品上",
                        "時間は少数作に偏る",
                        "Hours pile on a few titles",
                    )
                } else {
                    t(
                        locale,
                        "库比最近在玩的名单大",
                        "ライブラリは最近の名簿より大きい",
                        "Library outgrows the recent list",
                    )
                }
                .into(),
                take_insights([
                    top.map(|g| {
                        format!(
                            "《{}》{}",
                            g.name,
                            t(
                                locale,
                                "吃掉最多终身分钟",
                                "が生涯分を最も食う",
                                " takes the most lifetime minutes"
                            )
                        )
                    }),
                    genre.map(|g| {
                        format!(
                            "{}{}",
                            t(locale, "类型气味偏", "ジャンルは", "Genre leans "),
                            g
                        )
                    }),
                    Some(format!(
                        "{} {} · {} {}",
                        t(locale, "库", "庫", "Library"),
                        analysis.games_count,
                        t(locale, "最近名单", "最近の名簿", "recent"),
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
                t(
                    locale,
                    "仓库影响力不等于日历密度",
                    "星とカレンダー密度は別物",
                    "Stars are not calendar density",
                )
                .into(),
                take_insights([
                    top.and_then(|repo| {
                        repo.stars.map(|n| {
                            format!(
                                "{} {} {} star",
                                repo.name,
                                t(locale, "最高", "最大", "top"),
                                n
                            )
                        })
                    }),
                    (cal_days > 0).then(|| {
                        format!(
                            "{} {} {}",
                            t(locale, "日历有", "カレンダー", "Calendar has"),
                            cal_days,
                            t(locale, "天有提交", "日コミットあり", "active days")
                        )
                    }),
                    lang.map(|name| {
                        format!(
                            "{} {}",
                            t(locale, "语言栈头是", "言語の頭は", "Top language"),
                            name
                        )
                    }),
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
                    t(
                        locale,
                        "公开区还没有能闻的片",
                        "公開区に嗅げる動画がない",
                        "No public videos to read",
                    )
                    .into(),
                    vec![t(
                        locale,
                        "video_count 为 0，频道资料在、片子不在",
                        "video_count は 0、資料だけある",
                        "video_count is 0; channel exists, videos do not",
                    )
                    .into()],
                    json!({
                        "vibe": t(locale, "冷启动空壳", "コールドスタート", "Cold start shell"),
                        "channel_type": t(locale, "冷启动号", "コールドスタート", "Cold start"),
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
                    t(
                        locale,
                        "订阅和均播要分开看",
                        "登録と平均再生は別物",
                        "Subs and average views split",
                    )
                    .into(),
                    take_insights([
                        Some(format!(
                            "{} {} · {} {}",
                            t(locale, "均播约", "平均再生", "Avg views"),
                            avg,
                            t(locale, "订阅", "登録", "subs"),
                            analysis.subscriber_count
                        )),
                        latest.map(|title| {
                            format!(
                                "{}《{}》",
                                t(locale, "最近标题", "最近のタイトル", "Latest"),
                                title
                            )
                        }),
                        analysis
                            .recent_videos
                            .first()
                            .and_then(|v| v.published_at.as_deref())
                            .map(|at| {
                                format!(
                                    "{} {}",
                                    t(locale, "最近上传", "最近の投稿", "Uploaded"),
                                    at
                                )
                            }),
                    ]),
                    json!({
                        "vibe": t(locale, "有片可闻", "動画あり", "Has videos"),
                        "channel_type": if stale {
                            t(locale, "停更沉寂", "更新停止", "Dormant")
                        } else {
                            t(locale, "有片", "動画あり", "Has videos")
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
            const MOOD_COLORS: [&str; 3] = ["#5B6ABF", "#3D4A7A", "#9AA4C2"];
            let mood_keywords: Vec<Value> = [genre, region, artist]
                .into_iter()
                .flatten()
                .take(3)
                .enumerate()
                .map(|(i, tag)| {
                    let color = MOOD_COLORS[i];
                    json!({ "tag": tag, "color": color })
                })
                .collect();
            (
                t(
                    locale,
                    "曲风比数量诚实",
                    "曲調は本数より正直",
                    "Genre is more honest than count",
                )
                .into(),
                take_insights([
                    artist.map(|name| {
                        format!(
                            "{} {}",
                            t(locale, "名单上反复出现", "名簿に繰り返す", "Keeps showing"),
                            name
                        )
                    }),
                    region.map(|name| {
                        format!("{}{}", t(locale, "地域偏", "地域は", "Region leans "), name)
                    }),
                    genre.map(|name| {
                        format!(
                            "{}{}",
                            t(locale, "曲风偏", "ジャンルは", "Genre leans "),
                            name
                        )
                    }),
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
                t(
                    locale,
                    "关注名单比发帖诚实",
                    "フォローは投稿より正直",
                    "Follows are more honest than posts",
                )
                .into(),
                take_insights([
                    Some(format!(
                        "{} {} {}",
                        t(locale, "账上", "投稿", "Posts"),
                        analysis.engagement_stats.total_posts,
                        t(locale, "帖", "", "")
                    )),
                    follow.map(|item| {
                        format!(
                            "{} @{}",
                            t(locale, "关注里有", "フォローに", "Follows"),
                            item.username
                        )
                    }),
                    post.map(|text| {
                        format!(
                            "{}「{}」",
                            t(locale, "近帖写", "近投稿は", "Recent post"),
                            text
                        )
                    }),
                ]),
                json!({
                    "vibe": if analysis.engagement_stats.total_posts == 0 {
                        t(locale, "沉浸观察", "観察に没入", "Watching")
                    } else {
                        t(locale, "有帖可闻", "投稿あり", "Has posts")
                    },
                    "engagement_level": if analysis.engagement_stats.total_posts == 0 {
                        t(locale, "沉浸观察者", "没入観察者", "Observer")
                    } else {
                        t(locale, "脉冲发帖", "パルス投稿", "Pulse poster")
                    },
                    "signature_topics": follow
                        .map(|item| vec![item.name.chars().take(6).collect::<String>()])
                        .unwrap_or_default(),
                    "interest_circles": [],
                    "following_highlights": follow
                        .map(|item| vec![json!({
                            "username": item.username,
                            "name": item.name,
                            "tag": t(locale, "关注样本", "フォロー標本", "Follow")
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
                t(locale, "潜水观察者", "潜水観察者", "Lurker")
            } else if owned > 0 {
                t(locale, "社群主理人", "コミュニティ主", "Community host")
            } else {
                t(locale, "圈子老炮", "古参", "Circle regular")
            };
            (
                t(
                    locale,
                    "身份看自建数，不看服多",
                    "身分は自作数で見る",
                    "Identity is owned servers, not count",
                )
                .into(),
                take_insights([
                    Some(format!(
                        "{} {} · {} {}",
                        t(locale, "自建", "自作", "Owned"),
                        owned,
                        t(locale, "管理", "管理", "admin"),
                        admin
                    )),
                    guild.map(|g| {
                        format!("{} {}", t(locale, "名单上有", "名簿に", "Listed"), g.name)
                    }),
                    bind.map(|name| format!("{} {}", t(locale, "绑定", "連携", "Linked"), name)),
                ]),
                json!({
                    "vibe": role,
                    "role_profile": role,
                    "community_tags": analysis.profile.badges.iter().take(3).cloned().collect::<Vec<_>>(),
                    "guild_takes": analysis.guilds_preview.iter().take(8).map(|g| json!({
                        "name": g.name,
                        "id": g.id,
                        "take": if g.owner {
                            t(locale, "自己的服", "自分の鯖", "Own server")
                        } else {
                            t(locale, "加入的服", "参加した鯖", "Joined")
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
                t(
                    locale,
                    "认绿光不认时长",
                    "緑は見る、時間は見ない",
                    "Greens, not hours",
                )
                .into(),
                take_insights([
                    Some(format!(
                        "{} {} / {} {:.0}%",
                        t(locale, "全成就", "コンプ", "Complete"),
                        analysis.completed_games,
                        t(locale, "平均完成", "平均達成", "avg"),
                        analysis.average_completion
                    )),
                    Some(format!("GS {}", analysis.gamerscore)),
                    title
                        .map(|name| format!("{}《{}》", t(locale, "近作", "近作", "Recent"), name)),
                ]),
                json!({
                    "gamer_type": if hunter {
                        t(locale, "全成就猎人", "実績コンプ勢", "Completion hunter")
                    } else {
                        t(locale, "广撒网玩家", "広く浅く", "Wide net")
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
                t(
                    locale,
                    "认奖杯柜不认时长",
                    "トロフィー棚は見る、時間は見ない",
                    "Cabinet, not hours",
                )
                .into(),
                take_insights([
                    Some(format!(
                        "{} {}",
                        t(locale, "白金", "プラチナ", "Platinum"),
                        analysis.platinum_count
                    )),
                    Some(format!(
                        "{} {}%",
                        t(locale, "平均进度", "平均進捗", "Avg progress"),
                        analysis.average_progress.round()
                    )),
                    title
                        .map(|name| format!("{}《{}》", t(locale, "近作", "近作", "Recent"), name)),
                ]),
                json!({
                    "hunter_type": if analysis.platinum_count >= 10 {
                        t(locale, "白金收藏家", "プラチナ収集家", "Platinum collector")
                    } else {
                        t(locale, "随缘奖杯党", "気まま勢", "Casual trophies")
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
        t(
            locale,
            "想看比进度诚实",
            "見たいは進捗より正直",
            "Wishlist outruns done",
        )
    } else if mal {
        t(
            locale,
            "列表偏做完",
            "リストは完了寄り",
            "List leans finished",
        )
    } else {
        t(
            locale,
            "进度压过想看",
            "進捗が見たいを上回る",
            "Done outruns wishlist",
        )
    };
    (
        taste.into(),
        take_insights([
            Some(format!("wish {} · done {}", wish, done)),
            top.map(|title| {
                format!(
                    "{}《{}》",
                    t(locale, "高分有", "高得点に", "High score"),
                    title
                )
            }),
            watching
                .map(|title| format!("{}《{}》", t(locale, "在追", "視聴中", "Watching"), title)),
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
    category: &crate::services::content_databases::anime_database::ContentCategory,
) -> &'static str {
    use crate::services::content_databases::anime_database::ContentCategory;
    match category {
        ContentCategory::Anime => "番",
        ContentCategory::TvSeries => "剧",
        ContentCategory::Movie => "电影",
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
