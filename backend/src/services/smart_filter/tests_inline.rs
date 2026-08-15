//! Unit tests for smart_filter.

use serde_json::Value;
use std::fs;
use std::path::Path;

use super::helpers::*;
#[allow(unused_imports)]
use super::{cache_tokens, filter_impl, process};

#[test]
fn test_smart_filter_integration() {
    // Read from the new split raw data files
    let raw_dir = Path::new("cache/raw");
    if !raw_dir.exists() {
        println!(
            "Skipping test: cache/raw directory not found at {:?}",
            raw_dir
        );
        return;
    }

    let mut all_data = serde_json::Map::new();

    // Load all platform files
    if let Ok(entries) = fs::read_dir(raw_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(platform_name) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(json) = serde_json::from_str(&content) {
                            all_data.insert(platform_name.to_string(), json);
                        }
                    }
                }
            }
        }
    }

    if all_data.is_empty() {
        println!("Skipping test: No platform data files found in cache/raw");
        return;
    }

    let data = Value::Object(all_data);

    // 使用 process_and_save_all，它会分平台保存
    match SmartFilter::process_and_save_all(&data) {
        Ok(_) => println!("Successfully processed and saved all platform data"),
        Err(e) => println!("Failed to process platform data: {}", e),
    }

    // 验证分平台文件是否已创建
    let platforms_dir = Path::new("cache/platforms");
    if platforms_dir.exists() {
        println!("Platform filtered files:");
        if let Ok(entries) = fs::read_dir(platforms_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.ends_with("_filtered.json"))
                    .unwrap_or(false)
                {
                    println!("  - {:?}", path.file_name().unwrap());
                }
            }
        }
    }
}

#[test]
fn filter_x_builds_engagement_summary() {
    let raw = serde_json::json!({
        "user": {
            "id": "42",
            "username": "demo",
            "name": "Demo User",
            "public_metrics": {
                "followers_count": 100,
                "following_count": 10,
                "tweet_count": 50
            }
        },
        "tweets": [
            {
                "id": "1",
                "text": "hello",
                "lang": "en",
                "created_at": "2026-01-01T00:00:00Z",
                "public_metrics": {
                    "like_count": 5,
                    "retweet_count": 1,
                    "reply_count": 0,
                    "impression_count": 20
                }
            },
            {
                "id": "2",
                "text": "world",
                "lang": "en",
                "created_at": "2026-01-02T00:00:00Z",
                "public_metrics": {
                    "like_count": 50,
                    "retweet_count": 10,
                    "reply_count": 2,
                    "impression_count": 200
                }
            }
        ]
    });

    let filtered = SmartFilter::filter("x", &raw).expect("filter x");
    assert_eq!(filtered.platform, "x");
    assert_eq!(filtered.user_summary.username, "demo");
    assert_eq!(filtered.user_summary.stats.follower_count, Some(100));

    match filtered.content_analysis {
        ContentAnalysis::X(analysis) => {
            // Profile public_metrics.tweet_count (50), not scraped list length (2)
            assert_eq!(analysis.engagement_stats.total_posts, 50);
            assert_eq!(analysis.engagement_stats.total_likes_received, 55);
            assert_eq!(analysis.top_posts[0].id, "2");
            assert!(analysis.post_summary.contains("@demo"));
        }
        other => panic!("expected X analysis, got {:?}", other),
    }
}

