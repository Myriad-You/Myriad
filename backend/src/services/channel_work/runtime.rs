//! Admission and run observation have separate lifetimes. Network delivery never owns the chat lock.
use super::*;
use std::sync::Weak;

static LOCKS: Lazy<Mutex<HashMap<String, Weak<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static ACTIVE: Lazy<Mutex<HashMap<String, ActiveWork>>> = Lazy::new(|| Mutex::new(HashMap::new()));

struct ActiveWork {
    run: Option<Arc<AgentRun>>,
    binding: ChannelBinding,
    observer: tokio::task::AbortHandle,
}

pub(super) async fn with_chat_lock<T>(
    key: &str,
    future: impl std::future::Future<Output = T>,
) -> T {
    let lock = {
        let mut locks = LOCKS.lock().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        match locks.get(key).and_then(Weak::upgrade) {
            Some(lock) => lock,
            None => {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(key.to_string(), Arc::downgrade(&lock));
                lock
            }
        }
    };
    let _guard = lock.lock().await;
    future.await
}

pub(super) async fn is_active(key: &str) -> bool {
    ACTIVE.lock().await.contains_key(key)
}
pub(super) async fn owns_run(key: &str, run_id: &str) -> bool {
    ACTIVE.lock().await.get(key).is_some_and(|active| {
        active
            .run
            .as_ref()
            .is_some_and(|run| run.run_id() == run_id)
    })
}

pub(super) async fn stop_delivery(key: &str) {
    let active = ACTIVE.lock().await.remove(key);
    if let Some(active) = active {
        active.observer.abort();
        if let Some(run) = active.run {
            run.abort_execution().await;
        }
    }
}

pub(super) async fn start_delivery(
    run: Arc<AgentRun>,
    db: DatabaseConnection,
    user_id: i32,
    key: String,
    sink: ChannelSink,
    input: String,
) {
    let mut stored = load_session(&db, sink.platform(), &key)
        .await
        .unwrap_or(StoredSession {
            session_id: String::new(),
            last_run_id: None,
            last_event_seq: 0,
            original_input: String::new(),
            binding: None,
            address: None,
        });
    if let Some(id) = run.session_id() {
        stored.session_id = id.to_string();
    }
    if stored.last_run_id.as_deref() != Some(run.run_id()) {
        stored.last_event_seq = 0;
    }
    stored.last_run_id = Some(run.run_id().to_string());
    stored.original_input = input.clone();
    stored.binding = Some(sink.binding.clone());
    stored.address = Some(sink.transport.address());
    if let Err(error) = put_session(&db, sink.platform(), user_id, &key, stored).await {
        run.abort_execution().await;
        cancel_session_tasks(&db, user_id, run.session_id().unwrap_or("")).await;
        warn!(%error, "cannot persist channel run attachment");
        return;
    }
    let (start, ready) = tokio::sync::oneshot::channel();
    let observer_key = key.clone();
    let observer_run = run.clone();
    let binding = sink.binding.clone();
    let observer = tokio::spawn(async move {
        let _ = ready.await;
        // Retained outbox always goes first, without rerunning Work.
        flush_outbound(&db, user_id, &observer_key, &sink).await;
        deliver_run(
            observer_run.clone(),
            db.clone(),
            user_id,
            observer_key.clone(),
            sink.clone(),
            input,
        )
        .await;
        if !sink.authorized().await {
            observer_run.abort_execution().await;
            cancel_session_tasks(&db, user_id, observer_run.session_id().unwrap_or("")).await;
        }
        let mut active = ACTIVE.lock().await;
        if active.get(&observer_key).is_some_and(|entry| {
            entry
                .run
                .as_ref()
                .is_some_and(|run| run.run_id() == observer_run.run_id())
        }) {
            active.remove(&observer_key);
        }
    });
    ACTIVE.lock().await.insert(
        key,
        ActiveWork {
            run: Some(run),
            binding,
            observer: observer.abort_handle(),
        },
    );
    let _ = start.send(());
}

pub(crate) async fn revoke_pairing(db: &DatabaseConnection, provider: &str, user_id: i32) {
    let platform = if provider == "discord_dm" {
        "discord"
    } else {
        provider
    };
    let candidates: Vec<_> = ACTIVE
        .lock()
        .await
        .iter()
        .filter(|(_, work)| work.binding.user_id == user_id && work.binding.provider == provider)
        .map(|(key, work)| (key.clone(), work.binding.clone()))
        .collect();
    for (key, binding) in candidates {
        if !binding.is_current(db).await {
            stop_delivery(&key).await;
        }
    }
    if let Ok(rows) = shared_registry::list(db, session_ns(platform), Some(user_id), None).await {
        for row in rows {
            if let Ok(session) = serde_json::from_value::<StoredSession>(row.payload) {
                // A new pairing may already exist after the revocation transaction committed.
                // Never sweep that generation's records or tasks.
                if let Some(binding) = &session.binding {
                    if binding.is_current(db).await {
                        continue;
                    }
                }
                destroy_session(db, platform, user_id, &row.record_id, &session).await;
            }
        }
    }
}

