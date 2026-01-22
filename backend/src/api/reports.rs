/// 新的双层报告系统 API
///
/// 架构：
/// 1. 平台元数据过滤 -> 提取5W关键信息
/// 2. 平台报告生成 -> 基于元数据生成各平台独立报告
/// 3. 全平台报告生成 -> 聚合各平台报告生成综合报告
use axum::{extract::State, http::StatusCode, Extension, Json};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::middleware::auth::Claims;
use crate::models::entities::platform_reports;
use crate::services::analyzer::{AiAnalyzer, AiProvider};
use crate::services::smart_filter::{SmartFilter, SmartFilteredData};
use crate::GLOBAL_DYNAMIC_CONFIG;

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneratePlatformReportsRequest {
    pub platforms: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateAllReportsRequest {
    pub style: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateComprehensiveReportRequest {
    pub platform_reports: Option<Vec<PlatformReport>>,
    pub style: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlatformReport {
    pub platform: String,
    pub metadata: SmartFilteredData,
    pub summary: String,
    pub insights: Vec<String>,
    #[serde(default)]
    pub card_visuals: Value,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CrossPlatformReport {
    pub platform_reports: Vec<PlatformReport>,
    #[serde(rename = "综合分析")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comprehensive_analysis: Option<ComprehensiveAnalysis>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComprehensiveAnalysis {
    // AI自由生成的内容字段 - 使用flatten接受任意字段
    #[serde(flatten)]
    pub content: serde_json::Map<String, Value>,

    // 必需的样式字段（用于前端渲染）
    #[serde(default = "default_theme_color")]
    pub theme_color: String,
    #[serde(default = "default_visual_style")]
    pub visual_style: String,
    #[serde(default = "default_decorative_emojis")]
    pub decorative_emojis: Vec<String>,
    #[serde(default = "default_card_subtitle")]
    pub card_subtitle: String,
    #[serde(default = "default_key_metric")]
    pub key_metric: String,

    // 可选的样式字段
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_elements: Option<Value>,
}

fn default_theme_color() -> String {
    "#4F46E5".to_string()
}
fn default_visual_style() -> String {
    "modern".to_string()
}
fn default_decorative_emojis() -> Vec<String> {
    vec!["📊".to_string(), "🎯".to_string()]
}
fn default_card_subtitle() -> String {
    "综合分析".to_string()
}
fn default_key_metric() -> String {
    "数据洞察".to_string()
}

/// 生成平台报告（第一层）
/// POST /api/reports/platform
pub async fn generate_platform_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<GeneratePlatformReportsRequest>,
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("📊 [ENTRY] generate_platform_reports called");
    tracing::info!("   Platforms: {:?}", req.platforms);
    tracing::info!("   User: {} (ID: {})", claims.username, claims.sub);

    let user_id = claims.sub.parse::<i32>().map_err(|e| {
        tracing::error!("❌ Failed to parse user_id: {}", e);
        StatusCode::UNAUTHORIZED
    })?;

    let platform_reports =
        generate_platform_reports_internal(&db, user_id, req.platforms.clone()).await;

    if platform_reports.is_empty() {
        tracing::warn!(
            "⚠️ No platform reports generated for platforms: {:?}",
            req.platforms
        );
        return Ok(Json(json!({
            "success": false,
            "message": "未能生成报告。请确保已获取平台数据。",
            "reports": [],
            "token_estimate": 0
        })));
    }

    // Token优化：估算每个报告的大小
    let total_tokens: usize = platform_reports
        .iter()
        .map(|r| crate::services::smart_filter::SmartFilter::estimate_token_size(&r.metadata))
        .sum();

    tracing::info!(
        "✅ Generated {} platform reports, estimated tokens: {}",
        platform_reports.len(),
        total_tokens
    );

    Ok(Json(json!({
        "success": true,
        "reports": platform_reports,
        "token_estimate": total_tokens,
    })))
}

/// 内部函数：生成平台报告逻辑（支持并行处理）
async fn generate_platform_reports_internal(
    db: &DatabaseConnection,
    user_id: i32,
    platforms: Vec<String>,
) -> Vec<PlatformReport> {
    use futures::future::join_all;

    // 并行处理所有平台
    let db_clone = db.clone();
    let futures = platforms.into_iter().map(move |platform| {
        let db_for_task = db_clone.clone();
        async move {
            tracing::info!("🔄 Processing platform: {}", platform);

            // 1. 获取平台数据 (自动处理缓存回退，支持数据库分片数据)
            let metadata = match get_platform_data(&platform, &db_for_task, user_id).await {
                Ok(data) => {
                    tracing::info!("✅ Loaded data for {}", platform);
                    data
                }
                Err(e) => {
                    tracing::warn!("⚠️ Skipping {}: {}", platform, e);
                    return None;
                }
            };

            // 3. 基于元数据生成平台报告
            tracing::debug!("🤖 Generating AI report for {}", platform);
            let (summary, ai_insights, mut card_visuals) =
                match generate_ai_report(&metadata, &platform).await {
                    Ok(res) => {
                        tracing::info!("✅ AI report generated for {}", platform);
                        res
                    }
                    Err(e) => {
                        tracing::warn!(
                            "⚠️ Failed to generate AI report for {}: {}, using fallback",
                            platform,
                            e
                        );
                        (
                            format!(
                                "{} 在 {} 平台上活跃",
                                metadata.user_summary.username, metadata.platform
                            ),
                            vec![],
                            json!({}),
                        )
                    }
                };

            // 4. 对于bilibili平台，额外添加资料库内容到card_visuals
            if platform == "bilibili" {
                if let Ok(library_items) = extract_bilibili_library_items(&metadata).await {
                    // 确保 card_visuals 是对象类型
                    if !card_visuals.is_object() {
                        card_visuals = json!({});
                    }
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }
            }

            // 5. 对于steam平台，额外添加资料库内容到card_visuals
            if platform == "steam" {
                if let Ok(library_items) = extract_steam_library_items(&metadata).await {
                    // 确保 card_visuals 是对象类型
                    if !card_visuals.is_object() {
                        card_visuals = json!({});
                    }
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }
            }

            // 6. 对于github平台，强制覆盖关键数据字段（避免AI生成不稳定的值）
            if platform == "github" {
                if let crate::services::smart_filter::ContentAnalysis::GitHub(analysis) =
                    &metadata.content_analysis
                {
                    // 确保 card_visuals 是对象类型
                    if !card_visuals.is_object() {
                        card_visuals = json!({});
                    }
                    if let Some(obj) = card_visuals.as_object_mut() {
                        // ✅ 使用真实数据强制覆盖关键字段
                        let total_contributions = analysis
                            .contribution_calendar
                            .as_ref()
                            .map(|calendar| {
                                let sum: i64 = calendar.iter().map(|day| day.count).sum();
                                sum
                            })
                            .unwrap_or(0);

                        let repos_count = analysis.recent_repos.len();

                        // 根据真实数据计算贡献等级
                        let contribution_level = if total_contributions > 1000 && repos_count > 20 {
                            "传奇开发者"
                        } else if total_contributions > 500 && repos_count > 10 {
                            "资深工程师"
                        } else if total_contributions > 200 || repos_count > 5 {
                            "活跃开发者"
                        } else {
                            "新兴贡献者"
                        };

                        // 强制覆盖这些字段（忽略AI可能生成的值）
                        obj.insert(
                            "total_contributions".to_string(),
                            json!(total_contributions),
                        );
                        obj.insert("repos_count".to_string(), json!(repos_count));
                        obj.insert("contribution_level".to_string(), json!(contribution_level));
                        obj.insert(
                            "contribution_calendar".to_string(),
                            json!(analysis.contribution_calendar),
                        );

                        tracing::info!(
                            "✅ GitHub card_visuals: contributions={}, repos={}, level={}",
                            total_contributions,
                            repos_count,
                            contribution_level
                        );
                    }
                }

                // 添加资料库内容
                if let Ok(library_items) = extract_github_library_items(&metadata).await {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }
            }

            // 7. 对于bilibili平台，额外添加用户统计数据到card_visuals
            if platform == "bilibili" {
                // 确保 card_visuals 是对象类型
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }

                // 从原始platform_data中提取用户统计数据
                if let Ok(stats) = extract_bilibili_user_stats(&metadata).await {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("user_level".to_string(), stats.level);
                        obj.insert("follower_count".to_string(), stats.follower_count);
                        obj.insert("following_count".to_string(), stats.following_count);
                    }
                }
            }

            // 8. 对于netease平台，额外添加资料库内容和用户统计数据到card_visuals
            if platform == "netease" {
                // 确保 card_visuals 是对象类型
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }

                // 添加library_items
                if let Ok(library_items) = extract_netease_library_items(&metadata).await {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }

                // 添加用户统计数据
                if let Ok(stats) = extract_netease_user_stats(&metadata).await {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("follower_count".to_string(), stats.follower_count);
                        obj.insert("playlist_count".to_string(), stats.playlist_count);
                        // level 由 AI 在 card_visuals 中生成，不需要手动插入
                    }
                }
            }

            let mut insights = ai_insights;

            // 如果AI没有生成洞察，使用备用逻辑
            if insights.is_empty() {
                match &metadata.content_analysis {
                    crate::services::smart_filter::ContentAnalysis::Bilibili(analysis) => {
                        insights.push(analysis.video_summary.clone());
                        if !analysis.anime_analysis.is_empty() {
                            let top_genre = analysis.anime_analysis[0]
                                .genres
                                .iter()
                                .max_by_key(|entry| entry.1)
                                .map(|(k, _)| k.as_str())
                                .unwrap_or("未知");
                            insights.push(format!("追番偏好：{}", top_genre));
                        }
                    }
                    crate::services::smart_filter::ContentAnalysis::Steam(analysis) => {
                        insights.push(analysis.game_summary.clone());
                        if !analysis.genre_analysis.is_empty() {
                            insights
                                .push(format!("最爱类型：{}", analysis.genre_analysis[0].genre));
                        }
                    }
                    crate::services::smart_filter::ContentAnalysis::GitHub(analysis) => {
                        insights.push(analysis.repo_summary.clone());
                        if let Some((lang, _)) = analysis
                            .language_distribution
                            .iter()
                            .max_by_key(|(_, v)| *v)
                        {
                            insights.push(format!("主要语言：{}", lang));
                        }
                    }
                    crate::services::smart_filter::ContentAnalysis::Netease(analysis) => {
                        insights.push(analysis.music_summary.clone());
                        if !analysis.artist_analysis.favorite_artists.is_empty() {
                            insights.push(format!(
                                "最爱歌手：{}",
                                analysis.artist_analysis.favorite_artists.join("、")
                            ));
                        }
                    }
                }
            }

            let report = PlatformReport {
                platform: platform.clone(),
                metadata: metadata.clone(),
                summary,
                insights,
                card_visuals: card_visuals.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            Some(report)
        }
    });

    let results = join_all(futures).await;
    let platform_reports: Vec<PlatformReport> = results.into_iter().flatten().collect();

    tracing::info!(
        "🎯 Generated {} platform reports, starting database save...",
        platform_reports.len()
    );

    // 批量保存到数据库（先删除旧报告，再插入新报告）
    // 为了性能，这里还是串行保存，但生成过程是并行的
    for report in &platform_reports {
        tracing::debug!("💾 Serializing report for platform: {}", report.platform);

        let report_json = match serde_json::to_value(report) {
            Ok(json) => json,
            Err(e) => {
                tracing::error!(
                    "❌ Failed to serialize report for {}: {}",
                    report.platform,
                    e
                );
                tracing::error!(
                    "   Report data: summary={}, insights={}, card_visuals={}",
                    report.summary,
                    report.insights.len(),
                    report.card_visuals
                );
                continue; // 跳过这个报告，继续处理其他的
            }
        };

        let metadata_json = match serde_json::to_value(&report.metadata) {
            Ok(json) => json,
            Err(e) => {
                tracing::error!(
                    "❌ Failed to serialize metadata for {}: {}",
                    report.platform,
                    e
                );
                continue;
            }
        };

        // 先删除该用户该平台的所有旧报告
        let delete_result = platform_reports::Entity::delete_many()
            .filter(platform_reports::Column::UserId.eq(user_id))
            .filter(platform_reports::Column::Platform.eq(&report.platform))
            .exec(db)
            .await;

        if let Err(e) = delete_result {
            tracing::warn!(
                "⚠️ Failed to delete old reports for {} (continuing): {}",
                report.platform,
                e
            );
        } else if let Ok(result) = delete_result {
            if result.rows_affected > 0 {
                tracing::info!(
                    "🗑️ Deleted {} old report(s) for platform {}",
                    result.rows_affected,
                    report.platform
                );
            }
        }

        // 插入新报告
        let active_model = platform_reports::ActiveModel {
            user_id: Set(user_id),
            platform: Set(report.platform.clone()),
            metadata: Set(metadata_json),
            report: Set(report_json),
            report_title: Set(None), // 平台报告不需要标题
            created_at: Set(chrono::Utc::now().naive_utc()),
            expires_at: Set((chrono::Utc::now() + chrono::Duration::days(7)).naive_utc()),
            ..Default::default()
        };

        match active_model.insert(db).await {
            Ok(_) => {
                tracing::info!("✅ Saved platform report for {}", report.platform);
            }
            Err(e) => {
                tracing::error!(
                    "❌ Failed to save platform report for {}: {}",
                    report.platform,
                    e
                );
            }
        }
    }

    tracing::info!(
        "✅ Database save completed for {} reports",
        platform_reports.len()
    );
    platform_reports
}

