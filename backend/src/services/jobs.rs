//! Process-owned background jobs.
//!
//! One cancellation root and one join set for every loop the web process runs
//! outside a request. Three kinds:
//!
//! - [`JobRunner::try_once`]: one finite task, admitted immediately or refused at
//!   capacity. Completion is reaped; panics are logged without retrying effects.
//! - [`JobRunner::periodic`]: a serial tick. Slow work delays its own next
//!   tick (missed ticks are skipped) and never overlaps itself or holds
//!   another job.
//! - [`JobRunner::supervised`]: one long-running future (a Postgres LISTEN
//!   session, say) that lives until cancelled. When it returns or panics it
//!   is started again after an exponential [`Backoff`]; a run that stayed up
//!   for at least the backoff ceiling resets the delay.
//!
//! Panic policy: a panicking tick (or supervised run) is caught, logged with the
//! job name, and the job resumes at its next tick (or restart). Every tick is a
//! fresh future from the job's factory, so a panic loses only that tick's work;
//! stopping the loop instead would silently end a scheduler for the rest of the
//! process lifetime.
//!
//! Shutdown cancels admission first, then waits for in-flight ticks up to a
//! deadline and aborts what is left. A tick is never interrupted before the
//! deadline, so work with a durable lease finishes or leaves the lease to expire.
//! A supervised run is dropped as soon as cancellation is observed, so supervised
//! work must be safe to drop at any await (a LISTEN `recv` is).
//!
//! The persona runtime keeps its own supervisor (`persona::drivers`): there a
//! stopped driver is a worker failure that restarts the process, which is a
//! different contract from these best-effort maintenance loops.
//!
//! New long-lived loops register here instead of spawning their own `loop`;
//! `long_lived_loops_are_not_spawned_outside_the_runner` scans the source and
//! lists the exceptions (supervisors, per-connection and per-lease tasks) with
//! their reasons.

use futures::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

/// When a periodic job runs.
#[derive(Clone, Copy, Debug)]
pub struct Every {
    period: Duration,
    first_delay: Duration,
    jitter: Duration,
    spaced: bool,
}

impl Every {
    /// First tick runs immediately, then every `period`.
    pub const fn new(period: Duration) -> Self {
        Self {
            period,
            first_delay: Duration::ZERO,
            jitter: Duration::ZERO,
            spaced: false,
        }
    }

    /// Delay the first tick.
    pub const fn after(mut self, first_delay: Duration) -> Self {
        self.first_delay = first_delay;
        self
    }

    /// Wait a random `0..=jitter` before each tick, so replicas sharing a
    /// database lease do not all wake on the same instant.
    pub const fn jitter(mut self, jitter: Duration) -> Self {
        self.jitter = jitter;
        self
    }

    /// Measure `period` from the end of each tick instead of its start, so a
    /// long tick is always followed by a full pause rather than an immediate
    /// catch-up tick. For batch jobs that should leave the database breathing
    /// room between batches.
    pub const fn spaced(mut self) -> Self {
        self.spaced = true;
        self
    }
}

/// How long a [`JobRunner::supervised`] job waits before starting again after
/// its run ended: `initial`, doubling per consecutive short run, capped at
/// `max`. A run that lasted at least `max` counts as healthy and resets the
/// next delay to `initial`.
#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    initial: Duration,
    max: Duration,
}

impl Backoff {
    pub const fn new(initial: Duration, max: Duration) -> Self {
        Self { initial, max }
    }

    fn next(&self, previous: Option<Duration>, ran_for: Duration) -> Duration {
        match previous {
            Some(previous) if ran_for < self.max => previous.saturating_mul(2).min(self.max),
            _ => self.initial.min(self.max),
        }
    }
}

/// Stops one job without touching the others. Dropping it does not stop the job.
#[derive(Debug)]
pub struct JobHandle(CancellationToken);

