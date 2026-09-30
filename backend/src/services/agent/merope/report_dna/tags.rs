//! The tag deck: pooled seed labels, localizing them, and keeping only tags that read as temperament.

use super::*;

pub(super) const PERSONA_POOL_KEYS: &[&str] = &[
    "thoughtful",
    "curious",
    "creative",
    "calm",
    "playful",
    "focused",
    "independent",
    "social",
    "persistent",
    "expressive",
    "bound",
    "loyal",
    "perfect",
    "solo",
    "order",
    "warm",
    "near",
    "deep_focus",
    "night",
    "soft",
    "space",
    "feel",
    "arranged",
    "slow_warm",
    "shy",
    "try_hard",
];

pub(super) fn seed_shuffle<T>(items: &mut [T], seed: &str) {
    if items.len() <= 1 {
        return;
    }
    let mut state = seed.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(33).wrapping_add(byte as u32)
    });
    if state == 0 {
        state = 1;
    }
    for i in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let j = (state as usize) % (i + 1);
        items.swap(i, j);
    }
}

pub(super) fn unique_push(out: &mut Vec<String>, tag: &str) {
    let tag = tag.trim();
    if tag.is_empty() {
        return;
    }
    if !out
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(tag))
    {
        out.push(tag.to_string());
    }
}

pub(super) fn persona_pool_labels(language: &str) -> Vec<String> {
    localize_report_seed_keys(
        &PERSONA_POOL_KEYS
            .iter()
            .map(|key| (*key).to_string())
            .collect::<Vec<_>>(),
        language,
    )
}

pub(super) fn complete_ai_tag_deck(
    primary: &[String],
    language: &str,
    target: usize,
) -> Vec<String> {
    let target = target.clamp(8, MAX_ONBOARDING_TAGS);
    let mut result = Vec::with_capacity(target);
    for tag in primary {
        if tag_matches_ui_language(tag, language) {
            unique_push(&mut result, tag);
        }
        if result.len() >= target {
            break;
        }
    }
    // Only pad to the playable minimum. Do not flood a good short deck
    // with generic pool leftovers just to hit targetTagCount.
    const MIN_PLAYABLE: usize = 8;
    if result.len() < MIN_PLAYABLE {
        for extra in persona_pool_labels(language) {
            unique_push(&mut result, &extra);
            if result.len() >= MIN_PLAYABLE {
                break;
            }
        }
    }
    sanitize_onboarding_tags_for_language(&result, language)
}

pub(super) fn fallback_tag_deck(
    evidence_tags: &[String],
    language: &str,
    seed: &str,
    target: usize,
) -> Vec<String> {
    let target = target.clamp(8, MAX_ONBOARDING_TAGS);
    let mut result = Vec::with_capacity(target);
    for tag in evidence_tags {
        if tag_matches_ui_language(tag, language) {
            unique_push(&mut result, tag);
        }
    }
    for extra in persona_pool_labels(language) {
        unique_push(&mut result, &extra);
    }
    seed_shuffle(&mut result, seed);
    result.truncate(target);
    sanitize_onboarding_tags_for_language(&result, language)
}

pub(super) fn traditional_ui_language(language: &str) -> bool {
    let lower = language.trim().to_ascii_lowercase().replace('_', "-");
    lower.starts_with("zh-tw")
        || lower.starts_with("zh-hk")
        || lower.starts_with("zh-mo")
        || lower.contains("hant")
}

pub(super) fn zh_seed_label(key: &str, traditional: bool) -> Option<&'static str> {
    Some(match (key, traditional) {
        ("thoughtful", false) => "善于思考",
        ("thoughtful", true) => "善於思考",
        ("curious", _) => "好奇",
        ("creative", false) => "有创造力",
        ("creative", true) => "有創造力",
        ("calm", false) => "沉静",
        ("calm", true) => "沉靜",
        ("playful", false) => "活泼",
        ("playful", true) => "活潑",
        ("focused", false) => "专注",
        ("focused", true) => "專注",
        ("independent", false) => "独立",
        ("independent", true) => "獨立",
        ("social", false) => "重视连接",
        ("social", true) => "重視連結",
        ("persistent", false) => "有韧性",
        ("persistent", true) => "有韌性",
        ("expressive", false) => "善于表达",
        ("expressive", true) => "善於表達",
        ("bound", false) => "边界感强",
        ("bound", true) => "邊界感強",
        ("loyal", false) => "朋友不多但很铁",
        ("loyal", true) => "朋友不多但很鐵",
        ("perfect", false) => "完美主义",
        ("perfect", true) => "完美主義",
        ("solo", false) => "独处才放松",
        ("solo", true) => "獨處才放鬆",
        ("order", false) => "讨厌混乱",
        ("order", true) => "討厭混亂",
        ("warm", false) => "外冷内热",
        ("warm", true) => "外冷內熱",
        ("near", _) => "想交心又怕熟",
        ("deep_focus", _) => "做事很沉",
        ("night", false) => "夜猫子",
        ("night", true) => "夜貓子",
        ("soft", false) => "嘴硬心软",
        ("soft", true) => "嘴硬心軟",
        ("space", false) => "礼貌但疏离",
        ("space", true) => "禮貌但疏離",
        ("feel", false) => "情绪来了先憋着",
        ("feel", true) => "情緒來了先憋著",
        ("arranged", false) => "喜欢把事情安排妥",
        ("arranged", true) => "喜歡把事情安排妥",
        ("slow_warm", false) => "慢热",
        ("slow_warm", true) => "慢熱",
        ("shy", _) => "社恐",
        ("try_hard", false) => "认真起来很拼",
        ("try_hard", true) => "認真起來很拼",
        _ => return None,
    })
}

