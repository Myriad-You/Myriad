//! Chat-only temporary outfit overlay.
//!
//! Chat Lite decides whether to change clothes. The live face only executes a
//! saved wardrobe label. It never writes `activeOutfitId` or the live rig pointer.

use serde_json::Value;

use crate::visual_design::DEFAULT_WARDROBE_ID;

const DEFAULT_WARDROBE_LABEL: &str = "Default outfit";

const GENERIC_HINTS: &[&str] = &[
    "衣服", "服装", "衣装", "外套", "上衣", "那套", "这套", "一件", "一套", "outfit", "clothes",
    "clothing", "dress", "wear", "coat",
];

const CHANGE_PHRASES: &[&str] = &[
    "换上",
    "换成",
    "换一件",
    "换一套",
    "换一下",
    "换衣服",
    "换装",
    "穿上",
    "改穿",
    "想看你",
    "看看你",
    "看你穿",
    "看你换",
    "给我看",
    "让我看",
    "穿给我看",
    "打扮成",
    "装扮成",
    "着替えて",
    "着替",
    "服を換えて",
    "服を着て",
    "着てるの見せ",
    "見たい",
];

const CHANGE_LATIN: &[&str] = &[
    "change into",
    "switch outfit",
    "change outfit",
    "put on",
    "wear",
    "show me",
    "let me see",
    "want to see you",
    "wanna see you",
    "see you in",
];

const REVERT_PHRASES: &[&str] = &[
    "换回来",
    "换回去",
    "换回原来",
    "换回默认",
    "穿回来",
    "穿回原来",
    "恢复原来",
    "换回日常",
    "元に戻",
    "いつもの服",
];

const REVERT_LATIN: &[&str] = &[
    "change back",
    "original outfit",
    "default outfit",
    "default clothes",
    "usual clothes",
];

const OTHER_PHRASES: &[&str] = &["别的", "其他", "随便", "另一套", "別の"];
const OTHER_LATIN: &[&str] = &["another", "something else", "different one"];

