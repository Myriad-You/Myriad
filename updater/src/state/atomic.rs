//! Atomic write helpers. See spec §6.2.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::Result;

/// Write `data` to `path` atomically:
/// 1. open `<path>.tmp.<pid>.<nanos>` with O_CREAT|O_EXCL
/// 2. write + flush + fsync
/// 3. rename → `path`
/// 4. fsync parent directory
pub fn write_atomic_bytes(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(
        ".{}.tmp.{}.{}",
        path.file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default(),
        pid,
        nanos
    ));

    // SAFETY: keep the file scope tight so it closes before rename.
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(data)?;
        f.flush()?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Best-effort cleanup on failure.
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    fsync_dir(parent)?;
    Ok(())
}

pub fn write_atomic_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_atomic_bytes(path, &bytes)
}

/// Completed attempts past which the log escalates from `warn` to `error`.
const OUTCOME_WRITE_WARN_ATTEMPTS: u32 = 5;

/// A completed operation must not be rerun because its outcome could not be saved,
/// so this never gives up. It does back off and escalate, so a host that cannot
/// persist state shows up in the log instead of silently spinning every 2s.
pub(crate) async fn write_json_until_saved<T: serde::Serialize>(path: &Path, value: &T) {
    let mut attempt: u32 = 0;
    loop {
        match write_atomic_json(path, value) {
            Ok(()) => {
                if attempt > 0 {
                    tracing::info!(
                        attempts = attempt + 1,
                        path = %path.display(),
                        "outcome write recovered"
                    );
                }
                return;
            }
            Err(error) if attempt < OUTCOME_WRITE_WARN_ATTEMPTS => {
                tracing::warn!(%error, attempt = attempt + 1, "retrying outcome write");
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    attempt = attempt + 1,
                    path = %path.display(),
                    "outcome write still failing; the result is not persisted yet"
                );
            }
        }
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(outcome_write_backoff(attempt)).await;
    }
}

/// 2s, 4s, 8s … capped at 60s. `attempt` counts completed writes.
fn outcome_write_backoff(attempt: u32) -> Duration {
    Duration::from_secs((2u64 << attempt.saturating_sub(1).min(5)).min(60))
}

#[cfg(unix)]
fn fsync_dir(p: &Path) -> Result<()> {
    let f = File::open(p)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn fsync_dir(_p: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test(start_paused = true)]
    async fn outcome_write_recovers_after_a_temporary_filesystem_failure() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("outcome.json");
        std::fs::create_dir(&target).unwrap();
        let path = target.clone();
        let task = tokio::spawn(async move {
            write_json_until_saved(&path, &serde_json::json!({"status":"succeeded"})).await;
        });
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        std::fs::remove_dir(&target).unwrap();
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        task.await.unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(target).unwrap()).unwrap();
        assert_eq!(value["status"], "succeeded");
    }

    #[test]
    fn outcome_write_backoff_grows_then_caps() {
        assert_eq!(outcome_write_backoff(1), Duration::from_secs(2));
        assert_eq!(outcome_write_backoff(2), Duration::from_secs(4));
        assert_eq!(outcome_write_backoff(3), Duration::from_secs(8));
        assert_eq!(outcome_write_backoff(6), Duration::from_secs(60));
        assert_eq!(outcome_write_backoff(50), Duration::from_secs(60));
    }

    #[test]
    fn roundtrip() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("foo.json");
        write_atomic_bytes(&target, b"hello").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        // overwrite
        write_atomic_bytes(&target, b"world").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"world");
    }
}
