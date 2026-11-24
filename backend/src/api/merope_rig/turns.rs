//! 在后台画的图（手势手臂、表情重绘）各占一轮：请求被受理就答复，页面不挂着等，
//! 后端自己画完、存好。页面读列表知道哪些还在画；画成的在列表里就是新图，没画成的
//! 原因也留在这里由列表带出去——错误体和状态码，与请求当场被拒时答的是同一种，只给
//! 同一位站长看。进程重启后两样都清空，页面看到的是「不在画、也没有新图」。
//! 受理不等于马上画：同时在画的有上限（`DRAWN_AT_ONCE`），其余的排队，排队时列表里
//! 也算在画。

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{Mutex, MutexGuard},
};

use axum::{Json, http::StatusCode};
use serde_json::{Map, Value, json};
use tokio::sync::Semaphore;

use super::{ApiResult, internal_error};

/// 同时在画的上限，手势手臂和表情重绘合起来算：一套增强导入是八个手势加三个表情，
/// 每个都要调生图，手势还要调视觉检查。服务商被打满时那张图就画不成，那次检查按
/// 「不可用」放行等于没查，而它们的限额不在平台手里。六是浏览器对同一站点的连接
/// 上限：由页面逐个挂着请求来画，在途的至多这么多，这些服务商在本站只验证到这个
/// 并发，更高的没有依据。一个名额从开始画占到存好；占着名额的会去等姿势检测的
/// 名额，反过来没有，所以不成环。
const DRAWN_AT_ONCE: usize = 6;
static PLACES: Semaphore = Semaphore::const_new(DRAWN_AT_ONCE);

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
    /// is held, once there is a place for it among the ones being drawn.
    /// `work` keeps what it draws itself, before it ends; what it fails with
    /// is kept for the list.
    pub(super) fn run(
        self,
        picture: String,
        work: impl Future<Output = ApiResult<()>> + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        self.run_within(&PLACES, picture, work)
    }

    fn run_within(
        self,
        places: &'static Semaphore,
        picture: String,
        work: impl Future<Output = ApiResult<()>> + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        let key = self.key.clone();
        tokio::spawn(async move {
            // On a task of its own: one that panics is a drawing not kept, like any other.
            let done = tokio::spawn(async move {
                // Waited for in the order asked, and given back however the work ends.
                let asked = std::time::Instant::now();
                let _place = places.acquire().await;
                let waited = asked.elapsed().as_secs();
                if waited > 0 {
                    tracing::info!(%key, waited, "a drawing waited for its place among the ones being drawn");
                }
                work.await
            })
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

    /// A drawing that never gets its place fails its test instead of hanging it.
    async fn within_a_while<T>(work: impl Future<Output = T>) -> T {
        tokio::time::timeout(std::time::Duration::from_secs(20), work)
            .await
            .expect("the drawings did not go on")
    }

    /// Lets the drawings in the background run until `so`.
    async fn until(so: impl Fn() -> bool) {
        while !so() {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn no_more_are_drawn_at_once_than_there_are_places_and_the_rest_follow_in_order() {
        static TURNS: Turns = Turns::new();
        static PLACES: Semaphore = Semaphore::const_new(6);
        static BEGUN: Mutex<Vec<usize>> = Mutex::new(Vec::new());
        let begun = || BEGUN.lock().unwrap().clone();
        let (mut release, mut running) = (Vec::new(), Vec::new());
        for i in 0..8 {
            let (go, held) = tokio::sync::oneshot::channel::<()>();
            release.push(go);
            let turn = TURNS.claim(&format!("gesture-{i}")).unwrap();
            running.push(turn.run_within(&PLACES, "p".into(), async move {
                BEGUN.lock().unwrap().push(i);
                let _ = held.await;
                Ok(())
            }));
        }
        within_a_while(until(|| begun().len() == 6)).await;
        // Given every chance to, the other two do not begin.
        for _ in 0..100 {
            tokio::task::yield_now().await;
        }
        assert_eq!(begun(), [0, 1, 2, 3, 4, 5]);
        assert_eq!(PLACES.available_permits(), 0);
        // Waiting for a place is being drawn, to the list.
        assert_eq!(TURNS.listed("", "p").0.len(), 8);
        // Each place given back goes to the one that has waited longest.
        let mut release = release.into_iter();
        for next in [6, 7] {
            release.next().unwrap().send(()).unwrap();
            within_a_while(until(|| begun().len() == next + 1)).await;
            assert_eq!(begun()[next], next);
        }
        for go in release {
            go.send(()).unwrap();
        }
        for drawn in running {
            within_a_while(drawn).await.unwrap();
        }
        assert!(TURNS.listed("", "p").0.is_empty() && TURNS.listed("", "p").1.is_empty());
        assert_eq!(PLACES.available_permits(), 6);
    }

    #[tokio::test]
    async fn a_drawing_that_panics_gives_its_place_back() {
        static TURNS: Turns = Turns::new();
        static PLACES: Semaphore = Semaphore::const_new(1);
        let broke = TURNS.claim("cry").unwrap();
        within_a_while(broke.run_within(&PLACES, "p".into(), broken()))
            .await
            .unwrap();
        // The only place there is: the next drawing would wait for ever were it still taken.
        let next = TURNS.claim("close").unwrap();
        within_a_while(next.run_within(&PLACES, "p".into(), async { Ok(()) }))
            .await
            .unwrap();
        assert_eq!(PLACES.available_permits(), 1);
        assert_eq!(TURNS.listed("", "p").1.keys().collect::<Vec<_>>(), ["cry"]);
    }
}
