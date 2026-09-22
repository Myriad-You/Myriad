//! Agora's cloud-facing Chat adapter and the browser's authenticated run notices.

use axum::{
    Extension, Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures::{Stream, StreamExt};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::json;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use crate::middleware::auth::{Claims, revalidate_bound_claims};
use crate::services::agent::{AgentInteractionMode, AgentProgressEvent};
use crate::services::agora_chat::{
    CALLBACK_PATH, ChatSession, CompletionRequest, completion_chunk,
};

pub fn callback_url(base: &str) -> Result<String, &'static str> {
    let mut url = url::Url::parse(base.trim())
        .map_err(|_| "Realtime talk needs the site's public HTTPS address")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Realtime talk needs the site's public HTTPS origin");
    }
    match url.host() {
        Some(url::Host::Domain(host))
            if host.trim_end_matches('.').contains('.')
                && !host.trim_end_matches('.').ends_with(".localhost")
                && !host.trim_end_matches('.').ends_with(".local") => {}
        Some(url::Host::Ipv4(ip)) if myriad_outbound::is_public_ip(ip.into()) => {}
        Some(url::Host::Ipv6(ip)) if myriad_outbound::is_public_ip(ip.into()) => {}
        _ => return Err("Realtime talk needs a publicly reachable HTTPS address"),
    }
    url.set_path(CALLBACK_PATH);
    Ok(url.into())
}

fn failure(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({"error": {"message": message, "type": "realtime_chat_error"}})),
    )
        .into_response()
}

pub async fn chat_completion(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Json(request): Json<CompletionRequest>,
) -> Response {
    let key = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    let Some(session) = (match key {
        Some(key) => ChatSession::from_key(key).await,
        None => None,
    }) else {
        return failure(StatusCode::UNAUTHORIZED, "Realtime session is unavailable");
    };
    let claims = match revalidate_bound_claims(&session.claims, &db).await {
        Ok(claims) => claims,
        Err(_) => {
            session.close().await;
            return failure(StatusCode::UNAUTHORIZED, "Realtime session is unavailable");
        }
    };
    let (provider_id, input) = match request.utterance() {
        Ok(value) => value,
        Err(message) => return failure(StatusCode::BAD_REQUEST, message),
    };
    // The browser established this session. The cloud cannot choose a user,
    // session, model, Work mode, system message, or replacement history.
    let Some(user_id) = crate::services::tapp_ownership::positive_user_id(&claims.sub) else {
        return failure(StatusCode::FORBIDDEN, "A durable user account is required");
    };
    let owned = crate::models::entities::agent_sessions::Entity::find_by_id(&session.session_id)
        .filter(crate::models::entities::agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await;
    if !matches!(owned, Ok(Some(ref row)) if row.context.as_ref().and_then(|c| c.get("mode")).and_then(|v| v.as_str()) == Some("chat"))
    {
        return failure(StatusCode::NOT_FOUND, "Chat session is unavailable");
    }
    let run = session
        .start_or_replay(provider_id, &input, || async {
            let live = crate::services::agent::consciousness::last_live_presence(user_id);
            let context = super::agent::ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: Some(session.session_id.clone()),
                rig_state: live
                    .rig_state
                    .as_ref()
                    .and_then(|rig| serde_json::to_value(rig).ok()),
                custom_data: Some(live_chat_context(&live)),
                ..Default::default()
            };
            super::agent::start_process_run(
                db.clone(),
                claims,
                super::agent::ProcessRequest {
                    input: input.clone(),
                    context: Some(context),
                },
            )
            .await
            .map_err(|_| "Could not start realtime Chat".to_string())
        })
        .await;
    let run = match run {
        Ok(run) => run,
        Err(message) => return failure(StatusCode::CONFLICT, &message),
    };
    Sse::new(completion_stream(run, session))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response()
}

