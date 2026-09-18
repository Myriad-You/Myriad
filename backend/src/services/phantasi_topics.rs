//! Phantasi 订阅文章主题。
//!
//! 主题名是自由文本，不是 10 个预置 key。一篇一个；空主题写 NULL，
//! **不要**写「其他」——聚类靠 NULL 把这些文章留在源磁贴里。
//!
//! 写入两条路：
//! - 站长手填（HTTP PUT）
//! - Lite 按标题/摘要快速归类：先从本站已有主题里筛，对不上再起短名
//!
//! 归类只走严格 Lite（短 JSON、限输出），Lite 没配好就保持 NULL，
//! **不能**落到 Standard / Pro。笔记源不走这里。

use std::time::Duration;

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
    sea_query::Expr,
};
use serde_json::json;

use crate::models::entities::phantasi_sources::SourceType;
use crate::models::entities::{phantasi_items, phantasi_sources};
use crate::services::ai::create_strict_lite_ai_analyzer_with_timeout;
use crate::services::analyzer::OutputBudget;

/// 与笔记分类同一上限：短名才适合磁贴。
pub const TOPIC_NAME_MAX_CHARS: usize = 40;
/// 给 Lite 看的正文前缀。归类不需要读完全文。
const EXCERPT_CHARS: usize = 240;
/// 提示词里最多带多少个已有主题，避免把词表整表塞进去。
const EXISTING_TOPIC_PROMPT_CAP: usize = 40;
/// 一次抓取/订阅最多打多少篇。剩下的留给站长手改或阅读器里再推荐。
const MAX_INGEST_SUGGESTS: usize = 20;
const CLASSIFY_TIMEOUT: Duration = Duration::from_secs(12);
const CLASSIFY_OUTPUT_BUDGET: OutputBudget = OutputBudget { max_tokens: 512 };
const CLASSIFY_SCHEMA_NAME: &str = "phantasi_topic";
const CLASSIFY_SYSTEM: &str = "\
Classify one subscription article into a single short topic. \
Prefer an existing topic. Reply with JSON only.";

/// AI 不会主动产出这些桶名。站长手填不受此限。
const REJECTED_AI_TOPICS: &[&str] = &[
    "other",
    "misc",
    "unknown",
    "none",
    "null",
    "n/a",
    "其他",
    "未分类",
    "未分類",
    "その他",
    "없음",
];

#[derive(Debug)]
pub enum TopicWriteError {
    NotFound,
    NoteItem,
    Store,
}

#[derive(Debug)]
pub enum TopicSuggestError {
    NotFound,
    NoteItem,
    Unavailable,
    Failed,
    Store,
}

pub fn normalize_topic_name(value: &str) -> Option<String> {
    let name = value.trim();
    if name.is_empty() {
        return None;
    }
    Some(name.chars().take(TOPIC_NAME_MAX_CHARS).collect())
}

pub fn is_rejected_ai_topic(name: &str) -> bool {
    let folded = name.trim().to_lowercase();
    REJECTED_AI_TOPICS
        .iter()
        .any(|rejected| folded == rejected.to_lowercase())
}

fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn excerpt_for_topic(summary: Option<&str>, content: Option<&str>) -> String {
    let raw = summary.or(content).unwrap_or("");
    strip_html(raw)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(EXCERPT_CHARS)
        .collect()
}

fn extract_json_object(response: &str) -> Option<&str> {
    let start = response.find('{')?;
    let end = response.rfind('}')?;
    if end < start {
        return None;
    }
    Some(&response[start..=end])
}

#[derive(Debug, serde::Deserialize)]
struct TopicSuggestion {
    topic: Option<String>,
}

/// 解析模型回的 `{"topic": "Rust"}` / `{"topic": null}`。
pub fn parse_suggested_topic(response: &str) -> Result<Option<String>, String> {
    let json = extract_json_object(response).ok_or_else(|| "no JSON object".to_string())?;
    let parsed: TopicSuggestion =
        serde_json::from_str(json).map_err(|e| format!("JSON parse error: {e}"))?;
    let Some(name) = parsed.topic.as_deref().and_then(normalize_topic_name) else {
        return Ok(None);
    };
    if is_rejected_ai_topic(&name) {
        return Ok(None);
    }
    Ok(Some(name))
}

fn topic_response_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "topic": { "type": ["string", "null"] },
        },
        "required": ["topic"],
    })
}

