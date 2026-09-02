//! Pro onboarding helpers: name roll, structured persona draft, visual design.
//! Prompts live in `onboarding_prompts`.

use serde_json::{json, Map, Value};
use std::time::Duration;

use crate::config::ModelTier;
use crate::services::ai::create_ai_analyzer_for_tier_with_timeout;
use crate::services::analyzer::OutputBudget;

use super::onboarding_prompts::{
    visual_design_system_prompt, IMPORT_PERSONA_SYSTEM_PROMPT, NAME_SYSTEM_PROMPT,
    PERSONA_SYSTEM_PROMPT,
};
use super::report_dna::sanitize_onboarding_tags_for_language;

/// Keep in sync with `PERSONA_GENERATION_TIMEOUT_MS` / `MEROPE_PROXY_TIMEOUT_MS`.
const ONBOARDING_AI_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// 起名不是长任务：答案是一个两字段的小对象。用长任务的 15 分钟，网关卡住时
/// 「换一个」的转圈会转一刻钟。
///
/// 两分钟而不是更短：这条路径要容忍冷启动的模型、排队中的共享网关，以及
/// 被拒一次后重试的那一跳。宁可偶尔等久一点，也不要把一次本来会成功的
/// 生成判成超时——那对用户来说和「坏了」没区别。
///
/// 超时不触发重试阶梯：`rejected_request` 要求有 HTTP 状态码，而超时是
/// 没有状态码的传输错误。所以最坏情况是一次慢调用，不是三次叠加。
///
/// Keep in sync with `NAME_SUGGEST_TIMEOUT_MS`（前端必须比这个大）。
const NAME_CALL_TIMEOUT: Duration = Duration::from_secs(2 * 60);
/// 失控保险，不是调优旋钮。
///
/// 名字加含义大概四十个 token。这里给到四千，是因为多数网关把思考 token 也
/// 算进这个额度，而额度卡在答案前面的后果是静默的：JSON 没吐完，
/// `extract_openai_completion_text` 在 content 为空时又会回落去读
/// `reasoning_content`，解析拿到的是一段思考文本——曾经设成 512，每次都判成
/// 「字形不对」，实际是被截断。
///
/// 所以这个数字的职责只有一个：挡住无上限地写下去。**不要**拿它去省 token
/// 或者压思考，压思考是各家自己的参数，`OutputBudget` 的文档里写了为什么这
/// 里没有那一个。
const NAME_OUTPUT_BUDGET: OutputBudget = OutputBudget { max_tokens: 4096 };
/// 带严格校验闸的生成都该自己重试；次数按这一条有多贵来定。
///
/// 起草人设有五道闸（不是 JSON / 丰满度 / 语言 / 文艺腔 / 清洗完整性），导入
/// 同样五道，视觉设定六道——模型在一次正常生成里踩中一道是常态。不重试就等于
/// 把重试写成给人看的提示，用户看到的是「不可用，请再试一次」。
///
/// 名字走 Lite、几十个 token，抽三次也很快；这三条是 Pro 的长文生成，一次几十
/// 秒，两次够把「这一把没写好」和「配置真有问题」分开，再多就是拿站长的时间
/// 换概率。
const VISUAL_DESIGN_ATTEMPTS: u8 = 2;
const PERSONA_ATTEMPTS: u8 = 2;
/// 名字的字形闸口很严：中文名要 2–4 个全汉字、不以 阿/小 开头、不在屏蔽名单
/// 里；拉丁名要全 ASCII 字母、最多一个大写、3–16 字符。模型在一次正常生成里
/// 交出一个过不了闸的名字是常态，不是异常——不在这里自己再抽一次，就等于把
/// 重试的活儿丢给用户，界面上表现为「不合规则，请再随机一次」。
const NAME_ATTEMPTS: u8 = 3;
const MAX_PERSONA_LIST_ITEMS: usize = 12;
const MAX_PERSONA_LIST_ITEM_CHARS: usize = 180;
const MIN_SUMMARY_CHARS: usize = 80;
const MIN_TEMPERAMENT_ITEMS: usize = 5;
const MIN_PAIR_ITEMS: usize = 4;
const MIN_GUIDANCE_CHARS: usize = 36;

#[derive(Debug)]
pub enum OnboardingAiError {
    AnalyzerUnavailable,
    ProviderFailed(String),
    UnusableResponse(&'static str),
    LanguageMismatch,
}

impl OnboardingAiError {
    pub fn public_detail(&self) -> Option<&str> {
        match self {
            Self::ProviderFailed(detail) if !detail.trim().is_empty() => Some(detail),
            Self::UnusableResponse(reason) => Some(reason),
            _ => None,
        }
    }
}

impl std::fmt::Display for OnboardingAiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnalyzerUnavailable => formatter.write_str("analyzer unavailable"),
            Self::ProviderFailed(detail) if detail.trim().is_empty() => {
                formatter.write_str("provider failed")
            }
            Self::ProviderFailed(detail) => write!(formatter, "provider failed: {detail}"),
            Self::UnusableResponse(reason) => write!(formatter, "unusable response ({reason})"),
            Self::LanguageMismatch => formatter.write_str("language mismatch"),
        }
    }
}

