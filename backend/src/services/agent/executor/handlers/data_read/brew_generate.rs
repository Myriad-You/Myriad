use super::super::HandlerContext;
use crate::models::entities::{brew_items, brew_sources};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Opt-in flag for external/web search fallback (brew.generateReadingList).
/// Accepts allowWebSearch / useWebSearch / webSearch / allowExternal / external.
/// Default false — local miss must not force ai.webSearch.
pub(super) fn parse_allow_web_search(params: &HashMap<String, Value>) -> bool {
    const KEYS: &[&str] = &[
        "allowWebSearch",
        "useWebSearch",
        "webSearch",
        "allowExternal",
        "external",
    ];
    for key in KEYS {
        if let Some(v) = params.get(*key) {
            if v.as_bool() == Some(true) {
                return true;
            }
            if matches!(v.as_i64(), Some(1)) || matches!(v.as_u64(), Some(1)) {
                return true;
            }
            if let Some(s) = v.as_str() {
                let s = s.trim().to_lowercase();
                if matches!(s.as_str(), "true" | "1" | "yes" | "on") {
                    return true;
                }
            }
        }
    }
    false
}

/// Granted-layer gate for the generateReadingList web path.
///
/// Opt-in (`allowWebSearch`) is not enough: outbound Search/Fetch is the same
/// work as `ai.webSearch`, so it requires granted `ai:search`. Local ranking
/// stays `brew:read` only. Autonomy caps, when present, must also include it.
pub(super) fn outbound_web_search_allowed(
    opt_in: bool,
    granted_has_ai_search: bool,
    autonomy_cap: Option<&[String]>,
) -> bool {
    if !opt_in || !granted_has_ai_search {
        return false;
    }
    match autonomy_cap {
        None => true,
        Some(cap) => cap.iter().any(|permission| permission == "ai:search"),
    }
}

