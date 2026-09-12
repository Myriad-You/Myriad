//! A durable ordered text/media outbox. Successful items advance independently.
use super::*;
use sea_orm::TransactionTrait;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum DeliveryItem {
    Text(String),
    Image(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredOutbound {
    items: Vec<DeliveryItem>,
    next_index: usize,
    prompt: Option<PendingPrompt>,
}

impl StoredOutbound {
    fn prepare(text: &str, images: &[String], limit: usize, prompt: Option<PendingPrompt>) -> Self {
        let items = split_channel_text(text, limit)
            .into_iter()
            .map(DeliveryItem::Text)
            .chain(images.iter().cloned().map(DeliveryItem::Image))
            .collect();
        Self {
            items,
            next_index: 0,
            prompt,
        }
    }
    fn markup(&self, platform: &str) -> Option<Value> {
        self.prompt
            .as_ref()
            .filter(|_| self.next_index + 1 == self.items.len())
            .and_then(|prompt| match platform {
                "telegram" => telegram_reply_markup(prompt),
                "discord" => discord_reply_markup(prompt),
                "feishu" => feishu_reply_markup(prompt),
                _ => None,
            })
    }
}

pub(super) async fn clear_outbound(db: &DatabaseConnection, platform: &str, key: &str) {
    let _ = shared_registry::take::<StoredOutbound>(db, outbound_ns(platform), key).await;
}

pub(super) async fn flush_outbound(
    db: &DatabaseConnection,
    user_id: i32,
    key: &str,
    sink: &ChannelSink,
) {
    let mut delay = 1;
    loop {
        if !sink.authorized().await {
            return;
        }
        let mut stored =
            match shared_registry::get::<StoredOutbound>(db, outbound_ns(sink.platform()), key)
                .await
            {
                Ok(Some(stored)) => stored,
                Ok(None) => return,
                Err(error) => {
                    warn!(%error, "channel outbox read failed");
                    return;
                }
            };
        let Some(item) = stored.items.get(stored.next_index) else {
            clear_outbound(db, sink.platform(), key).await;
            return;
        };
        let markup = stored.markup(sink.platform());
        // Authorization is rechecked for each network item, including retries.
        let result = match item {
            DeliveryItem::Text(text) => sink.transport.send_text_chunk(text, markup).await,
            DeliveryItem::Image(url) => sink.transport.send_image_chunk(url, markup).await,
        };
        match result {
            Ok(()) => {
                stored.next_index += 1;
                if let Err(error) = shared_registry::put(
                    db,
                    outbound_ns(sink.platform()),
                    key,
                    identity(user_id),
                    &stored,
                    (Utc::now() + ChronoDuration::days(2)).timestamp(),
                )
                .await
                {
                    // Unknown commit after an acknowledged send can duplicate this item; never replay Work.
                    warn!(%error, "channel outbox progress write failed");
                    return;
                }
                delay = 1;
            }
            Err(error) => {
                warn!(%error, retry_seconds = delay, "channel delivery failed; retrying stored item");
                tokio::time::sleep(Duration::from_secs(delay)).await;
                delay = (delay * 2).min(60);
            }
        }
    }
}

async fn project_delivery(
    db: &DatabaseConnection,
    user_id: i32,
    key: &str,
    sink: &ChannelSink,
    run_id: &str,
    sequence: u64,
    content: &str,
    images: &[String],
    prompt: Option<PendingPrompt>,
) -> Result<bool, DbErr> {
    with_chat_lock(key, async {
        if !owns_run(key, run_id).await || !sink.authorized().await {
            return Ok(false);
        }
        let Some(mut session) = load_session(db, sink.platform(), key).await else {
            return Ok(false);
        };
        session.last_run_id = Some(run_id.to_string());
        session.last_event_seq = sequence;
        let outbox = StoredOutbound::prepare(content, images, sink.text_limit(), prompt.clone());
        let txn = db.begin().await?;
        let expiry = (Utc::now() + ChronoDuration::days(2)).timestamp();
        shared_registry::put(
            &txn,
            outbound_ns(sink.platform()),
            key,
            identity(user_id),
            &outbox,
            expiry,
        )
        .await?;
        if let Some(prompt) = prompt {
            let pending = StoredPending {
                prompt,
                last_event_seq: sequence,
                expected_user_id: Some(user_id),
            };
            shared_registry::put(
                &txn,
                pending_ns(sink.platform()),
                key,
                identity(user_id),
                &pending,
                expiry,
            )
            .await?;
        } else {
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND record_id = $2",
                [pending_ns(sink.platform()).into(), key.into()],
            ))
            .await?;
        }
        shared_registry::put(
            &txn,
            session_ns(sink.platform()),
            key,
            identity(user_id),
            &session,
            (Utc::now() + ChronoDuration::seconds(BINDING_TTL_SECS)).timestamp(),
        )
        .await?;
        txn.commit().await?;
        Ok(true)
    })
    .await
}

pub(super) async fn deliver_run(
    run: Arc<AgentRun>,
    db: DatabaseConnection,
    user_id: i32,
    key: String,
    sink: ChannelSink,
    input: String,
) {
    let run_id = run.run_id().to_string();
    let after = load_session(&db, sink.platform(), &key)
        .await
        .filter(|session| session.last_run_id.as_deref() == Some(&run_id))
        .map(|session| session.last_event_seq);
    let mut envelopes = Box::pin(crate::api::agent::agent_run_envelopes(run));
    let mut refresh = tokio::time::interval(TYPING_REFRESH);
    loop {
        tokio::select! {
            event = futures::StreamExt::next(&mut envelopes) => {
                let Some(envelope) = event else { return; };
                if !should_deliver_sequence(after, envelope.sequence) { continue; }
                let Some((event, prompt)) = map_progress(&envelope.event, &input, &sink.capabilities()) else { continue; };
                let (content, images) = match plan_delivery(&event, &sink.delivery_context()) {
                    DeliveryPlan::ActiveText { content, image_urls } | DeliveryPlan::PassiveText { content, image_urls, .. } => (content, image_urls),
                    DeliveryPlan::FailVisible { content, .. } => (content, Vec::new()),
                    DeliveryPlan::Drop => continue,
                };
                if matches!(event, ChannelEvent::TaskStarted { .. }) {
                    let _ = sink.send_text(&content).await;
                    continue;
                }
                match project_delivery(&db, user_id, &key, &sink, &run_id, envelope.sequence, &content, &images, prompt).await {
                    Ok(true) => flush_outbound(&db, user_id, &key, &sink).await,
                    Ok(false) => {},
                    Err(error) => warn!(%error, "channel projection failed; run cursor remains replayable"),
                }
                return;
            }
            _ = refresh.tick() => {
                if !sink.authorized().await { return; }
                sink.send_typing().await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_only_and_partial_delivery_survive_serialization() {
        let image_only = StoredOutbound::prepare("", &["/image.png".into()], 10, None);
        assert_eq!(
            image_only.items,
            vec![DeliveryItem::Image("/image.png".into())]
        );
        let mut outbox = StoredOutbound::prepare("hello", &["/image.png".into()], 10, None);
        outbox.next_index = 1;
        let restored: StoredOutbound =
            serde_json::from_value(serde_json::to_value(&outbox).unwrap()).unwrap();
        assert_eq!(
            restored.items.get(restored.next_index),
            Some(&DeliveryItem::Image("/image.png".into()))
        );
    }
}
