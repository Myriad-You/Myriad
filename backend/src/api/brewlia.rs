//! Brewlia AI 增强功能 API
//!
//! 提供 AI 阅读辅助注释、内容理解等增强阅读功能
//!
//! 权限说明：
//! - 获取注释/播客：缓存命中谁都能读；无缓存仅管理员生成
//! - 重新生成 / 风格标签：仅管理员

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::middleware::auth::verify_current_admin_from_headers;
use crate::models::entities::brew_annotations::{self, AnnotationType};
use crate::services::ai::create_ai_analyzer_for_tier;
use crate::services::data_paths::paths;

fn brewlia_store_failed(
    context: &'static str,
    error: impl std::fmt::Display,
) -> axum::response::Response {
    tracing::error!(%error, context, "brewlia store failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "success": false,
            "error": format!("Failed to {context}"),
        })),
    )
        .into_response()
}

// 权限验证辅助函数

/// 验证是否是管理员（生成 / 重新生成 / 风格标签）
#[allow(clippy::result_large_err)]
async fn verify_admin(
    headers: &axum::http::HeaderMap,
    db: &sea_orm::DatabaseConnection,
) -> Result<(), axum::response::Response> {
    verify_current_admin_from_headers(headers, db)
        .await
        .map(|_| ())
        .map_err(|(status, body)| (status, body).into_response())
}

/// 创建 Brewlia API 路由
pub fn create_brewlia_routes(_app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    Router::<crate::state::AppState>::new()
        // 获取文章注释：缓存命中即返回；无缓存仅管理员生成
        .route("/items/{item_id}/annotations", get(get_annotations))
        // 重新生成注释
        .route(
            "/items/{item_id}/annotations/regenerate",
            post(regenerate_annotations),
        )
        // AI 播客：生成对话式文稿
        .route("/items/{item_id}/podcast", get(get_podcast_script))
        // AI 播客：强制重新生成
        .route(
            "/items/{item_id}/podcast/regenerate",
            post(regenerate_podcast_script),
        )
        // AI 风格标签：为订阅源生成风格标签
        .route("/sources/{source_id}/style-tags", post(generate_style_tags))
}

// 注释类型

/// 注释项（API 响应）
#[derive(Debug, Serialize, Clone)]
pub struct AnnotationItem {
    /// 注释 ID（持久化后有值）
    pub id: Option<i32>,
    /// 注释类型
    #[serde(rename = "type")]
    pub annotation_type: String,
    /// 原词/短语
    pub term: String,
    /// 注释说明
    pub explanation: String,
    /// 位置（可选）
    pub position: Option<i32>,
    /// 上下文提示（用于指代类注释）
    pub context_hint: Option<String>,
}

/// 注释响应
#[derive(Debug, Serialize)]
pub struct AnnotationsResponse {
    pub success: bool,
    pub annotations: Vec<AnnotationItem>,
    /// 是否从缓存读取
    pub from_cache: bool,
    /// 文章语言
    pub detected_language: Option<String>,
}

// 播客类型

/// 播客对话项
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PodcastDialogue {
    /// 说话者字符串（提示词示例 host_a/host_b；本层不校验枚举）
    pub speaker: String,
    /// 对话内容
    pub text: String,
}

/// 播客脚本响应
#[derive(Debug, Serialize)]
pub struct PodcastResponse {
    pub success: bool,
    /// 播客标题
    pub title: String,
    /// 对话列表
    pub dialogues: Vec<PodcastDialogue>,
    /// 检测到的语言
    pub language: Option<String>,
    /// 预计时长（秒）
    pub estimated_duration: i32,
}

// 获取注释

/// 获取文章注释
///
/// 游客和一般用户可访问已缓存的注释（只读）
/// 如果没有缓存且是管理员，则生成并保存
/// 如果没有缓存且非管理员，返回空数组
async fn get_annotations(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> impl IntoResponse {
    // 尝试从数据库获取（任何人都可以访问已缓存的注释）
    let existing = brew_annotations::Entity::find()
        .filter(brew_annotations::Column::ItemId.eq(item_id))
        .all(&db)
        .await;

    if let Ok(annotations) = existing {
        if !annotations.is_empty() {
            let items: Vec<AnnotationItem> = annotations
                .into_iter()
                .map(|a| AnnotationItem {
                    id: Some(a.id),
                    annotation_type: match a.annotation_type {
                        AnnotationType::Term => "term".to_string(),
                        AnnotationType::Reference => "reference".to_string(),
                        AnnotationType::Implicit => "implicit".to_string(),
                        AnnotationType::Context => "context".to_string(),
                        AnnotationType::Abbreviation => "abbreviation".to_string(),
                    },
                    term: a.term,
                    explanation: a.explanation,
                    position: a.position,
                    context_hint: a.context_hint,
                })
                .collect();

            return (
                StatusCode::OK,
                Json(json!(AnnotationsResponse {
                    success: true,
                    annotations: items,
                    from_cache: true,
                    detected_language: None,
                })),
            )
                .into_response();
        }
    }

    // 数据库没有缓存，检查是否是管理员（只有管理员可以生成新注释）
    if verify_admin(&headers, &db).await.is_err() {
        // 非管理员返回空数组（不生成新内容）
        return (
            StatusCode::OK,
            Json(json!(AnnotationsResponse {
                success: true,
                annotations: Vec::<AnnotationItem>::new(),
                from_cache: false,
                detected_language: None,
            })),
        )
            .into_response();
    }

    // 管理员：获取文章内容并生成注释
    use crate::models::entities::brew_items;
    let item = match brew_items::Entity::find_by_id(item_id).one(&db).await {
        Ok(Some(item)) => item,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(AppError::fail_json("Article not found")),
            )
                .into_response();
        }
        Err(e) => {
            return brewlia_store_failed("load article", e);
        }
    };

    let content = item
        .content
        .unwrap_or_else(|| item.summary.unwrap_or_default());
    if content.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("Article has no content")),
        )
            .into_response();
    }

    // 生成注释
    generate_and_save_annotations(&db, item_id, &content).await
}