pub fn build_topic_suggest_prompt(title: &str, excerpt: &str, existing: &[String]) -> String {
    let catalog = if existing.is_empty() {
        "(none yet — invent a short name if the topic is clear)".to_string()
    } else {
        existing
            .iter()
            .take(EXISTING_TOPIC_PROMPT_CAP)
            .map(|name| format!("- {name}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        r#"Screen this article into one topic.

## Existing topics (pick one of these when it fits)
{catalog}

## Article
Title: {title}
Excerpt: {excerpt}

## Rules
1. If an existing topic fits, copy that name exactly
2. If none fit and the topic is clear, invent a short new name: 2–8 CJK characters or 1–3 Latin words
3. If the topic is unclear, return null
4. Same language as the article
5. Do not use generic buckets: other, misc, 其他, 未分类, unknown
6. Do not explain

## Output
{{"topic":"Rust"}} or {{"topic":null}}
"#
    )
}

async fn list_existing_subscription_topics(
    db: &DatabaseConnection,
    include_admin_only: bool,
) -> Result<Vec<String>, sea_orm::DbErr> {
    let mut hidden = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(SourceType::Note));
    if !include_admin_only {
        hidden = phantasi_sources::Entity::find().filter(
            sea_orm::Condition::any()
                .add(phantasi_sources::Column::SourceType.eq(SourceType::Note))
                .add(phantasi_sources::Column::AdminOnly.eq(true)),
        );
    }
    let hidden_ids: Vec<i32> = hidden
        .select_only()
        .column(phantasi_sources::Column::Id)
        .into_tuple()
        .all(db)
        .await?;

    let mut query = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::Topic.is_not_null())
        .filter(phantasi_items::Column::Topic.ne(""))
        .select_only()
        .column(phantasi_items::Column::Topic)
        .distinct();
    if !hidden_ids.is_empty() {
        query = query.filter(phantasi_items::Column::SourceId.is_not_in(hidden_ids));
    }

    let rows: Vec<Option<String>> = query.into_tuple().all(db).await?;
    let mut names: Vec<String> = rows
        .into_iter()
        .flatten()
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// 订阅主题目录。笔记分类不进这里。
pub async fn list_subscription_topic_names(
    db: &DatabaseConnection,
    include_admin_only: bool,
) -> Result<Vec<String>, sea_orm::DbErr> {
    list_existing_subscription_topics(db, include_admin_only).await
}

async fn ensure_subscription_item(
    db: &DatabaseConnection,
    item_id: i32,
) -> Result<(), TopicWriteError> {
    let source_id = phantasi_items::Entity::find_by_id(item_id)
        .select_only()
        .column(phantasi_items::Column::SourceId)
        .into_query();
    let source_type = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::Id.in_subquery(source_id))
        .select_only()
        .column(phantasi_sources::Column::SourceType)
        .into_tuple::<SourceType>()
        .one(db)
        .await
        .map_err(|_| TopicWriteError::Store)?
        .ok_or(TopicWriteError::NotFound)?;
    if source_type == SourceType::Note {
        return Err(TopicWriteError::NoteItem);
    }
    Ok(())
}

pub async fn set_subscription_item_topic(
    db: &DatabaseConnection,
    item_id: i32,
    topic: Option<&str>,
) -> Result<Option<String>, TopicWriteError> {
    ensure_subscription_item(db, item_id).await?;
    let normalized = topic.and_then(normalize_topic_name);
    let result = phantasi_items::Entity::update_many()
        .col_expr(
            phantasi_items::Column::Topic,
            Expr::value(normalized.clone()),
        )
        .filter(phantasi_items::Column::Id.eq(item_id))
        .exec(db)
        .await
        .map_err(|_| TopicWriteError::Store)?;
    if result.rows_affected == 0 {
        return Err(TopicWriteError::NotFound);
    }
    Ok(normalized)
}

async fn suggest_topic_name(
    title: &str,
    excerpt: &str,
    existing: &[String],
) -> Result<Option<String>, TopicSuggestError> {
    let analyzer = create_strict_lite_ai_analyzer_with_timeout(Some(CLASSIFY_TIMEOUT))
        .await
        .ok_or(TopicSuggestError::Unavailable)?;
    let prompt = build_topic_suggest_prompt(title, excerpt, existing);
    let schema = topic_response_schema();
    let owner = crate::services::ai_cost_ledger::resolve_site_owner_id()
        .await
        .map_err(|_| TopicSuggestError::Unavailable)?;
    let raw = crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "phantasi",
        "topic_classify",
        analyzer.analyze_json_short(
            CLASSIFY_SYSTEM,
            &prompt,
            CLASSIFY_SCHEMA_NAME,
            Some(&schema),
            CLASSIFY_OUTPUT_BUDGET,
        ),
    )
    .await
    .map_err(|error| {
        tracing::error!(%error, "subscription topic Lite provider failed");
        TopicSuggestError::Unavailable
    })?;
    parse_suggested_topic(&raw).map_err(|reason| {
        tracing::warn!(
            reason,
            raw = %raw.chars().take(200).collect::<String>(),
            "subscription topic Lite response was unusable"
        );
        TopicSuggestError::Failed
    })
}

