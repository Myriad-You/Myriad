//! How long each stage of one reply turn took (P13), logged once per turn as
//! numbers only: milliseconds, how many times the model was asked, and the
//! user id the chat logs already carry. Nothing said, no name, prompt or
//! memory is ever in it.
//!
//! A turn's [`TurnTimer`] times what it wraps. The stages deep inside (her
//! wait for cues, her recollection) and the model's first piece of answer
//! are noted through a task-local, so nothing on the way down changes shape;
//! outside a timed turn the notes do nothing.
//!
//! The line goes out when the turn finishes ([`TurnTimer::finish`]) or, when
//! it never gets there (an early return, the turn cancelled or dropped), when
//! the timer is dropped: `dropped=true`, with only the stages it reached.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

/// Not noted (yet).
const UNSET: u64 = u64::MAX;

fn ms(took: Duration) -> u64 {
    u64::try_from(took.as_millis()).unwrap_or(UNSET - 1)
}

/// What is noted from inside a timed turn. Instants are kept as
/// milliseconds since `started`.
#[derive(Debug)]
struct Marks {
    started: Instant,
    cues_ms: AtomicU64,
    recollection_ms: AtomicU64,
    asked_at: AtomicU64,
    first_token_at: AtomicU64,
}

impl Marks {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            cues_ms: AtomicU64::new(UNSET),
            recollection_ms: AtomicU64::new(UNSET),
            asked_at: AtomicU64::new(UNSET),
            first_token_at: AtomicU64::new(UNSET),
        }
    }

    fn since_start(&self) -> u64 {
        ms(self.started.elapsed())
    }
}

fn noted(cell: &AtomicU64) -> Option<u64> {
    Some(cell.load(Relaxed)).filter(|&value| value != UNSET)
}

tokio::task_local! {
    static MARKS: Arc<Marks>;
}

/// `note` the turn timed here; nothing outside a timed turn.
fn with_marks(note: impl FnOnce(&Arc<Marks>)) {
    let _ = MARKS.try_with(note);
}

/// Her wait for the cues of what to remember (`remembering::cues`) took
/// `took`.
pub(crate) fn cues_waited(took: Duration) {
    with_marks(|marks| marks.cues_ms.store(ms(took), Relaxed));
}

/// Her whole recollection, the wait for cues included, took `took`.
pub(crate) fn recollected(took: Duration) {
    with_marks(|marks| marks.recollection_ms.store(ms(took), Relaxed));
}

/// A piece of the model's answer (words or sound) came now. Only the first
/// after each ask counts.
pub(crate) fn first_token() {
    with_marks(|marks| {
        if marks.asked_at.load(Relaxed) == UNSET {
            return;
        }
        let at = marks.since_start();
        let first = &marks.first_token_at;
        let _ = first.compare_exchange(UNSET, at, Relaxed, Relaxed);
    });
}

/// One reply turn's line. Numbers and flags only: there is nowhere in it to
/// put words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReplyTiming {
    pub(crate) user_id: i32,
    /// A group turn (`group_turn`), not a private chat.
    pub(crate) group: bool,
    /// How many times the model was asked (once more after an empty or cut
    /// reply).
    pub(crate) attempts: u32,
    /// 1. Her wait for the cues of what to remember.
    pub(crate) cues_ms: Option<u64>,
    /// 2. The rest of her recollection: recall, what she always knows of
    /// them, scrolling back.
    pub(crate) recall_ms: Option<u64>,
    /// 3. Putting the whole prompt together, 1 and 2 included.
    pub(crate) prompt_ms: Option<u64>,
    /// 4. From the last ask to the model's first piece of answer.
    pub(crate) first_token_ms: Option<u64>,
    /// 5. From the last ask to its whole answer, or to its failing.
    pub(crate) completion_ms: Option<u64>,
    /// The whole turn as timed.
    pub(crate) total_ms: u64,
    pub(crate) ok: bool,
    /// It ended before it finished: an early return, or cancelled.
    pub(crate) dropped: bool,
}

impl ReplyTiming {
    /// A stage it did not reach is left out of the line.
    pub(crate) fn log(&self) {
        tracing::info!(
            user_id = self.user_id,
            group = self.group,
            attempts = self.attempts,
            cues_ms = self.cues_ms,
            recall_ms = self.recall_ms,
            prompt_ms = self.prompt_ms,
            first_token_ms = self.first_token_ms,
            completion_ms = self.completion_ms,
            total_ms = self.total_ms,
            ok = self.ok,
            dropped = self.dropped,
            "[Chat] reply timing"
        );
    }
}