/// 重新生成注释（仅管理员可用）
async fn regenerate_annotations(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> impl IntoResponse {
    // 验证管理员身份
    if let Err(e) = verify_admin(&headers, &db).await {
        return e.into_response();
    }

    // 删除该 item 已有 brew_annotations
    let _ = brew_annotations::Entity::delete_many()
        .filter(brew_annotations::Column::ItemId.eq(item_id))
        .exec(&db)
        .await;

    // 获取文章内容
    use crate::models::entities::brew_items;
    let item = match brew_items::Entity::find_by_id(item_id).one(&db).await {
        Ok(Some(item)) => item,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(AppError::fail_json("Article not found")),
            )
                .into_response();
        }
        Err(e) => {
            return brewlia_store_failed("load article", e);
        }
    };

    let content = item
        .content
        .unwrap_or_else(|| item.summary.unwrap_or_default());
    if content.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("Article has no content")),
        )
            .into_response();
    }

    generate_and_save_annotations(&db, item_id, &content).await
}

/// 生成注释并保存到数据库
async fn generate_and_save_annotations(
    db: &DatabaseConnection,
    item_id: i32,
    content: &str,
) -> axum::response::Response {
    // `create_ai_analyzer_for_tier(Standard)`（按配置 provider 路由）
    let ai_analyzer = match create_ai_analyzer_for_tier(crate::config::ModelTier::Standard).await {
        Some(analyzer) => analyzer,
        None => {
            tracing::error!("AI analyzer unavailable: no API key configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("AI service unavailable")),
            )
                .into_response();
        }
    };

    // 截到 30000 字节（`.len()`，可能落在 UTF-8 边界）
    let max_len = 30000;
    let truncated = if content.len() > max_len {
        &content[..max_len]
    } else {
        content
    };

    let prompt = build_annotation_prompt(truncated);

    // `json_object` 而不是 schema：提示词里已经写清了形状，这里只要求「必须是
    // 合法 JSON」——把最常撞的那道闸从源头消掉，又不会和提示词的形状打架。
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id().await;
    match ai_parse_with_retry(
        || {
            crate::services::ai_cost_ledger::with_site_ai_ledger(
                owner,
                "brewlia",
                "annotate",
                ai_analyzer.analyze_json("", &prompt, "brew_annotations", None),
            )
        },
        parse_annotations,
    )
    .await
    {
        Ok((annotations, detected_language)) => {
            tracing::info!(
                "Parsed {} annotations from AI response for item {}",
                annotations.len(),
                item_id
            );

            // 保存到数据库
            let mut saved_annotations = Vec::new();
            for ann in &annotations {
                let annotation_type = match ann.annotation_type.as_str() {
                    "reference" => AnnotationType::Reference,
                    "implicit" => AnnotationType::Implicit,
                    "context" => AnnotationType::Context,
                    "abbreviation" => AnnotationType::Abbreviation,
                    _ => AnnotationType::Term,
                };

                let active = brew_annotations::ActiveModel {
                    item_id: Set(item_id),
                    annotation_type: Set(annotation_type),
                    term: Set(ann.term.clone()),
                    explanation: Set(ann.explanation.clone()),
                    position: Set(ann.position),
                    context_hint: Set(ann.context_hint.clone()),
                    created_at: Set(Utc::now().into()),
                    ..Default::default()
                };

                match active.insert(db).await {
                    Ok(saved) => {
                        saved_annotations.push(AnnotationItem {
                            id: Some(saved.id),
                            annotation_type: ann.annotation_type.clone(),
                            term: ann.term.clone(),
                            explanation: ann.explanation.clone(),
                            position: ann.position,
                            context_hint: ann.context_hint.clone(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Failed to save annotation '{}': {}", ann.term, e);
                    }
                }
            }

            tracing::info!(
                "Returning {} annotations for item {}",
                saved_annotations.len(),
                item_id
            );

            (
                StatusCode::OK,
                Json(json!(AnnotationsResponse {
                    success: true,
                    annotations: saved_annotations,
                    from_cache: false,
                    detected_language,
                })),
            )
                .into_response()
        }
        Err(BrewliaAiFailure::Unusable(e)) => {
            tracing::warn!("Failed to parse AI annotations: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to parse AI response",
                    "code": "ai_response_invalid"
                })),
            )
                .into_response()
        }
        Err(BrewliaAiFailure::Provider(e)) => {
            tracing::error!("AI annotation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "AI generation failed",
                    "code": "ai_generation_failed",
                })),
            )
                .into_response()
        }
    }
}

