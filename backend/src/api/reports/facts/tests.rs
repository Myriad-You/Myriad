//! Card fields stay exactly what the old per-platform overrides wrote: the
//! golden file was produced by the pre-fact-layer code over these fixtures
//! (keys read from `cache/raw` files left out, they differ by machine).

use serde_json::{Value, json};

use super::{build, card_visuals};
use crate::services::smart_filter::{ContentAnalysis, SmartFilteredData};

fn user(username: &str, followers: Option<i64>, following: Option<i64>) -> Value {
    json!({
        "username": username,
        "user_id": "u-1",
        "level": null,
        "stats": { "follower_count": followers, "following_count": following, "total_content": 3 }
    })
}

fn data(platform: &str, content: Value, followers: Option<i64>) -> SmartFilteredData {
    serde_json::from_value(json!({
        "platform": platform,
        "user_summary": user("tester", followers, Some(12)),
        "content_analysis": content,
        "raw_unknown_content": [],
    }))
    .unwrap_or_else(|e| panic!("{platform} fixture: {e}"))
}

fn catalog_content() -> Value {
    json!({
        "collection_summary": "machine",
        "subject_type_distribution": { "anime": 30, "book": 4 },
        "collection_type_distribution": { "done": 20, "wish": 9, "doing": 2 },
        "tag_distribution": { "科幻": 6, "日常": 3 },
        "top_rated_subjects": [
            { "subject_id": 1, "title": "Steins;Gate", "subject_type": "anime", "collection_type": "done", "rate": 10, "cover": "https://lain.bgm.tv/pic/cover/l/a.jpg", "updated_at": null },
            { "subject_id": 2, "title": "Mushishi", "subject_type": "anime", "collection_type": "done", "rate": 9, "cover": null, "updated_at": null }
        ],
        "watching_subjects": [
            { "subject_id": 3, "title": "Frieren", "subject_type": "anime", "collection_type": "doing", "rate": 0, "cover": "https://lain.bgm.tv/pic/cover/l/b.jpg", "updated_at": "2026-10-01" }
        ],
        "recent_updates": []
    })
}

