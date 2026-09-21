//! Atomic write helpers. See spec §6.2.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// A completed operation must not be rerun because its outcome could not be saved.
pub(crate) async fn write_json_until_saved<T: serde::Serialize>(path: &Path, value: &T) {
    loop {
        match write_atomic_json(path, value) {
            Ok(()) => return,
            Err(error) => tracing::warn!(%error, "retrying outcome write"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
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