pub async fn suggest_display_name(
    selected_tags: &[String],
    gender: &str,
    avoid_name: Option<&str>,
    language: &str,
    name_style: &str,
) -> Result<String, OnboardingAiError> {
    let seeds = sanitize_onboarding_tags_for_language(selected_tags, language);
    let style = normalize_name_style(name_style, language);
    let avoid = avoid_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(40).collect::<String>());
    let mut last_reason = "name had no usable meaning or script";
    for attempt in 0..NAME_ATTEMPTS {
        let input = json!({
            "task": "name",
            "language": language,
            "nameStyle": style,
            "genderPresentation": normalize_gender(gender),
            "avoidName": avoid.clone().unwrap_or_default(),
            "selectedTags": seeds,
            // 每次换一个，否则重试只会拿回同一个过不了闸的名字。
            "rollId": format!("n{}", uuid::Uuid::new_v4().simple()),
        })
        .to_string();
        let raw = run_name_call(NAME_SYSTEM_PROMPT, &input).await?;
        match parse_display_name_suggestion(&raw, avoid.as_deref(), style) {
            Ok(name) => return Ok(name),
            Err(reason) => {
                // 模型到底回了什么，只有这里知道。不记下来的话，四种失败在
                // 日志里长得一模一样。这是我们自己的模型输出，进的是服务端
                // 日志，不是返回给客户端的载荷。
                tracing::warn!(
                    reason,
                    attempt,
                    name_style = style,
                    raw = %raw.chars().take(400).collect::<String>(),
                    "name suggestion was unusable"
                );
                last_reason = reason;
            }
        }
    }
    Err(OnboardingAiError::UnusableResponse(last_reason))
}

pub async fn suggest_persona(
    name: &str,
    language: &str,
    selected_tags: &[String],
    gender: &str,
    extra_requirements: &str,
) -> Result<Value, OnboardingAiError> {
    retry_unusable(PERSONA_ATTEMPTS, "persona draft was unusable", || {
        suggest_persona_once(name, language, selected_tags, gender, extra_requirements)
    })
    .await
}

async fn suggest_persona_once(
    name: &str,
    language: &str,
    selected_tags: &[String],
    gender: &str,
    extra_requirements: &str,
) -> Result<Value, OnboardingAiError> {
    let tags = sanitize_onboarding_tags_for_language(selected_tags, language);
    let fallback = myriad_merope::fallback_persona_draft(name, language, &tags);
    let input = json!({
        "pipeline": "onboarding/persona",
        "task": "design_character_persona",
        "rollId": format!("p{}", uuid::Uuid::new_v4().simple()),
        "name": name.chars().take(50).collect::<String>(),
        "language": language,
        "genderPresentation": normalize_gender(gender),
        "selectedTags": tags,
        "extraRequirements": extra_requirements.chars().take(500).collect::<String>(),
        "fullness": {
            "summaryChars": "80-220",
            "temperamentCount": "5-8",
            "likesCount": "4-6",
            "likesAreHabitsNotScenes": true,
            "drivesCount": "4-6",
            "guidance": "1-2 sentences each for socialStyle and speechStyle",
            "noLiterarySludge": true,
        },
    })
    .to_string();
    let raw = run_onboarding_call(PERSONA_SYSTEM_PROMPT, &input).await?;
    let parsed = parse_json_object(&raw).ok_or(OnboardingAiError::UnusableResponse(
        "persona draft was not valid JSON",
    ))?;
    if !persona_draft_meets_generation_quality(&parsed) {
        tracing::warn!(language, "persona draft failed fullness checks");
        return Err(OnboardingAiError::UnusableResponse(
            "persona draft failed fullness checks",
        ));
    }
    if !persona_matches_ui_language(&parsed, language) {
        tracing::warn!(language, "persona draft failed language check");
        return Err(OnboardingAiError::UnusableResponse(
            "persona draft failed language check",
        ));
    }
    if myriad_merope::persona_has_literary_sludge(&parsed) {
        tracing::warn!(language, "persona draft failed literary-sludge check");
        return Err(OnboardingAiError::UnusableResponse(
            "persona draft used literary sludge",
        ));
    }
    let persona = myriad_merope::sanitize_persona_draft(&parsed, &fallback)
        .filter(myriad_merope::persona_draft_is_complete)
        .ok_or_else(|| {
            tracing::warn!(language, "persona draft failed sanitize/complete check");
            OnboardingAiError::UnusableResponse("persona draft failed sanitize/complete check")
        })?;
    Ok(persona)
}

pub async fn import_persona(
    name: &str,
    language: &str,
    gender: &str,
    source: &str,
) -> Result<Value, OnboardingAiError> {
    retry_unusable(PERSONA_ATTEMPTS, "imported persona was unusable", || {
        import_persona_once(name, language, gender, source)
    })
    .await
}

async fn import_persona_once(
    name: &str,
    language: &str,
    gender: &str,
    source: &str,
) -> Result<Value, OnboardingAiError> {
    let source = source.trim();
    if source.is_empty() {
        return Err(OnboardingAiError::UnusableResponse(
            "import source was empty",
        ));
    }
    let fallback = myriad_merope::fallback_persona_draft(name, language, &[]);
    let input = json!({
        "pipeline": "onboarding/persona-import",
        "task": "import_character_persona",
        "name": name.chars().take(50).collect::<String>(),
        "language": language,
        "genderPresentation": normalize_gender(gender),
        "source": source.chars().take(6_000).collect::<String>(),
        // 重试时换一个，否则第二次会照抄第一次那份没过闸的稿。
        "rollId": format!("i{}", uuid::Uuid::new_v4().simple()),
    })
    .to_string();
    let raw = run_onboarding_call(IMPORT_PERSONA_SYSTEM_PROMPT, &input).await?;
    let parsed = parse_json_object(&raw).ok_or(OnboardingAiError::UnusableResponse(
        "imported persona was not valid JSON",
    ))?;
    if !persona_draft_meets_generation_quality(&parsed) {
        return Err(OnboardingAiError::UnusableResponse(
            "imported persona failed fullness checks",
        ));
    }
    if !persona_matches_ui_language(&parsed, language) {
        return Err(OnboardingAiError::UnusableResponse(
            "imported persona failed language check",
        ));
    }
    if myriad_merope::persona_has_literary_sludge(&parsed) {
        return Err(OnboardingAiError::UnusableResponse(
            "imported persona used literary sludge",
        ));
    }
    myriad_merope::sanitize_persona_draft(&parsed, &fallback)
        .filter(myriad_merope::persona_draft_is_complete)
        .ok_or(OnboardingAiError::UnusableResponse(
            "imported persona failed sanitize/complete check",
        ))
}