fn live_chat_context(
    live: &crate::services::agent::consciousness::SelfLivePresence,
) -> serde_json::Value {
    let elapsed_ms = live
        .captured_at
        .map(|captured| {
            chrono::Utc::now()
                .signed_duration_since(captured)
                .num_milliseconds()
                .max(0)
        })
        .unwrap_or(i64::MAX);
    let perception = live
        .perception_payload
        .iter()
        .filter_map(|item| {
            let mut item = item.clone();
            let remaining = item.get("ttlMs")?.as_i64()?.saturating_sub(elapsed_ms);
            if remaining <= 0 {
                return None;
            }
            item["ttlMs"] = json!(remaining);
            Some(item)
        })
        .collect::<Vec<_>>();
    let mut context = json!({
        "presence": {"faceVisible": live.face_visible, "pageVisible": live.page_visible,
            "panelVisible": live.panel_visible, "visibleMode": "chat", "speaking": live.speaking},
        "perception": perception,
    });
    if let Some(music) = live.music_status.as_ref() {
        context["musicStatus"] = music.clone();
    }
    context
}

fn completion_stream(
    run: std::sync::Arc<crate::services::agent::run_hub::AgentRun>,
    session: std::sync::Arc<ChatSession>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let changed = session.subscribe();
    let events = Box::pin(super::agent::agent_run_envelopes(run.clone()));
    futures::stream::unfold(
        Some(CompletionFeed {
            run,
            session,
            changed,
            events,
            emitted: false,
            sequence: 0,
            missing_prefix: false,
            pending: VecDeque::new(),
            finishing: false,
        }),
        |feed| async move {
            let mut feed = feed?;
            if let Some(event) = feed.pending.pop_front() {
                return Some((event, Some(feed)));
            }
            if feed.finishing {
                return Some((Ok(Event::default().data("[DONE]")), None));
            }
            loop {
                let envelope = tokio::select! {
                    next = feed.events.next() => next,
                    _ = feed.changed.changed() => {
                        let closed = feed.session.is_closed().await;
                        if closed {
                            return Some((Ok(Event::default().data("[DONE]")), None));
                        }
                        continue;
                    }
                };
                let closed = feed.session.is_closed().await;
                if closed {
                    return Some((Ok(Event::default().data("[DONE]")), None));
                }
                let Some(envelope) = envelope else {
                    return Some((Ok(Event::default().data("[DONE]")), None));
                };
                // A bounded run replay may have evicted its first tokens. Wait for
                // the authoritative final reply instead of speaking only its tail.
                if envelope.sequence > feed.sequence + 1 {
                    if feed.emitted {
                        feed.finishing = true;
                        return Some((
                            Ok(Event::default().data(
                                json!({"error":{"message":"Reply stream interrupted","type":"realtime_chat_error"}}).to_string(),
                            )),
                            Some(feed),
                        ));
                    }
                    feed.missing_prefix = true;
                }
                feed.sequence = envelope.sequence;
                match envelope.event {
                    AgentProgressEvent::SummaryToken { token, done: false }
                        if !feed.missing_prefix && !token.is_empty() =>
                    {
                        feed.emitted = true;
                        return Some((
                            Ok(Event::default().data(
                                completion_chunk(feed.run.run_id(), Some(&token), false)
                                    .to_string(),
                            )),
                            Some(feed),
                        ));
                    }
                    AgentProgressEvent::TaskCompleted {
                        success, response, ..
                    } => {
                        if success {
                            if !feed.emitted {
                                if let Some(text) = response.get("message").and_then(|v| v.as_str())
                                {
                                    feed.pending.push_back(Ok(Event::default().data(
                                        completion_chunk(feed.run.run_id(), Some(text), false)
                                            .to_string(),
                                    )));
                                }
                            }
                            feed.pending.push_back(Ok(Event::default().data(
                                completion_chunk(feed.run.run_id(), None, true).to_string(),
                            )));
                        } else {
                            feed.pending.push_back(Ok(Event::default().data(
                                json!({"error":{"message":"Chat turn ended without a reply","type":"realtime_chat_error"}}).to_string(),
                            )));
                        }
                        feed.finishing = true;
                        if let Some(event) = feed.pending.pop_front() {
                            return Some((event, Some(feed)));
                        }
                        return Some((Ok(Event::default().data("[DONE]")), None));
                    }
                    AgentProgressEvent::Error { .. } => {
                        // Do not forward provider error bodies or diagnostic details.
                        feed.finishing = true;
                        return Some((
                            Ok(Event::default().data(
                                json!({"error":{"message":"Chat is unavailable","type":"realtime_chat_error"}}).to_string(),
                            )),
                            Some(feed),
                        ));
                    }
                    _ => {}
                }
            }
        },
    )
}

