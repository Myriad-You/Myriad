//! After a Chat reply, optionally distill one persona-memory fact.
//!
//! Fail-open: missing Lite, timeout, empty JSON, or a duplicate fact never
//! block the spoken reply. Delivery motion is spawned first so extract does
//! not steal the first Lite slot.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::ingest::{compact_summary, persona_remember_insert};
use super::is_logged_in_addressee;
use myriad_merope::chat_remember::{
    EXTRACT_SCHEMA_NAME, MIN_USER_CHARS, extract_schema, extract_system_prompt, strip_json_fence,
};

pub fn should_extract_chat_remember(user_text: &str) -> bool {
    should_extract_chat_remember_against(user_text, &[])
}

pub fn should_extract_chat_remember_against(user_text: &str, existing: &[String]) -> bool {
    let Some(compact) = memory_user_text(user_text) else {
        return false;
    };
    if compact.chars().filter(|ch| ch.is_alphanumeric()).count() < MIN_USER_CHARS {
        return false;
    }
    // Do not deduplicate a long utterance by its first 240 characters: a
    // correction can occur at the end, after repeating the previous fact.
    compact.chars().count() > 240 || persona_remember_insert(&compact, existing).is_some()
}

// Nobody waits on this: generous enough to ride out a stalled provider.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(20);

fn memory_user_text(text: &str) -> Option<String> {
    let text = super::ingest::redact_event_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // Refuse an oversized assertion rather than turning a truncated prefix into
    // a fact. A missing final negation must never authorize a memory rewrite.
    (!text.is_empty() && text.chars().count() <= 2_000).then_some(text)
}

/// One bounded interpretation of the user's own assertion. Targets are exact
/// recalled facts, never free-form selectors or model-generated database ids.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChatMemoryUpdate {
    pub fact: Option<String>,
    pub supersedes: Vec<String>,
    pub evidence: Option<String>,
    /// What `fact` is about, for recall to find it by other names.
    pub concepts: Vec<crate::services::agent::memory::unified::Concept>,
}

/// Something she told them that she would remember having said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub said: String,
    pub evidence: String,
}

/// What one message leaves her: single updates applied in order (a
/// withdrawal first, then each new fact), and what she said worth keeping.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatMemoryUpdates {
    pub updates: Vec<ChatMemoryUpdate>,
    pub said: Vec<Said>,
    /// Public things they suggested she try herself, each about the thing
    /// with no one named: hers to wonder about (see `merope::heard`).
    pub put_onto: Vec<String>,
}

#[cfg(test)]
impl ChatMemoryUpdates {
    /// Seen as one update, for checks written against one: its first new
    /// fact, and everything it withdraws.
    pub fn combined(&self) -> ChatMemoryUpdate {
        ChatMemoryUpdate {
            fact: self.updates.iter().find_map(|update| update.fact.clone()),
            supersedes: self
                .updates
                .iter()
                .flat_map(|update| update.supersedes.iter().cloned())
                .collect(),
            evidence: None,
            concepts: Vec::new(),
        }
    }
}