/// 只重试「模型这一把没写好」。
///
/// 供应商不可用、调用本身失败，重试只会拖长等待并多烧一次钱——那两种直接
/// 上抛，交给上层去说「模型没配好」。
async fn retry_unusable<T, F, Fut>(
    attempts: u8,
    fallback_reason: &'static str,
    mut once: F,
) -> Result<T, OnboardingAiError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, OnboardingAiError>>,
{
    let mut last_error = OnboardingAiError::UnusableResponse(fallback_reason);
    for attempt in 0..attempts {
        match once().await {
            Ok(value) => return Ok(value),
            Err(
                error @ (OnboardingAiError::AnalyzerUnavailable
                | OnboardingAiError::ProviderFailed(_)),
            ) => return Err(error),
            Err(error) => {
                tracing::warn!(attempt, %error, "onboarding draft was unusable; retrying");
                last_error = error;
            }
        }
    }
    Err(last_error)
}

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

async fn suggest_visual_design_once(
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
    let input = json!({
        "pipeline": "onboarding/upper-body-visual-design",
        "task": "design_upper_body_visual_identity",
        "rollId": format!("v{}", uuid::Uuid::new_v4().simple()),
        "name": name.chars().take(50).collect::<String>(),
        "language": language,
        "genderPresentation": normalize_gender(gender),
        "clothingStyle": clothing_style,
        "clothingStyleGrammar": clothing_grammar,
        "keepCharacter": kept_character.is_some(),
        "existingCharacter": kept_character.clone().unwrap_or(Value::Null),
        "persona": persona_input,
        "visualRequirements": visual_requirements.chars().take(500).collect::<String>(),
        "paletteFromPersona": {
            "from": ["likes", "temperament", "drives"],
            "onlyWhenVisualRequirementsDoNotSetPalette": true,
            "citeSourcesInPaletteHint": false,
            "paletteNamedColorsOnPartsOnly": true,
            "sameSourcesForCostumeAndAccessory": true,
            "minDistinctHues": 3,
            "assignColorsToGarmentsAndAccessories": true,
            "forbidMonochromeFamily": true,
            "accessories": "hero plus two or three supporting",
        },
        "variety": {
            "clothingStyleIsFamilyNotKit": true,
            "appliesToEveryStyle": true,
            "changeConstructionNotJustColors": true,
            "forbidInterchangeableDefaultKit": true,
        },
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

/// `{"name":..., "meaning":...}` —— 和 `NAME_SYSTEM_PROMPT` 里那句同一个契约，
/// 只是这一份是发给供应商的，由 API 强制，而不是求模型自觉。
fn name_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "meaning": { "type": "string" },
        },
        "required": ["name", "meaning"],
    })
}

async fn run_name_call(system: &str, input: &str) -> Result<String, OnboardingAiError> {
    if crate::GLOBAL_DYNAMIC_CONFIG.read().await.lite_enabled {
        match run_name_call_on_tier(ModelTier::Lite, system, input).await {
            Ok(raw) => return Ok(raw),
            Err(OnboardingAiError::AnalyzerUnavailable) => {}
            Err(error) => return Err(error),
        }
    }
    run_name_call_on_tier(ModelTier::Standard, system, input).await
}

/// 起名走短结构化调用：限输出、关思考、JSON 由 API 保证。
///
/// 和 `run_onboarding_call_on_tier` 分开是因为那条是给人设起草和视觉设定用的
/// ——那两个确实要写长文，给它们套预算会截断。
async fn run_name_call_on_tier(
    tier: ModelTier,
    system: &str,
    input: &str,
) -> Result<String, OnboardingAiError> {
    let Some(analyzer) =
        create_ai_analyzer_for_tier_with_timeout(tier, Some(NAME_CALL_TIMEOUT)).await
    else {
        return Err(OnboardingAiError::AnalyzerUnavailable);
    };
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id().await;
    let schema = name_response_schema();
    match crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "merope",
        "onboarding",
        analyzer.analyze_json_short(
            system,
            input,
            "persona_name",
            Some(&schema),
            NAME_OUTPUT_BUDGET,
        ),
    )
    .await
    {
        Ok(raw) if !raw.trim().is_empty() => Ok(raw),
        Ok(_) => {
            tracing::warn!(?tier, "name model returned empty text");
            Err(OnboardingAiError::ProviderFailed(
                "name model returned empty text".into(),
            ))
        }
        Err(error) => {
            tracing::error!(%error, ?tier, "name model call failed");
            Err(OnboardingAiError::ProviderFailed(error.to_string()))
        }
    }
}

async fn run_onboarding_call(system: &str, input: &str) -> Result<String, OnboardingAiError> {
    run_onboarding_call_on_tier(ModelTier::Pro, system, input).await
}