/// 生成全平台综合报告（第二层）
/// POST /api/reports/comprehensive
pub async fn generate_comprehensive_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<GenerateComprehensiveReportRequest>,
) -> Result<Json<Value>, StatusCode> {
    let user_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // 1. 获取平台报告：如果请求中没有，则从数据库获取最新的
    let platform_reports = if let Some(reports) = req.platform_reports {
        reports
    } else {
        // 从数据库获取最新的单平台报告
        let user_reports = platform_reports::Entity::find()
            .filter(platform_reports::Column::UserId.eq(user_id))
            .filter(platform_reports::Column::Platform.ne("all"))
            .order_by_desc(platform_reports::Column::CreatedAt)
            .all(&db)
            .await
            .map_err(|e| {
                tracing::error!("Database error fetching user reports: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        if user_reports.is_empty() {
            tracing::info!("No platform reports found for user {}", user_id);
            return Ok(Json(json!({
                "success": false,
                "message": "请先生成至少一个平台报告",
                "hint": "点击平台卡片生成单平台报告后,再生成综合分析"
            })));
        }

        use std::collections::HashMap;
        let mut latest_map = HashMap::new();

        // 保留每个平台最新的一份报告
        for r in user_reports {
            if let std::collections::hash_map::Entry::Vacant(e) = latest_map.entry(r.platform) {
                // 将数据库中的 JSON 转换回 PlatformReport 结构
                if let Ok(report_struct) = serde_json::from_value::<PlatformReport>(r.report) {
                    e.insert(report_struct);
                }
            }
        }

        latest_map.into_values().collect()
    };

    let style = req.style;

    if platform_reports.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // 2. 聚合所有平台的洞察
    let mut all_interests = Vec::new();
    let mut all_activities = Vec::new();
    let mut platform_summaries = Vec::new();

    for report in &platform_reports {
        platform_summaries.push(format!("{}：{}", report.platform, report.summary));

        // 从 SmartFilteredData 提取兴趣和活动
        match &report.metadata.content_analysis {
            crate::services::smart_filter::ContentAnalysis::Bilibili(analysis) => {
                all_activities.push("观看视频".to_string());
                if !analysis.anime_analysis.is_empty() {
                    all_activities.push("追番".to_string());
                    for anime in &analysis.anime_analysis {
                        for genre in anime.genres.keys() {
                            all_interests.push(genre.clone());
                        }
                    }
                }
            }
            crate::services::smart_filter::ContentAnalysis::Steam(analysis) => {
                all_activities.push("玩游戏".to_string());
                for genre in &analysis.genre_analysis {
                    all_interests.push(genre.genre.clone());
                }
            }
            crate::services::smart_filter::ContentAnalysis::GitHub(analysis) => {
                all_activities.push("写代码".to_string());
                all_activities.push("开源贡献".to_string());
                for lang in analysis.language_distribution.keys() {
                    all_interests.push(lang.clone());
                }
            }
            crate::services::smart_filter::ContentAnalysis::Netease(analysis) => {
                all_activities.push("听音乐".to_string());
                for genre in &analysis.artist_analysis.genre_analysis {
                    all_interests.push(genre.genre.clone());
                }
            }
        }
    }

    // 去重
    all_interests.sort();
    all_interests.dedup();
    all_activities.sort();
    all_activities.dedup();

    // 3. 生成综合分析
    let comprehensive =
        match generate_ai_comprehensive_report(&platform_reports, style.as_deref()).await {
            Ok(analysis) => analysis,
            Err(e) => {
                tracing::warn!("Failed to generate AI comprehensive report: {}", e);
                // 降级处理：使用简单的聚合逻辑
                let mut content = serde_json::Map::new();
                content.insert(
                    "总体画像".to_string(),
                    Value::String(format!(
                        "一个活跃在 {} 个平台的数字游民",
                        platform_reports.len()
                    )),
                );
                content.insert(
                    "跨平台洞察".to_string(),
                    json!(vec![
                        format!("涉及 {} 个不同领域", all_activities.len()),
                        format!(
                            "兴趣广泛，包括：{}",
                            all_interests
                                .iter()
                                .take(5)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("、")
                        ),
                    ]),
                );
                content.insert("兴趣图谱".to_string(), json!(all_interests));
                content.insert("行为模式".to_string(), json!(all_activities));
                content.insert(
                    "建议".to_string(),
                    json!(vec![
                        "继续保持多元化的兴趣爱好",
                        "可以考虑将不同平台的内容进行跨平台整合",
                    ]),
                );

                ComprehensiveAnalysis {
                    content,
                    theme_color: default_theme_color(),
                    visual_style: default_visual_style(),
                    decorative_emojis: default_decorative_emojis(),
                    card_subtitle: default_card_subtitle(),
                    key_metric: default_key_metric(),
                    theme_icon: None,
                    icon_image_url: None,
                    icon_prompt: None,
                    background_elements: None,
                }
            }
        };

    let report = CrossPlatformReport {
        platform_reports,
        comprehensive_analysis: Some(comprehensive),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    // 保存综合报告到数据库 (platform = "all")
    let report_json = serde_json::to_value(&report).unwrap_or(json!({}));

    // 从综合分析中提取 visual_style 作为报告标题
    let report_title = report
        .comprehensive_analysis
        .as_ref()
        .map(|analysis| analysis.visual_style.clone());

    // 综合报告不覆盖，直接插入新的一份
    let active_model = platform_reports::ActiveModel {
        user_id: Set(user_id),
        platform: Set("all".to_string()),
        metadata: Set(json!({})), // 综合报告没有单一的 metadata
        report: Set(report_json),
        report_title: Set(report_title), // 使用 visual_style 作为报告标题
        created_at: Set(chrono::Utc::now().naive_utc()),
        expires_at: Set((chrono::Utc::now() + chrono::Duration::days(30)).naive_utc()), // 综合报告保留30天
        ..Default::default()
    };

    let inserted_model = match active_model.insert(&db).await {
        Ok(model) => model,
        Err(e) => {
            tracing::error!("Failed to save comprehensive report: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    tracing::info!(
        "✓ Saved comprehensive report with ID: {}",
        inserted_model.id
    );

    // 构建包含ID的响应
    let report_with_id = serde_json::to_value(&report).unwrap_or(json!({}));
    let report_with_id = if let Some(mut obj) = report_with_id.as_object().cloned() {
        obj.insert("id".to_string(), json!(inserted_model.id));
        serde_json::Value::Object(obj)
    } else {
        report_with_id
    };

    Ok(Json(json!({
        "success": true,
        "report": report_with_id,
    })))
}

/// 一键生成完整报告（两层）
/// POST /api/reports/generate-all
pub async fn generate_all_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<GenerateAllReportsRequest>,
) -> Result<Json<Value>, StatusCode> {
    let user_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // 1. 获取用户启用的所有平台
    let enabled_platforms = vec![
        "bilibili".to_string(),
        "steam".to_string(),
        "github".to_string(),
        "netease".to_string(),
    ];

    // 2. 生成平台报告 (使用内部函数，避免序列化开销)
    let platform_reports =
        generate_platform_reports_internal(&db, user_id, enabled_platforms).await;

    // 3. 生成综合报告
    let comprehensive_req = GenerateComprehensiveReportRequest {
        platform_reports: Some(platform_reports),
        style: req.style,
    };

    let comprehensive_resp =
        generate_comprehensive_report(State(db), Extension(claims), Json(comprehensive_req))
            .await?;

    Ok(comprehensive_resp)
}

/// 获取最新的平台报告（只包含单平台报告，不包含综合报告）
/// GET /api/reports/latest
/// 支持未认证访问，默认返回管理员（user_id=1）的报告
pub async fn get_latest_report(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    // 尝试从请求头提取用户信息，如果没有则使用默认user_id=1
    let user_id = crate::middleware::auth::extract_optional_claims(&headers)
        .and_then(|claims| claims.sub.parse::<i32>().ok())
        .unwrap_or(1);

    // 获取所有单平台报告，保留每个平台最新的一份
    let user_reports = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .filter(platform_reports::Column::Platform.ne("all"))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|e| {
            tracing::error!("Database error fetching user reports: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    use std::collections::hash_map::Entry;
    use std::collections::HashMap;
    let mut latest_map = HashMap::new();

    // 保留每个平台最新的一份报告
    for r in user_reports {
        if let Entry::Vacant(e) = latest_map.entry(r.platform) {
            e.insert(r.report);
        }
    }

    let platform_reports_list: Vec<Value> = latest_map.into_values().collect();

    // 如果没有任何平台报告
    if platform_reports_list.is_empty() {
        return Ok(Json(json!({
            "success": false,
            "message": "No valid report found",
        })));
    }

    // 只返回平台报告，不包含综合分析
    Ok(Json(json!({
        "success": true,
        "platform_reports": platform_reports_list,
        "created_at": chrono::Utc::now().to_rfc3339()
    })))
}

/// 获取所有综合报告列表
/// GET /api/reports/comprehensive/list
/// 支持未认证访问，默认返回管理员（user_id=1）的报告
pub async fn get_comprehensive_reports_list(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let user_id = crate::middleware::auth::extract_optional_claims(&headers)
        .and_then(|claims| claims.sub.parse::<i32>().ok())
        .unwrap_or(1);

    // 查找所有综合报告（platform="all"）
    let reports = platform_reports::Entity::find()
        .filter(platform_reports::Column::Platform.eq("all"))
        .filter(platform_reports::Column::UserId.eq(user_id))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|e| {
            tracing::error!("Database error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let now = chrono::Utc::now().naive_utc();

    // 只返回未过期的报告的摘要信息
    let report_list: Vec<Value> = reports
        .into_iter()
        .filter(|r| r.expires_at > now)
        .map(|r| {
            json!({
                "id": r.id,
                "report_title": r.report_title,
                "created_at": r.created_at.to_string(),
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "reports": report_list,
    })))
}

/// 删除特定的综合报告
/// DELETE /api/reports/comprehensive/:id
pub async fn delete_comprehensive_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    axum::extract::Path(report_id): axum::extract::Path<i32>,
) -> Result<Json<Value>, StatusCode> {
    let user_id = claims.sub.parse::<i32>().unwrap_or(1);

    // 查找报告并验证所有权
    let report = platform_reports::Entity::find_by_id(report_id)
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("Database error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    match report {
        Some(r) => {
            // 验证是否是综合报告且属于当前用户
            if r.platform != "all" {
                return Ok(Json(json!({
                    "success": false,
                    "message": "只能删除综合报告"
                })));
            }

            if r.user_id != user_id {
                return Ok(Json(json!({
                    "success": false,
                    "message": "无权删除此报告"
                })));
            }

            // 删除报告
            platform_reports::Entity::delete_by_id(report_id)
                .exec(&db)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to delete report: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            tracing::info!("✓ Deleted comprehensive report ID: {}", report_id);

            Ok(Json(json!({
                "success": true,
                "message": "报告已删除"
            })))
        }
        None => Ok(Json(json!({
            "success": false,
            "message": "报告不存在"
        }))),
    }
}

/// 根据ID获取特定的综合报告
/// GET /api/reports/comprehensive/:id
/// 支持未认证访问，默认返回管理员（user_id=1）的报告
pub async fn get_comprehensive_report_by_id(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(report_id): axum::extract::Path<i32>,
) -> Result<Json<Value>, StatusCode> {
    let user_id = crate::middleware::auth::extract_optional_claims(&headers)
        .and_then(|claims| claims.sub.parse::<i32>().ok())
        .unwrap_or(1);

    let report_model = platform_reports::Entity::find_by_id(report_id)
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("Database error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if let Some(model) = report_model {
        // 验证报告所有权
        if model.user_id != user_id {
            return Err(StatusCode::FORBIDDEN);
        }

        // 检查是否过期
        let now = chrono::Utc::now().naive_utc();
        if model.expires_at <= now {
            return Ok(Json(json!({
                "success": false,
                "message": "Report has expired",
            })));
        }

        return Ok(Json(json!({
            "success": true,
            "report": model.report,
            "created_at": model.created_at.to_string(),
        })));
    }

    Ok(Json(json!({
        "success": false,
        "message": "Report not found",
    })))
}

/// 辅助函数：从缓存获取平台数据
/// 辅助函数：从缓存获取平台数据（重构版 - 单平台处理，支持数据库分片数据）
async fn get_platform_data(
    platform: &str,
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<SmartFilteredData, String> {
    use once_cell::sync::Lazy;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // 每个平台独立的处理锁
    type PlatformLocks = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;
    static PLATFORM_LOCKS: Lazy<PlatformLocks> = Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

    // 1. 尝试从独立平台缓存加载
    if let Ok(cached_data) = SmartFilter::load_platform_cache(platform) {
        tracing::debug!("✓ Loaded {} from platform cache", platform);
        return Ok(cached_data);
    }

    tracing::info!("Platform cache miss for {}, processing...", platform);

    // 2. 获取平台特定的锁
    let lock = {
        let mut locks = PLATFORM_LOCKS.lock().await;
        locks
            .entry(platform.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };

    let _guard = lock.lock().await;

    // 3. 再次检查缓存（可能被其他请求已处理）
    if let Ok(cached_data) = SmartFilter::load_platform_cache(platform) {
        tracing::info!("✓ {} processed by concurrent request", platform);
        return Ok(cached_data);
    }

    // 4. 🚀 NEW: 尝试从数据库读取数据（支持分片数据）
    let batch_saver = crate::services::batch_saver::BatchSaver::new(db.clone());

    if let Ok(Some(platform_data)) = batch_saver.load_chunked_metadata(user_id, platform).await {
        tracing::info!(
            "✓ Loaded {} from database (with chunked data support)",
            platform
        );

        // 处理并缓存该平台数据
        let filtered_data = SmartFilter::process_and_save_single(platform, &platform_data)
            .map_err(|e| format!("Failed to process {}: {}", platform, e))?;

        tracing::info!(
            "✓ Successfully processed and cached {} from database",
            platform
        );
        return Ok(filtered_data);
    }

    // 5. FALLBACK: 从平台特定的raw文件读取数据
    let raw_cache_path = PathBuf::from(format!("./cache/raw/{}.json", platform));
    if !raw_cache_path.exists() {
        return Err(format!("Raw data file not found: {:?}", raw_cache_path));
    }

    tracing::info!("⚙️  Processing {} from raw data...", platform);

    let content = fs::read_to_string(&raw_cache_path).map_err(|e| e.to_string())?;
    let platform_data: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // 6. 处理并缓存该平台数据（只处理单个平台！）
    let filtered_data = SmartFilter::process_and_save_single(platform, &platform_data)
        .map_err(|e| format!("Failed to process {}: {}", platform, e))?;

    tracing::info!("✓ Successfully processed and cached {}", platform);
    Ok(filtered_data)
}

/// 辅助函数：调用AI生成报告
#[allow(dead_code)]
async fn generate_ai_report(
    metadata: &SmartFilteredData,
    platform: &str,
) -> Result<(String, Vec<String>, Value), String> {
    // 1. 获取配置
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;

    // 2. 确定 Provider 和 Key
    let (provider, api_key, model, base_url) = match config.ai_provider.as_str() {
        "openai" => (
            AiProvider::OpenAI,
            config.openai_api_key.clone(),
            config.openai_model.clone(),
            Some(config.openai_base_url.clone()),
        ),
        _ => (
            AiProvider::Gemini,
            config.gemini_api_key.clone(),
            config.gemini_model.clone(),
            None,
        ),
    };

    // 3. 检查 API Key 是否存在
    if api_key.is_none() || api_key.as_ref().unwrap().is_empty() {
        tracing::warn!(
            "AI API key not configured for provider: {}. Using mock report.",
            config.ai_provider
        );
        return generate_mock_report(metadata, platform);
    }

    // 4. 初始化 Analyzer
    let analyzer = AiAnalyzer::new(provider, api_key.unwrap(), model, base_url).await;

    // 5. 构建 Prompt
    let (system_prompt, tone_desc, visual_req) = match platform {
        "bilibili" => (
            "你是一个资深二次元评论家，说话幽默风趣，懂各种B站梗。",
            "用B站用户的口吻，带有二次元浓度，分析用户的追番、看视频习惯。",
            "card_visuals必须包含 'danmaku' 字段（字符串数组，5-8条简短有趣的弹幕风格评价）。"
        ),
        "steam" => (
            "你是一个硬核游戏玩家，看重'肝度'、'全成就'和'喜加一'。",
            "用硬核玩家的口吻，分析用户的游戏品味、游玩时长和'剁手'习惯。",
            "card_visuals必须包含 'player_type' (字符串), 'hardcore_score' (0-100数字), 'games_count' (数字，游戏总数量), 'total_playtime' (数字，总游戏时长小时数)。"
        ),
        "github" => (
            "你是一个极客技术大佬，崇尚开源精神，说话严谨但带有技术幽默。",
            "用技术大佬的口吻，评估用户的代码贡献、技术栈深度和开源影响力。",
            "card_visuals必须包含 'contribution_level' (字符串，如'传奇开发者'、'资深工程师'、'活跃开发者'), 'languages' (对象数组 {name, percentage})。注意：不要生成 'total_contributions'、'repos_count' 和 'contribution_calendar' 字段，这些将由系统自动计算。"
        ),
        "netease" => (
            "你是一个文艺青年/乐评人，感性细腻，喜欢用歌词或诗意的语言表达。",
            "用文艺感性的口吻，解读用户的听歌品味、情感倾向和深夜听歌习惯。",
            "card_visuals必须包含 'soul_color' (十六进制颜色), 'mood_keywords' (对象数组，每个对象包含 'tag' 和 'color' 字段，例如 [{\"tag\": \"感性\", \"color\": \"#7B68EE\"}, {\"tag\": \"深夜\", \"color\": \"#FF6B9D\"}])，'level' (数字1-10，根据用户的歌曲数量、歌单数量、听歌品味的广度和深度综合评估，越资深等级越高)。根据每个标签的情感色彩选择合适的颜色。"
        ),
        _ => (
            "你是一个专业的数据分析师，客观理性。",
            "用专业客观的口吻分析用户数据。",
            "card_visuals可以是空对象。"
        ),
    };

    let data_str = serde_json::to_string_pretty(metadata).map_err(|e| e.to_string())?;
    let full_prompt = format!(
        "System: {}\nTask: {}\nRequirement: Return ONLY a valid JSON object (no markdown, no code blocks) with the following structure:\n{{\n  \"summary\": \"一段简短的总结(50字以内)\",\n  \"insights\": [\"3-5条详细的洞察分析\"],\n  \"card_visuals\": {{ ...根据以下要求生成: {} }}\n}}\n\nData:\n{}",
        system_prompt, tone_desc, visual_req, data_str
    );

    // 6. 调用 AI
    let input_data = json!({
        "prompt": full_prompt
    });

    match analyzer.analyze_profile(&input_data).await {
        Ok(response) => {
            tracing::info!(
                "Generated AI report for {}: {} chars",
                platform,
                response.len()
            );

            // 尝试解析 JSON
            // 清理可能的 markdown 标记
            let clean_json = response
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();

            #[derive(Deserialize)]
            struct AiResponse {
                summary: String,
                insights: Vec<String>,
                card_visuals: Value,
            }

            match serde_json::from_str::<AiResponse>(clean_json) {
                Ok(res) => Ok((res.summary, res.insights, res.card_visuals)),
                Err(e) => {
                    tracing::error!("Failed to parse AI JSON response: {}. Raw: {}", e, response);
                    // 降级处理：把整个回复当做 summary
                    Ok((response, vec![], json!({})))
                }
            }
        }
        Err(e) => {
            tracing::error!("AI generation failed: {}", e);
            generate_mock_report(metadata, platform)
        }
    }
}

fn generate_mock_report(
    metadata: &SmartFilteredData,
    _platform: &str,
) -> Result<(String, Vec<String>, Value), String> {
    // 模拟AI生成的回复
    let (summary, insights, visuals) = match &metadata.content_analysis {
        crate::services::smart_filter::ContentAnalysis::Bilibili(analysis) => {
            let genre_str = if !analysis.anime_analysis.is_empty() {
                analysis.anime_analysis[0]
                    .genres
                    .keys()
                    .next()
                    .map(|s| s.as_str())
                    .unwrap_or("未知")
            } else {
                "涉猎广泛"
            };
            (
                format!(
                    "{}这位更是重量级！{}的大佬。",
                    metadata.user_summary.username,
                    metadata.user_summary.level.as_deref().unwrap_or("?")
                ),
                vec![
                    format!("观看偏好：{}", analysis.video_summary),
                    format!("追番口味：{}", genre_str),
                    "下次一定：经常忘记投币".to_string(),
                ],
                json!({
                    "danmaku": ["高能预警", "下次一定", "火钳刘明", "AWSL", "泪目", "爷青回"]
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::Steam(analysis) => {
            let games_count = analysis
                .genre_analysis
                .iter()
                .map(|g| g.examples.len())
                .sum::<usize>();
            // 计算总游戏时长（从recent_games的playtime字段累加，单位：分钟，转换为小时）
            let total_playtime_minutes: i64 =
                analysis.recent_games.iter().map(|g| g.playtime).sum();
            let total_playtime_hours = (total_playtime_minutes / 60) as u64;

            (
                format!(
                    "检测到高能玩家反应！{}，{}。",
                    metadata.user_summary.username, analysis.game_summary
                ),
                vec![
                    format!(
                        "最爱类型：{}",
                        if !analysis.genre_analysis.is_empty() {
                            &analysis.genre_analysis[0].genre
                        } else {
                            "多"
                        }
                    ),
                    format!(
                        "最近在玩：{}",
                        analysis
                            .recent_games
                            .first()
                            .map(|g| g.name.as_str())
                            .unwrap_or("游戏")
                    ),
                    "G胖的微笑由你守护".to_string(),
                ],
                json!({
                    "player_type": "硬核玩家",
                    "hardcore_score": 85,
                    "games_count": games_count,
                    "total_playtime": total_playtime_hours,
                    "top_genres": analysis.genre_analysis.iter().take(3).map(|g| &g.genre).collect::<Vec<_>>()
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::GitHub(analysis) => {
            // ✅ 使用完整的贡献日历数据计算总提交数（365天的真实数据）
            let total_contributions = analysis
                .contribution_calendar
                .as_ref()
                .map(|calendar| {
                    let sum: i64 = calendar.iter().map(|day| day.count).sum();
                    tracing::info!(
                        "📊 GitHub total contributions: {} from {} days",
                        sum,
                        calendar.len()
                    );
                    sum
                })
                .unwrap_or(0);

            // 根据真实贡献数和仓库数量确定贡献等级
            let contribution_level =
                if total_contributions > 1000 && analysis.recent_repos.len() > 20 {
                    "传奇开发者"
                } else if total_contributions > 500 && analysis.recent_repos.len() > 10 {
                    "资深工程师"
                } else if total_contributions > 200 || analysis.recent_repos.len() > 5 {
                    "活跃开发者"
                } else {
                    "新兴贡献者"
                };

            // 计算语言百分比
            let total_lang_count: usize = analysis.language_distribution.values().sum();
            let languages = if total_lang_count > 0 {
                analysis
                    .language_distribution
                    .iter()
                    .take(3)
                    .map(|(k, v)| {
                        let percentage =
                            (*v as f64 / total_lang_count as f64 * 100.0).round() as i32;
                        json!({"name": k, "percentage": percentage})
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![]
            };

            (
                format!(
                    "Scanning profile... Target: {}。Talk is cheap, show me the code。",
                    metadata.user_summary.username
                ),
                vec![
                    format!(
                        "主要语言：{}",
                        analysis
                            .language_distribution
                            .keys()
                            .next()
                            .map(|s| s.as_str())
                            .unwrap_or("Unknown")
                    ),
                    format!("仓库概况：{}", analysis.repo_summary),
                    format!("贡献等级：{}", contribution_level),
                ],
                json!({
                    "contribution_level": contribution_level,
                    "total_contributions": total_contributions,
                    "repos_count": analysis.recent_repos.len(),
                    "languages": languages,
                    "contribution_calendar": analysis.contribution_calendar
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::Netease(analysis) => {
            // 根据歌曲数量估算等级（1-10）
            let song_count = analysis.recent_songs.len();
            let artist_count = analysis.artist_analysis.favorite_artists.len();
            let genre_diversity = analysis.artist_analysis.genre_analysis.len();

            // 综合评分：歌曲数量 + 艺术家多样性 + 风格多样性
            let level = ((song_count / 50).min(4)
                + (artist_count / 10).min(3)
                + (genre_diversity / 2).min(3))
            .clamp(1, 10);

            (
                format!(
                    "夜深了，{}。愿音乐永远是你的避风港。",
                    metadata.user_summary.username
                ),
                vec![
                    format!("听歌品味：{}", analysis.music_summary),
                    format!(
                        "最爱风格：{}",
                        if !analysis.artist_analysis.genre_analysis.is_empty() {
                            &analysis.artist_analysis.genre_analysis[0].genre
                        } else {
                            "流行"
                        }
                    ),
                    "深夜emo时刻".to_string(),
                ],
                json!({
                    "soul_color": "#7B68EE",
                    "mood_keywords": [
                        {"tag": "感性", "color": "#7B68EE"},
                        {"tag": "深夜", "color": "#FF6B9D"},
                        {"tag": "治愈", "color": "#4ECDC4"},
                        {"tag": "怀旧", "color": "#FFB347"}
                    ],
                    "level": level
                }),
            )
        }
    };

    Ok((summary, insights, visuals))
}

/// 生成图标图片URL（使用Pollinations AI）
fn generate_icon_image_url(prompt: &str) -> String {
    use urlencoding::encode;

    let encoded_prompt = encode(prompt);
    // 图标尺寸：512x512，使用flux模型，无logo，增强效果
    let seed = chrono::Utc::now().timestamp() % 100000;

    format!(
        "https://image.pollinations.ai/prompt/{}?width=512&height=512&model=flux&nologo=true&enhance=true&seed={}",
        encoded_prompt, seed
    )
}

/// 辅助函数：调用AI生成综合报告
async fn generate_ai_comprehensive_report(
    reports: &[PlatformReport],
    style: Option<&str>,
) -> Result<ComprehensiveAnalysis, String> {
    // 1. 获取配置
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;

    // 2. 确定 Provider 和 Key
    let (provider, api_key, model, base_url) = match config.ai_provider.as_str() {
        "openai" => (
            AiProvider::OpenAI,
            config.openai_api_key.clone(),
            config.openai_model.clone(),
            Some(config.openai_base_url.clone()),
        ),
        "gemini" => (
            AiProvider::Gemini,
            config.gemini_api_key.clone(),
            config.gemini_model.clone(),
            None,
        ),
        _ => (
            AiProvider::Gemini,
            config.gemini_api_key.clone(),
            config.gemini_model.clone(),
            None,
        ),
    };

    // 3. 检查 API Key 是否存在
    if api_key.is_none() || api_key.as_ref().unwrap().is_empty() {
        return Err("AI API key not configured".to_string());
    }

    // 4. 初始化 Analyzer
    let analyzer = AiAnalyzer::new(provider, api_key.unwrap(), model, base_url).await;

    // 5. 构建 Prompt
    // 提取每个平台的摘要和洞察，减少 Token 消耗
    let summaries: Vec<Value> = reports
        .iter()
        .map(|r| {
            json!({
                "platform": r.platform,
                "summary": r.summary,
                "insights": r.insights
            })
        })
        .collect();

    let style_instruction = if let Some(s) = style {
        format!(
            "用户要求的风格: '{}'。你必须严格按照这个风格进行分析、表达和视觉设计。",
            s
        )
    } else {
        "用户未指定风格，请根据数据自行创造独特的风格。".to_string()
    };

    let emoji_instruction = if let Some(s) = style {
        format!(
            "decorative_emojis 必须直接反映用户的风格描述 '{}'。\n\
            例如：\n\
            - 如果是'赛博朋克'→使用 🤖💾⚡🌃🔮\n\
            - 如果是'诗意'→使用 📖🌸🍃✨🎭\n\
            - 如果是'游戏主播'→使用 🎮🎬🔥👾🏆\n\
            - 如果是'极客'→使用 💻🔧⚙️🚀🧠\n\
            不要使用通用emoji如❤️🌟👍，必须具体且与风格强相关。",
            s
        )
    } else {
        "decorative_emojis 必须反映用户的核心兴趣和平台行为特征，要具体不要泛用。".to_string()
    };

    let data_str = serde_json::to_string_pretty(&summaries).map_err(|e| e.to_string())?;
    let full_prompt = format!(
        "System: 你是一个富有创造力的数字艺术家和心理分析师。\n\n\
        {}\n\n\
        Task: 深入分析用户数据，用你认为最合适的方式和结构呈现洞察。不要被固定框架限制，自由创作内容。\n\n\
        CRITICAL: 返回纯JSON对象（不要```，不要markdown）\n\n\
        JSON结构完全由你决定，但必须包含以下样式字段用于视觉渲染：\n\
        {{\n\
          // 你自由创作的内容字段（字段名、数量、结构完全自定义）\n\
          // ⚠️ CRITICAL: 内容字段只能是【字符串】或【字符串数组】，禁止嵌套对象！\n\
          // ✅ 正确示例: \"开篇语\": \"这是一段文字\", \"核心洞察\": [\"洞察1\", \"洞察2\"]\n\
          // ❌ 错误示例: \"章节\": {{\"标题\": \"...\", \"内容\": \"...\"}} ← 禁止这样的嵌套对象\n\
          \n\
          // 必需的样式字段（用于前端渲染）：\n\
          \"theme_color\": \"十六进制颜色，必须精确匹配'{}'风格的典型色彩\",\n\
          \"visual_style\": \"风格描述（限制15字以内），要独特且准确\",\n\
          \"decorative_emojis\": [\"{}的emoji（限制3-6个），每个必须精准匹配风格\"],\n\
          \"card_subtitle\": \"副标题文本（限制30字以内）\",\n\
          \"key_metric\": \"核心指标文本（限制20字以内）\",\n\
          \n\
          // 图标字段（推荐使用icon_prompt自动生成）：\n\
          \"icon_prompt\": \"**推荐**：提供专业的英文绘画提示词(prompt)用于AI图标生成。\n\
CRITICAL: 必须是图标(icon)设计，不是完整插画或场景！\n\
必须使用英文，遵循以下结构：\n\
1. 主体元素（简洁，1-2个核心物体）\n\
2. 风格关键词\n\
3. 颜色方案\n\
4. 必须包含：'icon design', 'simple', 'minimalist', 'flat design' 或 'logo style'\n\
5. **必须包含**：'transparent background' 或 'no background'（必须透明背景，不要白色或其他颜色背景！）\n\
6. 质量词：'clean', 'vector art', 'high quality', 'sharp edges'\n\n\
正确示例：\n\
- 'elegant scales of justice with quill pen, Fontaine baroque style, water blue and gold colors, icon design, minimalist, transparent background, clean vector art, centered composition'\n\
- 'robot head with circuit pattern, cyberpunk style, neon purple and blue glow, icon design, flat design, no background, simple geometric shapes, high quality'\n\
- 'musical note with heartbeat wave, modern style, gradient pink to red, logo style icon, transparent background, minimalist vector art, sharp edges'\n\n\
错误示例（不要生成）：\n\
- 'detailed landscape with mountains and sky'（太复杂，不是图标）\n\
- 'realistic portrait of a person'（太写实，不是图标风格）\n\
- 'complex scene with multiple characters'（场景，不是图标）\",\n\
          \"icon_image_url\": \"如果有特定的图标URL，可以直接提供（会覆盖icon_prompt）\",\n\
          \"theme_icon\": \"备选方案：从 FaRobot,FaBrain,FaHeart,FaMusic,FaCode,FaGamepad,FaPalette,FaRocket 中选择\",\n\
          \n\
          \"background_elements\": [\n\
            {{\"type\": \"circle/rect\", \"className\": \"Tailwind classes\", \"style\": {{\"top/left等\": \"值\", \"background\": \"rgba\", \"filter\": \"blur\"}}, \"animate\": {{\"x/y/scale/rotate\": [数组]}}, \"transition\": {{\"duration\": 数字, \"repeat\": \"Infinity\", \"ease\": \"easeInOut\"}}}}\n\
          ]\n\
        }}\n\n\
        关键原则：\n\
        1. **扁平化结构**: 内容字段必须是字符串或字符串数组，禁止嵌套对象！用有意义的字段名代替层级结构\n\
        2. **内容自由**: 除了样式字段，所有内容的字段名、数量完全由你决定\n\
        3. **样式精准**: theme_color和decorative_emojis必须100%匹配'{}'这个风格\n\
        4. **风格优先**: 所有内容都要用'{}'风格的语言和视角表达\n\
        5. **拒绝通用**: 不要用\"多元化\"\"全面\"等空洞词汇\n\
        6. **必须具体化**: 禁止空洞描述！每个分析必须包含：\n\
           - 具体的数据实例（歌曲名、游戏名、项目名等）\n\
           - 实际的行为模式（如\"最常听YOASOBI的《アイドル》\"而非\"喜欢日系音乐\"）\n\
           - 真实的时间/数量统计（如\"136位追随者\"\"3765星项目\"）\n\
           - 平台具体内容引用（B站收藏的动漫、GitHub的技术栈、Steam的游戏类型）\n\
        7. **引用原始数据**: 从Platform Data中提取真实信息，不要编造或泛泛而谈\n\n\
        结构示例 - \"原神风格\"（扁平化，无嵌套对象）:\n\
        {{\n\
          \"theme_color\": \"#4A90E2\",\n\
          \"visual_style\": \"提瓦特叙事绘卷\",\n\
          \"decorative_emojis\": [\"✨\", \"🌌\", \"🗺️\", \"📜\", \"💎\"],\n\
          \"card_subtitle\": \"游历四方，心系万象的旅者\",\n\
          \"key_metric\": \"元素共鸣度：深邃\",\n\
          \"icon_prompt\": \"...\",\n\
          \"开篇语\": \"敬爱的旅者，我是提瓦特的星空观测者...\",\n\
          \"核心洞察\": \"你是一位对'叙事'与'情感'有着极度渴望的探索者...\",\n\
          \"数字足迹\": [\"平台1的观察\", \"平台2的发现\", \"平台3的解析\"],\n\
          \"总结寄语\": \"愿星辰指引你的路途...\"\n\
        }}\n\
        ⚠️ 注意：以上示例中所有内容字段都是字符串或字符串数组，没有嵌套对象！\n\n\
        Platform Data:\n{}",
        style_instruction,
        style.unwrap_or("用户风格"),
        emoji_instruction,
        style.unwrap_or("用户风格"),
        style.unwrap_or("用户风格"),
        data_str
    );

    // 6. 调用 AI
    let input_data = json!({
        "prompt": full_prompt
    });

    match analyzer.analyze_profile(&input_data).await {
        Ok(response) => {
            tracing::info!(
                "Generated AI comprehensive report: {} chars",
                response.len()
            );

            // 清理可能的 markdown 标记
            let clean_json = response
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();

            match serde_json::from_str::<ComprehensiveAnalysis>(clean_json) {
                Ok(mut res) => {
                    // 调试：打印解析后的所有字段
                    tracing::info!(
                        "🔍 Parsed content keys: {:?}",
                        res.content.keys().collect::<Vec<_>>()
                    );
                    tracing::info!("🎨 Initial style fields - color: {}, style: {}, emojis: {:?}, subtitle: {}, metric: {}",
                        res.theme_color, res.visual_style, res.decorative_emojis, res.card_subtitle, res.key_metric);

                    // 检查是否使用了默认值（说明AI没有正确返回样式字段）
                    if res.theme_color == default_theme_color() {
                        tracing::warn!(
                            "⚠️ Using default theme_color - AI may not have provided it"
                        );
                    }
                    if res.visual_style == default_visual_style() {
                        tracing::warn!(
                            "⚠️ Using default visual_style - AI may not have provided it"
                        );
                    }

                    // 从content Map中提取样式字段到结构体字段
                    if let Some(Value::String(icon_url)) = res.content.remove("icon_image_url") {
                        tracing::info!("📌 Extracted icon_image_url: {}", icon_url);
                        res.icon_image_url = Some(icon_url);
                    }
                    if let Some(Value::String(icon_p)) = res.content.remove("icon_prompt") {
                        tracing::info!("📌 Extracted icon_prompt: {}", icon_p);
                        res.icon_prompt = Some(icon_p);
                    }
                    if let Some(Value::String(icon)) = res.content.remove("theme_icon") {
                        tracing::info!("📌 Extracted theme_icon: {}", icon);
                        res.theme_icon = Some(icon);
                    }
                    if let Some(bg_elements) = res.content.remove("background_elements") {
                        tracing::info!("📌 Extracted background_elements");
                        res.background_elements = Some(bg_elements);
                    }

                    // 调试：打印最终结构体的图标字段
                    tracing::info!(
                        "📦 Final icon fields - image_url: {:?}, prompt: {:?}, theme: {:?}",
                        res.icon_image_url,
                        res.icon_prompt,
                        res.theme_icon
                    );

                    // 如果有icon_prompt但没有icon_image_url，自动生成图标
                    if res.icon_image_url.is_none() && res.icon_prompt.is_some() {
                        if let Some(prompt) = &res.icon_prompt {
                            tracing::info!("🎨 Generating icon image from prompt: {}", prompt);
                            let icon_url = generate_icon_image_url(prompt);
                            tracing::info!("✅ Generated icon URL: {}", icon_url);
                            res.icon_image_url = Some(icon_url);
                        }
                    }

                    Ok(res)
                }
                Err(e) => {
                    tracing::error!("Failed to parse AI JSON response: {}. Raw: {}", e, response);
                    Err(format!("Failed to parse AI response: {}", e))
                }
            }
        }
        Err(e) => Err(format!("AI generation failed: {}", e)),
    }
}

/// 从bilibili平台数据中提取资料库内容（基于报告中提到的作品）
async fn extract_bilibili_library_items(
    metadata: &SmartFilteredData,
) -> Result<Vec<Value>, String> {
    use std::fs;
    use std::path::PathBuf;

    let mut library_items = Vec::new();

    if let crate::services::smart_filter::ContentAnalysis::Bilibili(analysis) =
        &metadata.content_analysis
    {
        println!("✅ Bilibili analysis found!");

        // 读取B站原始数据以获取封面信息
        let raw_cache_path = PathBuf::from("./cache/raw/bilibili.json");
        let mut bangumi_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        if raw_cache_path.exists() {
            if let Ok(content) = fs::read_to_string(&raw_cache_path) {
                if let Ok(raw_json) = serde_json::from_str::<Value>(&content) {
                    // 提取 bangumi 数据构建标题->封面映射
                    if let Some(bangumi_array) = raw_json.get("bangumi").and_then(|v| v.as_array())
                    {
                        for item in bangumi_array {
                            if let (Some(title), Some(cover)) = (
                                item.get("title").and_then(|v| v.as_str()),
                                item.get("cover").and_then(|v| v.as_str()),
                            ) {
                                bangumi_map.insert(title.to_string(), cover.to_string());
                            }
                        }
                        println!(
                            "  - Loaded {} bangumi covers from raw data",
                            bangumi_map.len()
                        );
                    }
                }
            }
        }

        // 从anime_analysis中提取examples（代表性作品）
        for anime_category in &analysis.anime_analysis {
            for example_title in &anime_category.examples {
                let cover = bangumi_map
                    .get(example_title.as_str())
                    .map(|url| {
                        let proxy_url = crate::api::profile::proxy_image_url(url);
                        println!("  - Anime '{}': {} -> {}", example_title, url, proxy_url);
                        proxy_url
                    })
                    .unwrap_or_else(|| {
                        println!("  - Anime '{}': No cover found", example_title);
                        String::new()
                    });

                library_items.push(json!({
                    "title": example_title,
                    "cover": cover,
                    "type": "anime"
                }));

                if library_items.len() >= 10 {
                    break;
                }
            }
            if library_items.len() >= 10 {
                break;
            }
        }

        // 从原始数据中提取视频封面信息（从收藏夹中读取）
        let mut video_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        if raw_cache_path.exists() {
            if let Ok(content) = fs::read_to_string(&raw_cache_path) {
                if let Ok(raw_json) = serde_json::from_str::<Value>(&content) {
                    // 从收藏夹中提取视频信息
                    if let Some(favorites) = raw_json.get("favorites").and_then(|f| f.as_array()) {
                        println!("  - Found {} favorite folders", favorites.len());
                        for fav_folder in favorites {
                            if let Some(videos) =
                                fav_folder.get("videos").and_then(|v| v.as_array())
                            {
                                for video in videos {
                                    if let (Some(title), Some(cover)) = (
                                        video.get("title").and_then(|t| t.as_str()),
                                        video.get("cover").and_then(|c| c.as_str()),
                                    ) {
                                        video_map.insert(title.to_string(), cover.to_string());
                                    }
                                }
                            }
                        }
                        println!("  - Loaded {} video covers from favorites", video_map.len());
                    } else {
                        println!("  - ⚠️  No favorites found");
                    }
                }
            }
        }

        // 从recent_videos中提取视频信息
        println!(
            "  - Searching covers for {} videos",
            analysis.recent_videos.len()
        );
        for video in &analysis.recent_videos {
            if library_items.len() >= 15 {
                break;
            }

            println!("  - Looking for video: '{}'", video.title);
            let cover = video_map
                .get(&video.title)
                .map(|url| {
                    let proxy_url = crate::api::profile::proxy_image_url(url);
                    println!("    ✓ Found cover: {} -> {}", url, proxy_url);
                    proxy_url
                })
                .unwrap_or_else(|| {
                    println!("    ✗ No cover found in video_map");
                    // 尝试模糊匹配
                    for (map_title, _) in video_map.iter().take(3) {
                        println!("      Available: '{}'", map_title);
                    }
                    String::new()
                });

            library_items.push(json!({
                "title": &video.title,
                "cover": cover,
                "type": "video"
            }));
        }

        // 去重
        let mut seen_titles = std::collections::HashSet::new();
        library_items.retain(|item: &Value| {
            if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
                seen_titles.insert(title.to_string())
            } else {
                false
            }
        });

        // 限制最多返回10个
        library_items.truncate(10);

        println!("  - Final library_items count: {}", library_items.len());
    } else {
        println!("❌ Not Bilibili analysis!");
    }

    Ok(library_items)
}

async fn extract_steam_library_items(metadata: &SmartFilteredData) -> Result<Vec<Value>, String> {
    use std::fs;
    use std::path::PathBuf;

    let mut library_items = Vec::new();

    if let crate::services::smart_filter::ContentAnalysis::Steam(analysis) =
        &metadata.content_analysis
    {
        println!("✅ Steam analysis found!");

        // 读取Steam原始数据以获取游戏封面信息
        let raw_cache_path = PathBuf::from("./cache/raw/steam.json");
        let mut game_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        if raw_cache_path.exists() {
            if let Ok(content) = fs::read_to_string(&raw_cache_path) {
                if let Ok(raw_json) = serde_json::from_str::<Value>(&content) {
                    // 提取 games 数据构建名称->封面映射
                    if let Some(games_array) = raw_json.get("games").and_then(|v| v.as_array()) {
                        for item in games_array {
                            if let (Some(name), Some(appid)) = (
                                item.get("name").and_then(|v| v.as_str()),
                                item.get("appid").and_then(|v| v.as_u64()),
                            ) {
                                // Steam游戏封面URL格式
                                let cover = format!(
                                    "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg",
                                    appid
                                );
                                game_map.insert(name.to_string(), cover);
                            }
                        }
                        println!("  - Loaded {} game covers from raw data", game_map.len());
                    }
                }
            }
        }

        // 从genre_analysis中提取代表性游戏
        for genre_category in &analysis.genre_analysis {
            for game in &genre_category.examples {
                let cover = game_map.get(game.as_str()).cloned().unwrap_or_default();

                library_items.push(json!({
                    "title": game,
                    "cover": cover,
                    "type": "game"
                }));

                if library_items.len() >= 10 {
                    break;
                }
            }
            if library_items.len() >= 10 {
                break;
            }
        }

        // 从recent_games中提取游戏信息
        for game_item in &analysis.recent_games {
            if library_items.len() >= 15 {
                break;
            }
            let cover = game_map
                .get(game_item.name.as_str())
                .cloned()
                .unwrap_or_default();

            library_items.push(json!({
                "title": &game_item.name,
                "cover": cover,
                "type": "game"
            }));
        }

        // 去重
        let mut seen_titles = std::collections::HashSet::new();
        library_items.retain(|item: &Value| {
            if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
                seen_titles.insert(title.to_string())
            } else {
                false
            }
        });

        // 限制最多返回10个
        library_items.truncate(10);

        println!(
            "  - Final steam library_items count: {}",
            library_items.len()
        );
    } else {
        println!("❌ Not Steam analysis!");
    }

    Ok(library_items)
}

async fn extract_github_library_items(metadata: &SmartFilteredData) -> Result<Vec<Value>, String> {
    let mut library_items = Vec::new();

    if let crate::services::smart_filter::ContentAnalysis::GitHub(analysis) =
        &metadata.content_analysis
    {
        println!("✅ GitHub analysis found!");

        // 从recent_repos中提取仓库信息
        for repo_item in analysis.recent_repos.iter().take(10) {
            library_items.push(json!({
                "title": &repo_item.name,
                "language": repo_item.language.as_ref().unwrap_or(&"Unknown".to_string()),
                "type": "repo",
                "stars": repo_item.stars.unwrap_or(0),
                "forks": repo_item.forks.unwrap_or(0),
                "description": repo_item.description.as_ref().unwrap_or(&String::new())
            }));
        }

        println!(
            "  - Final github library_items count: {}",
            library_items.len()
        );
    } else {
        println!("❌ Not GitHub analysis!");
    }

    Ok(library_items)
}

async fn extract_netease_library_items(metadata: &SmartFilteredData) -> Result<Vec<Value>, String> {
    use std::fs;
    use std::path::PathBuf;

    let mut library_items = Vec::new();

    if let crate::services::smart_filter::ContentAnalysis::Netease(analysis) =
        &metadata.content_analysis
    {
        println!("✅ Netease analysis found!");

        // 读取原始网易云缓存文件以获取歌曲封面信息
        let raw_cache_path = PathBuf::from("./cache/raw/netease.json");
        let mut song_map: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();

        if raw_cache_path.exists() {
            if let Ok(content) = fs::read_to_string(&raw_cache_path) {
                if let Ok(raw_json) = serde_json::from_str::<Value>(&content) {
                    // 从 liked_songs 字段提取歌曲数据
                    let songs_array = raw_json.get("liked_songs").and_then(|v| v.as_array());

                    if let Some(songs_array) = songs_array {
                        for item in songs_array {
                            if let (Some(name), cover, artists) = (
                                item.get("name").and_then(|v| v.as_str()),
                                item.get("al")
                                    .and_then(|al| al.get("picUrl"))
                                    .and_then(|v| v.as_str())
                                    .or_else(|| item.get("picUrl").and_then(|v| v.as_str()))
                                    .unwrap_or(""),
                                item.get("ar")
                                    .and_then(|v| v.as_array())
                                    .and_then(|arr| {
                                        arr.first()
                                            .and_then(|a| a.get("name"))
                                            .and_then(|n| n.as_str())
                                    })
                                    .unwrap_or("未知艺术家"),
                            ) {
                                song_map.insert(
                                    name.to_string(),
                                    (
                                        crate::api::profile::proxy_image_url(cover),
                                        artists.to_string(),
                                    ),
                                );
                            }
                        }
                        println!(
                            "  - 从 cache/raw/netease.json 加载了 {} 首歌曲",
                            song_map.len()
                        );

                        // 打印前3个song_map条目作为样本
                        let sample: Vec<_> = song_map.iter().take(3).collect();
                        if !sample.is_empty() {
                            println!("  - song_map样本(前3个):");
                            for (song_title, (_, artist)) in sample {
                                println!("    '{}'  by  '{}'", song_title, artist);
                            }
                        }
                    } else {
                        println!("  ⚠️ cache/raw/netease.json 中没有找到 liked_songs 字段");
                    }
                } else {
                    println!("  ⚠️ 无法解析 cache/raw/netease.json");
                }
            } else {
                println!("  ⚠️ 无法读取 cache/raw/netease.json");
            }
        } else {
            println!("  ⚠️ cache/raw/netease.json 不存在");
        }

        // 从artist_analysis的favorite_artists中提取歌曲
        // 优化：限制每个艺术家最多2首歌，确保歌曲多样性
        const MAX_SONGS_PER_ARTIST: usize = 2;

        // 如果song_map为空(缓存文件不存在),从recent_songs构建基础map
        if song_map.is_empty() {
            println!("  ⚠️ song_map为空,从recent_songs构建基础map");
            for song in &analysis.recent_songs {
                song_map.insert(
                    song.title.clone(),
                    (String::new(), song.artist.clone()), // 封面为空
                );
            }
            println!("  - 从recent_songs构建了{}首歌曲的map", song_map.len());
        }

        println!(
            "  - 开始从 {} 位喜爱艺术家中筛选歌曲...",
            analysis.artist_analysis.favorite_artists.len()
        );
        println!("  - song_map大小: {}", song_map.len());
        println!(
            "  - 喜爱艺术家列表: {:?}",
            analysis.artist_analysis.favorite_artists
        );

        for artist_name in &analysis.artist_analysis.favorite_artists {
            let mut artist_song_count = 0;
            println!("  - 正在处理艺术家: '{}'", artist_name);

            // 在song_map中查找该艺术家的歌曲
            for (song_name, (cover, song_artist)) in &song_map {
                // 检查是否已经添加过这首歌
                let already_added = library_items.iter().any(|item: &Value| {
                    item.get("title")
                        .and_then(|v| v.as_str())
                        .map(|t| t == song_name)
                        .unwrap_or(false)
                });

                if already_added {
                    continue;
                }

                // 跳过未知艺术家
                if song_artist == "未知艺术家" {
                    continue;
                }

                // 使用包含关系匹配，因为艺术家名可能格式不完全一致
                // 例如："YOASOBI" vs "YOASOBI/幾田りら"
                let matches =
                    song_artist.contains(artist_name) || artist_name.contains(song_artist);

                if matches {
                    println!(
                        "    ✓ 匹配成功! '{}' (目标) vs '{}' (歌曲艺术家) -> 歌曲: {}",
                        artist_name, song_artist, song_name
                    );

                    library_items.push(json!({
                        "title": song_name,
                        "cover": cover,
                        "artist": song_artist,  // 使用原始艺术家名
                        "type": "music"
                    }));

                    artist_song_count += 1;

                    // 达到该艺术家的歌曲上限，切换到下一个艺术家
                    if artist_song_count >= MAX_SONGS_PER_ARTIST {
                        println!(
                            "    → {} 已达到上限({}/{}首)，切换下一位艺术家",
                            artist_name, artist_song_count, MAX_SONGS_PER_ARTIST
                        );
                        break;
                    }

                    // 达到总体上限
                    if library_items.len() >= 10 {
                        break;
                    }
                }
            }

            if library_items.len() >= 10 {
                break;
            }
        }

        println!("  - 从喜爱艺术家筛选完成: {}/10 首", library_items.len());

        // 如果favorite_artists提取的不够，从song_map中补充
        // 优化：也限制每个艺术家最多2首歌，确保补充阶段也保持多样性
        if library_items.len() < 10 {
            println!("  - 开始从所有歌曲中补充({}/10)...", library_items.len());
            use std::collections::HashMap;
            let mut artist_count_map: HashMap<String, usize> = HashMap::new();

            // 统计已添加歌曲的艺术家计数
            for item in &library_items {
                if let Some(artist_name) = item.get("artist").and_then(|v| v.as_str()) {
                    *artist_count_map.entry(artist_name.to_string()).or_insert(0) += 1;
                }
            }

            // 从song_map中补充，避免单个艺术家过多
            for (song_name, (cover, artist)) in song_map.iter() {
                if library_items.len() >= 10 {
                    break;
                }

                // 跳过未知艺术家
                if artist == "未知艺术家" {
                    continue;
                }

                // 检查是否已经添加过这首歌
                let already_added = library_items.iter().any(|item: &Value| {
                    item.get("title")
                        .and_then(|v| v.as_str())
                        .map(|t| t == song_name)
                        .unwrap_or(false)
                });

                if already_added {
                    continue;
                }

                // 检查该艺术家是否已达上限
                let current_count = artist_count_map.get(artist.as_str()).unwrap_or(&0);
                if *current_count >= MAX_SONGS_PER_ARTIST {
                    continue;
                }

                println!("    + 补充: {} - {}", artist, song_name);

                library_items.push(json!({
                    "title": song_name,
                    "cover": cover,
                    "artist": artist,
                    "type": "music"
                }));

                *artist_count_map.entry(artist.to_string()).or_insert(0) += 1;
            }

            println!("  - 补充完成: {}/10 首", library_items.len());
        }

        // 兜底：如果仍然没有收集到可展示的歌曲（例如原始缓存缺失或结构差异），
        // 使用智能过滤结果中的 recent_songs 构建基础的 library_items（无封面时前端会自动回退头像）。
        if library_items.is_empty() {
            for song in &analysis.recent_songs {
                library_items.push(json!({
                    "title": song.title,
                    "cover": "",
                    "artist": song.artist,
                    "type": "music"
                }));
                if library_items.len() >= 10 {
                    break;
                }
            }
        }

        // 去重（虽然上面已经检查过，但再确保一次）
        let mut seen_titles = std::collections::HashSet::new();
        library_items.retain(|item: &Value| {
            if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
                seen_titles.insert(title.to_string())
            } else {
                false
            }
        });

        // 限制最多返回10个
        library_items.truncate(10);

        println!(
            "  - Final netease library_items count: {}",
            library_items.len()
        );
    } else {
        println!("❌ Not Netease analysis!");
    }

    Ok(library_items)
}

// 哔哩哔哩用户统计数据结构
struct BilibiliUserStats {
    level: Value,
    follower_count: Value,
    following_count: Value,
}

// 网易云音乐用户统计数据结构
struct NeteaseUserStats {
    follower_count: Value,
    playlist_count: Value,
    // level 字段移除，改由 AI 生成
}

/// 提取哔哩哔哩用户统计数据
async fn extract_bilibili_user_stats(
    _metadata: &SmartFilteredData,
) -> Result<BilibiliUserStats, String> {
    use std::fs;
    use std::path::PathBuf;

    // 先尝试从原始B站缓存文件中读取
    let raw_cache_path = PathBuf::from("./cache/raw/bilibili.json");

    if !raw_cache_path.exists() {
        return Err("Bilibili raw cache not found".to_string());
    }

    let content = fs::read_to_string(&raw_cache_path)
        .map_err(|e| format!("Failed to read bilibili cache: {}", e))?;

    let raw_json: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse bilibili cache: {}", e))?;

    // 从 user_info 获取用户信息
    let user_info = raw_json.get("user_info");

    if let Some(user_info) = user_info {
        // 提取统计数据
        let level = user_info.get("level").cloned().unwrap_or(json!(0));
        let follower_count = user_info.get("follower").cloned().unwrap_or(json!(0));
        let following_count = user_info.get("following").cloned().unwrap_or(json!(0));

        Ok(BilibiliUserStats {
            level,
            follower_count,
            following_count,
        })
    } else {
        Err("Bilibili user_info not found in raw cache".to_string())
    }
}

/// 提取网易云音乐用户统计数据
async fn extract_netease_user_stats(
    _metadata: &SmartFilteredData,
) -> Result<NeteaseUserStats, String> {
    use std::fs;
    use std::path::PathBuf;

    // 先尝试从原始网易云缓存文件中读取
    let raw_cache_path = PathBuf::from("./cache/raw/netease.json");

    if !raw_cache_path.exists() {
        return Err("Netease raw cache not found".to_string());
    }

    let content = fs::read_to_string(&raw_cache_path)
        .map_err(|e| format!("Failed to read netease cache: {}", e))?;

    let raw_json: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse netease cache: {}", e))?;

    // 从 profile 获取用户信息
    let profile = raw_json.get("profile");

    if let Some(profile) = profile {
        // 提取统计数据
        let follower_count = profile.get("followeds").cloned().unwrap_or(json!(0));

        // 歌单数从 profile 中获取
        let playlist_count = profile.get("playlistCount").cloned().unwrap_or(json!(0));

        Ok(NeteaseUserStats {
            follower_count,
            playlist_count,
        })
    } else {
        Err("Netease profile not found in raw cache".to_string())
    }
}
