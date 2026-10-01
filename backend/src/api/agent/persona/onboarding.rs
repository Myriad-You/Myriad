//! Onboarding endpoints: naming, drafting and importing a persona, its visual design, and report signals.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPersonaRequest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub extra_requirements: String,
    #[serde(default = "default_signals_language")]
    pub language: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPersonaRequest {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub gender: String,
    #[serde(default = "default_signals_language")]
    pub language: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestNameRequest {
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub avoid_name: Option<String>,
    #[serde(default)]
    pub name_style: String,
    #[serde(default = "default_signals_language")]
    pub language: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuggestVisualDesignRequest {
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub visual_requirements: String,
    #[serde(default)]
    pub clothing_style: String,
    #[serde(default)]
    pub keep_character: bool,
    #[serde(default)]
    pub regenerate: bool,
    #[serde(default)]
    pub existing_visual_identity: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSignalsRequest {
    pub(super) consent: bool,
    #[serde(default = "default_signals_language")]
    pub(super) language: String,
    #[serde(default)]
    pub(super) regenerate: bool,
}

pub(super) fn default_signals_language() -> String {
    "en-US".to_string()
}

pub(super) fn normalize_signals_language(raw: &str) -> &'static str {
    crate::api::reports::locale::normalize_report_locale(raw)
}

pub(super) async fn report_platform_count(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<usize, HttpError> {
    merope::api::report_dna::count_report_platforms(db, user_id)
        .await
        .map_err(|error| persona_store_http("count persona reports", error))
}

pub(super) async fn require_persona_reports(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<usize, HttpError> {
    let count = report_platform_count(db, user_id).await?;
    if count < merope::api::report_dna::MIN_PERSONA_REPORTS {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Need at least 3 platform reports",
                "code": "persona_reports_required",
                "reportCount": count,
                "required": merope::api::report_dna::MIN_PERSONA_REPORTS,
            })),
        )));
    }
    Ok(count)
}

/// POST /api/agent/persona/signals
/// Distill spoken personality tags from the owner's latest reports. No visual assets.
pub async fn report_signals(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<ReportSignalsRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = require_site_owner(&claims, &db).await?;
    require_persona_reports(&db, user_id).await?;
    if !request.consent {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Explicit consent is required",
                "code": "consent_required"
            })),
        )));
    }
    let language = normalize_signals_language(&request.language);
    let distilled =
        merope::api::report_dna::distill_report_dna(&db, user_id, language, request.regenerate)
            .await
            .map_err(distill_error)?;
    Ok(Json(json!({
        "reportCount": distilled.report_count,
        "tags": distilled.tags,
        "aiDistilled": distilled.ai_distilled,
    })))
}

pub(super) fn distill_error(error: merope::api::report_dna::DistillReportDnaError) -> HttpError {
    match error {
        merope::api::report_dna::DistillReportDnaError::Db(error) => {
            persona_store_http("load persona reports", error)
        }
    }
}

/// POST /api/agent/persona/name
/// Strict Lite rolls one given name in the selected style. Tags stay out.
pub async fn suggest_name(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<SuggestNameRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = require_site_owner(&claims, &db).await?;
    require_persona_reports(&db, user_id).await?;
    let language = normalize_signals_language(&body.language);
    match merope::api::onboarding_ai::suggest_display_name(
        &body.gender,
        body.avoid_name.as_deref(),
        language,
        &body.name_style,
    )
    .await
    {
        Ok(name) => Ok(Json(json!({
            "name": name,
        }))),
        Err(error) => Err(onboarding_generation_error(
            "name",
            "Failed to suggest a name",
            error,
        )),
    }
}

/// POST /api/agent/persona/draft
/// Pro writes a structured character persona. No appearance, room, or clothes.
pub async fn draft_persona(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<DraftPersonaRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let user_id = require_site_owner(&claims, &db).await?;
    require_persona_reports(&db, user_id).await?;
    let language = normalize_signals_language(&body.language);
    let tags = merope::api::report_dna::sanitize_onboarding_tags_for_language(&body.tags, language);
    let name = body.name.trim();
    let display = if name.is_empty() { "Arael" } else { name };
    let persona = match merope::api::onboarding_ai::suggest_persona(
        display,
        language,
        &tags,
        &body.gender,
        &body.extra_requirements,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Err(onboarding_generation_error(
                "persona",
                "Failed to draft a persona",
                error,
            ));
        }
    };
    Ok(Json(json!({
        "persona": persona,
    })))
}

