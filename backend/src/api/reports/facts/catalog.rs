//! GitHub, NetEase, Bangumi and MAL facts.

use serde_json::{Value, json};

use super::PlatformFacts;
use crate::api::reports::extract::{
    extract_bangumi_library_items, extract_github_library_items, extract_mal_library_items,
    extract_netease_library_items, extract_netease_user_stats,
};
use crate::services::smart_filter::{ContentAnalysis, SmartFilteredData};

pub(super) async fn github(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    if let ContentAnalysis::GitHub(analysis) = &metadata.content_analysis {
        let total_contributions = analysis
            .contribution_calendar
            .as_ref()
            .map(|calendar| {
                let sum: i64 = calendar.iter().map(|day| day.count.max(0)).sum();
                sum.max(0)
            })
            .unwrap_or(0);
        let repos_count = analysis
            .public_repos
            .map(|n| n.max(0) as usize)
            .unwrap_or(0)
            .max(analysis.recent_repos.len());
        // star 总数是衡量开发者影响力的重要因素
        let total_stars: i64 = analysis
            .recent_repos
            .iter()
            .filter_map(|repo| repo.stars.map(|s| s.max(0)))
            .sum();
        // Stable non-locale tier keys; FE maps via i18n.
        let contribution_level =
            github_contribution_level(total_contributions, repos_count, total_stars);

        // 语言占比：用实测 language_distribution，不用 AI 百分比
        let total_lang: usize = analysis.language_distribution.values().sum();
        let mut top_language = None;
        if total_lang > 0 {
            let mut langs: Vec<_> = analysis
                .language_distribution
                .iter()
                .map(|(name, n)| {
                    let percentage = ((*n as f64 / total_lang as f64) * 100.0)
                        .round()
                        .clamp(0.0, 100.0) as i64;
                    (name.clone(), *n, percentage)
                })
                .collect();
            langs.sort_by(|a, b| b.1.cmp(&a.1));
            top_language = langs
                .first()
                .map(|(name, _, percentage)| (name.clone(), *percentage));
            let languages: Vec<Value> = langs
                .into_iter()
                .take(5)
                .map(|(name, _, percentage)| json!({ "name": name, "percentage": percentage }))
                .collect();
            out.card("languages", json!(languages));
        }
        out.card("total_contributions", json!(total_contributions));
        out.card("repos_count", json!(repos_count));
        out.card("total_stars", json!(total_stars));
        out.card("contribution_level", json!(contribution_level));
        out.card(
            "contribution_calendar",
            json!(analysis.contribution_calendar),
        );

        out.fact(
            "total_contributions",
            "Contributions in the calendar",
            total_contributions,
        );
        if let Some(calendar) = analysis.contribution_calendar.as_ref() {
            let active = calendar.iter().filter(|day| day.count > 0).count();
            out.fact("active_days", "Days with a contribution", active);
        }
        out.fact("repos_count", "Public repositories", repos_count);
        out.fact(
            "total_stars",
            "Stars across sampled repositories",
            total_stars,
        );
        out.fact(
            "contribution_level",
            "Contribution tier (emerging/active/veteran/legendary)",
            contribution_level,
        );
        if let Some((name, percentage)) = top_language {
            out.fact("top_language", "Most used language", name);
            out.fact(
                "top_language_share",
                "Share of that language, percent",
                percentage,
            );
        }
        if let Some(repo) = analysis
            .recent_repos
            .iter()
            .filter(|repo| repo.stars.unwrap_or(0) > 0)
            .max_by_key(|repo| repo.stars.unwrap_or(0))
        {
            out.fact("top_repo", "Most starred repository", repo.name.clone());
            out.fact(
                "top_repo_stars",
                "Stars on that repository",
                repo.stars.unwrap_or(0).max(0),
            );
        }
    }
    if let Ok(library_items) = extract_github_library_items(metadata).await {
        out.card("library_items", json!(library_items));
    }
    out
}

pub(super) async fn netease(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    if let Ok(library_items) = extract_netease_library_items(metadata).await {
        out.card("library_items", json!(library_items));
    }
    if let Ok(stats) = extract_netease_user_stats(metadata).await {
        out.fact("follower_count", "Followers", stats.follower_count.clone());
        out.fact("playlist_count", "Playlists", stats.playlist_count.clone());
        out.card("follower_count", stats.follower_count);
        out.card("playlist_count", stats.playlist_count);
        // level 由 AI 在 card_visuals 中生成，不需要手动插入
    }
    if let ContentAnalysis::Netease(analysis) = &metadata.content_analysis {
        let music = &analysis.artist_analysis;
        out.fact("total_songs", "Songs in the library", music.total_songs);
        out.fact("artist_count", "Distinct artists", music.artist_count);
        if let Some(artist) = music.favorite_artists.first() {
            out.fact("top_artist", "Most collected artist", artist.clone());
        }
        if let Some(genre) = music.genre_analysis.iter().max_by_key(|g| g.count) {
            out.fact("top_genre", "Main genre", genre.genre.clone());
        }
        if let Some((region, _)) = music
            .region_distribution
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        {
            out.fact("top_region", "Main artist region", region.clone());
        }
        if let Some(song) = analysis
            .recent_songs
            .iter()
            .find(|song| !song.title.trim().is_empty())
        {
            let line = if song.artist.trim().is_empty() {
                song.title.clone()
            } else {
                format!("{} — {}", song.title, song.artist)
            };
            out.fact_with_media("recent_song", "A recent song", line, song.cover.clone());
        }
    }
    out
}

