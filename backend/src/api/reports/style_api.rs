//! Report style API, admin only: read and save the owner's styles, and try a
//! style on one chapter without storing anything.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{Extension, Json, http::HeaderMap};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::platform_id::PlatformId;
use crate::services::report_styles::{
    CHAPTER_KEYS, MAX_AVOID_CHARS, MAX_AVOID_ITEMS, MAX_CUSTOM_STYLES, MAX_NAME_CHARS,
    MAX_SAMPLE_CHARS, MAX_TONE_CHARS, REPORT_STYLES_KEY, ReportStyle, ReportStylesConfig,
    StyleRejection, builtin_styles, chapter_meaning, load_report_styles,
};
use myriad_error::AppError;

/// A preview stays good this long for the same platform, style text, chapter
/// and locale: trying a style twice does not pay twice.
const PREVIEW_TTL: Duration = Duration::from_secs(10 * 60);
const PREVIEW_CACHE_ENTRIES: usize = 32;

type PreviewKey = (String, String, String, String);

static PREVIEW_CACHE: Lazy<Mutex<HashMap<PreviewKey, (Instant, Value)>>> =
    Lazy::new(Default::default);

/// One preview at a time for the whole site: each is one or two Standard
/// model calls, and only the owner can start one.
static PREVIEW_GATE: Lazy<tokio::sync::Mutex<()>> = Lazy::new(Default::default);

fn cached_preview(key: &PreviewKey) -> Option<Value> {
    let mut cache = PREVIEW_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|_, (at, _)| at.elapsed() < PREVIEW_TTL);
    cache.get(key).map(|(_, body)| body.clone())
}

fn remember_preview(key: PreviewKey, body: &Value) {
    let mut cache = PREVIEW_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= PREVIEW_CACHE_ENTRIES {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (at, _))| *at)
            .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(key, (Instant::now(), body.clone()));
}

fn rejected(rejection: StyleRejection) -> HttpError {
    HttpError(
        AppError::bad_request("Invalid report style")
            .with_code(rejection.code)
            .with_message(rejection.detail),
    )
}

fn styles_body(config: &ReportStylesConfig) -> Value {
    let current = config.current_style();
    json!({
        "success": true,
        "current": config.current,
        "current_hash": current.hash(),
        "revision": config.revision(),
        "builtin": builtin_styles(),
        "custom": config.custom,
        "chapter_keys": CHAPTER_KEYS
            .iter()
            .map(|(key, meaning)| json!({ "key": key, "meaning": meaning }))
            .collect::<Vec<_>>(),
        "limits": {
            "custom_styles": MAX_CUSTOM_STYLES,
            "name_chars": MAX_NAME_CHARS,
            "tone_chars": MAX_TONE_CHARS,
            "sample_chars": MAX_SAMPLE_CHARS,
            "avoid_items": MAX_AVOID_ITEMS,
            "avoid_chars": MAX_AVOID_CHARS,
        },
    })
}

/// GET /api/reports/styles
pub async fn get_report_styles(
    crate::extract::Db(db): crate::extract::Db,
) -> Result<Json<Value>, HttpError> {
    Ok(Json(styles_body(&load_report_styles(&db).await)))
}

/// Serializes style saves; see [`update_report_styles`].
static SAVE_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(Default::default);

/// PUT body: the whole custom list and current id, plus the `revision` the
/// editor read them at.
#[derive(Debug, Deserialize)]
pub struct UpdateReportStylesRequest {
    pub base_revision: String,
    #[serde(flatten)]
    pub styles: ReportStylesConfig,
}

/// PUT /api/reports/styles — the whole custom list plus the current id. A
/// save edited from an older `revision` is refused with 409
/// (`report_styles_conflict`), so a second tab cannot overwrite the first.
pub async fn update_report_styles(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<UpdateReportStylesRequest>,
) -> Result<Json<Value>, HttpError> {
    let UpdateReportStylesRequest {
        base_revision,
        styles,
    } = payload;
    styles.validate().map_err(rejected)?;
    let config = styles.normalized();
    // Check-and-write under one lock: two saves from the same revision cannot
    // both pass the check. The backend is one process per site.
    let _saving = SAVE_LOCK.lock().await;
    let stored = load_report_styles(&db).await;
    if stored.revision() != base_revision.trim() {
        return Err(HttpError(
            AppError::conflict("Report styles changed since they were read")
                .with_details(json!({ "revision": stored.revision() })),
        ));
    }
    let value = serde_json::to_value(&config)
        .map_err(|_| HttpError(AppError::internal("Failed to save report styles")))?;
    crate::services::config_service::ConfigService::new(db)
        .update_config(REPORT_STYLES_KEY, value)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to save report styles");
            HttpError(AppError::internal("Failed to save report styles"))
        })?;
    Ok(Json(styles_body(&config)))
}

#[derive(Debug, Deserialize)]
pub struct StylePreviewRequest {
    pub platform: String,
    /// Chapter to write; defaults to the style's first chapter.
    #[serde(default)]
    pub chapter: Option<String>,
    /// An unsaved draft to try. Wins over `style_id`.
    #[serde(default)]
    pub style: Option<ReportStyle>,
    /// A saved or built-in style; defaults to the current one.
    #[serde(default)]
    pub style_id: Option<String>,
}