/// POST /api/agent/persona/import
/// Pro rewrites an owner-supplied write-up into the structured persona fields.
pub async fn import_persona(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<ImportPersonaRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let _user_id = require_site_owner(&claims, &db).await?;
    let language = normalize_signals_language(&body.language);
    let source = body.source.trim();
    if source.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Import source is required",
                "code": "import_source_required"
            })),
        )));
    }
    let name = body.name.trim();
    let display = if name.is_empty() { "Arael" } else { name };
    let persona = match merope::api::onboarding_ai::import_persona(
        display,
        language,
        &body.gender,
        source,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Err(onboarding_generation_error(
                "persona",
                "Failed to import a persona",
                error,
            ));
        }
    };
    Ok(Json(json!({
        "persona": persona,
    })))
}

/// POST /api/agent/persona/visual-design
/// Pro turns the saved persona into one complete upper-body visual identity.
/// The suggestion is returned for owner review and is not persisted here.
pub async fn suggest_visual_design(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<SuggestVisualDesignRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let _user_id = require_site_owner(&claims, &db).await?;
    let persona = merope::get_persona(&db)
        .await
        .map_err(|error| persona_store_http("load persona", error))?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Save the persona before designing appearance",
                    "code": "persona_required"
                })),
            ))
        })?;
    let structured = persona.persona_json.as_ref().ok_or_else(|| {
        HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Structured persona is required before visual design",
                "code": "structured_persona_required"
            })),
        ))
    })?;
    if !myriad_merope::persona_draft_is_complete(structured) {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Structured persona is incomplete",
                "code": "persona_contract_invalid"
            })),
        )));
    }
    let language = required_visual_language(&body.language).ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a supported interface language before generating the visual design",
                "code": "visual_language_required"
            })),
        ))
    })?;
    let gender = required_visual_gender(&body.gender).ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a valid gender presentation before generating the visual design",
                "code": "gender_required"
            })),
        ))
    })?;
    let requirements = myriad_merope::normalize_visual_requirements_for_design_with_gender(
        &sanitize_visual_text(
            &body.visual_requirements,
            myriad_merope::MAX_VISUAL_NOTES_CHARS,
            "visualRequirements",
        )?,
        gender,
    );
    let clothing_style =
        myriad_merope::normalize_clothing_style(&body.clothing_style).ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Choose a clothing style before generating the visual design",
                    "code": "clothing_style_required"
                })),
            ))
        })?;
    let explicit_existing = match body.existing_visual_identity.as_ref() {
        Some(value) => {
            let sanitized =
                myriad_merope::sanitize_upper_body_visual_identity(value).ok_or_else(|| {
                    HttpError::from((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "Existing visual identity is incomplete",
                            "code": "visual_identity_invalid"
                        })),
                    ))
                })?;
            Some(
                myriad_merope::normalize_visual_identity_for_prompt_checked(&sanitized)
                    .map_err(|issue| visual_profile_issue(issue.prefixed("visualIdentity")))?,
            )
        }
        None => None,
    };
    let existing =
        (body.regenerate || body.keep_character).then(|| explicit_existing.unwrap_or(Value::Null));
    if body.keep_character && existing.as_ref().is_none_or(Value::is_null) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "An explicit existing visual identity is required to keep the character",
                "code": "visual_identity_invalid"
            })),
        )));
    }
    let identity = match merope::api::onboarding_ai::suggest_visual_design(
        persona.name.trim(),
        language,
        structured,
        gender,
        clothing_style,
        &requirements,
        existing.as_ref(),
        body.regenerate,
        body.keep_character,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return Err(onboarding_generation_error(
                "visual",
                "Failed to design upper-body appearance",
                error,
            ));
        }
    };
    Ok(Json(json!({
        "visualIdentity": identity,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservePortraitVisualRequest {
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub language: String,
}

/// POST /api/agent/persona/visual-from-portrait
/// Pro reads the stored master portrait into visualIdentity + clothingStyle.
/// Suggestion is returned for the import finish write; nothing is persisted here.
pub async fn observe_visual_from_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<ObservePortraitVisualRequest>,
) -> Result<Json<Value>, HttpError> {
    require_merope_enabled().await?;
    let _user_id = require_site_owner(&claims, &db).await?;
    let language = required_visual_language(&body.language).ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a supported interface language before reading the portrait",
                "code": "visual_language_required"
            })),
        ))
    })?;
    let gender = required_visual_gender(&body.gender).ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Choose a valid gender presentation before reading the portrait",
                "code": "gender_required"
            })),
        ))
    })?;
    let persona = merope::get_persona(&db)
        .await
        .map_err(|error| persona_store_http("load persona", error))?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Upload a master portrait before reading visual features",
                    "code": "portrait_required"
                })),
            ))
        })?;
    let portrait_url = persona.portrait_asset_id.as_deref().ok_or_else(|| {
        HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Upload a master portrait before reading visual features",
                "code": "portrait_required"
            })),
        ))
    })?;
    // An uploaded portrait lives in the media store (`/media/assets/...`), not the
    // image cache, so read it through the loader that covers both. Reading the
    // cache directly reports an uploaded portrait as missing.
    let image = crate::services::image_generation::load_local_reference(portrait_url)
        .await
        .map_err(|error| {
            tracing::error!(%error, "imported portrait bytes missing");
            HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Uploaded master portrait is missing from storage",
                    "code": "portrait_required"
                })),
            ))
        })?;
    let observed =
        match merope::api::onboarding_ai::observe_visual_from_portrait(language, gender, &image).await {
            Ok(value) => value,
            Err(error) => {
                return Err(onboarding_generation_error(
                    "visual",
                    "Failed to read visual features from the portrait",
                    error,
                ));
            }
        };
    Ok(Json(json!({
        "visualIdentity": observed.visual_identity,
        "clothingStyle": observed.clothing_style,
    })))
}