/// Every platform, each with data that reaches its card branch.
pub(super) fn fixtures() -> Vec<(&'static str, SmartFilteredData)> {
    let mut out = vec![
        (
            "steam",
            data(
                "steam",
                json!({
                    "game_summary": "machine",
                    "genre_analysis": [
                        { "genre": "RPG", "count": 4, "percentage": 40.0, "examples": ["ELDEN RING", "Persona 5 Royal"], "total_playtime": 9000 },
                        { "genre": "Roguelike", "count": 2, "percentage": 20.0, "examples": ["Hades"], "total_playtime": 1200 }
                    ],
                    "recent_games": [
                        { "name": "ELDEN RING", "playtime": 7260, "appid": 1245620, "image": "https://cdn.cloudflare.steamstatic.com/steam/apps/1245620/header.jpg" },
                        { "name": "Hades", "playtime": 1200 }
                    ],
                    "games_count": 0,
                    "total_playtime_minutes": 10261
                }),
                None,
            ),
        ),
        (
            "github",
            data(
                "github",
                json!({
                    "repo_summary": "machine",
                    "language_distribution": { "Rust": 6, "TypeScript": 3, "Python": 1 },
                    "recent_repos": [
                        { "name": "myriad", "language": "Rust", "stars": 240, "forks": 12, "description": "site" },
                        { "name": "dotfiles", "language": null, "stars": null, "forks": null, "description": null }
                    ],
                    "contribution_calendar": [
                        { "date": "2026-10-01", "count": 5 },
                        { "date": "2026-10-02", "count": 0 },
                        { "date": "2026-10-03", "count": -1 },
                        { "date": "2026-10-04", "count": 9 }
                    ],
                    "public_repos": 1
                }),
                None,
            ),
        ),
        (
            "youtube",
            data(
                "youtube",
                json!({
                    "video_summary": "machine",
                    "subscriber_count": 1200,
                    "view_count": 98000,
                    "video_count": 2,
                    "recent_videos": [
                        { "title": "Rig test", "video_id": "a1", "cover": "https://i.ytimg.com/vi/a1/hq.jpg", "view_count": 300, "published_at": "2026-10-10T00:00:00Z", "duration": "PT4M13S", "url": "https://youtu.be/a1" },
                        { "title": "Old vlog", "video_id": "b2", "view_count": 9000, "like_count": 40 }
                    ],
                    "avatar": "https://yt3.ggpht.com/x.jpg",
                    "channel_url": "https://youtube.com/@t",
                    "custom_url": "@t"
                }),
                None,
            ),
        ),
        (
            "bilibili",
            data(
                "bilibili",
                json!({
                    "video_summary": "machine",
                    "anime_analysis": [{
                        "category": "Anime", "count": 3, "percentage": 75.0,
                        "genres": { "科幻": 2 }, "examples": ["命运石之门", "虫师"], "summary": "machine"
                    }],
                    "recent_videos": [{ "title": "杂谈一期", "cover": "https://i0.hdslb.com/bfs/archive/c.jpg", "bvid": "BV1" }]
                }),
                None,
            ),
        ),
        (
            "netease",
            data(
                "netease",
                json!({
                    "music_summary": "machine",
                    "artist_analysis": {
                        "total_songs": 120, "artist_count": 40,
                        "genre_analysis": [{ "genre": "J-Pop", "count": 50, "percentage": 41.0 }, { "genre": "Post-rock", "count": 20, "percentage": 16.0 }],
                        "region_distribution": { "日本": 70, "华语": 30 },
                        "summary": "machine",
                        "favorite_artists": ["ヨルシカ", "坂本龍一"],
                        "unknown_songs": []
                    },
                    "recent_songs": [{ "title": "だから僕は音楽を辞めた", "artist": "ヨルシカ", "cover": "https://p1.music.126.net/a.jpg" }]
                }),
                None,
            ),
        ),
        ("bangumi", data("bangumi", catalog_content(), None)),
        (
            "x",
            data(
                "x",
                json!({
                    "post_summary": "machine",
                    "user_name": "Tester",
                    "user_avatar": "https://pbs.twimg.com/profile_images/1/a_normal.jpg",
                    "engagement_stats": { "total_posts": 2, "total_likes_received": 30, "total_retweets_received": 2, "total_replies_received": 1, "total_impressions": 0, "liked_posts_count": 0 },
                    "following_summary": "machine",
                    "following_sample": [
                        { "username": "rustlang", "name": "Rust", "description": "lang", "follower_count": 9, "verified": true, "profile_image_url": "https://pbs.twimg.com/profile_images/2/b_normal.png" }
                    ],
                    "recent_posts": [],
                    "top_posts": [
                        { "id": "1", "text": "shipping the report rewrite today", "created_at": null, "like_count": 25, "retweet_count": 1, "reply_count": 0, "impression_count": 0 },
                        { "id": "2", "text": "coffee", "created_at": null, "like_count": 5, "retweet_count": 1, "reply_count": 1, "impression_count": 0 }
                    ],
                    "language_distribution": { "en": 2 }
                }),
                Some(321),
            ),
        ),
        (
            "discord",
            data(
                "discord",
                json!({
                    "community_summary": "machine",
                    "profile": { "display_name": "T", "username": "t", "avatar_url": "https://cdn.discordapp.com/avatars/1/a.png", "badges": ["hypesquad"], "mfa_enabled": true, "account_age_years": 6 },
                    "guild_stats": { "guild_count": 0, "owned_guild_count": 1, "admin_guild_count": 1, "manage_guild_count": 0, "total_member_reach": 0, "total_online_reach": 40, "community_guild_count": 1, "partnered_or_verified_count": 0 },
                    "guilds_preview": [
                        { "id": "g1", "name": "Rust 中文", "icon_url": "https://cdn.discordapp.com/icons/g1/a.png", "owner": false, "permissions_highlight": [], "member_count": 5000, "feature_highlight": ["COMMUNITY"] },
                        { "id": "g2", "name": "自家小屋", "icon_url": null, "owner": true, "permissions_highlight": ["ADMINISTRATOR"], "feature_highlight": [] }
                    ],
                    "connections": [
                        { "type": "github", "name": "t", "id": "c1", "verified": true, "visibility": 1 },
                        { "type": "steam", "name": "t", "id": "c2", "verified": true, "visibility": 0 }
                    ],
                    "identity_graph": { "linked_platforms": ["github"], "cross_check": {} }
                }),
                None,
            ),
        ),
        (
            "xbox",
            data(
                "xbox",
                json!({
                    "gaming_summary": "machine",
                    "gamerscore": 12000, "games_count": 30, "achievement_games": 20, "completed_games": 2,
                    "total_achievements_earned": 400, "total_achievements_available": 900,
                    "average_completion": 44.6, "hardcore_score": 61,
                    "avatar": "http://images-eds.xboxlive.com/a.png", "display_gamertag": "Tester GT",
                    "recent_titles": [
                        { "title_id": "t2", "name": "Halo Infinite", "display_image": "http://store-images.s-microsoft.com/h.png", "achievements_earned": 10, "achievements_total": 100, "gamerscore_earned": 150, "gamerscore_total": 1000, "progress": 15.4, "last_played": "2026-10-01" },
                        { "title_id": "t3", "name": "Forza", "display_image": null, "achievements_earned": 1, "achievements_total": 50, "gamerscore_earned": 10, "gamerscore_total": 1000, "progress": 2.0, "last_played": null }
                    ],
                    "top_completed_titles": [
                        { "title_id": "t1", "name": "Hollow Knight", "display_image": "http://store-images.s-microsoft.com/k.png", "achievements_earned": 63, "achievements_total": 63, "gamerscore_earned": 1000, "gamerscore_total": 1000, "progress": 100.0, "last_played": null }
                    ]
                }),
                None,
            ),
        ),
        (
            "psn",
            data(
                "psn",
                json!({
                    "trophy_summary_text": "machine",
                    "trophy_level": 320, "platinum_count": 3, "gold_count": 20, "silver_count": 60, "bronze_count": 300,
                    "total_trophies": 383, "games_count": 40, "completed_games": 3, "average_progress": 37.5, "hardcore_score": 55,
                    "display_online_id": "tester_psn", "is_plus": true,
                    "recent_titles": [
                        { "name": "Astro Bot", "platform": "PS5", "icon_url": "https://image.api.playstation.com/a.png", "progress": 40, "earned_platinum": 0, "earned_gold": 1, "earned_silver": 2, "earned_bronze": 10, "last_updated": null }
                    ],
                    "top_completed_titles": [
                        { "name": "Bloodborne", "platform": "PS4", "icon_url": "http://image.api.playstation.com/b.png", "progress": 100, "earned_platinum": 1, "earned_gold": 5, "earned_silver": 10, "earned_bronze": 20, "last_updated": null }
                    ]
                }),
                None,
            ),
        ),
    ];
    // MAL JSON reads back as Bangumi through the untagged enum; build it directly.
    let mut mal_content = catalog_content();
    mal_content["mean_score"] = json!(7.9);
    mal_content["days_watched"] = json!(41.5);
    let mut mal = data(
        "mal",
        json!({ "video_summary": "x", "anime_analysis": [], "recent_videos": [] }),
        None,
    );
    mal.content_analysis =
        ContentAnalysis::Mal(serde_json::from_value(mal_content).expect("mal fixture"));
    out.push(("mal", mal));
    out
}

