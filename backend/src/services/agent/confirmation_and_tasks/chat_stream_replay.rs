//! Her real private talk, answered again by the build as it is now, and the
//! shapes that made her read as a performance rather than someone (see the
//! 2026-10-01 review) counted for both what she said then and what she says
//! now: replies that end by telling them what to do, exclamation marks,
//! length, words from her own notes, and openings that repeat. Counted in
//! code, so no reviewer is needed and a prompt change can be compared on
//! the same real inputs.
//!
//! Read only (the site's database), and it asks the chat model once per
//! turn. What she knows now (her day, her memory, the hour) is today's, not
//! what it was when the turn happened: it compares how she answers the same
//! words, not a rerun of the past.

use super::*;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{Value, json};

/// Words that, at the end of a reply, tell them what to do or hurry them.
const URGES: [&str; 16] = [
    "赶紧",
    "快说",
    "快去",
    "快听",
    "快来",
    "快点",
    "快快",
    "快发",
    "记得",
    "小心",
    "别忘",
    "早点睡",
    "多喝",
    "歇会",
    "告诉我",
    "跟我说说",
];
/// Words her own notes lean on, which read as jargon in talk.
const NOTE_WORDS: [&str; 4] = ["撞", "劲", "笼子", "不是这个"];

fn said_lines(reply: &str) -> Vec<&str> {
    reply
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("[["))
        .collect()
}

#[derive(Default)]
struct Shape {
    replies: usize,
    urging: usize,
    bangs: usize,
    lines: usize,
    chars: Vec<usize>,
    note_words: usize,
}

impl Shape {
    fn add(&mut self, reply: &str) {
        let lines = said_lines(reply);
        if lines.is_empty() {
            return;
        }
        self.replies += 1;
        if lines
            .last()
            .is_some_and(|last| URGES.iter().any(|urge| last.contains(urge)))
        {
            self.urging += 1;
        }
        self.bangs += reply.matches(['！', '!']).count();
        self.lines += lines.len();
        self.chars
            .push(lines.iter().map(|line| line.chars().count()).sum());
        if NOTE_WORDS.iter().any(|word| reply.contains(word)) {
            self.note_words += 1;
        }
    }

    fn line(&self) -> String {
        let n = self.replies.max(1) as f64;
        let mut chars = self.chars.clone();
        chars.sort_unstable();
        let median = chars.get(chars.len() / 2).copied().unwrap_or(0);
        format!(
            "replies {}  urging-end {:.0}%  bangs/reply {:.1}  lines/reply {:.1}  chars median {}  note-words {:.0}%",
            self.replies,
            self.urging as f64 / n * 100.0,
            self.bangs as f64 / n,
            self.lines as f64 / n,
            median,
            self.note_words as f64 / n * 100.0,
        )
    }
}

async fn history_before(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    before: chrono::DateTime<chrono::FixedOffset>,
) -> Vec<crate::services::agent::ConversationMessage> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.role, m.content, m.metadata, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
               AND m.created_at < $2 ORDER BY m.created_at DESC LIMIT 20",
            [user_id.into(), before.into()],
        ))
        .await
        .unwrap();
    rows.iter()
        .rev()
        .map(|row| {
            let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "created_at").unwrap();
            crate::services::agent::chat_prompt::reconstruct_conversation_message(
                row.try_get("", "role").unwrap(),
                row.try_get("", "content").unwrap(),
                Some(at.to_rfc3339()),
                row.try_get::<Option<Value>>("", "metadata")
                    .unwrap()
                    .as_ref(),
                true,
            )
        })
        .collect()
}