pub(super) fn onboarding_error_body(
    error: &str,
    code: &str,
    message: Option<&str>,
) -> serde_json::Value {
    let mut body = json!({ "error": error, "code": code });
    if let Some(message) = message.map(str::trim).filter(|value| !value.is_empty()) {
        if message != error {
            body["message"] = json!(message);
        }
    }
    body
}

pub(super) fn onboarding_generation_error(
    kind: &str,
    failed_message: &str,
    error: merope::api::onboarding_ai::OnboardingAiError,
) -> HttpError {
    use merope::api::onboarding_ai::OnboardingAiError;
    tracing::error!(%error, kind, "onboarding generation failed");
    let detail = error.public_detail().map(str::to_string);
    match error {
        OnboardingAiError::AnalyzerUnavailable => HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(onboarding_error_body(
                if kind == "name" {
                    "Lite model is unavailable"
                } else {
                    "Pro model is unavailable"
                },
                if kind == "name" {
                    "lite_unavailable"
                } else {
                    "pro_unavailable"
                },
                None,
            )),
        )),
        OnboardingAiError::LanguageMismatch => HttpError::from((
            StatusCode::BAD_GATEWAY,
            Json(onboarding_error_body(
                "Visual design did not match the interface language",
                "visual_design_language",
                None,
            )),
        )),
        OnboardingAiError::UnusableResponse(_) => {
            let (label, code) = match kind {
                "name" => (
                    "The model returned a name without a usable meaning or script",
                    "name_unusable",
                ),
                "persona" => (
                    "The model returned an unusable persona draft",
                    "persona_unusable",
                ),
                _ => (
                    "The model returned an unusable visual design",
                    "visual_design_unusable",
                ),
            };
            HttpError::from((
                StatusCode::BAD_GATEWAY,
                Json(onboarding_error_body(label, code, detail.as_deref())),
            ))
        }
        OnboardingAiError::ProviderFailed(_) => {
            let code = match kind {
                "name" => "name_suggest_failed",
                "persona" => "persona_draft_failed",
                _ => "visual_design_failed",
            };
            HttpError::from((
                StatusCode::BAD_GATEWAY,
                Json(onboarding_error_body(
                    failed_message,
                    code,
                    detail.as_deref(),
                )),
            ))
        }
    }
}
