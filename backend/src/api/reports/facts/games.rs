//! Steam, Xbox and PSN facts. Counts are measured, never the model's.

use serde_json::{Map, Value, json};

use super::PlatformFacts;
use crate::api::reports::extract::extract_steam_library_items;
use crate::services::smart_filter::{
    ContentAnalysis, PsnTitleItem, SmartFilter, SmartFilteredData, XboxTitleItem,
};

pub(super) async fn steam(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    if let ContentAnalysis::Steam(analysis) = &metadata.content_analysis {
        let games_count = if analysis.games_count > 0 {
            analysis.games_count
        } else {
            // 旧缓存无 games_count 时，用 recent + 未知列表兜底
            analysis.recent_games.len().max(
                analysis
                    .genre_analysis
                    .iter()
                    .map(|g| g.examples.len())
                    .sum(),
            )
        };
        let minutes = analysis.total_playtime_minutes.max(0);
        let hours = (minutes / 60) as u64;
        out.card("games_count", json!(games_count));
        out.card("total_playtime", json!(hours));
        out.fact("games_count", "Games owned", games_count);
        out.fact("total_playtime_hours", "Lifetime playtime, hours", hours);
        if let Some(top) = analysis
            .recent_games
            .iter()
            .filter(|g| !g.name.trim().is_empty())
            .max_by_key(|g| g.playtime)
        {
            out.fact_with_media(
                "top_game",
                "Most played game",
                top.name.clone(),
                top.image.clone(),
            );
            out.fact(
                "top_game_hours",
                "Hours in the most played game",
                top.playtime.max(0) / 60,
            );
        }
        if let Some(genre) = analysis
            .genre_analysis
            .iter()
            .filter(|g| !g.genre.trim().is_empty())
            .max_by_key(|g| g.count)
        {
            out.fact("top_genre", "Most owned genre", genre.genre.clone());
        }
    }
    if let Ok(library_items) = extract_steam_library_items(metadata).await {
        out.card("library_items", json!(library_items));
    }
    out
}

/// Steam keeps the model's `hardcore_score` flavour, clamped to 0..=100, and
/// maps Chinese or English `player_type` labels to a stable enum.
pub(super) fn normalize_steam_visuals(obj: &mut Map<String, Value>) {
    let score = obj
        .get("hardcore_score")
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|u| u as i64))
                .or_else(|| {
                    v.as_f64()
                        .and_then(|f| f.is_finite().then_some(f.round() as i64))
                })
        })
        .unwrap_or(50)
        .clamp(0, 100);
    obj.insert("hardcore_score".to_string(), json!(score));
    if let Some(raw) = obj
        .get("player_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
    {
        obj.insert(
            "player_type".to_string(),
            json!(normalize_steam_player_type(&raw)),
        );
    }
}