/// The extraction's answer (see `extract_schema`), each part held to the
/// same checks as a single update; `reply` is what her said-lines must
/// quote. None when the answer is malformed as a whole.
pub fn parse_chat_memory_updates(
    raw: &str,
    user_text: &str,
    reply: &str,
    existing: &[String],
) -> Option<ChatMemoryUpdates> {
    let value: serde_json::Value = serde_json::from_str(strip_json_fence(raw)).ok()?;
    let facts = value.get("facts")?.as_array()?;
    let supersedes = value.get("supersedes")?.as_array()?;
    let said = value.get("said")?.as_array()?;
    let mut out = ChatMemoryUpdates::default();
    // A withdrawal or correction of what she knew comes first, on its own.
    if !supersedes.is_empty() {
        let single = serde_json::json!({
            "fact": null,
            "supersedes": supersedes,
            "evidence": value.get("supersedesEvidence").cloned().unwrap_or_default(),
            "concepts": [],
        });
        out.updates.push(parse_chat_memory_update(
            &single.to_string(),
            user_text,
            existing,
        )?);
    }
    for fact in facts.iter().take(myriad_merope::chat_remember::MAX_FACTS) {
        let single = serde_json::json!({
            "fact": fact.get("fact"),
            "supersedes": [],
            "evidence": fact.get("evidence"),
            "concepts": fact.get("concepts").cloned().unwrap_or_else(|| serde_json::json!([])),
        });
        // One fact that does not hold up does not take the others with it.
        if let Some(update) = parse_chat_memory_update(&single.to_string(), user_text, existing)
            && update.fact.is_some()
            && !out.updates.iter().any(|kept| kept.fact == update.fact)
        {
            out.updates.push(update);
        }
    }
    for item in said.iter().take(myriad_merope::chat_remember::MAX_SAID) {
        let (Some(line), Some(evidence)) = (
            item.get("said").and_then(serde_json::Value::as_str),
            item.get("evidence").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let line = compact_summary(line);
        let evidence = evidence.trim();
        if line.is_empty()
            || line.chars().count() > 200
            || evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2
            || !reply.contains(evidence)
        {
            continue;
        }
        out.said.push(Said {
            said: line,
            evidence: evidence.to_string(),
        });
    }
    // Older answers have none; one that does not quote them is dropped.
    let put_onto = value
        .get("putOnto")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in put_onto
        .iter()
        .take(myriad_merope::chat_remember::MAX_PUT_ONTO)
    {
        let (Some(thing), Some(evidence)) = (
            item.get("thing").and_then(serde_json::Value::as_str),
            item.get("evidence").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let thing = compact_summary(thing);
        let evidence = evidence.trim();
        if thing.is_empty()
            || thing.chars().count() > 160
            || evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2
            || !user_text.contains(evidence)
            || out.put_onto.contains(&thing)
        {
            continue;
        }
        out.put_onto.push(thing);
    }
    Some(out)
}

pub fn parse_chat_memory_update(
    raw: &str,
    user_text: &str,
    existing: &[String],
) -> Option<ChatMemoryUpdate> {
    let stripped = strip_json_fence(raw);
    let value: serde_json::Value = serde_json::from_str(stripped).ok()?;
    // Nullable fields are still required; missing is not an old-format fallback.
    if !["fact", "supersedes", "evidence", "concepts"]
        .iter()
        .all(|key| value.get(key).is_some())
    {
        return None;
    }
    let mut update: ChatMemoryUpdate = serde_json::from_value(value).ok()?;
    update.concepts = if update.fact.is_some() {
        crate::services::agent::memory::unified::clean_concepts(std::mem::take(
            &mut update.concepts,
        ))
    } else {
        Vec::new()
    };
    if let Some(fact) = &update.fact {
        if fact.chars().count() > 240 || compact_summary(fact).is_empty() {
            return None;
        }
        update.fact = Some(compact_summary(fact));
    }
    if update.supersedes.len() > 8
        || update
            .supersedes
            .iter()
            .any(|old| !existing.contains(old) || update.fact.as_ref() == Some(old))
    {
        return None;
    }
    let mut unique = update.supersedes.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != update.supersedes.len() {
        return None;
    }
    if update.fact.is_some() || !update.supersedes.is_empty() {
        let evidence = update.evidence.as_deref()?.trim();
        if evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2
            || evidence.chars().count() > 240
            || !user_text.contains(evidence)
        {
            return None;
        }
    } else if update.evidence.is_some() {
        return None;
    }
    Some(update)
}

pub async fn enqueue_chat_remember(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    user_text: String,
    reply: String,
    input_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    present: crate::services::agent::memory::unified::Audience,
    turn: super::TurnContext,
) {
    let Some(input_at) = input_at else {
        return;
    };
    let Some(user_text) = memory_user_text(&user_text) else {
        return;
    };
    if !is_logged_in_addressee(user_id) || !should_extract_chat_remember(&user_text) {
        return;
    }
    let id = super::memory_jobs::key(&[
        "chat",
        &user_id.to_string(),
        &input_at.to_rfc3339(),
        &present.venue(),
    ]);
    let data = super::memory_jobs::Payload::Chat {
        user_text,
        reply: compact_summary(&reply),
        input_at,
        present,
        turn,
    };
    if !matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            super::store::memory_jobs::enqueue(db, &id, user_id, data, None)
        )
        .await,
        Ok(Ok(()))
    ) {
        tracing::warn!(user_id, outcome = "enqueue_failed", "[Merope] chat memory");
    }
}

