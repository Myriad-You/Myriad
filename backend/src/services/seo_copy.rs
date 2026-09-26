//! AI-assisted SEO / GEO copy for the site settings: the description,
//! keywords and llms.txt intro, drafted from what the site shows its guests.
//! The settings form (`api::seo_geo`), the scheduled review and the agent
//! all draft through it.

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::ModelTier;
use crate::services::ai::create_ai_analyzer_for_tier;

#[derive(Debug, Deserialize, Default)]
pub struct GenerateSiteSeoRequest {
    /// Current site title (required for quality output).
    #[serde(default)]
    pub site_title: String,
    /// Existing description (optional context).
    #[serde(default)]
    pub site_description: String,
    /// Free-form hint from the owner (who they are, topics, tone).
    #[serde(default)]
    pub hint: String,
    /// Preferred output language: `zh` | `zh-TW` | `en` | `ja` | `ko` | `fr` | `de` | auto from title/hint.
    #[serde(default)]
    pub language: String,
    /// Which fields to generate: `site_description` | `site_keywords` | `site_ai_intro`.
    /// Empty / omit → all three.
    #[serde(default)]
    pub fields: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GenerateSiteSeoResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_keywords: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_ai_intro: Option<String>,
    /// `ai` | `fallback`
    pub source: String,
}

#[derive(Clone, Copy)]
struct FieldSet {
    description: bool,
    keywords: bool,
    ai_intro: bool,
}

impl FieldSet {
    fn from_request(fields: &[String]) -> Self {
        if fields.is_empty() {
            return Self {
                description: true,
                keywords: true,
                ai_intro: true,
            };
        }
        let mut s = Self {
            description: false,
            keywords: false,
            ai_intro: false,
        };
        for f in fields {
            match f.trim() {
                "site_description" | "description" => s.description = true,
                "site_keywords" | "keywords" => s.keywords = true,
                "site_ai_intro" | "ai_intro" => s.ai_intro = true,
                _ => {}
            }
        }
        if !s.description && !s.keywords && !s.ai_intro {
            // Unknown fields → generate all rather than empty response
            return Self {
                description: true,
                keywords: true,
                ai_intro: true,
            };
        }
        s
    }

    fn requested_keys_prompt(self) -> String {
        let mut keys = Vec::new();
        if self.description {
            keys.push("site_description");
        }
        if self.keywords {
            keys.push("site_keywords");
        }
        if self.ai_intro {
            keys.push("site_ai_intro");
        }
        keys.join(", ")
    }