/// 长文生成，两次够把「这一把没写好」和「提示词/模型真有问题」分开。
const BREWLIA_AI_ATTEMPTS: u8 = 2;

/// 模型没写好 vs 供应商挂了。两者在界面上不是一回事：前者再抽一次可能就好，
/// 后者是站长要去配的。
enum BrewliaAiFailure {
    Provider(String),
    Unusable(String),
}

/// 一次「调模型 → 严格解析」，模型这一把没写好就再抽一次（[`BREWLIA_AI_ATTEMPTS`]）。
/// 供应商失败（没配 key、网关挂了）立刻上抛，不重试。
async fn ai_parse_with_retry<T, F, Fut>(
    mut call: F,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<T, BrewliaAiFailure>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<String>>,
{
    let mut last = String::from("empty response");
    for attempt in 0..BREWLIA_AI_ATTEMPTS {
        let raw = call()
            .await
            .map_err(|error| BrewliaAiFailure::Provider(error.to_string()))?;
        match parse(&raw) {
            Ok(value) => return Ok(value),
            Err(reason) => {
                tracing::warn!(
                    attempt,
                    reason,
                    raw = %raw.chars().take(400).collect::<String>(),
                    "Brewlia AI response was unusable"
                );
                last = reason;
            }
        }
    }
    Err(BrewliaAiFailure::Unusable(last))
}

// AI 提示词

/// 构建阅读辅助注释提示词
fn build_annotation_prompt(content: &str) -> String {
    format!(
        r#"You are a professional reading assistant. Analyze the following article and provide helpful annotations.

## Task
Analyze the content and identify items that need annotation:

1. **Pronoun references (reference)**: Pronouns like "he/she/it", "this/that", explain what they specifically refer to
2. **Implicit information (implicit)**: Omitted subjects/objects, assumed background knowledge
3. **Technical terms (term)**: Technical jargon, professional terms that need explanation
4. **Cultural/background knowledge (context)**: Cultural, historical, regional context
5. **Abbreviations (abbreviation)**: Acronyms, shortened forms

## Important Rules
- **Write all explanations in the SAME LANGUAGE as the article**
- If the article is in Chinese, write explanations in Chinese
- If the article is in English, write explanations in English
- If the article is in Japanese, write explanations in Japanese
- Prioritize pronoun references and implicit content that affect comprehension
- Each annotation should help readers better understand the article
- Provide 15-20 annotations maximum
- Keep explanations concise

## Output Format
Output in JSON format:
```json
{{
  "language": "detected language code (e.g., zh-CN, zh-TW, en-US, ja-JP, ko-KR, fr-FR, de-DE)",
  "annotations": [
    {{
      "type": "reference",
      "term": "他",
      "explanation": "指代前文提到的张三",
      "context_hint": "出现在第二段"
    }},
    {{
      "type": "term",
      "term": "API",
      "explanation": "Application Programming Interface，应用程序编程接口"
    }}
  ]
}}
```

For English articles, annotations would look like:
```json
{{
  "language": "en-US",
  "annotations": [
    {{
      "type": "reference",
      "term": "they",
      "explanation": "refers to the development team mentioned earlier",
      "context_hint": "appears in paragraph 3"
    }},
    {{
      "type": "term",
      "term": "microservices",
      "explanation": "an architectural style that structures an application as a collection of loosely coupled services"
    }}
  ]
}}
```

## Article Content
{content}
"#,
        content = content
    )
}

// 解析逻辑

/// 解析 AI 返回的注释
fn parse_annotations(response: &str) -> Result<(Vec<AnnotationItem>, Option<String>), String> {
    let json_str = extract_json_from_response(response)?;

    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;

    let detected_language = parsed
        .get("language")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let annotations_value = parsed
        .get("annotations")
        .ok_or("Missing 'annotations' field")?;

    let annotations_array = annotations_value
        .as_array()
        .ok_or("'annotations' is not an array")?;

    let mut annotations = Vec::new();
    for item in annotations_array {
        let term = item
            .get("term")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        if term.is_empty() {
            continue;
        }

        let annotation_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("term")
            .to_string();

        let explanation = item
            .get("explanation")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let context_hint = item
            .get("context_hint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        annotations.push(AnnotationItem {
            id: None,
            annotation_type,
            term,
            explanation,
            position: None,
            context_hint,
        });
    }

    Ok((annotations, detected_language))
}

/// 从 AI 响应中提取 JSON
fn extract_json_from_response(response: &str) -> Result<String, String> {
    let json_content = if let Some(start) = response.find("```json") {
        let start = start + 7;
        if let Some(end) = response[start..].find("```") {
            response[start..start + end].trim().to_string()
        } else {
            response[start..].trim().to_string()
        }
    } else if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            response[start..=end].to_string()
        } else {
            response[start..].to_string()
        }
    } else {
        return Err("No JSON found in response".to_string());
    };

    fix_incomplete_json(&json_content)
}