/// Times one reply turn, and logs its line once: finished, or dropped.
#[derive(Debug)]
pub(crate) struct TurnTimer {
    marks: Arc<Marks>,
    user_id: i32,
    group: bool,
    attempts: u32,
    prompt_ms: Option<u64>,
    completion_ms: Option<u64>,
    logged: bool,
}

impl TurnTimer {
    pub(crate) fn start(user_id: i32, group: bool) -> Self {
        Self {
            marks: Arc::new(Marks::new()),
            user_id,
            group,
            attempts: 0,
            prompt_ms: None,
            completion_ms: None,
            logged: false,
        }
    }

    /// Put the prompt together within the turn: how long it took, and what
    /// her cues and recollection note on the way. `work` is boxed at once:
    /// a layer holding it unboxed would double the futures above it
    /// (`call_future_sizes_tests`).
    pub(crate) fn prompt<F: Future>(&mut self, work: F) -> impl Future<Output = F::Output> {
        let work = Box::pin(work);
        async move {
            let started = self.marks.since_start();
            let out = MARKS.scope(self.marks.clone(), work).await;
            self.prompt_ms = Some(self.marks.since_start().saturating_sub(started));
            out
        }
    }

    /// Ask the model within the turn. Each ask starts the first piece and
    /// the completion over. `work` is boxed at once, as for [`Self::prompt`].
    pub(crate) fn model<F: Future>(&mut self, work: F) -> impl Future<Output = F::Output> {
        let work = Box::pin(work);
        async move {
            self.attempts = self.attempts.saturating_add(1);
            self.completion_ms = None;
            self.marks.first_token_at.store(UNSET, Relaxed);
            let asked = self.marks.since_start();
            self.marks.asked_at.store(asked, Relaxed);
            let out = MARKS.scope(self.marks.clone(), work).await;
            self.completion_ms = Some(self.marks.since_start().saturating_sub(asked));
            out
        }
    }

    /// The turn is over; `ok` when it gave a reply.
    pub(crate) fn finish(mut self, ok: bool) {
        self.emit(ok, false);
    }

    fn line(&self, ok: bool, dropped: bool) -> ReplyTiming {
        let cues_ms = noted(&self.marks.cues_ms);
        let recall_ms = noted(&self.marks.recollection_ms)
            .map(|all| all.saturating_sub(cues_ms.unwrap_or(0)));
        let first_token_ms = noted(&self.marks.asked_at)
            .zip(noted(&self.marks.first_token_at))
            .map(|(asked, first)| first.saturating_sub(asked));
        ReplyTiming {
            user_id: self.user_id,
            group: self.group,
            attempts: self.attempts,
            cues_ms,
            recall_ms,
            prompt_ms: self.prompt_ms,
            first_token_ms,
            completion_ms: self.completion_ms,
            total_ms: self.marks.since_start(),
            ok,
            dropped,
        }
    }

    fn emit(&mut self, ok: bool, dropped: bool) {
        if std::mem::replace(&mut self.logged, true) {
            return;
        }
        self.line(ok, dropped).log();
    }
}

impl Drop for TurnTimer {
    fn drop(&mut self) {
        self.emit(false, true);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use futures::FutureExt;
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context, SubscriberExt};

    use super::*;

    /// How a field reached the log.
    #[derive(Clone, Debug, PartialEq)]
    enum Logged {
        Number(i128),
        Flag(bool),
        /// Anything else: text, or something formatted.
        Other(String),
    }