/// What a model writes into `card_visuals`, including values the system
/// clamps (oversized lists, out-of-range scores, empty enums).
pub(super) fn model_visuals(platform: &str) -> Value {
    match platform {
        "steam" => {
            json!({ "player_type": "硬核玩家", "hardcore_score": 130.6, "games_count": 9999 })
        }
        "netease" => json!({
            "soul_color": "#334455", "level": 7,
            "mood_keywords": (1..=8).map(|i| json!({ "tag": format!("t{i}"), "color": "#000000" })).collect::<Vec<_>>()
        }),
        "x" => json!({
            "engagement_level": "immersed observer",
            "interest_circles": (0..6).map(|i| json!({ "name": format!("c{i}"), "count": 1, "accounts": ["a", "b", "c", "d"] })).collect::<Vec<_>>(),
            "following_highlights": (0..7).map(|i| json!({ "username": format!("u{i}") })).collect::<Vec<_>>(),
            "stats": { "followers": 1 }
        }),
        "discord" => json!({
            "role_profile": "circle regular",
            "community_tags": ["a", "b", "c", "d"],
            "guild_takes": [{ "name": "Rust 中文", "take": "a take that runs well past twenty characters" }, { "name": "invented", "take": "x" }]
        }),
        "xbox" => json!({ "gamer_type": "", "gamerscore": 1 }),
        "psn" => json!({ "platinum_count": 99 }),
        "github" => json!({ "contribution_level": "legendary", "languages": [] }),
        "youtube" => {
            json!({ "vibe": "quiet", "channel_type": "has videos", "subscriber_count": 1 })
        }
        "bangumi" | "mal" => json!({ "taste_profile": "slow burn", "status_counts": {} }),
        "bilibili" => json!({ "danmaku": ["a", "b"], "follower_count": 5 }),
        _ => json!({}),
    }
}