/// 修复不完整的 JSON
fn fix_incomplete_json(json_str: &str) -> Result<String, String> {
    // 先尝试原样解析
    if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
        return Ok(json_str.to_string());
    }

    let mut fixed = json_str.trim().to_string();

    // 移除尾部逗号和不完整的字符串/对象
    fixed = clean_trailing_incomplete(&fixed);

    // 统计并闭合括号
    let (open_braces, open_brackets) = count_open_brackets(&fixed);

    for _ in 0..open_brackets {
        fixed.push(']');
    }
    for _ in 0..open_braces {
        fixed.push('}');
    }

    if serde_json::from_str::<serde_json::Value>(&fixed).is_ok() {
        tracing::debug!("Fixed JSON by closing brackets");
        return Ok(fixed);
    }

    // 如果简单修复失败，尝试提取已完整的注释对象
    extract_complete_annotations(json_str)
}

/// 清理尾部不完整的内容
fn clean_trailing_incomplete(json_str: &str) -> String {
    let mut fixed = json_str.trim().to_string();

    // 反复清理尾部问题
    for _ in 0..10 {
        let trimmed = fixed.trim_end();

        // 移除尾部逗号
        if let Some(stripped) = trimmed.strip_suffix(',') {
            fixed = stripped.to_string();
            continue;
        }

        // 移除不完整的键值对（以冒号结尾）
        if let Some(stripped) = trimmed.strip_suffix(':') {
            if let Some(quote_pos) = stripped.rfind('"') {
                fixed = trimmed[..quote_pos].trim_end().to_string();
                // 如果以逗号结尾，也删除
                if let Some(s) = fixed.strip_suffix(',') {
                    fixed = s.to_string();
                }
                continue;
            }
        }

        // 移除不完整的字符串值（奇数个引号）
        let quote_count = trimmed.chars().filter(|&c| c == '"').count();
        if quote_count % 2 != 0 {
            // 找到最后一个完整的值
            if let Some(last_complete) = find_last_complete_value(trimmed) {
                fixed = last_complete;
                continue;
            }
        }

        break;
    }

    fixed
}

/// 从后往前找到 depth==0 的逗号，返回其前缀
fn find_last_complete_value(s: &str) -> Option<String> {
    // 反向扫描：`}`/`]` 加深、`{`/`[` 变浅；真正截断点是 depth==0 的 `,`
    let mut depth = 0i32;
    let mut in_string = false;
    // 收集字符及其字节索引
    let char_indices: Vec<(usize, char)> = s.char_indices().collect();

    for i in (0..char_indices.len()).rev() {
        let (byte_idx, c) = char_indices[i];
        let prev = if i > 0 { char_indices[i - 1].1 } else { ' ' };

        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }

        if !in_string {
            match c {
                '}' | ']' => depth += 1,
                '{' | '[' => depth -= 1,
                ',' if depth == 0 => {
                    // 找到了一个分隔逗号，使用字节索引截取
                    return Some(s[..byte_idx].to_string());
                }
                _ => {}
            }
        }
    }

    None
}

/// 统计未闭合的括号数
fn count_open_brackets(s: &str) -> (i32, i32) {
    let mut open_braces = 0i32;
    let mut open_brackets = 0i32;
    let mut in_string = false;
    let mut prev_char = ' ';

    for c in s.chars() {
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            match c {
                '{' => open_braces += 1,
                '}' => open_braces -= 1,
                '[' => open_brackets += 1,
                ']' => open_brackets -= 1,
                _ => {}
            }
        }
        prev_char = c;
    }

    (open_braces.max(0), open_brackets.max(0))
}

/// 从不完整的 JSON 中提取已完整的注释对象
fn extract_complete_annotations(json_str: &str) -> Result<String, String> {
    // 查找 annotations 数组
    let annotations_start = json_str
        .find("\"annotations\"")
        .ok_or("Cannot find annotations field")?;

    let array_start = json_str[annotations_start..]
        .find('[')
        .ok_or("Cannot find annotations array")?;

    let array_start_pos = annotations_start + array_start;

    // 提取完整的对象
    let mut annotations = Vec::new();
    let mut depth = 0i32;
    let mut obj_start: Option<usize> = None;
    let mut in_string = false;
    let mut prev_char = ' ';

    for (i, c) in json_str[array_start_pos..].char_indices() {
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            match c {
                '[' if depth == 0 => depth = 1,
                '{' if depth == 1 => {
                    obj_start = Some(array_start_pos + i);
                    depth = 2;
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 1 {
                        if let Some(start) = obj_start {
                            let obj_str = &json_str[start..=array_start_pos + i];
                            // 验证是否是有效 JSON
                            if serde_json::from_str::<serde_json::Value>(obj_str).is_ok() {
                                annotations.push(obj_str.to_string());
                            }
                        }
                        obj_start = None;
                    }
                }
                ']' if depth == 1 => break,
                _ => {}
            }
        }
        prev_char = c;
    }

    if annotations.is_empty() {
        return Err("No complete annotation objects found".to_string());
    }

    // 提取 language 字段
    let language = extract_language_field(json_str);

    // 构建修复后的 JSON
    let fixed = format!(
        r#"{{"language": {}, "annotations": [{}]}}"#,
        language.unwrap_or_else(|| "null".to_string()),
        annotations.join(",")
    );

    tracing::debug!(
        "Extracted {} complete annotations from incomplete JSON",
        annotations.len()
    );
    Ok(fixed)
}

/// 提取 language 字段
fn extract_language_field(json_str: &str) -> Option<String> {
    let lang_start = json_str.find("\"language\"")?;
    let rest = &json_str[lang_start..];
    let colon = rest.find(':')?;
    let after_colon = rest[colon + 1..].trim_start();

    if let Some(stripped) = after_colon.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(format!("\"{}\"", &stripped[..end]))
    } else if after_colon.starts_with("null") {
        Some("null".to_string())
    } else {
        None
    }
}