    /// Per-field writing briefs injected into the user prompt.
    fn field_briefs(self, language: &str) -> String {
        let mut parts = Vec::new();
        if self.description {
            parts.push(match language {
                "zh" | "zh-TW" => "site_description：给搜索结果与社交分享卡片的 meta description。1～2 句自然中文，约 70～150 字（优先 ≤160 字符）。开头可含站点名或核心主题；说明「是谁的站 / 有什么」；避免「欢迎访问」「本站提供」等空话与关键词堆砌。",
                "ja" => "site_description：検索・SNS 向け meta description。自然な日本語 1～2 文、おおよそ 70～150 文字（目安 ≤160）。サイト名や主題から入り、誰のサイトで何があるかを述べる。定型挨拶やキーワード羅列は禁止。",
                "ko" => "site_description: 검색·SNS용 meta description. 자연스러운 한국어 1–2문장, 대략 70–150자(목적 ≤160). 사이트명이나 주제로 시작해 누구의 사이트인지, 무엇을 담았는지 말한다. 정형 인사나 키워드 나열 금지.",
                _ => "site_description: meta description for SERP + social cards. 1–2 natural sentences, prefer 80–155 characters (hard cap ~160). Lead with who/what; include a concrete topic from the title or owner hint; no “Welcome to…”, no keyword stuffing, no call-to-action spam.",
            });
        }
        if self.keywords {
            parts.push(match language {
                "zh" | "zh-TW" => "site_keywords：5～10 个短语，英文逗号分隔、无 #、无句号。含站点名（若有）、身份/领域、内容类型。短语优先于单字堆砌；勿重复同一词；勿编造未提及的品牌或平台。",
                "ja" => "site_keywords：5～10 語、カンマ区切り、# と句点なし。サイト名・分野・コンテンツ種別を含める。重複や未言及のブランドを入れない。",
                "ko" => "site_keywords: 5–10개 구, 쉼표 구분, #와 마침표 없음. 사이트명·분야·콘텐츠 종류 포함. 중복이나 언급되지 않은 브랜드를 넣지 말 것.",
                _ => "site_keywords: 5–10 comma-separated phrases (no hashtags, no trailing period). Include site name if known, identity/domain, and content types. Prefer multi-word phrases; no duplicates; do not invent brands or platforms not implied by title/hint.",
            });
        }
        if self.ai_intro {
            parts.push(match language {
                "zh" | "zh-TW" => "site_ai_intro：写入 /llms.txt 的「引用友好」简介。2～4 句、约 120～350 字（上限 ~400 字符）。结构：① 一句话实体定义（谁的站、做什么）；② 公开内容类型——只写 Public surface 里访客可见的模块/应用/写作；③ 引用以公开页面为准。事实优先；禁止广告腔。身份与主题以 owner hint 为准。",
                "ja" => "site_ai_intro：/llms.txt 用の引用しやすい紹介。2～4 文（目安 120～350 文字、上限 ~400）。① 誰のサイトか・何をするか；② 公開コンテンツは Public surface にゲスト公開とあるものだけ；③ 公開ページを優先して引用。事実ベース。身分・主題は owner hint が根拠。",
                "ko" => "site_ai_intro: /llms.txt용 인용하기 쉬운 소개. 2–4문장(대략 120–350자, 상한 ~400). ① 누구의 사이트인지·무엇을 하는지 ② 공개 콘텐츠는 Public surface에 손님에게 열린 것만 ③ 공개 페이지를 우선 인용. 사실 위주. 정체성·주제는 owner hint가 근거.",
                _ => "site_ai_intro: citation-friendly blurb for /llms.txt. 2–4 plain sentences, prefer 150–380 characters (cap ~400). (1) one-sentence entity definition; (2) public content types only from the Public surface guest-visible list; (3) cite public pages, do not invent admin areas. Factual, no hype. Owner hint is identity/topics.",
            });
        }
        parts.join("\n")
    }
}

/// Shared system prompt: role, output contract, anti-hallucination, language.
const SYSTEM_PROMPT: &str = r#"You are the copywriter for Agent SEO/GEO on one personal Myriad site (self-hosted digital-life homepage). You write settings fields the owner can paste. You are not advertising Myriad the product unless the site title itself is "Myriad".

Ground truth (highest wins):
1. Public surface in the user message — which modules/apps/writing actually exist for guests.
2. Owner hint — identity, topics, tone. Weave it in; do not paste it back as a block.
3. Existing description — keep verifiable claims, drop fluff, do not invent a new biography.

Output contract:
- Return ONLY one JSON object. No markdown fences, no commentary.
- Include ONLY the keys listed in the user message.
- Every string value fully in the requested language (zh / zh-TW / en / ja / ko / fr / de).
- Plain text only: no HTML, no markdown links, no bullets, no emoji spam.
- The three fields must not share the same sentence. Each field has a different job.

Quality bar:
- Concrete entities (role, topics, media types) over vague adjectives.
- Mention Library / Journal / Reports / Tapp / apps only when the public surface lists them as guest-visible.
- Do NOT invent: real names, employers, cities, social handles, unmentioned hobbies, private admin features, brands or platforms not in title/hint/public surface.
- Do NOT mention robots.txt, sitemap, llms.txt, or other technical SEO settings inside the copy values.
- Character limits are hard-ish; slightly under is better than over.