/// Fixture shaped like Data API v3 `channels` + `videos` (public, no OAuth).
#[test]
fn filter_youtube_channel_and_videos() {
    let raw = serde_json::json!({
        "channel": {
            "id": "UC_x5XG1OV2P6uZZ5FSM9Ttw",
            "snippet": {
                "title": "Google for Developers",
                "customUrl": "@GoogleDevelopers",
                "description": "Subscribe to join a community of creative developers!",
                "thumbnails": {
                    "high": { "url": "https://example.com/avatar.jpg" }
                }
            },
            "statistics": {
                "viewCount": "250000000",
                "subscriberCount": "2300000",
                "videoCount": "5800"
            },
            "contentDetails": {
                "relatedPlaylists": { "uploads": "UU_x5XG1OV2P6uZZ5FSM9Ttw" }
            }
        },
        "uploads_playlist_id": "UU_x5XG1OV2P6uZZ5FSM9Ttw",
        "playlist_items": [],
        "videos": [
            {
                "id": "dQw4w9WgXcQ",
                "snippet": {
                    "title": "Sample Upload One",
                    "publishedAt": "2026-01-01T12:00:00Z",
                    "thumbnails": {
                        "medium": { "url": "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg" }
                    }
                },
                "statistics": {
                    "viewCount": "1000",
                    "likeCount": "50",
                    "commentCount": "5"
                },
                "contentDetails": { "duration": "PT3M33S" }
            },
            {
                "id": "abc123xyz",
                "snippet": {
                    "title": "Sample Upload Two",
                    "publishedAt": "2026-01-02T12:00:00Z",
                    "thumbnails": {
                        "medium": { "url": "https://i.ytimg.com/vi/abc123xyz/mqdefault.jpg" }
                    }
                },
                "statistics": {
                    "viewCount": "200",
                    "likeCount": "10",
                    "commentCount": "1"
                },
                "contentDetails": { "duration": "PT10M" }
            }
        ]
    });

    let filtered = SmartFilter::filter("youtube", &raw).expect("filter youtube");
    assert_eq!(filtered.platform, "youtube");
    assert_eq!(filtered.user_summary.username, "Google for Developers");
    assert_eq!(filtered.user_summary.user_id, "UC_x5XG1OV2P6uZZ5FSM9Ttw");
    assert_eq!(filtered.user_summary.stats.follower_count, Some(2_300_000));
    assert_eq!(filtered.user_summary.stats.total_content, 5800);

    match &filtered.content_analysis {
        ContentAnalysis::YouTube(analysis) => {
            assert_eq!(analysis.subscriber_count, 2_300_000);
            assert_eq!(analysis.view_count, 250_000_000);
            assert_eq!(analysis.video_count, 5800);
            assert_eq!(analysis.recent_videos.len(), 2);
            assert_eq!(analysis.recent_videos[0].video_id, "dQw4w9WgXcQ");
            assert_eq!(analysis.recent_videos[0].view_count, Some(1000));
            assert!(analysis.recent_videos[0].cover.is_some());
            assert!(analysis
                .channel_url
                .as_deref()
                .unwrap_or("")
                .contains("GoogleDevelopers"));
            assert!(analysis.video_summary.contains("订阅"));
        }
        other => panic!("expected YouTube analysis, got {:?}", other),
    }
    // Round-trip through SmartFilteredData JSON (cache shape)
    let as_json = serde_json::to_value(&filtered).expect("serialize");
    assert_eq!(as_json["platform"], "youtube");
    assert!(as_json["content_analysis"]["recent_videos"].is_array());
    assert_eq!(as_json["content_analysis"]["subscriber_count"], 2_300_000);
}

/// Empty public channel (0 videos) is a valid success — not a filter/fetch error.
#[test]
fn filter_youtube_empty_channel_is_valid() {
    let raw = serde_json::json!({
        "channel": {
            "id": "UCempty00000000000000000",
            "snippet": {
                "title": "Empty Channel",
                "customUrl": "@empty",
                "thumbnails": { "high": { "url": "https://example.com/a.jpg" } }
            },
            "statistics": {
                "viewCount": "0",
                "subscriberCount": "0",
                "videoCount": "0"
            },
            "contentDetails": {
                "relatedPlaylists": { "uploads": "UUempty" }
            }
        },
        "uploads_playlist_id": "UUempty",
        "playlist_items": [],
        "videos": []
    });
    let filtered = SmartFilter::filter("youtube", &raw).expect("empty youtube ok");
    assert_eq!(filtered.platform, "youtube");
    assert_eq!(filtered.user_summary.username, "Empty Channel");
    match &filtered.content_analysis {
        ContentAnalysis::YouTube(a) => {
            assert_eq!(a.video_count, 0);
            assert!(a.recent_videos.is_empty());
            assert!(
                a.video_summary.contains("暂无上传") || a.video_summary.contains("空频道"),
                "summary should say empty is ok: {}",
                a.video_summary
            );
        }
        other => panic!("expected YouTube, got {other:?}"),
    }
}