// AI 播客功能

use crate::models::entities::brew_podcasts;

/// 获取文章的播客脚本
///
/// 游客和普通用户可以访问已缓存的播客脚本
/// 如果没有缓存，只有管理员可以生成新的播客脚本
async fn get_podcast_script(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> impl IntoResponse {
    // 先尝试从数据库获取缓存（任何人都可以访问缓存）
    if let Ok(Some(cached)) = brew_podcasts::Entity::find()
        .filter(brew_podcasts::Column::ItemId.eq(item_id))
        .one(&db)
        .await
    {
        tracing::debug!("Found cached podcast for item {}", item_id);
        let response = brew_podcasts::PodcastResponse::from(cached);
        return (StatusCode::OK, Json(json!(response))).into_response();
    }

    // 没有缓存时，验证管理员身份才能生成
    if verify_admin(&headers, &db).await.is_err() {
        // 非管理员返回空结果而不是错误
        return (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "podcast_script": null,
                "message": "No podcast script available. Only admins can generate new scripts."
            })),
        )
            .into_response();
    }

    // 获取文章内容
    use crate::models::entities::brew_items;
    let item = match brew_items::Entity::find_by_id(item_id).one(&db).await {
        Ok(Some(item)) => item,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(AppError::fail_json("Article not found")),
            )
                .into_response();
        }
        Err(e) => {
            return brewlia_store_failed("load article", e);
        }
    };

    let title = item.title.clone();
    let content = item
        .content
        .unwrap_or_else(|| item.summary.unwrap_or_default());

    if content.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("Article has no content")),
        )
            .into_response();
    }

    // 初始化 AI 服务（provider 感知）
    let ai_analyzer = match create_ai_analyzer_for_tier(crate::config::ModelTier::Standard).await {
        Some(analyzer) => analyzer,
        None => {
            tracing::error!("AI analyzer unavailable: no API key configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("AI service unavailable")),
            )
                .into_response();
        }
    };

    // 截到 20000 字节（`.len()`，可能落在 UTF-8 边界）
    let max_len = 20000;
    let content = if content.len() > max_len {
        &content[..max_len]
    } else {
        &content
    };

    // 构建播客提示词
    let prompt = build_podcast_prompt(&title, content);

    // 调用 AI 生成
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id().await;
    match ai_parse_with_retry(
        || {
            crate::services::ai_cost_ledger::with_site_ai_ledger(
                owner,
                "brewlia",
                "podcast",
                ai_analyzer.analyze_json("", &prompt, "brew_podcast", None),
            )
        },
        parse_podcast_script,
    )
    .await
    {
        Ok((dialogues, language)) => {
            // 估算时长：按 UTF-8 字节计，中文 0.15 秒/字节、英文 0.06 秒/字节；下限 60 秒。
            let is_chinese = language.as_deref().is_some_and(|l| l.starts_with("zh"));
            let char_count: usize = dialogues.iter().map(|d| d.text.len()).sum();
            let estimated_duration = if is_chinese {
                (char_count as f32 * 0.15) as i32
            } else {
                (char_count as f32 * 0.06) as i32
            };
            let estimated_duration = estimated_duration.max(60); // 最少 60 秒

            let podcast_title = format!("Deep dive: {}", title);

            // 保存到数据库
            let podcast_model = brew_podcasts::ActiveModel {
                item_id: Set(item_id),
                title: Set(podcast_title.clone()),
                language: Set(language.clone()),
                dialogues: Set(serde_json::to_value(&dialogues).unwrap_or_default()),
                estimated_duration: Set(Some(estimated_duration)),
                created_at: Set(Utc::now().into()),
                ..Default::default()
            };

            if let Err(e) = podcast_model.insert(&db).await {
                tracing::warn!("Failed to save podcast to database: {}", e);
                // 保存失败不影响返回结果
            } else {
                tracing::info!("Saved podcast for item {} to database", item_id);
            }

            (
                StatusCode::OK,
                Json(json!(PodcastResponse {
                    success: true,
                    title: podcast_title,
                    dialogues,
                    language,
                    estimated_duration,
                })),
            )
                .into_response()
        }
        Err(BrewliaAiFailure::Unusable(e)) => {
            tracing::warn!("Failed to parse podcast script: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to parse AI response",
                    "code": "ai_response_invalid"
                })),
            )
                .into_response()
        }
        Err(BrewliaAiFailure::Provider(e)) => {
            tracing::error!("AI podcast generation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "AI generation failed",
                    "code": "ai_generation_failed",
                })),
            )
                .into_response()
        }
    }
}