impl JobHandle {
    pub fn cancel(&self) {
        self.0.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

pub struct JobRunner {
    root: CancellationToken,
    tasks: Mutex<JoinSet<&'static str>>,
}

impl Default for JobRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRunner {
    /// Needs no runtime; jobs are spawned when registered.
    pub fn new() -> Self {
        Self {
            root: CancellationToken::new(),
            tasks: Mutex::new(JoinSet::new()),
        }
    }

    /// Register a periodic job. After [`JobRunner::shutdown`] started, the job
    /// is refused and the returned handle is already cancelled.
    pub fn periodic<F, Fut>(&self, name: &'static str, every: Every, work: F) -> JobHandle
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.register(name, move |cancel| run_periodic(name, cancel, every, work))
    }

    /// Register a supervised job: `run()` starts now and starts again, after
    /// `backoff`, whenever its future returns or panics, until the job or the
    /// runner is cancelled. After [`JobRunner::shutdown`] started, the job is
    /// refused and the returned handle is already cancelled.
    pub fn supervised<F, Fut>(&self, name: &'static str, backoff: Backoff, run: F) -> JobHandle
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.register(name, move |cancel| {
            run_supervised(name, cancel, backoff, run)
        })
    }

    /// Admit a one-shot background task without creating an unbounded waiter queue.
    /// Finished tasks are reaped before checking capacity. Panics are observed here;
    /// work is never retried because it may already have performed side effects.
    pub fn try_once<F>(&self, name: &'static str, capacity: usize, work: F) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        while let Some(finished) = tasks.try_join_next() {
            log_join_result(finished);
        }
        if self.root.is_cancelled() || tasks.len() >= capacity {
            tracing::warn!(
                job = name,
                stopping = self.root.is_cancelled(),
                capacity,
                "background task refused"
            );
            return false;
        }
        tasks.spawn(async move {
            if let Err(panic) = AssertUnwindSafe(work).catch_unwind().await {
                tracing::error!(
                    job = name,
                    panic = panic_message(panic.as_ref()),
                    "background task panicked"
                );
            }
            name
        });
        true
    }

    /// Close admission immediately, before starting any asynchronous drain.
    pub fn stop_admission(&self) {
        let _guard = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        self.root.cancel();
    }

    fn register<J>(&self, name: &'static str, job: impl FnOnce(CancellationToken) -> J) -> JobHandle
    where
        J: Future<Output = ()> + Send + 'static,
    {
        let cancel = self.root.child_token();
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        // Checked under the lock: shutdown cancels before it takes the set, so
        // nothing can slip in after the drain started.
        if self.root.is_cancelled() {
            tracing::warn!(
                job = name,
                "background job refused: runner is shutting down"
            );
            return JobHandle(cancel);
        }
        // Stopped jobs (JobHandle::cancel) leave results behind; reap them here
        // so start/stop cycles do not grow the set.
        while let Some(finished) = tasks.try_join_next() {
            log_join_result(finished);
        }
        let job = job(cancel.clone());
        tasks.spawn(async move {
            job.await;
            name
        });
        JobHandle(cancel)
    }

    /// Stop admission, wait for in-flight ticks until `timeout`, then abort
    /// the rest. Idempotent.
    pub async fn shutdown(&self, timeout: Duration) {
        let mut tasks = {
            let mut guard = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
            self.root.cancel();
            std::mem::take(&mut *guard)
        };
        let drained = tokio::time::timeout(timeout, async {
            while let Some(finished) = tasks.join_next().await {
                log_join_result(finished);
            }
        })
        .await;
        if drained.is_err() {
            tracing::warn!(
                remaining = tasks.len(),
                timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                "background job drain deadline reached; aborting in-flight work"
            );
            tasks.shutdown().await;
        }
    }
}

/// How long process shutdown waits for in-flight ticks. The `backend` compose
/// service has no `stop_grace_period`, so Docker's 10s default applies.
pub const SHUTDOWN_DRAIN: Duration = Duration::from_secs(8);

static JOBS: LazyLock<JobRunner> = LazyLock::new(JobRunner::new);

/// The process-wide runner.
pub fn jobs() -> &'static JobRunner {
    &JOBS
}

/// Shut down the process-wide runner. See [`JobRunner::shutdown`].
pub async fn shutdown(timeout: Duration) {
    JOBS.shutdown(timeout).await;
}

fn log_join_result(result: Result<&'static str, tokio::task::JoinError>) {
    match result {
        Ok(name) => tracing::debug!(job = name, "background job stopped"),
        // Ticks catch their own panics; this is a panic in the loop itself.
        Err(error) if error.is_panic() => tracing::error!(%error, "background job loop panicked"),
        Err(_) => {}
    }
}

fn jitter_delay(jitter: Duration) -> Duration {
    let max = u64::try_from(jitter.as_millis()).unwrap_or(u64::MAX);
    if max == 0 {
        return Duration::ZERO;
    }
    Duration::from_millis(rand::random::<u64>() % (max.saturating_add(1)))
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload")
}

async fn run_periodic<F, Fut>(
    name: &'static str,
    cancel: CancellationToken,
    every: Every,
    mut work: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let start = tokio::time::Instant::now() + every.first_delay;
    let mut interval = tokio::time::interval_at(start, every.period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = interval.tick() => {}
        }
        let pause = jitter_delay(every.jitter);
        if !pause.is_zero() {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(pause) => {}
            }
        }
        // A cancel that raced with a ready tick must not admit more work.
        if cancel.is_cancelled() {
            return;
        }
        // The factory call happens inside the first poll, so it is covered too.
        let tick = AssertUnwindSafe(async { work().await }).catch_unwind();
        if let Err(panic) = tick.await {
            tracing::error!(
                job = name,
                panic = panic_message(panic.as_ref()),
                "background job tick panicked; resuming at next tick"
            );
        }
        if every.spaced {
            interval.reset();
        }
    }
}

