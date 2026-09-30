//! Suggesting her display name, and keeping a suggestion to names a person could have.

use super::*;

/// 起名不是长任务：答案是一个两字段的小对象。用长任务的 15 分钟，网关卡住时
/// 「换一个」的转圈会转一刻钟。
///
/// 五分钟而不是更短：这条路径要容忍冷启动的模型、排队中的共享网关，以及
/// 被拒一次后重试的那一跳。宁可偶尔等久一点，也不要把一次本来会成功的
/// 生成判成超时——那对用户来说和「坏了」没区别。
///
/// 超时不触发重试阶梯：`rejected_request` 要求有 HTTP 状态码，而超时是
/// 没有状态码的传输错误。所以最坏情况是一次慢调用，不是三次叠加。
///
/// Keep in sync with `NAME_SUGGEST_TIMEOUT_MS`（前端必须比这个大）。
pub(super) const NAME_CALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// 失控保险，不是调优旋钮。
///
/// 名字加含义约四十 token。额度 4096：网关把思考 token 也算进这个额度；
/// 额度卡在答案前会静默截断，`extract_openai_completion_text` 在 content 为空时
/// 回落 `reasoning_content`。
///
/// 所以这个数字的职责只有一个：挡住无上限地写下去。**不要**拿它去省 token
/// 或者压思考，压思考是各家自己的参数，`OutputBudget` 的文档里写了为什么这
/// 里没有那一个。
pub(super) const NAME_OUTPUT_BUDGET: OutputBudget = OutputBudget { max_tokens: 4096 };

/// 名字的字形闸口很严：中文名要 2–4 个全汉字、不以 阿/小 开头、不在屏蔽名单
/// 里；拉丁名要全 ASCII 字母、最多一个大写、3–16 字符。模型在一次正常生成里
/// 交出一个过不了闸的名字是常态，不是异常——不在这里自己再抽一次，就等于把
/// 重试的活儿丢给用户，界面上表现为「不合规则，请再随机一次」。
pub(super) const NAME_ATTEMPTS: u8 = 3;

pub async fn suggest_display_name(
    gender: &str,
    avoid_name: Option<&str>,
    language: &str,
    name_style: &str,
) -> Result<String, OnboardingAiError> {
    let style = normalize_name_style(name_style, language);
    let avoid = avoid_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(40).collect::<String>());
    let mut last_reason = "name had no usable meaning or script";
    for attempt in 0..NAME_ATTEMPTS {
        // 风格写进系统提示，这里只剩语言、性别、避开上次、换一次 roll。
        // 标签是人设草稿的材料，不进起名。
        let mut input = json!({
            "language": language,
            "genderPresentation": normalize_gender(gender),
            "rollId": format!("n{}", uuid::Uuid::new_v4().simple()),
        });
        if let Some(avoid) = avoid.clone() {
            input["avoidName"] = json!(avoid);
        }
        let raw = run_name_call(&name_system_prompt(style), &input.to_string()).await?;
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

/// `{"name":..., "meaning":...}` —— 和 `name_system_prompt` 里那句同一个契约，
/// 只是这一份是发给供应商的，由 API 强制，而不是求模型自觉。
pub(super) fn name_response_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "meaning": { "type": "string" },
        },
        "required": ["name", "meaning"],
    })
}

/// 起名只走严格 Lite：限输出、短 JSON，绝不借 Standard / Pro 的模型。
///
/// 和 `run_onboarding_call_on_tier` 分开是因为那条是给人设起草和视觉设定用的
/// ——那两个确实要写长文，给它们套预算会截断。Lite 没配好就直接不可用，
/// 不能静默落到 Standard，否则「换一个」会按思考模型的延迟转圈。
pub(super) async fn run_name_call(system: &str, input: &str) -> Result<String, OnboardingAiError> {
    let Some(analyzer) = create_strict_lite_ai_analyzer_with_timeout(Some(NAME_CALL_TIMEOUT)).await
    else {
        return Err(OnboardingAiError::AnalyzerUnavailable);
    };
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id()
        .await
        .map_err(|_| OnboardingAiError::AnalyzerUnavailable)?;
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
            tracing::warn!("name model returned empty text");
            Err(OnboardingAiError::ProviderFailed(
                "name model returned empty text".into(),
            ))
        }
        Err(error) => {
            tracing::error!(%error, "name model call failed");
            Err(OnboardingAiError::ProviderFailed(error.to_string()))
        }
    }
}

pub(super) fn normalize_name_style(value: &str, language: &str) -> &'static str {
    match value.trim() {
        "chinese" | "zh" | "liyue" | "xianzhou" => "chinese",
        "japanese" | "ja" | "inazuma" | "wafuu" | "wa" => "japanese",
        "european" | "en" | "western" => "european",
        "mythic" | "mythology" | "classical" | "myth" => "mythic",
        _ => match language {
            "zh-CN" | "zh-TW" => "chinese",
            "ja-JP" => "japanese",
            _ => "european",
        },
    }
}

/// 四种失败各自有名字，经 `public_detail` 给到站长。
pub(super) fn parse_display_name_suggestion(
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

pub(super) fn sanitize_display_name_candidate(raw: &str, name_style: &str) -> String {
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

pub(super) fn sanitize_latin_display_name(value: &str, max_chars: usize) -> String {
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

pub(super) fn is_blocked_en_display_name(value: &str) -> bool {
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

pub(super) fn is_blocked_ja_display_name(value: &str) -> bool {
    const BLOCKED: &[&str] = &[
        "綾華", "绫华", "万葉", "万叶", "宵宮", "宵宫", "早柚", "神子", "雷電", "雷电",
    ];
    BLOCKED
        .iter()
        .any(|blocked| value == *blocked || value.contains(blocked))
}

pub(super) fn is_blocked_zh_display_name(value: &str) -> bool {
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

pub(super) fn is_cjk_han(ch: char) -> bool {
    matches!(
        ch,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
    )
}

pub(super) fn is_hiragana(ch: char) -> bool {
    matches!(ch, '\u{3041}'..='\u{3096}')
}

pub(super) fn is_katakana_letter(ch: char) -> bool {
    matches!(ch, '\u{30A1}'..='\u{30FA}' | '\u{30FC}')
}

#[cfg(test)]
pub(super) fn japanese_name_length_hint(roll_id: &str) -> (&'static str, u8) {
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

pub(super) fn sanitize_cjk_display_name(value: &str, japanese: bool, max_chars: usize) -> String {
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
