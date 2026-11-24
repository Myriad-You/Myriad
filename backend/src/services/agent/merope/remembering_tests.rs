//! How what to remember is asked, and what each try came to.

use super::super::call::{Ask, Model};
use super::*;
use crate::services::analyzer::{AiAnalyzer, AiProvider};
use crate::services::channel_group::g0;
use serde_json::Value;

/// The bodies `ask` sends for what to remember of a message, and the input.
async fn sent_by(ask: Ask) -> (Vec<Value>, String) {
    use axum::{Json, Router, http::StatusCode, routing::post};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tell, mut told) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let server = tokio::spawn(async move {
        let answer = post(move |Json(body): Json<Value>| {
            let _ = tell.send(body);
            async { StatusCode::UNAUTHORIZED }
        });
        axum::serve(listener, Router::new().fallback(answer))
            .await
            .unwrap();
    });
    let analyzer = AiAnalyzer::new(
        AiProvider::OpenAI,
        "k".into(),
        "m".into(),
        Some(format!("http://{address}/v1")),
    )
    .await;
    let input = input("明天看电影", None, "2026-11-24");
    let model = Model::of(analyzer, ask.within(Duration::from_secs(5)));
    let _ = (model.json(&system(), &input, SCHEMA_NAME, &schema())).await;
    server.abort();
    let mut bodies = Vec::new();
    while let Ok(body) = told.try_recv() {
        bodies.push(body);
    }
    (bodies, input)
}

/// Asked as it was measured (2026-11-24, A2): no `response_format`, and the
/// schema after the message. Another step is held by the provider as ever.
#[tokio::test]
async fn what_to_remember_is_asked_with_its_schema_in_the_prompt_alone() {
    let (bodies, input) = sent_by(asking(Voice::Judge, 1)).await;
    let schema_said = schema().to_string();
    assert_eq!(
        bodies,
        [serde_json::json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": system()},
                {"role": "user", "content": format!(
                    "{input}\n\nReturn one valid JSON value only, without Markdown fences. \
                     The JSON value must satisfy this schema:\n{schema_said}"
                )},
            ],
        })]
    );
    // The end of what was sent then, as it was kept.
    assert!(schema_said.ends_with(
        "\"thorough\":{\"type\":\"boolean\"}},\"required\":[\"cues\",\"thorough\",\"range\",\
         \"open\",\"lookBack\",\"aboutYou\"],\"type\":\"object\"}"
    ));
    let (bodies, input) = sent_by(Ask::new(Voice::Judge, 1, "another")).await;
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        bodies[0]["response_format"]["json_schema"]["name"],
        SCHEMA_NAME
    );
    assert_eq!(bodies[0]["messages"][1]["content"], input);
}

/// Each try is counted as what it came to; one with no cues is an answer.
#[tokio::test]
async fn each_try_is_counted_as_what_it_came_to() {
    let mut world = g0::World::new(0.5);
    let answers = [
        Some(r#"{"cues":["电影"]}"#),
        Some(r#"{"cues":[]}"#),
        Some(r#"{"cues":["  "],"lookBack":true}"#),
        Some("好的"),
        None,
        Some(r#"{"cues":["早饭"]}"#),
        Some(r#"{"cues":["早饭"]}"#),
    ];
    let answers = answers.map(|answer| answer.map(str::to_string));
    world.models.insert("remembering".into(), answers.into());
    tries(|all| all.iter().for_each(|one| one.store(0, Relaxed)));
    g0::enter(world);
    for _ in 0..7 {
        let _ = think(1, "明天看电影".into()).await;
    }
    let _ = g0::leave();
    assert_eq!(
        counts(),
        serde_json::json!({"cues": 3, "empty": 2, "unreadable": 1, "unanswered": 1})
    );
}