pub(super) fn xbox(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    let ContentAnalysis::Xbox(analysis) = &metadata.content_analysis else {
        return out;
    };
    out.card(
        "gamertag",
        json!(
            analysis
                .display_gamertag
                .clone()
                .unwrap_or_else(|| metadata.user_summary.username.clone())
        ),
    );
    if let Some(ref av) = analysis.avatar {
        out.card("avatar", json!(av));
    }
    if let Some(ref tier) = analysis.account_tier {
        out.card("account_tier", json!(tier));
    }
    out.card("gamerscore", json!(analysis.gamerscore));
    out.card("games_count", json!(analysis.games_count));
    out.card("achievement_games", json!(analysis.achievement_games));
    out.card("completed_games", json!(analysis.completed_games));
    out.card(
        "completion_rate",
        json!(analysis.average_completion.round()),
    );
    out.card(
        "total_achievements",
        json!(analysis.total_achievements_earned),
    );
    out.card(
        "total_achievements_available",
        json!(analysis.total_achievements_available),
    );
    out.card("hardcore_score", json!(analysis.hardcore_score));
    out.card(
        "top_titles",
        json!(
            analysis
                .top_completed_titles
                .iter()
                .take(6)
                .map(|t| json!({
                    "name": t.name,
                    "progress": t.progress.round(),
                    "gamerscore": t.gamerscore_earned,
                    "achievements_earned": t.achievements_earned,
                    "achievements_total": t.achievements_total,
                    "image": t.display_image,
                }))
                .collect::<Vec<_>>()
        ),
    );
    // 详情轮播：优先「有进度 + 有封面」→ 最近有封面 → 其余
    let mut lib_src: Vec<&XboxTitleItem> = Vec::new();
    for t in analysis.top_completed_titles.iter() {
        if t.display_image.is_some() {
            lib_src.push(t);
        }
    }
    for t in analysis.recent_titles.iter() {
        if lib_src.len() >= 12 {
            break;
        }
        if t.display_image.is_some() && !lib_src.iter().any(|x| x.title_id == t.title_id) {
            lib_src.push(t);
        }
    }
    for t in analysis.recent_titles.iter() {
        if lib_src.len() >= 12 {
            break;
        }
        if !lib_src.iter().any(|x| x.title_id == t.title_id) {
            lib_src.push(t);
        }
    }
    let library_items: Vec<Value> = lib_src
        .iter()
        .take(12)
        .map(|t| {
            let cover = t
                .display_image
                .as_ref()
                .map(|u| SmartFilter::normalize_xbox_media_url(u));
            json!({
                "title": t.name,
                "type": "game",
                "cover": cover,
                "progress": t.progress.round(),
                "achievements_earned": t.achievements_earned,
                "achievements_total": t.achievements_total,
                "gamerscore": t.gamerscore_earned,
            })
        })
        .collect();
    out.card("library_items", json!(library_items));

    out.fact("gamerscore", "Gamerscore", analysis.gamerscore);
    out.fact("games_count", "Games played", analysis.games_count);
    out.fact("completed_games", "Games at 100%", analysis.completed_games);
    out.fact(
        "completion_rate",
        "Average completion, percent",
        analysis.average_completion.round(),
    );
    out.fact(
        "total_achievements",
        "Achievements unlocked",
        analysis.total_achievements_earned,
    );
    if let Some(top) = analysis
        .top_completed_titles
        .first()
        .or_else(|| analysis.recent_titles.first())
    {
        out.fact_with_media(
            "top_title",
            "Furthest-completed game",
            top.name.clone(),
            top.display_image
                .as_ref()
                .map(|u| SmartFilter::normalize_xbox_media_url(u)),
        );
        out.fact(
            "top_title_progress",
            "Completion of that game, percent",
            top.progress.round(),
        );
    }
    if let Some(recent) = analysis.recent_titles.first() {
        out.fact_with_media(
            "recent_title",
            "Most recently played game",
            recent.name.clone(),
            recent
                .display_image
                .as_ref()
                .map(|u| SmartFilter::normalize_xbox_media_url(u)),
        );
    }
    out
}