async fn write_suggested_topic(
    db: &DatabaseConnection,
    item_id: i32,
    topic: Option<String>,
) -> Result<Option<String>, TopicSuggestError> {
    let result = phantasi_items::Entity::update_many()
        .col_expr(phantasi_items::Column::Topic, Expr::value(topic.clone()))
        .filter(phantasi_items::Column::Id.eq(item_id))
        .exec(db)
        .await
        .map_err(|_| TopicSuggestError::Store)?;
    if result.rows_affected == 0 {
        return Err(TopicSuggestError::NotFound);
    }
    Ok(topic)
}

/// 给一篇订阅文章建议主题。`apply` 为真时写回。
pub async fn suggest_subscription_item_topic(
    db: &DatabaseConnection,
    item_id: i32,
    apply: bool,
) -> Result<Option<String>, TopicSuggestError> {
    ensure_subscription_item(db, item_id)
        .await
        .map_err(|err| match err {
            TopicWriteError::NotFound => TopicSuggestError::NotFound,
            TopicWriteError::NoteItem => TopicSuggestError::NoteItem,
            TopicWriteError::Store => TopicSuggestError::Store,
        })?;
    let (title, excerpt) = phantasi_items::Entity::find_by_id(item_id)
        .select_only()
        .column(phantasi_items::Column::Title)
        .column_as(
            Expr::cust("LEFT(COALESCE(summary, content), 8192)"),
            phantasi_items::Column::Summary,
        )
        .into_tuple::<(String, Option<String>)>()
        .one(db)
        .await
        .map_err(|_| TopicSuggestError::Store)?
        .ok_or(TopicSuggestError::NotFound)?;
    let existing = list_existing_subscription_topics(db, true)
        .await
        .map_err(|_| TopicSuggestError::Store)?;
    let excerpt = excerpt_for_topic(excerpt.as_deref(), None);
    let topic = suggest_topic_name(&title, &excerpt, &existing).await?;
    if apply {
        write_suggested_topic(db, item_id, topic).await
    } else {
        Ok(topic)
    }
}

async fn recommend_one(db: &DatabaseConnection, item_id: i32) {
    match suggest_subscription_item_topic(db, item_id, true).await {
        Ok(Some(topic)) => {
            tracing::info!(item_id, %topic, "assigned subscription topic");
        }
        Ok(None) => {
            tracing::debug!(item_id, "subscription topic left empty");
        }
        Err(TopicSuggestError::Unavailable) => {
            tracing::debug!(item_id, "subscription topic AI unavailable");
        }
        Err(err) => {
            tracing::warn!(item_id, ?err, "subscription topic suggest failed");
        }
    }
}

/// 入库后异步打标。一篇一篇来，好让后文复用刚写出的新名字。
pub async fn recommend_topics_for_item_ids(db: &DatabaseConnection, item_ids: &[i32]) {
    if item_ids.is_empty() {
        return;
    }
    if create_strict_lite_ai_analyzer_with_timeout(Some(CLASSIFY_TIMEOUT))
        .await
        .is_none()
    {
        tracing::debug!("subscription topic Lite unavailable; skip ingest classify");
        return;
    }
    for item_id in item_ids.iter().copied().take(MAX_INGEST_SUGGESTS) {
        recommend_one(db, item_id).await;
    }
}

