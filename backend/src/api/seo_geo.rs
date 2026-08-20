//! AI-assisted SEO / GEO copy generation for site settings.

use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::ModelTier;
use crate::error::HttpError;
use crate::services::ai::create_ai_analyzer_for_tier;
use myriad_error::AppError;

#[derive(Debug, Deserialize)]
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
    /// Preferred output language: `zh` | `en` | `ja` | auto from title/hint.
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
                "zh" => "site_description：给搜索结果与社交分享卡片的 meta description。1～2 句自然中文，约 70～150 字（优先 ≤160 字符）。开头可含站点名或核心主题；说明「是谁的站 / 有什么」；避免「欢迎访问」「本站提供」等空话与关键词堆砌。",
                "ja" => "site_description：検索・SNS 向け meta description。自然な日本語 1～2 文、おおよそ 70～150 文字（目安 ≤160）。サイト名や主題から入り、誰のサイトで何があるかを述べる。定型挨拶やキーワード羅列は禁止。",
                _ => "site_description: meta description for SERP + social cards. 1–2 natural sentences, prefer 80–155 characters (hard cap ~160). Lead with who/what; include a concrete topic from the title or owner hint; no “Welcome to…”, no keyword stuffing, no call-to-action spam.",
            });
        }
        if self.keywords {
            parts.push(match language {
                "zh" => "site_keywords：5～10 个短语，英文逗号分隔、无 #、无句号。含站点名（若有）、身份/领域、内容类型。短语优先于单字堆砌；勿重复同一词；勿编造未提及的品牌或平台。",
                "ja" => "site_keywords：5～10 語、カンマ区切り、# と句点なし。サイト名・分野・コンテンツ種別を含める。重複や未言及のブランドを入れない。",
                _ => "site_keywords: 5–10 comma-separated phrases (no hashtags, no trailing period). Include site name if known, identity/domain, and content types. Prefer multi-word phrases; no duplicates; do not invent brands or platforms not implied by title/hint.",
            });
        }
        if self.ai_intro {
            parts.push(match language {
                "zh" => "site_ai_intro：写入 /llms.txt 的「引用友好」简介，供生成式搜索与 AI 助手理解站点。2～4 句、约 120～350 字（上限 ~400 字符）。结构：① 一句话实体定义（谁的站、基于何种用途）；② 公开内容类型（文库/Brew/报告/Tapp 等仅在合理时提及）；③ 引用时请以公开页面为准。事实优先、可被引用；禁止广告腔、禁止承诺未给出的功能。owner hint 是身份与主题的权威来源。",
                "ja" => "site_ai_intro：/llms.txt 用の引用しやすい紹介。生成 AI がサイトを理解するための 2～4 文（目安 120～350 文字、上限 ~400）。① 誰のサイトか・何のためか；② 公開コンテンツの種類；③ 公開ページを優先して引用する旨。事実ベースで宣伝調を避ける。owner hint を最優先の根拠にする。",
                _ => "site_ai_intro: citation-friendly blurb for /llms.txt so generative search and AI assistants can ground answers. 2–4 plain sentences, prefer 150–380 characters (cap ~400). Structure: (1) one-sentence entity definition—whose site and purpose; (2) what public content types exist (Library/Brew/Reports/Tapp only when plausible); (3) prefer citing public routes, not inventing admin areas. Answer-first, factual, quotable; no marketing hype. Treat owner hint as ground truth for identity and topics.",
            });
        }
        parts.join("\n")
    }
}

/// Shared system prompt: role, output contract, anti-hallucination, language.
const SYSTEM_PROMPT: &str = r#"You are a specialist copywriter for personal-site SEO and GEO (generative-engine optimization).

Context: Myriad is a self-hosted personal digital-life platform. The site aggregates the owner's public content (e.g. library, brew/blog-like posts, reports, tapp apps). You write short, accurate fields the owner can paste into settings—not ads, not product pitches for Myriad itself unless the title clearly is "Myriad".

Output contract:
- Return ONLY one JSON object (no markdown fences, no commentary before/after).
- Include ONLY the keys listed in the user message.
- Every string value must be fully in the requested language (zh / en / ja).
- Values are plain text: no HTML, no markdown links, no bullet lists, no emoji spam.