/// The headed sections of her prompt whose own content her reply drew on,
/// as the judgment model reads it: a fact, memory, state or thing of hers
/// that shows in what she said. How-to-talk rules do not count, nor what the
/// conversation or their words already hold.
async fn drew_on(prompt: &str, said: &str, reply: &str) -> Vec<String> {
    let head = prompt
        .find("\nConversation:\n")
        .or_else(|| prompt.find("\nThey said:"))
        .map_or(prompt, |end| &prompt[..end]);
    let tail = prompt
        .find("\nConversation:\n")
        .map(|start| &prompt[start..])
        .and_then(|rest| rest.find("\nLatest message:").map(|end| &rest[..end]))
        .unwrap_or_default();
    let conversation: String = tail
        .lines()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let sections: Vec<(String, String)> = head
        .split("\n## ")
        .skip(1)
        .map(|part| {
            let heading = part.lines().next().unwrap_or_default().trim().to_string();
            (heading, part.chars().take(1500).collect())
        })
        .collect();
    let input = json!({
        "sections": sections.iter().enumerate()
            .map(|(i, (_, text))| json!({ "i": i, "section": text }))
            .collect::<Vec<_>>(),
        "conversation": conversation,
        "theirLatestMessage": said,
        "reply": reply,
    })
    .to_string();
    let system = "You check which parts of a briefing a chat reply actually drew on. The briefing is given as numbered sections. List i for each section whose own specific content (a fact, a memory, a name, a state of hers, an event, a thing she did or has) shows in the reply: said outright, alluded to, or plainly shaping what it says. Rules about how to talk never count. Content that is also in the conversation or their latest message does not count for a section. Most replies draw on few sections or none.";
    let schema = json!({
        "type": "object",
        "properties": { "used": { "type": "array", "items": { "type": "integer" } } },
        "required": ["used"],
        "additionalProperties": false
    });
    let Some(judge) = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(None).await
    else {
        return Vec::new();
    };
    let Ok(raw) = judge
        .analyze_json(system, &input, "replay_drew_on", Some(&schema))
        .await
    else {
        return Vec::new();
    };
    let used: Vec<usize> = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| serde_json::from_value(value["used"].clone()).ok())
        .unwrap_or_default();
    used.into_iter()
        .filter_map(|i| sections.get(i).map(|(heading, _)| heading.clone()))
        .collect()
}

/// The prompt without the sections headed `headings`: each runs to the next
/// heading, or to the conversation.
fn without_sections(prompt: &str, headings: &[&str]) -> String {
    let mut prompt = prompt.to_string();
    for heading in headings {
        let marker = format!("\n## {heading}\n");
        let Some(start) = prompt.find(&marker) else {
            continue;
        };
        let rest = &prompt[start + 1..];
        let end = ["\n## ", "\nConversation:", "\nLatest message:"]
            .iter()
            .filter_map(|next| rest.find(next))
            .min()
            .map_or(prompt.len(), |end| start + 1 + end);
        prompt.replace_range(start..end, "");
    }
    prompt
}

/// The last lines of the conversation in a prompt, for a reader of a reply.
fn conversation_tail(prompt: &str) -> String {
    let tail = prompt
        .find("\nConversation:\n")
        .map(|start| &prompt[start..])
        .and_then(|rest| rest.find("\nLatest message:").map(|end| &rest[..end]))
        .unwrap_or_default();
    let lines: Vec<&str> = tail.lines().rev().take(8).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// Two replies to the same words, read blind (in either order): whether
/// they read as the same person talking the same way, and which answers the
/// moment better, as someone who knows them would. (same voice, preferred:
/// 0 for `a`, 1 for `b`, none for neither).
async fn compare(prompt: &str, said: &str, a: &str, b: &str) -> Option<(bool, Option<usize>)> {
    let flip: bool = rand::random();
    let (x, y) = if flip { (b, a) } else { (a, b) };
    let input = json!({
        "conversation": conversation_tail(prompt),
        "theirLatestMessage": said,
        "x": x,
        "y": y,
    })
    .to_string();
    let system = "Two replies, x and y, to the same message from someone she knows well. sameVoice: would someone who knows her take both as her, the same person talking the same way (tone, warmth, how much she says, how she treats them), allowing that a person never says the same thing twice? better: which answers this moment better, as one who knows them and what is going on between them: x, y, or same when neither does clearly. Judge only what is in the replies and the conversation.";
    let schema = json!({
        "type": "object",
        "properties": {
            "sameVoice": { "type": "boolean" },
            "better": { "type": "string", "enum": ["x", "y", "same"] }
        },
        "required": ["sameVoice", "better"],
        "additionalProperties": false
    });
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(None).await?;
    let raw = judge
        .analyze_json(system, &input, "replay_compare", Some(&schema))
        .await
        .ok()?;
    let value: Value = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim())
        .and_then(|json| serde_json::from_str(&json).ok())?;
    let same = value["sameVoice"].as_bool()?;
    let better = match (value["better"].as_str()?, flip) {
        ("x", false) | ("y", true) => Some(0),
        ("y", false) | ("x", true) => Some(1),
        _ => None,
    };
    Some((same, better))
}