#[test]
fn test_filter_bilibili_user_key() {
    // 抓取写入 `user`（与 steam/github 一致）；filter 内部期望 user_info
    let raw = serde_json::json!({
        "user": {
            "mid": 10398973,
            "name": "染川瞳",
            "face": "https://example.com/face.jpg",
            "sign": "",
            "level": 6,
            "follower": 251,
            "following": 56
        },
        "videos": [
            { "title": "视频A", "bvid": "BV1xx", "cover": "https://example.com/a.jpg" }
        ],
        "bangumi": [
            { "title": "名侦探柯南", "season_id": 33378, "cover": "https://example.com/c.jpg", "progress": "", "season_type": 1 }
        ]
    });

    let filtered = SmartFilter::filter("bilibili", &raw).expect("filter bilibili");
    assert_eq!(filtered.user_summary.username, "染川瞳");
    assert_eq!(filtered.user_summary.user_id, "10398973");
    assert_eq!(filtered.user_summary.level.as_deref(), Some("Lv6"));
    assert_eq!(filtered.user_summary.stats.follower_count, Some(251));
    assert_eq!(filtered.user_summary.stats.following_count, Some(56));
    assert_eq!(filtered.user_summary.stats.total_content, 2);
}

#[test]
fn test_filter_bilibili_legacy_user_info_key() {
    let raw = serde_json::json!({
        "user_info": {
            "mid": "42",
            "name": "legacy",
            "level": 3,
            "follower": 10,
            "following": 2
        },
        "videos": [],
        "bangumi": []
    });
    let filtered = SmartFilter::filter("bilibili", &raw).expect("filter bilibili legacy");
    assert_eq!(filtered.user_summary.username, "legacy");
    assert_eq!(filtered.user_summary.user_id, "42");
    assert_eq!(filtered.user_summary.stats.follower_count, Some(10));
}