pub(super) fn psn(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    let ContentAnalysis::Psn(analysis) = &metadata.content_analysis else {
        return out;
    };
    out.card(
        "online_id",
        json!(
            analysis
                .display_online_id
                .clone()
                .unwrap_or_else(|| metadata.user_summary.username.clone())
        ),
    );
    if let Some(ref av) = analysis.avatar {
        out.card("avatar", json!(av));
    }
    if let Some(plus) = analysis.is_plus {
        out.card("is_plus", json!(plus));
    }
    out.card("trophy_level", json!(analysis.trophy_level));
    out.card("platinum_count", json!(analysis.platinum_count));
    out.card("gold_count", json!(analysis.gold_count));
    out.card("silver_count", json!(analysis.silver_count));
    out.card("bronze_count", json!(analysis.bronze_count));
    out.card("total_trophies", json!(analysis.total_trophies));
    out.card("games_count", json!(analysis.games_count));
    out.card("completed_games", json!(analysis.completed_games));
    out.card("completion_rate", json!(analysis.average_progress.round()));
    out.card("hardcore_score", json!(analysis.hardcore_score));
    out.card(
        "top_titles",
        json!(
            analysis
                .top_completed_titles
                .iter()
                .take(6)
                .map(|t| json!({
                    "name": t.name,
                    "progress": t.progress,
                    "platinum": t.earned_platinum > 0,
                    "platform": t.platform,
                    "image": t.icon_url.as_ref().map(|u| {
                        SmartFilter::normalize_https_media_url(u)
                    }),
                }))
                .collect::<Vec<_>>()
        ),
    );
    // 详情轮播：有进度+封面优先 → 最近有封面 → 其余
    let mut lib_src: Vec<&PsnTitleItem> = Vec::new();
    for t in analysis.top_completed_titles.iter() {
        if t.icon_url.is_some() {
            lib_src.push(t);
        }
    }
    for t in analysis.recent_titles.iter() {
        if lib_src.len() >= 12 {
            break;
        }
        if t.icon_url.is_some() && !lib_src.iter().any(|x| x.name == t.name) {
            lib_src.push(t);
        }
    }
    for t in analysis.recent_titles.iter() {
        if lib_src.len() >= 12 {
            break;
        }
        if !lib_src.iter().any(|x| x.name == t.name) {
            lib_src.push(t);
        }
    }
    let library_items: Vec<Value> = lib_src
        .iter()
        .take(12)
        .map(|t| {
            let cover = t
                .icon_url
                .as_ref()
                .map(|u| SmartFilter::normalize_https_media_url(u));
            json!({
                "title": t.name,
                "type": "game",
                "cover": cover,
                "progress": t.progress,
                "platinum": t.earned_platinum > 0,
                "platform": t.platform,
            })
        })
        .collect();
    out.card("library_items", json!(library_items));

    out.fact("trophy_level", "Trophy level", analysis.trophy_level);
    out.fact(
        "platinum_count",
        "Platinum trophies",
        analysis.platinum_count,
    );
    out.fact("total_trophies", "Trophies earned", analysis.total_trophies);
    out.fact("games_count", "Games played", analysis.games_count);
    out.fact("completed_games", "Games at 100%", analysis.completed_games);
    out.fact(
        "completion_rate",
        "Average progress, percent",
        analysis.average_progress.round(),
    );
    if let Some(top) = analysis
        .top_completed_titles
        .first()
        .or_else(|| analysis.recent_titles.first())
    {
        out.fact_with_media(
            "top_title",
            "Furthest-completed game",
            top.name.clone(),
            top.icon_url
                .as_ref()
                .map(|u| SmartFilter::normalize_https_media_url(u)),
        );
        out.fact(
            "top_title_progress",
            "Progress in that game, percent",
            top.progress,
        );
    }
    if let Some(recent) = analysis.recent_titles.first() {
        out.fact_with_media(
            "recent_title",
            "Most recently played game",
            recent.name.clone(),
            recent
                .icon_url
                .as_ref()
                .map(|u| SmartFilter::normalize_https_media_url(u)),
        );
    }
    out
}

/// Steam `player_type` → stable enum for FE i18n.
pub(crate) fn normalize_steam_player_type(raw: &str) -> &'static str {
    let t = raw.trim();
    let lower = t.to_ascii_lowercase();
    if lower == "hardcore" || t.contains("硬核") || lower.contains("hardcore") || t.contains("肝帝")
    {
        return "hardcore";
    }
    if lower == "casual" || t.contains("休闲") || lower.contains("casual") || t.contains("佛系")
    {
        return "casual";
    }
    if lower == "balanced" || t.contains("均衡") || lower.contains("balanced") {
        return "balanced";
    }
    // 空串 casual；无法识别的非空文案 balanced。
    if t.is_empty() { "casual" } else { "balanced" }
}

pub(super) fn xbox_gamer_type_fallback(
    locale: &str,
    completed: usize,
    avg: f64,
    gs: i64,
) -> String {
    if completed >= 5 {
        crate::i18n::reports(locale, "xbox.hunter")
    } else if avg >= 50.0 {
        crate::i18n::reports(locale, "xbox.deepCompleter")
    } else if gs >= 10_000 {
        crate::i18n::reports(locale, "xbox.gsCollector")
    } else {
        crate::i18n::reports(locale, "xbox.wideNet")
    }
}

pub(super) fn psn_hunter_type_fallback(locale: &str, platinum: i64, avg: f64) -> String {
    if platinum >= 10 {
        crate::i18n::reports(locale, "psn.collector")
    } else if platinum > 0 {
        crate::i18n::reports(locale, "psn.storyCompleter")
    } else if avg >= 50.0 {
        crate::i18n::reports(locale, "psn.trophyHunter")
    } else {
        crate::i18n::reports(locale, "psn.casual")
    }
}