/// 强制重新生成播客脚本（删除缓存后重新生成，仅管理员可用）
async fn regenerate_podcast_script(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> axum::response::Response {
    // 验证管理员身份
    if let Err(e) = verify_admin(&headers, &db).await {
        return e.into_response();
    }

    // 获取文章信息以获取 source_id（用于清理 TTS 缓存）
    use crate::models::entities::brew_items;
    let source_id = match brew_items::Entity::find_by_id(item_id).one(&db).await {
        Ok(Some(item)) => item.source_id,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(AppError::fail_json("Article not found")),
            )
                .into_response();
        }
        Err(e) => {
            return brewlia_store_failed("load article", e);
        }
    };

    // 删除现有播客脚本缓存
    if let Err(e) = brew_podcasts::Entity::delete_many()
        .filter(brew_podcasts::Column::ItemId.eq(item_id))
        .exec(&db)
        .await
    {
        tracing::warn!("Failed to delete existing podcast cache: {}", e);
    } else {
        tracing::info!("Deleted existing podcast cache for item {}", item_id);
    }

    // 清理 TTS 音频缓存目录
    // 结构: `{brew}/{source_id}/{item_id}/tts/`
    let tts_dir = paths()
        .brew
        .join(source_id.to_string())
        .join(item_id.to_string())
        .join("tts");

    if tts_dir.exists() {
        match std::fs::remove_dir_all(&tts_dir) {
            Ok(_) => {
                tracing::info!("Deleted TTS cache directory: {:?}", tts_dir);
            }
            Err(e) => {
                tracing::warn!("Failed to delete TTS cache directory {:?}: {}", tts_dir, e);
            }
        }
    }

    // 接着走 `get_podcast_script`（无缓存则生成）
    get_podcast_script(State(db), headers, Path(item_id))
        .await
        .into_response()
}

/// 构建播客脚本提示词
fn build_podcast_prompt(title: &str, content: &str) -> String {
    format!(
        r#"You are a professional podcast script writer. Create an engaging two-host podcast dialogue based on the following article.

## Hosts
- **Host A (Alex)**: The main host who introduces topics and asks questions. Knowledgeable but curious.
- **Host B (Blake)**: The expert co-host who provides deeper insights and explanations. Analytical and engaging.

## Requirements
1. Create a natural, conversational dialogue between two hosts discussing the article
2. Start with a brief introduction of the topic
3. Break down complex concepts into digestible explanations
4. Include interesting observations, questions, and insights
5. End with a summary or takeaway
6. **Use the SAME LANGUAGE as the article** (if Chinese article, write Chinese dialogue)
7. Each dialogue turn should be 1-3 sentences (suitable for TTS)
8. Total 15-25 dialogue turns
9. Make it sound natural and engaging, not like reading an article

## Output Format
Output in JSON format:
```json
{{
  "language": "detected language code (zh-CN, zh-TW, en-US, ja-JP, ko-KR, fr-FR, de-DE)",
  "dialogues": [
    {{ "speaker": "host_a", "text": "Welcome to the show. Today we have a really interesting topic." }},
    {{ "speaker": "host_b", "text": "Yes, this one has been getting a lot of attention lately." }},
    {{ "speaker": "host_a", "text": "Let's dive in. First..." }}
  ]
}}
```

## Article Title
{title}

## Article Content
{content}
"#,
        title = title,
        content = content
    )
}

/// 解析播客脚本响应
fn parse_podcast_script(response: &str) -> Result<(Vec<PodcastDialogue>, Option<String>), String> {
    // 提取 JSON
    let json_str = match extract_json_from_response(response) {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to extract JSON: {}", e)),
    };

    // 尝试直接解析
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
        if let Some(dialogues_arr) = parsed.get("dialogues").and_then(|d| d.as_array()) {
            let dialogues: Vec<PodcastDialogue> = dialogues_arr
                .iter()
                .filter_map(|d| {
                    let speaker = d.get("speaker")?.as_str()?.to_string();
                    let text = d.get("text")?.as_str()?.to_string();
                    Some(PodcastDialogue { speaker, text })
                })
                .collect();

            if !dialogues.is_empty() {
                let language = parsed
                    .get("language")
                    .and_then(|l| l.as_str())
                    .map(|s| s.to_string());
                return Ok((dialogues, language));
            }
        }
    }

    // 尝试修复不完整的 JSON
    if let Ok(fixed) = fix_incomplete_podcast_json(&json_str) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&fixed) {
            if let Some(dialogues_arr) = parsed.get("dialogues").and_then(|d| d.as_array()) {
                let dialogues: Vec<PodcastDialogue> = dialogues_arr
                    .iter()
                    .filter_map(|d| {
                        let speaker = d.get("speaker")?.as_str()?.to_string();
                        let text = d.get("text")?.as_str()?.to_string();
                        Some(PodcastDialogue { speaker, text })
                    })
                    .collect();

                if !dialogues.is_empty() {
                    let language = parsed
                        .get("language")
                        .and_then(|l| l.as_str())
                        .map(|s| s.to_string());
                    return Ok((dialogues, language));
                }
            }
        }
    }

    Err("Failed to parse podcast dialogues".to_string())
}

