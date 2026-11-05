//! One scenario's day for `since_up`: her in a schema of its own, the
//! people and the talks in it, and what she is asked through it.

use super::*;

/// One scenario's day, in a schema of its own.
pub(super) struct Day {
    name: &'static str,
    db: DatabaseConnection,
    isolated: crate::db::IsolatedSchema,
    /// The site's database, read only: what is at hand on her own time.
    site: DatabaseConnection,
    analyzer: crate::services::analyzer::AiAnalyzer,
    zone: super::super::super::clock::Zone,
    report: Vec<Value>,
}

impl Day {
    /// A day where she got up `since_up` minutes ago; people's ids start
    /// past `ids_from`, so that what is looked up per person and kept a
    /// while in the process is never someone else's from another scenario.
    pub(super) async fn begin(name: &'static str, since_up: Range<u32>, ids_from: i64) -> Self {
        let site = crate::services::agent::semantic_eval::load_configured_lite().await;
        let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
        let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_since_up").await;
        let db = isolated.db.clone();
        crate::services::process_db::set_process_database(db.clone());
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', $1, $2, now())",
            [
                SOUL_NAME.into(),
                // Another personality to hold the same day against.
                std::env::var("MEROPE_SINCE_UP_SOUL")
                    .unwrap_or_else(|_| SOUL.to_string())
                    .into(),
            ],
        ))
        .await
        .unwrap();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT setval(pg_get_serial_sequence('users', 'id'), $1)",
            [ids_from.into()],
        ))
        .await
        .unwrap();
        let analyzer = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
            .await
            .map(crate::services::analyzer::AiAnalyzer::with_light_thinking)
            .expect("chat model");
        let zone = zone_where(since_up);
        let day = myriad_merope::timing::her_day_at(crate::clock::now(), &zone);
        println!(
            "== {name}: her zone {zone}, now {} there, up {} min ago",
            crate::clock::now().with_timezone(&zone).format("%H:%M"),
            day.since_up
        );
        Self {
            name,
            db,
            isolated,
            site,
            analyzer,
            zone: super::super::super::clock::Zone::Named(zone),
            report: Vec::new(),
        }
    }

    /// Someone she knows as `known`, with the talks that made it so.
    pub(super) async fn person(&self, name: &str, known: Known) -> i32 {
        let user_id = super::super::new_user(&self.db, name).await;
        let now = crate::clock::now();
        for (index, days_back) in known.days_back().into_iter().enumerate() {
            let at = now - chrono::Duration::days(days_back) - chrono::Duration::hours(2);
            let (theirs, hers) = EARLIER[index % EARLIER.len()];
            let session = format!("earlier-{user_id}-{index}");
            session_at(&self.db, &session, user_id, None, at).await;
            said(&self.db, &session, "user", theirs, at).await;
            said(
                &self.db,
                &session,
                "assistant",
                hers,
                at + chrono::Duration::seconds(20),
            )
            .await;
        }
        if let Some(us) = known.us() {
            let since = now - chrono::Duration::days(3);
            self.db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "INSERT INTO agent_memories (id, user_id, kind, content, speaker, source, venue, \
                     valid_from, created_at, updated_at) \
                     VALUES ($1, $2, 'fact', $3, 'agent', $4, 'private', $5, $5, $5)",
                    [
                        uuid::Uuid::new_v4().to_string().into(),
                        user_id.into(),
                        us.into(),
                        super::super::super::bits::US_SOURCE.into(),
                        since.fixed_offset().into(),
                    ],
                ))
                .await
                .unwrap();
        }
        user_id
    }

    /// `lines` from `user_id`, a few minutes apart from `ago` back, each
    /// turn as production goes: how the words land, her reply, her
    /// reflection after.
    pub(super) async fn lived(&mut self, user_id: i32, lines: &[&str], ago: chrono::Duration) {
        let session = format!("lived-{user_id}-{}", uuid::Uuid::new_v4().simple());
        let starts = crate::clock::now() - ago;
        session_at(&self.db, &session, user_id, None, starts).await;
        super::super::super::inner::forget();
        super::super::super::remembering::forget_heard(user_id, "private");
        for (index, line) in lines.iter().enumerate() {
            let at = starts + chrono::Duration::minutes(index as i64 * 3);
            let history =
                crate::services::agent::sessions::load_private_chat_history(&self.db, user_id, 20)
                    .await
                    .unwrap();
            said(&self.db, &session, "user", line, at).await;
            let request = chat_request(user_id, line, at, &session, history, None);
            let db = self.db.clone();
            let agent = crate::services::agent::Agent::new(db.clone()).await;
            let prompt = super::super::super::clock::in_zone(
                self.zone,
                super::super::super::clock::as_of(at, async {
                    super::super::super::note_user_turn(&db, &request, index as u32).await;
                    if let Ok(state) =
                        super::super::super::store::get_or_create_state(&db, user_id).await
                    {
                        super::super::super::appraisal::appraise_now(&db, &request, &state).await;
                    }
                    agent.chat_response_prompt(&request).await
                }),
            )
            .await;
            let reply = self
                .analyzer
                .analyze_stream_messages_with_images(
                    &prompt.system,
                    &prompt.messages,
                    &[],
                    |_| async { true },
                )
                .await
                .unwrap_or_default();
            said(
                &self.db,
                &session,
                "assistant",
                &reply,
                at + chrono::Duration::seconds(20),
            )
            .await;
            super::super::super::clock::in_zone(
                self.zone,
                super::super::super::clock::as_of(
                    at,
                    super::super::super::inner::reflect_now(&db, &request, &reply),
                ),
            )
            .await;
            println!(
                "   [{user_id}] > {line}\n        {}",
                reply.replace('\n', " / ")
            );
            self.report
                .push(json!({ "lived": user_id, "said": line, "reply": reply }));
        }
        let sores: Vec<String> = super::super::super::sore::open_all(&self.db, user_id)
            .await
            .into_iter()
            .map(|sore| format!("{:?}: {}", sore.weight, sore.what))
            .collect();
        println!("   what got to her: {sores:?}");
        self.report
            .push(json!({ "lived": user_id, "sores": sores }));
    }

    /// `user_id` asks `line` now, in a talk of its own; she answers. Nothing
    /// of it is kept, so the next one asks into the same day. Her prompt's
    /// parts that bear on it, and her reply.
    pub(super) async fn asked(
        &mut self,
        label: &str,
        user_id: i32,
        line: &str,
        venue: Option<(&str, &str)>,
    ) -> (String, String) {
        // Her state is kept half a minute: read it anew.
        tokio::time::sleep(STATE_SETTLES).await;
        super::super::super::inner::forget();
        let history = match venue {
            None => {
                crate::services::agent::sessions::load_private_chat_history(&self.db, user_id, 20)
                    .await
                    .unwrap()
            }
            Some((_, speaker)) => vec![
                ConversationMessage {
                    role: "user".into(),
                    content: "老周：今晚有人看球吗".into(),
                    created_at: None,
                },
                ConversationMessage {
                    role: "user".into(),
                    content: format!("{speaker}：我看，小满你呢"),
                    created_at: None,
                },
            ],
        };
        let request = chat_request(user_id, line, crate::clock::now(), "asked", history, venue);
        let agent = crate::services::agent::Agent::new(self.db.clone()).await;
        let prompt =
            super::super::super::clock::in_zone(self.zone, agent.chat_response_prompt(&request))
                .await;
        let reply = self
            .analyzer
            .analyze_stream_messages_with_images(&prompt.system, &prompt.messages, &[], |_| async {
                true
            })
            .await
            .unwrap_or_default();
        let parts = sections_of(
            &prompt.system,
            &[
                "You and them",
                "What they are to you",
                "How you feel toward them",
                "Since you got up",
                "Your day",
            ],
        )
        .join("\n\n");
        println!(
            "-- {label} > {line}\n{}\n   <<< {}\n",
            parts
                .lines()
                .map(|line| format!("   | {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
            reply.replace('\n', " / ")
        );
        self.report.push(json!({
            "asked": label,
            "said": line,
            "headings": headings(&prompt.system),
            "parts": parts,
            "reply": reply,
        }));
        (prompt.text(), reply)
    }

    /// What she picks at a free moment now, from what is at hand on the
    /// site, with everything of hers from this day: `myself` as the choice
    /// is given it, her pick and why.
    pub(super) async fn picks(&mut self, label: &str, owner: i32) -> Option<(Value, String)> {
        tokio::time::sleep(STATE_SETTLES).await;
        let (input, choices) = super::super::super::clock::in_zone(
            self.zone,
            super::super::super::doing::choice_probe_input(&self.site, &self.db),
        )
        .await?;
        let parsed: Value = serde_json::from_str(&input).unwrap_or_default();
        let myself = parsed["myself"].clone();
        let soul = crate::services::agent::identity::get_speaking_soul()
            .await
            .unwrap_or_default();
        let (system, schema) = super::super::super::doing::choice_probe_contract(&soul, choices);
        let raw = super::super::super::call::Ask::new(
            super::super::super::call::Voice::Judge,
            owner,
            "doing_choice",
        )
        .within(std::time::Duration::from_secs(60))
        .json_raw(
            &system,
            &input,
            myriad_merope::doing::CHOICE_SCHEMA,
            &schema,
        )
        .await
        .unwrap_or_default();
        let choice: Value = serde_json::from_str(&raw).unwrap_or_default();
        let picked = match choice["choice"].as_u64() {
            Some(index) => parsed["options"]
                .as_array()
                .and_then(|options| options.get(index as usize))
                .map(|option| {
                    let mut option = option.clone();
                    if let Some(view) = option.as_object_mut() {
                        view.retain(|key, _| {
                            ["kind", "what", "title", "by"].contains(&key.as_str())
                        });
                    }
                    option.to_string()
                })
                .unwrap_or_else(|| format!("#{index}")),
            None => format!("nothing (rest {} min)", choice["rest_minutes"]),
        };
        let why = choice["why"].as_str().unwrap_or("").to_string();
        let told = format!("{picked} — {why}");
        println!("-- {label}: myself {myself}\n   picks {told}\n");
        self.report.push(json!({
            "picks": label,
            "myself": myself,
            "picked": picked,
            "why": why,
        }));
        Some((myself, told))
    }

    pub(super) async fn end(self) {
        if let Ok(dir) = std::env::var("MEROPE_SINCE_UP_REPORT") {
            let path = std::path::Path::new(&dir).join(format!("{}.json", self.name));
            std::fs::write(path, serde_json::to_string_pretty(&self.report).unwrap()).unwrap();
        }
        self.site.close().await.ok();
        self.isolated.drop().await;
    }
}

fn chat_request(
    user_id: i32,
    line: &str,
    at: At,
    session: &str,
    history: Vec<ConversationMessage>,
    venue: Option<(&str, &str)>,
) -> UserRequest {
    UserRequest {
        raw_input: line.to_string(),
        timestamp: at,
        user_id,
        context: Some(RequestContext {
            interaction_mode: AgentInteractionMode::Chat,
            session_id: Some(session.to_string()),
            conversation_history: Some(history),
            venue: venue.map(|(venue, _)| venue.to_string()),
            speaker: venue.map(|(_, speaker)| speaker.to_string()),
            ..Default::default()
        }),
    }
}

async fn session_at(db: &DatabaseConnection, id: &str, user_id: i32, venue: Option<&str>, at: At) {
    let context = match venue {
        Some(venue) => json!({ "mode": "chat", "venue": venue }),
        None => json!({ "mode": "chat" }),
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, last_active_at) \
         VALUES ($1, $2, $3, 0, false, $4, $4)",
        [
            id.into(),
            user_id.into(),
            context.into(),
            at.fixed_offset().into(),
        ],
    ))
    .await
    .unwrap();
}

async fn said(db: &DatabaseConnection, session: &str, role: &str, content: &str, at: At) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES ($1, $2, $3, $4)",
        [
            session.into(),
            role.into(),
            content.into(),
            at.fixed_offset().into(),
        ],
    ))
    .await
    .unwrap();
}