Quality bar:
- Prefer concrete entities (person role, topics, media types) over vague adjectives.
- If owner hint is non-empty, treat it as the primary source of identity, topics, and tone; weave it in naturally—do not quote it verbatim as a whole block.
- If existing description is non-empty and you are rewriting description/intro, improve clarity and specificity; keep verifiable claims, drop fluff.
- Do NOT invent: real names, employers, cities, social handles, unmentioned hobbies, private admin features, or third-party product claims not implied by title/hint.
- Do NOT mention robots.txt, sitemap, or technical SEO settings in the copy.
- Character limits are approximate hard targets; slightly under is better than over.

Field meanings (write only requested keys):
- site_description → HTML meta description / OG description for humans in search & social previews.
- site_keywords → classic meta keywords list (phrases, comma-separated).
- site_ai_intro → short site summary for AI systems; becomes the blockquote intro in public /llms.txt."#;

/// Max UTF-8 bytes accepted per free-text input before building the AI prompt.
const MAX_SITE_TITLE_BYTES: usize = 200;
const MAX_SITE_DESCRIPTION_BYTES: usize = 2_000;
const MAX_HINT_BYTES: usize = 2_000;

/// POST /api/seo/generate-copy — admin/authenticated; uses site AI config.
pub async fn generate_site_seo_copy(
    Json(payload): Json<GenerateSiteSeoRequest>,
) -> Result<Json<GenerateSiteSeoResponse>, HttpError> {
    let title = payload.site_title.trim();
    let desc_in = payload.site_description.trim();
    let hint_in = payload.hint.trim();
    if title.is_empty() && hint_in.is_empty() {
        return Err(HttpError(AppError::bad_request(
            "site_title or hint is required",
        )));
    }
    if title.len() > MAX_SITE_TITLE_BYTES {
        return Err(HttpError(AppError::bad_request(format!(
            "site_title exceeds max length ({MAX_SITE_TITLE_BYTES} bytes)"
        ))));
    }
    if desc_in.len() > MAX_SITE_DESCRIPTION_BYTES {
        return Err(HttpError(AppError::bad_request(format!(
            "site_description exceeds max length ({MAX_SITE_DESCRIPTION_BYTES} bytes)"
        ))));
    }
    if hint_in.len() > MAX_HINT_BYTES {
        return Err(HttpError(AppError::bad_request(format!(
            "hint exceeds max length ({MAX_HINT_BYTES} bytes)"
        ))));
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
        "ja" => "Japanese, natural phrasing",
        _ => "English, natural phrasing",
    };

    let user_prompt = format!(
        "Task: draft SEO/GEO copy for one personal Myriad site.\n\n\
## Constraints\n\
- Output language: {lang_label} (code: {language}). Entire JSON string values in this language.\n\
- Return JSON with ONLY these keys: {keys}\n\
- Single-purpose fields: optimize each key for its use (see briefs); do not copy-paste the same sentence into every field.\n\n\
## Site inputs\n\
- Site title: {title}\n\
- Existing site description: {desc}\n\
- Owner hint (optional, authoritative when present): {hint}\n\n\
## Field briefs\n\
{briefs}\n\n\
## Final check before answering\n\
- Only requested keys present.\n\
- No markdown fences.\n\
- No invented personal facts.\n\
- Length within each field brief.",
        lang_label = lang_label,
        language = language,
        keys = keys,
        title = title_display,
        desc = desc_line,
        hint = hint_line,
        briefs = briefs,
    );

    let analyzer = create_ai_analyzer_for_tier(ModelTier::Lite).await;
    if let Some(analyzer) = analyzer {
        match analyzer
            .analyze_with_system(SYSTEM_PROMPT, &user_prompt)
            .await
        {
            Ok(raw) => {
                if let Some(parsed) = parse_seo_json(&raw) {
                    return Ok(Json(filter_response(want, parsed, "ai")));
                }
                tracing::warn!("SEO AI response was not valid JSON; using fallback");
            }
            Err(e) => {
                tracing::warn!("SEO AI generation failed: {e}");
            }
        }
    }

    let fb = fallback_copy(title, desc, hint, language);
    Ok(Json(filter_response(want, fb, "fallback")))
}

