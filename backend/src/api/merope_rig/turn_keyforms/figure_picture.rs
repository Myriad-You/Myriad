//! A standing figure's picture enlarged by the Space's super-resolution
//! before it is decomposed in tiles, kept by the master's own pixels: the
//! decomposition is cut from it, so the importer checks it against the same
//! picture. The master itself stays as it was drawn.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Enlarged pictures kept, the newest; one per figure master in use.
const KEEP: usize = 8;

fn root() -> PathBuf {
    crate::services::data_paths::paths()
        .root
        .join("merope/figure-picture")
}

fn file_in(root: &Path, master: &[u8]) -> PathBuf {
    let digest = Sha256::digest(master);
    let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    root.join(format!("{name}.png"))
}

/// The enlarged picture of a master, if one was made.
pub(super) async fn read(master: &[u8]) -> Option<Vec<u8>> {
    read_in(&root(), master).await
}

async fn read_in(root: &Path, master: &[u8]) -> Option<Vec<u8>> {
    tokio::fs::read(file_in(root, master)).await.ok()
}

/// Keeps a master's enlarged picture, dropping the oldest made past KEEP.
pub(super) async fn write(master: &[u8], picture: &[u8]) {
    write_in(&root(), master, picture).await
}

async fn write_in(root: &Path, master: &[u8], picture: &[u8]) {
    let result = async {
        tokio::fs::create_dir_all(root).await?;
        let path = file_in(root, master);
        let partial = path.with_extension("partial");
        tokio::fs::write(&partial, picture).await?;
        tokio::fs::rename(&partial, &path).await
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(%error, "enlarged figure picture could not be kept");
        return;
    }
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return;
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.path().extension().is_some_and(|ext| ext == "png")
            && let Ok(meta) = entry.metadata().await
        {
            files.push((meta.modified().ok(), entry.path()));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in files.into_iter().skip(KEEP) {
        let _ = tokio::fs::remove_file(path).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_master_reads_back_its_own_picture_and_old_ones_go() {
        let root = std::env::temp_dir().join(format!(
            "figure-picture-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        write_in(&root, b"master-a", b"picture-a").await;
        assert_eq!(read_in(&root, b"master-a").await.unwrap(), b"picture-a");
        assert!(read_in(&root, b"master-b").await.is_none());
        for i in 0..KEEP + 2 {
            write_in(&root, format!("m{i}").as_bytes(), b"p").await;
        }
        let kept = std::fs::read_dir(&root).unwrap().count();
        assert_eq!(kept, KEEP);
        std::fs::remove_dir_all(root).unwrap();
    }
}