/// 修复不完整的播客 JSON
fn fix_incomplete_podcast_json(json_str: &str) -> Result<String, String> {
    let mut dialogues: Vec<String> = Vec::new();
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut obj_start: Option<usize> = None;
    let mut prev_char = ' ';

    // 找到 dialogues 数组开始
    let dialogues_start = json_str
        .find("\"dialogues\"")
        .ok_or("No dialogues field found")?;
    let array_start = json_str[dialogues_start..]
        .find('[')
        .ok_or("No dialogues array found")?;
    let search_str = &json_str[dialogues_start + array_start..];

    for (i, c) in search_str.char_indices() {
        if escape_next {
            escape_next = false;
            prev_char = c;
            continue;
        }
        if c == '\\' && in_string {
            escape_next = true;
            prev_char = c;
            continue;
        }
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            match c {
                '{' => {
                    if depth == 1 && obj_start.is_none() {
                        obj_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if depth == 1 {
                        if let Some(start) = obj_start {
                            let obj_str = &search_str[start..=i];
                            // 验证对话对象
                            if obj_str.contains("\"speaker\"") && obj_str.contains("\"text\"") {
                                dialogues.push(obj_str.to_string());
                            }
                        }
                        obj_start = None;
                    }
                }
                '[' => {
                    if depth == 0 {
                        depth = 1;
                    }
                }
                ']' if depth == 1 => break,
                _ => {}
            }
        }
        prev_char = c;
    }

    if dialogues.is_empty() {
        return Err("No complete dialogue objects found".to_string());
    }

    let language = extract_language_field(json_str);
    let fixed = format!(
        r#"{{"language": {}, "dialogues": [{}]}}"#,
        language.unwrap_or_else(|| "null".to_string()),
        dialogues.join(",")
    );

    Ok(fixed)
}

// AI 风格标签功能

use crate::models::entities::brew_sources;

/// 风格标签响应
#[derive(Debug, Serialize)]
pub struct StyleTagsResponse {
    pub success: bool,
    /// 风格标签数组
    pub tags: Vec<String>,
    /// 是否从缓存读取
    pub from_cache: bool,
}

/// 为订阅源生成 AI 风格标签
///
/// 基于最近 10 篇文章的标题和前 100 字正文，让 AI 分析并生成风格标签
/// 仅管理员可调用
async fn generate_style_tags(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(source_id): Path<i32>,
) -> impl IntoResponse {
    // 验证管理员身份
    if let Err(e) = verify_admin(&headers, &db).await {
        return e.into_response();
    }

    // 获取订阅源信息
    let source = match brew_sources::Entity::find_by_id(source_id).one(&db).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(AppError::fail_json("Source not found")),
            )
                .into_response();
        }
        Err(e) => {
            return brewlia_store_failed("load source", e);
        }
    };

    // 获取最近 10 篇文章
    use crate::models::entities::brew_items;
    let items = match brew_items::Entity::find()
        .filter(brew_items::Column::SourceId.eq(source_id))
        .order_by_desc(brew_items::Column::PublishedAt)
        .limit(10)
        .all(&db)
        .await
    {
        Ok(items) => items,
        Err(e) => {
            return brewlia_store_failed("load articles", e);
        }
    };

    if items.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("No articles found for this source")),
        )
            .into_response();
    }

    // 构建文章摘要
    let mut articles_summary = String::new();
    for (i, item) in items.iter().enumerate() {
        // 预编译正则表达式（避免在循环中重复编译）
        static HTML_TAG_RE: once_cell::sync::Lazy<regex::Regex> =
            once_cell::sync::Lazy::new(|| regex::Regex::new(r"<[^>]+>").unwrap());

        let content = item
            .content
            .as_ref()
            .or(item.summary.as_ref())
            .map(|c| {
                // 清理 HTML 标签并截取前 100 字
                let cleaned = c
                    .replace("<br>", " ")
                    .replace("<br/>", " ")
                    .replace("<br />", " ")
                    .replace("</p>", " ")
                    .replace("</div>", " ");
                // 使用正则表达式清理 HTML 标签
                let cleaned = HTML_TAG_RE.replace_all(&cleaned, " ").to_string();
                cleaned.chars().take(100).collect::<String>()
            })
            .unwrap_or_default();

        articles_summary.push_str(&format!(
            "{}. Title: {}\n   Summary: {}\n\n",
            i + 1,
            item.title,
            content
        ));
    }

    tracing::info!(
        "Generating style tags for source {} ({}) with {} articles",
        source_id,
        source.name,
        items.len()
    );
    tracing::debug!("Articles summary for AI:\n{}", articles_summary);

    // 初始化 AI 服务（provider 感知）
    let ai_analyzer = match create_ai_analyzer_for_tier(crate::config::ModelTier::Standard).await {
        Some(analyzer) => analyzer,
        None => {
            tracing::error!("AI analyzer unavailable: no API key configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("AI service unavailable")),
            )
                .into_response();
        }
    };

    // 构建提示词
    let prompt = build_style_tags_prompt(&source.name, &articles_summary);

    // 调用 AI 生成
    // 这一处不换成 `json_object`：`parse_style_tags` 解析的是顶层数组，强制
    // 对象会直接砸掉它。改输出形状要连提示词和解析器一起动，那是另一件事。
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id().await;
    match ai_parse_with_retry(
        || {
            crate::services::ai_cost_ledger::with_site_ai_ledger(
                owner,
                "brewlia",
                "style_tags",
                ai_analyzer.analyze(&prompt),
            )
        },
        parse_style_tags,
    )
    .await
    {
        Ok(tags) => {
            // 保存到数据库
            let tags_json = serde_json::to_value(&tags).unwrap_or_default();
            let mut active: brew_sources::ActiveModel = source.into();
            active.ai_style_tags = Set(Some(tags_json));
            active.updated_at = Set(Utc::now().into());

            if let Err(e) = active.update(&db).await {
                tracing::warn!("Failed to save style tags: {}", e);
            } else {
                tracing::info!("Saved style tags for source {}: {:?}", source_id, tags);
            }

            (
                StatusCode::OK,
                Json(json!(StyleTagsResponse {
                    success: true,
                    tags,
                    from_cache: false,
                })),
            )
                .into_response()
        }
        Err(BrewliaAiFailure::Unusable(e)) => {
            tracing::warn!("Failed to parse style tags: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to parse AI response",
                    "code": "ai_response_invalid",
                    // 回解析原因，不把任意模型文本原样吐给客户端。
                    "reason": e
                })),
            )
                .into_response()
        }
        Err(BrewliaAiFailure::Provider(e)) => {
            tracing::error!("AI style tags generation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "AI generation failed",
                    "code": "ai_generation_failed",
                })),
            )
                .into_response()
        }
    }
}

