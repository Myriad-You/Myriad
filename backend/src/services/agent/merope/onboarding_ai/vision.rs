//! Her upper-body visual design: drafting it from the persona, and reading it back off a portrait.

use super::*;

/// 带严格校验闸的生成都该自己重试；次数按这一条有多贵来定。
///
/// 起草人设有五道闸（不是 JSON / 丰满度 / 语言 / 文艺腔 / 清洗完整性），导入
/// 同样五道；视觉设定常开五道质量闸，新脸再加两道。模型在一次正常生成里踩中一道是常态。不重试就等于
/// 把重试写成给人看的提示，用户看到的是「不可用，请再试一次」。
///
/// 名字走严格 Lite、几十个 token，抽三次也很快；这三条是 Pro 的长文生成，一次
/// 几十秒，两次够把「这一把没写好」和「配置真有问题」分开，再多就是拿站长的
/// 时间换概率。
pub(super) const VISUAL_DESIGN_ATTEMPTS: u8 = 2;

pub async fn suggest_visual_design(
    name: &str,
    language: &str,
    persona: &Value,
    gender: &str,
    clothing_style: &str,
    visual_requirements: &str,
    existing_visual_identity: Option<&Value>,
    regenerate: bool,
    keep_character: bool,
) -> Result<Value, OnboardingAiError> {
    let persona_input = visual_design_persona_input(persona);
    retry_unusable(VISUAL_DESIGN_ATTEMPTS, "visual design was unusable", || {
        suggest_visual_design_once(
            name,
            language,
            &persona_input,
            gender,
            clothing_style,
            visual_requirements,
            existing_visual_identity,
            regenerate,
            keep_character,
        )
    })
    .await
}

