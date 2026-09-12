//! Per-platform voice, Look paths, and model-owned card_visuals.
//!
//! Task = how to judge. Look = which JSON keys. Do not repeat one in the other.
//! Fields that `generate.rs` overwrites go in `omit`.

use super::shared;

#[derive(Clone, Copy, Debug)]
pub struct PlatformVoice {
    pub role: &'static str,
    /// Axes to cover — one insight per axis, skip an axis if Look has no data.
    pub cover: &'static str,
    pub task: &'static str,
    pub visuals: &'static str,
    /// Real JSON paths under Data. Cite names from these fields only.
    pub look: &'static str,
    pub omit: &'static [&'static str],
    pub uses_vibe: bool,
    pub uses_mass_accounts: bool,
    pub uses_console_clock: bool,
}

const STEAM_OMIT: &[&str] = &["games_count", "total_playtime", "library_items"];
const GITHUB_OMIT: &[&str] = &[
    "contribution_level",
    "languages",
    "total_contributions",
    "repos_count",
    "total_stars",
    "contribution_calendar",
    "library_items",
];
const YOUTUBE_OMIT: &[&str] = &[
    "subscriber_count",
    "view_count",
    "video_count",
    "video_summary",
    "recent_videos",
    "library_items",
    "is_empty_channel",
    "channel_title",
    "channel_id",
];
const X_OMIT: &[&str] = &["stats", "profile", "following_sample", "library_items"];
const DISCORD_OMIT: &[&str] = &[
    "stats",
    "guild_stats",
    "identity_graph",
    "connections",
    "library_items",
    "profile",
    "linked_platforms",
];
const XBOX_OMIT: &[&str] = &[
    "gamerscore",
    "games_count",
    "completed_games",
    "achievement_games",
    "completion_rate",
    "hardcore_score",
    "total_achievements",
    "total_achievements_available",
    "top_titles",
    "library_items",
];
const PSN_OMIT: &[&str] = &[
    "trophy_level",
    "platinum_count",
    "gold_count",
    "silver_count",
    "bronze_count",
    "total_trophies",
    "games_count",
    "completed_games",
    "completion_rate",
    "hardcore_score",
    "is_plus",
    "top_titles",
    "library_items",
];

const GENERIC: PlatformVoice = PlatformVoice {
    role: "You are a calm data analyst.",
    cover: "① structure ② a named object ③ tone. Skip a missing axis.",
    task: "Write a portrait.",
    visuals: "card_visuals is an empty object.",
    look: "Proper nouns in content_analysis. user_summary is identity only.",
    omit: &[],
    uses_vibe: false,
    uses_mass_accounts: false,
    uses_console_clock: false,
};