const KEEP_PHRASES: &[&str] = &[
    "穿的就是",
    "已经穿了",
    "已经是这套",
    "已经是这件",
    "不用换",
    "不必换",
    "不换了",
    "不换衣服",
    "不想换",
    "就这样穿",
    "就穿着这套",
    "本来就穿",
    "没必要换",
    "这套就行",
    "不用换衣服",
];
const KEEP_LATIN: &[&str] = &[
    "already wearing",
    "already have it on",
    "no need to change",
    "don't change",
    "keep this on",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WardrobeLook {
    pub id: String,
    pub label: String,
    pub clothing_style: String,
    pub portrait_asset_id: Option<String>,
    pub rig_asset_id: Option<String>,
    pub generation_fingerprint: Option<String>,
    pub hints: Vec<String>,
}

impl WardrobeLook {
    pub fn playable(&self) -> bool {
        self.portrait_asset_id.is_some() || self.rig_asset_id.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayDecision<'a> {
    Unchanged,
    Clear,
    Wear(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WearDirective {
    Revert,
    Label(String),
}

const WEAR_OPEN: &str = "[[wear:";
const WEAR_CLOSE: &str = "]]";
const WEAR_OPEN_UNI: &str = "⟦wear:";
const WEAR_CLOSE_UNI: &str = "⟧";

pub fn worn_outfit_id(profile: &Value) -> Option<&str> {
    profile.get("activeOutfitId").and_then(Value::as_str)
}

pub fn wardrobe_look<'a>(looks: &'a [WardrobeLook], id: &str) -> Option<&'a WardrobeLook> {
    looks.iter().find(|look| look.id == id)
}

pub fn looks_from_visual_profile(profile: &Value) -> Vec<WardrobeLook> {
    let Some(items) = profile.get("wardrobe").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut looks: Vec<WardrobeLook> = items.iter().filter_map(look_from_item).collect();
    uniquify_generated_labels(&mut looks);
    looks
}

fn look_from_item(item: &Value) -> Option<WardrobeLook> {
    let id = item.get("id").and_then(Value::as_str)?.trim();
    if id.is_empty() {
        return None;
    }
    let clothing_style = item.get("clothingStyle").and_then(Value::as_str)?.trim();
    if clothing_style.is_empty() {
        return None;
    }
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let label = if id == DEFAULT_WARDROBE_ID {
        DEFAULT_WARDROBE_LABEL.to_string()
    } else if let Some(name) = name {
        name.to_string()
    } else {
        spoken_style_label(clothing_style).to_string()
    };
    let construction = item
        .get("outfit")
        .and_then(|outfit| outfit.get("outfitConstruction"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut hints = Vec::new();
    push_hint(&mut hints, &label);
    if let Some(name) = name {
        push_hint(&mut hints, name);
    }
    if id == DEFAULT_WARDROBE_ID {
        for alias in ["默认", "默认服装", "default outfit", "default"] {
            push_hint(&mut hints, alias);
        }
    } else {
        push_hint(&mut hints, clothing_style);
        for alias in clothing_style_aliases(clothing_style) {
            push_hint(&mut hints, alias);
        }
        for token in construction_tokens(construction) {
            push_hint(&mut hints, &token);
        }
    }
    push_hint(&mut hints, construction);
    Some(WardrobeLook {
        id: id.to_string(),
        label,
        clothing_style: clothing_style.to_string(),
        portrait_asset_id: string_field(item, "portraitAssetId"),
        rig_asset_id: string_field(item, "rigAssetId"),
        generation_fingerprint: string_field(item, "generationFingerprint"),
        hints,
    })
}

fn string_field(item: &Value, key: &str) -> Option<String> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn push_hint(hints: &mut Vec<String>, raw: &str) {
    let value = raw.trim();
    if value.is_empty() {
        return;
    }
    if !hints.iter().any(|existing| existing == value) {
        hints.push(value.to_string());
    }
}

fn clothing_style_aliases(style: &str) -> &'static [&'static str] {
    match style {
        "everyday" => &[
            "日常",
            "便服",
            "日常便装",
            "便装",
            "平时",
            "casual",
            "everyday",
        ],
        "uniform" => &["校服", "制服", "水手服", "制服裙", "uniform"],
        "fantasy" => &["幻想", "奇幻", "奇幻冒险", "fantasy"],
        "urban" => &["都市", "街头", "城市", "urban", "city"],
        "east-asian" => &["国风", "中式", "汉服"],
        "japanese" => &["和风", "着物", "和服"],
        "sci-fi" => &["科幻", "sci-fi"],
        "formal" => &["正装", "礼服", "formal"],
        "sport" => &["运动", "运动服", "sport"],
        "idol" => &[
            "舞台装",
            "舞台服",
            "舞台",
            "偶像",
            "偶像服",
            "演出",
            "演出服",
            "舞台衣",
            "stage",
            "ステージ",
        ],
        "gothic" => &["哥特", "gothic"],
        "lounge" => &["居家", "睡衣", "lounge"],
        "royal" => &["宫廷", "royal"],
        "mystic" => &["神秘", "mystic"],
        "travel" => &["旅行", "旅人", "travel"],
        "vintage" => &["复古", "vintage"],
        "rain" => &["雨衣", "雨天", "风衣", "trench"],
        _ => &[],
    }
}

pub fn format_chat_wardrobe_section(
    looks: &[WardrobeLook],
    worn_id: &str,
    overlay_id: Option<&str>,
) -> Option<String> {
    if looks.is_empty() {
        return None;
    }
    let showing_id = overlay_id.unwrap_or(worn_id);
    let catalog: Vec<&WardrobeLook> = looks.iter().filter(|look| look.playable()).collect();
    if catalog.is_empty() {
        return None;
    }
    let showing = wardrobe_look(looks, showing_id)
        .map(|look| look.label.as_str())
        .unwrap_or(showing_id);
    let wearing = if overlay_id.is_some() && overlay_id != Some(worn_id) {
        format!(
            "This round you're wearing: {showing} (temporary for this chat, not the outfit you actually have on)"
        )
    } else {
        format!("This round you're wearing: {showing}")
    };
    let lines = catalog
        .iter()
        .map(|look| format!("- {}", catalog_line(look, showing_id, looks)))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "## Clothes\n{wearing}\n{lines}\n\
         You decide whether to change this round. If they name a set or want to see one, change when it matches. \
         Short name, also-called names, or a recognizable part of the outfit all count as that set. \
         Even if the current set is the same style, change to the one they named if the short name is different. \
         To change, write a last line that is only [[wear:short-name]]; the short name is the name after each \"-\" up to the colon or period. \
         To change back, write [[wear:back]]. Also-called names help you recognize a set — do not put them in [[wear:]]. \
         That line is executed live; do not speak it. If you say you'll change but omit the line, live still switches to the named set. \
         If they clearly don't want a change, omit the line."
    ))
}

fn spoken_style_label(style: &str) -> &str {
    clothing_style_aliases(style)
        .iter()
        .copied()
        .find(|alias| {
            alias
                .chars()
                .all(|ch| ch.is_ascii_alphabetic() || ch == '-')
        })
        .unwrap_or(style)
}

fn catalog_line(look: &WardrobeLook, showing_id: &str, all: &[WardrobeLook]) -> String {
    let mut line = look.label.clone();
    let construction = look
        .hints
        .iter()
        .filter(|hint| *hint != &look.label && !hint.is_ascii() && hint.chars().count() > 4)
        .max_by_key(|hint| hint.chars().count())
        .cloned();
    if let Some(construction) = construction.as_deref() {
        if !look.label.contains(construction) {
            line.push('：');
            line.push_str(construction);
        }
    }
    let aliases = catalog_aliases(look, showing_id, all, construction.as_deref());
    if !aliases.is_empty() {
        line.push_str(". Also called ");
        line.push_str(&aliases.join("、"));
    }
    if look.id == showing_id {
        line.push_str(". Wearing this round");
    }
    line
}

fn catalog_aliases<'a>(
    look: &'a WardrobeLook,
    showing_id: &str,
    all: &'a [WardrobeLook],
    construction: Option<&str>,
) -> Vec<&'a str> {
    let mut aliases = Vec::new();
    if look.id != DEFAULT_WARDROBE_ID {
        for alias in clothing_style_aliases(&look.clothing_style) {
            if catalog_alias_ok(alias, look, showing_id, all, construction) {
                aliases.push(*alias);
            }
        }
    }
    for hint in look.hints.iter().map(String::as_str) {
        if aliases.contains(&hint) {
            continue;
        }
        if catalog_alias_ok(hint, look, showing_id, all, construction) {
            aliases.push(hint);
        }
        if aliases.len() >= 6 {
            break;
        }
    }
    aliases.truncate(6);
    aliases
}

