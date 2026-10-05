//! Sanitizing and merging the visual profile a persona write carries.

use super::*;

/// Site assets only. The public face must not be able to point off-site, so a
/// scheme, host, or traversal is refused rather than quietly rewritten.
/// Accepts a same-origin path (`/uploads/face.png`) or a bare asset id.
pub(super) fn sanitize_portrait_asset_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.is_empty() {
        return Some(String::new());
    }
    if value.len() > 512
        || value.contains(':')
        || value.contains("..")
        || value.starts_with("//")
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let bare_id = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if value.starts_with('/') || bare_id {
        Some(value.to_string())
    } else {
        None
    }
}

pub(super) fn sanitize_structured_persona(
    name: &str,
    value: &Value,
    visual_profile: Option<&Value>,
) -> Result<Value, HttpError> {
    let language = visual_profile
        .and_then(|profile| profile.get("language"))
        .and_then(Value::as_str)
        .unwrap_or("en-US");
    let fallback = myriad_merope::fallback_persona_draft(name, language, &[]);
    let persona = myriad_merope::sanitize_persona_draft(value, &fallback)
        .filter(myriad_merope::persona_draft_is_complete)
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Structured persona is incomplete",
                    "code": "persona_contract_invalid"
                })),
            ))
        })?;
    Ok(persona)
}

pub(super) fn required_visual_gender(value: &str) -> Option<&str> {
    match value.trim() {
        gender @ ("female" | "male" | "nonbinary" | "unspecified") => Some(gender),
        _ => None,
    }
}

pub(super) fn required_visual_language(value: &str) -> Option<&'static str> {
    let language = value.trim();
    if language.is_empty() {
        return None;
    }
    let lower = language.to_ascii_lowercase().replace('_', "-");
    if lower.starts_with("zh-tw")
        || lower.starts_with("zh-hk")
        || lower.starts_with("zh-mo")
        || lower.contains("hant")
    {
        Some("zh-TW")
    } else if lower.starts_with("zh") {
        Some("zh-CN")
    } else if lower.starts_with("ja") {
        Some("ja-JP")
    } else if lower.starts_with("en") {
        Some("en-US")
    } else {
        None
    }
}

/// A worn set's id as sent: absent keeps what was stored, `null` takes the
/// set off, anything else must be a plain id.
fn sanitize_active_set(
    source: &Map<String, Value>,
    key: &'static str,
    profile: &mut Map<String, Value>,
) -> Result<(), HttpError> {
    let Some(active) = source.get(key) else {
        return Ok(());
    };
    if active.is_null() {
        profile.insert(key.into(), Value::Null);
    } else {
        let id = match active.as_str().map(str::trim) {
            None => {
                return Err(visual_profile_issue(
                    myriad_merope::VisualProfileIssue::new(
                        key,
                        myriad_merope::VisualProfileReason::Invalid,
                    ),
                ));
            }
            Some("") => {
                return Err(visual_profile_issue(
                    myriad_merope::VisualProfileIssue::new(
                        key,
                        myriad_merope::VisualProfileReason::Empty,
                    ),
                ));
            }
            Some(id) if id.chars().count() > myriad_merope::MAX_WARDROBE_ID_CHARS => {
                return Err(visual_profile_issue(
                    myriad_merope::VisualProfileIssue::new(
                        key,
                        myriad_merope::VisualProfileReason::TooLong {
                            max_chars: myriad_merope::MAX_WARDROBE_ID_CHARS,
                        },
                    ),
                ));
            }
            Some(id) if id.chars().any(char::is_control) => {
                return Err(visual_profile_issue(
                    myriad_merope::VisualProfileIssue::new(
                        key,
                        myriad_merope::VisualProfileReason::ControlChar,
                    ),
                ));
            }
            Some(id) => id,
        };
        profile.insert(key.into(), json!(id));
    }
    Ok(())
}

