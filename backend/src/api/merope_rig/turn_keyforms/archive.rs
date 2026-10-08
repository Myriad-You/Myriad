//! Every finished turn keys job kept whole: the master, the four turned
//! drawings, the five decompositions, the keys and the baked front PSD, with
//! what they were made with. A kept job can be downloaded, or fitted again
//! with the Space's current fit without drawing or decomposing anything.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Jobs kept for one master (one portrait in one slot); older ones are removed.
pub(super) const KEEP_PER_MASTER: usize = 2;
const META: &str = "archive.json";
/// The files of a job, in the order the fit takes them.
pub(super) const TURNED: [&str; 4] = [
    "turned-0.png",
    "turned-1.png",
    "turned-2.png",
    "turned-3.png",
];
pub(super) const DECOMPOSITIONS: [&str; 5] = [
    "decomposition-0.psd",
    "decomposition-1.psd",
    "decomposition-2.psd",
    "decomposition-3.psd",
    "decomposition-4.psd",
];
pub(super) const KEYFORMS: &str = "keyforms.json";
pub(super) const BAKED: &str = "baked.psd";

/// What a kept job was made from and with.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArchiveMeta {
    pub id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// None: the worn bust; Some: a full-body set.
    pub outfit_id: Option<String>,
    pub source_master_asset_id: String,
    pub source_generation_fingerprint: Option<String>,
    /// The kept job this one was fitted again from, if any.
    pub parent: Option<String>,
    /// The master's file in the archive (its type decides the extension).
    pub master: String,
    pub master_media_type: String,
    pub generation_provider: String,
    pub generation_model: String,
    pub prompts: Vec<String>,
    pub space: String,
    /// Sides drawn again after the Space's check ("plus", "minus", "up", "down").
    pub redrawn: Vec<String>,
    /// Bytes on disk, all files together.
    #[serde(default)]
    pub bytes: u64,
}

/// One job's files, in memory.
pub(super) struct ArchiveFiles {
    pub master: Vec<u8>,
    pub turned: [Vec<u8>; 4],
    pub decompositions: [Vec<u8>; 5],
    pub keyforms: Vec<u8>,
    pub baked: Vec<u8>,
}

pub(super) fn archive_root() -> PathBuf {
    crate::services::data_paths::paths()
        .root
        .join("merope/turn-archive")
}

/// An id this module made: a timestamp and random hex, nothing that walks paths.
pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte.is_ascii_lowercase() || byte == b'-')
}

pub(super) fn new_id() -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
        &random[..8]
    )
}

fn master_file(media_type: &str) -> &'static str {
    match media_type {
        "image/jpeg" => "master.jpg",
        "image/webp" => "master.webp",
        _ => "master.png",
    }
}

/// Writes a job whole (into a temporary directory, then renamed) and drops
/// the oldest jobs of the same master past KEEP_PER_MASTER.
pub(super) async fn save(mut meta: ArchiveMeta, files: &ArchiveFiles) -> std::io::Result<String> {
    save_in(&archive_root(), &mut meta, files).await
}

async fn save_in(
    root: &Path,
    meta: &mut ArchiveMeta,
    files: &ArchiveFiles,
) -> std::io::Result<String> {
    meta.master = master_file(&meta.master_media_type).to_string();
    let mut entries: Vec<(&str, &[u8])> = vec![(meta.master.as_str(), &files.master)];
    entries.extend(
        TURNED
            .iter()
            .copied()
            .zip(files.turned.iter().map(Vec::as_slice)),
    );
    entries.extend(
        DECOMPOSITIONS
            .iter()
            .copied()
            .zip(files.decompositions.iter().map(Vec::as_slice)),
    );
    entries.push((KEYFORMS, &files.keyforms));
    entries.push((BAKED, &files.baked));
    meta.bytes = entries.iter().map(|(_, bytes)| bytes.len() as u64).sum();

    tokio::fs::create_dir_all(root).await?;
    let partial = root.join(format!(".{}.partial", meta.id));
    let _ = tokio::fs::remove_dir_all(&partial).await;
    tokio::fs::create_dir_all(&partial).await?;
    for (name, bytes) in &entries {
        tokio::fs::write(partial.join(name), bytes).await?;
    }
    let json = serde_json::to_vec_pretty(meta).map_err(std::io::Error::other)?;
    tokio::fs::write(partial.join(META), json).await?;
    tokio::fs::rename(&partial, root.join(&meta.id)).await?;
    prune_master(root, meta).await;
    Ok(meta.id.clone())
}

async fn prune_master(root: &Path, kept: &ArchiveMeta) {
    let mut same: Vec<ArchiveMeta> = list_in(root)
        .await
        .into_iter()
        .filter(|meta| {
            meta.outfit_id == kept.outfit_id
                && meta.source_master_asset_id == kept.source_master_asset_id
        })
        .collect();
    same.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    for old in same.into_iter().skip(KEEP_PER_MASTER) {
        if let Err(error) = tokio::fs::remove_dir_all(root.join(&old.id)).await {
            tracing::warn!(%error, archive = %old.id, "old turn keys archive could not be removed");
        }
    }
}

/// Every kept job, newest first.
pub(super) async fn list() -> Vec<ArchiveMeta> {
    list_in(&archive_root()).await
}

async fn list_in(root: &Path) -> Vec<ArchiveMeta> {
    let mut found = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return found;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !valid_id(&name) {
            continue;
        }
        if let Some(meta) = read_meta_in(root, &name).await {
            found.push(meta);
        }
    }
    found.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    found
}