pub(super) fn localize_report_seed_keys(keys: &[String], language: &str) -> Vec<String> {
    keys.iter()
        .filter_map(|key| {
            let label = match language {
                value if traditional_ui_language(value) => zh_seed_label(key, true)?,
                value if value.starts_with("zh") => zh_seed_label(key, false)?,
                value if value.starts_with("ja") => match key.as_str() {
                    "thoughtful" => "思慮深い",
                    "curious" => "好奇心旺盛",
                    "creative" => "創造的",
                    "calm" => "穏やか",
                    "playful" => "遊び好き",
                    "focused" => "集中力がある",
                    "independent" => "自立心がある",
                    "social" => "つながりを大切にする",
                    "persistent" => "粘り強い",
                    "expressive" => "表現豊か",
                    "bound" => "境界線がはっきり",
                    "loyal" => "友は少ないが深い",
                    "perfect" => "完璧主義",
                    "solo" => "一人の時間が必要",
                    "order" => "混沌が苦手",
                    "warm" => "一見クールで実は温かい",
                    "near" => "近づきたいけど怖い",
                    "deep_focus" => "没頭すると止まらない",
                    "night" => "夜型",
                    "soft" => "口は堅いが心は柔らかい",
                    "space" => "礼儀正しい距離感",
                    "feel" => "感情をすぐ出さない",
                    "arranged" => "段取り好き",
                    "slow_warm" => "スロースターター",
                    "shy" => "人見知り",
                    "try_hard" => "本気になると粘る",
                    _ => return None,
                },
                _ => match key.as_str() {
                    "thoughtful" => "Thoughtful",
                    "curious" => "Curious",
                    "creative" => "Creative",
                    "calm" => "Calm",
                    "playful" => "Playful",
                    "focused" => "Focused",
                    "independent" => "Independent",
                    "social" => "Connection-minded",
                    "persistent" => "Persistent",
                    "expressive" => "Expressive",
                    "bound" => "Clear boundaries",
                    "loyal" => "Few but loyal friends",
                    "perfect" => "Perfectionist streak",
                    "solo" => "Recharges alone",
                    "order" => "Dislikes chaos",
                    "warm" => "Cool face, warm core",
                    "near" => "Wants closeness, wary",
                    "deep_focus" => "Goes deep when working",
                    "night" => "Night owl",
                    "soft" => "Blunt mouth, soft heart",
                    "space" => "Polite but distant",
                    "feel" => "Holds feelings first",
                    "arranged" => "Likes things sorted",
                    "slow_warm" => "Slow to warm up",
                    "shy" => "Socially cautious",
                    "try_hard" => "Goes all-in when serious",
                    _ => return None,
                },
            };
            Some(label.to_string())
        })
        .collect()
}

pub(super) fn looks_like_job_or_identity_label(label: &str) -> bool {
    let label = label.trim();
    if label.is_empty() {
        return false;
    }
    const EXACT: &[&str] = &[
        "学生",
        "大学生",
        "研究生",
        "上班族",
        "打工人",
        "社畜",
        "程序员",
        "码农",
        "工程师",
        "设计师",
        "产品经理",
        "自由职业",
        "创业者",
        "老板",
        "老师",
        "教师",
        "医生",
        "护士",
        "律师",
        "会计",
        "司机",
        "主播",
        "博主",
        "网红",
        "up主",
        "UP主",
        "北漂",
        "沪漂",
        "男生",
        "女生",
        "男人",
        "女人",
    ];
    if EXACT.iter().any(|blocked| label == *blocked) {
        return true;
    }
    let lower = label.to_lowercase();
    label.contains("工程师")
        || label.contains("程序员")
        || label.contains("架构师")
        || label.contains("开发者")
        || label.contains("分析师")
        || label.contains("设计师")
        || label.contains("产品经理")
        || label.contains("职业")
        || label.contains("专业")
        || label.contains("学历")
        || label.contains("本科")
        || label.contains("硕士")
        || label.contains("博士")
        || lower.contains("engineer")
        || lower.contains("programmer")
        || lower.contains("manager")
        || lower.contains("teacher")
        || lower.contains("doctor")
        || lower.contains("nurse")
        || lower.contains("lawyer")
        || lower.contains("student")
        || lower.contains("designer")
        || label.contains("教師")
        || label.contains("医者")
        || label.contains("看護師")
        || label.contains("弁護士")
}