struct CompletionFeed {
    run: std::sync::Arc<crate::services::agent::run_hub::AgentRun>,
    session: std::sync::Arc<ChatSession>,
    changed: tokio::sync::watch::Receiver<u64>,
    events: Pin<Box<dyn Stream<Item = crate::services::agent::run_hub::AgentRunEnvelope> + Send>>,
    emitted: bool,
    sequence: u64,
    missing_prefix: bool,
    pending: VecDeque<Result<Event, Infallible>>,
    finishing: bool,
}

#[derive(Deserialize)]
pub struct EventsQuery {
    pub agent_id: String,
    #[serde(default)]
    pub after: u64,
}

pub async fn conversation_events(
    Extension(claims): Extension<Claims>,
    Query(query): Query<EventsQuery>,
) -> Response {
    let Some(user_id) = crate::services::tapp_ownership::positive_user_id(&claims.sub) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let session = match crate::services::agora_convo::chat_session(user_id, &query.agent_id).await {
        Ok(session) if session.claims.tv == claims.tv => session,
        // No owned chat session or tv mismatch: 204. Active streams emit event("closed") on teardown.
        _ => return StatusCode::NO_CONTENT.into_response(),
    };
    let changed = session.subscribe();
    let stream = futures::stream::unfold(
        Some(ConversationNoticeFeed {
            session,
            changed,
            after: query.after,
            pending: VecDeque::new(),
            closed: false,
        }),
        |feed| async move {
            let mut feed = feed?;
            loop {
                if let Some(event) = feed.pending.pop_front() {
                    return Some((event, Some(feed)));
                }
                if feed.closed {
                    return Some((Ok(Event::default().event("closed").data("{}")), None));
                }
                let (notices, closed) = feed.session.notices().await;
                for notice in notices {
                    if notice.sequence <= feed.after {
                        continue;
                    }
                    let sequence = notice.sequence;
                    feed.after = sequence;
                    feed.pending.push_back(Ok::<Event, Infallible>(
                        Event::default()
                            .id(sequence.to_string())
                            .json_data(notice)
                            .unwrap(),
                    ));
                }
                if closed {
                    feed.closed = true;
                    continue;
                }
                if !feed.pending.is_empty() {
                    continue;
                }
                let notified = feed.changed.changed().await;
                if notified.is_err() {
                    return None;
                }
            }
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

struct ConversationNoticeFeed {
    session: std::sync::Arc<ChatSession>,
    changed: tokio::sync::watch::Receiver<u64>,
    after: u64,
    pending: VecDeque<Result<Event, Infallible>>,
    closed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[test]
    fn callback_uses_only_the_configured_public_site_origin() {
        assert_eq!(
            callback_url("https://myriad.example/"),
            Ok(format!("https://myriad.example{CALLBACK_PATH}"))
        );
        for invalid in [
            "http://myriad.example",
            "https://localhost",
            "https://localhost.",
            "https://127.0.0.1",
            "https://10.0.0.1",
            "https://[::1]",
            "https://user:password@myriad.example",
            "https://myriad.example/?key=secret",
            "https://myriad.example/subpath",
        ] {
            assert!(callback_url(invalid).is_err(), "{invalid}");
        }
    }

    #[tokio::test]
    async fn callback_rejects_missing_or_invalid_key_before_database_or_models() {
        for header in [None, Some("Bearer not-a-key"), Some("Basic fake")] {
            let mut headers = HeaderMap::new();
            if let Some(value) = header {
                headers.insert(axum::http::header::AUTHORIZATION, value.parse().unwrap());
            }
            let request = serde_json::from_value(json!({"stream":true, "messages":[]})).unwrap();
            let response =
                chat_completion(State(DatabaseConnection::default()), headers, Json(request)).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[test]
    fn voice_chat_reuses_only_sanitized_live_scene_and_player_facts() {
        let live = crate::services::agent::consciousness::live_presence_from_custom_data(&json!({
            "presence": {"pageVisible": true, "panelVisible": true},
            "perception": [{
                "sourceId": "music_track", "kind": "music", "ttlMs": 2000,
                "summary": "Song — Artist", "privacy": "consented",
                "safeFacts": {"title": "Song", "nested": {"secret": "no"}}
            }, {
                "sourceId": "pointer", "kind": "pointer", "ttlMs": 2000,
                "summary": "MUST_NOT_SURVIVE", "privacy": "local",
                "safeFacts": {"selected": true}
            }],
            "musicStatus": {
                "isPlaying": true, "isEnabled": true, "playUrl": "secret-url",
                "currentSong": {"name": "Song", "artist": "Artist", "playUrl": "secret-url"},
                "currentSongIndex": 2, "playlistLength": 4
            }
        }));
        let context = live_chat_context(&live);
        assert_eq!(context["perception"][0]["sourceId"], "music_track");
        assert_eq!(context["perception"][1]["summary"], "");
        assert_eq!(context["musicStatus"]["currentSong"]["name"], "Song");
        let wire = context.to_string();
        assert!(!wire.contains("MUST_NOT_SURVIVE"));
        assert!(!wire.contains("secret-url"));
        assert!(!wire.contains("nested"));
    }

    #[tokio::test]
    async fn cloud_stream_excludes_reasoning_and_waits_for_the_shared_terminal() {
        let claims = crate::middleware::auth::mint_session_claims(7201, "test", false, false, 0);
        let (session, key) = ChatSession::register(claims, "chat-wire".into())
            .await
            .unwrap();
        let run = crate::services::agent::run_hub::create_run_for_test(7201, Some("chat-wire".into())).await;
        run.publish(AgentProgressEvent::ThinkingToken {
            token: "private reasoning".into(),
            done: false,
        })
        .await;
        run.publish(AgentProgressEvent::SummaryToken {
            token: "你好".into(),
            done: false,
        })
        .await;
        run.publish(AgentProgressEvent::SummaryToken {
            token: "".into(),
            done: true,
        })
        .await;
        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: "".into(),
            success: true,
            response: Box::new(json!({"message":"你好","data":{"mode":"chat"}})),
        })
        .await;
        let body = Sse::new(completion_stream(run, session.clone()))
            .into_response()
            .into_body();
        let text = String::from_utf8(
            tokio::time::timeout(std::time::Duration::from_secs(5), to_bytes(body, 64 * 1024))
                .await
                .expect("speech stream must terminate")
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(text.contains("你好"));
        assert_eq!(text.matches("你好").count(), 1);
        assert!(!text.contains("private reasoning"));
        assert!(!text.contains(&key));
        assert!(text.contains("[DONE]"));
        session.close().await;
    }

    #[tokio::test]
    async fn an_evicted_token_prefix_is_replaced_by_the_complete_final_reply() {
        let claims = crate::middleware::auth::mint_session_claims(7202, "test", false, false, 0);
        let (session, _) = ChatSession::register(claims, "chat-long-wire".into())
            .await
            .unwrap();
        let run =
            crate::services::agent::run_hub::create_run_for_test(7202, Some("chat-long-wire".into())).await;
        for _ in 0..520 {
            run.publish(AgentProgressEvent::SummaryToken {
                token: "partial ".into(),
                done: false,
            })
            .await;
        }
        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: "".into(),
            success: true,
            response: Box::new(json!({"message":"whole reply","data":{"mode":"chat"}})),
        })
        .await;
        let body = Sse::new(completion_stream(run, session.clone()))
            .into_response()
            .into_body();
        let text = String::from_utf8(
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                to_bytes(body, 256 * 1024),
            )
            .await
            .expect("speech stream must terminate")
            .unwrap()
            .to_vec(),
        )
        .unwrap();
        assert_eq!(text.matches("whole reply").count(), 1);
        assert!(!text.contains("partial"));
        assert!(text.contains("[DONE]"));
        session.close().await;
    }
}
