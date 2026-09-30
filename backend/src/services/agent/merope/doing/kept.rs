//! What she is doing now, kept across a restart while it is still fresh.

use super::*;

/// Where what she is in the middle of is kept, so a restart does not wipe
/// it: a book half read is still half read.
pub const PRESENT_NAMESPACE: &str = "merope_present";

pub(super) const PRESENT: &str = "now";

/// Something that ended while she was not running is still written down if
/// it ended at most this long ago; older, it has gone by.
pub(super) const STILL_FRESH: chrono::Duration = chrono::Duration::hours(2);

pub(super) static RESTORED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct KeptNow {
    pub(super) doing: Option<KeptDoing>,
    pub(super) lazing: Option<KeptLazing>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct KeptDoing {
    pub(super) thing: Thing,
    pub(super) started: DateTime<Utc>,
    pub(super) ends: DateTime<Utc>,
    pub(super) why: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct KeptLazing {
    pub(super) kind: String,
    pub(super) started: DateTime<Utc>,
    pub(super) ends: DateTime<Utc>,
}

pub(super) fn present_identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

/// Keep what she is in the middle of now.
pub(super) async fn keep_now(db: &DatabaseConnection) {
    let kept = LIFE.lock().ok().map(|life| KeptNow {
        doing: life.now.as_ref().map(|doing| KeptDoing {
            thing: doing.thing.clone(),
            started: doing.started,
            ends: doing.ends,
            why: doing.why.clone(),
        }),
        lazing: life.lazing.as_ref().map(|lazing| KeptLazing {
            kind: lazing.kind.to_string(),
            started: lazing.started,
            ends: lazing.ends,
        }),
    });
    let Some(kept) = kept else {
        return;
    };
    let keep_until = (Utc::now() + chrono::Duration::days(2)).timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        db,
        PRESENT_NAMESPACE,
        PRESENT,
        present_identity(),
        &kept,
        keep_until,
    )
    .await
    {
        tracing::warn!(%error, "[Merope] could not keep what she is in the middle of");
    }
}

/// After a restart, once: pick up what she was in the middle of. What is
/// still going goes on; what ended meanwhile is written down if lately,
/// and lazing is counted.
pub(super) async fn restore_now(db: &DatabaseConnection, owner: i32) {
    if RESTORED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let kept =
        match crate::services::runtime_registry::get::<KeptNow>(db, PRESENT_NAMESPACE, PRESENT)
            .await
        {
            Ok(kept) => kept.unwrap_or_default(),
            Err(error) => {
                RESTORED.store(false, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!(%error, "[Merope] could not read what she was in the middle of");
                return;
            }
        };
    let now = Utc::now();
    if let Some(kept) = kept.doing {
        let doing = Doing {
            thing: kept.thing,
            started: kept.started,
            ends: kept.ends,
            why: kept.why,
        };
        if doing.ends > now {
            tracing::info!(kind = %doing.thing.key(), "[Merope] back to what she was in the middle of");
            if let Ok(mut life) = LIFE.lock() {
                life.now = Some(doing);
            }
        } else if now - doing.ends <= STILL_FRESH {
            tracing::info!(kind = %doing.thing.key(), "[Merope] writing down what she finished meanwhile");
            let _ = tokio::time::timeout(Duration::from_secs(120), finish(db, owner, doing)).await;
        }
    }
    if let Some(kept) = kept.lazing {
        let kind = super::super::pace::LAZING
            .iter()
            .find(|(kind, _)| *kind == kept.kind)
            .map_or(super::super::pace::LAZING[0].0, |(kind, _)| kind);
        if kept.ends > now {
            if let Ok(mut life) = LIFE.lock() {
                life.next_at = Some(kept.ends);
                life.lazing = Some(Lazing {
                    kind,
                    started: kept.started,
                    ends: kept.ends,
                });
            }
        } else {
            let minutes = kept.ends.signed_duration_since(kept.started).num_minutes();
            super::super::pace::lazed(db, minutes.max(0) as f64).await;
        }
    }
    keep_now(db).await;
}

/// A new persona is in the middle of nothing.
pub async fn forget_kept<C: sea_orm::ConnectionTrait>(db: &C) -> Result<u64, sea_orm::DbErr> {
    crate::services::runtime_registry::delete_matching(
        db,
        PRESENT_NAMESPACE,
        None,
        None,
        None,
        None,
    )
    .await
}
