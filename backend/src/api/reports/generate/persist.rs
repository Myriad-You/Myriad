//! Atomic platform-report persist (MYR-020) and bounded fan-out (MYR-021).

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};

use crate::models::entities::platform_reports;

use super::PlatformReport;

/// Max concurrent platform/AI report tasks (MYR-021).
/// Generous enough for multi-platform generate-all (~11 platforms) while
/// preventing unbounded cost amplification from `join_all` fan-out.
pub(crate) const MAX_CONCURRENT_PLATFORM_REPORTS: usize = 6;

/// Atomically replace the stored report for `(user_id, platform)`.
///
/// DELETE + INSERT run in one DB transaction so a failed insert never leaves
/// the platform without its previous report (MYR-020). Serialization happens
/// *before* the transaction begins, so a serialize failure also never deletes.
pub(super) async fn persist_platform_report_atomic(
    db: &DatabaseConnection,
    user_id: i32,
    report: &PlatformReport,
    report_settings: &crate::api::config::ReportSettings,
) -> Result<(), String> {
    tracing::debug!("💾 Serializing report for platform: {}", report.platform);

    let report_json = serde_json::to_value(report).map_err(|error| {
        tracing::error!(platform = %report.platform, %error, "failed to serialize report");
        "Failed to save report".to_string()
    })?;

    let metadata_json = serde_json::to_value(&report.metadata).map_err(|error| {
        tracing::error!(platform = %report.platform, %error, "failed to serialize report metadata");
        "Failed to save report".to_string()
    })?;

    let txn = db.begin().await.map_err(|error| {
        tracing::error!(%error, "failed to begin report persist transaction");
        "Failed to save report".to_string()
    })?;

    let delete_result = platform_reports::Entity::delete_many()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .filter(platform_reports::Column::Platform.eq(&report.platform))
        .exec(&txn)
        .await
        .map_err(|error| {
            tracing::error!(platform = %report.platform, %error, "failed to delete old reports");
            "Failed to save report".to_string()
        })?;

    if delete_result.rows_affected > 0 {
        tracing::info!(
            "🗑️ Deleted {} old report(s) for platform {} (txn)",
            delete_result.rows_affected,
            report.platform
        );
    }

    let active_model = platform_reports::ActiveModel {
        user_id: Set(user_id),
        platform: Set(report.platform.clone()),
        metadata: Set(metadata_json),
        report: Set(report_json),
        report_title: Set(None),
        created_at: Set(chrono::Utc::now().naive_utc()),
        expires_at: Set(
            (chrono::Utc::now() + chrono::Duration::days(report_settings.expiry_days)).naive_utc(),
        ),
        ..Default::default()
    };

    active_model.insert(&txn).await.map_err(|error| {
        tracing::error!(platform = %report.platform, %error, "failed to insert report");
        "Failed to save report".to_string()
    })?;

    txn.commit().await.map_err(|error| {
        tracing::error!(platform = %report.platform, %error, "failed to commit report persist");
        "Failed to save report".to_string()
    })?;

    tracing::info!("✅ Saved platform report for {}", report.platform);
    Ok(())
}

#[cfg(test)]
mod report_persist_concurrency_tests {
    use super::MAX_CONCURRENT_PLATFORM_REPORTS;
    use futures::stream::{self, StreamExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn max_concurrent_platform_reports_is_generous_but_bounded() {
        // Enough for multi-platform generate-all (~11 platforms); never unbounded.
        // Compare via binding so clippy does not treat this as a constant assertion.
        let limit = MAX_CONCURRENT_PLATFORM_REPORTS;
        assert!(
            (4..=16).contains(&limit),
            "unexpected concurrency limit: {limit}"
        );
    }

    /// Mirrors MYR-021 fan-out: many platform tasks, at most N in flight.
    /// Also models partial cancel — dropping the stream keeps completed work.
    #[tokio::test]
    async fn platform_report_fanout_respects_concurrency_bound() {
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let n = 20usize;
        let limit = MAX_CONCURRENT_PLATFORM_REPORTS;

        stream::iter(0..n)
            .map(|_| {
                let current = current.clone();
                let max_seen = max_seen.clone();
                let completed = completed.clone();
                async move {
                    let c = current.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(c, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    current.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                }
            })
            .buffer_unordered(limit)
            .collect::<Vec<_>>()
            .await;

        let peak = max_seen.load(Ordering::SeqCst);
        assert!(
            peak <= limit,
            "peak concurrency {peak} exceeded bound {limit}"
        );
        assert!(peak > 1, "expected some parallelism, peak was {peak}");
        assert_eq!(completed.load(Ordering::SeqCst), n);
    }

    /// Dropping the consumer mid-flight must not lose already-finished units
    /// (MYR-021 partial cancel + MYR-020 persist-as-you-go model).
    #[tokio::test]
    async fn partial_cancel_keeps_completed_units() {
        let completed = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let n = 12usize;
        let limit = MAX_CONCURRENT_PLATFORM_REPORTS;

        let completed_c = completed.clone();
        let started_c = started.clone();
        let mut stream = stream::iter(0..n)
            .map(move |i| {
                let completed = completed_c.clone();
                let started = started_c.clone();
                async move {
                    started.fetch_add(1, Ordering::SeqCst);
                    // First few finish quickly; later ones block.
                    if i < 3 {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        completed.fetch_add(1, Ordering::SeqCst);
                        return i;
                    }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    completed.fetch_add(1, Ordering::SeqCst);
                    i
                }
            })
            .buffer_unordered(limit);

        // Collect only the first 3 completed items, then drop the stream (cancel).
        let mut got = Vec::new();
        while let Some(v) = stream.next().await {
            got.push(v);
            if got.len() >= 3 {
                break;
            }
        }
        drop(stream);

        assert_eq!(got.len(), 3);
        assert!(
            completed.load(Ordering::SeqCst) >= 3,
            "completed counter should reflect finished units kept after cancel"
        );
        // Not all N should have completed (slow ones abandoned).
        assert!(
            completed.load(Ordering::SeqCst) < n,
            "cancel should abandon remaining work"
        );
    }
}
