// Platform report generation and AI report internals.

use axum::{extract::State, Extension, Json};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Max concurrent platform/AI report tasks (MYR-021).
/// Generous enough for multi-platform generate-all (~11 platforms) while
/// preventing unbounded cost amplification from `join_all` fan-out.
pub(crate) const MAX_CONCURRENT_PLATFORM_REPORTS: usize = 6;

use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::platform_reports;
use crate::services::analyzer::{AiAnalyzer, AiProvider};
use crate::services::smart_filter::{SmartFilter, SmartFilteredData};
use myriad_error::AppError;

use super::extract::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct GeneratePlatformReportsRequest {
    pub platforms: Vec<String>,
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

/// User id under which public platform reports are stored.
/// Prefer durable site owner so home ReportCards (which always *read* owner
/// reports) find rows written by any admin who generates them.
async fn report_storage_user_id(db: &DatabaseConnection, actor_id: i32) -> i32 {
    match crate::api::profile::site_owner_user_id(db).await {
        Ok(owner_id) => {
            if owner_id != actor_id {
                tracing::info!(
                    "Storing platform reports under site owner {} (actor was {})",
                    owner_id,
                    actor_id
                );
            }
            owner_id
        }
        Err(_) => actor_id,
    }
}

/// 生成平台报告（第一层）
/// POST /api/reports/platform
pub async fn generate_platform_reports(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<GeneratePlatformReportsRequest>,
) -> Result<Json<Value>, HttpError> {
    tracing::info!("📊 [ENTRY] generate_platform_reports called");
    tracing::info!("   Platforms: {:?}", req.platforms);
    tracing::info!("   User: {} (ID: {})", claims.username, claims.sub);

    let actor_id = claims.sub.parse::<i32>().map_err(|e| {
        tracing::error!("❌ Failed to parse user_id: {}", e);
        HttpError(AppError::unauthorized("Unauthorized"))
    })?;
    let user_id = report_storage_user_id(&db, actor_id).await;

    let (platform_reports, skipped) =
        generate_platform_reports_internal(&db, user_id, req.platforms.clone(), dynamic_config)
            .await;

    // 将跳过原因结构化，便于前端逐平台展示
    let skipped_json: Vec<Value> = skipped
        .iter()
        .map(|(platform, reason)| json!({ "platform": platform, "reason": reason }))
        .collect();

    if platform_reports.is_empty() {
        tracing::warn!(
            "⚠️ No platform reports generated for platforms: {:?}, skipped: {:?}",
            req.platforms,
            skipped
        );
        // 有具体原因时透出首个原因，否则回退到通用文案
        let message = skipped
            .first()
            .map(|(platform, reason)| format!("{} 未能生成报告：{}", platform, reason))
            .unwrap_or_else(|| "未能生成报告。请确保已获取平台数据。".to_string());
        return Ok(Json(json!({
            "success": false,
            "message": message,
            "reports": [],
            "skipped": skipped_json,
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
        "skipped": skipped_json,
        "token_estimate": total_tokens,
    })))
}

/// Atomically replace the stored report for `(user_id, platform)`.
///
/// DELETE + INSERT run in one DB transaction so a failed insert never leaves
/// the platform without its previous report (MYR-020). Serialization happens
/// *before* the transaction begins, so a serialize failure also never deletes.
async fn persist_platform_report_atomic(
    db: &DatabaseConnection,
    user_id: i32,
    report: &PlatformReport,
    report_settings: &crate::api::config::ReportSettings,
) -> Result<(), String> {
    tracing::debug!("💾 Serializing report for platform: {}", report.platform);

    let report_json = serde_json::to_value(report).map_err(|e| {
        tracing::error!(
            "❌ Failed to serialize report for {}: {}",
            report.platform,
            e
        );
        format!("serialize report for {}: {e}", report.platform)
    })?;

    let metadata_json = serde_json::to_value(&report.metadata).map_err(|e| {
        tracing::error!(
            "❌ Failed to serialize metadata for {}: {}",
            report.platform,
            e
        );
        format!("serialize metadata for {}: {e}", report.platform)
    })?;

    let txn = db
        .begin()
        .await
        .map_err(|e| format!("begin report persist transaction: {e}"))?;

    let delete_result = platform_reports::Entity::delete_many()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .filter(platform_reports::Column::Platform.eq(&report.platform))
        .exec(&txn)
        .await
        .map_err(|e| format!("delete old reports for {}: {e}", report.platform))?;

    if delete_result.rows_affected > 0 {
        tracing::info!(
            "🗑️ Deleted {} old report(s) for platform {} (txn)",
            delete_result.rows_affected,
            report.platform
        );
    }

    let active_model = platform_reports::ActiveModel {
        user_id: Set(user_id),
        platform: Set(report.platform.clone()),
        metadata: Set(metadata_json),
        report: Set(report_json),
        report_title: Set(None),
        created_at: Set(chrono::Utc::now().naive_utc()),
        expires_at: Set(
            (chrono::Utc::now() + chrono::Duration::days(report_settings.expiry_days)).naive_utc(),
        ),
        ..Default::default()
    };

    active_model
        .insert(&txn)
        .await
        .map_err(|e| format!("insert report for {}: {e}", report.platform))?;

    txn.commit()
        .await
        .map_err(|e| format!("commit report persist for {}: {e}", report.platform))?;

    tracing::info!("✅ Saved platform report for {}", report.platform);
    Ok(())
}

/// 内部函数：生成平台报告逻辑（有界并行 + 逐平台原子落库）
///
/// 返回 (成功生成的报告, 被跳过的平台及原因)。跳过原因用于回传给前端，
/// 避免像以前那样只在日志里 warn、用户完全看不到失败在哪一步。
///
/// - MYR-020: each platform is persisted in a DELETE+INSERT transaction; insert
///   failure rolls back and keeps the previous report.
/// - MYR-021: platform/AI fan-out is bounded by [`MAX_CONCURRENT_PLATFORM_REPORTS`].
/// - Partial cancel: reports are persisted as each platform finishes. If the
///   overall future is dropped (client disconnect / task cancel), already-saved
///   platforms remain and remaining work is abandoned.
pub(crate) async fn generate_platform_reports_internal(
    db: &DatabaseConnection,
    user_id: i32,
    platforms: Vec<String>,
    dynamic_config: Arc<RwLock<DynamicConfig>>,
) -> (Vec<PlatformReport>, Vec<(String, String)>) {
    use futures::stream::{self, StreamExt};

    // 过期天数可配置（设置页 → 模块设置 → 报告页设置）
    let report_settings = crate::api::config::load_report_settings(db).await;

    let db_clone = db.clone();
    let results = stream::iter(platforms)
        .map(move |platform| {
            let db_for_task = db_clone.clone();
            let dynamic_config = dynamic_config.clone();
            let report_settings = report_settings.clone();
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
                    return Err((
                        platform.clone(),
                        format!("平台数据未获取或处理失败（请先成功抓取该平台数据）：{}", e),
                    ));
                }
            };

            // 3. 基于元数据生成平台报告
            tracing::debug!("🤖 Generating AI report for {}", platform);
            let (summary, ai_insights, mut card_visuals) =
                match generate_ai_report(&metadata, &platform, &dynamic_config).await {
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

            // 5. Steam：强制覆盖数字字段（AI 易把分钟当小时或乱估数量）
            if platform == "steam" {
                if let crate::services::smart_filter::ContentAnalysis::Steam(analysis) =
                    &metadata.content_analysis
                {
                    if !card_visuals.is_object() {
                        card_visuals = json!({});
                    }
                    if let Some(obj) = card_visuals.as_object_mut() {
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
                        obj.insert("games_count".to_string(), json!(games_count));
                        obj.insert("total_playtime".to_string(), json!(hours));
                        // hardcore_score: keep AI flavour but clamp 0..=100
                        let score = obj
                            .get("hardcore_score")
                            .and_then(|v| {
                                v.as_i64()
                                    .or_else(|| v.as_u64().map(|u| u as i64))
                                    .or_else(|| {
                                        v.as_f64().and_then(|f| {
                                            f.is_finite().then_some(f.round() as i64)
                                        })
                                    })
                            })
                            .unwrap_or(50)
                            .clamp(0, 100);
                        obj.insert("hardcore_score".to_string(), json!(score));
                        // player_type: stable enum for FE i18n (legacy Chinese → key)
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
                        tracing::info!(
                            "✅ Steam card_visuals: games={}, playtime_h={} (from {} min)",
                            games_count,
                            hours,
                            minutes
                        );
                    }
                }
                if let Ok(library_items) = extract_steam_library_items(&metadata).await {
                    if !card_visuals.is_object() {
                        card_visuals = json!({});
                    }
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }
            }

            // 6. GitHub：强制覆盖关键数据字段（避免 AI 生成不稳定的值）
            if platform == "github" {
                if let crate::services::smart_filter::ContentAnalysis::GitHub(analysis) =
                    &metadata.content_analysis
                {
                    // 确保 card_visuals 是对象类型
                    if !card_visuals.is_object() {
                        card_visuals = json!({});
                    }
                    if let Some(obj) = card_visuals.as_object_mut() {
                        // 使用真实数据强制覆盖关键字段
                        let total_contributions = analysis
                            .contribution_calendar
                            .as_ref()
                            .map(|calendar| {
                                let sum: i64 = calendar
                                    .iter()
                                    .map(|day| day.count.max(0))
                                    .sum();
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
                        let contribution_level = github_contribution_level(
                            total_contributions,
                            repos_count,
                            total_stars,
                        );

                        // 语言占比：用实测 language_distribution，不用 AI 百分比
                        let total_lang: usize =
                            analysis.language_distribution.values().sum();
                        if total_lang > 0 {
                            let mut langs: Vec<_> = analysis
                                .language_distribution
                                .iter()
                                .map(|(name, n)| {
                                    let percentage = ((*n as f64 / total_lang as f64) * 100.0)
                                        .round()
                                        .clamp(0.0, 100.0)
                                        as i64;
                                    (name.clone(), *n, percentage)
                                })
                                .collect();
                            langs.sort_by(|a, b| b.1.cmp(&a.1));
                            let languages: Vec<Value> = langs
                                .into_iter()
                                .take(5)
                                .map(|(name, _, percentage)| {
                                    json!({ "name": name, "percentage": percentage })
                                })
                                .collect();
                            obj.insert("languages".to_string(), json!(languages));
                        }

                        // 强制覆盖这些字段（忽略AI可能生成的值）
                        obj.insert(
                            "total_contributions".to_string(),
                            json!(total_contributions),
                        );
                        obj.insert("repos_count".to_string(), json!(repos_count));
                        obj.insert("total_stars".to_string(), json!(total_stars));
                        obj.insert("contribution_level".to_string(), json!(contribution_level));
                        obj.insert(
                            "contribution_calendar".to_string(),
                            json!(analysis.contribution_calendar),
                        );

                        tracing::info!(
                            "✅ GitHub card_visuals: contributions={}, repos={}, stars={}, level={}",
                            total_contributions,
                            repos_count,
                            total_stars,
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

            // 6b. YouTube — 公开频道统计 + 最近上传轮播（0 视频仍是合法成功）
            if platform == "youtube" {
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }
                if let crate::services::smart_filter::ContentAnalysis::YouTube(analysis) =
                    &metadata.content_analysis
                {
                    let is_empty_channel =
                        analysis.video_count == 0 && analysis.recent_videos.is_empty();
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert(
                            "subscriber_count".to_string(),
                            json!(analysis.subscriber_count),
                        );
                        obj.insert("view_count".to_string(), json!(analysis.view_count));
                        obj.insert("video_count".to_string(), json!(analysis.video_count));
                        obj.insert(
                            "video_summary".to_string(),
                            json!(analysis.video_summary),
                        );
                        obj.insert("is_empty_channel".to_string(), json!(is_empty_channel));
                        if let Some(ref avatar) = analysis.avatar {
                            obj.insert("avatar".to_string(), json!(avatar));
                        }
                        if let Some(ref url) = analysis.channel_url {
                            obj.insert("channel_url".to_string(), json!(url));
                        }
                        if let Some(ref cu) = analysis.custom_url {
                            obj.insert("custom_url".to_string(), json!(cu));
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
                        obj.insert("library_items".to_string(), json!(library_items));
                        obj.insert(
                            "recent_videos".to_string(),
                            json!(analysis.recent_videos),
                        );
                    }
                }
                // user_summary username for face header
                if let Some(obj) = card_visuals.as_object_mut() {
                    obj.insert(
                        "channel_title".to_string(),
                        json!(metadata.user_summary.username),
                    );
                    obj.insert(
                        "channel_id".to_string(),
                        json!(metadata.user_summary.user_id),
                    );
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

            // 9. 对于 Bangumi 平台，额外添加资料库内容和稳定统计数据
            if platform == "bangumi" {
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }

                if let Ok(library_items) = extract_bangumi_library_items(&metadata).await {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }

                if let crate::services::smart_filter::ContentAnalysis::Bangumi(analysis) =
                    &metadata.content_analysis
                {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert(
                            "subject_type_distribution".to_string(),
                            json!(analysis.subject_type_distribution),
                        );
                        obj.insert(
                            "collection_type_distribution".to_string(),
                            json!(analysis.collection_type_distribution),
                        );
                        // 与 MAL 对齐：卡片优先读 status_counts（五态齐全，缺项补 0）
                        obj.insert(
                            "status_counts".to_string(),
                            anime_status_counts_five(&analysis.collection_type_distribution),
                        );
                        obj.insert(
                            "favorite_tags".to_string(),
                            json!(analysis.tag_distribution),
                        );
                        obj.insert(
                            "top_subjects".to_string(),
                            json!(analysis.top_rated_subjects),
                        );
                    }
                }
            }

            // 9b. MyAnimeList — 与 Bangumi 同构的卡片字段
            if platform == "mal" {
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }

                if let Ok(library_items) = extract_mal_library_items(&metadata).await {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }

                if let crate::services::smart_filter::ContentAnalysis::Mal(analysis) =
                    &metadata.content_analysis
                {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert(
                            "subject_type_distribution".to_string(),
                            json!(analysis.subject_type_distribution),
                        );
                        obj.insert(
                            "collection_type_distribution".to_string(),
                            json!(analysis.collection_type_distribution),
                        );
                        obj.insert(
                            "status_counts".to_string(),
                            anime_status_counts_five(&analysis.collection_type_distribution),
                        );
                        obj.insert(
                            "favorite_tags".to_string(),
                            json!(analysis.tag_distribution),
                        );
                        obj.insert(
                            "top_subjects".to_string(),
                            json!(analysis.top_rated_subjects),
                        );
                        if let Some(mean) = analysis.mean_score {
                            obj.insert("mean_score".to_string(), json!(mean));
                        }
                        if let Some(days) = analysis.days_watched {
                            obj.insert("days_watched".to_string(), json!(days));
                        }
                    }
                }
            }

            // 10. 对于 X 平台，用真实统计覆盖不稳定的 AI 字段
            if platform == "x" {
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }

                // pbs.twimg.com：升到 _400x400；代理交给出口 normalize_json_media_urls
                fn upscale_x_avatar(url: &str) -> String {
                    url.replace("_normal.", "_400x400.")
                }

                if let crate::services::smart_filter::ContentAnalysis::X(analysis) =
                    &metadata.content_analysis
                {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        // 归一化 AI 产出：钳制数组上限（AI 偶尔无视 prompt 约束）
                        if let Some(circles) = obj
                            .get_mut("interest_circles")
                            .and_then(|v| v.as_array_mut())
                        {
                            circles.truncate(4);
                            for circle in circles.iter_mut() {
                                if let Some(accounts) = circle
                                    .get_mut("accounts")
                                    .and_then(|v| v.as_array_mut())
                                {
                                    accounts.truncate(3);
                                }
                            }
                        }
                        if let Some(highlights) = obj
                            .get_mut("following_highlights")
                            .and_then(|v| v.as_array_mut())
                        {
                            highlights.truncate(5);
                        }
                        // 账号本人资料（概览卡 header）
                        let own_avatar =
                            analysis.user_avatar.as_deref().map(upscale_x_avatar);
                        obj.insert(
                            "profile".to_string(),
                            json!({
                                "username": metadata.user_summary.username,
                                "name": analysis.user_name,
                                "avatar": own_avatar,
                            }),
                        );
                        // Prefer Option null over forcing 0 for missing follow counts
                        obj.insert(
                            "stats".to_string(),
                            json!({
                                "followers": metadata.user_summary.stats.follower_count,
                                "following": metadata.user_summary.stats.following_count,
                                "posts": analysis.engagement_stats.total_posts,
                                "likes_received": analysis.engagement_stats.total_likes_received,
                                "retweets_received": analysis.engagement_stats.total_retweets_received,
                                "replies_received": analysis.engagement_stats.total_replies_received,
                                "impressions": analysis.engagement_stats.total_impressions,
                                "liked_posts": analysis.engagement_stats.liked_posts_count,
                            }),
                        );
                        // top_posts/recent_posts/language_distribution 不再进 card_visuals：
                        // X 卡已聚焦关注图谱，前端零消费；AI 分析用的推文数据走 metadata
                        if !analysis.following_sample.is_empty() {
                            let sample: Vec<Value> = analysis
                                .following_sample
                                .iter()
                                .map(|item| {
                                    let avatar = item
                                        .profile_image_url
                                        .as_deref()
                                        .map(upscale_x_avatar);
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
                            obj.insert("following_sample".to_string(), json!(sample));
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
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }
            }

            // 11. Discord：社区足迹与连接图谱
            if platform == "discord" {
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }

                if let crate::services::smart_filter::ContentAnalysis::Discord(analysis) =
                    &metadata.content_analysis
                {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        // 归一化 AI 产出：clamp community_tags（概览单行最多 3 个）
                        if let Some(tags) =
                            obj.get_mut("community_tags").and_then(|v| v.as_array_mut())
                        {
                            tags.truncate(3);
                        }

                        // 归一化 / 兜底 guild_takes（详情面服务器锐评）
                        normalize_discord_guild_takes(obj, &analysis.guilds_preview);

                        // 账号画像（概览卡 header）—— 全部实测，覆盖 AI 幻觉
                        obj.insert("profile".to_string(), json!(analysis.profile));

                        // Single guild list for FE (library_items); guild_count falls back to list len
                        // when member_reach is dead / OAuth omits approximate counts.
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
                            analysis
                                .guilds_preview
                                .len()
                                .max(library_items.len())
                        };

                        obj.insert(
                            "stats".to_string(),
                            json!({
                                "guilds": guild_count,
                                "owned_guilds": analysis.guild_stats.owned_guild_count,
                                "admin_guilds": analysis.guild_stats.admin_guild_count,
                                "manage_guilds": analysis.guild_stats.manage_guild_count,
                                // connections count filled after public filter below
                                "connections": 0,
                                "member_reach": analysis.guild_stats.total_member_reach,
                                "online_reach": analysis.guild_stats.total_online_reach,
                            }),
                        );
                        obj.insert(
                            "guild_stats".to_string(),
                            json!(analysis.guild_stats),
                        );
                        obj.insert(
                            "identity_graph".to_string(),
                            json!(analysis.identity_graph),
                        );
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
                        if let Some(stats_obj) = obj.get_mut("stats").and_then(|v| v.as_object_mut())
                        {
                            stats_obj.insert(
                                "connections".to_string(),
                                json!(public_connections.len()),
                            );
                            // Omit zero reach (no approximate_* without privileged intents)
                            if analysis.guild_stats.total_member_reach == 0 {
                                stats_obj.remove("member_reach");
                            }
                            if analysis.guild_stats.total_online_reach == 0 {
                                stats_obj.remove("online_reach");
                            }
                        }
                        obj.insert("connections".to_string(), json!(public_connections));
                        obj.insert(
                            "linked_platforms".to_string(),
                            json!(analysis.identity_graph.linked_platforms),
                        );
                        // Single list: library_items only (do not dual-write guilds_preview)
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }
            }

            // 12. Xbox / PSN：用真实成就/奖杯统计覆盖不稳定的 AI 数字
            if platform == "xbox" {
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }
                if let crate::services::smart_filter::ContentAnalysis::Xbox(analysis) =
                    &metadata.content_analysis
                {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        // 身份
                        obj.insert(
                            "gamertag".to_string(),
                            json!(analysis
                                .display_gamertag
                                .clone()
                                .unwrap_or_else(|| metadata.user_summary.username.clone())),
                        );
                        if let Some(ref av) = analysis.avatar {
                            obj.insert("avatar".to_string(), json!(av));
                        }
                        if let Some(ref tier) = analysis.account_tier {
                            obj.insert("account_tier".to_string(), json!(tier));
                        }
                        // 核心指标（一律用实测值，覆盖 AI 幻觉）
                        obj.insert("gamerscore".to_string(), json!(analysis.gamerscore));
                        obj.insert("games_count".to_string(), json!(analysis.games_count));
                        obj.insert(
                            "achievement_games".to_string(),
                            json!(analysis.achievement_games),
                        );
                        obj.insert(
                            "completed_games".to_string(),
                            json!(analysis.completed_games),
                        );
                        obj.insert(
                            "completion_rate".to_string(),
                            json!(analysis.average_completion.round()),
                        );
                        obj.insert(
                            "total_achievements".to_string(),
                            json!(analysis.total_achievements_earned),
                        );
                        obj.insert(
                            "total_achievements_available".to_string(),
                            json!(analysis.total_achievements_available),
                        );
                        obj.insert(
                            "hardcore_score".to_string(),
                            json!(analysis.hardcore_score),
                        );
                        // gamer_type 保留 AI 生成值；缺省时用规则兜底
                        if !obj.contains_key("gamer_type")
                            || obj
                                .get("gamer_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .is_empty()
                        {
                            obj.insert(
                                "gamer_type".to_string(),
                                json!(if analysis.completed_games >= 5 {
                                    "全成就猎人"
                                } else if analysis.average_completion >= 50.0 {
                                    "深度攻略型"
                                } else if analysis.gamerscore >= 10_000 {
                                    "GS 收藏家"
                                } else {
                                    "广撒网玩家"
                                }),
                            );
                        }
                        obj.insert(
                            "top_titles".to_string(),
                            json!(analysis
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
                                .collect::<Vec<_>>()),
                        );
                        // 详情轮播：优先「有进度 + 有封面」→ 最近有封面 → 其余
                        let mut lib_src: Vec<&crate::services::smart_filter::XboxTitleItem> =
                            Vec::new();
                        for t in analysis.top_completed_titles.iter() {
                            if t.display_image.is_some() {
                                lib_src.push(t);
                            }
                        }
                        for t in analysis.recent_titles.iter() {
                            if lib_src.len() >= 12 {
                                break;
                            }
                            if t.display_image.is_some()
                                && !lib_src.iter().any(|x| x.title_id == t.title_id)
                            {
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
                                let cover = t.display_image.as_ref().map(|u| {
                                    crate::services::smart_filter::SmartFilter::normalize_xbox_media_url(u)
                                });
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
                        obj.insert("library_items".to_string(), json!(library_items));
                    }
                }
            }

            if platform == "psn" {
                if !card_visuals.is_object() {
                    card_visuals = json!({});
                }
                if let crate::services::smart_filter::ContentAnalysis::Psn(analysis) =
                    &metadata.content_analysis
                {
                    if let Some(obj) = card_visuals.as_object_mut() {
                        obj.insert(
                            "online_id".to_string(),
                            json!(analysis
                                .display_online_id
                                .clone()
                                .unwrap_or_else(|| metadata.user_summary.username.clone())),
                        );
                        if let Some(ref av) = analysis.avatar {
                            obj.insert("avatar".to_string(), json!(av));
                        }
                        if let Some(plus) = analysis.is_plus {
                            obj.insert("is_plus".to_string(), json!(plus));
                        }
                        obj.insert("trophy_level".to_string(), json!(analysis.trophy_level));
                        obj.insert(
                            "platinum_count".to_string(),
                            json!(analysis.platinum_count),
                        );
                        obj.insert("gold_count".to_string(), json!(analysis.gold_count));
                        obj.insert("silver_count".to_string(), json!(analysis.silver_count));
                        obj.insert("bronze_count".to_string(), json!(analysis.bronze_count));
                        obj.insert(
                            "total_trophies".to_string(),
                            json!(analysis.total_trophies),
                        );
                        obj.insert("games_count".to_string(), json!(analysis.games_count));
                        obj.insert(
                            "completed_games".to_string(),
                            json!(analysis.completed_games),
                        );
                        obj.insert(
                            "completion_rate".to_string(),
                            json!(analysis.average_progress.round()),
                        );
                        obj.insert(
                            "hardcore_score".to_string(),
                            json!(analysis.hardcore_score),
                        );
                        if !obj.contains_key("hunter_type")
                            || obj
                                .get("hunter_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .is_empty()
                        {
                            obj.insert(
                                "hunter_type".to_string(),
                                json!(if analysis.platinum_count >= 10 {
                                    "白金收藏家"
                                } else if analysis.platinum_count > 0 {
                                    "单机通关派"
                                } else if analysis.average_progress >= 50.0 {
                                    "深度奖杯党"
                                } else {
                                    "随缘奖杯党"
                                }),
                            );
                        }
                        obj.insert(
                            "top_titles".to_string(),
                            json!(analysis
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
                                .collect::<Vec<_>>()),
                        );
                        // 详情轮播：有进度+封面优先 → 最近有封面 → 其余
                        let mut lib_src: Vec<&crate::services::smart_filter::PsnTitleItem> =
                            Vec::new();
                        for t in analysis.top_completed_titles.iter() {
                            if t.icon_url.is_some() {
                                lib_src.push(t);
                            }
                        }
                        for t in analysis.recent_titles.iter() {
                            if lib_src.len() >= 12 {
                                break;
                            }
                            if t.icon_url.is_some()
                                && !lib_src.iter().any(|x| x.name == t.name)
                            {
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
                                let cover = t.icon_url.as_ref().map(|u| {
                                    SmartFilter::normalize_https_media_url(u)
                                });
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
                        obj.insert("library_items".to_string(), json!(library_items));
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
                    crate::services::smart_filter::ContentAnalysis::YouTube(analysis) => {
                        insights.push(analysis.video_summary.clone());
                        insights.push(format!(
                            "订阅 {} · 观看 {} · 视频 {}",
                            analysis.subscriber_count, analysis.view_count, analysis.video_count
                        ));
                        if let Some(v) = analysis.recent_videos.first() {
                            insights.push(format!("最近上传：{}", v.title));
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
                    crate::services::smart_filter::ContentAnalysis::Bangumi(analysis) => {
                        insights.push(analysis.collection_summary.clone());
                        if let Some((tag, _)) = analysis
                            .tag_distribution
                            .iter()
                            .max_by_key(|(_, count)| *count)
                        {
                            insights.push(format!("常见标签：{}", tag));
                        }
                    }
                    crate::services::smart_filter::ContentAnalysis::Mal(analysis) => {
                        insights.push(analysis.collection_summary.clone());
                        if let Some((tag, _)) = analysis
                            .tag_distribution
                            .iter()
                            .max_by_key(|(_, count)| *count)
                        {
                            insights.push(format!("常见题材：{}", tag));
                        }
                    }
                    crate::services::smart_filter::ContentAnalysis::X(analysis) => {
                        insights.push(analysis.post_summary.clone());
                        insights.push(format!(
                            "互动：获赞 {} · 转推 {} · 评论 {}",
                            analysis.engagement_stats.total_likes_received,
                            analysis.engagement_stats.total_retweets_received,
                            analysis.engagement_stats.total_replies_received
                        ));
                        if let Some((lang, _)) = analysis
                            .language_distribution
                            .iter()
                            .max_by_key(|(_, count)| *count)
                        {
                            insights.push(format!("主要语言：{}", lang));
                        }
                    }
                    crate::services::smart_filter::ContentAnalysis::Discord(analysis) => {
                        insights.push(analysis.community_summary.clone());
                        insights.push(format!(
                            "社区：{} 服 · 自建 {} · 管理 {}",
                            analysis.guild_stats.guild_count,
                            analysis.guild_stats.owned_guild_count,
                            analysis.guild_stats.manage_guild_count
                        ));
                        if analysis.guild_stats.total_member_reach > 0 {
                            insights.push(format!(
                                "社区触达：约 {} 名成员",
                                analysis.guild_stats.total_member_reach
                            ));
                        }
                        if !analysis.identity_graph.linked_platforms.is_empty() {
                            insights.push(format!(
                                "已绑定：{}",
                                analysis.identity_graph.linked_platforms.join("、")
                            ));
                        }
                    }
                    crate::services::smart_filter::ContentAnalysis::Xbox(analysis) => {
                        insights.push(analysis.gaming_summary.clone());
                        if let Some(title) = analysis.recent_titles.first() {
                            insights.push(format!(
                                "最近在玩：{}（成就 {}/{}）",
                                title.name, title.achievements_earned, title.achievements_total
                            ));
                        }
                    }
                    crate::services::smart_filter::ContentAnalysis::Psn(analysis) => {
                        insights.push(analysis.trophy_summary_text.clone());
                        if let Some(title) = analysis.recent_titles.first() {
                            insights.push(format!(
                                "最近奖杯动态：{}（完成度 {}%）",
                                title.name, title.progress
                            ));
                        }
                    }
                }
            }

            // 出口统一媒体规范化（防盗链代理）；各平台分支可写原始 CDN
            crate::api::profile::normalize_json_media_urls(&mut card_visuals);

            let report = PlatformReport {
                platform: platform.clone(),
                metadata: metadata.clone(),
                summary,
                insights,
                card_visuals,
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            // Persist as soon as this platform finishes (MYR-020 atomic txn).
            // Partial cancel: if the parent future is dropped later, this row stays.
            if let Err(e) =
                persist_platform_report_atomic(&db_for_task, user_id, &report, &report_settings)
                    .await
            {
                tracing::error!(
                    "❌ Failed to atomically save platform report for {}: {} (previous report retained)",
                    report.platform,
                    e
                );
                // Still return the in-memory report so the API can surface content;
                // DB keeps the old row because the transaction rolled back.
            }

            Ok(report)
            }
        })
        .buffer_unordered(MAX_CONCURRENT_PLATFORM_REPORTS)
        .collect::<Vec<_>>()
        .await;

    let mut platform_reports: Vec<PlatformReport> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for result in results {
        match result {
            Ok(report) => platform_reports.push(report),
            Err(reason) => skipped.push(reason),
        }
    }

    tracing::info!(
        "🎯 Generated and saved {} platform reports (concurrency ≤ {}), skipped {}",
        platform_reports.len(),
        MAX_CONCURRENT_PLATFORM_REPORTS,
        skipped.len()
    );
    (platform_reports, skipped)
}

/// 一键生成所有启用平台的平台报告
/// POST /api/reports/generate-all
pub async fn generate_all_reports(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let actor_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| HttpError(AppError::unauthorized("Unauthorized")))?;
    let user_id = report_storage_user_id(&db, actor_id).await;

    // 1. 获取用户启用的所有平台（AppState.dynamic_config，与 GLOBAL_* 同 Arc）
    let config = dynamic_config.read().await;
    let enabled_platforms = [
        (
            "bilibili",
            config
                .bilibili_enabled
                .unwrap_or(config.bilibili_uid.as_ref().is_some()),
        ),
        (
            "steam",
            config
                .steam_enabled
                .unwrap_or(config.steam_api_key.as_ref().is_some()),
        ),
        (
            "github",
            config
                .github_enabled
                .unwrap_or(config.github_username.as_ref().is_some()),
        ),
        (
            "youtube",
            config.youtube_enabled.unwrap_or(
                config.youtube_api_key.as_ref().is_some()
                    && config.youtube_channel_id.as_ref().is_some(),
            ),
        ),
        (
            "netease",
            config
                .netease_enabled
                .unwrap_or(config.netease_user_id.as_ref().is_some()),
        ),
        (
            "bangumi",
            config.bangumi_enabled.unwrap_or(
                config.bangumi_username.as_ref().is_some()
                    || config.bangumi_access_token.as_ref().is_some(),
            ),
        ),
        (
            "x",
            config.x_enabled.unwrap_or(
                config.x_username.as_ref().is_some() && config.x_bearer_token.as_ref().is_some(),
            ),
        ),
        (
            "discord",
            config
                .discord_enabled
                .unwrap_or(config.discord_access_token.as_ref().is_some()),
        ),
        (
            "mal",
            config
                .mal_enabled
                .unwrap_or(config.mal_username.as_ref().is_some()),
        ),
        ("xbox", {
            let has_gamertag = config
                .xbox_gamertag
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("XBOX_GAMERTAG").is_ok();
            let has_key = config
                .openxbl_api_key
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("OPENXBL_API_KEY").is_ok()
                || std::env::var("XBL_API_KEY").is_ok();
            config.xbox_enabled.unwrap_or(has_gamertag && has_key)
        }),
        ("psn", {
            let has_id = config
                .psn_online_id
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("PSN_ONLINE_ID").is_ok();
            let has_npsso = config
                .psn_npsso
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("PSN_NPSSO").is_ok();
            config.psn_enabled.unwrap_or(has_id && has_npsso)
        }),
    ]
    .into_iter()
    .filter(|&(_, enabled)| enabled)
    .map(|(platform, _)| platform.to_string())
    .collect::<Vec<_>>();
    drop(config);

    // 2. 生成平台报告 (使用内部函数，避免序列化开销)
    let (platform_reports, skipped) =
        generate_platform_reports_internal(&db, user_id, enabled_platforms, dynamic_config).await;
    let skipped_json: Vec<_> = skipped
        .iter()
        .map(|(platform, reason)| json!({ "platform": platform, "reason": reason }))
        .collect();
    if !skipped.is_empty() {
        tracing::warn!("⚠️ generate-all skipped platforms: {:?}", skipped);
    }

    Ok(Json(json!({
        "success": true,
        "platform_reports": platform_reports,
        "skipped": skipped_json,
    })))
}

/// 获取最新的平台报告
/// GET /api/reports/latest
/// Public home / report cards: always return the **site owner's** platform reports
/// (same authority as `/api/user`, library, activities). Do not switch to the
/// viewer's user id when a session cookie is present — logged-in guests would
/// otherwise get empty cards on the owner's dashboard.
/// 过期报告自动重生成的在途去重表（key: "user_id:platform"）
/// In-flight set for regen; `std::sync::Mutex` short critical section only (no await while held).
pub(crate) static REPORT_REGEN_IN_FLIGHT: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Resolve which user's reports the public latest/list endpoints should serve.
/// Prefers durable site owner (`is_owner`); falls back to positive claims / 1.
/// Never uses a non-owner viewer session — that emptied home ReportCards.
pub(crate) async fn public_report_owner_user_id(
    db: &DatabaseConnection,
    headers: &axum::http::HeaderMap,
) -> i32 {
    if let Ok(owner_id) = crate::api::profile::site_owner_user_id(db).await {
        return owner_id;
    }
    crate::middleware::auth::extract_optional_claims(headers)
        .and_then(|claims| claims.sub.parse::<i32>().ok())
        .filter(|id| *id > 0)
        .unwrap_or(1)
}

/// Prefer `preferred` when they have platform reports; otherwise use the user_id
/// that most recently wrote a non-`all` platform report.
///
/// Historical generations stored under actor claims (pre-#144) left site-owner
/// home cards empty even though reports exist under another admin id.
pub(crate) async fn resolve_report_user_id_for_public_read(
    db: &DatabaseConnection,
    preferred: i32,
) -> Result<i32, HttpError> {
    let preferred_count = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(preferred))
        .filter(platform_reports::Column::Platform.ne("all"))
        .count(db)
        .await
        .map_err(|e| {
            tracing::error!("count platform_reports for owner {}: {}", preferred, e);
            HttpError(AppError::internal("Database error"))
        })?;
    if preferred_count > 0 {
        return Ok(preferred);
    }

    let fallback = platform_reports::Entity::find()
        .filter(platform_reports::Column::Platform.ne("all"))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("fallback platform_reports lookup failed: {}", e);
            HttpError(AppError::internal("Database error"))
        })?;

    if let Some(row) = fallback {
        if row.user_id != preferred {
            tracing::warn!(
                preferred,
                fallback = row.user_id,
                "Site owner has no platform_reports; serving latest reports from user_id={}",
                row.user_id
            );
        }
        return Ok(row.user_id);
    }
    Ok(preferred)
}

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
/// and render stats even when older stored JSON is missing / double-encoded.
pub(crate) fn finalize_public_platform_report(platform: &str, report: Value) -> Value {
    // Some historical rows double-encoded the JSON column as a string.
    let report = match report {
        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        other => other,
    };
    // Unwrap accidental `{ "report": { …PlatformReport } }` envelopes.
    let report = match &report {
        Value::Object(map)
            if map.contains_key("report")
                && !map.contains_key("card_visuals")
                && map.get("report").map(|v| v.is_object()).unwrap_or(false) =>
        {
            map.get("report").cloned().unwrap_or(report.clone())
        }
        _ => report,
    };

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
                // （旧逻辑只返回内层，导致普通对象走 None、读路径从不 normalize）
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
            // GitHub / Steam: legacy Chinese labels → stable enums for FE i18n
            if let Some(vobj) = visuals.as_object_mut() {
                if platform.eq_ignore_ascii_case("github") {
                    if let Some(raw) = vobj
                        .get("contribution_level")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                    {
                        vobj.insert(
                            "contribution_level".to_string(),
                            json!(normalize_github_contribution_level(&raw)),
                        );
                    }
                }
                if platform.eq_ignore_ascii_case("steam") {
                    if let Some(raw) = vobj
                        .get("player_type")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                    {
                        vobj.insert(
                            "player_type".to_string(),
                            json!(normalize_steam_player_type(&raw)),
                        );
                    }
                }
            }
            obj.insert("card_visuals".to_string(), visuals);
        }
    }
    body
}

// moved from latest_and_list (AI + platform data)
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

    // 4. 从统一元数据服务读取。新数据直接是完整 JSONB，该方法也会
    // 合并旧版 BatchSaver 留下的 `*_chunk_N` 记录。
    let metadata_service = crate::services::metadata_service::MetadataService::new(db.clone());
    if let Ok(all_metadata) = metadata_service.get_all_latest_metadata(user_id).await {
        if let Some(platform_data) = all_metadata.get(platform) {
            tracing::info!("✓ Loaded {} from unified metadata storage", platform);

            // 处理并缓存该平台数据
            let filtered_data = SmartFilter::process_and_save_single(platform, platform_data)
                .map_err(|e| format!("Failed to process {}: {}", platform, e))?;

            tracing::info!(
                "✓ Successfully processed and cached {} from database",
                platform
            );
            return Ok(filtered_data);
        }
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
    dynamic_config: &std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
) -> Result<(String, Vec<String>, Value), String> {
    // 1. 获取配置（与 AppState.dynamic_config 同 Arc）
    let config = dynamic_config.read().await;

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
            "card_visuals必须包含 'player_type' (稳定枚举: hardcore|casual|balanced，勿写中文玩家类型), 'hardcore_score' (0-100数字), 'games_count' (数字，游戏总数量), 'total_playtime' (数字，总游戏时长小时数)。"
        ),
        "github" => (
            "你是一个极客技术大佬，崇尚开源精神，说话严谨但带有技术幽默。",
            "用技术大佬的口吻，综合评估用户的代码贡献、技术栈深度和开源影响力。特别强调：仓库获得的 star 数量是衡量开发者水平和开源影响力的重要因素，高 star 项目往往代表更强的技术实力和社区认可度，评价时务必重点参考。",
            "card_visuals必须包含 'contribution_level' (稳定枚举: legendary|veteran|active|emerging，勿写中文等级名), 'languages' (对象数组 {name, percentage})。在判定 contribution_level 时，除了贡献数和仓库数量，务必重点权衡仓库获得的 star 总数——star 越高代表开源影响力越强，应对应更高的等级。注意：不要生成 'total_contributions'、'repos_count' 和 'contribution_calendar' 字段，这些将由系统自动计算。"
        ),
        "youtube" => (
            "你是一个熟悉 YouTube 创作者生态的频道观察者，能从订阅规模、累计观看、上传节奏与最近视频标题/互动里读出频道定位——是教程站、Vlog、评测、剪辑二创，还是长期停更的沉寂号。你只依据公开频道数据下结论，绝不编造不存在的视频、播放量或合作品牌。",
            "用干净利落、略带互联网锋芒的口吻写频道评语。主线抓三件事：① 体量——订阅/总观看/视频数的量级与匹配度；② 内容气味——从 recent_videos 标题与时长推断题材；③ 活跃度——冷启动/沉寂如实写，不要拔高。硬性要求：数字用原值；summary 与 insights 可稍展开；card_visuals.vibe 与 X 卡同规——一句话人设、严格≤20汉字、首页最多两行、禁止换行与两句堆叠、有锋芒不客套、不带引号；禁止字段名与空话；空频道也给≤20字短评。",
            "card_visuals必须包含：'vibe'（字符串，一句话频道人设，与 X 卡一致：≤20字，首页 line-clamp-2，禁止换行符，有锋芒不客套，不带引号）；'channel_type'（字符串，≤8字定位标签，如'技术教程'/'生活Vlog'/'冷启动号'/'停更沉寂'/'高播放低订阅'）。徽章材质由系统按订阅数映射 Creator Awards，不要生成 badge_color。订阅/观看/视频列表等由系统写入，一律省略。"
        ),
        "netease" => (
            "你是一个文艺青年/乐评人，感性细腻，喜欢用歌词或诗意的语言表达。",
            "用文艺感性的口吻，解读用户的听歌品味、情感倾向和深夜听歌习惯。",
            "card_visuals必须包含 'soul_color' (十六进制颜色), 'mood_keywords' (对象数组，每个对象包含 'tag' 和 'color' 字段，例如 [{\"tag\": \"感性\", \"color\": \"#7B68EE\"}, {\"tag\": \"深夜\", \"color\": \"#FF6B9D\"}])，'level' (数字1-10，根据用户的歌曲数量、歌单数量、听歌品味的广度和深度综合评估，越资深等级越高)。根据每个标签的情感色彩选择合适的颜色。"
        ),
        "bangumi" => (
            "你是一个熟悉动画、漫画、游戏与影像作品的资深 ACG 评论者，能从收藏状态、评分和标签里读出审美轨迹。",
            "用温和但有洞察力的口吻，分析用户在 Bangumi 上的收藏结构、评分偏好、正在追的作品和长期兴趣。",
            "card_visuals必须包含 'taste_profile' (字符串), 'status_counts' (对象), 'score_distribution' (对象), 'favorite_tags' (字符串数组), 'top_subjects' (对象数组，字段至少包含 title 和 rate)。"
        ),
        "mal" => (
            "你是一个熟悉国际动画/漫画社区的 MyAnimeList 评论者，能从列表状态、分数和题材标签里读出口味。",
            "用轻松但有洞察力的口吻，分析用户在 MyAnimeList 上的动画/漫画收藏结构、评分偏好、正在追的作品和长期兴趣。",
            "card_visuals必须包含 'taste_profile' (字符串), 'status_counts' (对象，done/doing/wish 等), 'score_distribution' (对象), 'favorite_tags' (字符串数组), 'top_subjects' (对象数组，字段至少包含 title 和 rate)。"
        ),
        "x" => (
            "你是一个熟悉社交媒体生态的 X (Twitter) 观察者，擅长从发帖节奏、互动数据、关注对象和话题偏好读出账号人设。关注了谁往往比发了什么更诚实——账号简介和粉丝量级能还原一个人真实的兴趣光谱。你只基于给定数据下结论，从不编造事实。",
            "用简洁有锋芒的互联网口吻分析这个账号。发帖多时以发帖风格和互动热度为主线；发帖少或为零时把账号当'沉浸观察者'解剖，以关注列表样本（following_sample，含账号简介和粉丝量）为主要证据聚类兴趣圈层。注意区分信号强弱：新闻媒体、连锁品牌/便利店、官方客服、抽奖羊毛号这类人人都会关注的大众功能性账号不体现个人品味，分析人设时应忽略它们，聚焦真正暴露兴趣的账号（创作者、小众领域、垂直社区等）。硬性要求：所有结论必须能在数据里找到出处，引用数字一律用原值不得虚构；summary 和 insights 的正文里禁止出现 following_sample、card_visuals 等字段名或任何技术术语；禁止'很有个性''内容丰富'这类放在谁身上都成立的空话；若关注样本为空，只分析发帖与资料，不得虚构关注对象。",
            "card_visuals必须包含以下全部字段，无数据时用空数组/空字符串占位，禁止缺字段：'vibe' (字符串，一句话账号人设，≤20字，有锋芒不客套，不要带引号)；'engagement_level' (字符串，如'高互动'/'沉浸观察者'/'脉冲发帖')；'signature_topics' (字符串数组，3-6个话题词，每个≤6字)；'interest_circles' (对象数组，2-4个兴趣圈层，先剔除新闻媒体/连锁品牌/官方客服等无品味信号的大众账号，再对剩余账号聚类：{\"name\": \"圈层名，严格≤6个字（如'国产手游''独立游戏'），具体可感，禁用'其他'\", \"count\": 圈层账号数, \"accounts\": [严格最多3个代表账号的username]}；每个入选账号只归入一个圈层，count 之和不超过样本总数，按 count 降序；样本为空时给 [])；'following_highlights' (对象数组，3-5个最能暴露个人品味的关注对象：{\"username\": \"...\", \"name\": \"显示名\", \"tag\": \"一词标签≤6字\"}；只选创作者/小众领域/垂直社区类账号，禁止选择新闻媒体、连锁品牌、便利店、官方客服等大众账号；username 和 name 必须逐字取自 following_sample 中的真实账号，禁止编造；样本为空时给 [])；'stats' (对象，原样引用数据中的 followers/following/posts/likes_received 数字)。"
        ),
        "xbox" => (
            "你是一个资深 Xbox 成就猎人，看重 Gamerscore、全成就（绿光成就宴）和稀有成就，说话带主机玩家的梗。",
            "用成就猎人的口吻分析用户的成就习惯：是全成就强迫症还是浅尝辄止型？最近在肝哪部作品？GS 规模与全成就密度如何？注意：Xbox 没有游玩时长数据，一切从成就进度和 Gamerscore 说话；数字字段系统会用实测值覆盖，你重点写准 gamer_type 人设标签。",
            "card_visuals必须包含 'gamer_type' (字符串，≤8字，如'全成就猎人'/'广撒网玩家'/'剧情通关党'/'GS收藏家'/'周末主机党')。其余数字字段（gamerscore/games_count/completion_rate/hardcore_score/top_titles 等）由系统写入，可省略。"
        ),
        "psn" => (
            "你是一个资深 PlayStation 白金猎人，把白金奖杯视为最高勋章，熟悉奖杯难度梗（如'白金神作'、'3秒白金'）。",
            "用白金猎人的口吻分析用户的奖杯柜：白金数量成色如何？是专注刷完一部再玩下一部，还是奖杯散落一地？最近哪部作品有奖杯动态？注意：PSN 没有游玩时长数据，一切从奖杯等级和完成度说话；数字字段系统会用实测值覆盖，你重点写准 hunter_type 人设标签。",
            "card_visuals必须包含 'hunter_type' (字符串，≤8字，如'白金收藏家'/'随缘奖杯党'/'单机通关派'/'深度奖杯党'/'周末主机党')。其余数字字段（trophy_level/platinum_count/hardcore_score/top_titles 等）由系统写入，可省略。"
        ),
        "discord" => (
            "你是一个懂 Discord 社区生态的观察者，擅长从一个人加入的服务器、担任的角色和绑定的第三方账号，读出他在网络社群里的位置与身份。服务器规模、自建/管理数量、账号年龄、跨平台绑定，共同拼出这个人的'社区人格'——是自建社群的主理人、深耕几个圈子的老玩家，还是广泛潜水的观察者。你只依据给定数据下结论，绝不编造服务器名、成员数或绑定关系。",
            "用干净利落、有洞察力的口吻分析这个 Discord 账号，可带轻幽默，但不要刻薄嘲讽或人身攻击。主线抓三件事：① 角色——自建/管理的服务器揭示 TA 是建设者还是参与者；② 社区触达——加入服务器的总成员规模说明 TA 活跃在大众广场还是垂直小圈；③ 跨平台身份——connections 绑定的 Steam/GitHub/YouTube 等暴露真实兴趣与职业线索。硬性要求：所有结论必须在数据里有出处，成员数/服务器数一律用原值不得虚构；summary 和 insights 正文里禁止出现 card_visuals、guilds_preview、identity_graph 等字段名或技术术语；禁止'很活跃''社交达人'这类放在谁身上都成立的空话；账号无绑定或主要是大型公共服时，如实写成'低调潜水型'，不要拔高。",
            "card_visuals必须包含以下字段，无数据时用空字符串/空数组占位，禁止缺字段：'role_profile'（字符串，≤8字社区角色定位，如'社群主理人'/'圈子老炮'/'潜水观察者'/'跨平台节点'，须与自建/管理数量相符）；'vibe'（字符串，一句话社区人格，≤20字，具体有趣、不客套、不刻薄，不带引号）；'community_tags'（字符串数组，2-3个刻画 TA 所在圈子气质的短标签，每个≤6字，如'开源社区''二次元''独立游戏'，概览单行展示，须能从服务器名/绑定平台推得，无据可依时给 []）；'guild_takes'（对象数组，针对 guilds_preview / 代表服务器列表的前 5-8 个各写一条点评：{\"name\": \"必须逐字取自数据中的真实服务器名\", \"id\": \"若数据有 id 则原样带上\", \"take\": \"≤16字点评，点出角色/规模/特色/圈层，可轻幽默，禁止刻薄嘲讽与空洞夸奖\"}；只覆盖数据里真实存在的服务器，禁止编造服务器名；无服务器时给 []）。其余结构化字段（stats/guild_stats/identity_graph/connections/library_items/profile 等）由系统写入，一律省略、不要生成。"
        ),
        _ => (
            "你是一个专业的数据分析师，客观理性。",
            "用专业客观的口吻分析用户数据。",
            "card_visuals可以是空对象。"
        ),
    };

    let data_str = {
        let full = serde_json::to_string_pretty(metadata).map_err(|e| e.to_string())?;
        // 截断过长的数据以避免 token 溢出
        let truncated: String = full.chars().take(12000).collect();
        if truncated.len() < full.len() {
            format!("{}\n... (数据已截断，仅显示前 12000 字符)", truncated)
        } else {
            truncated
        }
    };
    let full_prompt = format!(
        "System: {}\nTask: {}\nRequirement: Return ONLY a valid JSON object (no markdown, no code blocks) with the following structure:\n{{\n  \"summary\": \"一段简短的总结(50字以内)\",\n  \"insights\": [\"3-5条详细的洞察分析\"],\n  \"card_visuals\": {{ ...根据以下要求生成: {} }}\n}}\n\nData:\n{}",
        system_prompt, tone_desc, visual_req, data_str
    );

    // 6. 调用 AI（全站费用账本：source=reports，主体记在站长；含管理员触发）
    let input_data = json!({
        "prompt": full_prompt
    });

    let admin_id = if let Ok(db) = crate::services::tapp_registry::database().await {
        crate::services::tapp_ownership::get_admin_user_id(&db)
            .await
            .unwrap_or(1)
    } else {
        1
    };
    let attr = crate::services::ai_cost_ledger::AiLedgerAttribution {
        subject_id: admin_id,
        owner_id: admin_id,
        source: "reports".into(),
        operation: "report".into(),
        tapp_id: "__reports__".into(),
        task_id: format!("report:{platform}"),
    };
    let ai_result = crate::services::ai_cost_ledger::with_ai_ledger_attribution(attr, async {
        analyzer.analyze_profile(&input_data).await
    })
    .await;

    match ai_result {
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
            let games_count = if analysis.games_count > 0 {
                analysis.games_count
            } else {
                analysis.recent_games.len().max(
                    analysis
                        .genre_analysis
                        .iter()
                        .map(|g| g.examples.len())
                        .sum(),
                )
            };
            // playtime_forever is minutes → hours for card UI
            let total_playtime_minutes = if analysis.total_playtime_minutes > 0 {
                analysis.total_playtime_minutes
            } else {
                analysis
                    .recent_games
                    .iter()
                    .map(|g| g.playtime.max(0))
                    .sum()
            };
            let total_playtime_hours = (total_playtime_minutes.max(0) / 60) as u64;

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
                    // Stable enum for FE i18n (hardcore/casual/balanced)
                    "player_type": "hardcore",
                    "hardcore_score": 85,
                    "games_count": games_count,
                    "total_playtime": total_playtime_hours,
                    "top_genres": analysis.genre_analysis.iter().take(3).map(|g| &g.genre).collect::<Vec<_>>()
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::GitHub(analysis) => {
            // 使用完整的贡献日历数据计算总提交数（365天的真实数据）
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

            // star 总数是衡量开发者影响力的重要因素
            let total_stars: i64 = analysis
                .recent_repos
                .iter()
                .filter_map(|repo| repo.stars.map(|s| s.max(0)))
                .sum();
            let repos_count = analysis
                .public_repos
                .map(|n| n.max(0) as usize)
                .unwrap_or(0)
                .max(analysis.recent_repos.len());

            let contribution_level =
                github_contribution_level(total_contributions, repos_count, total_stars);

            // 计算语言百分比（按仓库数排序，百分比 clamp）
            let total_lang_count: usize = analysis.language_distribution.values().sum();
            let languages = if total_lang_count > 0 {
                let mut pairs: Vec<_> = analysis.language_distribution.iter().collect();
                pairs.sort_by(|a, b| b.1.cmp(a.1));
                pairs
                    .into_iter()
                    .take(5)
                    .map(|(k, v)| {
                        let percentage = ((*v as f64 / total_lang_count as f64) * 100.0)
                            .round()
                            .clamp(0.0, 100.0) as i32;
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
                    "repos_count": repos_count,
                    "total_stars": total_stars,
                    "languages": languages,
                    "contribution_calendar": analysis.contribution_calendar
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::YouTube(analysis) => {
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
            let is_empty_channel = analysis.video_count == 0 && analysis.recent_videos.is_empty();
            let subs = analysis.subscriber_count;
            let views = analysis.view_count;
            let vids = analysis.video_count;
            // Rough views-per-video for mock 评语 tone (not shown as a metric field)
            let vpv = if vids > 0 { views / vids } else { 0 };
            let (vibe, channel_type, summary, insights) = if is_empty_channel {
                (
                    "冷启动空壳频道".to_string(),
                    "冷启动号".to_string(),
                    format!(
                        "「{}」已挂上公开频道，但上传区还是一片空白——人设比内容先到位。",
                        metadata.user_summary.username
                    ),
                    vec![
                        analysis.video_summary.clone(),
                        "不是抓取失败：频道资料能读到，只是公开视频数为 0。".to_string(),
                        "有第一支公开片再同步，评语才会从「空壳」变成「有气味」。".to_string(),
                    ],
                )
            } else {
                let latest = analysis
                    .recent_videos
                    .first()
                    .map(|v| v.title.as_str())
                    .unwrap_or("（无标题样本）");
                let type_guess = if vpv >= 50_000 {
                    "高播放密度"
                } else if vids >= 50 && subs < 1_000 {
                    "长尾堆量"
                } else if vids <= 5 {
                    "精品少更"
                } else {
                    "稳定更新"
                };
                (
                    // Match X / AI prompt: vibe ≤20 汉字
                    format!("{}·{}", type_guess, metadata.user_summary.username)
                        .chars()
                        .take(20)
                        .collect::<String>(),
                    type_guess.to_string(),
                    format!(
                        "「{}」：{} 订阅 / {} 支片 / 均播约 {}——公开区已经有可闻的内容气味。",
                        metadata.user_summary.username, subs, vids, vpv
                    ),
                    vec![
                        analysis.video_summary.clone(),
                        format!(
                            "体量：订阅 {} · 累计观看 {} · 视频 {}（均播约 {}）。",
                            subs, views, vids, vpv
                        ),
                        format!("最近上传《{}》——标题是当前题材的直接证据。", latest),
                        "评语来自本地 mock（未配置 AI）：重生成后会换成模型口吻。".to_string(),
                    ],
                )
            };
            (
                summary,
                insights,
                json!({
                    "vibe": vibe,
                    "channel_type": channel_type,
                    "subscriber_count": analysis.subscriber_count,
                    "view_count": analysis.view_count,
                    "video_count": analysis.video_count,
                    "video_summary": analysis.video_summary,
                    "channel_title": metadata.user_summary.username,
                    "channel_id": metadata.user_summary.user_id,
                    "avatar": analysis.avatar,
                    "channel_url": analysis.channel_url,
                    "custom_url": analysis.custom_url,
                    "is_empty_channel": is_empty_channel,
                    "library_items": library_items,
                    "recent_videos": analysis.recent_videos,
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
        crate::services::smart_filter::ContentAnalysis::Bangumi(analysis) => {
            let done = analysis
                .collection_type_distribution
                .get("done")
                .copied()
                .unwrap_or_default();
            let doing = analysis
                .collection_type_distribution
                .get("doing")
                .copied()
                .unwrap_or_default();
            let top_title = analysis
                .top_rated_subjects
                .first()
                .map(|item| item.title.as_str())
                .unwrap_or("收藏作品");
            let favorite_tags = analysis
                .tag_distribution
                .iter()
                .take(6)
                .map(|(tag, _)| tag.clone())
                .collect::<Vec<_>>();

            (
                format!(
                    "{} 的 Bangumi 书架透露出稳定的审美坐标。",
                    metadata.user_summary.username
                ),
                vec![
                    format!("收藏概况：{}", analysis.collection_summary),
                    format!("完成 {} 部，正在进行 {} 部", done, doing),
                    format!("高分代表作：{}", top_title),
                ],
                json!({
                    "taste_profile": "细腻的 ACG 收藏家",
                    "status_counts": anime_status_counts_five(&analysis.collection_type_distribution),
                    "subject_type_distribution": analysis.subject_type_distribution,
                    "favorite_tags": favorite_tags,
                    "top_subjects": analysis.top_rated_subjects.iter().take(5).collect::<Vec<_>>()
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::Mal(analysis) => {
            let done = analysis
                .collection_type_distribution
                .get("done")
                .copied()
                .unwrap_or_default();
            let doing = analysis
                .collection_type_distribution
                .get("doing")
                .copied()
                .unwrap_or_default();
            let top_title = analysis
                .top_rated_subjects
                .first()
                .map(|item| item.title.as_str())
                .unwrap_or("listed title");
            let favorite_tags = analysis
                .tag_distribution
                .iter()
                .take(6)
                .map(|(tag, _)| tag.clone())
                .collect::<Vec<_>>();

            (
                format!(
                    "{} 的 MyAnimeList 列表勾勒出清晰的二次元轨迹。",
                    metadata.user_summary.username
                ),
                vec![
                    format!("收藏概况：{}", analysis.collection_summary),
                    format!("完成 {} 部，正在进行 {} 部", done, doing),
                    format!("高分代表作：{}", top_title),
                ],
                json!({
                    "taste_profile": "MAL 列表收藏家",
                    "status_counts": anime_status_counts_five(&analysis.collection_type_distribution),
                    "collection_type_distribution": analysis.collection_type_distribution,
                    "subject_type_distribution": analysis.subject_type_distribution,
                    "favorite_tags": favorite_tags,
                    "top_subjects": analysis.top_rated_subjects.iter().take(5).collect::<Vec<_>>(),
                    "mean_score": analysis.mean_score,
                    "days_watched": analysis.days_watched
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::X(analysis) => {
            let top_text = analysis
                .top_posts
                .first()
                .map(|p| p.text.chars().take(40).collect::<String>())
                .unwrap_or_else(|| "暂无热帖".to_string());
            (
                format!(
                    "@{} 的时间线像一场持续在线的数字独白。",
                    metadata.user_summary.username
                ),
                vec![
                    analysis.post_summary.clone(),
                    format!(
                        "互动火力：获赞 {} · 转推 {}",
                        analysis.engagement_stats.total_likes_received,
                        analysis.engagement_stats.total_retweets_received
                    ),
                    format!("代表帖：{}", top_text),
                ],
                json!({
                    "vibe": "在线观察者",
                    "engagement_level": if analysis.engagement_stats.total_likes_received > 1000 {
                        "高互动"
                    } else if analysis.engagement_stats.total_posts > 20 {
                        "活跃发帖"
                    } else {
                        "低调输出"
                    },
                    "signature_topics": ["互联网", "日常", "观点"],
                    "stats": {
                        "followers": metadata.user_summary.stats.follower_count,
                        "following": metadata.user_summary.stats.following_count,
                        "posts": analysis.engagement_stats.total_posts,
                        "likes_received": analysis.engagement_stats.total_likes_received,
                    },
                    "top_posts": analysis.top_posts.iter().take(5).collect::<Vec<_>>(),
                    "library_items": analysis.top_posts.iter().take(8).map(|p| json!({
                        "title": p.text.chars().take(80).collect::<String>(),
                        "type": "post",
                    })).collect::<Vec<_>>(),
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::Discord(analysis) => {
            let gs = &analysis.guild_stats;
            let top_guild = analysis
                .guilds_preview
                .first()
                .map(|g| g.name.as_str())
                .unwrap_or("社区");
            let linked = if analysis.identity_graph.linked_platforms.is_empty() {
                "暂无公开绑定".to_string()
            } else {
                analysis.identity_graph.linked_platforms.join("、")
            };
            // 角色定位：自建 > 管理 > 触达规模 > 潜水
            let role_profile = if gs.owned_guild_count > 0 {
                "社群主理人"
            } else if gs.admin_guild_count > 0 {
                "社区管理员"
            } else if gs.manage_guild_count > 0 {
                "社区协作者"
            } else if gs.total_member_reach >= 100_000 {
                "社区广场党"
            } else if !analysis.identity_graph.linked_platforms.is_empty() {
                "跨平台节点"
            } else {
                "潜水观察者"
            };
            // 社区标签：绑定平台名做兜底标签（AI 缺席时的占位；概览单行最多 3 个）
            let community_tags: Vec<String> = analysis
                .identity_graph
                .linked_platforms
                .iter()
                .take(3)
                .cloned()
                .collect();
            // 详情面服务器锐评：按角色/规模写模板，保证无 AI 时 UI 仍有内容
            let guild_takes: Vec<Value> = analysis
                .guilds_preview
                .iter()
                .take(8)
                .map(|g| {
                    json!({
                        "name": g.name,
                        "id": g.id,
                        "take": discord_fallback_guild_take(g),
                    })
                })
                .collect();

            let mut insights = vec![
                analysis.community_summary.clone(),
                format!(
                    "服务器：{} 个（自建 {} · 管理 {}）",
                    gs.guild_count, gs.owned_guild_count, gs.manage_guild_count
                ),
                format!("代表服务器：{}", top_guild),
                format!("绑定平台：{}", linked),
            ];
            if gs.total_member_reach > 0 {
                insights.push(format!(
                    "社区触达：约 {} 名成员（在线 {}）",
                    gs.total_member_reach, gs.total_online_reach
                ));
            }
            if !analysis.profile.badges.is_empty() {
                insights.push(format!("账号徽章：{}", analysis.profile.badges.join("、")));
            }

            (
                format!(
                    "{} 在 Discord 上留下清晰的社区足迹与跨平台身份线。",
                    metadata.user_summary.username
                ),
                insights,
                json!({
                    "vibe": "社区节点",
                    "role_profile": role_profile,
                    "community_tags": community_tags,
                    "guild_takes": guild_takes,
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::Xbox(analysis) => {
            let top_title = analysis
                .top_completed_titles
                .first()
                .map(|t| t.name.as_str())
                .unwrap_or("暂无作品");
            let gamertag = analysis
                .display_gamertag
                .clone()
                .unwrap_or_else(|| metadata.user_summary.username.clone());
            let mut library_items: Vec<Value> = Vec::new();
            for t in analysis
                .top_completed_titles
                .iter()
                .chain(analysis.recent_titles.iter())
            {
                if library_items.len() >= 12 {
                    break;
                }
                if t.display_image.is_none() {
                    continue;
                }
                let title = t.name.as_str();
                if library_items
                    .iter()
                    .any(|x| x.get("title").and_then(|v| v.as_str()) == Some(title))
                {
                    continue;
                }
                library_items.push(json!({
                    "title": t.name,
                    "type": "game",
                    "cover": t.display_image,
                    "progress": t.progress.round(),
                    "achievements_earned": t.achievements_earned,
                    "achievements_total": t.achievements_total,
                    "gamerscore": t.gamerscore_earned,
                }));
            }
            (
                format!("{} 的 Xbox 成就柜写满了绿色的勋章。", gamertag),
                vec![
                    analysis.gaming_summary.clone(),
                    format!("完成度最高：{}", top_title),
                ],
                json!({
                    "gamer_type": if analysis.completed_games >= 5 {
                        "全成就猎人"
                    } else if analysis.average_completion >= 50.0 {
                        "深度攻略型"
                    } else if analysis.gamerscore >= 10_000 {
                        "GS 收藏家"
                    } else {
                        "广撒网玩家"
                    },
                    "gamertag": gamertag,
                    "avatar": analysis.avatar,
                    "account_tier": analysis.account_tier,
                    "gamerscore": analysis.gamerscore,
                    "games_count": analysis.games_count,
                    "achievement_games": analysis.achievement_games,
                    "completed_games": analysis.completed_games,
                    "completion_rate": analysis.average_completion.round(),
                    "total_achievements": analysis.total_achievements_earned,
                    "total_achievements_available": analysis.total_achievements_available,
                    "hardcore_score": analysis.hardcore_score,
                    "top_titles": analysis.top_completed_titles.iter().take(6).map(|t| json!({
                        "name": t.name,
                        "progress": t.progress.round(),
                        "gamerscore": t.gamerscore_earned,
                        "image": t.display_image,
                    })).collect::<Vec<_>>(),
                    "library_items": library_items,
                }),
            )
        }
        crate::services::smart_filter::ContentAnalysis::Psn(analysis) => {
            let top_title = analysis
                .top_completed_titles
                .first()
                .map(|t| t.name.as_str())
                .unwrap_or("暂无作品");
            let online_id = analysis
                .display_online_id
                .clone()
                .unwrap_or_else(|| metadata.user_summary.username.clone());
            let mut library_items: Vec<Value> = Vec::new();
            for t in analysis
                .top_completed_titles
                .iter()
                .chain(analysis.recent_titles.iter())
            {
                if library_items.len() >= 12 {
                    break;
                }
                if t.icon_url.is_none() {
                    continue;
                }
                let title = t.name.as_str();
                if library_items
                    .iter()
                    .any(|x| x.get("title").and_then(|v| v.as_str()) == Some(title))
                {
                    continue;
                }
                library_items.push(json!({
                    "title": t.name,
                    "type": "game",
                    "cover": t.icon_url.as_ref().map(|u| SmartFilter::normalize_https_media_url(u)),
                    "progress": t.progress,
                    "platinum": t.earned_platinum > 0,
                    "platform": t.platform,
                }));
            }
            (
                format!("{} 的 PSN 奖杯柜闪着白金的光。", online_id),
                vec![
                    analysis.trophy_summary_text.clone(),
                    format!("完成度最高：{}", top_title),
                ],
                json!({
                    "hunter_type": if analysis.platinum_count >= 10 {
                        "白金收藏家"
                    } else if analysis.platinum_count > 0 {
                        "单机通关派"
                    } else if analysis.average_progress >= 50.0 {
                        "深度奖杯党"
                    } else {
                        "随缘奖杯党"
                    },
                    "online_id": online_id,
                    "avatar": analysis.avatar,
                    "is_plus": analysis.is_plus,
                    "trophy_level": analysis.trophy_level,
                    "platinum_count": analysis.platinum_count,
                    "gold_count": analysis.gold_count,
                    "silver_count": analysis.silver_count,
                    "bronze_count": analysis.bronze_count,
                    "total_trophies": analysis.total_trophies,
                    "games_count": analysis.games_count,
                    "completed_games": analysis.completed_games,
                    "completion_rate": analysis.average_progress.round(),
                    "hardcore_score": analysis.hardcore_score,
                    "top_titles": analysis.top_completed_titles.iter().take(6).map(|t| json!({
                        "name": t.name,
                        "progress": t.progress,
                        "platinum": t.earned_platinum > 0,
                        "platform": t.platform,
                        "image": t.icon_url.as_ref().map(|u| SmartFilter::normalize_https_media_url(u)),
                    })).collect::<Vec<_>>(),
                    "library_items": library_items,
                }),
            )
        }
    };

    Ok((summary, insights, visuals))
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
    // Unknown free-text from older AI: default balanced (not casual)
    if t.is_empty() {
        "casual"
    } else {
        "balanced"
    }
}

/// GitHub contribution tier for `card_visuals.contribution_level`.
/// Stable English enum keys only — FE maps to locale labels / badge colors.
/// Thresholds match the historical Chinese tiering (star as independent path).
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

/// Map legacy Chinese / alternate labels to enum keys (stored reports).
pub(crate) fn normalize_github_contribution_level(raw: &str) -> &'static str {
    match raw.trim() {
        "legendary" | "传奇开发者" | "Legendary" | "Legendary Dev" | "Legendary Developer" => {
            "legendary"
        }
        "veteran" | "资深工程师" | "资深开发者" | "Veteran" | "Veteran Developer" => {
            "veteran"
        }
        "active"
        | "活跃开发者"
        | "高级开发者"
        | "中级开发者"
        | "Senior"
        | "Senior Dev"
        | "Senior Developer"
        | "Intermediate Developer" => "active",
        "emerging" | "新兴贡献者" | "初级开发者" | "Beginner" | "Beginner Dev"
        | "Beginner Developer" => "emerging",
        other => {
            let lower = other.to_ascii_lowercase();
            match lower.as_str() {
                "legendary" | "veteran" | "active" | "emerging" => {
                    // re-borrow static
                    match lower.as_str() {
                        "legendary" => "legendary",
                        "veteran" => "veteran",
                        "active" => "active",
                        _ => "emerging",
                    }
                }
                _ => "emerging",
            }
        }
    }
}

#[cfg(test)]
mod report_persist_concurrency_tests {
    use super::MAX_CONCURRENT_PLATFORM_REPORTS;
    use futures::stream::{self, StreamExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn max_concurrent_platform_reports_is_generous_but_bounded() {
        // Enough for multi-platform generate-all (~11 platforms); never unbounded.
        // Compare via binding so clippy does not treat this as a constant assertion.
        let limit = MAX_CONCURRENT_PLATFORM_REPORTS;
        assert!(
            (4..=16).contains(&limit),
            "unexpected concurrency limit: {limit}"
        );
    }

    /// Mirrors MYR-021 fan-out: many platform tasks, at most N in flight.
    /// Also models partial cancel — dropping the stream keeps completed work.
    #[tokio::test]
    async fn platform_report_fanout_respects_concurrency_bound() {
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let n = 20usize;
        let limit = MAX_CONCURRENT_PLATFORM_REPORTS;

        stream::iter(0..n)
            .map(|_| {
                let current = current.clone();
                let max_seen = max_seen.clone();
                let completed = completed.clone();
                async move {
                    let c = current.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(c, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    current.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                }
            })
            .buffer_unordered(limit)
            .collect::<Vec<_>>()
            .await;

        let peak = max_seen.load(Ordering::SeqCst);
        assert!(
            peak <= limit,
            "peak concurrency {peak} exceeded bound {limit}"
        );
        assert!(peak > 1, "expected some parallelism, peak was {peak}");
        assert_eq!(completed.load(Ordering::SeqCst), n);
    }

    /// Dropping the consumer mid-flight must not lose already-finished units
    /// (MYR-021 partial cancel + MYR-020 persist-as-you-go model).
    #[tokio::test]
    async fn partial_cancel_keeps_completed_units() {
        let completed = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let n = 12usize;
        let limit = MAX_CONCURRENT_PLATFORM_REPORTS;

        let completed_c = completed.clone();
        let started_c = started.clone();
        let mut stream = stream::iter(0..n)
            .map(move |i| {
                let completed = completed_c.clone();
                let started = started_c.clone();
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    // First few finish quickly; later ones block.
                    if i < 3 {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        completed.fetch_add(1, Ordering::SeqCst);
                        return i;
                    }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    completed.fetch_add(1, Ordering::SeqCst);
                    i
                }
            })
            .buffer_unordered(limit);

        // Collect only the first 3 completed items, then drop the stream (cancel).
        let mut got = Vec::new();
        while let Some(v) = stream.next().await {
            got.push(v);
            if got.len() >= 3 {
                break;
            }
        }
        drop(stream);

        assert_eq!(got.len(), 3);
        assert!(
            completed.load(Ordering::SeqCst) >= 3,
            "completed counter should reflect finished units kept after cancel"
        );
        // Not all N should have completed (slow ones abandoned).
        assert!(
            completed.load(Ordering::SeqCst) < n,
            "cancel should abandon remaining work"
        );
    }
}

#[cfg(test)]
mod finalize_public_report_media_tests {
    use super::*;

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
    fn normalize_github_contribution_level_maps_legacy_chinese() {
        assert_eq!(
            normalize_github_contribution_level("传奇开发者"),
            "legendary"
        );
        assert_eq!(normalize_github_contribution_level("资深工程师"), "veteran");
        assert_eq!(normalize_github_contribution_level("活跃开发者"), "active");
        assert_eq!(
            normalize_github_contribution_level("新兴贡献者"),
            "emerging"
        );
        assert_eq!(normalize_github_contribution_level("veteran"), "veteran");
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
