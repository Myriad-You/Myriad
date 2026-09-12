use crate::error::HttpError;
use crate::middleware::auth::Claims;
use axum::{Extension, Json};
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

/// 生成图片生成提示词的 API 端点
pub async fn generate_prompt(
    Extension(claims): Extension<Claims>,
    Json(payload): Json<GeneratePromptRequest>,
) -> Result<Json<GeneratePromptResponse>, HttpError> {
    tracing::info!("Generating prompt for: {}", payload.title);

    let system_prompt = r#"You are a professional prompt engineer for AI image generation.
Your task is to convert the user's description into a high-quality Studio Ghibli style illustration prompt.

Requirements:
1. Create prompts for transparent background PNG images with a single isolated subject
2. Use "A [character description]" format
3. Include quality keywords: masterpiece, highest quality, 8K resolution, ultra detailed
4. Add Ghibli-specific terms: Studio Ghibli art style, Hayao Miyazaki inspired, watercolor texture, hand-drawn animation style
5. The character should be cute chibi/kawaii style
6. Keep the prompt concise but descriptive (under 150 words)
7. If the subject is a known character (anime, game, etc.), include their accurate visual features
8. Output ONLY the prompt text, no explanations"#;

    let user_prompt = format!(
        "Create a Studio Ghibli style illustration prompt for this activity:\nTitle: {}\nDescription: {}\nCategory: {}",
        payload.title,
        payload.summary,
        payload.category.as_deref().unwrap_or("general")
    );

    let negative_prompt = "background, scenery, landscape, complex background, busy background, multiple subjects, crowd, low quality, blurry, distorted, deformed, ugly, bad anatomy, duplicate, text, watermark".to_string();

    // 尝试使用 AI 生成高质量提示词
    if let Some(analyzer) = create_ai_analyzer().await {
        let user_id = claims.sub.parse().unwrap_or(0);
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
        "A {}, {}, in Studio Ghibli art style, transparent background, PNG format, no background, isolated subject, masterpiece, highest quality, detailed character design, soft lighting, hand-drawn animation style, Hayao Miyazaki inspired, watercolor texture, gentle colors, whimsical atmosphere, professional illustration, 8K resolution, ultra detailed, cute kawaii style",
        character,
        activity_hint
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
        // 对于英文内容，可以包含一些具体信息
        let max_len = 40;
        if content.len() <= max_len {
            content
        } else {
            format!("{}...", &content[..max_len])
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
}
