//! Fixed driver count, serial ticks and a bounded shutdown drain.
use futures::{StreamExt, stream};
use std::{future::Future, time::Duration};
use tokio::{sync::watch, task::JoinSet};

pub(super) struct Drivers {
    stop: watch::Sender<bool>,
    tasks: JoinSet<&'static str>,
}

impl Drivers {
    pub fn new() -> Self {
        Self {
            stop: watch::channel(false).0,
            tasks: JoinSet::new(),
        }
    }

    pub fn stopped(&self) -> watch::Receiver<bool> {
        self.stop.subscribe()
    }

    pub fn periodic<F, Fut>(
        &mut self,
        name: &'static str,
        every: Duration,
        delay: Duration,
        work: F,
    ) where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        // How often each ticks, for what shows whether she is living.
        crate::services::agent::alive::ticks_every(name, every);
        let stop = self.stopped();
        self.tasks.spawn(async move {
            periodic(stop, every, delay, work).await;
            name
        });
    }

    /// Long-running channel connections stop admission immediately on shutdown.
    pub fn continuous<F>(&mut self, name: &'static str, work: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut stopped = self.stopped();
        self.tasks.spawn(async move {
            if !*stopped.borrow() {
                tokio::select! {
                    biased;
                    _ = stopped.changed() => {},
                    _ = work => {},
                }
            }
            name
        });
    }

    pub fn supervise(mut self, failure: watch::Sender<Option<String>>, drain: Duration) -> Runtime {
        let stop = self.stop.clone();
        let mut stopped = self.stopped();
        let join = tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = stopped.changed() => {},
                finished = self.tasks.join_next() => {
                    let error = format!("persona background driver stopped unexpectedly: {finished:?}");
                    tracing::error!(%error);
                    failure.send_replace(Some(error));
                },
            }
            self.stop.send_replace(true);
            let draining = async {
                while let Some(result) = self.tasks.join_next().await {
                    if let Err(error) = result {
                        failure.send_replace(Some(format!(
                            "persona driver failed during shutdown: {error}"
                        )));
                    }
                }
            };
            if tokio::time::timeout(drain, draining).await.is_err() {
                tracing::warn!(
                    remaining = self.tasks.len(),
                    "Persona driver drain deadline reached"
                );
                self.tasks.shutdown().await;
            }
        });
        Runtime { stop, join }
    }
}

pub(super) struct Runtime {
    stop: watch::Sender<bool>,
    join: tokio::task::JoinHandle<()>,
}

