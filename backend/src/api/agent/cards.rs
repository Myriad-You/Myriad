//! Her cards before the site owner, answered on the site
//! (`merope::cards`). Site owner only.
//!
//! - `POST /api/agent/cards/{card}/answer` `{version, action}`: the owner
//!   chose `action` on `version` of the card. Kept once however often it
//!   comes. Answers with `{kept, code?, card?}` as the link's
//!   `card.answered` does (`card_stale`: an older version, kept as the
//!   answer to what they saw then), and the card as it now stands.
//!
//! Errors say what was refused by a code, never with what.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::post,
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::agent::merope::cards;

fn refused(status: StatusCode, code: &str) -> HttpError {
    HttpError::from((status, Json(json!({ "error": code, "code": code }))))
}

#[derive(Debug, Deserialize)]
struct Answer {
    version: String,
    action: String,
}

/// POST /api/agent/cards/{card}/answer
async fn answer(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(card): Path<String>,
    Json(answer): Json<Answer>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    let owner = super::helpers::parse_user_id(&claims)?;
    if !myriad_nodes::cards::id_ok(&card)
        || !myriad_nodes::cards::word(&answer.action)
        || answer.version.is_empty()
        || answer.version.len() > 64
    {
        return Err(refused(StatusCode::NOT_FOUND, "card_unknown"));
    }
    let answered = cards::answer(&db, owner, &card, &answer.version, &answer.action)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[cards] an answer to a card was not kept");
            refused(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        })?;
    if !answered.kept {
        let code = answered.code.unwrap_or("card_unknown");
        let status = match code {
            "card_unknown" => StatusCode::NOT_FOUND,
            _ => StatusCode::CONFLICT,
        };
        return Err(refused(status, code));
    }
    let mut body = json!({ "kept": true });
    if let Some(code) = answered.code {
        body["code"] = code.into();
    }
    if let Some(now) = answered.now {
        body["card"] = serde_json::to_value(now).unwrap_or(Value::Null);
    }
    Ok(Json(body))
}

pub(super) fn routes(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    let auth = || from_fn_with_state(app_state.clone(), crate::middleware::auth::auth_middleware);
    Router::<crate::state::AppState>::new()
        .route("/cards/{card}/answer", post(answer).route_layer(auth()))
}