/// What the extraction reads: their words, her reply, and what surrounded
/// them. Only what they said in `userText` may become a fact.
pub(super) fn extract_input(
    user_text: &str,
    reply: &str,
    turn: &super::TurnContext,
    now: chrono::DateTime<chrono::Local>,
) -> String {
    let mut input = json!({
        "userText": user_text,
        "reply": compact_summary(reply),
        "today": now.format("%Y-%m-%d (%A)").to_string(),
    });
    if let Some(before) = &turn.before {
        input["before"] = json!(before);
    }
    if let Some(scene) = &turn.scene {
        input["scene"] = json!(scene);
    }
    if turn.in_game {
        input["inGame"] = json!(true);
    }
    input.to_string()
}

pub(super) async fn extract(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    user_text: &str,
    reply: &str,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    present: &crate::services::agent::memory::unified::Audience,
    turn: &super::TurnContext,
) -> Result<super::memory_jobs::Effect, super::memory_jobs::Failure> {
    use super::memory_jobs::{Effect, Failure};
    let (existing, _) = super::store::recall_remembered_primed(
        db,
        user_id,
        present,
        Some(user_text),
        8,
        &crate::services::agent::memory::unified::Priming::default(),
        1.0,
    )
    .await
    .map_err(|_| Failure::Storage)?;
    if !should_extract_chat_remember_against(user_text, &existing) {
        return Ok(Effect::NoChange);
    }
    // Recovery retains the date of the original assertion.
    let input = extract_input(
        user_text,
        reply,
        turn,
        input_at.with_timezone(&chrono::Local),
    );
    let raw = super::call::Ask::new(super::call::Voice::Judge, user_id, "chat_remember")
        .within(EXTRACT_TIMEOUT)
        .json_raw(
            &extract_system_prompt(&existing),
            &input,
            EXTRACT_SCHEMA_NAME,
            &extract_schema(),
        )
        .await?;
    let updates = parse_chat_memory_updates(&raw, user_text, reply, &existing)
        .ok_or(super::call::Failure::InvalidOutput)?;
    if updates.updates.is_empty() && updates.said.is_empty() && updates.put_onto.is_empty() {
        return Ok(Effect::NoChange);
    }
    Ok(Effect::Chat(updates))
}

#[cfg(test)]
pub(crate) fn live_probe_contract(existing: &[String]) -> (String, serde_json::Value) {
    (extract_system_prompt(existing), extract_schema())
}

/// The extraction input as production builds it, on a fixed date.
#[cfg(test)]
pub(crate) fn probe_input(user_text: &str, reply: &str, turn: &super::TurnContext) -> String {
    use chrono::TimeZone;
    let now = chrono::Local
        .with_ymd_and_hms(2026, 9, 25, 21, 0, 0)
        .single()
        .expect("fixed date");
    extract_input(user_text, reply, turn, now)
}