#[test]
fn test_filter_discord() {
    // ADMINISTRATOR = 1<<3 = 8; public_flags: HypeSquad Balance(1<<8=256) | Active Developer(1<<22)
    let raw = serde_json::json!({
        "user": {
            // 真实 snowflake（2016 年注册），用于验证账号年龄解算
            "id": "155149108183695360",
            "username": "myriad",
            "global_name": "Myriad",
            "premium_type": 2,
            "public_flags": 256 | (1u64 << 22),
            "avatar": "abcavatarhash",
            "accent_color": 5793266,
            "mfa_enabled": true
        },
        "guilds": [
            {
                "id": "g1",
                "name": "Owned Server",
                "owner": true,
                "permissions": "8",
                "icon": "abc",
                "approximate_member_count": 1200,
                "approximate_presence_count": 300,
                "features": ["COMMUNITY", "NEWS"]
            },
            {
                "id": "g2",
                "name": "Big Partner Server",
                "owner": false,
                "permissions": "0",
                "approximate_member_count": 50000,
                "approximate_presence_count": 8000,
                "features": ["PARTNERED", "VERIFIED"]
            }
        ],
        "connections": [
            {
                "type": "steam",
                "name": "myriad_steam",
                "id": "76561198000000000",
                "verified": true,
                "visibility": 1
            },
            {
                "type": "github",
                "name": "octocat",
                "id": "1",
                "verified": true,
                "visibility": 0
            }
        ],
        "myriad_cross_refs": {
            "steam_id": "76561198000000000",
            "github_username": "octocat"
        }
    });

    let filtered = SmartFilter::filter("discord", &raw).expect("filter discord");
    assert_eq!(filtered.platform, "discord");
    assert_eq!(filtered.user_summary.username, "Myriad");
    assert_eq!(filtered.user_summary.user_id, "155149108183695360");
    assert_eq!(filtered.user_summary.level.as_deref(), Some("Nitro"));
    assert_eq!(filtered.user_summary.stats.total_content, 2);

    match filtered.content_analysis {
        ContentAnalysis::Discord(analysis) => {
            assert_eq!(analysis.guild_stats.guild_count, 2);
            assert_eq!(analysis.guild_stats.owned_guild_count, 1);
            assert_eq!(analysis.guild_stats.admin_guild_count, 1);
            // Owner 服务器仍排首位，即使它成员数更少
            assert_eq!(analysis.guilds_preview[0].name, "Owned Server");
            assert!(analysis.guilds_preview[0]
                .permissions_highlight
                .contains(&"ADMINISTRATOR".to_string()));
            assert_eq!(analysis.connections.len(), 2);
            assert!(analysis
                .identity_graph
                .linked_platforms
                .contains(&"steam".to_string()));
            let steam = analysis.identity_graph.cross_check.get("steam").unwrap();
            assert!(steam.discord_linked);
            assert!(steam.myriad_configured);
            assert_eq!(steam.id_match, Some(true));
            let github = analysis.identity_graph.cross_check.get("github").unwrap();
            assert_eq!(github.name_match, Some(true));
            assert!(analysis.community_summary.contains("Myriad"));

            // with_counts 派生：总触达 / 在线 / 社区规格计数
            assert_eq!(analysis.guild_stats.total_member_reach, 51_200);
            assert_eq!(analysis.guild_stats.total_online_reach, 8_300);
            assert_eq!(analysis.guild_stats.community_guild_count, 1);
            assert_eq!(analysis.guild_stats.partnered_or_verified_count, 1);
            assert_eq!(analysis.guilds_preview[0].member_count, Some(1200));
            assert!(analysis.guilds_preview[0]
                .feature_highlight
                .contains(&"COMMUNITY".to_string()));
            // NEWS 属噪声特性，应被过滤
            assert!(!analysis.guilds_preview[0]
                .feature_highlight
                .contains(&"NEWS".to_string()));

            // identify 派生：徽章 / 头像 / accent / MFA / 账号年龄
            assert!(analysis
                .profile
                .badges
                .contains(&"Active Developer".to_string()));
            assert!(analysis
                .profile
                .badges
                .contains(&"HypeSquad Balance".to_string()));
            assert!(analysis.profile.mfa_enabled);
            assert_eq!(analysis.profile.accent_color.as_deref(), Some("#5865F2"));
            assert!(analysis
                .profile
                .avatar_url
                .as_deref()
                .unwrap()
                .contains("155149108183695360"));
            assert!(analysis.profile.account_age_years.unwrap() >= 8);
        }
        other => panic!("expected Discord analysis, got {:?}", other),
    }
}

#[test]
fn test_discord_permissions_highlight() {
    let admin = SmartFilter::discord_permissions_highlight(Some(8));
    assert_eq!(admin, vec!["ADMINISTRATOR".to_string()]);

    // MANAGE_GUILD | MANAGE_CHANNELS = 32 | 16 = 48
    let manage = SmartFilter::discord_permissions_highlight(Some(48));
    assert!(manage.contains(&"MANAGE_GUILD".to_string()));
    assert!(manage.contains(&"MANAGE_CHANNELS".to_string()));
    assert!(!manage.contains(&"ADMINISTRATOR".to_string()));
}

#[test]
fn load_youtube_filtered_cache_from_disk_if_present() {
    // Drives SmartFilter::load_platform_cache against real on-disk shape
    // (backend/cache/platforms/youtube_filtered.json). Skip if missing.
    let path = std::path::Path::new("cache/platforms/youtube_filtered.json");
    if !path.exists() {
        eprintln!("skip: no on-disk youtube_filtered.json");
        return;
    }
    let data = SmartFilter::load_platform_cache("youtube")
        .unwrap_or_else(|e| panic!("load youtube cache failed: {e}"));
    assert_eq!(data.platform, "youtube");
    match data.content_analysis {
        ContentAnalysis::YouTube(ref a) => {
            assert!(
                !a.video_summary.is_empty(),
                "expected video_summary on youtube analysis"
            );
        }
        other => panic!("expected ContentAnalysis::YouTube, got {other:?}"),
    }
    // Round-trip used by report DB save path
    let v = serde_json::to_value(&data).expect("serialize");
    let back: SmartFilteredData =
        serde_json::from_value(v).expect("deserialize SmartFilteredData after save shape");
    assert_eq!(back.platform, "youtube");
    assert!(matches!(back.content_analysis, ContentAnalysis::YouTube(_)));
}