async fn run_onboarding_call_on_tier(
    tier: ModelTier,
    system: &str,
    input: &str,
) -> Result<String, OnboardingAiError> {
    if matches!(tier, ModelTier::Pro) {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        if !config.pro_enabled {
            return Err(OnboardingAiError::AnalyzerUnavailable);
        }
    }
    let Some(analyzer) =
        create_ai_analyzer_for_tier_with_timeout(tier, Some(ONBOARDING_AI_TIMEOUT)).await
    else {
        return Err(OnboardingAiError::AnalyzerUnavailable);
    };
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id().await;
    match crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "merope",
        "onboarding",
        analyzer.analyze_with_system(system, input),
    )
    .await
    {
        Ok(raw) if !raw.trim().is_empty() => Ok(raw),
        Ok(_) => {
            tracing::warn!(?tier, "onboarding model returned empty text");
            Err(OnboardingAiError::ProviderFailed(
                "onboarding model returned empty text".into(),
            ))
        }
        Err(error) => {
            tracing::error!(%error, ?tier, "onboarding model call failed");
            Err(OnboardingAiError::ProviderFailed(error.to_string()))
        }
    }
}

fn visual_design_persona_input(persona: &Value) -> Value {
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

fn normalize_gender(value: &str) -> &str {
    match value {
        "female" | "male" | "nonbinary" | "unspecified" => value,
        _ => "unspecified",
    }
}

fn normalize_name_style(value: &str, language: &str) -> &'static str {
    match value.trim() {
        "chinese" | "zh" | "liyue" | "xianzhou" => "chinese",
        "japanese" | "ja" | "inazuma" | "wafuu" | "wa" => "japanese",
        "european" | "en" | "western" => "european",
        "mythic" | "mythology" | "classical" | "myth" => "mythic",
        _ => match language {
            "zh-CN" => "chinese",
            "ja-JP" => "japanese",
            _ => "european",
        },
    }
}

fn parse_json_object(raw: &str) -> Option<Value> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    serde_json::from_str(&raw[start..=end]).ok()
}

/// 四种失败各自有名字。
///
/// 它们原本共用一句「name had no usable meaning or script」，而这句话会经
/// `public_detail` 直接给到站长——于是「模型被截断了」和「模型给了个拉丁名
/// 但要求是中文名」在界面上长得一模一样，谁也没法判断该重试还是该改配置。
fn parse_display_name_suggestion(
    raw: &str,
    avoid: Option<&str>,
    name_style: &str,
) -> Result<String, &'static str> {
    let parsed = parse_json_object(raw).ok_or("name response was not JSON")?;
    parsed
        .get("meaning")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| value.chars().count() >= 2)
        .ok_or("name came back without a meaning")?;
    let candidates = if let Some(name) = parsed.get("name").and_then(Value::as_str) {
        vec![name.to_string()]
    } else if let Some(arr) = parsed.get("names").and_then(Value::as_array) {
        arr.iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    } else {
        return Err("name response carried no name");
    };
    let avoid_norm = avoid.map(str::trim).filter(|v| !v.is_empty());
    for candidate in candidates {
        let cleaned = sanitize_display_name_candidate(&candidate, name_style);
        if cleaned.is_empty() {
            continue;
        }
        if avoid_norm.is_some_and(|avoid| cleaned == avoid) {
            continue;
        }
        return Ok(cleaned);
    }
    Err("name did not fit the requested script")
}

fn sanitize_display_name_candidate(raw: &str, name_style: &str) -> String {
    let trimmed = raw
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>();
    let trimmed = trimmed.trim().trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '「' | '」' | '『' | '』' | '《' | '》' | ' '
        )
    });
    let collapsed = trimmed.split_whitespace().collect::<Vec<_>>().join("");
    match name_style {
        "european" | "mythic" => sanitize_latin_display_name(&collapsed, 16),
        "japanese" => sanitize_cjk_display_name(&collapsed, true, 5),
        _ => sanitize_cjk_display_name(&collapsed, false, 4),
    }
}

fn sanitize_latin_display_name(value: &str, max_chars: usize) -> String {
    if !value.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return String::new();
    }
    let count = value.chars().count();
    if count < 3 || count > max_chars {
        return String::new();
    }
    if value.chars().filter(|ch| ch.is_ascii_uppercase()).count() > 1 {
        return String::new();
    }
    if is_blocked_en_display_name(value) {
        return String::new();
    }
    value.to_string()
}

fn is_blocked_en_display_name(value: &str) -> bool {
    const BLOCKED: &[&str] = &[
        "robin",
        "sunday",
        "firefly",
        "sparkle",
        "stelle",
        "caelus",
        "jean",
        "diluc",
        "amber",
        "lisa",
        "maris",
        "cael",
        "liora",
        "zeus",
        "athena",
        "apollo",
        "artemis",
        "aphrodite",
        "hera",
        "hades",
        "persephone",
        "hermes",
        "poseidon",
        "nike",
        "nyx",
        "selene",
        "helios",
        "eos",
        "gaia",
        "odin",
        "thor",
        "loki",
        "freya",
        "freyja",
        "frigg",
        "baldur",
        "venus",
        "mars",
        "jupiter",
        "minerva",
        "diana",
        "mercury",
        "neptune",
        "pluto",
        "juno",
        "ceres",
    ];
    let lower = value.to_ascii_lowercase();
    BLOCKED.iter().any(|blocked| lower == *blocked)
}