pub(super) async fn read_meta(id: &str) -> Option<ArchiveMeta> {
    read_meta_in(&archive_root(), id).await
}

async fn read_meta_in(root: &Path, id: &str) -> Option<ArchiveMeta> {
    if !valid_id(id) {
        return None;
    }
    let bytes = tokio::fs::read(root.join(id).join(META)).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// A kept job's inputs and outputs.
pub(super) async fn read_files(meta: &ArchiveMeta) -> std::io::Result<ArchiveFiles> {
    read_files_in(&archive_root(), meta).await
}

async fn read_files_in(root: &Path, meta: &ArchiveMeta) -> std::io::Result<ArchiveFiles> {
    let dir = root.join(&meta.id);
    let read = |name: &str| tokio::fs::read(dir.join(name));
    let mut turned: [Vec<u8>; 4] = Default::default();
    for (slot, name) in turned.iter_mut().zip(TURNED) {
        *slot = read(name).await?;
    }
    let mut decompositions: [Vec<u8>; 5] = Default::default();
    for (slot, name) in decompositions.iter_mut().zip(DECOMPOSITIONS) {
        *slot = read(name).await?;
    }
    Ok(ArchiveFiles {
        master: read(&meta.master).await?,
        turned,
        decompositions,
        keyforms: read(KEYFORMS).await?,
        baked: read(BAKED).await?,
    })
}

pub(super) async fn delete(id: &str) -> std::io::Result<bool> {
    if !valid_id(id) {
        return Ok(false);
    }
    match tokio::fs::remove_dir_all(archive_root().join(id)).await {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// A kept job as one zip (stored: PNG and PSD do not compress further).
pub(super) fn zip_blocking(id: &str) -> std::io::Result<Vec<u8>> {
    zip_in(&archive_root(), id)
}

fn zip_in(root: &Path, id: &str) -> std::io::Result<Vec<u8>> {
    if !valid_id(id) {
        return Err(std::io::ErrorKind::NotFound.into());
    }
    let dir = root.join(id);
    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    for name in names {
        zip.start_file(format!("{id}/{name}"), options)
            .map_err(std::io::Error::other)?;
        let mut file = std::fs::File::open(dir.join(&name))?;
        std::io::copy(&mut file, &mut zip)?;
    }
    Ok(zip.finish().map_err(std::io::Error::other)?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(tag: u8) -> ArchiveFiles {
        ArchiveFiles {
            master: vec![tag; 3],
            turned: std::array::from_fn(|i| vec![tag, i as u8]),
            decompositions: std::array::from_fn(|i| vec![tag, 10 + i as u8]),
            keyforms: b"{\"canvas\":[1,1],\"keyforms\":{}}".to_vec(),
            baked: vec![tag; 5],
        }
    }

    fn meta(master: &str, seconds: i64) -> ArchiveMeta {
        ArchiveMeta {
            id: format!("20261008-0000{seconds:02}-abcdef{seconds:02}"),
            created_at: chrono::DateTime::from_timestamp(1_800_000_000 + seconds, 0).unwrap(),
            outfit_id: None,
            source_master_asset_id: master.into(),
            source_generation_fingerprint: None,
            parent: None,
            master: String::new(),
            master_media_type: "image/png".into(),
            generation_provider: "openai".into(),
            generation_model: "gpt-image".into(),
            prompts: vec!["p".into()],
            space: "owner/space".into(),
            redrawn: vec![],
            bytes: 0,
        }
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "turn-archive-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn a_job_is_kept_whole_and_read_back_as_written() {
        let root = scratch();
        let mut kept = meta("/media/assets/a/master.png", 1);
        let id = save_in(&root, &mut kept, &files(7)).await.unwrap();
        let read = read_meta_in(&root, &id).await.unwrap();
        assert_eq!(read.master, "master.png");
        assert_eq!(read.bytes, 3 + 4 * 2 + 5 * 2 + 30 + 5);
        let back = read_files_in(&root, &read).await.unwrap();
        assert_eq!(back.turned[2], vec![7, 2]);
        assert_eq!(back.decompositions[4], vec![7, 14]);
        let zip = zip_in(&root, &id).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
        assert_eq!(
            archive.len(),
            13,
            "master, 4 drawings, 5 decompositions, keys, baked, meta"
        );
        assert!(archive.by_name(&format!("{id}/baked.psd")).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn only_the_two_newest_jobs_of_a_master_are_kept() {
        let root = scratch();
        for seconds in 1..=3 {
            save_in(&root, &mut meta("/m/one.png", seconds), &files(1))
                .await
                .unwrap();
        }
        save_in(&root, &mut meta("/m/two.png", 4), &files(2))
            .await
            .unwrap();
        let kept: Vec<_> = list_in(&root)
            .await
            .into_iter()
            .map(|m| (m.source_master_asset_id, m.id))
            .collect();
        assert_eq!(kept.len(), 3);
        assert_eq!(
            kept.iter()
                .filter(|(master, _)| master == "/m/one.png")
                .count(),
            2
        );
        assert!(
            !kept.iter().any(|(_, id)| id.ends_with("abcdef01")),
            "the oldest of one.png is gone"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ids_that_could_walk_paths_are_refused() {
        assert!(valid_id("20261008-101010-abcd1234"));
        assert!(!valid_id("../etc"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(""));
        assert!(valid_id(&new_id()));
    }
}
