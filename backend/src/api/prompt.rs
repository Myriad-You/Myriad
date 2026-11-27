use crate::error::HttpError;
use crate::middleware::auth::Claims;
use axum::{Extension, Json, http::StatusCode};
use myriad_error::AppError;
use serde::{Deserialize, Serialize};

use crate::services::ai::create_ai_analyzer;

#[derive(Debug, Deserialize)]
pub struct GeneratePromptRequest {
    pub title: String,
    pub summary: String,
    pub category: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GeneratePromptResponse {
    pub prompt: String,
    pub negative_prompt: String,
}

/// 活动插画的画风：只写画法，不点名任何工作室、画师或作品。
macro_rules! activity_illustration_style {
    () => {
        "soft hand-drawn 2D animation illustration, clean thin linework, flat cel colors softened by a light watercolor wash texture, gentle warm lighting, muted natural palette, rounded simple shapes"
    };
}

const ACTIVITY_ILLUSTRATION_STYLE: &str = activity_illustration_style!();

const ACTIVITY_ILLUSTRATION_SYSTEM_PROMPT: &str = concat!(
    "You are a professional prompt engineer for AI image generation.
Your task is to convert the user's activity description into a high-quality illustration prompt for one original character.

Requirements:
1. Create prompts for transparent background PNG images with a single isolated subject
2. Use \"A [character description]\" format
3. Include quality keywords: masterpiece, highest quality, 8K resolution, ultra detailed
4. Describe the art style by technique only: ",
    activity_illustration_style!(),
    ". Do not name any studio, artist, or existing work
5. The character should be cute chibi/kawaii style
6. Keep the prompt concise but descriptive (under 150 words)
7. If the description says what the character looks like, follow that description; otherwise design an original character. Do not imitate or name characters from existing anime, games, or other media
8. Output ONLY the prompt text, no explanations"
);

fn activity_illustration_user_prompt(payload: &GeneratePromptRequest) -> String {
    format!(
        "Create an illustration prompt for this activity:\nTitle: {}\nDescription: {}\nCategory: {}",
        payload.title,
        payload.summary,
        payload.category.as_deref().unwrap_or("general")
    )
}

/// 生成图片生成提示词的 API 端点
pub async fn generate_prompt(
    Extension(claims): Extension<Claims>,
    Json(payload): Json<GeneratePromptRequest>,
) -> Result<Json<GeneratePromptResponse>, HttpError> {
    tracing::info!("Generating prompt for: {}", payload.title);

    let system_prompt = ACTIVITY_ILLUSTRATION_SYSTEM_PROMPT;
    let user_prompt = activity_illustration_user_prompt(&payload);

    let negative_prompt = "background, scenery, landscape, complex background, busy background, multiple subjects, crowd, low quality, blurry, distorted, deformed, ugly, bad anatomy, duplicate, text, watermark".to_string();

    // 尝试使用 AI 生成高质量提示词
    if let Some(analyzer) = create_ai_analyzer().await {
        let Some(user_id) = claims.durable_user_id() else {
            return Err(HttpError::from((
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("A durable user account is required")),
            )));
        };
        match crate::services::ai_cost_ledger::with_site_ai_ledger(
            user_id,
            "prompt",
            "generate",
            analyzer.analyze_with_system(system_prompt, &user_prompt),
        )
        .await
        {
            Ok(result) => {
                let cleaned = result.trim().trim_matches('"').trim_matches('`').trim();
                if !cleaned.is_empty() {
                    tracing::info!("AI-generated prompt length: {} chars", cleaned.len());
                    return Ok(Json(GeneratePromptResponse {
                        prompt: cleaned.to_string(),
                        negative_prompt,
                    }));
                }
            }
            Err(e) => {
                tracing::warn!("AI prompt generation failed, falling back to rules: {}", e);
            }
        }
    }

    // AI 不可用时使用规则引擎降级
    let prompt = generate_prompt_with_rules(&payload.title, &payload.summary);
    tracing::info!("Rule-generated prompt length: {} chars", prompt.len());

    Ok(Json(GeneratePromptResponse {
        prompt,
        negative_prompt,
    }))
}

fn content_hits(content: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| content.contains(needle))
}