impl Runtime {
    pub fn request_stop(&self) {
        self.stop.send_replace(true);
    }
    pub async fn shutdown(mut self) {
        self.stop.send_replace(true);
        if let Err(error) = (&mut self.join).await {
            tracing::error!(%error, "Persona supervisor failed during shutdown");
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Also stop admission when startup/router construction fails. The
        // supervisor continues its bounded drain even if nobody awaits it.
        self.stop.send_replace(true);
    }
}

async fn periodic<F, Fut>(
    mut stopped: watch::Receiver<bool>,
    every: Duration,
    delay: Duration,
    mut work: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + delay, every);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *stopped.borrow() {
            return;
        }
        tokio::select! {
            biased;
            _ = stopped.changed() => return,
            _ = interval.tick() => {},
        }
        // A stop that raced with a ready tick must not admit more work.
        if *stopped.borrow() {
            return;
        }
        // Await in the driver: slow work cannot create overlapping ticks or an
        // unbounded task queue. Other drivers keep their independent intervals.
        work().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn continuous_driver_stops_admission_without_waiting_for_drain() {
        let mut drivers = Drivers::new();
        let (started, entered) = tokio::sync::oneshot::channel();
        let (dropped, released) = tokio::sync::oneshot::channel::<()>();
        drivers.continuous("channel", async move {
            let _owned = dropped;
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        let (failure, _) = watch::channel(None);
        let runtime = drivers.supervise(failure.clone(), Duration::from_secs(30));
        entered.await.unwrap();
        runtime.request_stop();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), released)
                .await
                .unwrap()
                .is_err()
        );
        runtime.shutdown().await;
        assert!(failure.borrow().is_none());
    }

    #[tokio::test]
    async fn blocked_tick_does_not_overlap_or_hold_another_driver() {
        let mut drivers = Drivers::new();
        let started = Arc::new(AtomicUsize::new(0));
        let other = Arc::new(AtomicUsize::new(0));
        let blocked = Arc::new(tokio::sync::Semaphore::new(0));
        let calls = started.clone();
        let release = blocked.clone();
        drivers.periodic(
            "slow",
            Duration::from_millis(1),
            Duration::ZERO,
            move || {
                let calls = calls.clone();
                let release = release.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    release.acquire().await.unwrap().forget();
                }
            },
        );
        let calls = other.clone();
        drivers.periodic(
            "fast",
            Duration::from_millis(1),
            Duration::ZERO,
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                std::future::ready(())
            },
        );
        let (failure, _) = watch::channel(None);
        let runtime = drivers.supervise(failure.clone(), Duration::from_secs(1));
        tokio::time::timeout(Duration::from_secs(2), async {
            while other.load(Ordering::SeqCst) < 5 || started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 1);
        runtime.stop.send_replace(true);
        blocked.add_permits(1);
        runtime.shutdown().await;
        assert_eq!(started.load(Ordering::SeqCst), 1);
        assert!(failure.borrow().is_none());
    }

    #[tokio::test]
    async fn panic_reports_failure_and_stops_other_drivers() {
        let mut drivers = Drivers::new();
        drivers.periodic("panic", Duration::from_secs(1), Duration::ZERO, || async {
            panic!("injected driver failure");
        });
        drivers.periodic(
            "idle",
            Duration::from_secs(1),
            Duration::from_secs(3600),
            || async {},
        );
        let (failure, mut failures) = watch::channel(None);
        let runtime = drivers.supervise(failure, Duration::from_secs(1));
        tokio::time::timeout(Duration::from_secs(2), failures.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(
            failures
                .borrow()
                .as_ref()
                .unwrap()
                .contains("stopped unexpectedly")
        );
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_cancels_stuck_work_after_drain_deadline() {
        struct Active(Arc<AtomicUsize>);
        impl Drop for Active {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let active = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let mut drivers = Drivers::new();
        let calls = active.clone();
        let notice = entered.clone();
        drivers.periodic(
            "stuck",
            Duration::from_millis(1),
            Duration::ZERO,
            move || {
                let calls = calls.clone();
                let notice = notice.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let _active = Active(calls);
                    notice.notify_one();
                    std::future::pending::<()>().await;
                }
            },
        );
        let (failure, _) = watch::channel(None);
        let runtime = drivers.supervise(failure, Duration::from_millis(5));
        tokio::time::timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), runtime.shutdown())
            .await
            .unwrap();
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }
}

pub(super) async fn run_batch<T, F, Fut>(items: Vec<T>, stopped: watch::Receiver<bool>, execute: F)
where
    F: Fn(T) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    stream::iter(items)
        .for_each_concurrent(2, |task| {
            let accepting = !*stopped.borrow();
            let execute = &execute;
            async move {
                if accepting {
                    execute(task).await;
                }
            }
        })
        .await;
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn batch_bounds_execution_and_stop_discards_only_unstarted_work() {
        let started = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let (stop, stopped) = watch::channel(false);
        let calls = started.clone();
        let completed = finished.clone();
        let permits = release.clone();
        let batch = tokio::spawn(run_batch(vec![1, 2, 3, 4, 5], stopped, move |_| {
            let calls = calls.clone();
            let completed = completed.clone();
            let permits = permits.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                permits.acquire().await.unwrap().forget();
                completed.fetch_add(1, Ordering::SeqCst);
            }
        }));
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while started.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert_eq!(finished.load(Ordering::SeqCst), 0);
        stop.send_replace(true);
        release.add_permits(2);
        tokio::time::timeout(std::time::Duration::from_secs(2), batch)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(started.load(Ordering::SeqCst), 2);
        assert_eq!(finished.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn batch_keeps_all_reserved_jobs_until_completed() {
        let calls = AtomicUsize::new(0);
        let (_stop, stopped) = watch::channel(false);
        run_batch(vec![1, 2, 3, 4, 5], stopped, |_| async {
            tokio::task::yield_now().await;
            calls.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 5);
    }
}