Field jobs (write only requested keys):
- site_description → humans: search snippet and social card. Who/what, 1–2 sentences.
- site_keywords → phrase list, comma-separated, no hashtags.
- site_ai_intro → machines: citation-friendly blurb that becomes the /llms.txt intro. Entity definition, then public content types, then “cite public pages”."#;

/// Max UTF-8 bytes accepted per free-text input before building the AI prompt.
const MAX_SITE_TITLE_BYTES: usize = 200;
const MAX_SITE_DESCRIPTION_BYTES: usize = 2_000;
const MAX_HINT_BYTES: usize = 2_000;

const DESC_OUT_CHARS: usize = 160;
const KEYWORDS_OUT_CHARS: usize = 300;
const INTRO_OUT_CHARS: usize = 400;

pub async fn generate_site_seo_copy_with_db(
    db: &DatabaseConnection,
    payload: GenerateSiteSeoRequest,
) -> Result<GenerateSiteSeoResponse, String> {
    let title = payload.site_title.trim();
    let desc_in = payload.site_description.trim();
    let hint_in = payload.hint.trim();
    if title.is_empty() && hint_in.is_empty() {
        return Err("site_title or hint is required".into());
    }
    if title.len() > MAX_SITE_TITLE_BYTES {
        return Err(format!(
            "site_title exceeds max length ({MAX_SITE_TITLE_BYTES} bytes)"
        ));
    }
    if desc_in.len() > MAX_SITE_DESCRIPTION_BYTES {
        return Err(format!(
            "site_description exceeds max length ({MAX_SITE_DESCRIPTION_BYTES} bytes)"
        ));
    }
    if hint_in.len() > MAX_HINT_BYTES {
        return Err(format!("hint exceeds max length ({MAX_HINT_BYTES} bytes)"));
    }

    let want = FieldSet::from_request(&payload.fields);
    let language = resolve_language(&payload.language, title, hint_in);
    let keys = want.requested_keys_prompt();
    let briefs = want.field_briefs(language);
    let title_display = if title.is_empty() {
        "(untitled personal site — infer only from owner hint)"
    } else {
        title
    };
    let desc = desc_in;
    let hint = hint_in;
    let desc_line = if desc.is_empty() {
        "(empty — draft from title and owner hint)"
    } else {
        desc
    };
    let hint_line = if hint.is_empty() {
        "(none — do not invent a personal bio; stay generic but concrete about a personal digital-life site)"
    } else {
        hint
    };
    let lang_label = match language {
        "zh" => "Chinese (Simplified), natural mainland phrasing",
        "zh-TW" => "Chinese (Traditional), natural Taiwan phrasing",
        "ja" => "Japanese, natural phrasing",
        "ko" => "Korean, natural phrasing",
        "fr" => "French, natural phrasing",
        "de" => "German, natural phrasing",
        _ => "English, natural phrasing",
    };

    let inventory = crate::services::public_site::public_geo_prompt_facts(db).await;
    let inventory_block = if inventory.trim().is_empty() {
        "Guest-visible modules unknown; stay generic about a personal digital-life site and do not invent sections.".to_string()
    } else {
        inventory
    };

    let user_prompt = format!(
        "Task: Agent SEO/GEO — draft settings copy for one personal site.\n\n\
## Constraints\n\
- Output language: {lang_label} (code: {language}). Entire JSON string values in this language.\n\
- Return JSON with ONLY these keys: {keys}\n\
- Each key has a different job (see briefs). Do not paste the same sentence into every field.\n\n\
## Site inputs\n\
- Site title: {title}\n\
- Existing site description: {desc}\n\
- Owner hint (identity/topics/tone; authoritative when present): {hint}\n\n\
## Public surface (ground truth for what exists; do not invent other sections)\n\
{inventory}\n\n\
## Field briefs\n\
{briefs}\n\n\
## Final check before answering\n\
- Only requested keys.\n\
- No markdown fences, no commentary.\n\
- No invented personal facts.\n\
- Modules/apps/writing mentioned only if listed as guest-visible above.\n\
- Length within each field brief.",
        lang_label = lang_label,
        language = language,
        keys = keys,
        title = title_display,
        desc = desc_line,
        hint = hint_line,
        inventory = inventory_block,
        briefs = briefs,
    );

    let analyzer = create_ai_analyzer_for_tier(ModelTier::Lite).await;
    if let Some(analyzer) = analyzer {
        let Ok(owner) = crate::services::ai_cost_ledger::resolve_site_owner_id().await else {
            tracing::warn!("SEO AI generation skipped: billing owner is unavailable");
            let fb = fallback_copy(title, desc, hint, language);
            return Ok(filter_response(want, fb, "fallback"));
        };
        match crate::services::ai_cost_ledger::with_site_ai_ledger(
            owner,
            "seo",
            "generate",
            analyzer.analyze_with_system(SYSTEM_PROMPT, &user_prompt),
        )
        .await
        {
            Ok(raw) => {
                if let Some(parsed) = parse_seo_json(&raw) {
                    return Ok(filter_response(want, parsed, "ai"));
                }
                tracing::warn!("SEO AI response was not valid JSON; using fallback");
            }
            Err(e) => {
                tracing::warn!("SEO AI generation failed: {e}");
            }
        }
    }

    let fb = fallback_copy(title, desc, hint, language);
    Ok(filter_response(want, fb, "fallback"))
}

