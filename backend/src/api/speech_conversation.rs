//! Agora's cloud-facing Chat adapter and the browser's authenticated run notices.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Extension, Json,
};
use futures::{Stream, StreamExt};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::time::Duration;

use crate::middleware::auth::{revalidate_bound_claims, Claims};
use crate::services::agent::{AgentInteractionMode, AgentProgressEvent};
use crate::services::agora_chat::{
    completion_chunk, ChatSession, CompletionRequest, CALLBACK_PATH,
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
    let user_id = claims.sub.parse().unwrap_or(0);
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
    async_stream::stream! {
        let mut changed = session.subscribe();
        let events = super::agent::agent_run_envelopes(run.clone());
        futures::pin_mut!(events);
        let mut emitted = false;
        let mut sequence = 0;
        let mut missing_prefix = false;
        loop {
            let envelope = tokio::select! {
                next = events.next() => next,
                _ = changed.changed() => {
                    if session.is_closed().await { break; }
                    continue;
                }
            };
            if session.is_closed().await { break; }
            let Some(envelope) = envelope else { break; };
            // A bounded run replay may have evicted its first tokens. Wait for
            // the authoritative final reply instead of speaking only its tail.
            if envelope.sequence > sequence + 1 {
                if emitted {
                    yield Ok(Event::default().data(json!({"error":{"message":"Reply stream interrupted","type":"realtime_chat_error"}}).to_string()));
                    break;
                }
                missing_prefix = true;
            }
            sequence = envelope.sequence;
            match envelope.event {
                AgentProgressEvent::SummaryToken { token, done: false } if !missing_prefix && !token.is_empty() => {
                    emitted = true;
                    yield Ok(Event::default().data(completion_chunk(run.run_id(), Some(&token), false).to_string()));
                }
                AgentProgressEvent::TaskCompleted {success, response, ..} => {
                    if success {
                        if !emitted {
                            if let Some(text) = response.get("message").and_then(|v| v.as_str()) {
                                yield Ok(Event::default().data(completion_chunk(run.run_id(), Some(text), false).to_string()));
                            }
                        }
                        yield Ok(Event::default().data(completion_chunk(run.run_id(), None, true).to_string()));
                    } else {
                        yield Ok(Event::default().data(json!({"error":{"message":"Chat turn ended without a reply","type":"realtime_chat_error"}}).to_string()));
                    }
                    break;
                }
                AgentProgressEvent::Error {..} => {
                    // Do not forward provider error bodies or diagnostic details.
                    yield Ok(Event::default().data(json!({"error":{"message":"Chat is unavailable","type":"realtime_chat_error"}}).to_string()));
                    break;
                }
                _ => {},
            }
        }
        yield Ok(Event::default().data("[DONE]"));
    }
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
    let session = match crate::services::agora_convo::chat_session(
        claims.sub.parse().unwrap_or(0),
        &query.agent_id,
    )
    .await
    {
        Ok(session) if session.claims.tv == claims.tv => session,
        // 204 tells native EventSource not to retry an owner binding that no
        // longer exists. An active stream receives the explicit `closed` event.
        _ => return StatusCode::NO_CONTENT.into_response(),
    };
    let stream = async_stream::stream! {
        let mut changed = session.subscribe();
        let mut after = query.after;
        loop {
            let (notices, closed) = session.notices().await;
            for notice in notices.into_iter().filter(|notice| notice.sequence > after).collect::<Vec<_>>() {
                after = notice.sequence;
                yield Ok::<Event, Infallible>(Event::default().id(after.to_string()).json_data(notice).unwrap());
            }
            if closed {
                yield Ok(Event::default().event("closed").data("{}"));
                break;
            }
            if changed.changed().await.is_err() { break; }
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
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
        let run = crate::services::agent::run_hub::create_run(7201, Some("chat-wire".into())).await;
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
        let text = String::from_utf8(to_bytes(body, 64 * 1024).await.unwrap().to_vec()).unwrap();
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
            crate::services::agent::run_hub::create_run(7202, Some("chat-long-wire".into())).await;
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
        let text = String::from_utf8(to_bytes(body, 256 * 1024).await.unwrap().to_vec()).unwrap();
        assert_eq!(text.matches("whole reply").count(), 1);
        assert!(!text.contains("partial"));
        assert!(text.contains("[DONE]"));
        session.close().await;
    }
}
