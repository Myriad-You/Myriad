// Platform report generation and AI report internals.

use axum::{extract::State, http::HeaderMap, Extension, Json};
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

use crate::config::{DynamicConfig, ModelTier};
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::platform_reports;
use crate::services::ai::create_ai_analyzer_for_tier;
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
    /// Host UI locale at generation time (`zh-CN` / `ja-JP` / `en-US`).
    #[serde(default)]
    pub locale: String,
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
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
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

    let locale = super::locale::locale_from_headers(&headers);
    tracing::info!("   Locale: {:?}", locale);
    let (platform_reports, skipped) =
        generate_platform_reports_internal(&db, user_id, req.platforms.clone(), locale).await;

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
        let locale_tag = locale.unwrap_or(super::locale::DEFAULT_AUTO_REGEN_LOCALE);
        let message = skipped
            .first()
            .map(|(_, reason)| reason.clone())
            .unwrap_or_else(|| super::locale::generate_none_message(locale_tag));
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
    locale: Option<&str>,
) -> (Vec<PlatformReport>, Vec<(String, String)>) {
    use futures::stream::{self, StreamExt};

    // 过期天数可配置（设置页 → 模块设置 → 报告页设置）
    let report_settings = crate::api::config::load_report_settings(db).await;

    let db_clone = db.clone();
    let locale_override = locale.map(|s| super::locale::normalize_report_locale(s).to_string());
    let results = stream::iter(platforms)
        .map(move |platform| {
            let db_for_task = db_clone.clone();
            let report_settings = report_settings.clone();
            let locale_override = locale_override.clone();
            async move {
            tracing::info!("🔄 Processing platform: {}", platform);
            let locale = match locale_override.as_deref() {
                Some(explicit) => explicit.to_string(),
                None => last_stored_report_locale(&db_for_task, user_id, &platform).await,
            };
            tracing::info!("   Report locale for {}: {locale}", platform);

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
                        super::locale::missing_platform_data_message(&locale),
                    ));
                }
            };

            // 3. 基于元数据生成平台报告
            tracing::debug!("🤖 Generating AI report for {}", platform);
            let (summary, ai_insights, mut card_visuals) =
                match generate_ai_report(&metadata, &platform, &locale).await {
                    Ok(res) => {
                        tracing::info!("✅ AI report generated for {}", platform);
                        res
                    }
                    Err(e) => {
                        tracing::warn!(
                            "⚠️ Failed to generate AI report for {}: {}, using mock",
                            platform,
                            e
                        );
                        super::mock::generate_mock_report(&metadata, &platform, &locale)
                            .unwrap_or_else(|_| (String::new(), vec![], json!({})))
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
                                json!(xbox_gamer_type_fallback(
                                    &locale,
                                    analysis.completed_games,
                                    analysis.average_completion,
                                    analysis.gamerscore
                                )),
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
                                json!(psn_hunter_type_fallback(
                                    &locale,
                                    analysis.platinum_count,
                                    analysis.average_progress
                                )),
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
            if insights.is_empty() {
                if let Ok((_, mock_insights, _)) =
                    super::mock::generate_mock_report(&metadata, &platform, &locale)
                {
                    insights = mock_insights;
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
                locale: locale.clone(),
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
    if !platform_reports.is_empty() {
        let names = platform_reports
            .iter()
            .map(|report| report.platform.as_str())
            .collect::<Vec<_>>()
            .join("、");
        crate::services::agent::merope::spawn_ingest(
            user_id,
            "agent.merope.report_ready",
            format!("这个人的报告算完了：{names}"),
        );
    }
    (platform_reports, skipped)
}

/// 一键生成所有启用平台的平台报告
/// POST /api/reports/generate-all
pub async fn generate_all_reports(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
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
    let locale = super::locale::locale_from_headers(&headers);
    let (platform_reports, skipped) =
        generate_platform_reports_internal(&db, user_id, enabled_platforms, locale).await;
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
/// Prefers durable site owner (`is_owner`); falls back to the legacy owner id 1.
/// Viewer credentials never select public report ownership.
pub(crate) async fn public_report_owner_user_id(db: &DatabaseConnection) -> i32 {
    if let Ok(owner_id) = crate::api::profile::site_owner_user_id(db).await {
        return owner_id;
    }
    1
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
    let report = super::locale::unwrap_stored_report_json(report);

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
                .map_err(|e| {
                    tracing::error!(platform, error = %e, "Failed to process platform data");
                    "Failed to process platform data".to_string()
                })?;

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
        return Err("Raw data file not found".to_string());
    }

    tracing::info!("⚙️  Processing {} from raw data...", platform);

    let content = fs::read_to_string(&raw_cache_path).map_err(|e| e.to_string())?;
    let platform_data: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // 6. 处理并缓存该平台数据（只处理单个平台！）
    let filtered_data = SmartFilter::process_and_save_single(platform, &platform_data)
        .map_err(|e| {
            tracing::error!(platform, error = %e, "Failed to process platform data");
            "Failed to process platform data".to_string()
        })?;

    tracing::info!("✓ Successfully processed and cached {}", platform);
    Ok(filtered_data)
}

/// 辅助函数：调用AI生成报告
async fn generate_ai_report(
    metadata: &SmartFilteredData,
    platform: &str,
    locale: &str,
) -> Result<(String, Vec<String>, Value), String> {
    let Some(analyzer) = create_ai_analyzer_for_tier(ModelTier::Standard).await else {
        tracing::warn!("AI analyzer unavailable for Standard tier. Using mock report.");
        return super::mock::generate_mock_report(metadata, platform, locale);
    };

    let data_str = super::prompt_data::serialize_for_report_prompt(metadata)?;
    let full_prompt = super::prompts::build_report_prompt(platform, &data_str, locale);
    let schema = super::prompt_data::platform_report_schema();

    // 6. 调用 AI（全站费用账本：source=reports，主体记在站长；含管理员触发）
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
        analyzer
            .analyze_json("", &full_prompt, "platform_report", Some(&schema))
            .await
    })
    .await;

    match ai_result {
        Ok(response) => {
            tracing::info!(
                "Generated AI report for {}: {} chars",
                platform,
                response.len()
            );
            match super::prompt_data::parse_platform_report_json(&response) {
                Ok(parsed) => Ok(parsed),
                Err(e) => {
                    tracing::error!(
                        "Failed to parse AI report JSON for {}: {}. Using mock.",
                        platform,
                        e
                    );
                    super::mock::generate_mock_report(metadata, platform, locale)
                }
            }
        }
        Err(e) => {
            tracing::error!("AI generation failed: {}", e);
            super::mock::generate_mock_report(metadata, platform, locale)
        }
    }
}

async fn last_stored_report_locale(
    db: &DatabaseConnection,
    user_id: i32,
    platform: &str,
) -> String {
    let row = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .filter(platform_reports::Column::Platform.eq(platform))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .one(db)
        .await
        .ok()
        .flatten();
    row.and_then(|r| super::locale::locale_from_stored_report(&r.report).map(str::to_string))
        .unwrap_or_else(|| super::locale::DEFAULT_AUTO_REGEN_LOCALE.to_string())
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

fn xbox_gamer_type_fallback(locale: &str, completed: usize, avg: f64, gs: i64) -> &'static str {
    use super::locale::pick;
    if completed >= 5 {
        pick(locale, "全成就猎人", "実績コンプ勢", "Completion hunter")
    } else if avg >= 50.0 {
        pick(locale, "深度攻略型", "攻略勢", "Deep completer")
    } else if gs >= 10_000 {
        pick(locale, "GS收藏家", "GSコレクター", "GS collector")
    } else {
        pick(locale, "广撒网玩家", "広く浅く", "Wide net")
    }
}

fn psn_hunter_type_fallback(locale: &str, platinum: i64, avg: f64) -> &'static str {
    use super::locale::pick;
    if platinum >= 10 {
        pick(locale, "白金收藏家", "プラチナ収集家", "Platinum collector")
    } else if platinum > 0 {
        pick(locale, "单机通关派", "単機クリア派", "Story completer")
    } else if avg >= 50.0 {
        pick(locale, "深度奖杯党", "トロフィー勢", "Trophy hunter")
    } else {
        pick(locale, "随缘奖杯党", "気まま勢", "Casual trophies")
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