    type Line = BTreeMap<String, Logged>;

    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<Line>>>);

    struct Fields<'a>(&'a mut Line);

    impl Fields<'_> {
        fn put(&mut self, field: &Field, value: Logged) {
            self.0.insert(field.name().into(), value);
        }
    }

    impl Visit for Fields<'_> {
        fn record_u64(&mut self, field: &Field, value: u64) {
            self.put(field, Logged::Number(value.into()));
        }

        fn record_i64(&mut self, field: &Field, value: i64) {
            self.put(field, Logged::Number(value.into()));
        }

        fn record_bool(&mut self, field: &Field, value: bool) {
            self.put(field, Logged::Flag(value));
        }

        fn record_str(&mut self, field: &Field, value: &str) {
            self.put(field, Logged::Other(value.into()));
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.put(field, Logged::Other(format!("{value:?}")));
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Lines {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            let mut line = Line::new();
            event.record(&mut Fields(&mut line));
            self.0.lock().unwrap().push(line);
        }
    }

    /// Every line logged while `work` runs.
    fn logged(work: impl FnOnce()) -> Vec<Line> {
        let lines = Lines::default();
        let subscriber = tracing_subscriber::registry().with(lines.clone());
        tracing::subscriber::with_default(subscriber, work);
        lines.0.lock().unwrap().clone()
    }

    /// `work`, which waits on nothing, run to its end.
    fn now<F: Future>(work: F) -> F::Output {
        work.now_or_never().expect("waits on nothing")
    }

    fn number(line: &Line, name: &str) -> Option<i128> {
        match line.get(name) {
            Some(Logged::Number(value)) => Some(*value),
            None => None,
            other => panic!("{name} is not a number: {other:?}"),
        }
    }

    fn flag(line: &Line, name: &str) -> bool {
        match line.get(name) {
            Some(Logged::Flag(value)) => *value,
            other => panic!("{name} is not a flag: {other:?}"),
        }
    }

    const NAMES: [&str; 11] = [
        "user_id",
        "group",
        "attempts",
        "cues_ms",
        "recall_ms",
        "prompt_ms",
        "first_token_ms",
        "completion_ms",
        "total_ms",
        "ok",
        "dropped",
    ];

    const STAGES: [&str; 5] = [
        "cues_ms",
        "recall_ms",
        "prompt_ms",
        "first_token_ms",
        "completion_ms",
    ];

    /// Only the fixed message, and numbers and flags under the known names.
    fn numbers_only(line: &Line) {
        let message = Logged::Other("[Chat] reply timing".into());
        assert_eq!(line.get("message"), Some(&message));
        for (name, value) in line.iter().filter(|(name, _)| *name != "message") {
            assert!(NAMES.contains(&name.as_str()), "field {name}");
            assert!(
                matches!(value, Logged::Number(_) | Logged::Flag(_)),
                "{name} logged as {value:?}"
            );
        }
    }

    async fn cue_and_recall() {
        cues_waited(Duration::from_millis(7));
        recollected(Duration::from_millis(20));
    }

    async fn recall_only() {
        recollected(Duration::from_millis(20));
    }

    async fn piece() {
        first_token();
    }

    /// A first piece, pinned at 500 ms after the ask, then a later piece.
    async fn pinned_then_later(marks: Arc<Marks>) {
        first_token();
        assert_ne!(marks.first_token_at.load(Relaxed), UNSET);
        let asked = marks.asked_at.load(Relaxed);
        marks.first_token_at.store(asked + 500, Relaxed);
        first_token();
    }

    #[test]
    fn the_line_is_numbers_and_flags_only_under_known_names() {
        let timing = ReplyTiming {
            user_id: 42,
            group: true,
            attempts: 2,
            cues_ms: Some(145),
            recall_ms: Some(34),
            prompt_ms: Some(210),
            first_token_ms: Some(412),
            completion_ms: Some(980),
            total_ms: 1190,
            ok: true,
            dropped: false,
        };
        let lines = logged(|| timing.log());
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        numbers_only(line);
        let names: Vec<&str> = line
            .keys()
            .map(String::as_str)
            .filter(|name| *name != "message")
            .collect();
        let mut expected = NAMES.to_vec();
        expected.sort_unstable();
        assert_eq!(names, expected);
        assert_eq!(number(line, "user_id"), Some(42));
        assert_eq!(number(line, "attempts"), Some(2));
        assert_eq!(number(line, "cues_ms"), Some(145));
        assert_eq!(number(line, "recall_ms"), Some(34));
        assert_eq!(number(line, "prompt_ms"), Some(210));
        assert_eq!(number(line, "first_token_ms"), Some(412));
        assert_eq!(number(line, "completion_ms"), Some(980));
        assert_eq!(number(line, "total_ms"), Some(1190));
        assert!(flag(line, "group"));
        assert!(flag(line, "ok"));
        assert!(!flag(line, "dropped"));
    }

    #[test]
    fn a_stage_not_reached_is_left_out_of_the_line() {
        let lines = logged(|| ReplyTiming::default().log());
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        numbers_only(line);
        for name in STAGES {
            assert!(!line.contains_key(name), "{name} logged");
        }
        assert_eq!(number(line, "total_ms"), Some(0));
    }

    #[test]
    fn cues_and_recollection_noted_within_the_prompt_make_its_stages() {
        let mut timer = TurnTimer::start(7, false);
        now(timer.prompt(cue_and_recall()));
        let line = timer.line(true, false);
        assert_eq!(line.cues_ms, Some(7));
        assert_eq!(line.recall_ms, Some(13));
        assert!(line.prompt_ms.is_some());
        assert_eq!(line.attempts, 0);
        assert_eq!(line.first_token_ms, None);
        assert_eq!(line.completion_ms, None);
        // No words to cue on: the whole recollection is recall.
        let mut plain = TurnTimer::start(7, false);
        now(plain.prompt(recall_only()));
        assert_eq!(plain.line(true, false).cues_ms, None);
        assert_eq!(plain.line(true, false).recall_ms, Some(20));
        timer.finish(true);
        plain.finish(true);
    }

    #[test]
    fn notes_outside_a_timed_turn_do_nothing() {
        now(cue_and_recall());
        now(piece());
        let mut timer = TurnTimer::start(7, false);
        now(timer.prompt(std::future::ready(())));
        // Noted once the prompt is put together: no longer within it.
        now(cue_and_recall());
        let line = timer.line(true, false);
        assert_eq!((line.cues_ms, line.recall_ms), (None, None));
        assert_eq!(line.first_token_ms, None);
        timer.finish(true);
    }

    #[test]
    fn only_the_first_piece_after_each_ask_counts() {
        let mut timer = TurnTimer::start(7, false);
        // A piece before any ask is not the model's answer.
        now(timer.prompt(piece()));
        assert_eq!(timer.line(true, false).first_token_ms, None);
        let marks = timer.marks.clone();
        now(timer.model(pinned_then_later(marks)));
        let line = timer.line(true, false);
        assert_eq!(line.attempts, 1);
        assert_eq!(line.first_token_ms, Some(500));
        assert!(line.completion_ms.is_some());
        // Asked once more: the first piece is that ask's, none yet.
        now(timer.model(std::future::ready(())));
        let line = timer.line(true, false);
        assert_eq!(line.attempts, 2);
        assert_eq!(line.first_token_ms, None);
        assert!(line.completion_ms.is_some());
        timer.finish(true);
    }

    #[test]
    fn a_finished_turn_logs_once() {
        let lines = logged(|| {
            let mut timer = TurnTimer::start(42, true);
            now(timer.prompt(std::future::ready(())));
            now(timer.model(piece()));
            timer.finish(true);
        });
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        numbers_only(line);
        assert!(flag(line, "ok"));
        assert!(!flag(line, "dropped"));
        assert!(flag(line, "group"));
        assert_eq!(number(line, "attempts"), Some(1));
        assert!(number(line, "first_token_ms").is_some());
        assert!(number(line, "completion_ms").is_some());
    }

    #[test]
    fn a_turn_left_early_logs_once_as_dropped() {
        // As `streamed_analyzer().await?` returning before anything else.
        let lines = logged(|| drop(TurnTimer::start(42, false)));
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        numbers_only(line);
        assert!(flag(line, "dropped"));
        assert!(!flag(line, "ok"));
        assert_eq!(number(line, "attempts"), Some(0));
        for name in STAGES {
            assert!(!line.contains_key(name), "{name} logged");
        }
    }

    #[test]
    fn a_turn_cancelled_while_the_model_answers_logs_what_it_reached() {
        let lines = logged(|| {
            let mut timer = TurnTimer::start(42, false);
            now(timer.prompt(std::future::ready(())));
            // The ask never comes back: cancelled (a newer turn, a stop, a
            // timeout), its future dropped unfinished.
            let asked = timer.model(std::future::pending::<()>());
            assert!(asked.now_or_never().is_none());
            drop(timer);
        });
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        numbers_only(line);
        assert!(flag(line, "dropped"));
        assert!(!flag(line, "ok"));
        assert_eq!(number(line, "attempts"), Some(1));
        assert!(number(line, "prompt_ms").is_some());
        assert_eq!(number(line, "first_token_ms"), None);
        assert_eq!(number(line, "completion_ms"), None);
    }
}
