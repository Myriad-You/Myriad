//! Her whole prompt for a scene, as the site puts it together, for setting
//! beside another end's (`myriad_merope::consistency::prompt_diff`). Both
//! ends read the same scene file and write what went to the model as a
//! `PromptRecording`; nothing here asks a model.
//!
//! `MYRIAD_CONSISTENCY_SCENES=<scenes.json>` and
//! `MYRIAD_CONSISTENCY_OUT=<dir>` (recordings stay out of the repository),
//! a throwaway database in `MYRIAD_MEDIA_TEST_DATABASE_URL` (a schema of its
//! own, dropped after), and `--ignored --test-threads=1`: the scene is set
//! as the process's database and configuration. The group scenes are
//! recorded by `channel_group::g0_scenes::consistency`.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{Value, json};

use super::*;

/// The scene file and where recordings go, or nothing when not asked for.
pub(crate) fn scenes() -> Option<(Value, std::path::PathBuf)> {
    let path = std::env::var("MYRIAD_CONSISTENCY_SCENES").ok()?;
    let out = std::env::var("MYRIAD_CONSISTENCY_OUT").ok()?;
    let file: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    Some((file, out.into()))
}

pub(crate) fn at(value: &Value) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(value.as_str().unwrap())
        .unwrap()
        .with_timezone(&chrono::Utc)
}

/// Write one recording: `messages` as `(role, text)`, in the order they went.
pub(crate) fn record(out: &std::path::Path, scene: &Value, messages: &[(&str, &str)]) {
    let name = scene["scene"].as_str().unwrap();
    let recording = json!({
        "version": myriad_merope::consistency::PROMPT_RECORDING_VERSION,
        "scene": name,
        "end": "site",
        "core": core_commit(),
        "input": scene,
        "messages": messages
            .iter()
            .map(|(role, text)| json!({ "role": role, "text": text }))
            .collect::<Vec<_>>(),
    });
    let path = out.join(format!("{}.json", name.replace('/', "__")));
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&recording).unwrap() + "\n",
    )
    .unwrap();
    println!("recorded {}", path.display());
}

/// The core commit the site is built against, as the submodule pins it.
fn core_commit() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "HEAD:core"])
        .output()
        .ok()?;
    let commit = String::from_utf8(out.stdout).ok()?;
    Some(commit.trim().chars().take(9).collect())
}

/// Her, as the scene file says, saved on the site; she is on, here.
pub(crate) async fn her_on_the_site<C: ConnectionTrait>(db: &C, file: &Value) {
    let her = &file["her"];
    super::merope::store::upsert_persona_on(
        db,
        her["name"].as_str().unwrap().to_string(),
        her["personality"].as_str().unwrap().to_string(),
        super::merope::store::PortraitUpdate::Keep,
        super::merope::store::PersonaContractUpdate {
            persona: super::merope::store::JsonDocumentUpdate::Set(
                json!({ "language": her["language"] }),
            ),
            zone: her["zone"].as_str().map(str::to_string),
            ..Default::default()
        },
        1,
    )
    .await
    .unwrap();
    let mut config = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    config.merope_enabled = true;
    config.pro_enabled = true;
    *crate::GLOBAL_DYNAMIC_CONFIG.write().await = config;
}

/// How she is with the owner, as the scene file says.
async fn affect_with<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    file: &Value,
    now: chrono::DateTime<chrono::Utc>,
) {
    let affect = &file["affect"];
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_addressee_state (user_id, mood, arousal, emotion, emotion_arousal, activity, \
           activity_updated_at, do_not_disturb, mood_settled_at, emotion_settled_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, '', $6, false, $6, $6, $6)",
        [
            user_id.into(),
            affect["mood"].as_f64().unwrap().into(),
            affect["arousal"].as_f64().unwrap().into(),
            affect["emotion"].as_f64().unwrap().into(),
            affect["emotionArousal"].as_f64().unwrap().into(),
            now.fixed_offset().into(),
        ],
    ))
    .await
    .unwrap();
}