pub(super) async fn bangumi(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    if let Ok(library_items) = extract_bangumi_library_items(metadata).await {
        out.card("library_items", json!(library_items));
    }
    if let ContentAnalysis::Bangumi(analysis) = &metadata.content_analysis {
        out.card(
            "subject_type_distribution",
            json!(analysis.subject_type_distribution),
        );
        out.card(
            "collection_type_distribution",
            json!(analysis.collection_type_distribution),
        );
        // 与 MAL 对齐：卡片优先读 status_counts（五态齐全，缺项补 0）
        out.card(
            "status_counts",
            anime_status_counts_five(&analysis.collection_type_distribution),
        );
        out.card("favorite_tags", json!(analysis.tag_distribution));
        out.card("top_subjects", json!(analysis.top_rated_subjects));
        status_facts(&mut out, &analysis.collection_type_distribution);
        if let Some(top) = analysis.top_rated_subjects.iter().max_by_key(|s| s.rate) {
            out.fact_with_media(
                "top_subject",
                "Highest rated title",
                top.title.clone(),
                top.cover.clone(),
            );
            out.fact(
                "top_subject_rate",
                "Their rating of it, out of 10",
                top.rate,
            );
        }
        if let Some(watching) = analysis.watching_subjects.first() {
            out.fact_with_media(
                "watching",
                "A title in progress",
                watching.title.clone(),
                watching.cover.clone(),
            );
        }
    }
    out
}

pub(super) async fn mal(metadata: &SmartFilteredData) -> PlatformFacts {
    let mut out = PlatformFacts::default();
    if let Ok(library_items) = extract_mal_library_items(metadata).await {
        out.card("library_items", json!(library_items));
    }
    if let ContentAnalysis::Mal(analysis) = &metadata.content_analysis {
        out.card(
            "subject_type_distribution",
            json!(analysis.subject_type_distribution),
        );
        out.card(
            "collection_type_distribution",
            json!(analysis.collection_type_distribution),
        );
        out.card(
            "status_counts",
            anime_status_counts_five(&analysis.collection_type_distribution),
        );
        out.card("favorite_tags", json!(analysis.tag_distribution));
        out.card("top_subjects", json!(analysis.top_rated_subjects));
        if let Some(mean) = analysis.mean_score {
            out.card("mean_score", json!(mean));
        }
        if let Some(days) = analysis.days_watched {
            out.card("days_watched", json!(days));
        }
        status_facts(&mut out, &analysis.collection_type_distribution);
        out.fact("mean_score", "Mean score they give", analysis.mean_score);
        out.fact("days_watched", "Days watched", analysis.days_watched);
        if let Some(top) = analysis.top_rated_subjects.iter().max_by_key(|s| s.rate) {
            out.fact_with_media(
                "top_subject",
                "Highest rated title",
                top.title.clone(),
                top.cover.clone(),
            );
            out.fact(
                "top_subject_rate",
                "Their rating of it, out of 10",
                top.rate,
            );
        }
        if let Some(watching) = analysis.watching_subjects.first() {
            out.fact_with_media(
                "watching",
                "A title in progress",
                watching.title.clone(),
                watching.cover.clone(),
            );
        }
    }
    out
}

fn status_facts(out: &mut PlatformFacts, dist: &std::collections::HashMap<String, usize>) {
    let get = |k: &str| dist.get(k).copied().unwrap_or(0);
    out.fact("done_count", "Finished", get("done"));
    out.fact("doing_count", "In progress", get("doing"));
    out.fact("wish_count", "On the wishlist", get("wish"));
    out.fact("dropped_count", "Dropped", get("dropped"));
}

/// Bangumi/MAL `status_counts`: always emit five keys (0 when absent).
pub(crate) fn anime_status_counts_five(dist: &std::collections::HashMap<String, usize>) -> Value {
    let get = |k: &str| dist.get(k).copied().unwrap_or(0);
    json!({
        "done": get("done"),
        "doing": get("doing"),
        "wish": get("wish"),
        "on_hold": get("on_hold"),
        "dropped": get("dropped"),
    })
}

/// GitHub contribution tier for `card_visuals.contribution_level`.
/// Stable English enum keys only — FE maps to locale labels / badge colors.
/// Stars path: `>=1000` legendary, `>=200` veteran, `>=50` active, else emerging.
pub(crate) fn github_contribution_level(
    total_contributions: i64,
    repos_count: usize,
    total_stars: i64,
) -> &'static str {
    if (total_contributions > 1000 && repos_count > 20) || total_stars >= 1000 {
        "legendary"
    } else if (total_contributions > 500 && repos_count > 10) || total_stars >= 200 {
        "veteran"
    } else if total_contributions > 200 || repos_count > 5 || total_stars >= 50 {
        "active"
    } else {
        "emerging"
    }
}