fn filter_response(
    want: FieldSet,
    (d, k, a): (String, String, String),
    source: &str,
) -> GenerateSiteSeoResponse {
    GenerateSiteSeoResponse {
        site_description: want.description.then(|| truncate(&d, DESC_OUT_CHARS)),
        site_keywords: want.keywords.then(|| truncate(&k, KEYWORDS_OUT_CHARS)),
        site_ai_intro: want.ai_intro.then(|| truncate(&a, INTRO_OUT_CHARS)),
        source: source.to_string(),
    }
}

pub(crate) fn resolve_language(explicit: &str, title: &str, hint: &str) -> &'static str {
    match explicit.trim().to_ascii_lowercase().as_str() {
        "zh-tw" | "zh-hk" | "zh-mo" | "zh-hant" => "zh-TW",
        "zh" | "zh-cn" | "zh-hans" | "chinese" => "zh",
        "en" | "english" | "en-us" | "en-gb" => "en",
        "ja" | "jp" | "japanese" | "ja-jp" => "ja",
        "ko" | "ko-kr" | "korean" => "ko",
        "fr" | "fr-fr" | "french" => "fr",
        "de" | "de-de" | "german" => "de",
        _ => {
            let sample = format!("{title}{hint}");
            if sample
                .chars()
                .any(|c| ('\u{3040}'..='\u{30ff}').contains(&c))
            {
                "ja"
            } else if sample
                .chars()
                .any(|c| ('\u{ac00}'..='\u{d7a3}').contains(&c))
            {
                "ko"
            } else if sample
                .chars()
                .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
            {
                "zh"
            } else {
                "en"
            }
        }
    }
}