/// 规则引擎生成提示词（AI 不可用时的降级路径）
fn generate_prompt_with_rules(title: &str, summary: &str) -> String {
    let content = format!("{} {}", title, summary).to_lowercase();

    // Leftover Chinese/Japanese tokens stay so existing titles still classify.
    let character = if content_hits(
        &content,
        &[
            "编程",
            "代码",
            "开发",
            "技术",
            "programming",
            "programmer",
            "coding",
            "プログラミング",
            "コード",
        ],
    ) {
        "cute chibi programmer character with laptop and glowing holographic code"
    } else if content_hits(&content, &["游戏", "game", "gaming", "ゲーム"]) {
        "cheerful gamer character holding game controller with pixel effects"
    } else if content_hits(&content, &["音乐", "歌", "music", "song", "音楽"]) {
        "gentle musician character with musical notes floating around"
    } else if content_hits(
        &content,
        &[
            "艺术",
            "设计",
            "画",
            "painting",
            "artwork",
            "illustration",
            "芸術",
            "デザイン",
        ],
    ) {
        "artistic character with paintbrush and colorful palette"
    } else if content_hits(&content, &["旅行", "旅游", "travel", "trip"]) {
        "adventurous traveler character with backpack and map"
    } else if content_hits(
        &content,
        &[
            "美食",
            "食物",
            "烹饪",
            "food",
            "cooking",
            "chef",
            "料理",
            "グルメ",
        ],
    ) {
        "happy chef character with chef hat and delicious food"
    } else if content_hits(&content, &["阅读", "书", "reading", "book", "読書"]) {
        "peaceful reader character holding an open book with glowing pages"
    } else if content_hits(
        &content,
        &["运动", "健身", "sport", "fitness", "workout", "運動"],
    ) {
        "energetic athlete character in active pose with motion effects"
    } else if content_hits(&content, &["学习", "教育", "study", "education", "学習"]) {
        "focused student character with books and lightbulb ideas"
    } else if content_hits(&content, &["社交", "分享", "social", "share", "交流"]) {
        "friendly character waving with speech bubbles and hearts"
    } else {
        "gentle character with soft smile and sparkles"
    };

    // 活动 hint：中文固定英文 filler，否则截 title+summary
    let activity_hint = extract_activity_context(title, summary);

    format!(
        "A {}, {}, {}, transparent background, PNG format, no background, isolated subject, masterpiece, highest quality, detailed character design, whimsical atmosphere, professional illustration, 8K resolution, ultra detailed, cute kawaii style",
        character, activity_hint, ACTIVITY_ILLUSTRATION_STYLE
    )
}