const GARMENT_WORDS: &[&str] = &[
    "马甲",
    "披肩",
    "缎带",
    "圆领",
    "方领",
    "高领",
    "水手领",
    "肩翼",
    "荷叶",
    "胸衣",
    "开窗",
    "佩普林",
    "大衣",
    "风衣",
    "披风",
    "斗篷",
    "铠甲",
    "浴衣",
    "羽织",
    "旗袍",
    "汉服",
    "西装",
    "背心",
    "衬衫",
    "毛衣",
    "睡衣",
    "礼服",
    "亮片",
    "夹克",
    "卫衣",
    "抹胸",
    "吊带",
    "云肩",
    "束腰",
    "头纱",
    "面纱",
    "皇冠",
    "蝴蝶结",
    "围巾",
    "短裙",
    "长裙",
    "腰带",
    "水手服",
    "校服",
];

const SKIP_CONSTRUCTION_PIECES: &[&str] = &[
    "内层", "外层", "中层", "下摆", "胸前", "高腰", "无袖", "闭合", "敞开", "露出", "短身", "色块",
    "搭扣", "中线", "主导", "轮廓", "驱动", "完全", "左右", "颈部", "锁骨", "切断", "收束", "结构",
    "分区", "以前", "自右", "垂下", "弧形",
];

fn construction_tokens(construction: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for word in GARMENT_WORDS {
        if construction.contains(word) {
            push_hint(&mut tokens, word);
        }
    }
    for piece in construction.split(|ch: char| {
        matches!(
            ch,
            '，' | ',' | '。' | '；' | ';' | '：' | ':' | '、' | '/' | '（' | '）' | '(' | ')'
        ) || ch.is_whitespace()
    }) {
        let piece = piece.trim();
        let n = piece.chars().count();
        if !(2..=6).contains(&n) {
            continue;
        }
        if piece.is_ascii() {
            continue;
        }
        if is_generic_hint(piece) || SKIP_CONSTRUCTION_PIECES.contains(&piece) {
            continue;
        }
        push_hint(&mut tokens, piece);
    }
    tokens
}

pub fn split_chat_wear_directive(raw: &str) -> (String, Option<WearDirective>) {
    let mut spoken = raw.to_string();
    let mut directive = None;
    while let Some((rest, inner)) = take_wear_marker(&spoken) {
        spoken = rest;
        directive = parse_wear_inner(&inner);
    }
    (collapse_blank_lines(&spoken), directive)
}

/// Drop an unfinished `[[wear:` / `⟦wear:` suffix so streaming does not flash it.
pub fn hold_incomplete_wear_marker(spoken: &str) -> &str {
    let hold = incomplete_wear_open_len(spoken);
    &spoken[..spoken.len() - hold]
}

pub fn resolve_wear_directive<'a>(
    directive: &WearDirective,
    looks: &'a [WardrobeLook],
    worn_id: &str,
    current_overlay: Option<&str>,
) -> OverlayDecision<'a> {
    match directive {
        WearDirective::Revert => finalize(OverlayDecision::Clear, worn_id, current_overlay),
        WearDirective::Label(label) => {
            resolve_chat_outfit_overlay(label, looks, worn_id, current_overlay)
        }
    }
}

/// Marker wins. If Lite agreed in prose but forgot `[[wear:]]`, execute the
/// outfit the other person named, unless the spoken line refused to change.
pub fn wear_directive_after_reply(
    user_input: &str,
    spoken: &str,
    marker: Option<WearDirective>,
) -> Option<WearDirective> {
    if marker.is_some() {
        return marker;
    }
    if spoken_keeps_outfit(spoken) {
        return None;
    }
    let user_input = user_input.trim();
    if user_input.is_empty() {
        return None;
    }
    Some(WearDirective::Label(user_input.to_string()))
}

fn spoken_keeps_outfit(spoken: &str) -> bool {
    KEEP_PHRASES.iter().any(|phrase| spoken.contains(phrase))
        || KEEP_LATIN
            .iter()
            .any(|phrase| contains_latin_phrase(spoken, phrase))
}