fn filter_response(
    want: FieldSet,
    (d, k, a): (String, String, String),
    source: &str,
) -> GenerateSiteSeoResponse {
    GenerateSiteSeoResponse {
        site_description: want.description.then(|| truncate(&d, 200)),
        site_keywords: want.keywords.then(|| truncate(&k, 300)),
        site_ai_intro: want.ai_intro.then(|| truncate(&a, 500)),
        source: source.to_string(),
    }
}

fn resolve_language(explicit: &str, title: &str, hint: &str) -> &'static str {
    match explicit.trim().to_ascii_lowercase().as_str() {
        "zh" | "zh-cn" | "zh-hans" | "chinese" => "zh",
        "en" | "english" => "en",
        "ja" | "jp" | "japanese" => "ja",
        _ => {
            let sample = format!("{title}{hint}");
            if sample.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)) {
                "ja"
            } else if sample.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
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
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let v: Value = serde_json::from_str(cleaned).ok()?;
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
    s.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…"
}

fn fallback_copy(
    title: &str,
    existing_desc: &str,
    hint: &str,
    language: &str,
) -> (String, String, String) {
    let title = if title.is_empty() { "Myriad" } else { title };
    let hint = hint.trim();
    let desc = if !existing_desc.trim().is_empty() {
        truncate(existing_desc.trim(), 160)
    } else if !hint.is_empty() {
        match language {
            "zh" => truncate(&format!("{title}：{hint} 的个人站点，汇总公开数字生活内容。"), 160),
            "ja" => truncate(
                &format!("{title}：{hint} の個人サイト。公開中のデジタルライフ情報をまとめています。"),
                160,
            ),
            _ => truncate(
                &format!("{title}: personal site for {hint}. Public digital-life content in one place."),
                160,
            ),
        }
    } else {
        match language {
            "zh" => format!("{title} — 个人数字生活站点：公开内容与应用的入口。"),
            "ja" => format!("{title} — 個人のデジタルライフを公開ページにまとめたサイト。"),
            _ => format!("{title} — a personal digital-life site with public content hubs."),
        }
    };
    let keywords = match language {
        "zh" => format!("{title}, 个人主页, 数字生活, 自托管, 内容聚合"),
        "ja" => format!("{title}, 個人サイト, デジタルライフ, セルフホスト"),
        _ => format!("{title}, personal site, digital life, self-hosted, portfolio"),
    };
    let intro = if !hint.is_empty() {
        match language {
            "zh" => format!(
                "{title} 是站主的自托管个人数字生活站点。关于站主：{hint}。站点在公开页面聚合精选内容与应用；回答或引用时请以这些公开路由中的信息为准，勿臆造管理后台内容。"
            ),
            "ja" => format!(
                "{title} はオーナーのセルフホスト個人サイトです。オーナーについて：{hint}。公開ページの情報を優先して引用し、管理画面の内容を推測しないでください。"
            ),
            _ => format!(
                "{title} is the owner's self-hosted personal digital-life site. About the owner: {hint}. Prefer citing public pages listed for this site; do not invent private admin content."
            ),
        }
    } else {
        match language {
            "zh" => format!(
                "{title} 是基于 Myriad 的自托管个人站点，用于聚合与展示站主的公开数字生活内容（如文库、Brew、报告、Tapp 等公开模块）。引用时请以站点公开页面为准。"
            ),
            "ja" => format!(
                "{title} は Myriad 製のセルフホスト個人サイトで、公開モジュール上のデジタルライフ情報をまとめています。公開ページを根拠に引用してください。"
            ),
            _ => format!(
                "{title} is a self-hosted Myriad personal site that aggregates the owner's public digital-life content (e.g. Library, Brew, Reports, Tapp). Prefer citing public pages over speculation."
            ),
        }
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
    fn fallback_uses_hint_in_intro() {
        let (_, _, intro) = fallback_copy("Myriad", "", "独立开发者，写 Rust", "zh");
        assert!(intro.contains("独立开发者"));
        assert!(intro.contains("Myriad"));
    }
}