#[derive(Default)]
struct Comparisons {
    judged: usize,
    same_voice: usize,
    first: usize,
    second: usize,
}

impl Comparisons {
    fn add(&mut self, judged: Option<(bool, Option<usize>)>) {
        let Some((same, better)) = judged else {
            return;
        };
        self.judged += 1;
        self.same_voice += usize::from(same);
        match better {
            Some(0) => self.first += 1,
            Some(_) => self.second += 1,
            None => {}
        }
    }

    fn line(&self, first: &str, second: &str) -> String {
        let n = self.judged.max(1) as f64;
        format!(
            "judged {}  same voice {:.0}%  better: {first} {}  {second} {}  neither {}",
            self.judged,
            self.same_voice as f64 / n * 100.0,
            self.first,
            self.second,
            self.judged - self.first - self.second,
        )
    }
}

/// MEROPE_REPLAY_N real turns (default 16), the latest, answered again;
/// MEROPE_REPLAY_REPORT=<file> keeps each turn's words, then and now.
/// MEROPE_REPLAY_SWAP=<old>|||<new>[&&&…] tries the prompt with passages
/// rewritten, to see what a wording would change before it ships.
/// MEROPE_REPLAY_PROMPTS=1 keeps each turn's whole prompt in the report;
/// MEROPE_REPLAY_ATTRIBUTE=1 asks which of its sections each reply drew on.
/// MEROPE_REPLAY_AB_DROP=<heading>[&&&…] also answers each turn again with
/// the prompt as is and without those sections, and reads both pairs blind:
/// the same prompt twice is how much replies differ anyway.
#[tokio::test]
#[ignore = "reads the site's database and asks its model"]
async fn her_real_talk_answered_again() {
    let db = crate::services::agent::semantic_eval::load_configured_lite().await;
    crate::services::process_db::set_process_database(db.clone());
    let n: i64 = std::env::var("MEROPE_REPLAY_N")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(16);
    let turns = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "WITH t AS (SELECT s.user_id, m.session_id, m.role, m.content, m.created_at, \
               lead(m.role) OVER w AS next_role, lead(m.content) OVER w AS next_content \
               FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
               WHERE s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
               WINDOW w AS (PARTITION BY s.user_id ORDER BY m.created_at)) \
             SELECT user_id, session_id, content, created_at, next_content FROM t \
             WHERE role = 'user' AND next_role = 'assistant' ORDER BY created_at DESC LIMIT $1",
            [n.into()],
        ))
        .await
        .unwrap();
    let analyzer = chat_analyzer().await.expect("chat model");
    let (mut then, mut now) = (Shape::default(), Shape::default());
    let (mut openers_then, mut openers_now) = (Vec::new(), Vec::new());
    let mut report = Vec::new();
    let drop: Vec<String> = std::env::var("MEROPE_REPLAY_AB_DROP")
        .map(|headings| headings.split("&&&").map(str::to_string).collect())
        .unwrap_or_default();
    let (mut again_shape, mut without_shape) = (Shape::default(), Shape::default());
    let (mut control, mut treatment) = (Comparisons::default(), Comparisons::default());
    let mut chars_saved = Vec::new();
    for turn in turns.iter().rev() {
        let user_id: i32 = turn.try_get("", "user_id").unwrap();
        let session: String = turn.try_get("", "session_id").unwrap();
        let said: String = turn.try_get("", "content").unwrap();
        let at: chrono::DateTime<chrono::FixedOffset> = turn.try_get("", "created_at").unwrap();
        let original: String = turn.try_get("", "next_content").unwrap();
        let history = history_before(&db, user_id, at).await;
        let opening = history
            .last()
            .and_then(|last| last.created_at.as_deref())
            .and_then(|last| chrono::DateTime::parse_from_rfc3339(last).ok())
            .is_none_or(|last| (at - last).num_hours() >= 3);
        let request = UserRequest {
            raw_input: said.clone(),
            // As it came then: how long since they last wrote is as it was.
            timestamp: at.with_timezone(&chrono::Utc),
            user_id,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                session_id: Some(session),
                conversation_history: Some(history),
                ..Default::default()
            }),
        };
        crate::services::agent::merope::remembering::begin(user_id, "private", &said);
        let agent = Agent::new(db.clone()).await;
        // As of then, and only looking: what comes to mind is not recalled.
        let prompt = crate::services::agent::memory::unified::quietly(
            crate::services::agent::merope::clock::as_of(
                at.with_timezone(&chrono::Utc),
                agent.chat_response_prompt(&request),
            ),
        )
        .await;
        let mut prompt = prompt;
        for swap in std::env::var("MEROPE_REPLAY_SWAP")
            .iter()
            .flat_map(|swaps| swaps.split("&&&"))
        {
            let (old, new) = swap.split_once("|||").expect("<old>|||<new>");
            assert!(
                prompt.contains(old),
                "the passage to rewrite is not in the prompt"
            );
            prompt = prompt.replace(old, new);
        }
        let again = analyzer
            .analyze_stream(&prompt, |_| true)
            .await
            .unwrap_or_default();
        then.add(&original);
        now.add(&again);
        if opening {
            openers_then.push(original.clone());
            openers_now.push(again.clone());
        }
        println!(
            "\n> {said}\n  then: {}\n  now:  {}",
            original.replace('\n', " / "),
            again.replace('\n', " / ")
        );
        let mut kept = json!({ "said": said, "at": at.to_rfc3339(), "opening": opening, "then": original, "now": again });
        // The whole prompt too, to see which parts of it her reply drew on.
        if std::env::var("MEROPE_REPLAY_PROMPTS").is_ok() {
            kept["prompt"] = json!(prompt);
        }
        if std::env::var("MEROPE_REPLAY_ATTRIBUTE").is_ok() {
            kept["drewOn"] = json!(drew_on(&prompt, &said, &again).await);
        }
        if !drop.is_empty() {
            let headings: Vec<&str> = drop.iter().map(String::as_str).collect();
            let lighter = without_sections(&prompt, &headings);
            chars_saved.push(prompt.chars().count() - lighter.chars().count());
            let twice = analyzer
                .analyze_stream(&prompt, |_| true)
                .await
                .unwrap_or_default();
            let without = analyzer
                .analyze_stream(&lighter, |_| true)
                .await
                .unwrap_or_default();
            again_shape.add(&twice);
            without_shape.add(&without);
            let same_prompt = compare(&prompt, &said, &again, &twice).await;
            let lighter_prompt = compare(&prompt, &said, &again, &without).await;
            control.add(same_prompt);
            treatment.add(lighter_prompt);
            println!(
                "  again: {}\n  without: {}",
                twice.replace('\n', " / "),
                without.replace('\n', " / ")
            );
            kept["again"] = json!(twice);
            kept["without"] = json!(without);
            kept["sameVsAgain"] = json!(same_prompt);
            kept["sameVsWithout"] = json!(lighter_prompt);
        }
        report.push(kept);
    }
    println!("\nthen  {}", then.line());
    println!("now   {}", now.line());
    if !drop.is_empty() {
        println!("again {}", again_shape.line());
        println!("without {}", without_shape.line());
        chars_saved.sort_unstable();
        println!(
            "without {:?}: chars saved median {}",
            drop,
            chars_saved.get(chars_saved.len() / 2).copied().unwrap_or(0)
        );
        println!("as is vs as is again:  {}", control.line("first", "again"));
        println!(
            "as is vs without:      {}",
            treatment.line("as is", "without")
        );
    }
    let leaning = |openers: &[String]| myriad_merope::vitals::leaned_on(openers, 0.3, 3);
    println!(
        "openers {}: then {:?}  now {:?}",
        openers_now.len(),
        leaning(&openers_then),
        leaning(&openers_now)
    );
    if let Ok(path) = std::env::var("MEROPE_REPLAY_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}

#[test]
fn a_section_left_out_runs_to_the_next_one_or_to_the_conversation() {
    let prompt = "Intro\n## About this person\nlikes cats\n\n## Now\nIt is 3pm.\n## Games\nZelda\n\nConversation:\nthem: hi\nLatest message: hi";
    let lighter = without_sections(prompt, &["About this person", "Games", "Not here"]);
    assert_eq!(
        lighter,
        "Intro\n## Now\nIt is 3pm.\nConversation:\nthem: hi\nLatest message: hi"
    );
    assert_eq!(conversation_tail(prompt), "\nConversation:\nthem: hi");
}
