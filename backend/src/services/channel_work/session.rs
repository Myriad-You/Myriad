//! Where a chat's Work session and its pending prompt are kept between messages.

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredSession {
    pub(super) session_id: String,
    #[serde(default)]
    pub(super) last_run_id: Option<String>,
    #[serde(default)]
    pub(super) last_event_seq: u64,
    #[serde(default)]
    pub(super) original_input: String,
    #[serde(default)]
    pub(super) binding: Option<ChannelBinding>,
    #[serde(default)]
    pub(super) address: Option<transport::ChannelAddress>,
    /// Her chat with them, apart from the Work she hands off.
    #[serde(default)]
    pub(super) chat_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredPending {
    pub(super) prompt: PendingPrompt,
    #[serde(default)]
    pub(super) last_event_seq: u64,
    #[serde(default)]
    pub(super) expected_user_id: Option<i32>,
}

pub(super) async fn bind_session(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    session_key: &str,
) -> Result<StoredSession, DbErr> {
    if let Some(stored) =
        shared_registry::get::<StoredSession>(db, platform.session_ns(), session_key).await?
    {
        if !stored.session_id.is_empty() {
            return Ok(stored);
        }
    }
    let session_id = crate::services::agent::sessions::ensure_session(
        db,
        None,
        user_id,
        AgentInteractionMode::Work,
    )
    .await
    .map_err(DbErr::Custom)?;
    let stored = StoredSession {
        session_id,
        last_run_id: None,
        last_event_seq: 0,
        original_input: String::new(),
        binding: None,
        address: None,
        chat_session_id: None,
    };
    put_session(db, platform, user_id, session_key, stored.clone()).await?;
    Ok(stored)
}

pub(super) async fn put_session(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    session_key: &str,
    stored: StoredSession,
) -> Result<(), DbErr> {
    shared_registry::put(
        db,
        platform.session_ns(),
        session_key,
        identity(user_id),
        &stored,
        (Utc::now() + ChronoDuration::seconds(BINDING_TTL_SECS)).timestamp(),
    )
    .await
}

pub(super) async fn load_session(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    session_key: &str,
) -> Option<StoredSession> {
    shared_registry::get::<StoredSession>(db, platform.session_ns(), session_key)
        .await
        .ok()
        .flatten()
}

pub(super) async fn load_pending(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    session_key: &str,
) -> Option<StoredPending> {
    if session_key.is_empty() {
        return None;
    }
    match shared_registry::get::<StoredPending>(db, platform.pending_ns(), session_key).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "channel pending load failed");
            None
        }
    }
}

pub(super) async fn take_pending_if_id(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    session_key: &str,
    expected_id: &str,
) -> Option<StoredPending> {
    let taken = match shared_registry::take::<StoredPending>(db, platform.pending_ns(), session_key)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "channel pending take failed");
            return None;
        }
    };
    let pending = taken?;
    let mut prompt = pending.prompt.clone();
    ensure_pending_id(&mut prompt);
    if !expected_id.is_empty() && prompt.id != expected_id {
        let _ = shared_registry::put(
            db,
            platform.pending_ns(),
            session_key,
            RegistryIdentity {
                subject_id: pending.expected_user_id,
                owner_id: pending.expected_user_id,
                tapp_id: None,
                runtime_id: None,
            },
            &pending,
            (Utc::now() + ChronoDuration::days(2)).timestamp(),
        )
        .await;
        return None;
    }
    Some(pending)
}

pub(super) async fn clear_pending(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    session_key: &str,
) {
    if session_key.is_empty() {
        return;
    }
    if let Err(error) =
        shared_registry::take::<StoredPending>(db, platform.pending_ns(), session_key).await
    {
        warn!(%error, "channel pending clear failed");
    }
}