/// AI 生成阅读列表
/// 根据用户需求筛选并生成符合条件的阅读列表
pub(super) async fn execute_brew_generate_reading_list(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let criteria = params
        .get("criteria")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("keyword").and_then(|v| v.as_str()))
        .or_else(|| params.get("topic").and_then(|v| v.as_str()))
        .or_else(|| params.get("query").and_then(|v| v.as_str()))
        .unwrap_or("最新文章");
    let max_items = params
        .get("maxItems")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;
    let source_name_filter = params.get("sourceName").and_then(|v| v.as_str());
    let days_back = params.get("daysBack").and_then(|v| v.as_i64()).unwrap_or(7);
    // Opt-in only: do not force ai.webSearch when local keyword miss.
    // Cascade escalation for other capabilities is owned by myriad-149.
    // Outbound still needs granted `ai:search` (same layer as `ai.webSearch`).
    let web_search_opt_in = parse_allow_web_search(params);
    let granted = crate::services::agent::get_user_permissions(ctx.db, ctx.user_id).await;
    let allow_web_search = outbound_web_search_allowed(
        web_search_opt_in,
        granted.iter().any(|permission| permission == "ai:search"),
        ctx.autonomy_permission_cap.as_deref(),
    );

    // 获取关键词过滤条件（支持多种参数名）
    let keyword = params
        .get("keyword")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("topic").and_then(|v| v.as_str()))
        .or_else(|| {
            params
                .get("filters")
                .and_then(|v| v.get("keyword"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            params
                .get("filters")
                .and_then(|v| v.get("topic"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| params.get("query").and_then(|v| v.as_str()))
        .unwrap_or("");

    tracing::info!(
        keyword = %keyword,
        criteria = %criteria,
        allow_web_search = allow_web_search,
        web_search_opt_in = web_search_opt_in,
        "[brew.generateReadingList] Parameters parsed"
    );

    // 计算时间范围
    let cutoff_time = chrono::Utc::now() - chrono::Duration::days(days_back);

    // 从数据库获取文章
    let sources = brew_sources::Entity::find()
        .all(ctx.db)
        .await
        .unwrap_or_default();
    let source_map: std::collections::HashMap<i32, &brew_sources::Model> =
        sources.iter().map(|s| (s.id, s)).collect();

    // 查询文章，按时间筛选
    let base_query = brew_items::Entity::find()
        .filter(brew_items::Column::PublishedAt.gte(cutoff_time))
        .order_by_desc(brew_items::Column::PublishedAt);

    // 如果指定了订阅源名称，过滤
    let base_query = if let Some(name) = source_name_filter {
        let matching_source_ids: Vec<i32> = sources
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&name.to_lowercase()))
            .map(|s| s.id)
            .collect();
        if !matching_source_ids.is_empty() {
            base_query.filter(brew_items::Column::SourceId.is_in(matching_source_ids))
        } else {
            base_query
        }
    } else {
        base_query
    };

    // 最小候选数量
    const MIN_CANDIDATES: usize = 20;
    let fetch_limit: u64 = 200;

    // 尝试关键词筛选
    let mut items: Vec<brew_items::Model> = if !keyword.is_empty() {
        use sea_orm::Condition;
        let keyword_lower = format!("%{}%", keyword.to_lowercase());
        let keyword_condition = Condition::any()
            .add(brew_items::Column::Title.like(&keyword_lower))
            .add(brew_items::Column::Content.like(&keyword_lower));

        let keyword_items = base_query
            .clone()
            .filter(keyword_condition)
            .limit(fetch_limit)
            .all(ctx.db)
            .await
            .unwrap_or_default();

        tracing::info!(
            keyword = %keyword,
            found = keyword_items.len(),
            "[brew.generateReadingList] Keyword search results"
        );

        // If keyword search is empty, do not pad with unrelated local articles.
        // Web search is only attempted later when allowWebSearch is explicit.
        if keyword_items.is_empty() {
            tracing::info!(
                keyword = %keyword,
                allow_web_search = allow_web_search,
                "[brew.generateReadingList] No keyword matches in local brew_items"
            );
            vec![]
        } else if keyword_items.len() < MIN_CANDIDATES {
            // 只有当有部分结果时才补充相关文章
            tracing::info!(
                keyword = %keyword,
                found = keyword_items.len(),
                "[brew.generateReadingList] Keyword results insufficient, fetching more articles"
            );
            let keyword_ids: std::collections::HashSet<i32> =
                keyword_items.iter().map(|i| i.id).collect();
            let additional = base_query
                .limit(fetch_limit)
                .all(ctx.db)
                .await
                .unwrap_or_default();

            // 合并，去重
            let mut combined = keyword_items;
            for item in additional {
                if !keyword_ids.contains(&item.id) && combined.len() < fetch_limit as usize {
                    combined.push(item);
                }
            }
            combined
        } else {
            keyword_items
        }
    } else {
        // 无关键词，直接取最新
        base_query
            .limit(fetch_limit)
            .all(ctx.db)
            .await
            .unwrap_or_default()
    };

    // Local miss / thin results: only call AI web search when explicitly opted in.
    // Default: honest empty + local suggestions (do not force ai.webSearch / Gemini).
    let needs_more = items.is_empty() || (items.len() < 3 && !keyword.is_empty());
    if needs_more {
        let list_name = if !keyword.is_empty() {
            keyword
        } else {
            criteria
        };
        let available_sources: Vec<&str> = sources
            .iter()
            .filter(|s| !s.name.is_empty())
            .map(|s| s.name.as_str())
            .take(8)
            .collect();

        if allow_web_search && (!keyword.is_empty() || !criteria.is_empty()) {
            tracing::info!(
                keyword = %keyword,
                db_results = items.len(),
                "[brew.generateReadingList] allowWebSearch=true, trying AI web search"
            );

            let search_query = if !keyword.is_empty() {
                format!("{} 相关文章 新闻 资讯", keyword)
            } else {
                format!("{} 相关文章", criteria)
            };

            let recency_minutes = u32::try_from(days_back.saturating_mul(24 * 60)).ok();
            match crate::services::agent::web_search::execute_reading_list(
                &search_query,
                max_items,
                recency_minutes,
            )
            .await
            {
                Ok(web_results) if !web_results.is_empty() => {
                    tracing::info!(
                        results = web_results.len(),
                        "[brew.generateReadingList] AI web search returned results"
                    );
                    return Ok(json!({
                        "readingList": web_results,
                        "totalMatched": web_results.len(),
                        "listName": format!("网络搜索 - {}", list_name),
                        "criteria": criteria,
                        "fromWebSearch": true,
                        "allowWebSearch": true,
                        "message": crate::services::agent::response_agent::web_search_fallback(web_results.len()),
                        "action": {
                            "type": "reading_list",
                            "payload": {
                                "items": web_results,
                                "name": format!("网络搜索 - {}", list_name),
                                "fromWebSearch": true
                            }
                        }
                    }));
                }
                Ok(_) => {
                    tracing::info!("[brew.generateReadingList] AI web search returned no results");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "[brew.generateReadingList] AI web search failed");
                }
            }

            // Opt-in web search attempted but empty/failed
            let mut suggestions = vec![
                "尝试更换关键词".to_string(),
                "放宽 daysBack 或去掉 sourceName 限制".to_string(),
                "订阅更多相关的 RSS 源".to_string(),
                "检查是否已配置 TinyFish 或 Gemini API Key".to_string(),
            ];
            if !available_sources.is_empty() {
                suggestions.insert(0, format!("本地已有订阅：{}", available_sources.join("、")));
            }

            return Ok(json!({
                "readingList": [],
                "totalMatched": 0,
                "listName": list_name,
                "criteria": criteria,
                "matched": false,
                "notFound": true,
                "fromWebSearch": false,
                "allowWebSearch": true,
                "message": crate::services::agent::response_agent::no_articles_found(
                    if keyword.is_empty() && criteria.is_empty() {
                        "请提供搜索关键词"
                    } else {
                        "本地与联网搜索均未返回结果"
                    }
                ),
                "suggestions": suggestions,
                "availableSources": available_sources,
                "action": {
                    "type": "reading_list",
                    "payload": {
                        "items": [],
                        "name": list_name
                    }
                }
            }));
        }

        // No explicit web/external request — if we still have a few local hits,
        // continue to AI local ranking; otherwise honest empty.
        if !items.is_empty() {
            tracing::info!(
                keyword = %keyword,
                db_results = items.len(),
                "[brew.generateReadingList] Thin local results, ranking without web search"
            );
        } else {
            tracing::info!(
                keyword = %keyword,
                allow_web_search = false,
                "[brew.generateReadingList] Local keyword empty; honest empty (no forced webSearch)"
            );

            let searched = if !keyword.is_empty() {
                keyword
            } else {
                criteria
            };
            let mut suggestions = vec![
                "尝试更换或放宽关键词".to_string(),
                "增大 daysBack 查看更早文章".to_string(),
                "用 brew.items / brew.read 浏览本地订阅".to_string(),
                "订阅更多相关 RSS 源后再生成列表".to_string(),
            ];
            if !available_sources.is_empty() {
                suggestions.insert(
                    0,
                    format!("可浏览的本地订阅：{}", available_sources.join("、")),
                );
            }
            if !web_search_opt_in {
                suggestions.push("如需联网补充，请显式传 allowWebSearch=true".to_string());
            } else if !allow_web_search {
                suggestions.push("联网补充需要授予权限 ai:search".to_string());
            }

            return Ok(json!({
                "readingList": [],
                "totalMatched": 0,
                "listName": list_name,
                "criteria": criteria,
                "matched": false,
                "notFound": true,
                "fromWebSearch": false,
                "allowWebSearch": false,
                "searchedFor": searched,
                "message": crate::services::agent::response_agent::no_articles_found(
                    &format!("本地订阅中无「{}」相关文章", searched)
                ),
                "suggestions": suggestions,
                "availableSources": available_sources,
                "action": {
                    "type": "reading_list",
                    "payload": {
                        "items": [],
                        "name": list_name
                    }
                }
            }));
        }
    }

    // 限制单个来源的最大数量（不超过总数的 1/2），确保多样性
    let max_per_source = (items.len() / 2).max(3); // 至少保留3篇
    let mut source_counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    items.retain(|item| {
        let count = source_counts.entry(item.source_id).or_insert(0);
        if *count < max_per_source {
            *count += 1;
            true
        } else {
            false
        }
    });

    // 确保至少有 MIN_CANDIDATES 篇（如果原始数据足够）
    let candidates_count = items.len().min(MIN_CANDIDATES.max(max_items * 2));
    let items: Vec<_> = items.into_iter().take(candidates_count).collect();

    // 准备 AI 分析的内容（摘要限制为前50字，去除HTML标签）
    let articles_for_ai: Vec<Value> = items
        .iter()
        .map(|item| {
            let source = source_map.get(&item.source_id);
            // 提取纯文本摘要：简单去除HTML标签，取前50字符
            let summary = item
                .content
                .as_ref()
                .map(|c| {
                    // 简单的HTML标签去除
                    let mut in_tag = false;
                    let text: String = c
                        .chars()
                        .filter(|&ch| {
                            if ch == '<' {
                                in_tag = true;
                                return false;
                            }
                            if ch == '>' {
                                in_tag = false;
                                return false;
                            }
                            !in_tag && ch != '\n' && ch != '\r'
                        })
                        .take(50)
                        .collect();
                    text.trim().to_string()
                })
                .unwrap_or_default();
            json!({
                "id": item.id,
                "title": item.title,
                "sourceName": source.map(|s| s.name.as_str()).unwrap_or(""),
                "summary": summary
            })
        })
        .collect();

    // 调用 AI 进行筛选和排序
    let ai_analyzer = ctx.ai_analyzer.ok_or("AI analyzer not configured")?;

    // 构建关键词提示（如果有）
    let keyword_hint = if !keyword.is_empty() {
        format!("\n关键词筛选条件：{}\n注意：候选文章已按关键词预筛选，请进一步判断与主题的真正相关性，排除标题党或仅表面相关的文章。", keyword)
    } else {
        String::new()
    };

    let prompt = format!(
        r#"你是一个智能阅读助手。请根据用户的需求从以下文章中筛选最符合条件的文章。

用户需求：{}{}

可选文章（JSON数组）：
{}

请返回一个JSON对象，格式如下：
{{
  "selectedIds": [文章ID数组，按推荐度排序，最多{}篇],
  "listName": "为这个阅读列表起一个简短的名字（与用户需求相关）",
  "reasons": {{
    "文章ID": "为什么推荐这篇文章（一句话）"
  }}
}}

筛选标准：
1. 与用户需求的相关性（最重要）
2. 内容质量和价值
3. 时效性
4. 如果没有真正符合条件的文章，selectedIds 可以为空数组

只返回JSON，不要其他内容。"#,
        criteria,
        keyword_hint,
        serde_json::to_string_pretty(&articles_for_ai).unwrap_or_default(),
        max_items
    );

    let ai_response = ai_analyzer.analyze(&prompt).await.map_err(|e| {
        tracing::error!(error = %e, "[brew.generateReadingList] AI analysis failed");
        "Failed to analyze with AI".to_string()
    })?;

    // 解析 AI 响应
    let ai_result: Value = extract_json_from_response(&ai_response)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| {
            json!({
                "selectedIds": items.iter().take(max_items).map(|i| i.id).collect::<Vec<_>>(),
                "listName": format!("阅读列表 - {}", criteria),
                "reasons": {}
            })
        });

    let selected_ids: Vec<i64> = ai_result
        .get("selectedIds")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    let list_name = ai_result
        .get("listName")
        .and_then(|v| v.as_str())
        .unwrap_or("智能阅读列表")
        .to_string();

    let reasons = ai_result.get("reasons").cloned().unwrap_or(json!({}));

    // 构建最终阅读列表
    let reading_list: Vec<Value> = selected_ids
        .iter()
        .filter_map(|&id| {
            items.iter().find(|item| item.id as i64 == id).map(|item| {
                let source = source_map.get(&item.source_id);
                let reason = reasons
                    .get(id.to_string())
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                json!({
                    "id": item.id,
                    "title": item.title,
                    "author": item.author,
                    "sourceName": source.map(|s| s.name.as_str()).unwrap_or(""),
                    "publishedAt": item.published_at.to_rfc3339(),
                    "summary": item.content.as_ref()
                        .map(|c| {
                            // 简单提取摘要：去除HTML标签，取前200字符
                            let text: String = c.chars()
                                .filter(|&ch| ch != '<' && ch != '>')
                                .take(200)
                                .collect();
                            text
                        })
                        .unwrap_or_default(),
                    "relevanceReason": reason,
                    "link": item.link
                })
            })
        })
        .take(max_items)
        .collect();

    tracing::info!(
        criteria = %criteria,
        total_candidates = items.len(),
        selected_count = reading_list.len(),
        "[brew.generateReadingList] Generated reading list"
    );

    Ok(json!({
        "readingList": reading_list,
        "totalMatched": reading_list.len(),
        "listName": list_name,
        "criteria": criteria,
        "action": {
            "type": "reading_list",
            "payload": {
                "items": reading_list,
                "name": list_name
            }
        }
    }))
}

/// 从 AI 响应中提取 JSON 字符串
fn extract_json_from_response(response: &str) -> Option<String> {
    // 尝试找到 JSON 代码块
    if let Some(start) = response.find("```json") {
        let content_start = start + 7;
        if let Some(end) = response[content_start..].find("```") {
            return Some(
                response[content_start..content_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // 尝试找到普通代码块
    if let Some(start) = response.find("```") {
        let content_start = start + 3;
        // 跳过可能的语言标识
        let actual_start = response[content_start..]
            .find('\n')
            .map(|n| content_start + n + 1)
            .unwrap_or(content_start);
        if let Some(end) = response[actual_start..].find("```") {
            return Some(
                response[actual_start..actual_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // 尝试直接解析为 JSON（查找 { 和 } 的匹配）
    if let Some(start) = response.find('{') {
        let mut depth = 0;
        let mut end_pos = start;
        for (i, ch) in response[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end_pos > start {
            return Some(response[start..end_pos].to_string());
        }
    }

    None
}
