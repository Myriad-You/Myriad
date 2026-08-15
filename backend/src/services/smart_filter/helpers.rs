
use serde::{Deserialize, Serialize};
use serde_json::Value;


/// Parse a non-negative integer from JSON without dropping valid `u64` / float /
/// string forms (Steam / GitHub APIs occasionally switch representation).
///
/// Returns `None` for missing/non-numeric; never returns negative.
pub(crate) fn json_nonneg_i64(v: &Value) -> Option<i64> {
    let n = if let Some(i) = v.as_i64() {
        i
    } else if let Some(u) = v.as_u64() {
        if u > i64::MAX as u64 {
            i64::MAX
        } else {
            u as i64
        }
    } else if let Some(f) = v.as_f64() {
        if !f.is_finite() {
            return None;
        }
        f.round() as i64
    } else {
        let s = v.as_str()?;
        let t = s.trim().replace(',', "");
        if t.is_empty() {
            return None;
        }
        t.parse::<i64>()
            .ok()
            .or_else(|| t.parse::<f64>().ok().map(|f| f.round() as i64))?
    };
    Some(n.max(0))
}

/// Steam `playtime_forever` is minutes. Cap absurd values (API glitches / unit
/// mix-ups) at ~100 years so downstream hour conversion stays sane.
pub(crate) const STEAM_PLAYTIME_MINUTES_CAP: i64 = 100 * 365 * 24 * 60;
/// Single-day contribution spikes above this are almost always bad data.
pub(crate) const GITHUB_CONTRIB_DAY_CAP: i64 = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartFilteredData {
    pub platform: String,
    pub user_summary: UserSummary,
    pub content_analysis: ContentAnalysis,
    pub raw_unknown_content: Vec<UnknownContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSummary {
    pub username: String,
    pub user_id: String,
    pub level: Option<String>,
    pub stats: UserStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStats {
    pub follower_count: Option<i64>,
    pub following_count: Option<i64>,
    pub total_content: usize,
}

/// Untagged: order matters. Prefer variants with distinctive required fields.
/// YouTube before Bilibili — both have `video_summary` + `recent_videos`; Bilibili
/// also needs `anime_analysis`, but unknown fields are ignored so a YouTube blob
/// must not be attempted as Bilibili first in edge cases.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ContentAnalysis {
    YouTube(YouTubeAnalysis),
    Bilibili(BilibiliAnalysis),
    Steam(SteamAnalysis),
    GitHub(GitHubAnalysis),
    Netease(NeteaseAnalysis),
    Bangumi(BangumiAnalysis),
    X(XAnalysis),
    Discord(DiscordAnalysis),
    Mal(MalAnalysis),
    Xbox(XboxAnalysis),
    Psn(PsnAnalysis),
}

/// YouTube public-channel analysis (Data API v3, API key only).
///
/// `deny_unknown_fields` keeps untagged `ContentAnalysis` from accepting a
/// Bilibili blob (which also has `video_summary` + `recent_videos` plus
/// `anime_analysis`) as YouTube.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YouTubeAnalysis {
    pub video_summary: String,
    #[serde(default)]
    pub subscriber_count: i64,
    #[serde(default)]
    pub view_count: i64,
    #[serde(default)]
    pub video_count: i64,
    pub recent_videos: Vec<YouTubeVideoItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeVideoItem {
    pub title: String,
    pub video_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub like_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Xbox 成就分析（Xbox Live 无游玩时长，走成就向叙事）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XboxAnalysis {
    pub gaming_summary: String,
    pub gamerscore: i64,
    pub games_count: usize,
    /// 带成就系统的游戏数（totalAchievements > 0）
    #[serde(default)]
    pub achievement_games: usize,
    /// 成就进度 100% 的游戏数
    pub completed_games: usize,
    pub total_achievements_earned: i64,
    /// 库内可解锁成就总数（仅统计有成就系统的作品）
    #[serde(default)]
    pub total_achievements_available: i64,
    /// 有成就系统的游戏的平均完成度（0-100）；无成就系统游戏不计入
    pub average_completion: f64,
    /// 0-100 综合硬核指数（完成度 + 全成就 + GS 规模）
    #[serde(default)]
    pub hardcore_score: i64,
    /// Silver / Gold 等
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_tier: Option<String>,
    /// 玩家头像（GameDisplayPicRaw）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// 展示用 gamertag（优先 UniqueModernGamertag）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_gamertag: Option<String>,
    /// XboxOneRep（如 GoodPlayer）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reputation: Option<String>,
    /// 按最近游玩排序
    pub recent_titles: Vec<XboxTitleItem>,
    /// 按完成度排序（优先展示接近全成就的作品）
    pub top_completed_titles: Vec<XboxTitleItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XboxTitleItem {
    pub title_id: String,
    pub name: String,
    pub display_image: Option<String>,
    pub achievements_earned: i64,
    pub achievements_total: i64,
    pub gamerscore_earned: i64,
    pub gamerscore_total: i64,
    /// 成就完成度（0-100）
    pub progress: f64,
    pub last_played: Option<String>,
    /// 设备列表（PC / XboxOne / XboxSeries 等），卡片上可做轻量标签
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<String>,
}

/// PSN 奖杯分析（同样无时长数据，走奖杯向叙事）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsnAnalysis {
    pub trophy_summary_text: String,
    pub trophy_level: i64,
    pub platinum_count: i64,
    pub gold_count: i64,
    pub silver_count: i64,
    pub bronze_count: i64,
    /// 四色奖杯合计
    #[serde(default)]
    pub total_trophies: i64,
    pub games_count: usize,
    /// 奖杯进度 100% 的游戏数
    pub completed_games: usize,
    /// 平均奖杯完成度（0-100）
    pub average_progress: f64,
    /// 0-100 猎人指数（白金 + 等级 + 完成度）
    #[serde(default)]
    pub hardcore_score: i64,
    /// 玩家头像
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// 展示用 Online ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_online_id: Option<String>,
    /// 是否 PS Plus（social 元数据里有时带）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_plus: Option<bool>,
    /// 按最近奖杯动态排序
    pub recent_titles: Vec<PsnTitleItem>,
    /// 按完成度排序
    pub top_completed_titles: Vec<PsnTitleItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PsnTitleItem {
    pub name: String,
    pub platform: String,
    pub icon_url: Option<String>,
    /// 奖杯完成度（0-100）
    pub progress: i64,
    pub earned_platinum: i64,
    pub earned_gold: i64,
    pub earned_silver: i64,
    pub earned_bronze: i64,
    pub last_updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XAnalysis {
    pub post_summary: String,
    // 账号本人的展示名/头像（概览卡 header 用）
    #[serde(default)]
    pub user_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_avatar: Option<String>,
    pub engagement_stats: XEngagementStats,
    // following 字段放在 posts 前面：prompt 会按 12000 字符截断，
    // 关注列表对兴趣分析的信号比推文正文更强，优先保留
    #[serde(default)]
    pub following_summary: String,
    #[serde(default)]
    pub following_sample: Vec<XFollowingItem>,
    pub recent_posts: Vec<XPostItem>,
    pub top_posts: Vec<XPostItem>,
    pub language_distribution: std::collections::HashMap<String, usize>,
}

/// Discord 社交身份分析（社区足迹 + 跨平台连接）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordAnalysis {
    pub community_summary: String,
    pub profile: DiscordProfile,
    pub guild_stats: DiscordGuildStats,
    pub guilds_preview: Vec<DiscordGuildItem>,
    pub connections: Vec<DiscordConnectionItem>,
    pub identity_graph: DiscordIdentityGraph,
}

/// 账号画像 —— 全部来自 identify scope，无需额外权限
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordProfile {
    pub display_name: String,
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
    /// #RRGGBB，取自 accent_color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    /// Nitro 等级（Nitro / Nitro Classic / Nitro Basic）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nitro: Option<String>,
    /// 账号创建时间（从 snowflake 解出），RFC3339
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// 账号年龄（整年）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_age_years: Option<i64>,
    /// public_flags 解出的徽章（HypeSquad / Early Supporter / Active Developer 等）
    pub badges: Vec<String>,
    pub mfa_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordGuildStats {
    pub guild_count: usize,
    pub owned_guild_count: usize,
    pub admin_guild_count: usize,
    pub manage_guild_count: usize,
    /// 加入社区的成员总触达（各服 approximate_member_count 之和）
    pub total_member_reach: u64,
    /// 各服在线人数之和（approximate_presence_count）
    pub total_online_reach: u64,
    /// 官方认证 / 合作 / 已开启社区功能的服务器数量
    pub community_guild_count: usize,
    pub partnered_or_verified_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordGuildItem {
    pub id: String,
    pub name: String,
    pub icon_url: Option<String>,
    pub owner: bool,
    pub permissions_highlight: Vec<String>,
    /// approximate_member_count（with_counts=true 时可用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_count: Option<u64>,
    /// approximate_presence_count（在线人数）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_count: Option<u64>,
    /// 关注的服务器特性标签（PARTNERED / VERIFIED / COMMUNITY 等）
    pub feature_highlight: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConnectionItem {
    pub r#type: String,
    pub name: String,
    pub id: String,
    pub verified: bool,
    pub visibility: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordIdentityGraph {
    pub linked_platforms: Vec<String>,
    pub cross_check: std::collections::HashMap<String, DiscordCrossCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordCrossCheck {
    pub discord_linked: bool,
    pub myriad_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_match: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_match: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XEngagementStats {
    pub total_posts: usize,
    pub total_likes_received: i64,
    pub total_retweets_received: i64,
    pub total_replies_received: i64,
    pub total_impressions: i64,
    pub liked_posts_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XFollowingItem {
    pub username: String,
    pub name: String,
    pub description: String,
    /// None when public_metrics missing — do not treat as 0 for ranking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follower_count: Option<i64>,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XPostItem {
    pub id: String,
    pub text: String,
    pub created_at: Option<String>,
    pub like_count: i64,
    pub retweet_count: i64,
    pub reply_count: i64,
    pub impression_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BilibiliAnalysis {
    pub video_summary: String,
    pub anime_analysis: Vec<crate::services::content_databases::anime_database::CategoryAnalysis>,
    pub recent_videos: Vec<VideoItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoItem {
    pub title: String,
    /// Bilibili cover URL (preserved for library picker / share cards).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// Bilibili video id (bvid) when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bvid: Option<String>,
    /// Numeric/media id when bvid is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamAnalysis {
    pub game_summary: String,
    pub genre_analysis: Vec<crate::services::content_databases::game_database::GameGenreAnalysis>,
    pub recent_games: Vec<GameItem>,
    /// Owned library size (not “recent only”). Default 0 for older cache files.
    #[serde(default)]
    pub games_count: usize,
    /// Sum of `playtime_forever` over owned games, **minutes** (Steam API unit).
    #[serde(default)]
    pub total_playtime_minutes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameItem {
    pub name: String,
    pub playtime: i64,
    /// Steam app id — needed for cover URLs and stable item ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appid: Option<i64>,
    /// CDN header image derived from appid (or upstream cover).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAnalysis {
    pub repo_summary: String,
    pub language_distribution: std::collections::HashMap<String, usize>,
    pub recent_repos: Vec<RepoItem>,
    pub contribution_calendar: Option<Vec<ContributionDay>>,
    /// `user.public_repos` when present (may exceed `recent_repos.len()`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_repos: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionDay {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoItem {
    pub name: String,
    pub language: Option<String>,
    pub stars: Option<i64>,
    pub forks: Option<i64>,
    pub description: Option<String>,
    /// Repository page URL (html_url).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Open Graph preview image for library cards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeteaseAnalysis {
    pub music_summary: String,
    pub artist_analysis: crate::services::content_databases::artist_database::MusicAnalysis,
    pub recent_songs: Vec<SongItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongItem {
    pub title: String,
    pub artist: String,
    /// Netease song id (stable item id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Album cover (al.picUrl).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// Album name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// VIP-only when fee is 1 or 4 (or explicit isVip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_vip: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiAnalysis {
    pub collection_summary: String,
    pub subject_type_distribution: std::collections::HashMap<String, usize>,
    pub collection_type_distribution: std::collections::HashMap<String, usize>,
    pub tag_distribution: std::collections::HashMap<String, usize>,
    pub top_rated_subjects: Vec<BangumiSubjectItem>,
    pub watching_subjects: Vec<BangumiSubjectItem>,
    pub recent_updates: Vec<BangumiSubjectItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BangumiSubjectItem {
    pub subject_id: i64,
    pub title: String,
    pub subject_type: String,
    pub collection_type: String,
    pub rate: i64,
    pub cover: Option<String>,
    pub updated_at: Option<String>,
}

/// MyAnimeList 收藏分析（结构对齐 Bangumi，便于报告卡复用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalAnalysis {
    pub collection_summary: String,
    pub subject_type_distribution: std::collections::HashMap<String, usize>,
    pub collection_type_distribution: std::collections::HashMap<String, usize>,
    pub tag_distribution: std::collections::HashMap<String, usize>,
    pub top_rated_subjects: Vec<MalSubjectItem>,
    pub watching_subjects: Vec<MalSubjectItem>,
    pub recent_updates: Vec<MalSubjectItem>,
    pub mean_score: Option<f64>,
    pub days_watched: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalSubjectItem {
    pub subject_id: i64,
    pub title: String,
    pub subject_type: String,
    pub collection_type: String,
    pub rate: i64,
    pub cover: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownContent {
    pub content_type: String,
    pub title: String,
    #[serde(skip_serializing_if = "std::collections::HashMap::is_empty")]
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

pub struct SmartFilter;