/// The owner's private talk so far, in a session of its own; its id.
async fn talk_so_far<C: ConnectionTrait>(db: &C, user_id: i32, history: &[Value]) -> String {
    let session = format!("ses_{}", uuid::Uuid::new_v4().simple());
    let first = history
        .first()
        .map(|line| at(&line["at"]))
        .unwrap_or_else(chrono::Utc::now);
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, created_at, last_active_at) \
         VALUES ($1, $2, NULL, '{\"mode\":\"chat\"}'::json, $3, false, $4, $4)",
        [
            session.clone().into(),
            user_id.into(),
            (history.len() as i32).into(),
            first.fixed_offset().into(),
        ],
    ))
    .await
    .unwrap();
    for line in history {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) \
             VALUES ($1, $2, $3, NULL, $4)",
            [
                session.clone().into(),
                line["role"].as_str().unwrap().into(),
                line["content"].as_str().unwrap().into(),
                at(&line["at"]).fixed_offset().into(),
            ],
        ))
        .await
        .unwrap();
    }
    session
}

fn history_of(history: &[Value]) -> Vec<ConversationMessage> {
    history
        .iter()
        .map(|line| {
            super::chat_prompt::reconstruct_conversation_message(
                line["role"].as_str().unwrap().to_string(),
                line["content"].as_str().unwrap().to_string(),
                Some(at(&line["at"]).to_rfc3339()),
                None,
                true,
            )
        })
        .collect()
}

#[tokio::test]
#[ignore = "writes recordings of her prompts for a scene file"]
async fn her_private_prompts_recorded() {
    let Some((file, out)) = scenes() else {
        eprintln!("skipped: MYRIAD_CONSISTENCY_SCENES / MYRIAD_CONSISTENCY_OUT not set");
        return;
    };
    for scene in file["scenes"].as_array().unwrap() {
        let occasion = scene["occasion"].as_str().unwrap();
        if !occasion.starts_with("private_") {
            continue;
        }
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            eprintln!("skipped: no throwaway database");
            return;
        };
        let db = fixture.db.clone();
        crate::services::process_db::set_process_database(db.clone());
        let now = at(&scene["now"]);
        db.execute_unprepared(&format!(
            "INSERT INTO users (id, username, display_name) VALUES (1, 'owner1', '{}')",
            file["owner"]["name"].as_str().unwrap()
        ))
        .await
        .unwrap();
        her_on_the_site(&db, &file).await;
        affect_with(&db, 1, &file, now).await;
        let history = scene["history"].as_array().unwrap();
        let session = talk_so_far(&db, 1, history).await;
        match occasion {
            "private_reply" => {
                let said = scene["latest"].as_str().unwrap().to_string();
                let request = UserRequest {
                    raw_input: said.clone(),
                    timestamp: now,
                    user_id: 1,
                    context: Some(RequestContext {
                        interaction_mode: AgentInteractionMode::Chat,
                        session_id: Some(session),
                        conversation_history: Some(history_of(history)),
                        ..Default::default()
                    }),
                };
                super::merope::remembering::begin(1, "private", &said);
                let agent = Agent::new(db.clone()).await;
                let prompt = super::memory::unified::quietly(super::merope::clock::as_of(
                    now,
                    agent.chat_response_prompt(&request),
                ))
                .await;
                // The site sends a chat turn as one text (`analyze_stream_parts`).
                record(&out, scene, &[("user", &prompt)]);
            }
            "private_proactive" => {
                let (system, prompt) =
                    super::memory::unified::quietly(super::merope::clock::as_of(
                        now,
                        super::merope::ingest::proactive_prompt(
                            &db,
                            1,
                            scene["gist"].as_str().unwrap(),
                            scene["happened"].as_str(),
                        ),
                    ))
                    .await;
                // `call::Model::text(system, prompt)`.
                record(&out, scene, &[("system", &system), ("user", &prompt)]);
            }
            other => panic!("no occasion {other}"),
        }
        fixture.close().await;
    }
}

