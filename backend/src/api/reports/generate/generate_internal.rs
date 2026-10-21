//! Platform report generation internals: bounded fan-out, facts, script, and platform data.

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde_json::Value;

use crate::models::entities::platform_reports;
use crate::services::smart_filter::{SmartFilter, SmartFilteredData};

use super::PlatformReport;
use super::persist::{MAX_CONCURRENT_PLATFORM_REPORTS, persist_platform_report_atomic};

/// 内部函数：生成平台报告逻辑（有界并行 + 逐平台原子落库）
///
/// 返回 (成功生成的报告, 被跳过的平台及原因)。跳过原因回传给前端。
///
/// - each platform is persisted in a DELETE+INSERT transaction; insert
///   failure rolls back and keeps the previous report.
/// - platform/AI fan-out is bounded by [`MAX_CONCURRENT_PLATFORM_REPORTS`].
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

    let style = crate::services::report_styles::load_report_styles(db)
        .await
        .current_style();
    let db_clone = db.clone();
    let locale_override =
        locale.map(|s| crate::api::reports::locale::normalize_report_locale(s).to_string());
    let results = stream::iter(platforms)
        .map(move |platform| {
            let db_for_task = db_clone.clone();
            let locale_override = locale_override.clone();
            let style = style.clone();
            async move {
            let Some(_generation) = super::ReportGeneration::claim(user_id, &platform) else {
                tracing::info!("⏭️ Report for {} is already being generated", platform);
                return Err((platform.clone(), "Report is already being generated".to_string()));
            };
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
                        crate::api::reports::locale::missing_platform_data_message(&locale),
                    ));
                }
            };

            // 2. 事实层：数字、名字、卡面字段都在这里算好，不经模型
            let facts = crate::api::reports::facts::build(&platform, &metadata).await;

            // 3. 文字层：按当前风格写剧本，不合格重试一次，再失败走本地兜底
            let written = super::write::write_or_mock(&super::write::WriteRequest {
                metadata: &metadata,
                platform: &platform,
                locale: &locale,
                style: &style,
                facts: &facts.facts,
                only: None,
                purpose: super::write::Purpose::Report,
            })
            .await;
            let card_visuals = crate::api::reports::facts::card_visuals(
                &platform,
                written.card_visuals,
                &facts,
                &metadata,
                &locale,
            );

            // 旧读取方（report_dna、TAPP、联邦、Agent）只认 summary / insights：
            // summary 是 headline，insights 是全部台词，都只从这份剧本来
            let summary = crate::api::reports::script::summary_of(&written.script);
            let insights = crate::api::reports::script::insights_of(&written.script);

            let report = PlatformReport {
                platform: platform.clone(),
                metadata: metadata.clone(),
                summary,
                insights,
                card_visuals,
                created_at: chrono::Utc::now().to_rfc3339(),
                locale: locale.clone(),
                script: Some(written.script),
                facts: crate::api::reports::facts::proxied_facts(&facts.facts),
                // 本地兜底不是按风格写的：不打戳，读出时 style_stale=true
                style: written.by_model.then(|| super::StyleStamp::of(&style)),
            };

            // Persist as soon as this platform finishes (atomic txn).
            // Partial cancel: if the parent future is dropped later, this row stays.
            if let Err(e) =
                persist_platform_report_atomic(&db_for_task, user_id, &report)
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
            .join(", ");
        crate::services::agent::merope::spawn_ingest(
            user_id,
            "agent.merope.report_ready",
            format!("This person's reports are ready: {names}"),
        );
    }
    (platform_reports, skipped)
}

/// Cached platform data, else the stored metadata, else the raw file.
pub(crate) async fn get_platform_data(
    platform: &str,
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<SmartFilteredData, String> {
    use crate::services::keyed_lock::KeyedLocks;
    use once_cell::sync::Lazy;
    use std::fs;

    static PLATFORM_LOCKS: Lazy<KeyedLocks> = Lazy::new(KeyedLocks::default);

    // 1. 尝试从独立平台缓存加载
    if let Ok(cached_data) = SmartFilter::load_platform_cache(platform) {
        tracing::debug!("✓ Loaded {} from platform cache", platform);
        return Ok(cached_data);
    }

    tracing::info!("Platform cache miss for {}, processing...", platform);

    // 2. 获取平台特定的锁
    let lock = PLATFORM_LOCKS.get(platform);

    let _guard = lock.lock().await;

    // 3. 再次检查缓存（可能被其他请求已处理）
    if let Ok(cached_data) = SmartFilter::load_platform_cache(platform) {
        tracing::info!("✓ {} processed by concurrent request", platform);
        return Ok(cached_data);
    }

    // 4. 从统一元数据服务只读取目标平台（`*_chunk_N` 行会并进主记录）。
    let metadata_service = crate::services::metadata_service::MetadataService::new(db.clone());
    match metadata_service
        .get_latest_platform_metadata(user_id, platform)
        .await
    {
        Ok(Some(platform_data)) => {
            tracing::info!("✓ Loaded {} from unified metadata storage", platform);

            // 处理并缓存该平台数据
            let filtered_data = SmartFilter::process_and_save_single(platform, &platform_data)
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
        Ok(None) => {}
        // 数据库故障不是正常的缓存未命中：记录后按原有策略降级到 raw 文件。
        Err(error) => {
            tracing::warn!(
                platform,
                error = %error,
                "Failed to load platform metadata; trying raw file"
            );
        }
    }

    // 5. FALLBACK: 从平台特定的raw文件读取数据
    let raw_cache_path = crate::services::data_paths::platform_raw_file(platform);
    if !raw_cache_path.exists() {
        return Err("Raw data file not found".to_string());
    }

    tracing::info!("⚙️  Processing {} from raw data...", platform);

    let content = fs::read_to_string(&raw_cache_path).map_err(|e| e.to_string())?;
    let platform_data: Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;

    // 6. 处理并缓存该平台数据（只处理单个平台！）
    let filtered_data =
        SmartFilter::process_and_save_single(platform, &platform_data).map_err(|e| {
            tracing::error!(platform, error = %e, "Failed to process platform data");
            "Failed to process platform data".to_string()
        })?;

    tracing::info!("✓ Successfully processed and cached {}", platform);
    Ok(filtered_data)
}

pub(crate) async fn last_stored_report_locale(
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
    if let Some(tag) = row.and_then(|r| {
        crate::api::reports::locale::locale_from_stored_report(&r.report).map(str::to_string)
    }) {
        return tag;
    }
    if let Some(tag) = crate::api::reports::locale::locale_from_user(db, user_id).await {
        return tag.to_string();
    }
    crate::api::reports::locale::DEFAULT_AUTO_REGEN_LOCALE.to_string()
}