pub(super) fn looks_like_media_catalog_label(label: &str) -> bool {
    let lower = label.trim().to_lowercase();
    const NEEDLES: &[&str] = &[
        "二次元",
        "动画",
        "动漫",
        "游戏",
        "摇滚",
        "开放世界",
        "独立游戏",
        "账号",
        "用户",
        "报告",
        "平台",
        "http",
        "www",
        ".com",
        "anime",
        "game",
        "report",
        "platform",
        "アニメ",
        "ゲーム",
        "マンガ",
    ];
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

pub(super) fn is_reasonable_persona_tag(label: &str) -> bool {
    let label = label.trim();
    let chars = label.chars().count();
    if !(2..=MAX_ONBOARDING_TAG_CHARS).contains(&chars) {
        return false;
    }
    if label.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    if looks_like_job_or_identity_label(label) || looks_like_media_catalog_label(label) {
        return false;
    }
    const LITERARY: &[&str] = &[
        "质感",
        "美学",
        "信仰",
        "虔诚",
        "月光",
        "余温",
        "藏锋",
        "证明存在",
        "消化情绪",
        "取自",
        "像把",
        "在心里",
    ];
    if LITERARY.iter().any(|blocked| label.contains(blocked)) {
        return false;
    }
    if label.contains('(') || label.contains(')') || label.contains('（') || label.contains('）')
    {
        return false;
    }
    true
}

pub(super) fn sanitize_report_dna_tags(tags: &[String]) -> Vec<String> {
    const BLOCKED: &[&str] = &[
        "engineer",
        "programmer",
        "student",
        "designer",
        "manager",
        "report",
        "platform",
        "工程师",
        "程序员",
        "学生",
        "设计师",
        "产品经理",
        "报告",
        "平台",
        "エンジニア",
        "プログラマー",
        "レポート",
        "プラットフォーム",
    ];
    let filtered = tags
        .iter()
        .filter(|tag| {
            let normalized = tag.trim().to_lowercase();
            if BLOCKED.iter().any(|blocked| normalized.contains(blocked)) {
                return false;
            }
            is_reasonable_persona_tag(tag)
        })
        .cloned()
        .collect::<Vec<_>>();
    sanitize_onboarding_tags(&filtered)
}

pub fn sanitize_onboarding_tags_for_language(tags: &[String], language: &str) -> Vec<String> {
    sanitize_onboarding_tags(tags)
        .into_iter()
        .filter(|tag| tag_matches_ui_language(tag, language))
        .collect()
}

pub(crate) fn tag_matches_ui_language(label: &str, language: &str) -> bool {
    let has_latin = label.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_kana = label
        .chars()
        .any(|ch| matches!(ch, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}' | '\u{30FC}'));
    let has_han = label.chars().any(|ch| {
        matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
    });
    match language {
        "en-US" => has_latin && !has_han && !has_kana,
        "ja-JP" => has_han || has_kana,
        _ => has_han && !has_latin && !has_kana,
    }
}

pub(super) fn parse_ai_tags(raw: &str) -> Vec<String> {
    let Some(json) = extract_json_object_from_ai_response(raw) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&json) else {
        return Vec::new();
    };
    let Some(items) = value
        .get("tags")
        .or_else(|| value.get("seeds"))
        .or_else(|| value.get("labels"))
    else {
        return Vec::new();
    };
    let tags = match items {
        Value::Array(items) => items
            .iter()
            .take(36)
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| {
                        item.as_object().and_then(|object| {
                            ["label", "name", "tag", "text", "value"]
                                .iter()
                                .find_map(|key| object.get(*key).and_then(Value::as_str))
                        })
                    })
                    .map(str::trim)
                    .filter(|label| !label.is_empty())
                    .map(str::to_string)
            })
            .collect::<Vec<_>>(),
        Value::String(value) => value
            .split(['、', '，', '\n', ';', '|'])
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .take(36)
            .map(str::to_string)
            .collect(),
        _ => return Vec::new(),
    };
    sanitize_report_dna_tags(&tags)
}
