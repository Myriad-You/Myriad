//! After a Chat reply, optionally distill one persona-memory fact.
//!
//! Fail-open: missing Lite, timeout, empty JSON, or a duplicate fact never
//! block the spoken reply. Delivery motion is spawned first so extract does
//! not steal the first Lite slot.

use std::time::Duration;

use serde_json::json;

use super::ingest::{compact_summary, persist_persona_remember, persona_remember_insert};
use super::is_logged_in_addressee;
use super::store::list_remembered;

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(4);
const EXTRACT_TOTAL_TIMEOUT: Duration = Duration::from_secs(5);
const EXTRACT_SCHEMA_NAME: &str = "merope_chat_remember";
const MIN_USER_CHARS: usize = 8;

pub fn should_extract_chat_remember(user_text: &str) -> bool {
    should_extract_chat_remember_against(user_text, &[])
}

pub fn should_extract_chat_remember_against(user_text: &str, existing: &[String]) -> bool {
    let compact = compact_summary(user_text);
    if compact.chars().count() < MIN_USER_CHARS {
        return false;
    }
    persona_remember_insert(&compact, existing).is_some()
}

pub fn parse_chat_remember_fact(raw: &str) -> Option<String> {
    let stripped = strip_json_fence(raw);
    let value: serde_json::Value = serde_json::from_str(stripped).ok()?;
    let fact = value.get("fact")?;
    if fact.is_null() {
        return None;
    }
    let text = fact.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

pub fn spawn_chat_remember(user_id: i32, user_text: String, reply: String) {
    if !should_extract_chat_remember(&user_text) {
        return;
    }
    tokio::spawn(async move {
        extract_and_store(user_id, &user_text, &reply).await;
    });
}

fn extract_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "fact": { "type": ["string", "null"], "maxLength": 240 }
        },
        "required": ["fact"],
        "additionalProperties": false
    })
}

fn extract_system_prompt(existing: &[String]) -> String {
    let known = if existing.is_empty() {
        "（还没有留下的事实）".to_string()
    } else {
        existing
            .iter()
            .take(8)
            .map(|fact| format!("- {fact}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "你在为人设整理对这个说话对象的记忆。只抽出 0 或 1 条关于这个人的短事实：偏好、习惯、关系、约定。\
不是回复，不是心情数字，不是办事教训或工具参数，也不是你自己正在做什么。\
已有事实不要重复。闲聊或没有新信息则 fact 为 null。\
已有：\n{known}"
    )
}

async fn extract_and_store(user_id: i32, user_text: &str, reply: &str) {
    if !is_logged_in_addressee(user_id) || !super::is_enabled().await {
        return;
    }
    let Ok(db) = crate::services::tapp_registry::database().await else {
        return;
    };
    let existing = match list_remembered(&db, user_id, 32).await {
        Ok(notes) => notes
            .into_iter()
            .map(|note| compact_summary(&note.content))
            .filter(|content| !content.is_empty())
            .collect::<Vec<_>>(),
        Err(_) => return,
    };
    if !should_extract_chat_remember_against(user_text, &existing) {
        return;
    }
    let Some(analyzer) =
        crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(EXTRACT_TIMEOUT))
            .await
    else {
        return;
    };
    let input = json!({
        "userText": compact_summary(user_text),
        "reply": compact_summary(reply),
    })
    .to_string();
    let schema = extract_schema();
    let system_prompt = extract_system_prompt(&existing);
    let call = analyzer.analyze_json(&system_prompt, &input, EXTRACT_SCHEMA_NAME, Some(&schema));
    let raw = match tokio::time::timeout(
        EXTRACT_TOTAL_TIMEOUT,
        crate::services::ai_cost_ledger::with_site_ai_ledger(
            user_id,
            "merope",
            "chat_remember",
            call,
        ),
    )
    .await
    {
        Ok(Ok(raw)) => raw,
        _ => return,
    };
    let Some(fact) = parse_chat_remember_fact(&raw) else {
        return;
    };
    if persona_remember_insert(&fact, &existing).is_none() {
        return;
    }
    persist_persona_remember(&db, user_id, Some(&fact)).await;
}

fn strip_json_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|inner| inner.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_short_user_text() {
        assert!(!should_extract_chat_remember("你好"));
        assert!(!should_extract_chat_remember("好"));
        assert!(should_extract_chat_remember("晚上想打独立游戏"));
        assert!(!should_extract_chat_remember_against(
            "晚上想打独立游戏",
            &["晚上想打独立游戏".into()]
        ));
        assert!(should_extract_chat_remember_against(
            "早上只喝美式咖啡",
            &["晚上想打独立游戏".into()]
        ));
    }

    #[test]
    fn parse_accepts_null_or_one_fact() {
        assert_eq!(parse_chat_remember_fact(r#"{"fact":null}"#), None);
        assert_eq!(parse_chat_remember_fact("{}"), None);
        assert_eq!(
            parse_chat_remember_fact(r#"{"fact":"晚上想打独立游戏"}"#).as_deref(),
            Some("晚上想打独立游戏")
        );
        assert_eq!(
            parse_chat_remember_fact("```json\n{\"fact\":\"早上喝美式\"}\n```").as_deref(),
            Some("早上喝美式")
        );
    }

    #[test]
    fn extract_prompt_is_not_a_reply_and_skips_work_lessons() {
        let prompt = extract_system_prompt(&["晚上想打独立游戏".into()]);
        assert!(prompt.contains("短事实"));
        assert!(prompt.contains("不是回复"));
        assert!(prompt.contains("办事教训"));
        assert!(prompt.contains("你自己正在做什么"));
        assert!(prompt.contains("晚上想打独立游戏"));
        assert!(prompt.contains("fact 为 null"));
    }

    #[test]
    fn chat_does_not_wait_on_extract_and_starts_motion_first() {
        let src = include_str!("../process_and_recipe.rs");
        let chat = src
            .find("stream_strict_lite_chat_response")
            .expect("streaming chat");
        let after = &src[chat..];
        let delivery = after
            .find("MotionPhase::Delivery")
            .expect("chat delivery motion");
        let extract = after
            .find("spawn_chat_remember")
            .expect("chat remember extract");
        let ret = after.find("return Ok(AgentResponse").expect("chat return");
        assert!(
            delivery < extract,
            "delivery motion must start before chat remember extract"
        );
        assert!(extract < ret, "extract must not delay the Chat response");
        assert!(!src.contains("extract_and_store("));
    }
}