pub(super) fn existing_outfit_palette_hint(identity: Option<&Value>) -> Value {
    let Some(root) = identity else {
        return Value::Null;
    };
    let palette = root
        .get("outfit")
        .and_then(|outfit| outfit.get("paletteHint"))
        .or_else(|| root.get("paletteHint"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty());
    match palette {
        Some(text) => json!(text.chars().take(340).collect::<String>()),
        None => Value::Null,
    }
}

pub(super) fn visual_design_variety(
    requirements_named: bool,
    keep_character: bool,
    remap_existing_palette: bool,
) -> Value {
    if requirements_named {
        json!({
            "keepNamedVisualRequirements": true,
            "clothingStyleIsFamilyNotKit": true,
            "fillSilenceFromGrammar": true,
        })
    } else if remap_existing_palette {
        json!({
            "clothingStyleIsFamilyNotKit": true,
            "keepExistingOutfitPalette": true,
            "remapExistingHuesOntoNewGarments": true,
            "changeConstructionNotJustColors": true,
            "forbidInterchangeableDefaultKit": true,
        })
    } else if keep_character {
        json!({
            "clothingStyleIsFamilyNotKit": true,
            "changeConstructionNotJustColors": true,
            "forbidInterchangeableDefaultKit": true,
        })
    } else {
        json!({
            "clothingStyleIsFamilyNotKit": true,
            "appliesToEveryStyle": true,
            "changeConstructionNotJustColors": true,
            "forbidInterchangeableDefaultKit": true,
        })
    }
}

pub(super) async fn suggest_visual_design_once(
    name: &str,
    language: &str,
    persona_input: &Value,
    gender: &str,
    clothing_style: &str,
    visual_requirements: &str,
    existing_visual_identity: Option<&Value>,
    regenerate: bool,
    keep_character: bool,
) -> Result<Value, OnboardingAiError> {
    let clothing_style = myriad_merope::normalize_clothing_style(clothing_style).ok_or(
        OnboardingAiError::UnusableResponse("clothing style is invalid"),
    )?;
    let clothing_grammar = myriad_merope::clothing_style_grammar(clothing_style).ok_or(
        OnboardingAiError::UnusableResponse("clothing style grammar is missing"),
    )?;
    let kept_character = keep_character
        .then(|| existing_visual_identity.and_then(myriad_merope::character_module))
        .flatten();
    let validate_new_face_construction = kept_character.is_none();
    let comparison_identity = if regenerate && kept_character.is_none() {
        existing_visual_identity.cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let requirements = visual_requirements.chars().take(500).collect::<String>();
    let requirements_named = !requirements.trim().is_empty();
    let existing_outfit_palette = if kept_character.is_some() && regenerate && !requirements_named {
        existing_outfit_palette_hint(existing_visual_identity)
    } else {
        Value::Null
    };
    let remap_existing_palette = !existing_outfit_palette.is_null();
    let input = json!({
        "pipeline": "onboarding/upper-body-visual-design",
        "task": "design_upper_body_visual_identity",
        "rollId": format!("v{}", uuid::Uuid::new_v4().simple()),
        "name": name.chars().take(50).collect::<String>(),
        "language": language,
        "genderPresentation": normalize_gender(gender),
        "clothingStyle": clothing_style,
        "clothingStyleGrammar": clothing_grammar,
        "clothingStyleGrammarRole": "gap-fill only",
        "keepCharacter": kept_character.is_some(),
        "existingCharacter": kept_character.clone().unwrap_or(Value::Null),
        "existingOutfitPalette": existing_outfit_palette,
        "persona": persona_input,
        "visualRequirements": requirements,
        "paletteFromPersona": {
            "from": ["likes", "temperament", "drives"],
            "onlyWhenVisualRequirementsDoNotSetPalette": true,
            "onlyWhenExistingOutfitPaletteAbsent": true,
            "citeSourcesInPaletteHint": false,
            "paletteNamedColorsOnPartsOnly": true,
            "sameSourcesForCostumeAndAccessory": true,
            "minDistinctHues": 3,
            "assignColorsToGarmentsAndAccessories": true,
            "forbidMonochromeFamily": true,
            "accessories": "hero plus two or three supporting",
        },
        "variety": visual_design_variety(
            requirements_named,
            kept_character.is_some(),
            remap_existing_palette,
        ),
        "regenerate": regenerate,
        "previousVisualIdentityForDifferenceOnly": comparison_identity,
    })
    .to_string();
    let raw = run_onboarding_call(&visual_design_system_prompt(), &input).await?;
    let parsed = parse_json_object(&raw).ok_or_else(|| {
        tracing::warn!("visual design returned non-JSON");
        OnboardingAiError::UnusableResponse("visual design was not valid JSON")
    })?;
    let mut identity =
        myriad_merope::sanitize_upper_body_visual_identity(&parsed).ok_or_else(|| {
            tracing::warn!("visual design failed field sanitize");
            OnboardingAiError::UnusableResponse("visual design failed field sanitize")
        })?;
    if let Some(character) = kept_character {
        if let Some(root) = identity.as_object_mut() {
            root.insert("character".into(), character);
        }
        identity = myriad_merope::sanitize_upper_body_visual_identity(&identity).ok_or(
            OnboardingAiError::UnusableResponse("visual design failed field sanitize"),
        )?;
    }
    myriad_merope::stamp_clothing_style(&mut identity, clothing_style);
    let reject = if myriad_merope::visual_identity_violates_style_lock(&identity) {
        Some("style-lock")
    } else if myriad_merope::visual_identity_has_high_collar(&identity) {
        Some("high-collar")
    } else if myriad_merope::visual_identity_has_body_proportion_drift(&identity) {
        Some("body-proportion")
    } else if myriad_merope::visual_identity_has_camera_composition_drift(&identity) {
        Some("camera-composition")
    } else if myriad_merope::visual_identity_has_literary_sludge(&identity) {
        Some("literary-sludge")
    } else if validate_new_face_construction
        && myriad_merope::visual_identity_has_facial_construction_drift(&identity)
    {
        Some("facial-construction")
    } else if validate_new_face_construction
        && !myriad_merope::visual_identity_matches_gender_presentation(&identity, gender)
    {
        Some("gender-presentation")
    } else {
        None
    };
    if let Some(reason) = reject {
        tracing::warn!(reason, language, "visual design failed quality check");
        return Err(OnboardingAiError::UnusableResponse(match reason {
            "style-lock" => "visual design failed style-lock check",
            "high-collar" => "visual design covered the neck",
            "body-proportion" => "visual design failed body-proportion check",
            "camera-composition" => "visual design failed camera-composition check",
            "literary-sludge" => "visual design used literary sludge",
            "facial-construction" => "visual design failed facial-construction check",
            "gender-presentation" => "visual design failed gender-presentation check",
            _ => "visual design failed quality check",
        }));
    }
    if !visual_design_matches_ui_language(&identity, language) {
        return Err(OnboardingAiError::LanguageMismatch);
    }
    Ok(identity)
}

pub struct ObservedPortraitVisual {
    pub clothing_style: &'static str,
    pub visual_identity: Value,
}

/// Read the uploaded master portrait into the visual-identity contract.
pub async fn observe_visual_from_portrait(
    language: &str,
    gender: &str,
    image: &ImageReference,
) -> Result<ObservedPortraitVisual, OnboardingAiError> {
    retry_unusable(
        VISUAL_DESIGN_ATTEMPTS,
        "portrait observation was unusable",
        || observe_visual_from_portrait_once(language, gender, image),
    )
    .await
}

pub(super) async fn observe_visual_from_portrait_once(
    language: &str,
    gender: &str,
    image: &ImageReference,
) -> Result<ObservedPortraitVisual, OnboardingAiError> {
    let prompt = observe_portrait_visual_prompt(language, normalize_gender(gender));
    let raw = run_vision_call(&prompt, image).await?;
    parse_observed_visual(&raw, language, gender)
}

pub(super) fn parse_observed_visual(
    raw: &str,
    language: &str,
    gender: &str,
) -> Result<ObservedPortraitVisual, OnboardingAiError> {
    let parsed = parse_json_object(raw).ok_or(OnboardingAiError::UnusableResponse(
        "portrait observation was not valid JSON",
    ))?;
    let clothing_style = parsed
        .get("clothingStyle")
        .and_then(Value::as_str)
        .and_then(myriad_merope::normalize_clothing_style)
        .unwrap_or("everyday");
    let mut identity = myriad_merope::sanitize_upper_body_visual_identity(&parsed).ok_or(
        OnboardingAiError::UnusableResponse("portrait observation failed field sanitize"),
    )?;
    myriad_merope::stamp_clothing_style(&mut identity, clothing_style);
    if let Some(fixed) = myriad_merope::ensure_visual_identity_states_gender(
        &identity,
        normalize_gender(gender),
        language,
    ) {
        identity = fixed;
    }
    if myriad_merope::visual_identity_has_literary_sludge(&identity) {
        return Err(OnboardingAiError::UnusableResponse(
            "portrait observation used literary sludge",
        ));
    }
    if !visual_design_matches_ui_language(&identity, language) {
        return Err(OnboardingAiError::LanguageMismatch);
    }
    Ok(ObservedPortraitVisual {
        clothing_style,
        visual_identity: identity,
    })
}

pub(super) async fn run_vision_call(
    prompt: &str,
    image: &ImageReference,
) -> Result<String, OnboardingAiError> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    if !config.pro_enabled {
        return Err(OnboardingAiError::AnalyzerUnavailable);
    }
    drop(config);
    let Ok(config) = get_ai_config_for_tier(ModelTier::Pro).await else {
        return Err(OnboardingAiError::AnalyzerUnavailable);
    };
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &image.bytes);
    let client = get_long_running_client().await;
    let request = match config.provider {
        AiProvider::Gemini => {
            let request = client
                .post(gemini_media::generate_content_url(
                    config.base_url.as_deref().unwrap_or_default(),
                    &config.model,
                ))
                .json(&json!({
                    "contents": [{ "parts": [
                        { "inlineData": { "mimeType": image.media_type, "data": encoded } },
                        { "text": prompt }
                    ]}],
                    "generationConfig": { "responseMimeType": "application/json" }
                }));
            if config.api_key.trim().is_empty() {
                request
            } else {
                request.header("x-goog-api-key", &config.api_key)
            }
        }
        AiProvider::OpenAI => {
            let data_url = format!("data:{};base64,{encoded}", image.media_type);
            let request = client
                .post(openai_chat_completions_url(config.base_url.as_deref()))
                .json(&json!({
                    "model": config.model,
                    "messages": [{
                        "role": "user",
                        "content": [
                            { "type": "text", "text": prompt },
                            { "type": "image_url", "image_url": { "url": data_url } }
                        ]
                    }],
                    "response_format": { "type": "json_object" }
                }));
            if config.api_key.trim().is_empty() {
                request
            } else {
                request.bearer_auth(&config.api_key)
            }
        }
        AiProvider::OpenAIResponses => {
            let data_url = format!("data:{};base64,{encoded}", image.media_type);
            let request = client
                .post(crate::services::analyzer::text_protocol::endpoint(
                    config.provider,
                    config.base_url.as_deref(),
                ))
                .json(&json!({
                    "model": config.model,
                    "store": false,
                    "input": [{
                        "role": "user",
                        "content": [
                            { "type": "input_text", "text": prompt },
                            { "type": "input_image", "image_url": data_url }
                        ]
                    }],
                    "text": { "format": { "type": "json_object" } }
                }));
            if config.api_key.trim().is_empty() {
                request
            } else {
                request.bearer_auth(&config.api_key)
            }
        }
        AiProvider::Anthropic => {
            let request = client
                .post(crate::services::analyzer::text_protocol::endpoint(
                    config.provider,
                    config.base_url.as_deref(),
                ))
                .header("anthropic-version", "2023-06-01")
                .json(&json!({
                    "model": config.model,
                    "max_tokens": 4096,
                    "messages": [{
                        "role": "user",
                        "content": [
                            { "type": "image", "source": {
                                "type": "base64",
                                "media_type": image.media_type,
                                "data": encoded
                            } },
                            { "type": "text", "text": format!(
                                "{prompt}\n\nReturn one valid JSON object only, without Markdown fences."
                            ) }
                        ]
                    }]
                }));
            if config.api_key.trim().is_empty() {
                request
            } else {
                request.header("x-api-key", &config.api_key)
            }
        }
    };
    let response = match tokio::time::timeout(ONBOARDING_AI_TIMEOUT, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            tracing::error!(%error, "portrait observation provider failed");
            return Err(OnboardingAiError::ProviderFailed(error.to_string()));
        }
        Err(_) => {
            return Err(OnboardingAiError::ProviderFailed(
                "portrait observation timed out".into(),
            ));
        }
    };
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| OnboardingAiError::ProviderFailed(error.to_string()))?;
    let preview = String::from_utf8_lossy(&bytes);
    record_ai_call_from_attribution(
        config.provider.as_str(),
        &config.model,
        prompt.len(),
        preview.len(),
        if status.is_success() {
            "completed"
        } else {
            "failed"
        },
        (!status.is_success()).then_some("AI_PROVIDER_ERROR"),
    )
    .await;
    if !status.is_success() {
        tracing::error!(%status, body = %preview.chars().take(400).collect::<String>(), "portrait observation HTTP error");
        return Err(OnboardingAiError::ProviderFailed(format!(
            "vision provider returned HTTP {status}"
        )));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        OnboardingAiError::UnusableResponse("portrait observation was not valid JSON")
    })?;
    let text = match config.provider {
        AiProvider::Gemini => value
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .and_then(|parts| {
                parts
                    .iter()
                    .find_map(|part| part.get("text").and_then(Value::as_str))
            })
            .map(str::to_string),
        AiProvider::OpenAI => {
            let content = value.pointer("/choices/0/message/content");
            content
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    content.and_then(Value::as_array).and_then(|parts| {
                        parts.iter().find_map(|part| {
                            part.get("text")
                                .and_then(Value::as_str)
                                .or_else(|| part.pointer("/text/value").and_then(Value::as_str))
                                .map(str::to_string)
                        })
                    })
                })
        }
        AiProvider::OpenAIResponses | AiProvider::Anthropic => {
            crate::services::analyzer::text_protocol::response_text(config.provider, &value).ok()
        }
    };
    text.filter(|value| !value.trim().is_empty())
        .ok_or(OnboardingAiError::UnusableResponse(
            "portrait observation contained no text",
        ))
}