pub fn voice_for(platform: &str) -> PlatformVoice {
    match platform {
        "bilibili" => PlatformVoice {
            role: "You are an anime-fluent critic: playful, never empty praise.",
            cover: "① bangumi topics ② video topics ③ taste width. Do not stack all three on currently-watching. Ban danmaku: 下次一定, AWSL, 高能预警, 泪目, next time for sure, hype warning.",
            task: "Each danmaku must match a name in Look. Without progress, do not write currently-watching or dropped.",
            visuals: "danmaku: 5-8 lines, each ≤12 chars, not synonymous.",
            look: "①anime_analysis[].examples / genres ②recent_videos[].title ③category (Anime/TvSeries/Movie) / count / percentage. Completion only from Data.raw_unknown_content[].metadata.progress. Do not put level in summary.",
            omit: &["library_items", "user_level", "follower_count", "following_count"],
            uses_vibe: false,
            uses_mass_accounts: false,
            uses_console_clock: false,
        },
        "steam" => PlatformVoice {
            role: "You are a hardcore player who reads grind and wishlist-addict habits.",
            cover: "① whether playtime is concentrated ② genre smell ③ library size vs recently played. Ban filler like 'wishlist hoarder'.",
            task: "player_type looks only at playtime concentration, not library size. A few games with huge minutes = hardcore; large library and low minutes per title = casual. Score hardcore_score 0-100 on that.",
            visuals: "player_type must be hardcore|casual|balanced. hardcore_score: integer 0-100.",
            look: "①recent_games[].name + playtime (lifetime minutes, not two weeks) ②genre_analysis[].genre / examples / count / total_playtime (minutes) ③games_count vs recent_games. total_playtime_minutes is total minutes; do not convert to hours. Names may also come from raw_unknown_content[].title.",
            omit: STEAM_OMIT,
            uses_vibe: false,
            uses_mass_accounts: false,
            uses_console_clock: false,
        },
        "github" => PlatformVoice {
            role: "You are an open-source reviewer: precise, a little sharp. Do not call everyone a veteran.",
            cover: "① highest-star repo ② calendar density ③ language stack. Skip ② if the calendar is empty.",
            task: "Stars are reach; the calendar is consistency. One-off high stars ≠ sustained work. Homework and template repos are not reach. If sparse, say so.",
            visuals: "card_visuals is an empty object.",
            look: "①recent_repos[].name / stars / forks / description. forks is times forked by others, not whether this repo is a fork; do not invent fork repos. ②calendar_span_days / calendar_active_days, contribution_calendar (only days with count>0; density from those two counts) ③language_distribution, public_repos (may exceed the sample). Name the repo with the most stars.",
            omit: GITHUB_OMIT,
            uses_vibe: false,
            uses_mass_accounts: false,
            uses_console_clock: false,
        },
        "youtube" => PlatformVoice {
            role: "You are a YouTube channel watcher: sharp, never inflating.",
            cover: "① whether subscribers match average views ② title topics ③ whether uploads have stopped. Do not invent series names.",
            task: "channel_type follows recent_videos titles only; do not invent from the channel name.",
            visuals: "vibe; channel_type ≤8 chars (has videos / tech tutorial / life vlog / cold-start / dormant). Ban 'stable updates'. Put fit in ①, not in channel_type.",
            look: "①subscriber_count / view_count / video_count (0 = empty channel) ②recent_videos[].title / view_count / like_count / comment_count ③published_at / duration (ISO 8601, e.g. PT4M13S; do not treat as hours).",
            omit: YOUTUBE_OMIT,
            uses_vibe: true,
            uses_mass_accounts: false,
            uses_console_clock: false,
        },
        "netease" => PlatformVoice {
            role: "You are a music critic who hates clichés. Sentences may be lyrical; judgments must land.",
            cover: "① a representative artist or genre ② language or region ③ emotional texture. Do not stack near-synonyms like 'soft gentle healing'.",
            task: "mood_keywords: at least 4, no near-synonyms, drawn from genre / language / mood / setting. soul_color follows genre. level is breadth and depth, not count.",
            visuals: "soul_color (#RRGGBB); mood_keywords 4-6 items {tag,color}; level 1-10.",
            look: "①recent_songs[].title / artist, artist_analysis.favorite_artists / genre_analysis / artist_count ②artist_analysis.region_distribution ③infer mood from genre; there is no time-of-day field.",
            omit: &["library_items", "follower_count", "playlist_count"],
            uses_vibe: false,
            uses_mass_accounts: false,
            uses_console_clock: false,
        },
        "bangumi" => PlatformVoice {
            role: "You read ACG taste from collections and scores.",
            cover: "① wish vs done ② how tight the scores are ③ one watching, high-score, or recently updated title, also using subject_type. No inventories.",
            task: "Judge whether the wishlist is piling up and whether scores are inflated. taste_profile is a badge, not an identity.",
            visuals: shared::CATALOG_VISUALS,
            look: "①collection_type_distribution (wish/done/doing/on_hold/dropped) ②top_rated_subjects[].title / rate ③watching_subjects[].title, recent_updates[].title, subject_type_distribution (book/anime/music/game/real). tag_distribution is for the card only.",
            omit: &[
                "library_items",
                "status_counts",
                "favorite_tags",
                "top_subjects",
                "subject_type_distribution",
                "collection_type_distribution",
            ],
            uses_vibe: false,
            uses_mass_accounts: false,
            uses_console_clock: false,
        },
        "mal" => PlatformVoice {
            role: "You are an international reviewer reading a MAL list.",
            cover: "① wish vs done ② mean_score vs personal scores ③ one English-title work, also using subject_type. No inventories. Ban: MAL collector, avid fan.",
            task: "If mean_score exists, compare it with top_rated rates: generous or harsh. taste_profile is a badge.",
            visuals: shared::CATALOG_VISUALS,
            look: "①collection_type_distribution (wish/done/doing/on_hold/dropped) ②mean_score + top_rated_subjects[].title / rate, days_watched (effort only) ③watching_subjects[].title (English original), recent_updates[].title, subject_type_distribution (book/anime/music/game/real).",
            omit: &[
                "library_items",
                "status_counts",
                "favorite_tags",
                "top_subjects",
                "subject_type_distribution",
                "collection_type_distribution",
                "mean_score",
                "days_watched",
            ],
            uses_vibe: false,
            uses_mass_accounts: false,
            uses_console_clock: false,
        },
        "x" => PlatformVoice {
            role: "You observe X. Who they follow is often more honest than what they post.",
            cover: "① post volume and engagement ② follow circles ③ one post or one follow. Skip ② if the sample is empty. Ban circle names: games, tech, entertainment, news, 游戏、科技、娱乐、新闻.",
            task: "If there are posts, read style and engagement; if few, treat them as an observer. If the sample is empty, do not invent follows.",
            visuals: "Always emit (empty if no data, never omit keys): \
engagement_level (high engagement / immersed observer / burst poster); \
signature_topics (3-6 if posts or follows exist, else [], each ≤6 chars); \
interest_circles (2-4 facets only if following_sample exists, else []: {\"name\":\"≤6-char concrete circle, never other\",\"count\":n,\"accounts\":[≤3 usernames]}, unique, count desc); \
following_highlights (3-5 if sample exists, else []: {\"username\",\"name\",\"tag\"} from following_sample).",
            look: "①engagement_stats (total_posts / total_likes_received / total_retweets_received / total_replies_received / total_impressions). impressions 0 means missing, not zero reach. No liked_posts: do not write like habits. ②following_sample[].username / name / description (prefer this over tweets when truncated) ③top_posts / recent_posts[].text, language_distribution.",
            omit: X_OMIT,
            uses_vibe: true,
            uses_mass_accounts: true,
            uses_console_clock: false,
        },
        "xbox" => PlatformVoice {
            role: "You are an Xbox achievement hunter. Trust completion, not inferred grind hours.",
            cover: "① completion density ② whether GS matches completion ③ one recent or 100% title. Do not call everyone a hunter.",
            task: "gamer_type follows completion: many finishes = hunter; high GS with low completion = collector or wide net.",
            visuals: "gamer_type ≤8 chars (completion hunter / wide net / story completer / GS collector / weekend console).",
            look: "①completed_games / average_completion / achievement_games / games_count ②gamerscore, total_achievements_earned / total_achievements_available ③recent_titles[].name / progress / last_played / devices, top_completed_titles[].name. account_tier / reputation are background only.",
            omit: XBOX_OMIT,
            uses_vibe: false,
            uses_mass_accounts: false,
            uses_console_clock: true,
        },
        "psn" => PlatformVoice {
            role: "You are a platinum hunter. Trust the trophy cabinet, not inferred grind hours.",
            cover: "① platinum count ② completion density ③ one recent or 100% trophy title. Do not call everyone a hunter.",
            task: "hunter_type follows platinums and completion: many platinums = collector; scattered trophies = casual.",
            visuals: "hunter_type ≤8 chars (platinum collector / casual trophies / story completer / trophy hunter / weekend console).",
            look: "①platinum_count / gold_count / silver_count / bronze_count ②average_progress / completed_games / games_count / trophy_level ③recent_titles[].name / progress / platform, top_completed_titles[].name. is_plus is background only.",
            omit: PSN_OMIT,
            uses_vibe: false,
            uses_mass_accounts: false,
            uses_console_clock: true,
        },
        "discord" => PlatformVoice {
            role: "You observe Discord communities. Light humor is fine; cruelty is not.",
            cover: "① owned/admin or not ② plaza vs small circle ③ one link, badge, or server name. If owned=admin=0, do not write host.",
            task: "connections are interest clues only. Each guild_takes entry must match a guilds_preview name.",
            visuals: "Always emit:\
role_profile ≤8 chars (community host / circle regular / lurker / cross-platform node; must match owned/admin counts);\
community_tags 2-3 items, each ≤6 chars;\
guild_takes: one take per guilds_preview entry, max 8, never invent servers: {\"name\": \"guilds_preview original name\", \"id\", \"take\": \"≤16 chars\"}.",
            look: "①guild_stats.owned_guild_count / admin_guild_count / manage_guild_count / guild_count ②guilds_preview[].name / id / member_count / owner / feature_highlight, guild_stats.total_member_reach / community_guild_count ③connections[].type / name (Data keeps public connections only), profile.badges / nitro / account_age_years, identity_graph.linked_platforms (already aligned with public connections).",
            omit: DISCORD_OMIT,
            uses_vibe: true,
            uses_mass_accounts: false,
            uses_console_clock: false,
        },
        _ => GENERIC,
    }
}

/// Platforms that have a dedicated report prompt.
///
/// Production does not read this list. `prompts/mod.rs` tests walk it so a
/// new platform without a prompt fails there.
#[cfg(test)]
pub const KNOWN_PLATFORMS: &[&str] = &[
    "bilibili", "steam", "github", "youtube", "netease", "bangumi", "mal", "x", "xbox", "psn",
    "discord",
];
