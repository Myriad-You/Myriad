//! Bilibili, YouTube, X and Discord facts.

use serde_json::{Value, json};

use super::PlatformFacts;
use crate::api::reports::extract::{extract_bilibili_library_items, extract_bilibili_user_stats};
use crate::services::smart_filter::{ContentAnalysis, SmartFilteredData};

pub(super) async fn bilibili(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    if let Ok(library_items) = extract_bilibili_library_items(metadata).await {
        out.card("library_items", json!(library_items));
    }
    // 从原始platform_data中提取用户统计数据
    if let Ok(stats) = extract_bilibili_user_stats(metadata).await {
        out.fact("user_level", "Account level", stats.level.clone());
        out.fact("follower_count", "Followers", stats.follower_count.clone());
        out.fact(
            "following_count",
            "Following",
            stats.following_count.clone(),
        );
        out.card("user_level", stats.level);
        out.card("follower_count", stats.follower_count);
        out.card("following_count", stats.following_count);
    }
    if let ContentAnalysis::Bilibili(analysis) = &metadata.content_analysis {
        if let Some(anime) = analysis
            .anime_analysis
            .iter()
            .flat_map(|item| item.examples.iter())
            .find(|s| !s.trim().is_empty())
        {
            out.fact("top_anime", "A series they follow", anime.clone());
        }
        if let Some(video) = analysis
            .recent_videos
            .iter()
            .find(|v| !v.title.trim().is_empty())
        {
            out.fact_with_media(
                "recent_video",
                "A recently watched video",
                video.title.clone(),
                video.cover.clone(),
            );
        }
        let followed: usize = analysis.anime_analysis.iter().map(|item| item.count).sum();
        out.fact("followed_series", "Series and films followed", followed);
    }
    out
}

pub(super) fn youtube(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    // 公开频道统计 + 最近上传轮播（0 视频仍是合法成功）
    if let ContentAnalysis::YouTube(analysis) = &metadata.content_analysis {
        let is_empty_channel = analysis.video_count == 0 && analysis.recent_videos.is_empty();
        out.card("subscriber_count", json!(analysis.subscriber_count));
        out.card("view_count", json!(analysis.view_count));
        out.card("video_count", json!(analysis.video_count));
        out.card("video_summary", json!(analysis.video_summary));
        out.card("is_empty_channel", json!(is_empty_channel));
        if let Some(ref avatar) = analysis.avatar {
            out.card("avatar", json!(avatar));
        }
        if let Some(ref url) = analysis.channel_url {
            out.card("channel_url", json!(url));
        }
        if let Some(ref cu) = analysis.custom_url {
            out.card("custom_url", json!(cu));
        }
        let library_items: Vec<Value> = analysis
            .recent_videos
            .iter()
            .take(12)
            .map(|v| {
                json!({
                    "title": v.title,
                    "type": "video",
                    "image": v.cover,
                    "cover": v.cover,
                    "url": v.url,
                    "video_id": v.video_id,
                    "view_count": v.view_count,
                    "like_count": v.like_count,
                    "comment_count": v.comment_count,
                    "published_at": v.published_at,
                    "duration": v.duration,
                })
            })
            .collect();
        out.card("library_items", json!(library_items));
        out.card("recent_videos", json!(analysis.recent_videos));

        out.fact("subscriber_count", "Subscribers", analysis.subscriber_count);
        out.fact("view_count", "Channel views", analysis.view_count);
        out.fact("video_count", "Videos uploaded", analysis.video_count);
        if let Some(top) = analysis
            .recent_videos
            .iter()
            .filter(|v| v.view_count.is_some())
            .max_by_key(|v| v.view_count.unwrap_or(0))
        {
            out.fact_with_media(
                "top_video",
                "Most viewed recent upload",
                top.title.clone(),
                top.cover.clone(),
            );
            out.fact("top_video_views", "Views on that upload", top.view_count);
        }
        if let Some(latest) = analysis.recent_videos.first() {
            out.fact(
                "latest_upload",
                "Latest upload date",
                latest.published_at.clone(),
            );
        }
    }
    // user_summary username for face header
    out.card("channel_title", json!(metadata.user_summary.username));
    out.card("channel_id", json!(metadata.user_summary.user_id));
    out
}

/// pbs.twimg.com：升到 _400x400；代理交给出口 normalize_json_media_urls
fn upscale_x_avatar(url: &str) -> String {
    url.replace("_normal.", "_400x400.")
}