pub(super) fn sanitize_visual_profile(value: &Value) -> Result<Value, HttpError> {
    let source = value.as_object().ok_or_else(|| {
        visual_profile_issue(myriad_merope::VisualProfileIssue::new(
            "visualProfile",
            myriad_merope::VisualProfileReason::NotObject,
        ))
    })?;
    let mut profile = Map::new();
    if let Some(gender) = source.get("gender").and_then(Value::as_str) {
        if !matches!(gender, "female" | "male" | "nonbinary" | "unspecified") {
            return Err(visual_profile_issue(
                myriad_merope::VisualProfileIssue::new(
                    "gender",
                    myriad_merope::VisualProfileReason::Invalid,
                ),
            ));
        }
        profile.insert("gender".into(), json!(gender));
    }
    // 显式 `null` 是「清掉」，和 `visualIdentity` 同一套写法。
    // 没有它的话前端无法清除这个字段：`merge_visual_profile` 会把缺席的键从
    // 旧值补上，于是上一次生成挑的服装风格会一直粘在后来的人设身上。
    if source.get("clothingStyle").is_some_and(Value::is_null) {
        profile.insert("clothingStyle".into(), Value::Null);
    } else if let Some(clothing_style) = source.get("clothingStyle").and_then(Value::as_str) {
        let clothing_style =
            myriad_merope::normalize_clothing_style(clothing_style).ok_or_else(|| {
                visual_profile_issue(myriad_merope::VisualProfileIssue::new(
                    "clothingStyle",
                    myriad_merope::VisualProfileReason::UnknownStyle,
                ))
            })?;
        profile.insert("clothingStyle".into(), json!(clothing_style));
    }
    if let Some(language) = source.get("language").and_then(Value::as_str) {
        profile.insert(
            "language".into(),
            json!(normalize_signals_language(language)),
        );
    }
    for (key, max_chars) in [
        ("extraRequirements", myriad_merope::MAX_VISUAL_NOTES_CHARS),
        (
            "personaExtraRequirements",
            myriad_merope::MAX_VISUAL_NOTES_CHARS,
        ),
    ] {
        if let Some(text) = source.get(key).and_then(Value::as_str) {
            let text = sanitize_visual_text(text, max_chars, key)?;
            profile.insert(key.into(), json!(text));
        }
    }
    if let Some(tags) = source.get("sourceTags").and_then(Value::as_array) {
        let tags = tags
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let tags = merope::api::report_dna::sanitize_onboarding_tags(&tags);
        profile.insert("sourceTags".into(), json!(tags));
    }
    if let Some(wardrobe) = source.get("wardrobe") {
        if wardrobe.is_null() {
            profile.insert("wardrobe".into(), json!([]));
        } else {
            let items = myriad_merope::sanitize_wardrobe_checked(wardrobe)
                .map_err(|issue| visual_profile_issue(issue.prefixed("wardrobe")))?;
            profile.insert("wardrobe".into(), json!(items));
        }
    }
    sanitize_active_set(source, "activeOutfitId", &mut profile)?;
    sanitize_active_set(source, "activeFullBodyOutfitId", &mut profile)?;
    if let Some(identity) = source.get("visualIdentity") {
        if identity.is_null() {
            profile.insert("visualIdentity".into(), Value::Null);
            profile.entry("wardrobe".to_string()).or_insert(json!([]));
            for key in ["activeOutfitId", "activeFullBodyOutfitId"] {
                profile.entry(key.to_string()).or_insert(Value::Null);
            }
            drop_stale_active_outfit(&mut profile);
            return Ok(Value::Object(profile));
        }
        let mut sanitized = myriad_merope::sanitize_upper_body_visual_identity_checked(identity)
            .map_err(|issue| visual_profile_issue(issue.prefixed("visualIdentity")))?;
        if let Some(style) = profile
            .get("clothingStyle")
            .and_then(Value::as_str)
            .and_then(myriad_merope::normalize_clothing_style)
            .or_else(|| myriad_merope::clothing_style_of(&sanitized))
        {
            profile.insert("clothingStyle".into(), json!(style));
            myriad_merope::stamp_clothing_style(&mut sanitized, style);
        }
        let mut sanitized = myriad_merope::normalize_visual_identity_for_prompt_checked(&sanitized)
            .map_err(|issue| visual_profile_issue(issue.prefixed("visualIdentity")))?;
        if let Some(gender) = profile.get("gender").and_then(Value::as_str) {
            let language = profile
                .get("language")
                .and_then(Value::as_str)
                .unwrap_or("en-US");
            if let Some(fixed) =
                myriad_merope::ensure_visual_identity_states_gender(&sanitized, gender, language)
            {
                sanitized = fixed;
            }
        }
        profile.insert("visualIdentity".into(), sanitized);
    }
    drop_stale_active_outfit(&mut profile);
    Ok(Value::Object(profile))
}