#[cfg(test)]
mod tests {
    #[test]
    fn what_they_put_her_onto_is_kept_only_when_they_said_it() {
        let user = "你可以去玩玩《Outer Wilds》，千万别看攻略";
        let raw = serde_json::json!({
            "facts": [], "supersedes": [], "supersedesEvidence": null, "said": [],
            "putOnto": [
                {"thing": "有人推荐《Outer Wilds》，说千万别看攻略", "evidence": "你可以去玩玩《Outer Wilds》"},
                {"thing": "有人推荐《星露谷》", "evidence": "星露谷也不错"}
            ]
        })
        .to_string();
        let updates = super::parse_chat_memory_updates(&raw, user, "好", &[]).unwrap();
        assert_eq!(
            updates.put_onto,
            vec!["有人推荐《Outer Wilds》，说千万别看攻略".to_string()]
        );
        // Answers from before there was putOnto still parse.
        let older = r#"{"facts":[],"supersedes":[],"supersedesEvidence":null,"said":[]}"#;
        assert!(
            super::parse_chat_memory_updates(older, user, "好", &[])
                .unwrap()
                .put_onto
                .is_empty()
        );
        let schema = myriad_merope::chat_remember::extract_schema();
        assert_eq!(
            schema["properties"]["putOnto"]["maxItems"],
            myriad_merope::chat_remember::MAX_PUT_ONTO
        );
        assert!(
            myriad_merope::chat_remember::extract_system_prompt(&[])
                .contains("with no names of people")
        );
    }