pub fn resolve_chat_outfit_overlay<'a>(
    input: &str,
    looks: &'a [WardrobeLook],
    worn_id: &str,
    current_overlay: Option<&str>,
) -> OverlayDecision<'a> {
    let showing = current_overlay.unwrap_or(worn_id);
    if has_revert_intent(input) {
        return finalize(OverlayDecision::Clear, worn_id, current_overlay);
    }
    if wants_other(input) && has_change_intent(input) {
        let others: Vec<&WardrobeLook> = looks
            .iter()
            .filter(|look| look.playable() && look.id != showing)
            .collect();
        return if others.len() == 1 {
            finalize(
                OverlayDecision::Wear(others[0].id.as_str()),
                worn_id,
                current_overlay,
            )
        } else {
            OverlayDecision::Unchanged
        };
    }
    let mut scored: Vec<(&WardrobeLook, u32)> = looks
        .iter()
        .filter(|look| look.playable())
        .filter_map(|look| {
            let score = score_look(input, look, looks);
            (score > 0).then_some((look, score))
        })
        .collect();
    if scored.is_empty() {
        return OverlayDecision::Unchanged;
    }
    let best = scored.iter().map(|(_, score)| *score).max().unwrap_or(0);
    scored.retain(|(_, score)| *score == best);
    let chosen = if scored.len() == 1 {
        scored[0].0
    } else {
        let others: Vec<&WardrobeLook> = scored
            .iter()
            .map(|(look, _)| *look)
            .filter(|look| look.id != showing)
            .collect();
        if others.len() == 1 {
            others[0]
        } else {
            return OverlayDecision::Unchanged;
        }
    };
    finalize(
        OverlayDecision::Wear(chosen.id.as_str()),
        worn_id,
        current_overlay,
    )
}

fn finalize<'a>(
    decision: OverlayDecision<'a>,
    worn_id: &str,
    current_overlay: Option<&str>,
) -> OverlayDecision<'a> {
    match decision {
        OverlayDecision::Wear(id) if id == worn_id => {
            if current_overlay.is_none() {
                OverlayDecision::Unchanged
            } else {
                OverlayDecision::Clear
            }
        }
        OverlayDecision::Wear(id) if current_overlay == Some(id) => OverlayDecision::Unchanged,
        OverlayDecision::Clear if current_overlay.is_none() => OverlayDecision::Unchanged,
        other => other,
    }
}

fn has_change_intent(input: &str) -> bool {
    CHANGE_PHRASES.iter().any(|phrase| input.contains(phrase))
        || CHANGE_LATIN
            .iter()
            .any(|phrase| contains_latin_phrase(input, phrase))
}

fn has_revert_intent(input: &str) -> bool {
    REVERT_PHRASES.iter().any(|phrase| input.contains(phrase))
        || REVERT_LATIN
            .iter()
            .any(|phrase| contains_latin_phrase(input, phrase))
}

fn wants_other(input: &str) -> bool {
    OTHER_PHRASES.iter().any(|phrase| input.contains(phrase))
        || OTHER_LATIN
            .iter()
            .any(|phrase| contains_latin_phrase(input, phrase))
}

fn contains_latin_phrase(input: &str, phrase: &str) -> bool {
    let haystack = latin_folded(input);
    let needle = latin_folded(phrase);
    if needle.is_empty() {
        return false;
    }
    if needle.contains(' ') {
        return haystack.contains(&needle);
    }
    haystack
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|token| token == needle)
}

fn latin_folded(value: &str) -> String {
    value.to_ascii_lowercase()
}

fn score_look(input: &str, look: &WardrobeLook, all: &[WardrobeLook]) -> u32 {
    let mut score = 0;
    if contains_hint(input, &look.label) {
        score += 100;
    }
    if look.id == DEFAULT_WARDROBE_ID {
        // Leftover Chinese name still has to match after the English label.
        if score == 0 && contains_hint(input, "默认服装") {
            score = 100;
        }
        return score;
    }
    if contains_hint(input, &look.clothing_style) {
        score += 40;
    }
    for hint in &look.hints {
        if !contains_hint(input, hint) {
            continue;
        }
        if is_generic_hint(hint) {
            continue;
        }
        if hint == &look.label {
            continue;
        }
        if hint_is_unique(hint, look.id.as_str(), all) {
            score += 50;
        } else {
            score += 25;
        }
    }
    score
}

fn catalog_alias_ok(
    hint: &str,
    look: &WardrobeLook,
    showing_id: &str,
    all: &[WardrobeLook],
    construction: Option<&str>,
) -> bool {
    if hint == look.label || construction == Some(hint) || is_generic_hint(hint) {
        return false;
    }
    if hint.is_ascii() || hint.chars().count() > 6 {
        return false;
    }
    if hint_is_unique(hint, look.id.as_str(), all) {
        return true;
    }
    // Shared nicknames stay on the sets not currently showing, so Lite does
    // not treat the worn set as the named look.
    look.id != showing_id && all.iter().all(|other| other.label != hint)
}

fn uniquify_generated_labels(looks: &mut [WardrobeLook]) {
    for i in 0..looks.len() {
        if looks[i].id == DEFAULT_WARDROBE_ID {
            continue;
        }
        let label = looks[i].label.clone();
        let duplicates = looks.iter().filter(|look| look.label == label).count();
        if duplicates < 2 {
            continue;
        }
        if looks[..i].iter().all(|look| look.label != label) {
            continue;
        }
        let mut next = format!("Another {label}");
        let mut n = 2;
        while looks.iter().any(|look| look.label == next) {
            n += 1;
            next = format!("{label}{n}");
        }
        looks[i].label = next.clone();
        push_hint(&mut looks[i].hints, &next);
    }
}

