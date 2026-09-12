//! Read-path enrichment for stored platform reports (Xbox/PSN cache fill + media).

use serde_json::{json, Value};

use crate::services::smart_filter::SmartFilter;

/// 读出旧报告时，用 filtered 缓存补齐 Xbox/PSN 封面/头像/库（避免必须重生成报告才有图）
fn enrich_stored_platform_report(mut report: Value) -> Value {
    let platform = report
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if platform != "xbox" && platform != "psn" {
        return report;
    }

    let Some(visuals) = report.get_mut("card_visuals") else {
        return report;
    };
    if !visuals.is_object() {
        *visuals = json!({});
    }
    let Some(obj) = visuals.as_object_mut() else {
        return report;
    };

    let normalize_field = |v: &mut Value, xbox: bool| {
        if let Some(s) = v.as_str() {
            // 仅 https / xbox SSL 规范化；防盗链代理交给 finalize 的 normalize_json_media_urls
            *v = json!(if xbox {
                SmartFilter::normalize_xbox_media_url(s)
            } else {
                SmartFilter::normalize_https_media_url(s)
            });
        }
    };
    let is_xbox = platform == "xbox";
    if let Some(av) = obj.get_mut("avatar") {
        normalize_field(av, is_xbox);
    }
    if let Some(items) = obj.get_mut("library_items").and_then(|v| v.as_array_mut()) {
        for item in items {
            if let Some(c) = item.get_mut("cover") {
                normalize_field(c, is_xbox);
            }
        }
    }
    if let Some(items) = obj.get_mut("top_titles").and_then(|v| v.as_array_mut()) {
        for item in items {
            if let Some(c) = item.get_mut("image") {
                normalize_field(c, is_xbox);
            }
        }
    }

    let needs_lib = obj
        .get("library_items")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.is_empty()
                || a.iter().all(|i| {
                    i.get("cover")
                        .and_then(|c| c.as_str())
                        .map(|s| s.is_empty())
                        .unwrap_or(true)
                })
        })
        .unwrap_or(true);

    if platform == "xbox" {
        if let Ok(meta) = SmartFilter::load_platform_cache("xbox") {
            if let crate::services::smart_filter::ContentAnalysis::Xbox(analysis) =
                &meta.content_analysis
            {
                if obj
                    .get("avatar")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    if let Some(ref av) = analysis.avatar {
                        obj.insert("avatar".to_string(), json!(av));
                    }
                }
                if obj
                    .get("gamertag")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
                {
                    if let Some(ref g) = analysis.display_gamertag {
                        obj.insert("gamertag".to_string(), json!(g));
                    }
                }
                if !obj.contains_key("hardcore_score") {
                    obj.insert("hardcore_score".to_string(), json!(analysis.hardcore_score));
                }
                if !obj.contains_key("total_achievements") {
                    obj.insert(
                        "total_achievements".to_string(),
                        json!(analysis.total_achievements_earned),
                    );
                }
                obj.insert("gamerscore".to_string(), json!(analysis.gamerscore));
                obj.insert("games_count".to_string(), json!(analysis.games_count));
                obj.insert(
                    "completed_games".to_string(),
                    json!(analysis.completed_games),
                );
                obj.insert(
                    "completion_rate".to_string(),
                    json!(analysis.average_completion.round()),
                );

                if needs_lib {
                    let mut items: Vec<Value> = Vec::new();
                    for t in analysis
                        .top_completed_titles
                        .iter()
                        .chain(analysis.recent_titles.iter())
                    {
                        if items.len() >= 12 {
                            break;
                        }
                        let Some(cover) = t.display_image.as_ref().filter(|s| !s.is_empty()) else {
                            continue;
                        };
                        let title = t.name.as_str();
                        if items
                            .iter()
                            .any(|x| x.get("title").and_then(|v| v.as_str()) == Some(title))
                        {
                            continue;
                        }
                        items.push(json!({
                            "title": t.name,
                            "type": "game",
                            "cover": SmartFilter::normalize_xbox_media_url(cover),
                            "progress": t.progress.round(),
                            "achievements_earned": t.achievements_earned,
                            "achievements_total": t.achievements_total,
                            "gamerscore": t.gamerscore_earned,
                        }));
                    }
                    if !items.is_empty() {
                        obj.insert("library_items".to_string(), json!(items));
                    }
                }

                if let Some(tops) = obj.get_mut("top_titles").and_then(|v| v.as_array_mut()) {
                    for top in tops.iter_mut() {
                        let has_img = top
                            .get("image")
                            .and_then(|v| v.as_str())
                            .map(|s| !s.is_empty())
                            .unwrap_or(false);
                        if has_img {
                            continue;
                        }
                        let name = top.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        if let Some(src) = analysis
                            .top_completed_titles
                            .iter()
                            .chain(analysis.recent_titles.iter())
                            .find(|t| t.name == name)
                        {
                            if let Some(ref img) = src.display_image {
                                if let Some(o) = top.as_object_mut() {
                                    o.insert(
                                        "image".to_string(),
                                        json!(SmartFilter::normalize_xbox_media_url(img)),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        return report;
    }

    // PSN
    if let Ok(meta) = SmartFilter::load_platform_cache("psn") {
        if let crate::services::smart_filter::ContentAnalysis::Psn(analysis) =
            &meta.content_analysis
        {
            if obj
                .get("avatar")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                if let Some(ref av) = analysis.avatar {
                    obj.insert("avatar".to_string(), json!(av));
                }
            }
            if obj
                .get("online_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                if let Some(ref id) = analysis.display_online_id {
                    obj.insert("online_id".to_string(), json!(id));
                }
            }
            if !obj.contains_key("hardcore_score") {
                obj.insert("hardcore_score".to_string(), json!(analysis.hardcore_score));
            }
            if !obj.contains_key("total_trophies") {
                obj.insert("total_trophies".to_string(), json!(analysis.total_trophies));
            }
            obj.insert("trophy_level".to_string(), json!(analysis.trophy_level));
            obj.insert("platinum_count".to_string(), json!(analysis.platinum_count));
            obj.insert("gold_count".to_string(), json!(analysis.gold_count));
            obj.insert("silver_count".to_string(), json!(analysis.silver_count));
            obj.insert("bronze_count".to_string(), json!(analysis.bronze_count));
            obj.insert("games_count".to_string(), json!(analysis.games_count));
            obj.insert(
                "completed_games".to_string(),
                json!(analysis.completed_games),
            );
            obj.insert(
                "completion_rate".to_string(),
                json!(analysis.average_progress.round()),
            );
            if let Some(plus) = analysis.is_plus {
                obj.insert("is_plus".to_string(), json!(plus));
            }

            if needs_lib {
                let mut items: Vec<Value> = Vec::new();
                for t in analysis
                    .top_completed_titles
                    .iter()
                    .chain(analysis.recent_titles.iter())
                {
                    if items.len() >= 12 {
                        break;
                    }
                    let Some(cover) = t.icon_url.as_ref().filter(|s| !s.is_empty()) else {
                        continue;
                    };
                    let title = t.name.as_str();
                    if items
                        .iter()
                        .any(|x| x.get("title").and_then(|v| v.as_str()) == Some(title))
                    {
                        continue;
                    }
                    items.push(json!({
                        "title": t.name,
                        "type": "game",
                        "cover": SmartFilter::normalize_https_media_url(cover),
                        "progress": t.progress,
                        "platinum": t.earned_platinum > 0,
                        "platform": t.platform,
                    }));
                }
                if !items.is_empty() {
                    obj.insert("library_items".to_string(), json!(items));
                }
            }

            if let Some(tops) = obj.get_mut("top_titles").and_then(|v| v.as_array_mut()) {
                for top in tops.iter_mut() {
                    let has_img = top
                        .get("image")
                        .and_then(|v| v.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                    if has_img {
                        continue;
                    }
                    let name = top.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(src) = analysis
                        .top_completed_titles
                        .iter()
                        .chain(analysis.recent_titles.iter())
                        .find(|t| t.name == name)
                    {
                        if let Some(ref img) = src.icon_url {
                            if let Some(o) = top.as_object_mut() {
                                o.insert(
                                    "image".to_string(),
                                    json!(SmartFilter::normalize_https_media_url(img)),
                                );
                                o.insert("platinum".to_string(), json!(src.earned_platinum > 0));
                            }
                        }
                    }
                }
            }
        }
    }

    report
}

/// Stamp platform + normalize card_visuals so home ReportCard widgets can match
/// and render stats even when stored JSON is missing fields or double-encoded.
pub(crate) fn finalize_public_platform_report(platform: &str, report: Value) -> Value {
    let report = crate::api::reports::locale::unwrap_stored_report_json(report);

    let mut body = enrich_stored_platform_report(report);
    if let Some(obj) = body.as_object_mut() {
        obj.insert("platform".to_string(), json!(platform));
        // Always lowercase platform for widget id equality (steam not Steam).
        if let Some(p) = obj.get("platform").and_then(|v| v.as_str()) {
            obj.insert("platform".to_string(), json!(p.to_lowercase()));
        }
        let normalized_visuals = match obj.get("card_visuals") {
            Some(v) if v.is_object() => {
                // 双层嵌套 { card_visuals: { …stats } } 时剥一层；否则用对象本身
                Some(
                    v.get("card_visuals")
                        .filter(|inner| inner.is_object())
                        .cloned()
                        .unwrap_or_else(|| v.clone()),
                )
            }
            Some(v) if v.is_string() => {
                let raw = v.as_str().unwrap_or("").to_string();
                Some(
                    serde_json::from_str::<Value>(&raw)
                        .ok()
                        .filter(|parsed| parsed.is_object())
                        .unwrap_or_else(|| json!({})),
                )
            }
            _ => Some(json!({})),
        };
        if let Some(mut visuals) = normalized_visuals {
            // 旧库直链 + 生成后漏代理：读出时统一再规范化
            crate::api::profile::normalize_json_media_urls(&mut visuals);
            obj.insert("card_visuals".to_string(), visuals);
        }
    }
    body
}

#[cfg(test)]
mod finalize_public_report_media_tests {
    use super::finalize_public_platform_report;
    use crate::api::reports::generate::{
        anime_status_counts_five, github_contribution_level, normalize_steam_player_type,
    };
    use serde_json::json;

    #[test]
    fn github_contribution_level_thresholds() {
        assert_eq!(github_contribution_level(50, 2, 0), "emerging");
        assert_eq!(github_contribution_level(250, 3, 0), "active");
        assert_eq!(github_contribution_level(600, 12, 0), "veteran");
        assert_eq!(github_contribution_level(1200, 25, 0), "legendary");
        // star-only paths
        assert_eq!(github_contribution_level(0, 0, 50), "active");
        assert_eq!(github_contribution_level(0, 0, 200), "veteran");
        assert_eq!(github_contribution_level(0, 0, 1000), "legendary");
    }

    #[test]
    fn normalize_steam_player_type_maps_legacy_and_enum() {
        assert_eq!(normalize_steam_player_type("硬核玩家"), "hardcore");
        assert_eq!(normalize_steam_player_type("hardcore"), "hardcore");
        assert_eq!(normalize_steam_player_type("休闲玩家"), "casual");
        assert_eq!(normalize_steam_player_type("balanced"), "balanced");
        assert_eq!(normalize_steam_player_type("均衡型"), "balanced");
    }

    #[test]
    fn anime_status_counts_five_fills_zeros() {
        let mut m = std::collections::HashMap::new();
        m.insert("done".to_string(), 10usize);
        m.insert("doing".to_string(), 2usize);
        let v = anime_status_counts_five(&m);
        assert_eq!(v["done"], 10);
        assert_eq!(v["doing"], 2);
        assert_eq!(v["wish"], 0);
        assert_eq!(v["on_hold"], 0);
        assert_eq!(v["dropped"], 0);
    }

    #[test]
    fn normalizes_plain_card_visuals_object_on_read() {
        // 常见落库形态：card_visuals 直接是 stats 对象（非双层嵌套）
        let raw = json!({
            "platform": "bilibili",
            "summary": "x",
            "insights": [],
            "card_visuals": {
                "avatar": "https://i0.hdslb.com/bfs/face/a.jpg",
                "library_items": [
                    { "title": "v", "cover": "https://i0.hdslb.com/bfs/archive/c.jpg" }
                ]
            }
        });
        let out = finalize_public_platform_report("bilibili", raw);
        let avatar = out["card_visuals"]["avatar"].as_str().unwrap_or("");
        let cover = out["card_visuals"]["library_items"][0]["cover"]
            .as_str()
            .unwrap_or("");
        assert!(
            avatar.starts_with("/api/proxy/image?url="),
            "plain object avatar not proxied: {avatar}"
        );
        assert!(
            cover.starts_with("/api/proxy/image?url="),
            "plain object cover not proxied: {cover}"
        );
    }

    #[test]
    fn normalizes_double_nested_card_visuals() {
        let raw = json!({
            "card_visuals": {
                "card_visuals": {
                    "avatar": "https://avatars.steamstatic.com/x_full.jpg"
                }
            }
        });
        let out = finalize_public_platform_report("steam", raw);
        let avatar = out["card_visuals"]["avatar"].as_str().unwrap_or("");
        assert!(
            avatar.starts_with("/api/proxy/image?url="),
            "nested avatar not proxied: {avatar}"
        );
        // 应已剥掉内层 card_visuals 键
        assert!(out["card_visuals"].get("card_visuals").is_none());
    }
}
