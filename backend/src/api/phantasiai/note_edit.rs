//! Stateless editing: only an explicit editor action applies the returned draft.
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use myriad_phantasi_notes::{
    ai_edit::{SYSTEM_PROMPT, build_prompt, validate_result},
    render_markdown_preview,
};
use serde_json::{Value, json};

fn failure(status: StatusCode, code: &str) -> Response {
    (
        status,
        Json(json!({"success":false,"code":code,"error":code})),
    )
        .into_response()
}

pub(super) async fn edit_note(
    _admin: crate::extract::AdminClaims,
    Json(request): Json<Value>,
) -> Response {
    let prompt = match build_prompt(&request) {
        Ok(prompt) => prompt,
        Err(code) => return failure(StatusCode::BAD_REQUEST, code),
    };
    let Some(analyzer) =
        super::create_ai_analyzer_for_tier(crate::config::ModelTier::Standard).await
    else {
        return failure(StatusCode::CONFLICT, "ai_not_configured");
    };
    let schema = json!({"type":"object","properties":{"replacement":{"type":"string"},"complete":{"type":"boolean"}},"required":["replacement","complete"],"additionalProperties":false});
    let owner = match crate::services::ai_cost_ledger::resolve_site_owner_id().await {
        Ok(owner) => owner,
        Err(_) => return failure(StatusCode::INTERNAL_SERVER_ERROR, "ai_billing_owner"),
    };
    let call = crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "phantasiai",
        "note_edit",
        analyzer.analyze_json(SYSTEM_PROMPT, &prompt, "note_edit", Some(&schema)),
    );
    let response = match tokio::time::timeout(std::time::Duration::from_secs(240), call).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            // Provider errors may contain request payloads or credentials. Only classify here.
            let message = error.to_string().to_ascii_lowercase();
            let context_limit = [
                "context_length",
                "context window",
                "context length",
                "too many tokens",
                "token limit",
                "maximum context",
                "input too long",
                "request too large",
            ]
            .iter()
            .any(|part| message.contains(part));
            return failure(
                StatusCode::BAD_GATEWAY,
                if context_limit {
                    "note_ai_context_limit"
                } else {
                    "note_ai_failed"
                },
            );
        }
        Err(_) => return failure(StatusCode::GATEWAY_TIMEOUT, "note_ai_timeout"),
    };
    match validate_result(&request, &response) {
        Ok(content_md) => {
            let html = render_markdown_preview(&content_md);
            (
                StatusCode::OK,
                Json(json!({"content_md":content_md,"html":html})),
            )
                .into_response()
        }
        Err(code) => failure(StatusCode::UNPROCESSABLE_ENTITY, code),
    }
}