fn contains_hint(input: &str, hint: &str) -> bool {
    let hint = hint.trim();
    if hint.chars().count() < 2 {
        return false;
    }
    if hint.is_ascii() {
        return contains_latin_phrase(input, hint);
    }
    if input.contains(hint) {
        return true;
    }
    hint.chars().count() <= 6 && garment_names_align(input, hint)
}

fn garment_names_align(input: &str, name: &str) -> bool {
    let input = compact_garment_name(input);
    let name = compact_garment_name(name);
    if input.chars().count() < 2 || name.chars().count() < 2 {
        return false;
    }
    if input == name {
        return true;
    }
    let input = strip_garment_suffix(&input);
    let name = strip_garment_suffix(&name);
    input.chars().count() >= 2 && input == name
}

fn compact_garment_name(raw: &str) -> String {
    raw.chars()
        .filter(|ch| !is_garment_name_noise(*ch))
        .collect::<String>()
        .to_ascii_lowercase()
}

fn is_garment_name_noise(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '：' | ':'
                | '。'
                | '.'
                | '，'
                | ','
                | '、'
                | '；'
                | ';'
                | '！'
                | '!'
                | '？'
                | '?'
                | '（'
                | ')'
                | '）'
                | '('
                | '「'
                | '」'
                | '"'
                | '\''
                | '《'
                | '》'
                | '-'
                | '—'
        )
}

fn strip_garment_suffix(name: &str) -> &str {
    name.strip_suffix('装')
        .or_else(|| name.strip_suffix('服'))
        .or_else(|| name.strip_suffix('衣'))
        .filter(|rest| rest.chars().count() >= 2)
        .unwrap_or(name)
}

fn hint_is_unique(hint: &str, owner_id: &str, all: &[WardrobeLook]) -> bool {
    all.iter()
        .filter(|other| other.id != owner_id && other.id != DEFAULT_WARDROBE_ID)
        .all(|other| !look_mentions(other, hint))
}

fn look_mentions(look: &WardrobeLook, hint: &str) -> bool {
    contains_hint(&look.label, hint)
        || look
            .hints
            .iter()
            .any(|candidate| contains_hint(candidate, hint))
}

fn is_generic_hint(hint: &str) -> bool {
    let folded = hint.trim();
    GENERIC_HINTS
        .iter()
        .any(|generic| folded.eq_ignore_ascii_case(generic))
}

fn take_wear_marker(text: &str) -> Option<(String, String)> {
    let ascii = text.find(WEAR_OPEN).map(|at| (at, WEAR_OPEN, WEAR_CLOSE));
    let uni = text
        .find(WEAR_OPEN_UNI)
        .map(|at| (at, WEAR_OPEN_UNI, WEAR_CLOSE_UNI));
    let (start, open, close) = match (ascii, uni) {
        (Some(ascii), Some(uni)) if uni.0 < ascii.0 => uni,
        (Some(ascii), _) => ascii,
        (None, Some(uni)) => uni,
        (None, None) => return None,
    };
    let inner_at = start + open.len();
    let after = text.get(inner_at..)?;
    let close_at = after.find(close)?;
    let inner = after[..close_at].trim().to_string();
    let end = inner_at + close_at + close.len();
    let mut spoken = String::with_capacity(text.len().saturating_sub(end - start));
    spoken.push_str(&text[..start]);
    spoken.push_str(&text[end..]);
    Some((spoken, inner))
}

fn parse_wear_inner(inner: &str) -> Option<WearDirective> {
    let label = wear_short_name(inner);
    if label.is_empty() {
        return Some(WearDirective::Revert);
    }
    let folded = label.to_ascii_lowercase();
    if matches!(
        label,
        "回来" | "换回" | "换回来" | "原来" | "默认" | "回来的"
    ) || matches!(folded.as_str(), "revert" | "back" | "original" | "default")
    {
        return Some(WearDirective::Revert);
    }
    Some(WearDirective::Label(label.to_string()))
}

fn wear_short_name(inner: &str) -> &str {
    let label = inner.trim();
    let cut = label
        .find(['：', ':', '。', '！', '？', '\n', '（', '('])
        .unwrap_or(label.len());
    label.get(..cut).unwrap_or(label).trim()
}

