//! Agent handlers for site SEO / GEO capabilities.

use super::HandlerContext;
use crate::services::seo_copy::{GenerateSiteSeoRequest, generate_site_seo_copy_with_db};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(super) async fn execute_seo_inspect(ctx: &HandlerContext<'_>) -> Result<Value, String> {
    Ok(crate::services::public_site::public_geo_inspect(ctx.db).await)
}

pub(super) async fn execute_seo_generate(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let (title, description, ai_intro) =
        crate::services::public_site::site_branding_copy(ctx.db).await;
    let mut payload = GenerateSiteSeoRequest {
        site_title: string_param(params, "site_title"),
        site_description: string_param(params, "site_description"),
        hint: string_param(params, "hint"),
        language: string_param(params, "language"),
        fields: params
            .get("fields")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    };
    if payload.site_title.trim().is_empty() {
        payload.site_title = title;
    }
    if payload.site_description.trim().is_empty() {
        payload.site_description = description;
    }
    if payload.site_title.trim().is_empty() && payload.hint.trim().is_empty() {
        payload.hint = ai_intro;
    }

    let resp = generate_site_seo_copy_with_db(ctx.db, payload).await?;
    serde_json::to_value(resp).map_err(|error| {
        tracing::error!(%error, "seo.generate serialize failed");
        "Failed to encode generated SEO copy".to_string()
    })
}

pub(super) async fn execute_seo_apply(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let saved = crate::services::seo_review::apply_site_seo_fields(
        ctx.db,
        params.get("site_description").and_then(Value::as_str),
        params.get("site_keywords").and_then(Value::as_str),
        params.get("site_ai_intro").and_then(Value::as_str),
    )
    .await?;
    Ok(json!({ "ok": true, "saved": saved }))
}

fn string_param(params: &HashMap<String, Value>, key: &str) -> String {
    params
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
