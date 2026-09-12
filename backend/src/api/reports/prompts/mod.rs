//! Platform report generation prompts.
//!
//! Shared evidence / prose live in [`shared`]. Each platform supplies
//! voice, Look paths, and the `card_visuals` the model must write.
//! Fields overwritten in `generate.rs` are listed as omit.
//!
//! Assembly order: Look → Cover → Task, Check last before Data — recency bias.

mod platforms;
mod shared;

pub(crate) use platforms::voice_for;

use shared::{
    AVOID, CHECK, CONSOLE_NO_PLAYTIME, EVIDENCE, JSON_ENVELOPE, MASS_ACCOUNTS, PROSE, VIBE,
};

/// Assemble the user prompt sent to `AiAnalyzer.analyze_profile`.
pub(crate) fn build_report_prompt(platform: &str, data: &str, locale: &str) -> String {
    let voice = voice_for(platform);
    let mut blocks = Vec::with_capacity(13);
    blocks.push(format!("# Role\n{}", voice.role));
    blocks.push(format!(
        "# Language\n{}",
        crate::api::reports::locale::language_rule(locale)
    ));
    if !voice.look.is_empty() {
        blocks.push(format!(
            "# Look\nUnless noted, paths are under Data.content_analysis:\n{}",
            voice.look
        ));
    }
    blocks.push(format!("# Cover\n{}", voice.cover));
    blocks.push(format!("# Task\n{}", voice.task));
    blocks.push(format!("# card_visuals\n{}", voice.visuals));
    if voice.uses_vibe {
        blocks.push(format!("# Vibe\n{VIBE}"));
    }
    if voice.uses_mass_accounts {
        blocks.push(format!("# Taste signals\n{MASS_ACCOUNTS}"));
    }
    if voice.uses_console_clock {
        blocks.push(format!("# Clock\n{CONSOLE_NO_PLAYTIME}"));
    }
    if !voice.omit.is_empty() {
        blocks.push(format!(
            "# Omit\nThe system writes these card_visuals keys; do not write them (same names in Data are still readable): {}.",
            voice.omit.join(", ")
        ));
    }
    blocks.push(format!("# Rules\n{EVIDENCE}\n{PROSE}\n{AVOID}"));
    blocks.push(format!("# Check\n{CHECK}"));
    blocks.push(format!("# Output\n{JSON_ENVELOPE}"));
    blocks.push(format!("# Data\n{data}"));
    blocks.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::reports::prompts::platforms::KNOWN_PLATFORMS;
    use crate::api::reports::prompts::shared::CATALOG_VISUALS;

    fn prompt(platform: &str) -> String {
        build_report_prompt(platform, "{\"fixture\":true}", "zh-CN")
    }

    fn instructions(platform: &str) -> String {
        build_report_prompt(platform, "", "zh-CN")
    }

    #[test]
    fn every_known_platform_gets_shared_rules_and_json_envelope() {
        for platform in KNOWN_PLATFORMS {
            let p = prompt(platform);
            assert!(p.contains(EVIDENCE), "{platform} missing evidence");
            assert!(p.contains(PROSE), "{platform} missing prose");
            assert!(p.contains(AVOID), "{platform} missing avoid list");
            assert!(p.contains(CHECK), "{platform} missing check");
            assert!(p.contains("# Look"), "{platform} missing look map");
            assert!(p.contains("# Cover"), "{platform} missing cover axes");
            assert!(p.contains("# Language"), "{platform} missing language");
            assert!(p.contains("zh-CN"), "{platform} missing locale");
            assert!(
                !voice_for(platform).look.is_empty(),
                "{platform} look must point at real fields"
            );
            assert!(
                !voice_for(platform).cover.is_empty(),
                "{platform} cover must name evaluation axes"
            );
            assert!(
                !p.replace("偏好：", "").contains("好："),
                "{platform} must not teach with golden samples"
            );
            assert!(p.contains("\"summary\""), "{platform} missing summary key");
            assert!(
                p.contains("\"insights\""),
                "{platform} missing insights key"
            );
            assert!(
                p.contains("\"card_visuals\""),
                "{platform} missing card_visuals key"
            );
            assert!(
                p.contains("{\"fixture\":true}"),
                "{platform} dropped data payload"
            );
        }
    }

    #[test]
    fn prompt_output_language_follows_request_locale() {
        let en = build_report_prompt("steam", "", "en-US");
        assert!(en.contains("Write every user-visible string in en-US"));
        let ja = build_report_prompt("steam", "", "ja-JP");
        assert!(ja.contains("Write every user-visible string in ja-JP"));
    }

    #[test]
    fn check_sits_immediately_before_output_and_data() {
        let p = prompt("steam");
        let check = p.find("# Check").expect("check");
        let output = p.find("# Output").expect("output");
        let data = p.find("# Data").expect("data");
        assert!(check < output && output < data);
    }

    #[test]
    fn instruction_budget_stays_readable() {
        // Data is packed to 16k chars; the instruction body must stay short
        // or the model skips it. X/Discord visuals are the longest.
        for platform in KNOWN_PLATFORMS {
            let body = instructions(platform);
            assert!(
                body.len() < 4600,
                "{platform} instruction body bloated: {} chars",
                body.len()
            );
        }
    }

    #[test]
    fn catalog_taste_profile_is_a_short_hook() {
        assert!(CATALOG_VISUALS.contains("≤20"));
        assert!(CATALOG_VISUALS.contains("delicate ACG collector"));
        assert!(CATALOG_VISUALS.contains("细腻的 ACG 收藏家"));
        assert!(!CATALOG_VISUALS.replace("偏好：", "").contains("好："));
        assert!(!AVOID.replace("偏好：", "").contains("好："));
        assert!(!VIBE.replace("偏好：", "").contains("好："));
    }

    #[test]
    fn bangumi_and_mal_share_catalog_visuals() {
        let bangumi = voice_for("bangumi");
        let mal = voice_for("mal");
        assert_eq!(bangumi.visuals, CATALOG_VISUALS);
        assert_eq!(mal.visuals, CATALOG_VISUALS);
        assert_ne!(bangumi.role, mal.role);
        assert_ne!(bangumi.task, mal.task);
        assert_ne!(bangumi.cover, mal.cover);
        assert!(mal.cover.contains("mean_score"));
        assert!(mal.cover.contains("English-title"));
        assert!(mal.look.contains("mean_score"));
        assert!(mal.look.contains("days_watched"));
        assert!(bangumi.look.contains("subject_type_distribution"));
        assert!(bangumi.omit.contains(&"status_counts"));
        assert!(mal.omit.contains(&"status_counts"));
        assert!(mal.omit.contains(&"mean_score"));
        assert!(CATALOG_VISUALS.contains("taste_profile"));
        assert!(!CATALOG_VISUALS.contains("status_counts"));
        assert!(!CATALOG_VISUALS.contains("score_distribution"));
    }

    #[test]
    fn xbox_and_psn_share_console_clock_not_voice() {
        let xbox = voice_for("xbox");
        let psn = voice_for("psn");
        assert!(xbox.uses_console_clock);
        assert!(psn.uses_console_clock);
        assert!(prompt("xbox").contains(CONSOLE_NO_PLAYTIME));
        assert!(prompt("psn").contains(CONSOLE_NO_PLAYTIME));
        assert!(xbox.visuals.contains("gamer_type"));
        assert!(psn.visuals.contains("hunter_type"));
        assert!(!xbox.visuals.contains("hunter_type"));
        assert!(!psn.visuals.contains("gamer_type"));
    }

    #[test]
    fn social_cards_share_vibe_rule() {
        for platform in ["youtube", "x", "discord"] {
            let voice = voice_for(platform);
            assert!(voice.uses_vibe, "{platform}");
            assert!(prompt(platform).contains(VIBE), "{platform}");
        }
        assert!(!voice_for("steam").uses_vibe);
    }

    #[test]
    fn steam_asks_enum_not_overwritten_counts() {
        let steam = voice_for("steam");
        assert!(steam.visuals.contains("player_type"));
        assert!(steam.visuals.contains("hardcore_score"));
        assert!(!steam.visuals.contains("games_count"));
        assert!(!steam.visuals.contains("total_playtime"));
        let p = prompt("steam");
        assert!(p.contains("games_count"));
        assert!(p.contains("do not write them"));
    }

    #[test]
    fn github_does_not_ask_model_to_compute_overwritten_tier() {
        let github = voice_for("github");
        assert!(github.visuals.contains("empty object"));
        assert!(github.omit.contains(&"contribution_level"));
        assert!(github.omit.contains(&"languages"));
        assert!(github.look.contains("stars"));
        assert!(github.look.contains("forked by others"));
        assert!(github.task.contains("sparse"));
        assert!(!github.task.contains("fork仓"));
    }

    #[test]
    fn platform_cover_names_distinct_axes() {
        for platform in KNOWN_PLATFORMS {
            let cover = voice_for(platform).cover;
            assert!(
                cover.contains('①') && cover.contains('②') && cover.contains('③'),
                "{platform} cover must number three axes"
            );
        }
        assert!(voice_for("bilibili").cover.contains("bangumi"));
        assert!(voice_for("steam").cover.contains("concentrated"));
        assert!(voice_for("github").cover.contains("star"));
        assert!(voice_for("youtube").cover.contains("subscriber"));
        assert!(voice_for("netease").cover.contains("artist"));
        assert!(voice_for("bangumi").cover.contains("wish"));
        assert!(voice_for("x").cover.contains("circle"));
        assert!(voice_for("xbox").cover.contains("completion"));
        assert!(voice_for("psn").cover.contains("platinum"));
        assert!(voice_for("discord").cover.contains("host"));
        assert!(voice_for("steam").task.contains("concentration"));
        assert!(voice_for("netease").task.contains("near-synonyms"));
        assert!(voice_for("netease").task.contains("at least 4"));
        assert!(voice_for("netease").visuals.contains("4-6"));
        assert!(voice_for("xbox").task.contains("completion"));
    }

    #[test]
    fn look_maps_use_real_content_analysis_fields() {
        assert!(voice_for("steam").look.contains("total_playtime_minutes"));
        assert!(voice_for("steam").look.contains("minutes"));
        assert!(voice_for("steam").look.contains("lifetime"));
        assert!(voice_for("steam").look.contains("genre_analysis"));
        assert!(voice_for("github").look.contains("recent_repos"));
        assert!(voice_for("github").look.contains("public_repos"));
        assert!(voice_for("github").look.contains("forks"));
        assert!(voice_for("github").look.contains("calendar_span_days"));
        assert!(voice_for("github").look.contains("calendar_active_days"));
        assert!(voice_for("netease").look.contains("no time-of-day"));
        assert!(voice_for("netease").look.contains("artist_count"));
        assert!(voice_for("netease")
            .look
            .contains("artist_analysis.region_distribution"));
        assert!(!voice_for("netease").look.contains("is_vip"));
        assert!(!voice_for("netease").look.contains("fee"));
        assert!(voice_for("x").look.contains("following_sample"));
        assert!(voice_for("x").look.contains("No liked_posts"));
        assert!(voice_for("x").look.contains("zero reach"));
        assert!(voice_for("x").look.contains("recent_posts"));
        assert!(voice_for("x").look.contains("language_distribution"));
        assert!(voice_for("x").visuals.contains("else []"));
        assert!(voice_for("discord").look.contains("public connections"));
        assert!(voice_for("discord").omit.contains(&"linked_platforms"));
        assert!(voice_for("discord").visuals.contains("one take per"));
        assert!(voice_for("discord").look.contains("owned_guild_count"));
        assert!(voice_for("discord").look.contains("guild_count"));
        assert!(voice_for("discord").look.contains("badges"));
        assert!(voice_for("discord").look.contains("linked_platforms"));
        assert!(voice_for("xbox").look.contains("recent_titles"));
        assert!(voice_for("xbox").look.contains("achievement_games"));
        assert!(voice_for("xbox").look.contains("last_played"));
        assert!(voice_for("bangumi").look.contains("watching_subjects"));
        assert!(voice_for("bangumi")
            .look
            .contains("subject_type_distribution"));
        assert!(voice_for("bangumi").look.contains("recent_updates"));
        assert!(voice_for("mal").look.contains("days_watched"));
        assert!(voice_for("mal").look.contains("subject_type_distribution"));
        assert!(voice_for("bilibili").look.contains("raw_unknown_content"));
        assert!(voice_for("bilibili").look.contains("percentage"));
        assert!(voice_for("youtube").look.contains("like_count"));
        assert!(voice_for("youtube").look.contains("ISO 8601"));
        assert!(voice_for("youtube").visuals.contains("has videos"));
        assert!(voice_for("youtube").visuals.contains("stable updates"));
        assert!(!voice_for("youtube").visuals.contains("高播放低订阅"));
        assert!(voice_for("psn").look.contains("completed_games"));
        assert!(voice_for("psn").look.contains("gold_count"));
        assert!(voice_for("psn").look.contains("platform"));
        assert!(voice_for("psn").omit.contains(&"gold_count"));
        assert!(voice_for("xbox").omit.contains(&"completed_games"));
        assert!(voice_for("bilibili").omit.contains(&"follower_count"));
        assert!(voice_for("netease").omit.contains(&"playlist_count"));
        assert!(voice_for("youtube").omit.contains(&"video_summary"));
    }

    #[test]
    fn x_omits_stats_and_keeps_graph_fields() {
        let x = voice_for("x");
        assert!(x.uses_mass_accounts);
        assert!(x.visuals.contains("interest_circles"));
        assert!(x.visuals.contains("following_highlights"));
        assert!(x.visuals.contains("facets"));
        assert!(!x.visuals.contains("'stats'"));
        assert!(x.omit.contains(&"stats"));
        assert!(prompt("x").contains(MASS_ACCOUNTS));
        assert!(MASS_ACCOUNTS.contains("drop"));
        assert!(MASS_ACCOUNTS.contains("catch-all"));
        assert!(
            !MASS_ACCOUNTS.contains("便利店"),
            "do not paper over unreadable follows by banning 便利店"
        );
    }

    #[test]
    fn unknown_platform_uses_generic_analyst() {
        let unknown = voice_for("unknown-platform");
        let generic = voice_for("");
        assert_eq!(unknown.role, generic.role);
        assert!(prompt("unknown-platform").contains("data analyst"));
    }

    #[test]
    fn required_visual_fields_stay_platform_owned() {
        let cases: &[(&str, &str)] = &[
            ("bilibili", "danmaku"),
            ("steam", "player_type"),
            ("youtube", "channel_type"),
            ("netease", "soul_color"),
            ("bangumi", "taste_profile"),
            ("mal", "taste_profile"),
            ("x", "signature_topics"),
            ("xbox", "gamer_type"),
            ("psn", "hunter_type"),
            ("discord", "guild_takes"),
        ];
        for (platform, field) in cases {
            assert!(
                voice_for(platform).visuals.contains(field),
                "{platform} must still ask for {field}"
            );
        }
    }
}