/// 提取活动上下文
fn extract_activity_context(title: &str, summary: &str) -> String {
    // title + summary；任一汉字则走固定 filler
    let content = format!("{} {}", title, summary);

    // 任一汉字即走中文分支（不是「主要是中文」）
    let has_chinese = content
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fa5}').contains(&c));

    if has_chinese {
        // 对于中文内容，只使用类型描述，不包含具体文本
        "engaged in their favorite activity".to_string()
    } else {
        // 其余文字截前 40 个字符放进提示。按字符而不是字节数：
        // 假名、带重音的拉丁字母和 emoji 都是多字节，按字节切会落在字符中间。
        const MAX_CHARS: usize = 40;
        if content.chars().count() <= MAX_CHARS {
            content
        } else {
            let clipped: String = content.chars().take(MAX_CHARS).collect();
            format!("{clipped}...")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_prompt_classifies_english_and_leftover_chinese() {
        let en = generate_prompt_with_rules("Weekend programming", "");
        let zh = generate_prompt_with_rules("周末编程", "");
        let ja = generate_prompt_with_rules("週末のプログラミング", "");
        assert!(en.contains("programmer"));
        assert!(zh.contains("programmer"));
        assert!(ja.contains("programmer"));
        let music = generate_prompt_with_rules("New song", "studio session");
        assert!(music.contains("musician"));
        let generic = generate_prompt_with_rules("Hello", "world");
        assert!(generic.contains("gentle character"));
    }

    /// 工作室、画师、作品名（英、中、日），任何一个都不应出现在提示里。
    const NAMED_STYLE_SOURCES: &[&str] = &[
        "ghibli",
        "miyazaki",
        "hayao",
        "kyoto animation",
        "makoto shinkai",
        "mihoyo",
        "hoyoverse",
        "genshin",
        "pixar",
        "disney",
        "吉卜力",
        "宫崎骏",
        "宮崎駿",
        "新海诚",
        "米哈游",
        "原神",
        "ジブリ",
        "宮崎",
        "新海誠",
        "崩坏",
    ];

    fn named_sources_in(prompt: &str) -> Vec<&'static str> {
        let lower = prompt.to_lowercase();
        NAMED_STYLE_SOURCES
            .iter()
            .copied()
            .filter(|name| lower.contains(name))
            .collect()
    }

    fn request(title: &str, summary: &str) -> GeneratePromptRequest {
        GeneratePromptRequest {
            title: title.to_string(),
            summary: summary.to_string(),
            category: Some("hobby".to_string()),
        }
    }

    #[test]
    fn system_prompt_names_no_studio_artist_or_work() {
        assert_eq!(
            named_sources_in(ACTIVITY_ILLUSTRATION_SYSTEM_PROMPT),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn user_prompt_names_no_studio_artist_or_work() {
        let prompt = activity_illustration_user_prompt(&request("Weekend sketching", "park"));
        assert_eq!(named_sources_in(&prompt), Vec::<&str>::new());
    }

    #[test]
    fn rule_prompt_names_no_studio_artist_or_work() {
        for (title, summary) in [
            ("Weekend programming", ""),
            ("周末编程", "写代码"),
            ("New song", "studio session"),
            ("Hello", "world"),
        ] {
            let prompt = generate_prompt_with_rules(title, summary);
            assert_eq!(named_sources_in(&prompt), Vec::<&str>::new(), "{prompt}");
        }
    }

    #[test]
    fn system_prompt_no_longer_asks_to_reproduce_known_characters() {
        let prompt = ACTIVITY_ILLUSTRATION_SYSTEM_PROMPT.to_lowercase();
        assert!(!prompt.contains("known character"));
        assert!(!prompt.contains("accurate visual features"));
        assert!(prompt.contains("do not imitate or name characters from existing"));
    }

    #[test]
    fn system_prompt_follows_the_described_appearance() {
        assert!(ACTIVITY_ILLUSTRATION_SYSTEM_PROMPT.contains(
            "If the description says what the character looks like, follow that description"
        ));
    }

    /// 经 HTTP 处理函数走规则降级路径。测试进程里没有配置文本模型，
    /// 先确认这一点，免得误打到付费模型。
    async fn rule_prompt_through_handler(title: &str, summary: &str) -> String {
        assert!(
            create_ai_analyzer().await.is_none(),
            "this test must run without a configured text model"
        );
        let claims = Claims {
            sub: "1".into(),
            username: "prompt-test".into(),
            is_admin: true,
            is_owner: false,
            exp: crate::clock::now().timestamp() + 60,
            iat: crate::clock::now().timestamp(),
            tv: 0,
            subject: crate::middleware::auth::AuthSubject::from_test_sub("1"),
        };
        let Json(response) = generate_prompt(Extension(claims), Json(request(title, summary)))
            .await
            .unwrap_or_else(|_| panic!("prompt generation failed"));
        response.prompt
    }

    /// 截出来的活动片段：提示里 character 之后、画风之前的那一段。
    fn activity_hint_of(prompt: &str) -> &str {
        let after_character = prompt.split_once(", ").expect("character").1;
        after_character
            .split_once(&format!(", {ACTIVITY_ILLUSTRATION_STYLE}"))
            .expect("style")
            .0
    }

    #[tokio::test]
    async fn rule_prompt_clips_multibyte_text_on_char_boundaries() {
        let kana = "ひらがなとカタカナだけでかいたとてもながいしゅうまつのかつどうのせつめいですよろしくね";
        let emoji_at_boundary = format!("{}👍🏽👨‍👩‍👧 tail", "a".repeat(38));
        let mixed = "Weekend café ｍｅｅｔｕｐ with カフェ friends and naïve résumé talk";
        for (title, summary) in [
            (kana, ""),
            ("", kana),
            (emoji_at_boundary.as_str(), "more"),
            (mixed, "ünïcödé"),
        ] {
            let prompt = rule_prompt_through_handler(title, summary).await;
            let hint = activity_hint_of(&prompt);
            let clipped = hint.strip_suffix("...").expect("long text is clipped");
            assert_eq!(clipped.chars().count(), 40, "{hint}");
            assert!(format!("{title} {summary}").starts_with(clipped), "{hint}");
        }
    }

    #[tokio::test]
    async fn rule_prompt_keeps_short_and_empty_text_whole() {
        for (title, summary) in [("Café", "ｎｉｇｈｔ"), ("かつどう", "👍🏽"), ("", "")]
        {
            let prompt = rule_prompt_through_handler(title, summary).await;
            assert_eq!(activity_hint_of(&prompt), format!("{title} {summary}"));
        }
    }

    #[test]
    fn both_paths_carry_the_same_technique_description() {
        assert!(ACTIVITY_ILLUSTRATION_SYSTEM_PROMPT.contains(ACTIVITY_ILLUSTRATION_STYLE));
        assert!(generate_prompt_with_rules("Hello", "world").contains(ACTIVITY_ILLUSTRATION_STYLE));
    }
}