/// Her real private turns (`MYRIAD_CONSISTENCY_REPLAY=<dir>` holding
/// `her.json` and `turns.jsonl`, exported from the site and kept out of the
/// repository) answered again by the site's Lite model, each from the same
/// little the other end has: her persona, how she is (`affect`) and the talk
/// before it, as of when it was said. Writes `site-replies.jsonl`. Reads the
/// site's database only for its model configuration; asks the model once a
/// turn.
#[tokio::test]
#[ignore = "asks the site's model once per replayed turn"]
async fn her_real_turns_answered_on_a_bare_site() {
    let Ok(dir) = std::env::var("MYRIAD_CONSISTENCY_REPLAY") else {
        eprintln!("skipped: MYRIAD_CONSISTENCY_REPLAY not set");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let her: Value = serde_json::from_slice(&std::fs::read(dir.join("her.json")).unwrap()).unwrap();
    let turns: Vec<Value> = std::fs::read_to_string(dir.join("turns.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // The model as the site has it configured; nothing else of the site.
    let site = super::semantic_eval::load_configured_lite().await;
    let _ = site.close().await;
    let analyzer = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
        .await
        .map(crate::services::analyzer::AiAnalyzer::with_light_thinking)
        .expect("Lite model configured");
    let mut replies = String::new();
    for turn in &turns {
        let fixture = crate::federation::test_db::SchemaDb::new_or_media()
            .await
            .expect("a throwaway database");
        let db = fixture.db.clone();
        crate::services::process_db::set_process_database(db.clone());
        let now = at(&turn["at"]);
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (id, username, display_name) VALUES (1, 'owner1', $1)",
            [her["owner"]["name"].as_str().unwrap().into()],
        ))
        .await
        .unwrap();
        her_on_the_site(&db, &her).await;
        affect_with(&db, 1, &her, now).await;
        let history = turn["history"].as_array().unwrap();
        let session = talk_so_far(&db, 1, history).await;
        let said = turn["said"].as_str().unwrap().to_string();
        let request = UserRequest {
            raw_input: said.clone(),
            timestamp: now,
            user_id: 1,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                session_id: Some(session),
                conversation_history: Some(history_of(history)),
                ..Default::default()
            }),
        };
        super::merope::remembering::begin(1, "private", &said);
        let agent = Agent::new(db.clone()).await;
        let prompt = super::memory::unified::quietly(super::merope::clock::as_of(
            now,
            agent.chat_response_prompt(&request),
        ))
        .await;
        let reply = analyzer
            .analyze_stream(&prompt, |_| true)
            .await
            .unwrap_or_default();
        println!("> {said}\n  {}", reply.replace('\n', " / "));
        replies.push_str(&json!({ "id": turn["id"], "reply": reply }).to_string());
        replies.push('\n');
        fixture.close().await;
    }
    std::fs::write(dir.join("site-replies.jsonl"), replies).unwrap();
}

/// The other end's whole prompts for the same real turns
/// (`MYRIAD_CONSISTENCY_REPLAY=<dir>`, its `native-prompts/<id>.json`, as
/// that end put them together) answered by the same Lite model the site
/// replay uses: the two ends then differ only in how they put her together.
/// Writes `native-replies.jsonl`; asks the model once a prompt.
#[tokio::test]
#[ignore = "asks the site's model once per recorded prompt"]
async fn the_other_ends_prompts_answered_by_the_site_model() {
    let Ok(dir) = std::env::var("MYRIAD_CONSISTENCY_REPLAY") else {
        eprintln!("skipped: MYRIAD_CONSISTENCY_REPLAY not set");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let turns: Vec<Value> = std::fs::read_to_string(dir.join("turns.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let site = super::semantic_eval::load_configured_lite().await;
    let _ = site.close().await;
    let analyzer = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
        .await
        .map(crate::services::analyzer::AiAnalyzer::with_light_thinking)
        .expect("Lite model configured");
    let mut replies = String::new();
    for turn in &turns {
        let id = turn["id"].to_string();
        let path = dir
            .join("native-prompts")
            .join(format!("{}.json", id.trim_matches('"')));
        let recording: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut system = String::new();
        let mut messages = Vec::new();
        for message in recording["messages"].as_array().unwrap() {
            let (role, text) = (
                message["role"].as_str().unwrap(),
                message["text"].as_str().unwrap(),
            );
            if role == "system" && messages.is_empty() {
                system.push_str(text);
            } else {
                messages.push(crate::services::analyzer::ChatMessage {
                    role: role.to_string(),
                    content: text.to_string(),
                });
            }
        }
        let reply = analyzer
            .analyze_with_messages(&system, messages)
            .await
            .unwrap_or_default();
        println!("> {}\n  {}", turn["said"], reply.replace('\n', " / "));
        replies.push_str(&json!({ "id": turn["id"], "reply": reply }).to_string());
        replies.push('\n');
    }
    std::fs::write(dir.join("native-replies.jsonl"), replies).unwrap();
}