/// 构建风格标签提示词
fn build_style_tags_prompt(source_name: &str, articles_summary: &str) -> String {
    format!(
        r#"You are a creative content analyst. Read the real articles below and produce 2 short, distinctive tags for this feed.

## Feed name
{source_name}

## Recent articles (read them)
{articles_summary}

## Rules
1. Judge from the actual article content, not just the feed name
2. Keep tags short: 2–4 characters for CJK, or 1–3 words for Latin scripts
3. Tags may cover topic, writing style, author voice, mood, reading experience, or any other angle
4. Be inventive: vivid, specific words are good, e.g. "码农日常", "深夜食堂", "硬核科普", "佛系更新", "late-night kitchen"
5. Avoid generic tags like "技术", "生活", "博客", "分享", "tech", "life", "blog"
6. The two tags should describe different angles so a reader sees what is distinctive
7. Write tags in the same language as the articles

## Output
JSON array only, no explanation:
["tag1", "tag2"]
"#,
        source_name = source_name,
        articles_summary = articles_summary
    )
}

/// 解析风格标签响应
fn parse_style_tags(response: &str) -> Result<Vec<String>, String> {
    // 尝试直接解析为 JSON 数组
    if let Ok(tags) = serde_json::from_str::<Vec<String>>(response.trim()) {
        return Ok(tags);
    }

    // 尝试从响应中提取 JSON 数组
    let json_str = extract_json_array_from_response(response)?;
    serde_json::from_str::<Vec<String>>(&json_str).map_err(|e| format!("JSON parse error: {}", e))
}

/// 从响应中提取 JSON 数组
fn extract_json_array_from_response(response: &str) -> Result<String, String> {
    // 查找 [ 和 ] 之间的内容
    if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            if end > start {
                return Ok(response[start..=end].to_string());
            }
        }
    }

    // 尝试从 markdown 代码块中提取
    if let Some(start) = response.find("```json") {
        let start = start + 7;
        if let Some(end) = response[start..].find("```") {
            let json_content = response[start..start + end].trim();
            if let Some(arr_start) = json_content.find('[') {
                if let Some(arr_end) = json_content.rfind(']') {
                    return Ok(json_content[arr_start..=arr_end].to_string());
                }
            }
        }
    }

    Err("No JSON array found in response".to_string())
}

#[cfg(test)]
mod ai_retry_tests {
    /// annotate/podcast/style_tags 三次调用都必须先 `ai_parse_with_retry`。
    #[test]
    fn every_reader_facing_call_retries_before_failing() {
        let source = include_str!("brewlia.rs");
        // 按字符取，不按字节：这个文件里全是中文注释，字节切片会切进半个字。
        let after_calls: Vec<String> = source
            .split("ai_parse_with_retry(")
            .skip(1)
            .map(|segment| segment.chars().take(300).collect())
            .collect();
        for op in ["\"annotate\"", "\"podcast\"", "\"style_tags\""] {
            assert!(
                after_calls.iter().any(|segment| segment.contains(op)),
                "{op} 仍然是一次调用就定生死"
            );
        }
        assert!(super::BREWLIA_AI_ATTEMPTS > 1, "次数是 1 等于没有重试");
    }

    /// 供应商失败不重试：那不是「这一把没写好」。
    #[test]
    fn a_broken_provider_is_not_retried() {
        let source = include_str!("brewlia.rs");
        let shell = source
            .split("async fn ai_parse_with_retry")
            .nth(1)
            .and_then(|rest| rest.split("\n// AI 提示词").next())
            .expect("retry shell");
        // 调用失败直接 `?` 上抛，不进循环的下一轮。
        assert!(shell.contains("BrewliaAiFailure::Provider(error.to_string()))?"));
        assert!(shell.contains("BrewliaAiFailure::Unusable(last)"));
    }

    /// annotate/podcast 走 `analyze_json`；style_tags 走 `analyze`（顶层数组）。
    #[test]
    fn only_the_object_rooted_calls_ask_for_json_mode() {
        let source = include_str!("brewlia.rs");
        assert!(source.contains(r#"analyze_json("", &prompt, "brew_annotations", None)"#));
        assert!(source.contains(r#"analyze_json("", &prompt, "brew_podcast", None)"#));

        let tags = source
            .split("\"style_tags\"")
            .nth(1)
            .and_then(|rest| rest.get(..200))
            .expect("style tags call");
        assert!(
            tags.contains("analyze(&prompt)"),
            "风格标签是顶层数组，不能强制 json_object"
        );
    }
}
use myriad_error::AppError;