fn is_blocked_ja_display_name(value: &str) -> bool {
    const BLOCKED: &[&str] = &[
        "綾華", "绫华", "万葉", "万叶", "宵宮", "宵宫", "早柚", "神子", "雷電", "雷电",
    ];
    BLOCKED
        .iter()
        .any(|blocked| value == *blocked || value.contains(blocked))
}

fn is_blocked_zh_display_name(value: &str) -> bool {
    const BLOCKED: &[&str] = &[
        "甘雨",
        "刻晴",
        "钟离",
        "行秋",
        "重云",
        "香菱",
        "凝光",
        "北斗",
        "辛焱",
        "云堇",
        "夜兰",
        "申鹤",
        "胡桃",
        "七七",
        "瑶瑶",
        "白术",
        "闲云",
        "魈",
        "景元",
        "丹恒",
        "符玄",
        "镜流",
        "彦卿",
        "素裳",
        "青雀",
        "停云",
        "驭空",
        "罗刹",
        "三月七",
        "花火",
        "黄泉",
        "流萤",
        "知更鸟",
        "藿藿",
        "寒鸦",
        "雪衣",
        "银狼",
        "姬子",
        "澄羽",
        "岚音",
        "星语",
        "月璃",
        "玄霄",
        "墨染",
        "夜雪",
        "凌霄",
    ];
    BLOCKED.iter().any(|blocked| {
        value == *blocked || (blocked.chars().count() >= 2 && value.contains(blocked))
    })
}

fn is_cjk_han(ch: char) -> bool {
    matches!(
        ch,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
    )
}

fn is_hiragana(ch: char) -> bool {
    matches!(ch, '\u{3041}'..='\u{3096}')
}

fn is_katakana_letter(ch: char) -> bool {
    matches!(ch, '\u{30A1}'..='\u{30FA}' | '\u{30FC}')
}

#[allow(dead_code)] // 仅测试调用：本仓无生产调用点（编译器已核）。
fn japanese_name_length_hint(roll_id: &str) -> (&'static str, u8) {
    let seed = roll_id.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(33).wrapping_add(byte as u32)
    });
    let chars = 2 + (seed % 4) as u8;
    let form = if seed % 2 == 0 {
        "modern-personal"
    } else {
        "inazuma-meaning"
    };
    (form, chars)
}

fn sanitize_cjk_display_name(value: &str, japanese: bool, max_chars: usize) -> String {
    let count = value.chars().count();
    if count < 2 || count > max_chars {
        return String::new();
    }
    if japanese && value.starts_with('々') {
        return String::new();
    }
    let script_ok = value.chars().all(|ch| {
        if japanese {
            is_cjk_han(ch) || is_hiragana(ch) || is_katakana_letter(ch) || ch == '々'
        } else {
            is_cjk_han(ch)
        }
    });
    if !script_ok {
        return String::new();
    }
    if !japanese && (value.starts_with('阿') || value.starts_with('小')) {
        return String::new();
    }
    if value.contains("小姐") || value.contains("大人") {
        return String::new();
    }
    if !japanese && is_blocked_zh_display_name(value) {
        return String::new();
    }
    if japanese
        && (value.ends_with("ちゃん")
            || value.ends_with("くん")
            || value.ends_with("さん")
            || value.ends_with('様')
            || is_blocked_ja_display_name(value))
    {
        return String::new();
    }
    value.to_string()
}

