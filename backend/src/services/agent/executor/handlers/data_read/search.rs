use super::super::HandlerContext;
use crate::models::entities::{brew_items, brew_sources, tapps};
use sea_orm::{ColumnTrait, EntityTrait, ExprTrait, QueryFilter, QuerySelect};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(super) async fn execute_fuzzy_search(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("Missing query parameter")?;
    let scope = params
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let search_type = params.get("type").and_then(|v| v.as_str());
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let query_lower = query.to_lowercase();
    let mut results: Vec<Value> = Vec::new();

    // 搜索 Brew 订阅源
    if scope == "all" || scope == "brew" {
        if search_type.is_none() || search_type == Some("source") {
            let sources = brew_sources::Entity::find()
                .all(ctx.db)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "Agent data_read database error");
                    "Database error".to_string()
                })?;

            for source in sources {
                // Score name, category, and site_url so「友情链接」hits friend-link sources
                let name_score = calculate_fuzzy_score(&query_lower, &source.name.to_lowercase());
                let category_score = source
                    .category
                    .as_deref()
                    .map(|c| calculate_fuzzy_score(&query_lower, &c.to_lowercase()))
                    .unwrap_or(0.0);
                let site_url_score = source
                    .site_url
                    .as_deref()
                    .map(|u| calculate_fuzzy_score(&query_lower, &u.to_lowercase()))
                    .unwrap_or(0.0);
                // Also score normalized friend-link aliases against the source category field.
                let alias_category_score = {
                    use crate::services::agent::executor::utils::normalize_brew_category_filter;
                    let normalized = normalize_brew_category_filter(query);
                    if normalized != query.trim() {
                        source
                            .category
                            .as_deref()
                            .map(|c| {
                                calculate_fuzzy_score(&normalized.to_lowercase(), &c.to_lowercase())
                            })
                            .unwrap_or(0.0)
                    } else {
                        0.0
                    }
                };
                let score = name_score
                    .max(category_score)
                    .max(site_url_score)
                    .max(alias_category_score);

                if score > 0.3 {
                    results.push(json!({
                        "id": source.id.to_string(),
                        "name": source.name,
                        "type": "source",
                        "scope": "brew",
                        "score": score,
                        "metadata": {
                            "icon": source.icon,
                            "siteUrl": source.site_url,
                            "category": source.category,
                            "sourceType": source.source_type.as_str()
                        }
                    }));
                }
            }
        }

        // 搜索 Brew 内容项
        if search_type.is_none() || search_type == Some("item") {
            let items = brew_items::Entity::find()
                .limit(100)
                .all(ctx.db)
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "Agent data_read database error");
                    "Database error".to_string()
                })?;

            for item in items {
                let title_lower = item.title.to_lowercase();
                let score = calculate_fuzzy_score(&query_lower, &title_lower);

                if score > 0.3 {
                    results.push(json!({
                        "id": item.guid.clone(),
                        "name": item.title,
                        "type": "item",
                        "scope": "brew",
                        "score": score,
                        "metadata": {
                            "sourceId": item.source_id,
                            "link": item.link,
                            "publishedAt": item.published_at
                        }
                    }));
                }
            }
        }
    }

    // 搜索 Tapp 应用（与 catalog / tapp.list 同一可见性边界）
    if (scope == "all" || scope == "tapp") && (search_type.is_none() || search_type == Some("app"))
    {
        let admin_id = crate::services::tapp_ownership::get_admin_user_id(ctx.db)
            .await
            .map_err(|e| e.to_string())?;
        let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await;
        let mut query = tapps::Entity::find();
        if !is_admin {
            query = query.filter(
                tapps::Column::UserId
                    .eq(ctx.user_id)
                    .or(tapps::Column::UserId.eq(admin_id)),
            );
        }
        let apps = query.all(ctx.db).await.map_err(|e| {
            tracing::error!(error = %e, "Agent data_read database error");
            "Database error".to_string()
        })?;

        for app in apps {
            if !crate::services::tapp_ownership::install_visible_to_viewer(
                &app,
                ctx.user_id,
                admin_id,
                is_admin,
            ) {
                continue;
            }
            let name_lower = app.name.to_lowercase();
            let score = calculate_fuzzy_score(&query_lower, &name_lower);

            let desc_score = app
                .description
                .as_ref()
                .map(|d| calculate_fuzzy_score(&query_lower, &d.to_lowercase()))
                .unwrap_or(0.0);

            let max_score = score.max(desc_score * 0.8);

            if max_score > 0.3 {
                results.push(json!({
                    "id": app.id.to_string(),
                    "name": app.name,
                    "type": "app",
                    "scope": "tapp",
                    "score": max_score,
                    "metadata": {
                        "description": app.description,
                        "author": app.author,
                        "version": app.version,
                        "icon": app.icon
                    }
                }));
            }
        }
    }

    // 按得分排序
    results.sort_by(|a, b| {
        let score_a = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let score_b = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results.truncate(limit);

    if results.is_empty() {
        let can_discover = scope == "all" || scope == "brew";
        let discovery_hint = if can_discover {
            Some(json!({
                "searchQuery": query,
                "message": crate::services::agent::response_agent::not_found_in_feeds(query),
                "suggestAction": "brew.discover",
                "suggestParams": { "query": query }
            }))
        } else {
            None
        };

        return Ok(json!({
            "results": [],
            "total": 0,
            "query": query,
            "notFound": true,
            "canDiscover": can_discover,
            "discoveryHint": discovery_hint,
            "suggestions": ["Try a different keyword", "Check the spelling"]
        }));
    }

    let high_score_count = results
        .iter()
        .filter(|r| r.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) > 0.7)
        .count();

    let needs_choice = high_score_count > 1 || (results.len() > 1 && high_score_count == 0);

    Ok(json!({
        "results": results,
        "total": results.len(),
        "query": query,
        "choices": if needs_choice {
            Some(results.iter().map(|r| json!({
                "value": r.get("id"),
                "label": r.get("name")
            })).collect::<Vec<_>>())
        } else {
            None
        }
    }))
}

/// 计算模糊匹配得分
pub(super) fn calculate_fuzzy_score(query: &str, target: &str) -> f64 {
    if query == target {
        return 1.0;
    }

    if target.contains(query) {
        let ratio = query.len() as f64 / target.len() as f64;
        return 0.7 + (ratio * 0.3);
    }

    let query_words: Vec<&str> = query.split_whitespace().collect();
    let target_words: Vec<&str> = target.split_whitespace().collect();

    let mut matched_words = 0;
    for qw in &query_words {
        for tw in &target_words {
            if tw.contains(qw) || qw.contains(tw) {
                matched_words += 1;
                break;
            }
        }
    }

    if !query_words.is_empty() {
        let word_ratio = matched_words as f64 / query_words.len() as f64;
        if word_ratio > 0.0 {
            return 0.3 + (word_ratio * 0.4);
        }
    }

    let query_chars: Vec<char> = query.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();

    let mut matches = 0;
    for qc in &query_chars {
        if target_chars.contains(qc) {
            matches += 1;
        }
    }

    if !query_chars.is_empty() {
        let char_ratio = matches as f64 / query_chars.len() as f64;
        return char_ratio * 0.3;
    }

    0.0
}