pub(super) fn visual_design_persona_input(persona: &Value) -> Value {
    let source = persona.get("persona").unwrap_or(persona);
    json!({
        "temperament": source
            .get("temperament")
            .or_else(|| source.get("traits"))
            .cloned()
            .unwrap_or(Value::Null),
        "likes": source.get("likes").cloned().unwrap_or(Value::Null),
        "drives": source.get("drives").cloned().unwrap_or(Value::Null),
    })
}

pub(super) fn visual_design_matches_ui_language(value: &Value, language: &str) -> bool {
    let mut text = String::new();
    let flat = myriad_merope::flatten_visual_identity(value).unwrap_or_else(|| value.clone());
    for (key, _) in myriad_merope::UPPER_BODY_VISUAL_IDENTITY_FIELDS {
        if let Some(part) = flat.get(key).and_then(Value::as_str) {
            text.push_str(part);
        }
    }
    let mut latin = 0usize;
    let mut cjk = 0usize;
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            latin += 1;
        } else if is_cjk_han(ch) || is_hiragana(ch) || is_katakana_letter(ch) {
            cjk += 1;
        }
    }
    let total = latin + cjk;
    if total < 8 {
        return false;
    }
    match language {
        "en-US" => latin * 2 >= total,
        _ => cjk * 2 >= total,
    }
}