fn collapse_blank_lines(raw: &str) -> String {
    let mut lines = Vec::new();
    let mut blank = false;
    for line in raw.lines() {
        if line.trim().is_empty() {
            if !lines.is_empty() {
                blank = true;
            }
            continue;
        }
        if blank {
            lines.push(String::new());
            blank = false;
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n").trim().to_string()
}

fn incomplete_wear_open_len(spoken: &str) -> usize {
    for open in [WEAR_OPEN, WEAR_OPEN_UNI] {
        for len in (1..=open.len()).rev() {
            if !open.is_char_boundary(len) {
                continue;
            }
            let prefix = &open[..len];
            if spoken.ends_with(prefix) {
                return prefix.len();
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn coat() -> WardrobeLook {
        WardrobeLook {
            id: "w-coat".into(),
            label: "冬日大衣".into(),
            clothing_style: "urban".into(),
            portrait_asset_id: Some("/uploads/coat.png".into()),
            rig_asset_id: None,
            generation_fingerprint: None,
            hints: vec![
                "冬日大衣".into(),
                "urban".into(),
                "都市".into(),
                "高领内搭叠短大衣".into(),
            ],
        }
    }

    fn uniform() -> WardrobeLook {
        WardrobeLook {
            id: "w-uniform".into(),
            label: "校服".into(),
            clothing_style: "uniform".into(),
            portrait_asset_id: Some("/uploads/uniform.png".into()),
            rig_asset_id: None,
            generation_fingerprint: None,
            hints: vec!["校服".into(), "uniform".into(), "水手领内搭叠短外套".into()],
        }
    }

    fn default_look() -> WardrobeLook {
        WardrobeLook {
            id: DEFAULT_WARDROBE_ID.into(),
            label: DEFAULT_WARDROBE_LABEL.into(),
            clothing_style: "everyday".into(),
            portrait_asset_id: Some("/uploads/default.png".into()),
            rig_asset_id: None,
            generation_fingerprint: None,
            hints: vec![
                DEFAULT_WARDROBE_LABEL.into(),
                "默认".into(),
                "默认服装".into(),
            ],
        }
    }

    fn catalog() -> Vec<WardrobeLook> {
        vec![default_look(), coat(), uniform()]
    }

    #[test]
    fn lite_wear_marker_is_stripped_and_resolved() {
        let (spoken, directive) =
            split_chat_wear_directive("行啊，等着。我去换。\n[[wear:舞台装]]");
        assert_eq!(spoken, "行啊，等着。我去换。");
        assert_eq!(directive, Some(WearDirective::Label("舞台装".into())));
        let (spoken, directive) =
            split_chat_wear_directive("换好了。\n[[wear:舞台装：斜裁舞台马甲]]");
        assert_eq!(spoken, "换好了。");
        assert_eq!(directive, Some(WearDirective::Label("舞台装".into())));
        let (spoken, directive) = split_chat_wear_directive("[[wear:回来]]\n好。");
        assert_eq!(spoken, "好。");
        assert_eq!(directive, Some(WearDirective::Revert));
        let (spoken, directive) = split_chat_wear_directive("你好");
        assert_eq!(spoken, "你好");
        assert_eq!(directive, None);
        assert_eq!(hold_incomplete_wear_marker("行啊[[wear:"), "行啊");
        assert_eq!(hold_incomplete_wear_marker("行啊。"), "行啊。");
        let mut stage = coat();
        stage.id = "w-stage".into();
        stage.label = "舞台装".into();
        stage.hints = vec!["舞台装".into()];
        let looks = vec![default_look(), stage];
        assert_eq!(
            resolve_wear_directive(
                &WearDirective::Label("舞台装".into()),
                &looks,
                "default",
                None
            ),
            OverlayDecision::Wear("w-stage")
        );
        assert_eq!(
            resolve_wear_directive(&WearDirective::Revert, &looks, "default", Some("w-stage")),
            OverlayDecision::Clear
        );
    }

    #[test]
    fn naming_a_unique_outfit_switches() {
        assert_eq!(
            resolve_chat_outfit_overlay("这件冬日大衣真好看", &catalog(), "default", None),
            OverlayDecision::Wear("w-coat")
        );
        assert_eq!(
            resolve_chat_outfit_overlay("你好", &catalog(), "default", None),
            OverlayDecision::Unchanged
        );
    }

    #[test]
    fn asking_to_see_a_named_outfit_wears_that_set() {
        let mut stage = coat();
        stage.id = "w-stage".into();
        stage.label = "舞台装".into();
        stage.clothing_style = "idol".into();
        stage.hints = vec![
            "舞台装".into(),
            "idol".into(),
            "偶像".into(),
            "舞台".into(),
            "stage".into(),
        ];
        let looks = vec![default_look(), stage, uniform()];
        assert_eq!(
            resolve_chat_outfit_overlay("我想看你舞台装", &looks, "default", None),
            OverlayDecision::Wear("w-stage")
        );
        assert_eq!(
            resolve_chat_outfit_overlay("show me your stage outfit", &looks, "default", None),
            OverlayDecision::Wear("w-stage")
        );
    }

    #[test]
    fn a_named_change_request_wears_that_set() {
        assert_eq!(
            resolve_chat_outfit_overlay("换上冬日大衣", &catalog(), "default", None),
            OverlayDecision::Wear("w-coat")
        );
        assert_eq!(
            resolve_chat_outfit_overlay(
                "Can you change into the uniform?",
                &catalog(),
                "default",
                None
            ),
            OverlayDecision::Wear("w-uniform")
        );
    }

    #[test]
    fn change_back_clears_the_overlay_instead_of_wearing_default() {
        assert_eq!(
            resolve_chat_outfit_overlay("换回来", &catalog(), "w-coat", Some("w-uniform")),
            OverlayDecision::Clear
        );
        assert_eq!(
            resolve_chat_outfit_overlay("换回原来的衣服", &catalog(), "w-coat", Some("w-uniform")),
            OverlayDecision::Clear
        );
    }

    #[test]
    fn wearing_the_persisted_set_clears_a_temporary_overlay() {
        assert_eq!(
            resolve_chat_outfit_overlay("换上默认服装", &catalog(), "default", Some("w-coat")),
            OverlayDecision::Clear
        );
        assert_eq!(
            resolve_chat_outfit_overlay("换上默认服装", &catalog(), "default", None),
            OverlayDecision::Unchanged
        );
    }

    #[test]
    fn another_outfit_picks_the_only_other_playable_set() {
        let two = vec![default_look(), coat()];
        assert_eq!(
            resolve_chat_outfit_overlay("换一套别的", &two, "default", None),
            OverlayDecision::Wear("w-coat")
        );
        assert_eq!(
            resolve_chat_outfit_overlay("换一套别的", &catalog(), "default", None),
            OverlayDecision::Unchanged
        );
    }

    #[test]
    fn unplayable_sets_are_not_candidates() {
        let mut empty_coat = coat();
        empty_coat.portrait_asset_id = None;
        empty_coat.rig_asset_id = None;
        let looks = vec![default_look(), empty_coat];
        assert_eq!(
            resolve_chat_outfit_overlay("换上冬日大衣", &looks, "default", None),
            OverlayDecision::Unchanged
        );
    }

    #[test]
    fn repeating_the_current_overlay_is_unchanged() {
        assert_eq!(
            resolve_chat_outfit_overlay("换上冬日大衣", &catalog(), "default", Some("w-coat")),
            OverlayDecision::Unchanged
        );
    }

    #[test]
    fn looks_read_saved_wardrobe_items_and_the_prompt_hides_ids() {
        let profile = json!({
            "activeOutfitId": "default",
            "wardrobe": [
                {
                    "id": "default",
                    "clothingStyle": "everyday",
                    "portraitAssetId": "/uploads/default.png",
                    "outfit": { "outfitConstruction": "水手领内搭叠短外套" }
                },
                {
                    "id": "w-coat",
                    "name": "冬日大衣",
                    "clothingStyle": "urban",
                    "portraitAssetId": "/uploads/coat.png",
                    "outfit": { "outfitConstruction": "高领内搭叠短大衣" }
                }
            ]
        });
        let looks = looks_from_visual_profile(&profile);
        assert_eq!(looks.len(), 2);
        assert_eq!(looks[0].label, DEFAULT_WARDROBE_LABEL);
        assert_eq!(looks[1].id, "w-coat");
        assert_eq!(worn_outfit_id(&profile), Some("default"));
        let section = format_chat_wardrobe_section(&looks, "default", Some("w-coat")).unwrap();
        assert!(section.contains("冬日大衣 (temporary for this chat"));
        assert!(section.contains("- Default outfit：水手领内搭叠短外套"));
        assert!(section.contains("- 冬日大衣：高领内搭叠短大衣"));
        assert!(section.contains("Also called 都市"));
        assert!(!section.contains("Also called 日常"));
        assert!(!looks[0]
            .hints
            .iter()
            .any(|hint| hint == "日常" || hint == "everyday"));
        assert!(section.contains("Wearing this round"));
        assert!(!section.contains("w-coat"));
        assert!(!section.contains("activeOutfitId"));
        assert!(!section.contains("JSON"));
        assert!(!section.contains("portraitAssetId"));
        assert!(section.contains("[[wear:"));
        assert!(section.contains("You decide"));
    }

    #[test]
    fn two_unnamed_idol_sets_do_not_share_the_wear_name() {
        let profile = json!({
            "activeOutfitId": "default",
            "wardrobe": [
                {
                    "id": "default",
                    "clothingStyle": "idol",
                    "portraitAssetId": "/uploads/default.png",
                    "outfit": { "outfitConstruction": "雪白方领舞台胸衣" }
                },
                {
                    "id": "w-stage-2",
                    "clothingStyle": "idol",
                    "portraitAssetId": "/uploads/stage2.png",
                    "outfit": { "outfitConstruction": "斜裁舞台马甲，不对称缎带披肩" }
                }
            ]
        });
        let looks = looks_from_visual_profile(&profile);
        assert_eq!(looks[0].label, DEFAULT_WARDROBE_LABEL);
        assert!(looks[0]
            .hints
            .iter()
            .all(|hint| hint == DEFAULT_WARDROBE_LABEL
                || hint == "默认"
                || hint == "默认服装"
                || hint == "default"
                || hint == "default outfit"
                || hint.contains("方领")));
        assert_eq!(looks[1].id, "w-stage-2");
        assert_eq!(looks[1].label, "stage");
        let section = format_chat_wardrobe_section(&looks, "default", None).unwrap();
        let default_line = section
            .lines()
            .find(|line| line.starts_with("- Default outfit"))
            .unwrap();
        let other_line = section
            .lines()
            .find(|line| line.starts_with("- stage"))
            .unwrap();
        assert!(default_line.contains("Wearing this round"));
        assert!(!default_line.contains("Also called 舞台"));
        assert!(other_line.contains("舞台服"));
        assert!(other_line.contains("Also called "));
        assert!(section.contains("if the short name is different"));
        assert_eq!(
            resolve_wear_directive(
                &WearDirective::Label("舞台装".into()),
                &looks,
                "default",
                None
            ),
            OverlayDecision::Wear("w-stage-2")
        );
        assert_eq!(
            resolve_wear_directive(
                &WearDirective::Label("舞台服".into()),
                &looks,
                "default",
                None
            ),
            OverlayDecision::Wear("w-stage-2")
        );
        assert_eq!(
            resolve_wear_directive(
                &WearDirective::Label("披肩".into()),
                &looks,
                "default",
                None
            ),
            OverlayDecision::Wear("w-stage-2")
        );
        assert_eq!(
            resolve_wear_directive(
                &WearDirective::Label("马甲".into()),
                &looks,
                "default",
                None
            ),
            OverlayDecision::Wear("w-stage-2")
        );
        assert_eq!(
            resolve_chat_outfit_overlay("我想看你舞台装", &looks, "default", None),
            OverlayDecision::Wear("w-stage-2")
        );
        assert_eq!(
            resolve_chat_outfit_overlay("披肩那套", &looks, "default", None),
            OverlayDecision::Wear("w-stage-2")
        );
        assert_eq!(
            resolve_chat_outfit_overlay("日常", &looks, "default", None),
            OverlayDecision::Unchanged
        );
    }

    #[test]
    fn default_outfit_does_not_occupy_a_clothing_style() {
        assert_eq!(
            resolve_chat_outfit_overlay("日常", &catalog(), "default", None),
            OverlayDecision::Unchanged
        );
        assert_eq!(
            resolve_chat_outfit_overlay("便服", &catalog(), "default", None),
            OverlayDecision::Unchanged
        );
    }

    #[test]
    fn live_unnamed_idol_wardrobe_wears_the_named_stage_set() {
        let profile = json!({
            "activeOutfitId": "default",
            "wardrobe": [
                {
                    "id": "default",
                    "clothingStyle": "idol",
                    "portraitAssetId": "/api/brew/image-cache/fb/default.png",
                    "rigAssetId": "61bf11a3fd6551f09c2c2cc1b52f20022791100de9edee6c59f625a0d6ecc4a3",
                    "outfit": { "outfitConstruction": "内层雪白方领无袖舞台胸衣，方领完全敞开" }
                },
                {
                    "id": "w-8d0eb1c5-837e-4b0f-9a08-87e6362b0c15",
                    "clothingStyle": "idol",
                    "portraitAssetId": "/api/brew/image-cache/22/stage.png",
                    "rigAssetId": "e4575c9b16a46e32d74d3393c1d4ff73028e9d12b1a4969d6c240f9aa5f5c7d7",
                    "outfit": { "outfitConstruction": "内层低开圆领短胸衣；中层斜裁舞台马甲。外层不对称缎带板块披肩" }
                }
            ]
        });
        let looks = looks_from_visual_profile(&profile);
        assert_eq!(looks[0].label, DEFAULT_WARDROBE_LABEL);
        assert_eq!(looks[1].label, "stage");
        let section = format_chat_wardrobe_section(&looks, "default", None).unwrap();
        let default_line = section
            .lines()
            .find(|line| line.starts_with("- Default outfit"))
            .unwrap();
        assert!(!default_line.contains("Also called 舞台"));
        let directive = wear_directive_after_reply(
            "想看你换舞台服唱歌",
            "行啊，你等着。先换了再说——唱完你可得老实夸我。",
            None,
        )
        .unwrap();
        assert_eq!(
            resolve_wear_directive(&directive, &looks, "default", None),
            OverlayDecision::Wear("w-8d0eb1c5-837e-4b0f-9a08-87e6362b0c15")
        );
    }

    #[test]
    fn a_forgotten_wear_marker_still_follows_the_named_set() {
        let profile = json!({
            "activeOutfitId": "default",
            "wardrobe": [
                {
                    "id": "default",
                    "clothingStyle": "idol",
                    "portraitAssetId": "/uploads/default.png",
                    "outfit": { "outfitConstruction": "雪白方领舞台胸衣" }
                },
                {
                    "id": "w-stage-2",
                    "clothingStyle": "idol",
                    "portraitAssetId": "/uploads/stage2.png",
                    "outfit": { "outfitConstruction": "斜裁舞台马甲，不对称缎带披肩" }
                }
            ]
        });
        let looks = looks_from_visual_profile(&profile);
        let directive = wear_directive_after_reply(
            "想看你换舞台服唱歌",
            "行啊，你等着。先换了再说——唱完你可得老实夸我。",
            None,
        )
        .unwrap();
        assert_eq!(
            resolve_wear_directive(&directive, &looks, "default", None),
            OverlayDecision::Wear("w-stage-2")
        );
        assert_eq!(
            wear_directive_after_reply("想看你舞台装", "穿的就是啊。早准备好了。", None),
            None
        );
        assert_eq!(
            wear_directive_after_reply("想看你舞台装", "好。", Some(WearDirective::Revert)),
            Some(WearDirective::Revert)
        );
    }
}