/// The worn bust must be a bust in the wardrobe, and the worn full body a
/// full-body set there: the two are worn apart, and a full-body set is never
/// worn on the panel.
pub(super) fn drop_stale_active_outfit(profile: &mut Map<String, Value>) {
    for (key, full_body) in [("activeOutfitId", false), ("activeFullBodyOutfitId", true)] {
        let Some(id) = profile.get(key).and_then(Value::as_str) else {
            continue;
        };
        let known = profile
            .get("wardrobe")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("id").and_then(Value::as_str) == Some(id)
                        && myriad_merope::is_full_body_item(item) == full_body
                })
            });
        if !known {
            profile.insert(key.into(), Value::Null);
        }
    }
}

pub(super) fn finish_wardrobe(mut profile: Value, previous: Option<&Map<String, Value>>) -> Value {
    myriad_merope::ensure_default_wardrobe(&mut profile, previous);
    myriad_merope::reconcile_wardrobe_rigs(&mut profile, previous);
    if let Some(map) = profile.as_object_mut() {
        drop_stale_active_outfit(map);
    }
    profile
}

pub(super) fn merge_visual_profile(incoming: Value, previous: Option<&Value>) -> Value {
    let Some(previous) = previous.and_then(Value::as_object) else {
        return finish_wardrobe(incoming, None);
    };
    let Some(target) = incoming.as_object() else {
        return incoming;
    };
    let mut merged = target.clone();
    let identity_context_changed = ["gender", "clothingStyle"]
        .iter()
        .any(|key| merged.get(*key).is_some() && merged.get(*key) != previous.get(*key));
    let identity_cleared = merged.get("visualIdentity").is_some_and(Value::is_null);
    for key in [
        "visualIdentity",
        "sourceTags",
        "personaExtraRequirements",
        "clothingStyle",
        "wardrobe",
        "activeOutfitId",
        "activeFullBodyOutfitId",
    ] {
        if key == "visualIdentity" && identity_context_changed {
            continue;
        }
        if matches!(
            key,
            "wardrobe" | "activeOutfitId" | "activeFullBodyOutfitId"
        ) && identity_cleared
        {
            continue;
        }
        if merged.get(key).is_none() {
            if let Some(value) = previous.get(key) {
                merged.insert(key.to_string(), value.clone());
            }
        }
    }
    finish_wardrobe(Value::Object(merged), Some(previous))
}

pub(super) fn sanitize_visual_text(
    value: &str,
    max_chars: usize,
    field: &str,
) -> Result<String, HttpError> {
    let value = value.trim();
    if value.chars().count() > max_chars {
        return Err(visual_profile_issue(
            myriad_merope::VisualProfileIssue::new(
                field,
                myriad_merope::VisualProfileReason::TooLong { max_chars },
            ),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(visual_profile_issue(
            myriad_merope::VisualProfileIssue::new(
                field,
                myriad_merope::VisualProfileReason::ControlChar,
            ),
        ));
    }
    Ok(value.to_string())
}

pub(super) fn visual_profile_issue(issue: myriad_merope::VisualProfileIssue) -> HttpError {
    tracing::warn!(
        field = %issue.field,
        reason = issue.reason.as_str(),
        "visual profile rejected"
    );
    HttpError::from((
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "Visual profile is invalid",
            "code": "visual_profile_invalid",
            "message": issue.message(),
        })),
    ))
}