fn persona_matches_ui_language(value: &Value, language: &str) -> bool {
    let source = value.get("persona").unwrap_or(value);
    let mut text = String::new();
    for key in ["summary", "socialStyle", "speechStyle"] {
        if let Some(part) = source.get(key).and_then(Value::as_str) {
            text.push_str(part);
        }
    }
    for key in ["temperament", "likes", "drives", "traits"] {
        if let Some(items) = list_from_value(source, &[key]) {
            for item in items {
                text.push_str(&item);
            }
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
        return true;
    }
    match language {
        "en-US" => latin * 2 >= total,
        _ => cjk * 2 >= total,
    }
}

fn visual_design_matches_ui_language(value: &Value, language: &str) -> bool {
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

fn persona_draft_meets_generation_quality(value: &Value) -> bool {
    let Some(source) = value.get("persona").unwrap_or(value).as_object() else {
        return false;
    };
    let summary_ready = source
        .get("summary")
        .and_then(Value::as_str)
        .is_some_and(|summary| summary.trim().chars().count() >= MIN_SUMMARY_CHARS);
    let temperament_ready = list_from(source, &["temperament", "traits"])
        .is_some_and(|items| items.len() >= MIN_TEMPERAMENT_ITEMS);
    let likes_ready =
        list_from(source, &["likes"]).is_some_and(|items| items.len() >= MIN_PAIR_ITEMS);
    let drives_ready = list_from(source, &["drives", "motivations"])
        .is_some_and(|items| items.len() >= MIN_PAIR_ITEMS);
    let social_ready = ["socialStyle", "social_style"]
        .iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
        .is_some_and(|value| value.trim().chars().count() >= MIN_GUIDANCE_CHARS);
    let speech_ready = ["speechStyle", "speech_style", "voice"]
        .iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
        .is_some_and(|value| value.trim().chars().count() >= MIN_GUIDANCE_CHARS);
    summary_ready
        && temperament_ready
        && likes_ready
        && drives_ready
        && social_ready
        && speech_ready
}

fn list_from(source: &Map<String, Value>, keys: &[&str]) -> Option<Vec<String>> {
    let value = keys.iter().find_map(|key| source.get(*key))?;
    list_value(value)
}

fn list_from_value(source: &Value, keys: &[&str]) -> Option<Vec<String>> {
    let value = keys.iter().find_map(|key| source.get(*key))?;
    list_value(value)
}

fn list_value(value: &Value) -> Option<Vec<String>> {
    let raw = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>(),
        Value::String(value) => value
            .split(['、', '，', ';', '/', '|'])
            .map(str::to_string)
            .collect(),
        _ => return None,
    };
    let mut sanitized = Vec::new();
    for item in raw {
        let item = bounded_text(&item, MAX_PERSONA_LIST_ITEM_CHARS);
        if item.is_empty() || sanitized.iter().any(|existing| existing == &item) {
            continue;
        }
        sanitized.push(item);
        if sanitized.len() == MAX_PERSONA_LIST_ITEMS {
            break;
        }
    }
    Some(sanitized)
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_display_name_follows_name_style() {
        assert_eq!(normalize_name_style("", "zh-CN"), "chinese");
        assert_eq!(normalize_name_style("european", "zh-CN"), "european");
        assert_eq!(normalize_name_style("inazuma", "en-US"), "japanese");
        assert_eq!(normalize_name_style("wafuu", "zh-CN"), "japanese");
        assert_eq!(normalize_name_style("classical", "zh-CN"), "mythic");
        assert!(sanitize_display_name_candidate("Athena", "mythic").is_empty());
        assert!(sanitize_display_name_candidate("Freya", "european").is_empty());
        assert_eq!(
            sanitize_display_name_candidate("Cassiopeia", "mythic"),
            "Cassiopeia"
        );
        assert!(sanitize_display_name_candidate("Cassiopeianoxxxxx", "european").is_empty());
        assert!(sanitize_display_name_candidate("Al", "european").is_empty());
        assert_eq!(
            sanitize_display_name_candidate("Helionara", "mythic"),
            "Helionara"
        );
        assert!(sanitize_display_name_candidate("Alice", "chinese").is_empty());
        assert!(sanitize_display_name_candidate("阿强", "chinese").is_empty());
        assert!(sanitize_display_name_candidate("小美", "chinese").is_empty());
        assert_eq!(sanitize_display_name_candidate("晚衡", "chinese"), "晚衡");
        assert_eq!(
            sanitize_display_name_candidate("听白川", "chinese"),
            "听白川"
        );
        assert_eq!(
            sanitize_display_name_candidate("司南映雪", "chinese"),
            "司南映雪"
        );
        assert!(sanitize_display_name_candidate("秋水长天阔", "chinese").is_empty());
        assert_eq!(
            sanitize_display_name_candidate("「听白」", "chinese"),
            "听白"
        );
        assert_eq!(
            sanitize_display_name_candidate("Alexandria", "european"),
            "Alexandria"
        );
        assert!(sanitize_display_name_candidate("澄羽", "chinese").is_empty());
        assert!(sanitize_display_name_candidate("甘雨", "chinese").is_empty());
        assert!(sanitize_display_name_candidate("景元", "chinese").is_empty());
        assert_eq!(
            sanitize_display_name_candidate("Alice", "european"),
            "Alice"
        );
        assert!(sanitize_display_name_candidate("NightOwl", "european").is_empty());
        assert!(sanitize_display_name_candidate("Robin", "european").is_empty());
        assert!(sanitize_display_name_candidate("澄羽", "european").is_empty());
        assert_eq!(
            sanitize_display_name_candidate("あおい", "japanese"),
            "あおい"
        );
        assert_eq!(
            sanitize_display_name_candidate("佐藤美咲", "japanese"),
            "佐藤美咲"
        );
        assert_eq!(
            sanitize_display_name_candidate("高橋蓮", "japanese"),
            "高橋蓮"
        );
        assert_eq!(
            sanitize_display_name_candidate("中村ひなた", "japanese"),
            "中村ひなた"
        );
        assert_eq!(
            sanitize_display_name_candidate("佐々木結衣", "japanese"),
            "佐々木結衣"
        );
        assert_eq!(
            sanitize_display_name_candidate("ひなた", "japanese"),
            "ひなた"
        );
        assert!(sanitize_display_name_candidate("々木美咲", "japanese").is_empty());
        assert_eq!(sanitize_display_name_candidate("雪見", "japanese"), "雪見");
        assert_eq!(
            sanitize_display_name_candidate("月あかり", "japanese"),
            "月あかり"
        );
        assert_eq!(
            sanitize_display_name_candidate("ひまわり", "japanese"),
            "ひまわり"
        );
        assert_eq!(
            sanitize_display_name_candidate("あまのがわ", "japanese"),
            "あまのがわ"
        );
        assert_eq!(
            sanitize_display_name_candidate("ほしのかげ", "japanese"),
            "ほしのかげ"
        );
        assert!(sanitize_display_name_candidate("あまのがわや", "japanese").is_empty());
        assert!(sanitize_display_name_candidate("あまのがわかぜ", "japanese").is_empty());
        assert!(sanitize_display_name_candidate("宵宮", "japanese").is_empty());
        assert!(sanitize_display_name_candidate("Alice", "japanese").is_empty());
        assert!(sanitize_display_name_candidate("澄羽A", "chinese").is_empty());
        assert!(sanitize_display_name_candidate("澄羽Ａ", "chinese").is_empty());
        assert!(sanitize_display_name_candidate("澄羽・", "chinese").is_empty());
        assert!(sanitize_display_name_candidate("Aoi雪", "japanese").is_empty());
        assert_eq!(sanitize_display_name_candidate("ハナ", "japanese"), "ハナ");
        assert_eq!(
            sanitize_display_name_candidate("サリー", "japanese"),
            "サリー"
        );
        assert!(sanitize_display_name_candidate("葵ちゃん", "japanese").is_empty());
        for n in 0..16u8 {
            let (form, chars) = japanese_name_length_hint(&format!("n{n}"));
            assert!((2..=5).contains(&chars), "{form} {chars}");
            assert!(form == "modern-personal" || form == "inazuma-meaning");
        }
    }

    #[test]
    fn name_parser_requires_a_meaning_clause() {
        assert_eq!(
            parse_display_name_suggestion(
                r#"{"name":"晚衡","meaning":"晚来仍能把方向稳住"}"#,
                None,
                "chinese",
            ),
            Ok("晚衡".to_string())
        );
        assert_eq!(
            parse_display_name_suggestion(r#"{"name":"晚衡"}"#, None, "chinese"),
            Err("name came back without a meaning")
        );
        assert_eq!(
            parse_display_name_suggestion(r#"{"name":"晚衡","meaning":" " }"#, None, "chinese"),
            Err("name came back without a meaning")
        );
    }

    /// 名字不合规则时宿主自己再抽，不把重试丢给用户。
    ///
    /// 字形闸口很严（中文 2–4 个全汉字、拉丁最多一个大写、外加屏蔽名单），
    /// 模型交出一个过不了闸的名字是常态。没有这一层的话，界面上就是「不合
    /// 规则，请再随机一次」——那是把系统该做的事写成了给人看的提示。
    #[test]
    fn the_name_call_retries_itself_with_a_fresh_roll() {
        let source = include_str!("onboarding_ai.rs");
        let body = source
            .split("pub async fn suggest_display_name(")
            .nth(1)
            .and_then(|rest| rest.split("\npub ").next())
            .expect("suggest_display_name body");

        assert!(body.contains("for attempt in 0..NAME_ATTEMPTS"));
        assert!(NAME_ATTEMPTS > 1, "只抽一次等于没有重试");
        // 每次要换 rollId，否则重试拿回同一个过不了闸的名字。
        assert!(body.contains("\"rollId\""));
        // 供应商真的失败（没配模型、网关挂了）不该被重试掩盖成「不合规则」。
        assert!(body.contains("run_name_call(NAME_SYSTEM_PROMPT, &input).await?"));

        // 提示词得说清楚新的 rollId 意味着换一个名字，否则模型会忽略它。
        assert!(
            crate::services::agent::merope::onboarding_prompts::NAME_SYSTEM_PROMPT
                .contains("rollId")
        );
    }

    /// 每一条带校验闸的生成都得自己重试，不能只有名字和视觉设定有。
    ///
    /// 起草人设五道闸、导入五道、视觉设定六道。少了重试，模型踩中任何一道
    /// 都会变成界面上的一句「不可用」——那是把系统该做的事写给人看。
    #[test]
    fn every_gated_draft_retries_itself() {
        let source = include_str!("onboarding_ai.rs");
        for (entry, attempts) in [
            ("pub async fn suggest_persona(", PERSONA_ATTEMPTS),
            ("pub async fn import_persona(", PERSONA_ATTEMPTS),
            (
                "pub async fn suggest_visual_design(",
                VISUAL_DESIGN_ATTEMPTS,
            ),
        ] {
            let body = source
                .split(entry)
                .nth(1)
                .and_then(|rest| rest.split("\nasync fn ").next())
                .unwrap_or_else(|| panic!("{entry} body"));
            assert!(
                body.contains("retry_unusable("),
                "{entry} 没有重试，模型踩中任何一道闸都会直接失败"
            );
            assert!(attempts > 1, "{entry} 的次数是 1，等于没有重试");
        }

        // 重试壳只吃「这一把没写好」。供应商不可用 / 调用失败要立刻上抛，
        // 否则一个没配好的模型会被重试拖成三倍等待。
        let shell = source
            .split("async fn retry_unusable")
            .nth(1)
            .and_then(|rest| rest.split("\npub async fn ").next())
            .expect("retry shell");
        assert!(shell.contains("OnboardingAiError::AnalyzerUnavailable"));
        assert!(shell.contains("OnboardingAiError::ProviderFailed(_)"));
        assert!(shell.contains("return Err(error)"));
    }

    /// 重试要换一个 roll，否则第二次照抄第一次那份没过闸的稿。
    #[test]
    fn every_retried_draft_varies_its_roll() {
        let source = include_str!("onboarding_ai.rs");
        for entry in [
            "async fn suggest_persona_once(",
            "async fn import_persona_once(",
        ] {
            let body = source
                .split(entry)
                .nth(1)
                .and_then(|rest| rest.split("\npub async fn ").next())
                .unwrap_or_else(|| panic!("{entry} body"));
            assert!(body.contains("\"rollId\""), "{entry} 重试时不会变");
        }
        // 视觉设定用 regenerate 而不是 rollId，提示词里两者都认。
        assert!(visual_design_system_prompt().contains("rollId"));
    }

    /// 四种失败必须各自可辨。
    ///
    /// 它们原本共用一句「name had no usable meaning or script」，而这句会经
    /// `public_detail` 直接给到站长——「模型被 max_tokens 截断了」和「模型给
    /// 了个拉丁名但要的是中文名」在界面上长得一模一样，谁也判断不了该重试
    /// 还是该改配置。这条真实发生过：把上限设成 512，推理模型把额度花在思考
    /// 上，回来的是一段思考文本，报的却是「script」不对。
    #[test]
    fn each_name_failure_says_which_stage_failed() {
        let reasons = [
            // 截断 / 回落到 reasoning 文本：根本不是 JSON
            ("我先想想这个名字应该", "name response was not JSON"),
            // 有 JSON 没含义
            (r#"{"name":"晚衡"}"#, "name came back without a meaning"),
            // 有含义没名字
            (r#"{"meaning":"稳住方向"}"#, "name response carried no name"),
            // 名字在，但不是要求的字形
            (
                r#"{"name":"Evelyn","meaning":"稳住方向"}"#,
                "name did not fit the requested script",
            ),
        ];
        let mut seen = std::collections::HashSet::new();
        for (raw, expected) in reasons {
            let reason = parse_display_name_suggestion(raw, None, "chinese")
                .expect_err("these are all failures");
            assert_eq!(reason, expected, "raw = {raw}");
            assert!(seen.insert(reason), "两种失败共用了同一句话：{reason}");
        }
    }

    #[test]
    fn persona_parser_requires_real_character_content() {
        let parsed = parse_json_object(
            r#"{"persona":{"summary":"安静但会认真回应对自己重要的事情。想靠近，又把话说得很短。认定谁值得之后，锋会收起来，把事情一件件安排妥。不爱解释自己为什么忽然变软，也不肯把私人节奏交给别人来定。","temperament":["克制","细心","慢热","嘴硬心软","边界感强"],"likes":["夜里听雨","把桌面重新排好","长时间安静地做事","把一件小事做到位"],"drives":["理解彼此","守住边界","把节奏握在自己手里","对认定的人认真"],"socialStyle":"先听，不抢着说话。熟了之后才会把句子拉长，把真正在意的人留在自己定的距离里。","speechStyle":"话少，用词干净。对在意的人会把锋收起来，把事说清楚，也不用漂亮句子掩饰不耐烦。"}}"#,
        )
        .unwrap();
        assert!(persona_draft_meets_generation_quality(&parsed));
        assert!(!myriad_merope::persona_has_literary_sludge(&parsed));
        assert!(persona_matches_ui_language(&parsed, "zh-CN"));
        assert!(!persona_matches_ui_language(&parsed, "en-US"));
        assert!(myriad_merope::persona_has_literary_sludge(&json!({
            "persona": {
                "summary": "安静但会认真回应对自己重要的事情。想靠近，又把话说得很短。认定谁值得之后，锋会收起来，把事情一件件安排妥。",
                "temperament": ["克制","细心","慢热","嘴硬心软","边界感强"],
                "likes": ["色彩取自叠在杯底过夜的纸条","把桌面重新排好","长时间安静地做事","把一件小事做到位"],
                "drives": ["理解彼此","守住边界","把节奏握在自己手里","对认定的人认真"],
                "socialStyle": "先听，不抢着说话。熟了之后才会把句子拉长。",
                "speechStyle": "话少，用词干净。对在意的人会把事说清楚。"
            }
        })));
        assert!(!persona_draft_meets_generation_quality(&json!({
            "persona": { "summary": "只有一句，没有性格数组" }
        })));
        assert!(!persona_draft_meets_generation_quality(&json!({
            "persona": {
                "summary": "安静但会认真回应对自己重要的事情。",
                "temperament": ["克制"]
            }
        })));
        assert!(!persona_draft_meets_generation_quality(&json!({
            "persona": {
                "summary": "安静但会认真回应对自己重要的事情。想靠近，又把门留一条缝。",
                "temperament": ["克制","细心","慢热","嘴硬心软","边界感强"],
                "likes": ["夜里听雨","把桌面重新排好","长时间安静地做事","把一件小事做到位"],
                "drives": ["理解彼此","守住边界","把节奏握在自己手里","对认定的人认真"],
                "socialStyle": "先听，不抢着说话。熟了之后才会把句子拉长，把真正在意的人留在自己定的距离里。",
                "speechStyle": "话少，用词干净。对在意的人会把锋收起来，把事说清楚，也不用漂亮句子掩饰不耐烦。"
            }
        })));
        let english = json!({
            "persona": {
                "summary": "Quiet, but answers the things that matter.",
                "temperament": ["restrained", "careful"],
                "likes": ["rain"],
                "drives": ["understand people"],
                "socialStyle": "does not interrupt",
                "speechStyle": "brief and warm"
            }
        });
        assert!(persona_matches_ui_language(&english, "en-US"));
        assert!(!persona_matches_ui_language(&english, "ja-JP"));
    }

    #[test]
    fn visual_design_keeps_only_associable_persona_fields() {
        let input = visual_design_persona_input(&json!({
            "summary": "安静但对认定的人会把话说清楚。",
            "temperament": ["克制", "细心"],
            "likes": ["夜里听雨"],
            "drives": ["守住边界"],
            "socialStyle": "先听，不抢着说话。",
            "speechStyle": "话少，用词干净。",
            "displayName": "晚衡",
            "draftSource": "ai"
        }));
        assert_eq!(
            input.as_object().map(|object| object.keys().count()),
            Some(3)
        );
        assert_eq!(input["temperament"][0], "克制");
        assert_eq!(input["likes"][0], "夜里听雨");
        assert_eq!(input["drives"][0], "守住边界");
        assert!(input.get("summary").is_none());
        assert!(input.get("socialStyle").is_none());
        assert!(input.get("speechStyle").is_none());
        assert!(input.get("displayName").is_none());
    }
}