async fn run_supervised<F, Fut>(
    name: &'static str,
    cancel: CancellationToken,
    backoff: Backoff,
    mut run: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut delay = None;
    while !cancel.is_cancelled() {
        let started = tokio::time::Instant::now();
        // The factory call happens inside the first poll, so it is covered too.
        let session = AssertUnwindSafe(async { run().await }).catch_unwind();
        let outcome = tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            outcome = session => outcome,
        };
        let next = backoff.next(delay, started.elapsed());
        delay = Some(next);
        let delay_ms = u64::try_from(next.as_millis()).unwrap_or(u64::MAX);
        match outcome {
            Ok(()) => tracing::debug!(job = name, delay_ms, "supervised job ended; restarting"),
            Err(panic) => tracing::error!(
                job = name,
                panic = panic_message(panic.as_ref()),
                delay_ms,
                "supervised job panicked; restarting"
            ),
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(next) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const FAST: Duration = Duration::from_millis(1);

    async fn wait_until(condition: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition() {
                tokio::time::sleep(FAST).await;
            }
        })
        .await
        .expect("condition not reached");
    }

    #[tokio::test]
    async fn shutdown_cancels_admission_and_awaits_in_flight_tick() {
        let runner = JobRunner::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let (entered_tx, entered) = tokio::sync::oneshot::channel::<()>();
        let entered_tx = Arc::new(Mutex::new(Some(entered_tx)));
        let (c, f) = (calls.clone(), finished.clone());
        runner.periodic("slow", Every::new(FAST), move || {
            let (c, f, entered_tx) = (c.clone(), f.clone(), entered_tx.clone());
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                if let Some(tx) = entered_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                f.store(true, Ordering::SeqCst);
            }
        });
        entered.await.unwrap();
        runner.shutdown(Duration::from_secs(5)).await;
        assert!(
            finished.load(Ordering::SeqCst),
            "in-flight tick was not awaited"
        );
        let after = calls.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            after,
            "tick admitted after shutdown"
        );
        assert_eq!(after, 1);
    }

    #[tokio::test]
    async fn shutdown_aborts_stuck_tick_after_deadline() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let runner = JobRunner::new();
        let dropped = Arc::new(AtomicBool::new(false));
        let entered = Arc::new(AtomicBool::new(false));
        let (d, e) = (dropped.clone(), entered.clone());
        runner.periodic("stuck", Every::new(FAST), move || {
            let guard = Dropped(d.clone());
            let e = e.clone();
            async move {
                let _guard = guard;
                e.store(true, Ordering::SeqCst);
                std::future::pending::<()>().await;
            }
        });
        wait_until(|| entered.load(Ordering::SeqCst)).await;
        tokio::time::timeout(
            Duration::from_secs(2),
            runner.shutdown(Duration::from_millis(50)),
        )
        .await
        .expect("shutdown must be bounded");
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn panicking_tick_is_contained_and_job_resumes() {
        let runner = JobRunner::new();
        let panics = Arc::new(AtomicUsize::new(0));
        let healthy = Arc::new(AtomicUsize::new(0));
        let p = panics.clone();
        runner.periodic("panics", Every::new(FAST), move || {
            let p = p.clone();
            async move {
                p.fetch_add(1, Ordering::SeqCst);
                panic!("injected tick failure");
            }
        });
        let h = healthy.clone();
        runner.periodic("healthy", Every::new(FAST), move || {
            h.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        // The panicking job keeps ticking and does not take the other one down.
        wait_until(|| panics.load(Ordering::SeqCst) >= 3 && healthy.load(Ordering::SeqCst) >= 3)
            .await;
        runner.shutdown(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn job_handle_stops_only_its_job() {
        let runner = JobRunner::new();
        let stopped = Arc::new(AtomicUsize::new(0));
        let running = Arc::new(AtomicUsize::new(0));
        let s = stopped.clone();
        let handle = runner.periodic("stopped", Every::new(FAST), move || {
            s.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        let r = running.clone();
        runner.periodic("running", Every::new(FAST), move || {
            r.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        wait_until(|| stopped.load(Ordering::SeqCst) >= 1).await;
        handle.cancel();
        tokio::time::sleep(Duration::from_millis(10)).await;
        let frozen = stopped.load(Ordering::SeqCst);
        let before = running.load(Ordering::SeqCst);
        wait_until(|| running.load(Ordering::SeqCst) > before + 2).await;
        assert_eq!(stopped.load(Ordering::SeqCst), frozen);
        runner.shutdown(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn registration_after_shutdown_is_refused() {
        let runner = JobRunner::new();
        runner.shutdown(Duration::from_secs(1)).await;
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let handle = runner.periodic("late", Every::new(FAST), move || {
            c.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        assert!(handle.is_cancelled());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn spaced_job_pauses_a_full_period_after_a_long_tick() {
        let runner = JobRunner::new();
        let starts = Arc::new(Mutex::new(Vec::new()));
        let s = starts.clone();
        runner.periodic(
            "spaced",
            Every::new(Duration::from_millis(40)).spaced(),
            move || {
                s.lock().unwrap().push(tokio::time::Instant::now());
                tokio::time::sleep(Duration::from_millis(80))
            },
        );
        wait_until(|| starts.lock().unwrap().len() >= 3).await;
        runner.shutdown(Duration::from_secs(1)).await;
        let starts = starts.lock().unwrap();
        for pair in starts.windows(2) {
            // Tick length plus a full period; an unspaced interval would
            // start the next tick as soon as the long one returned.
            assert!(pair[1] - pair[0] >= Duration::from_millis(120));
        }
    }

    #[tokio::test]
    async fn first_delay_and_jitter_hold_the_first_tick() {
        let runner = JobRunner::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        runner.periodic(
            "delayed",
            Every::new(FAST)
                .after(Duration::from_secs(3600))
                .jitter(Duration::from_millis(5)),
            move || {
                c.fetch_add(1, Ordering::SeqCst);
                std::future::ready(())
            },
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        runner.shutdown(Duration::from_secs(1)).await;
        assert!(jitter_delay(Duration::from_millis(5)) <= Duration::from_millis(5));
        assert_eq!(jitter_delay(Duration::ZERO), Duration::ZERO);
    }

    struct DropFlag(Arc<AtomicBool>);
    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn supervised_run_is_dropped_on_cancel_and_awaited_by_shutdown() {
        let runner = JobRunner::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let (dropped_a, dropped_b) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let (r, d) = (runs.clone(), dropped_a.clone());
        let handle = runner.supervised("a", Backoff::new(FAST, FAST), move || {
            r.fetch_add(1, Ordering::SeqCst);
            let guard = DropFlag(d.clone());
            async move {
                let _guard = guard;
                std::future::pending::<()>().await;
            }
        });
        let d = dropped_b.clone();
        runner.supervised("b", Backoff::new(FAST, FAST), move || {
            let guard = DropFlag(d.clone());
            async move {
                let _guard = guard;
                std::future::pending::<()>().await;
            }
        });
        wait_until(|| runs.load(Ordering::SeqCst) == 1).await;
        handle.cancel();
        wait_until(|| dropped_a.load(Ordering::SeqCst)).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(runs.load(Ordering::SeqCst), 1, "cancelled job restarted");
        assert!(
            !dropped_b.load(Ordering::SeqCst),
            "cancel leaked to sibling"
        );
        tokio::time::timeout(
            Duration::from_secs(2),
            runner.shutdown(Duration::from_secs(5)),
        )
        .await
        .expect("a pending supervised run must not hold shutdown to its deadline");
        assert!(dropped_b.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn supervised_restarts_after_ending_with_growing_backoff() {
        let runner = JobRunner::new();
        let starts = Arc::new(Mutex::new(Vec::new()));
        let s = starts.clone();
        runner.supervised(
            "flaky",
            Backoff::new(Duration::from_millis(20), Duration::from_millis(80)),
            move || {
                s.lock().unwrap().push(tokio::time::Instant::now());
                std::future::ready(())
            },
        );
        wait_until(|| starts.lock().unwrap().len() >= 5).await;
        runner.shutdown(Duration::from_secs(1)).await;
        let starts = starts.lock().unwrap();
        let expected = [20, 40, 80, 80];
        for (pair, floor) in starts.windows(2).zip(expected) {
            assert!(
                pair[1] - pair[0] >= Duration::from_millis(floor),
                "restart came before its {floor}ms backoff"
            );
        }
    }

    #[test]
    fn backoff_doubles_to_ceiling_and_resets_after_a_healthy_run() {
        let backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(30));
        let short = Duration::from_millis(10);
        assert_eq!(backoff.next(None, short), Duration::from_secs(1));
        assert_eq!(
            backoff.next(Some(Duration::from_secs(1)), short),
            Duration::from_secs(2)
        );
        assert_eq!(
            backoff.next(Some(Duration::from_secs(20)), short),
            Duration::from_secs(30)
        );
        assert_eq!(
            backoff.next(Some(Duration::from_secs(30)), Duration::from_secs(30)),
            Duration::from_secs(1)
        );
    }

    #[tokio::test]
    async fn supervised_panic_is_contained_and_restarted() {
        let runner = JobRunner::new();
        let runs = Arc::new(AtomicUsize::new(0));
        let healthy = Arc::new(AtomicUsize::new(0));
        let r = runs.clone();
        runner.supervised("panics", Backoff::new(FAST, FAST), move || {
            let attempt = r.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt < 2 {
                    panic!("injected supervised failure");
                }
                std::future::pending::<()>().await;
            }
        });
        let h = healthy.clone();
        runner.periodic("healthy", Every::new(FAST), move || {
            h.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        });
        wait_until(|| runs.load(Ordering::SeqCst) >= 3 && healthy.load(Ordering::SeqCst) >= 3)
            .await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(runs.load(Ordering::SeqCst), 3, "a live run was restarted");
        runner.shutdown(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn supervised_registration_after_shutdown_is_refused() {
        let runner = JobRunner::new();
        runner.shutdown(Duration::from_secs(1)).await;
        let runs = Arc::new(AtomicUsize::new(0));
        let r = runs.clone();
        let handle = runner.supervised("late", Backoff::new(FAST, FAST), move || {
            r.fetch_add(1, Ordering::SeqCst);
            std::future::pending::<()>()
        });
        assert!(handle.is_cancelled());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn one_shots_are_bounded_reaped_and_drained() {
        let runner = JobRunner::new();
        let (release, wait) = tokio::sync::oneshot::channel();
        let (done, finished) = tokio::sync::oneshot::channel();
        assert!(runner.try_once("first", 1, async move {
            let _ = wait.await;
            let _ = done.send(());
        }));
        assert!(!runner.try_once("overflow", 1, async {
            panic!("must not run");
        }));
        release.send(()).unwrap();
        finished.await.unwrap();
        // JoinSet publishes completion immediately after the future returns.
        tokio::task::yield_now().await;
        let (release, wait) = tokio::sync::oneshot::channel();
        let (done, finished) = tokio::sync::oneshot::channel();
        assert!(runner.try_once("replacement", 1, async move {
            let _ = wait.await;
            let _ = done.send(());
        }));
        runner.stop_admission();
        assert!(!runner.try_once("late", 2, async {
            panic!("must not run");
        }));
        release.send(()).unwrap();
        runner.shutdown(Duration::from_secs(1)).await;
        finished.await.unwrap();
    }

    #[tokio::test]
    async fn one_shot_panic_is_contained_and_shutdown_aborts_stuck_work() {
        let runner = JobRunner::new();
        assert!(runner.try_once("panics", 2, async {
            panic!("injected");
        }));
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(dropped.clone());
        assert!(runner.try_once("stuck", 2, async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        }));
        tokio::time::timeout(
            Duration::from_secs(1),
            runner.shutdown(Duration::from_millis(10)),
        )
        .await
        .expect("bounded shutdown");
        assert!(dropped.load(Ordering::SeqCst));
        assert!(!runner.try_once("after shutdown", 2, async {}));
    }

    /// Spawns that are allowed to hold a long-lived `loop` / interval outside
    /// the runner, per file, with why. Counts are exact: migrating one onto the
    /// runner means removing it here too.
    const LOOP_SPAWN_ALLOWLIST: &[(&str, usize, &str)] = &[
        (
            "src/federation/worker.rs",
            1,
            "config refresh deliberately fails the worker: stale policy must stop claims",
        ),
        (
            "src/persona/worker.rs",
            1,
            "config refresh deliberately fails the worker: stale policy must stop ticks",
        ),
        (
            "src/api/agent/notifications.rs",
            1,
            "per-SSE-connection forwarder; ends when the client disconnects",
        ),
        (
            "src/federation/delivery/queue.rs",
            1,
            "per-row lease heartbeat; aborted when that delivery finishes",
        ),
        (
            "src/services/agent/work_loop/store.rs",
            1,
            "per-task lease heartbeat; aborted when the lease drops",
        ),
        (
            "src/services/bot_supervisor.rs",
            1,
            "credential watcher scoped to one supervised bot session (AbortTask)",
        ),
        (
            "src/services/feishu_ws.rs",
            1,
            "per-connection ping; aborted with the connection (AbortTask)",
        ),
    ];

    fn rust_sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_sources(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// Spawn sites in `source` whose body starts a `loop` or an interval
    /// within a few lines. Test modules and test-only files are not scanned.
    fn loop_spawns(source: &str) -> Vec<usize> {
        let spawn =
            regex::Regex::new(r"(tokio::spawn|task::spawn|task::spawn_blocking|thread::spawn)\(")
                .unwrap();
        let next_spawn = regex::Regex::new(r"spawn(_blocking)?\(").unwrap();
        let looping = regex::Regex::new(r"\bloop\s*\{|time::interval(_at)?\(").unwrap();
        let test_mod = regex::Regex::new(r"^\s*(pub(\(\w+\))?\s+)?mod \w+").unwrap();
        let mut lines: Vec<&str> = source.lines().collect();
        if let Some(cut) = lines
            .windows(2)
            .position(|pair| pair[0].trim() == "#[cfg(test)]" && test_mod.is_match(pair[1]))
        {
            lines.truncate(cut);
        }
        let mut hits = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("//") || !spawn.is_match(line) {
                continue;
            }
            let body = std::iter::once(*line).chain(
                lines[index + 1..]
                    .iter()
                    .take(11)
                    .copied()
                    .take_while(|next| !next_spawn.is_match(next)),
            );
            if body.into_iter().any(|l| looping.is_match(l)) {
                hits.push(index + 1);
            }
        }
        hits
    }

    #[test]
    fn scanner_flags_inline_loops_and_skips_test_modules() {
        let source = "fn a() {\n    tokio::spawn(async move {\n        loop {\n        }\n    });\n}\n#[cfg(test)]\nmod tests {\n    fn b() { tokio::spawn(async { loop {} }); }\n}\n";
        assert_eq!(loop_spawns(source), vec![2]);
        assert!(loop_spawns("tokio::spawn(async move { work().await });\n").is_empty());
    }

    /// Long-lived background loops belong on the runner, which owns their
    /// shutdown and contains their panics. A new bare `spawn` + `loop` must
    /// either move onto [`JobRunner::periodic`] / [`JobRunner::supervised`] or be
    /// listed above with a reason.
    #[test]
    fn long_lived_loops_are_not_spawned_outside_the_runner() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        rust_sources(&root.join("src"), &mut files);
        let mut found = std::collections::BTreeMap::new();
        for path in files {
            let name = path.file_name().unwrap().to_string_lossy();
            if name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let hits = loop_spawns(&source);
            if !hits.is_empty() {
                let relative = path.strip_prefix(root).unwrap().to_string_lossy();
                found.insert(relative.replace('\\', "/"), hits);
            }
        }
        let mut problems = Vec::new();
        for (file, hits) in &found {
            let allowed = LOOP_SPAWN_ALLOWLIST
                .iter()
                .find(|(path, _, _)| path == file)
                .map_or(0, |(_, count, _)| *count);
            if hits.len() != allowed {
                problems.push(format!(
                    "{file}: loop spawns at lines {hits:?}, allowlisted {allowed}"
                ));
            }
        }
        for (file, _, _) in LOOP_SPAWN_ALLOWLIST {
            if !found.contains_key(*file) {
                problems.push(format!(
                    "{file}: allowlisted but no loop spawn left; drop the entry"
                ));
            }
        }
        assert!(
            problems.is_empty(),
            "register long-lived loops on services::jobs::JobRunner (or allowlist with a reason):\n{}",
            problems.join("\n")
        );
    }
}