/// Keys read from `cache/raw` files at generation time; they differ by machine.
fn raw_file_keys(platform: &str) -> &'static [&'static str] {
    match platform {
        "steam" => &["library_items"],
        "bilibili" => &[
            "library_items",
            "user_level",
            "follower_count",
            "following_count",
        ],
        "netease" => &["library_items", "follower_count", "playlist_count"],
        _ => &[],
    }
}

fn without_raw_keys(platform: &str, mut value: Value) -> Value {
    if let Some(obj) = value.as_object_mut() {
        for key in raw_file_keys(platform) {
            obj.remove(*key);
        }
    }
    value
}

#[tokio::test]
async fn card_visuals_match_the_pre_fact_layer_overrides() {
    let golden: Value =
        serde_json::from_str(include_str!("card_golden.json")).expect("golden json");
    for (platform, metadata) in fixtures() {
        let facts = build(platform, &metadata).await;
        let visuals = card_visuals(
            platform,
            model_visuals(platform),
            &facts,
            &metadata,
            "zh-CN",
        );
        assert_eq!(
            without_raw_keys(platform, visuals),
            without_raw_keys(platform, golden[platform].clone()),
            "{platform} card_visuals drifted from the old overrides"
        );
    }
}

#[tokio::test]
async fn facts_are_unique_and_agree_with_the_card() {
    for (platform, metadata) in fixtures() {
        let facts = build(platform, &metadata).await;
        assert!(!facts.facts.is_empty(), "{platform} has no facts");
        let mut ids = std::collections::HashSet::new();
        for fact in &facts.facts {
            assert!(
                ids.insert(fact.id.as_str()),
                "{platform} repeats {}",
                fact.id
            );
            assert!(!fact.label.is_empty());
            assert!(!fact.value.is_null(), "{platform}.{} is null", fact.id);
            // A fact named like a scalar card field shows the same value.
            if let Some(card) = facts.card.get(&fact.id) {
                if !card.is_object() && !card.is_array() {
                    assert_eq!(card, &fact.value, "{platform}.{} disagrees", fact.id);
                }
            }
        }
    }
}

#[tokio::test]
async fn the_system_owns_counts_the_model_wrote() {
    let fixtures = fixtures();
    let pick = |name: &str| {
        fixtures
            .iter()
            .find(|(p, _)| *p == name)
            .map(|(_, d)| d.clone())
            .unwrap()
    };
    let steam = pick("steam");
    let facts = build("steam", &steam).await;
    let visuals = card_visuals("steam", model_visuals("steam"), &facts, &steam, "zh-CN");
    // games_count 0 in the cache: the larger of recent games and genre examples.
    assert_eq!(visuals["games_count"], 3);
    assert_eq!(visuals["total_playtime"], 171);
    assert_eq!(visuals["hardcore_score"], 100);
    assert_eq!(visuals["player_type"], "hardcore");

    let xbox = pick("xbox");
    let facts = build("xbox", &xbox).await;
    let visuals = card_visuals("xbox", model_visuals("xbox"), &facts, &xbox, "en-US");
    assert_eq!(visuals["gamerscore"], 12000);
    assert!(!visuals["gamer_type"].as_str().unwrap_or("").is_empty());
    let top = facts.facts.iter().find(|f| f.id == "top_title").unwrap();
    assert_eq!(top.value, "Hollow Knight");
    assert!(top.media.as_deref().unwrap().starts_with("https://"));
}

#[test]
fn fact_media_goes_through_the_image_proxy() {
    let facts = vec![super::Fact {
        id: "top_anime".into(),
        label: "A series".into(),
        value: json!("虫师"),
        media: Some("https://i0.hdslb.com/bfs/archive/c.jpg".into()),
    }];
    let out = super::proxied_facts(&facts);
    assert!(
        out[0]
            .media
            .as_deref()
            .unwrap()
            .starts_with("/api/proxy/image?url=")
    );
}