    #[test]
    fn one_message_can_leave_several_facts_and_what_she_said() {
        let user = "上周五把自行车修好了，下周二车要去保养。对了我换工作了";
        let reply = "修好就好！我推荐你试试「夜航」这首歌，下班路上听。";
        let raw = serde_json::json!({
            "facts": [
                {"fact": "2026-09-19 把自行车修好了", "evidence": "上周五把自行车修好了", "concepts": []},
                {"fact": "2026-09-30 车要去保养", "evidence": "下周二车要去保养", "concepts": []},
                // Not what they said: dropped, the others stay.
                {"fact": "他很喜欢骑车", "evidence": "我喜欢骑车", "concepts": []},
                {"fact": "换了工作", "evidence": "我换工作了", "concepts": []}
            ],
            "supersedes": ["在银行上班"],
            "supersedesEvidence": "我换工作了",
            "said": [
                {"said": "我推荐了「夜航」这首歌", "evidence": "我推荐你试试「夜航」这首歌"},
                {"said": "我说过要陪他跑步", "evidence": "陪你跑步"}
            ]
        })
        .to_string();
        let updates = parse_chat_memory_updates(&raw, user, reply, &["在银行上班".into()]).unwrap();
        // The withdrawal first, then each fact that holds up.
        assert_eq!(
            updates.updates[0].supersedes,
            vec!["在银行上班".to_string()]
        );
        assert_eq!(updates.updates[0].fact, None);
        let facts: Vec<&str> = updates.updates[1..]
            .iter()
            .filter_map(|update| update.fact.as_deref())
            .collect();
        assert_eq!(
            facts,
            [
                "2026-09-19 把自行车修好了",
                "2026-09-30 车要去保养",
                "换了工作"
            ]
        );
        // What she said is kept only when her reply says it.
        assert_eq!(updates.said.len(), 1);
        assert_eq!(updates.said[0].said, "我推荐了「夜航」这首歌");
        assert_eq!(
            updates.combined().supersedes,
            vec!["在银行上班".to_string()]
        );
        assert_eq!(
            updates.combined().fact.as_deref(),
            Some("2026-09-19 把自行车修好了")
        );
        // Nothing to keep is an empty answer, not an invalid one.
        let nothing = r#"{"facts":[],"supersedes":[],"supersedesEvidence":null,"said":[]}"#;
        assert_eq!(
            parse_chat_memory_updates(nothing, user, reply, &[]),
            Some(ChatMemoryUpdates::default())
        );
        assert_eq!(
            parse_chat_memory_updates(r#"{"fact":null}"#, user, reply, &[]),
            None
        );
        // A withdrawal of something she never knew spoils the whole answer.
        let unknown =
            r#"{"facts":[],"supersedes":["从没记过"],"supersedesEvidence":"我换工作了","said":[]}"#;
        assert_eq!(parse_chat_memory_updates(unknown, user, reply, &[]), None);
    }

    use super::*;

    #[test]
    fn short_facts_are_evaluated_instead_of_silently_dropped() {
        assert!(should_extract_chat_remember("你好")); // Lite can choose null.
        assert!(!should_extract_chat_remember("好"));
        assert!(!should_extract_chat_remember("……！！"));
        assert!(!should_extract_chat_remember("   "));
        assert!(should_extract_chat_remember("我不喝咖啡了"));
        assert!(should_extract_chat_remember("我吃素"));
        assert!(should_extract_chat_remember("猫が好き"));
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
    fn the_extraction_sees_what_surrounded_their_words() {
        let turn = crate::services::agent::merope::TurnContext {
            before: Some("你最喜欢什么动物？".into()),
            scene: Some("They are listening to 晴天.".into()),
            in_game: true,
            images: 0,
        };
        let input: serde_json::Value =
            serde_json::from_str(&probe_input("橘猫吧", "好品味", &turn)).unwrap();
        assert_eq!(input["before"], "你最喜欢什么动物？");
        assert_eq!(input["inGame"], true);
        assert_eq!(input["today"], "2026-09-25 (Friday)");
        assert!(input["scene"].as_str().unwrap().contains("晴天"));
        let quiet: serde_json::Value = serde_json::from_str(&probe_input(
            "橘猫吧",
            "好品味",
            &crate::services::agent::merope::TurnContext::default(),
        ))
        .unwrap();
        assert!(quiet.get("before").is_none() && quiet.get("inGame").is_none());
        let prompt = extract_system_prompt(&[]);
        assert!(prompt.contains("If inGame is true"));
        assert!(prompt.contains("still taking the fact from userText"));
    }

    #[test]
    fn corrections_at_the_end_of_a_long_utterance_are_not_truncated_or_deduplicated() {
        let prefix = "以前喜欢咖啡".repeat(45);
        let text = format!("{prefix}。但我现在不喝咖啡了。");
        assert!(should_extract_chat_remember_against(
            &text,
            &[compact_summary(&prefix)]
        ));
        let input = memory_user_text(&text).unwrap();
        assert!(input.ends_with("但我现在不喝咖啡了。"));
        let raw = r#"{"fact":"现在不喝咖啡","supersedes":["喜欢咖啡"],"evidence":"但我现在不喝咖啡了","concepts":[]}"#;
        assert!(parse_chat_memory_update(raw, &input, &["喜欢咖啡".into()]).is_some());
        assert!(memory_user_text(&"茶".repeat(2_001)).is_none());
        assert!(!should_extract_chat_remember(&"茶".repeat(2_001)));
    }

    #[test]
    fn parse_accepts_bounded_add_replace_retract_or_no_change() {
        let existing = vec!["喜欢咖啡".into()];
        for (raw, input, fact, targets) in [
            (
                r#"{"fact":null,"supersedes":[],"evidence":null,"concepts":[]}"#,
                "你好",
                None,
                vec![],
            ),
            (
                r#"{"fact":"也喜欢茶","supersedes":[],"evidence":"我也喜欢茶","concepts":[]}"#,
                "我也喜欢茶",
                Some("也喜欢茶"),
                vec![],
            ),
            (
                r#"{"fact":"现在不喝咖啡","supersedes":["喜欢咖啡"],"evidence":"我不喝咖啡了","concepts":[]}"#,
                "我不喝咖啡了",
                Some("现在不喝咖啡"),
                vec!["喜欢咖啡"],
            ),
            (
                r#"{"fact":null,"supersedes":["喜欢咖啡"],"evidence":"咖啡偏好记错了，请撤回","concepts":[]}"#,
                "咖啡偏好记错了，请撤回",
                None,
                vec!["喜欢咖啡"],
            ),
        ] {
            let update = parse_chat_memory_update(raw, input, &existing).unwrap();
            assert_eq!(update.fact.as_deref(), fact);
            assert_eq!(update.supersedes, targets);
            assert_eq!(
                parse_chat_memory_update(&format!("```json\n{raw}\n```"), input, &existing),
                Some(update)
            );
        }
    }

    #[test]
    fn rejects_unknown_targets_missing_evidence_and_old_or_malformed_contracts() {
        let existing = vec!["喜欢咖啡".into()];
        for raw in [
            r#"{"fact":null}"#,
            r#"{"fact":null,"supersedes":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":[],"evidence":"我不喝咖啡了"}"#,
            r#"{"fact":null,"supersedes":[],"evidence":"我不喝咖啡了","concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":["其他人的事实"],"evidence":"我不喝咖啡了","concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":["喜欢咖啡"],"evidence":null,"concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":["喜欢咖啡"],"evidence":"模型猜测","concepts":[]}"#,
            r#"{"fact":"喜欢咖啡","supersedes":["喜欢咖啡"],"evidence":"我不喝咖啡了","concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":["喜欢咖啡","喜欢咖啡"],"evidence":"我不喝咖啡了","concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":[],"evidence":"我不喝咖啡了","concepts":[],"action":"delete_all"}"#,
        ] {
            assert!(
                parse_chat_memory_update(raw, "我不喝咖啡了", &existing).is_none(),
                "{raw}"
            );
        }
        let long = json!({"fact":"茶".repeat(241),"supersedes":[],"evidence":"我不喝咖啡了","concepts":[]});
        assert!(parse_chat_memory_update(&long.to_string(), "我不喝咖啡了", &existing).is_none());
    }

    #[test]
    fn concepts_are_cleaned_and_dropped_without_a_new_fact() {
        let raw = r#"{"fact":"养了一只猫叫年糕","supersedes":[],"evidence":"我养了一只猫叫年糕","concepts":[{"name":" 猫 ","aliases":["喵","猫","x","猫咪"]},{"name":"猫","aliases":[]},{"name":"年糕","aliases":[]}]}"#;
        let update = parse_chat_memory_update(raw, "我养了一只猫叫年糕", &[]).unwrap();
        let names: Vec<(&str, Vec<&str>)> = update
            .concepts
            .iter()
            .map(|concept| {
                (
                    concept.name.as_str(),
                    concept.aliases.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(names, vec![("猫", vec!["喵", "猫咪"]), ("年糕", vec![])]);
        let retract = r#"{"fact":null,"supersedes":["喜欢咖啡"],"evidence":"咖啡偏好记错了","concepts":[{"name":"咖啡","aliases":[]}]}"#;
        let update =
            parse_chat_memory_update(retract, "咖啡偏好记错了", &["喜欢咖啡".into()]).unwrap();
        assert!(update.concepts.is_empty());
        let unknown = r#"{"fact":"养猫","supersedes":[],"evidence":"我养猫","concepts":[{"name":"猫","kind":"pet"}]}"#;
        assert!(parse_chat_memory_update(unknown, "我养猫", &[]).is_none());
    }

    #[test]
    fn chat_does_not_wait_on_extract_and_starts_motion_first() {
        let src = include_str!("../process_chat.rs");
        let chat = src
            .find("stream_strict_lite_chat_response")
            .expect("streaming chat");
        let after = &src[chat..];
        let reaction = src
            .find("local_directive(&reaction_context)")
            .expect("immediate reaction");
        let director = src
            .find("spawn_chat_motion_refinement(")
            .expect("parallel delivery observer");
        let extract = after
            .find("enqueue_chat_remember")
            .expect("chat remember extract");
        let ret = after.find("return Ok(AgentResponse").expect("chat return");
        assert!(
            reaction < director && director < chat,
            "immediate reaction and delivery observer must start before Chat, not after memory extraction"
        );
        assert!(after.find("response_agent::finish_stream").unwrap() < extract);
        assert!(extract < ret, "extract must not delay the Chat response");
        assert!(!src.contains("extract_and_store("));
    }
}