/// POST /api/reports/styles/preview — write one chapter in a style. Nothing is
/// stored; the model call is billed like a report (source `reports`). The
/// same try within ten minutes is answered from memory; a second preview
/// while one runs is refused.
pub async fn preview_report_style(
    crate::extract::Db(db): crate::extract::Db,
    Extension(claims): Extension<Claims>,
    headers: HeaderMap,
    Json(req): Json<StylePreviewRequest>,
) -> Result<Json<Value>, HttpError> {
    let platform = req.platform.trim().to_ascii_lowercase();
    if !PlatformId::ALL.iter().any(|id| id.slug() == platform) {
        return Err(HttpError(AppError::bad_request("Unknown report platform")));
    }
    let config = load_report_styles(&db).await;
    let style = match (req.style, req.style_id) {
        (Some(draft), _) => {
            let draft = draft.normalized();
            if draft.name.is_empty() {
                return Err(rejected(StyleRejection {
                    code: "report_style_invalid",
                    detail: "name must be 1-24 characters".into(),
                }));
            }
            draft
        }
        (None, Some(id)) => config.find(id.trim()).ok_or_else(|| {
            rejected(StyleRejection {
                code: "report_style_not_found",
                detail: format!("no style {}", id.trim()),
            })
        })?,
        (None, None) => config.current_style(),
    };
    let chapter = match req.chapter.map(|key| key.trim().to_ascii_lowercase()) {
        Some(key) if chapter_meaning(&key).is_some() => key,
        Some(key) => {
            return Err(rejected(StyleRejection {
                code: "report_style_invalid",
                detail: format!("unknown chapter key {key}"),
            }));
        }
        None => style
            .chapters
            .first()
            .cloned()
            .unwrap_or_else(|| "portrait".to_string()),
    };

    let actor_id = claims
        .durable_user_id()
        .ok_or_else(|| HttpError(AppError::unauthorized("Unauthorized")))?;
    let user_id = super::generate::report_storage_user_id(&db, actor_id).await;
    let locale = match super::locale::locale_from_headers(&headers) {
        Some(tag) => tag.to_string(),
        None => super::generate::last_stored_report_locale(&db, user_id, &platform).await,
    };
    let key: PreviewKey = (
        platform.clone(),
        style.hash(),
        chapter.clone(),
        locale.clone(),
    );
    if let Some(body) = cached_preview(&key) {
        return Ok(Json(body));
    }
    let Ok(_running) = PREVIEW_GATE.try_lock() else {
        return Err(HttpError(AppError::too_many_requests(
            "A report style preview is already running",
        )));
    };

    let metadata = super::generate::get_platform_data(&platform, &db, user_id)
        .await
        .map_err(|_| HttpError(AppError::not_found("No platform data for this report")))?;
    let facts = super::facts::build(&platform, &metadata).await;

    let request = super::generate::write::WriteRequest {
        metadata: &metadata,
        platform: &platform,
        locale: &locale,
        style: &style,
        facts: &facts.facts,
        only: Some(&chapter),
        purpose: super::generate::write::Purpose::StylePreview,
    };
    use super::generate::write::WriteError;
    let report = match super::generate::write::ask_model(&request).await {
        Ok(report) => report,
        Err(WriteError::NoModel) => {
            return Err(HttpError(AppError::service_unavailable(
                "Report model is not configured",
            )));
        }
        Err(WriteError::CallFailed) => {
            return Err(HttpError(AppError::bad_gateway(
                "The report model did not write a usable chapter",
            )));
        }
        Err(WriteError::Rejected(problems)) => {
            return Err(HttpError(
                AppError::bad_gateway("The report model did not write a usable chapter")
                    .with_details(json!({ "problems": problems })),
            ));
        }
    };

    let body = json!({
        "success": true,
        "platform": platform,
        "locale": locale,
        "style": super::generate::StyleStamp::of(&style),
        "headline": report.script.headline,
        "chapter": report.script.chapters.into_iter().next(),
        "facts": super::facts::proxied_facts(&facts.facts),
    });
    remember_preview(key, &body);
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_names_the_revision_it_was_edited_from() {
        let body: UpdateReportStylesRequest = serde_json::from_value(json!({
            "base_revision": "0011223344556677",
            "current": "default",
            "custom": []
        }))
        .unwrap();
        assert_eq!(body.base_revision, "0011223344556677");
        assert_eq!(body.styles, ReportStylesConfig::default());
        assert!(
            serde_json::from_value::<UpdateReportStylesRequest>(json!({ "current": "default" }))
                .is_err(),
            "a save without base_revision cannot be checked"
        );
    }

    #[test]
    fn a_preview_is_answered_from_memory_and_the_cache_stays_small() {
        let key = |n: usize| -> PreviewKey {
            (
                format!("test-{n}"),
                "hash".into(),
                "taste".into(),
                "zh-CN".into(),
            )
        };
        remember_preview(key(0), &json!({ "headline": "h0" }));
        assert_eq!(cached_preview(&key(0)).unwrap()["headline"], "h0");
        assert!(cached_preview(&key(999)).is_none());
        for n in 1..=PREVIEW_CACHE_ENTRIES + 4 {
            remember_preview(key(n), &json!({ "headline": n }));
        }
        let len = PREVIEW_CACHE.lock().unwrap().len();
        assert!(len <= PREVIEW_CACHE_ENTRIES, "{len}");
        assert!(cached_preview(&key(PREVIEW_CACHE_ENTRIES + 4)).is_some());
    }
}