pub(super) fn x(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    let ContentAnalysis::X(analysis) = &metadata.content_analysis else {
        return out;
    };
    let stats = &analysis.engagement_stats;
    // 账号本人资料（概览卡 header）
    let own_avatar = analysis.user_avatar.as_deref().map(upscale_x_avatar);
    out.card(
        "profile",
        json!({
            "username": metadata.user_summary.username,
            "name": analysis.user_name,
            "avatar": own_avatar,
        }),
    );
    // Prefer Option null over forcing 0 for missing follow counts
    out.card(
        "stats",
        json!({
            "followers": metadata.user_summary.stats.follower_count,
            "following": metadata.user_summary.stats.following_count,
            "posts": stats.total_posts,
            "likes_received": stats.total_likes_received,
            "retweets_received": stats.total_retweets_received,
            "replies_received": stats.total_replies_received,
            "impressions": stats.total_impressions,
            "liked_posts": stats.liked_posts_count,
        }),
    );
    // top_posts/recent_posts/language_distribution 不写入 card_visuals。
    if !analysis.following_sample.is_empty() {
        let sample: Vec<Value> = analysis
            .following_sample
            .iter()
            .map(|item| {
                let avatar = item.profile_image_url.as_deref().map(upscale_x_avatar);
                json!({
                    "username": item.username,
                    "name": item.name,
                    "description": item.description,
                    "follower_count": item.follower_count,
                    "verified": item.verified,
                    "avatar": avatar,
                })
            })
            .collect();
        out.card("following_sample", json!(sample));
    }
    // library_items：用热门帖子文本做卡片展示
    let library_items: Vec<Value> = analysis
        .top_posts
        .iter()
        .take(12)
        .map(|p| {
            json!({
                "id": p.id,
                "title": p.text.chars().take(80).collect::<String>(),
                "text": p.text,
                "like_count": p.like_count,
                "retweet_count": p.retweet_count,
                "created_at": p.created_at,
            })
        })
        .collect();
    out.card("library_items", json!(library_items));

    out.fact(
        "followers",
        "Followers",
        metadata.user_summary.stats.follower_count,
    );
    out.fact(
        "following",
        "Following",
        metadata.user_summary.stats.following_count,
    );
    out.fact("posts", "Posts in the sample", stats.total_posts);
    out.fact(
        "likes_received",
        "Likes received on those posts",
        stats.total_likes_received,
    );
    if let Some(post) = analysis.top_posts.iter().max_by_key(|p| p.like_count) {
        out.fact(
            "top_post",
            "Most liked post (first 80 characters)",
            post.text.chars().take(80).collect::<String>(),
        );
        out.fact("top_post_likes", "Likes on that post", post.like_count);
    }
    out
}

pub(super) fn discord(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    let ContentAnalysis::Discord(analysis) = &metadata.content_analysis else {
        return out;
    };
    // 账号画像（概览卡 header）—— 全部实测，覆盖 AI 幻觉
    out.card("profile", json!(analysis.profile));

    // Single guild list for library_items; guild_count 缺 member_reach 时回落到列表长度。
    let library_items: Vec<Value> = analysis
        .guilds_preview
        .iter()
        .take(12)
        .map(|g| {
            json!({
                "id": g.id,
                "title": g.name,
                "name": g.name,
                "icon": g.icon_url,
                "owner": g.owner,
                "permissions": g.permissions_highlight,
                "member_count": g.member_count,
                "presence_count": g.presence_count,
                "features": g.feature_highlight,
            })
        })
        .collect();
    let guild_count = if analysis.guild_stats.guild_count > 0 {
        analysis.guild_stats.guild_count
    } else {
        analysis.guilds_preview.len().max(library_items.len())
    };
    // 仅展示 visibility!=0 的连接；stats.connections 与数组口径一致
    let public_connections: Vec<Value> = analysis
        .connections
        .iter()
        .filter(|c| c.visibility != 0)
        .map(|c| {
            json!({
                "type": c.r#type,
                "name": c.name,
                "verified": c.verified,
            })
        })
        .collect();
    let mut stats = json!({
        "guilds": guild_count,
        "owned_guilds": analysis.guild_stats.owned_guild_count,
        "admin_guilds": analysis.guild_stats.admin_guild_count,
        "manage_guilds": analysis.guild_stats.manage_guild_count,
        "connections": public_connections.len(),
        "member_reach": analysis.guild_stats.total_member_reach,
        "online_reach": analysis.guild_stats.total_online_reach,
    });
    if let Some(stats_obj) = stats.as_object_mut() {
        // Omit zero reach (no approximate_* without privileged intents)
        if analysis.guild_stats.total_member_reach == 0 {
            stats_obj.remove("member_reach");
        }
        if analysis.guild_stats.total_online_reach == 0 {
            stats_obj.remove("online_reach");
        }
    }
    out.card("stats", stats);
    out.card("guild_stats", json!(analysis.guild_stats));
    out.card("identity_graph", json!(analysis.identity_graph));
    let public_count = public_connections.len();
    out.card("connections", json!(public_connections));
    out.card(
        "linked_platforms",
        json!(analysis.identity_graph.linked_platforms),
    );
    // Single list: library_items only (do not dual-write guilds_preview)
    out.card("library_items", json!(library_items));

    out.fact("guilds", "Servers joined", guild_count);
    out.fact(
        "owned_guilds",
        "Servers they own",
        analysis.guild_stats.owned_guild_count,
    );
    out.fact(
        "admin_guilds",
        "Servers where they are admin",
        analysis.guild_stats.admin_guild_count,
    );
    out.fact("connections", "Public linked accounts", public_count);
    if let Some(top) = analysis
        .guilds_preview
        .iter()
        .filter(|g| g.member_count.is_some())
        .max_by_key(|g| g.member_count.unwrap_or(0))
    {
        out.fact_with_media(
            "largest_guild",
            "Largest server joined",
            top.name.clone(),
            top.icon_url.clone(),
        );
        out.fact(
            "largest_guild_members",
            "Members in that server",
            top.member_count,
        );
    }
    if let Some(years) = analysis.profile.account_age_years {
        out.fact("account_age_years", "Account age, years", years);
    }
    out
}