fn parse_seo_json(raw: &str) -> Option<(String, String, String)> {
    let cleaned = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let v: Value = match serde_json::from_str(cleaned) {
        Ok(v) => v,
        Err(_) => {
            let start = cleaned.find('{')?;
            let end = cleaned.rfind('}')?;
            if end <= start {
                return None;
            }
            serde_json::from_str(cleaned.get(start..=end)?).ok()?
        }
    };
    let d = v
        .get("site_description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let k = v
        .get("site_keywords")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let a = v
        .get("site_ai_intro")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if d.is_empty() && k.is_empty() && a.is_empty() {
        return None;
    }
    Some((d, k, a))
}

fn truncate(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    s.chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn fallback_copy(
    title: &str,
    existing_desc: &str,
    hint: &str,
    language: &str,
) -> (String, String, String) {
    let title = if title.is_empty() { "Myriad" } else { title };
    let hint = hint.trim();
    let locale = match language {
        "zh-TW" => "zh-TW",
        "zh" => "zh-CN",
        "ja" => "ja-JP",
        "ko" => "ko-KR",
        "fr" => "fr-FR",
        "de" => "de-DE",
        _ => "en-US",
    };
    let params = [("title", title), ("hint", hint)];
    let desc = if !existing_desc.trim().is_empty() {
        truncate(existing_desc.trim(), 160)
    } else if !hint.is_empty() {
        truncate(&crate::i18n::seo_f(locale, "descWithHint", &params), 160)
    } else {
        crate::i18n::seo_f(locale, "descPlain", &params)
    };
    let keywords = crate::i18n::seo_f(locale, "keywords", &params);
    let intro = if !hint.is_empty() {
        crate::i18n::seo_f(locale, "introWithHint", &params)
    } else {
        crate::i18n::seo_f(locale, "introPlain", &params)
    };
    (desc, keywords, truncate(&intro, 400))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_json() {
        let raw = r#"{"site_description":"Hello","site_keywords":"a, b","site_ai_intro":"Intro"}"#;
        let p = parse_seo_json(raw).unwrap();
        assert_eq!(p.0, "Hello");
        assert_eq!(p.1, "a, b");
    }

    #[test]
    fn parse_partial_json() {
        let raw = r#"{"site_keywords":"a, b, c"}"#;
        let p = parse_seo_json(raw).unwrap();
        assert_eq!(p.1, "a, b, c");
        assert!(p.0.is_empty());
    }

    #[test]
    fn parse_json_wrapped_in_prose() {
        let raw = "Sure.\n{\"site_description\":\"Hello\",\"site_keywords\":\"a\",\"site_ai_intro\":\"Intro\"}\nThanks";
        let p = parse_seo_json(raw).unwrap();
        assert_eq!(p.0, "Hello");
        assert_eq!(p.2, "Intro");
    }

    #[test]
    fn field_set_single() {
        let s = FieldSet::from_request(&["site_keywords".into()]);
        assert!(!s.description && s.keywords && !s.ai_intro);
        assert_eq!(s.requested_keys_prompt(), "site_keywords");
        let brief = s.field_briefs("en");
        assert!(brief.contains("site_keywords"));
        assert!(!brief.contains("site_description"));
        assert!(!brief.contains("site_ai_intro"));
    }

    #[test]
    fn field_briefs_ai_intro_zh() {
        let s = FieldSet::from_request(&["site_ai_intro".into()]);
        let brief = s.field_briefs("zh");
        assert!(brief.contains("llms.txt"));
        assert!(brief.contains("site_ai_intro"));
    }

    #[test]
    fn language_detect_zh() {
        assert_eq!(resolve_language("", "我的主页", ""), "zh");
    }

    #[test]
    fn language_detect_ko_and_explicit() {
        assert_eq!(resolve_language("", "나의 홈", ""), "ko");
        assert_eq!(resolve_language("ko-KR", "Myriad", ""), "ko");
        assert_eq!(resolve_language("fr-FR", "Myriad", ""), "fr");
    }

    #[test]
    fn field_briefs_ko() {
        let s = FieldSet::from_request(&["site_description".into()]);
        let brief = s.field_briefs("ko");
        assert!(brief.contains("meta description"));
        assert!(brief.contains("한국어"));
    }

    #[test]
    fn truncate_matches_serp_budget() {
        let long = "a".repeat(200);
        let out = truncate(&long, DESC_OUT_CHARS);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), DESC_OUT_CHARS);
    }

    #[test]
    fn fallback_uses_hint_in_intro() {
        let (_, _, intro) = fallback_copy("Myriad", "", "独立开发者，写 Rust", "zh");
        assert!(intro.contains("独立开发者"));
        assert!(intro.contains("Myriad"));
        let (_, keywords, _) = fallback_copy("Myriad", "", "", "zh-TW");
        assert!(keywords.contains("數位生活"));
        let (_, ko_keywords, _) = fallback_copy("Myriad", "", "", "ko");
        assert!(ko_keywords.contains("디지털"));
    }
}