async fn destroy_session(
    db: &DatabaseConnection,
    platform: &str,
    user_id: i32,
    key: &str,
    session: &StoredSession,
) {
    with_chat_lock(key, async {
        stop_delivery(key).await;
        if let Some(run_id) = &session.last_run_id {
            if let Some(run) = crate::services::agent::run_hub::get_run_for_user(run_id, user_id).await {
                run.abort_execution().await;
            }
        }
        cancel_session_tasks(db, user_id, &session.session_id).await;
        if let Err(error) = db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "DELETE FROM tapp_runtime_registry WHERE subject_id = $1 AND \
             ((namespace IN ($2, $3, $4) AND record_id = $5) OR (namespace = $6 AND starts_with(record_id, $5 || ':')))",
            [user_id.into(), session_ns(platform).into(), pending_ns(platform).into(), outbound_ns(platform).into(), key.into(), inbound_ns(platform).into()])).await {
            warn!(%error, "revoked channel projection cleanup failed");
        }
    }).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn accepted_work_does_not_hold_admission_lock() {
        let (finish, done) = tokio::sync::oneshot::channel::<()>();
        let task = with_chat_lock("admission-test", async {
            tokio::spawn(async {
                let _ = done.await;
            })
        })
        .await;
        tokio::time::timeout(
            Duration::from_millis(100),
            with_chat_lock("admission-test", async {}),
        )
        .await
        .unwrap();
        finish.send(()).unwrap();
        task.await.unwrap();
    }
}

/// Reattach only to an existing owned run; recovery never calls start_process_run.
/// Caller owns the short chat admission lock.
pub(super) async fn recover_session(
    db: &DatabaseConnection,
    key: &str,
    provided: Option<ChannelSink>,
) -> bool {
    if is_active(key).await {
        return false;
    }
    let platform = match provided.as_ref() {
        Some(sink) => sink.platform(),
        None => return false,
    };
    let Some(session) = load_session(db, platform, key).await else {
        return false;
    };
    let (Some(binding), Some(run_id), Some(sink)) =
        (session.binding, session.last_run_id, provided)
    else {
        return false;
    };
    if !binding.is_current(db).await {
        return false;
    }
    let outbox = shared_registry::list(db, outbound_ns(platform), Some(binding.user_id), None)
        .await
        .is_ok_and(|rows| rows.iter().any(|row| row.record_id == key));
    let Some(run) =
        crate::services::agent::run_hub::get_run_for_user(&run_id, binding.user_id).await
    else {
        if outbox {
            start_outbox_delivery(db.clone(), key.to_string(), sink).await;
        }
        return outbox;
    };
    let (events, _, _) = run.snapshot().await;
    let unseen = events.iter().any(|event| {
        event.sequence > session.last_event_seq
            && map_progress(&event.event, &session.original_input, &sink.capabilities()).is_some()
    });
    if !unseen && !outbox && !run.is_executing().await {
        return false;
    }
    start_delivery(
        run,
        db.clone(),
        binding.user_id,
        key.to_string(),
        sink,
        session.original_input,
    )
    .await;
    true
}

// An already projected result remains deliverable after run history expires.
async fn start_outbox_delivery(db: DatabaseConnection, key: String, sink: ChannelSink) {
    let (start, ready) = tokio::sync::oneshot::channel();
    let binding = sink.binding.clone();
    let task_key = key.clone();
    let observer = tokio::spawn(async move {
        let _ = ready.await;
        flush_outbound(&db, sink.binding.user_id, &task_key, &sink).await;
        ACTIVE.lock().await.remove(&task_key);
    });
    ACTIVE.lock().await.insert(
        key,
        ActiveWork {
            run: None,
            binding,
            observer: observer.abort_handle(),
        },
    );
    let _ = start.send(());
}

pub(crate) fn spawn_recovery_worker() {
    tokio::spawn(async {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tick.tick().await;
            let Ok(db) = shared_registry::database().await else {
                continue;
            };
            for platform in ["qq", "telegram", "discord", "feishu"] {
                let Ok(rows) = shared_registry::list(&db, session_ns(platform), None, None).await
                else {
                    continue;
                };
                for row in rows {
                    if is_active(&row.record_id).await {
                        continue;
                    }
                    let Ok(session) = serde_json::from_value::<StoredSession>(row.payload) else {
                        continue;
                    };
                    let (Some(binding), Some(address)) =
                        (session.binding.clone(), session.address.clone())
                    else {
                        continue;
                    };
                    if !binding.is_current(&db).await {
                        destroy_session(&db, platform, binding.user_id, &row.record_id, &session)
                            .await;
                        continue;
                    }
                    let Some(transport) = address.connect(&db).await else {
                        continue;
                    };
                    let sink = ChannelSink {
                        transport,
                        binding,
                        db: db.clone(),
                    };
                    with_chat_lock(
                        &row.record_id,
                        recover_session(&db, &row.record_id, Some(sink)),
                    )
                    .await;
                }
            }
        }
    });
}