/// Agent 订阅没有 RETURNING id，按最近未打标的抓。
pub async fn recommend_unlabeled_for_source(
    db: &DatabaseConnection,
    source_id: i32,
    inserted_count: usize,
) {
    let limit = inserted_count.min(MAX_INGEST_SUGGESTS) as u64;
    if limit == 0 {
        return;
    }
    let ids = match phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.eq(source_id))
        .filter(phantasi_items::Column::Topic.is_null())
        .order_by_desc(phantasi_items::Column::FetchedAt)
        .limit(limit)
        .select_only()
        .column(phantasi_items::Column::Id)
        .into_tuple::<i32>()
        .all(db)
        .await
    {
        Ok(items) => items,
        Err(error) => {
            tracing::warn!(%error, source_id, "list unlabeled items for topic suggest failed");
            return;
        }
    };
    recommend_topics_for_item_ids(db, &ids).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manual_topic_write_only_needs_metadata_and_rejects_notes() {
        use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement};
        let Ok(url) = std::env::var("PHANTASI_TEST_DATABASE_URL") else {
            eprintln!("skipped PostgreSQL topic test: PHANTASI_TEST_DATABASE_URL is unset");
            return;
        };
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        // Intentionally no body columns: classification commands must not depend
        // on loading or returning the article model.
        db.execute_unprepared(
            "CREATE TEMP TABLE phantasi_sources (id integer PRIMARY KEY, source_type text NOT NULL);
             CREATE TEMP TABLE phantasi_items (id integer PRIMARY KEY, source_id integer, topic text);
             INSERT INTO phantasi_sources VALUES (1, 'rss'), (2, 'note');
             INSERT INTO phantasi_items VALUES (10, 1, NULL), (20, 2, 'Keep')",
        )
        .await
        .unwrap();
        assert_eq!(
            set_subscription_item_topic(&db, 10, Some(" Rust "))
                .await
                .unwrap(),
            Some("Rust".into())
        );
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT topic FROM phantasi_items WHERE id = 10",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "topic").unwrap(), "Rust");
        assert_eq!(
            set_subscription_item_topic(&db, 10, None).await.unwrap(),
            None
        );
        assert!(matches!(
            set_subscription_item_topic(&db, 20, Some("Forbidden")).await,
            Err(TopicWriteError::NoteItem)
        ));
        assert!(matches!(
            set_subscription_item_topic(&db, 999, Some("Missing")).await,
            Err(TopicWriteError::NotFound)
        ));
    }

    #[test]
    fn normalize_trims_and_caps() {
        assert_eq!(normalize_topic_name("  "), None);
        assert_eq!(normalize_topic_name(" Rust "), Some("Rust".into()));
        let long = "字".repeat(TOPIC_NAME_MAX_CHARS + 8);
        assert_eq!(
            normalize_topic_name(&long).unwrap().chars().count(),
            TOPIC_NAME_MAX_CHARS
        );
    }

    #[test]
    fn ai_rejects_bucket_names() {
        assert!(is_rejected_ai_topic("其他"));
        assert!(is_rejected_ai_topic("Other"));
        assert!(is_rejected_ai_topic("MISC"));
        assert!(!is_rejected_ai_topic("Rust"));
        assert!(!is_rejected_ai_topic("工程实践"));
    }

    #[test]
    fn parse_object_and_null() {
        assert_eq!(
            parse_suggested_topic(r#"{"topic":"Rust"}"#).unwrap(),
            Some("Rust".into())
        );
        assert_eq!(parse_suggested_topic(r#"{"topic":null}"#).unwrap(), None);
        assert_eq!(
            parse_suggested_topic("here you go\n{\"topic\":\"安全\"}\n").unwrap(),
            Some("安全".into())
        );
        assert_eq!(parse_suggested_topic(r#"{"topic":"其他"}"#).unwrap(), None);
        assert_eq!(parse_suggested_topic(r#"{"topic":"  "}"#).unwrap(), None);
        assert!(parse_suggested_topic("not json").is_err());
    }

    #[test]
    fn prompt_reuses_existing_and_forbids_buckets() {
        let prompt = build_topic_suggest_prompt(
            "Tokio 0.3",
            "async runtime notes",
            &["Rust".into(), "系统".into()],
        );
        assert!(prompt.contains("- Rust"));
        assert!(prompt.contains("Tokio 0.3"));
        assert!(prompt.contains("pick one of these"));
        assert!(prompt.contains("其他"));
        assert!(prompt.contains(r#"{"topic":null}"#));
    }

    /// 归类必须是严格 Lite：短 JSON、限输出，不能借 Standard 的模型和延迟。
    #[test]
    fn classify_is_strict_lite_with_a_small_payload() {
        let source = include_str!("phantasi_topics.rs");
        let body = source
            .split("async fn suggest_topic_name(")
            .nth(1)
            .and_then(|rest| rest.split("\nasync fn ").next())
            .expect("suggest_topic_name body");

        assert!(body.contains("create_strict_lite_ai_analyzer_with_timeout"));
        assert!(body.contains("analyze_json_short"));
        assert!(body.contains("CLASSIFY_OUTPUT_BUDGET"));
        assert!(!body.contains("create_ai_analyzer_for_tier"));
        assert!(!body.contains("ModelTier::Standard"));
        assert!(!body.contains("ModelTier::Lite"));
        assert!(!body.contains("lite_enabled"));
    }

    #[test]
    fn public_topic_names_exclude_admin_only_sources() {
        let src = include_str!("phantasi_topics.rs");
        let start = src
            .find("async fn list_existing_subscription_topics")
            .expect("list_existing_subscription_topics");
        let body = &src[start..];
        assert!(body.contains("include_admin_only"));
        assert!(body.contains("AdminOnly"));
    }

    #[test]
    fn excerpt_strips_html_and_caps() {
        let out = excerpt_for_topic(Some("<p>聊聊 <b>embedding</b></p>"), None);
        assert_eq!(out, "聊聊 embedding");
        let far = format!("{}kernel", "啊".repeat(EXCERPT_CHARS));
        assert!(!excerpt_for_topic(Some(&far), None).contains("kernel"));
    }
}
