//! 在后台画的图（手势手臂、表情重绘）各占一轮：请求被受理就答复，页面不挂着等，
//! 后端自己画完、存好。页面读列表知道哪些还在画；画成的在列表里就是新图，没画成的
//! 原因也留在这里由列表带出去——就是原先那次请求会答的那个错误，只给同一位站长看。
//! 进程重启后两样都清空，页面看到的是「不在画、也没有新图」。

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{Mutex, MutexGuard},
};

use axum::{Json, http::StatusCode};
use serde_json::{Map, Value, json};

use super::{ApiResult, internal_error};

/// What is being drawn, and why the last drawing of each was not kept.
pub(super) struct Turns(Mutex<State>);

struct State {
    drawing: BTreeSet<String>,
    failed: BTreeMap<String, Failed>,
}

/// A drawing that was not kept: of which picture, and what its request would
/// have been answered.
struct Failed {
    picture: String,
    status: StatusCode,
    body: Value,
}

/// One drawing's turn, held until it ends.
pub(super) struct Turn {
    turns: &'static Turns,
    key: String,
}

impl Turns {
    pub(super) const fn new() -> Self {
        Self(Mutex::new(State {
            drawing: BTreeSet::new(),
            failed: BTreeMap::new(),
        }))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// The turn to draw `key`, unless it is being drawn already: one drawing
    /// takes a minute or two and is paid for. What went wrong the last time is
    /// forgotten.
    pub(super) fn claim(&'static self, key: &str) -> Option<Turn> {
        let mut state = self.state();
        if !state.drawing.insert(key.to_string()) {
            return None;
        }
        state.failed.remove(key);
        Some(Turn {
            turns: self,
            key: key.to_string(),
        })
    }

    /// For a list, the keys under `scope` (without it): the ones being drawn,
    /// and the ones whose last drawing of `picture` was not kept, each with
    /// what went wrong. Read before the kept drawings are: one that ends in
    /// between is then found among them, never missing from both.
    pub(super) fn listed(&self, scope: &str, picture: &str) -> (Vec<String>, Map<String, Value>) {
        let state = self.state();
        let within = |key: &str| {
            key.strip_prefix(scope)
                .filter(|key| !key.contains('/'))
                .map(str::to_string)
        };
        let drawing = state.drawing.iter().filter_map(|key| within(key)).collect();
        let failed = state
            .failed
            .iter()
            .filter(|(_, failed)| failed.picture == picture)
            .filter_map(|(key, failed)| Some((within(key)?, failed.answer())))
            .collect();
        (drawing, failed)
    }
}

impl Failed {
    /// The body the request would have been answered, and its status.
    fn answer(&self) -> Value {
        let mut answer = match &self.body {
            Value::Object(body) => body.clone(),
            other => Map::from_iter([("error".to_string(), other.clone())]),
        };
        answer.insert("status".to_string(), json!(self.status.as_u16()));
        Value::Object(answer)
    }
}

impl Turn {
    /// Draws `picture`'s drawing in the background for as long as this turn
    /// is held. `work` keeps what it draws itself, before it ends; what it
    /// fails with is kept for the list.
    pub(super) fn run(
        self,
        picture: String,
        work: impl Future<Output = ApiResult<()>> + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            // On a task of its own: one that panics is a drawing not kept, like any other.
            let done = tokio::spawn(work)
                .await
                .unwrap_or_else(|error| Err(internal_error(error.to_string())));
            if let Err((status, Json(body))) = done {
                let code = body.get("code").and_then(Value::as_str).unwrap_or_default();
                tracing::warn!(key = %self.key, %status, code, "a drawing begun in the background was not kept");
                self.failed(picture, status, body);
            }
        })
    }

    /// Ends the turn with what went wrong, the two at once: a list never
    /// finds the drawing neither being drawn nor failed.
    fn failed(self, picture: String, status: StatusCode, body: Value) {
        let mut state = self.turns.state();
        state.drawing.remove(&self.key);
        let failed = Failed {
            picture,
            status,
            body,
        };
        state.failed.insert(self.key.clone(), failed);
        // Given back before the turn itself goes (which asks for it again).
        drop(state);
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        self.turns.state().drawing.remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejected() -> (StatusCode, Json<Value>) {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "not drawn", "code": "rejected", "differences": ["sleeve"] })),
        )
    }

    async fn broken() -> ApiResult<()> {
        panic!("drawing broke")
    }

    #[test]
    fn a_key_is_claimed_once_until_its_turn_ends() {
        static TURNS: Turns = Turns::new();
        let first = TURNS.claim("wave-left").expect("free");
        assert!(TURNS.claim("wave-left").is_none());
        assert!(TURNS.claim("wave-right").is_some());
        assert_eq!(TURNS.listed("", "a").0, ["wave-left"]);
        drop(first);
        assert!(TURNS.listed("", "a").0.is_empty());
        assert!(TURNS.claim("wave-left").is_some());
    }

    #[test]
    fn a_list_names_its_own_scope_s_keys_without_the_scope() {
        static TURNS: Turns = Turns::new();
        let _bust = TURNS.claim("chin-left").unwrap();
        let _set = TURNS.claim("w1/chin-right").unwrap();
        let other = TURNS.claim("w2/wave-left").unwrap();
        other.failed(
            "p2".into(),
            StatusCode::BAD_GATEWAY,
            json!({ "error": "x" }),
        );
        assert_eq!(TURNS.listed("", "p").0, ["chin-left"]);
        assert_eq!(TURNS.listed("w1/", "p").0, ["chin-right"]);
        let (drawing, failed) = TURNS.listed("w2/", "p2");
        assert!(drawing.is_empty());
        assert_eq!(failed.keys().collect::<Vec<_>>(), ["wave-left"]);
        // Another set's failure is not the bust's, nor this set's.
        assert!(TURNS.listed("", "p2").1.is_empty() && TURNS.listed("w1/", "p2").1.is_empty());
    }

    #[tokio::test]
    async fn what_went_wrong_is_listed_with_its_status_once_the_turn_has_ended() {
        static TURNS: Turns = Turns::new();
        let (release, held) = tokio::sync::oneshot::channel::<()>();
        let turn = TURNS.claim("point-left").unwrap();
        let running = turn.run("portrait-a".into(), async move {
            let _ = held.await;
            Err(rejected())
        });
        // Still being drawn: nothing has gone wrong yet.
        assert_eq!(TURNS.listed("", "portrait-a").0, ["point-left"]);
        assert!(TURNS.listed("", "portrait-a").1.is_empty());
        release.send(()).unwrap();
        running.await.unwrap();
        let (drawing, failed) = TURNS.listed("", "portrait-a");
        assert!(drawing.is_empty());
        assert_eq!(
            failed["point-left"],
            json!({ "error": "not drawn", "code": "rejected", "differences": ["sleeve"], "status": 422 })
        );
        // It was another picture's drawing that failed, not this one's.
        assert!(TURNS.listed("", "portrait-b").1.is_empty());
        // Asked for again, the old failure is forgotten.
        let again = TURNS.claim("point-left").unwrap();
        assert!(TURNS.listed("", "portrait-a").1.is_empty());
        again
            .run("portrait-a".into(), async { Ok(()) })
            .await
            .unwrap();
        let (drawing, failed) = TURNS.listed("", "portrait-a");
        assert!(drawing.is_empty() && failed.is_empty());
    }

    #[tokio::test]
    async fn a_drawing_that_panics_ends_its_turn_as_one_not_kept() {
        static TURNS: Turns = Turns::new();
        let turn = TURNS.claim("cry").unwrap();
        turn.run("portrait".into(), broken()).await.unwrap();
        let (drawing, failed) = TURNS.listed("", "portrait");
        assert!(drawing.is_empty());
        assert_eq!(failed["cry"]["status"], 500);
        assert_eq!(failed["cry"]["code"], "merope_rig_failed");
    }
}
