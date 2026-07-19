/// 新的双层报告系统 API
///
/// 架构：
/// 1. 平台元数据过滤 -> 提取5W关键信息
/// 2. 平台报告生成 -> 基于元数据生成各平台独立报告
/// 3. 全平台报告生成 -> 聚合各平台报告生成综合报告
use axum::{extract::State, http::StatusCode, Extension, Json};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
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
    Json(req): Json<GeneratePlatformReportsRequest>,
) -> Result<Json<Value>, StatusCode> {
    tracing::info!("📊 [ENTRY] generate_platform_reports called");
    tracing::info!("   Platforms: {:?}", req.platforms);
    tracing::info!("   User: {} (ID: {})", claims.username, claims.sub);

    let actor_id = claims.sub.parse::<i32>().map_err(|e| {
        tracing::error!("❌ Failed to parse user_id: {}", e);
        StatusCode::UNAUTHORIZED
    })?;
    let user_id = report_storage_user_id(&db, actor_id).await;

    let (platform_reports, skipped) =
        generate_platform_reports_internal(&db, user_id, req.platforms.clone()).await;

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

/// 内部函数：生成平台报告逻辑（支持并行处理）
///
/// 返回 (成功生成的报告, 被跳过的平台及原因)。跳过原因用于回传给前端，
/// 避免像以前那样只在日志里 warn、用户完全看不到失败在哪一步。
async fn generate_platform_reports_internal(
    db: &DatabaseConnection,
    user_id: i32,
    platforms: Vec<String>,
) -> (Vec<PlatformReport>, Vec<(String, String)>) {
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
                    return Err((
                        platform.clone(),
                        format!("平台数据未获取或处理失败（请先成功抓取该平台数据）：{}", e),
                    ));
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

                        // ⭐ star 总数是衡量开发者影响力的重要因素
                        let total_stars: i64 = analysis
                            .recent_repos
                            .iter()
                            .filter_map(|repo| repo.stars)
                            .sum();

                        // 根据真实数据计算贡献等级（star 数作为独立的晋级通道）
                        let contribution_level = if (total_contributions > 1000
                            && repos_count > 20)
                            || total_stars >= 1000
                        {
                            "传奇开发者"
                        } else if (total_contributions > 500 && repos_count > 10)
                            || total_stars >= 200
                        {
                            "资深工程师"
                        } else if total_contributions > 200 || repos_count > 5 || total_stars >= 50 {
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
                            json!(analysis.collection_type_distribution),
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

                // pbs.twimg.com 头像统一处理：升到 _400x400（头像墙/详情面共用），走站内代理
                fn proxied_x_avatar(url: &str) -> String {
                    let upscaled = url.replace("_normal.", "_400x400.");
                    format!("/api/proxy/image?url={}", urlencoding::encode(&upscaled))
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
                            analysis.user_avatar.as_deref().map(proxied_x_avatar);
                        obj.insert(
                            "profile".to_string(),
                            json!({
                                "username": metadata.user_summary.username,
                                "name": analysis.user_name,
                                "avatar": own_avatar,
                            }),
                        );
                        obj.insert(
                            "stats".to_string(),
                            json!({
                                "followers": metadata.user_summary.stats.follower_count.unwrap_or(0),
                                "following": metadata.user_summary.stats.following_count.unwrap_or(0),
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
                                        .map(proxied_x_avatar);
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
                        // 归一化 AI 产出：clamp community_tags（AI 偶尔无视上限）
                        if let Some(tags) =
                            obj.get_mut("community_tags").and_then(|v| v.as_array_mut())
                        {
                            tags.truncate(4);
                        }

                        // 账号画像（概览卡 header）—— 全部实测，覆盖 AI 幻觉
                        obj.insert("profile".to_string(), json!(analysis.profile));

                        obj.insert(
                            "stats".to_string(),
                            json!({
                                "guilds": analysis.guild_stats.guild_count,
                                "owned_guilds": analysis.guild_stats.owned_guild_count,
                                "admin_guilds": analysis.guild_stats.admin_guild_count,
                                "manage_guilds": analysis.guild_stats.manage_guild_count,
                                "connections": analysis.connections.len(),
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
                        // 仅展示 visibility!=0 的连接名称；仍保留 type 列表
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
                        obj.insert("connections".to_string(), json!(public_connections));
                        obj.insert(
                            "linked_platforms".to_string(),
                            json!(analysis.identity_graph.linked_platforms),
                        );
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
                        obj.insert("library_items".to_string(), json!(library_items));
                        obj.insert(
                            "guilds_preview".to_string(),
                            json!(analysis.guilds_preview),
                        );
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

            let report = PlatformReport {
                platform: platform.clone(),
                metadata: metadata.clone(),
                summary,
                insights,
                card_visuals: card_visuals.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
            };

            Ok(report)
        }
    });

    let results = join_all(futures).await;
    // 过期天数可配置（设置页 → 模块设置 → 报告页设置）
    let report_settings = crate::api::config::load_report_settings(db).await;
    let mut platform_reports: Vec<PlatformReport> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for result in results {
        match result {
            Ok(report) => platform_reports.push(report),
            Err(reason) => skipped.push(reason),
        }
    }

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
            expires_at: Set((chrono::Utc::now()
                + chrono::Duration::days(report_settings.expiry_days))
            .naive_utc()),
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
    (platform_reports, skipped)
}

/// 生成全平台综合报告（第二层）
/// POST /api/reports/comprehensive
pub async fn generate_comprehensive_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<GenerateComprehensiveReportRequest>,
) -> Result<Json<Value>, StatusCode> {
    let actor_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let user_id = report_storage_user_id(&db, actor_id).await;

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
            crate::services::smart_filter::ContentAnalysis::Bangumi(analysis) => {
                all_activities.push("追番与收藏".to_string());
                for tag in analysis.tag_distribution.keys() {
                    all_interests.push(tag.clone());
                }
            }
            crate::services::smart_filter::ContentAnalysis::Mal(analysis) => {
                all_activities.push("追番与漫画".to_string());
                for tag in analysis.tag_distribution.keys() {
                    all_interests.push(tag.clone());
                }
            }
            crate::services::smart_filter::ContentAnalysis::X(analysis) => {
                all_activities.push("发帖与互动".to_string());
                for lang in analysis.language_distribution.keys() {
                    all_interests.push(format!("lang:{}", lang));
                }
                for post in analysis.top_posts.iter().take(3) {
                    let snippet: String = post.text.chars().take(24).collect();
                    if !snippet.is_empty() {
                        all_interests.push(snippet);
                    }
                }
            }
            crate::services::smart_filter::ContentAnalysis::Discord(analysis) => {
                all_activities.push("社区交流".to_string());
                if analysis.guild_stats.manage_guild_count > 0 {
                    all_activities.push("服务器管理".to_string());
                }
                for platform in &analysis.identity_graph.linked_platforms {
                    all_interests.push(format!("linked:{}", platform));
                }
            }
            crate::services::smart_filter::ContentAnalysis::Xbox(analysis) => {
                all_activities.push("主机游戏".to_string());
                if analysis.completed_games > 0 {
                    all_activities.push("全成就攻略".to_string());
                }
                for title in analysis.recent_titles.iter().take(3) {
                    all_interests.push(title.name.clone());
                }
            }
            crate::services::smart_filter::ContentAnalysis::Psn(analysis) => {
                all_activities.push("主机游戏".to_string());
                if analysis.platinum_count > 0 {
                    all_activities.push("白金奖杯收集".to_string());
                }
                for title in analysis.recent_titles.iter().take(3) {
                    all_interests.push(title.name.clone());
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
    let report_settings = crate::api::config::load_report_settings(&db).await;
    let active_model = platform_reports::ActiveModel {
        user_id: Set(user_id),
        platform: Set("all".to_string()),
        metadata: Set(json!({})), // 综合报告没有单一的 metadata
        report: Set(report_json),
        report_title: Set(report_title), // 使用 visual_style 作为报告标题
        created_at: Set(chrono::Utc::now().naive_utc()),
        expires_at: Set(
            (chrono::Utc::now() + chrono::Duration::days(report_settings.expiry_days)).naive_utc(),
        ),
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
    let actor_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let user_id = report_storage_user_id(&db, actor_id).await;

    // 1. 获取用户启用的所有平台
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
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
        generate_platform_reports_internal(&db, user_id, enabled_platforms).await;
    if !skipped.is_empty() {
        tracing::warn!("⚠️ generate-all skipped platforms: {:?}", skipped);
    }

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
/// Public home / report cards: always return the **site owner's** platform reports
/// (same authority as `/api/user`, library, activities). Do not switch to the
/// viewer's user id when a session cookie is present — logged-in guests would
/// otherwise get empty cards on the owner's dashboard.
/// 过期报告自动重生成的在途去重表（key: "user_id:platform"）
static REPORT_REGEN_IN_FLIGHT: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

/// Resolve which user's reports the public latest/list endpoints should serve.
/// Prefers durable site owner (`is_owner`); falls back to positive claims / 1.
/// Never uses a non-owner viewer session — that emptied home ReportCards.
async fn public_report_owner_user_id(
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
async fn resolve_report_user_id_for_public_read(
    db: &DatabaseConnection,
    preferred: i32,
) -> Result<i32, StatusCode> {
    let preferred_count = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(preferred))
        .filter(platform_reports::Column::Platform.ne("all"))
        .count(db)
        .await
        .map_err(|e| {
            tracing::error!("count platform_reports for owner {}: {}", preferred, e);
            StatusCode::INTERNAL_SERVER_ERROR
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
            StatusCode::INTERNAL_SERVER_ERROR
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

/// Stamp platform + normalize card_visuals so home ReportCard widgets can match
/// and render stats even when older stored JSON is missing / double-encoded.
fn finalize_public_platform_report(platform: &str, report: Value) -> Value {
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
                // Unwrap double-nested card_visuals: { card_visuals: { …stats } }
                if let Some(inner) = v.get("card_visuals").filter(|i| i.is_object()) {
                    Some(inner.clone())
                } else {
                    None
                }
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
        if let Some(visuals) = normalized_visuals {
            obj.insert("card_visuals".to_string(), visuals);
        }
    }
    body
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

/// 后台重新生成过期的平台报告（只用已有缓存数据调 AI，不重新抓平台）
fn spawn_report_auto_regen(db: DatabaseConnection, user_id: i32, platforms: Vec<String>) {
    let to_run: Vec<String> = {
        let mut in_flight = REPORT_REGEN_IN_FLIGHT.lock().unwrap();
        platforms
            .into_iter()
            .filter(|p| in_flight.insert(format!("{user_id}:{p}")))
            .collect()
    };
    if to_run.is_empty() {
        return;
    }

    tokio::spawn(async move {
        tracing::info!("♻️ Auto-regenerating expired reports: {:?}", to_run);
        let (_reports, skipped) =
            generate_platform_reports_internal(&db, user_id, to_run.clone()).await;
        if !skipped.is_empty() {
            tracing::warn!("♻️ Auto-regen skipped some platforms: {:?}", skipped);
        }
        let mut in_flight = REPORT_REGEN_IN_FLIGHT.lock().unwrap();
        for platform in to_run {
            in_flight.remove(&format!("{user_id}:{platform}"));
        }
    });
}

pub async fn get_latest_report(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    // Public dashboard: site owner first; historical rows may live under another admin.
    let preferred = public_report_owner_user_id(&db, &headers).await;
    let user_id = resolve_report_user_id_for_public_read(&db, preferred).await?;

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

    let settings = crate::api::config::load_report_settings(&db).await;
    let now = chrono::Utc::now().naive_utc();

    // 保留每个平台最新的一份报告；按过期设置过滤
    let mut seen = std::collections::HashSet::new();
    let mut platform_reports_list: Vec<Value> = Vec::new();
    let mut expired_platforms: Vec<String> = Vec::new();

    for r in user_reports {
        if !seen.insert(r.platform.clone()) {
            continue;
        }
        let expired_at = r.created_at + chrono::Duration::days(settings.expiry_days);
        let expired = settings.expiry_enabled && expired_at <= now;
        let body = finalize_public_platform_report(&r.platform, r.report);
        if !expired {
            platform_reports_list.push(body);
        } else if settings.auto_regenerate {
            // stale-while-revalidate：先返回旧报告，后台异步重新生成
            expired_platforms.push(r.platform.clone());
            platform_reports_list.push(body);
        }
        // 过期且未开自动重生成：直接隐藏
    }

    if !expired_platforms.is_empty() {
        spawn_report_auto_regen(db.clone(), user_id, expired_platforms);
    }

    // 如果没有任何平台报告
    if platform_reports_list.is_empty() {
        return Ok(Json(json!({
            "success": false,
            "message": "No valid report found",
        })));
    }

    // 只返回平台报告，不包含综合分析.
    // Include user_id so clients/debug can verify which account fed home cards.
    Ok(Json(json!({
        "success": true,
        "user_id": user_id,
        "platform_reports": platform_reports_list,
        "created_at": chrono::Utc::now().to_rfc3339()
    })))
}

/// 获取所有综合报告列表
/// GET /api/reports/comprehensive/list
/// Public: site owner's comprehensive reports (same owner resolution as /latest).
pub async fn get_comprehensive_reports_list(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let user_id = public_report_owner_user_id(&db, &headers).await;

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
    let settings = crate::api::config::load_report_settings(&db).await;

    // 只返回未过期的报告的摘要信息（过期机制关闭时全部返回）
    let report_list: Vec<Value> = reports
        .into_iter()
        .filter(|r| {
            !settings.expiry_enabled
                || r.created_at + chrono::Duration::days(settings.expiry_days) > now
        })
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
/// DELETE /api/reports/comprehensive/{id}
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
/// GET /api/reports/comprehensive/{id}
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

        // 检查是否过期（过期机制关闭时跳过）
        let now = chrono::Utc::now().naive_utc();
        let settings = crate::api::config::load_report_settings(&db).await;
        let expired_at = model.created_at + chrono::Duration::days(settings.expiry_days);
        if settings.expiry_enabled && expired_at <= now {
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
            "用技术大佬的口吻，综合评估用户的代码贡献、技术栈深度和开源影响力。特别强调：仓库获得的 star 数量是衡量开发者水平和开源影响力的重要因素，高 star 项目往往代表更强的技术实力和社区认可度，评价时务必重点参考。",
            "card_visuals必须包含 'contribution_level' (字符串，如'传奇开发者'、'资深工程师'、'活跃开发者'), 'languages' (对象数组 {name, percentage})。在判定 contribution_level 时，除了贡献数和仓库数量，务必重点权衡仓库获得的 star 总数——star 越高代表开源影响力越强，应对应更高的等级。注意：不要生成 'total_contributions'、'repos_count' 和 'contribution_calendar' 字段，这些将由系统自动计算。"
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
            "用干净利落、有洞察力的口吻分析这个 Discord 账号。主线抓三件事：① 角色——自建/管理的服务器揭示 TA 是建设者还是参与者；② 社区触达——加入服务器的总成员规模说明 TA 活跃在大众广场还是垂直小圈；③ 跨平台身份——connections 绑定的 Steam/GitHub/YouTube 等暴露真实兴趣与职业线索。硬性要求：所有结论必须在数据里有出处，成员数/服务器数一律用原值不得虚构；summary 和 insights 正文里禁止出现 card_visuals、guilds_preview、identity_graph 等字段名或技术术语；禁止'很活跃''社交达人'这类放在谁身上都成立的空话；账号无绑定或全是路人服务器时，如实写成'低调潜水型'，不要拔高。",
            "card_visuals必须包含以下字段，无数据时用空字符串/空数组占位，禁止缺字段：'role_profile'（字符串，≤8字社区角色定位，如'社群主理人'/'圈子老炮'/'潜水观察者'/'跨平台节点'，须与自建/管理数量相符）；'vibe'（字符串，一句话社区人格，≤20字，具体有锋芒不客套，不带引号）；'community_tags'（字符串数组，2-4个刻画 TA 所在圈子气质的短标签，每个≤6字，如'开源社区''二次元''独立游戏''硬核玩家'，须能从服务器名/绑定平台推得，无据可依时给 []）。其余结构化字段（stats/guild_stats/identity_graph/connections/library_items/profile 等）由系统写入，一律省略、不要生成。"
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

            // ⭐ star 总数是衡量开发者影响力的重要因素
            let total_stars: i64 = analysis
                .recent_repos
                .iter()
                .filter_map(|repo| repo.stars)
                .sum();

            // 根据真实贡献数、仓库数量和 star 数确定贡献等级（star 作为独立晋级通道）
            let contribution_level = if (total_contributions > 1000
                && analysis.recent_repos.len() > 20)
                || total_stars >= 1000
            {
                "传奇开发者"
            } else if (total_contributions > 500 && analysis.recent_repos.len() > 10)
                || total_stars >= 200
            {
                "资深工程师"
            } else if total_contributions > 200
                || analysis.recent_repos.len() > 5
                || total_stars >= 50
            {
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
                    "total_stars": total_stars,
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
                    "status_counts": analysis.collection_type_distribution,
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
                    "status_counts": analysis.collection_type_distribution,
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
                        "followers": metadata.user_summary.stats.follower_count.unwrap_or(0),
                        "following": metadata.user_summary.stats.following_count.unwrap_or(0),
                        "posts": analysis.engagement_stats.total_posts,
                        "likes_received": analysis.engagement_stats.total_likes_received,
                    },
                    "top_posts": analysis.top_posts.iter().take(5).collect::<Vec<_>>(),
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
            // 社区标签：绑定平台名做兜底标签（AI 缺席时的占位）
            let community_tags: Vec<String> = analysis
                .identity_graph
                .linked_platforms
                .iter()
                .take(4)
                .cloned()
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

async fn extract_bangumi_library_items(metadata: &SmartFilteredData) -> Result<Vec<Value>, String> {
    let mut library_items = Vec::new();

    if let crate::services::smart_filter::ContentAnalysis::Bangumi(analysis) =
        &metadata.content_analysis
    {
        let mut candidates = analysis
            .top_rated_subjects
            .iter()
            .filter(|item| item.rate >= 8)
            .cloned()
            .collect::<Vec<_>>();

        if candidates.len() < 10 {
            for item in &analysis.watching_subjects {
                if !candidates
                    .iter()
                    .any(|candidate| candidate.subject_id == item.subject_id)
                {
                    candidates.push(item.clone());
                }
                if candidates.len() >= 10 {
                    break;
                }
            }
        }

        if candidates.len() < 10 {
            for item in &analysis.recent_updates {
                if !candidates
                    .iter()
                    .any(|candidate| candidate.subject_id == item.subject_id)
                {
                    candidates.push(item.clone());
                }
                if candidates.len() >= 10 {
                    break;
                }
            }
        }

        for item in candidates.into_iter().take(10) {
            library_items.push(json!({
                "title": item.title,
                "cover": item.cover.map(|cover| crate::api::profile::proxy_image_url(&cover)).unwrap_or_default(),
                "type": match item.subject_type.as_str() {
                    "book" => "book",
                    "anime" => "anime",
                    "game" => "game",
                    "music" => "music",
                    "real" => "tv_series",
                    _ => "video",
                },
                "platform": "bangumi",
                "rate": item.rate,
                "url": format!("https://bgm.tv/subject/{}", item.subject_id)
            }));
        }
    }

    Ok(library_items)
}

async fn extract_mal_library_items(metadata: &SmartFilteredData) -> Result<Vec<Value>, String> {
    let mut library_items = Vec::new();

    if let crate::services::smart_filter::ContentAnalysis::Mal(analysis) =
        &metadata.content_analysis
    {
        let mut candidates = analysis
            .top_rated_subjects
            .iter()
            .filter(|item| item.rate >= 8)
            .cloned()
            .collect::<Vec<_>>();

        if candidates.len() < 10 {
            for item in &analysis.watching_subjects {
                if !candidates
                    .iter()
                    .any(|candidate| candidate.subject_id == item.subject_id)
                {
                    candidates.push(item.clone());
                }
                if candidates.len() >= 10 {
                    break;
                }
            }
        }

        if candidates.len() < 10 {
            for item in &analysis.recent_updates {
                if !candidates
                    .iter()
                    .any(|candidate| candidate.subject_id == item.subject_id)
                {
                    candidates.push(item.clone());
                }
                if candidates.len() >= 10 {
                    break;
                }
            }
        }

        for item in candidates.into_iter().take(10) {
            let path_kind = if item.subject_type == "manga" {
                "manga"
            } else {
                "anime"
            };
            library_items.push(json!({
                "title": item.title,
                "cover": item.cover.map(|cover| crate::api::profile::proxy_image_url(&cover)).unwrap_or_default(),
                "type": match item.subject_type.as_str() {
                    "manga" => "book",
                    "anime" => "anime",
                    _ => "video",
                },
                "platform": "mal",
                "rate": item.rate,
                "url": format!("https://myanimelist.net/{}/{}", path_kind, item.subject_id)
            }));
        }
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
