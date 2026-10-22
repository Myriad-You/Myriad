//! Every finished turn keys job kept whole: the master, the four turned
//! drawings, the five decompositions, the keys and the baked front PSD, with
//! what they were made with. A kept job can be downloaded, or fitted again
//! with the Space's current fit without drawing or decomposing anything. A
//! standing figure's job keyed its head as a bust: the master and the rest are
//! the head's, and the figure's own picture and decomposition are kept beside
//! them to put the head back into.

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
pub(super) const FIGURE_DECOMPOSITION: &str = "figure.psd";

/// A standing figure whose head was keyed as a bust.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FigureMeta {
    /// The figure's picture in the archive.
    pub master: String,
    pub master_media_type: String,
    /// The head's box in the figure picture's pixels (x0, y0, x1, y1).
    pub crop_box: [i32; 4],
}

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
    /// A standing figure keyed by its head: the figure it goes back into.
    #[serde(default)]
    pub figure: Option<FigureMeta>,
    /// A figure's: the body motions keyed beside its head ("turn", "weight").
    /// None for a bust, and for figures kept before it was recorded.
    #[serde(default)]
    pub body_motions: Option<Vec<String>>,
}

/// One job's files, in memory.
pub(super) struct ArchiveFiles {
    pub master: Vec<u8>,
    pub turned: [Vec<u8>; 4],
    pub decompositions: [Vec<u8>; 5],
    pub keyforms: Vec<u8>,
    pub baked: Vec<u8>,
    /// A standing figure keyed by its head: its picture and decomposition.
    pub figure: Option<FigureFiles>,
}

pub(super) struct FigureFiles {
    pub master: Vec<u8>,
    pub decomposition: Vec<u8>,
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

fn figure_file(media_type: &str) -> &'static str {
    match media_type {
        "image/jpeg" => "figure.jpg",
        "image/webp" => "figure.webp",
        _ => "figure.png",
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
    if let (Some(figure), Some(kept)) = (meta.figure.as_mut(), files.figure.as_ref()) {
        figure.master = figure_file(&figure.master_media_type).to_string();
        entries.push((figure.master.as_str(), &kept.master));
        entries.push((FIGURE_DECOMPOSITION, &kept.decomposition));
    }
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
    let figure = match &meta.figure {
        Some(figure) => Some(FigureFiles {
            master: read(&figure.master).await?,
            decomposition: read(FIGURE_DECOMPOSITION).await?,
        }),
        None => None,
    };
    Ok(ArchiveFiles {
        master: read(&meta.master).await?,
        turned,
        decompositions,
        keyforms: read(KEYFORMS).await?,
        baked: read(BAKED).await?,
        figure,
    })
}

/// What a kept job hands the importer: its decomposition and its keys.
pub(super) async fn read_result(meta: &ArchiveMeta) -> std::io::Result<(Vec<u8>, Vec<u8>)> {
    let dir = archive_root().join(&meta.id);
    Ok((
        tokio::fs::read(dir.join(BAKED)).await?,
        tokio::fs::read(dir.join(KEYFORMS)).await?,
    ))
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

/// A kept job as one uploaded zip (`zip_in`'s, or the same files zipped
/// again), arriving in pieces: kept beside the archives, under a name the
/// listing passes over, until it is read back.
pub(super) fn upload_path(upload: &str) -> Option<PathBuf> {
    valid_id(upload).then(|| archive_root().join(format!(".upload-{upload}.zip")))
}

/// Largest upload taken (and all its files read out), and largest file in it.
pub(super) const UPLOAD_MAX: u64 = 1024 * 1024 * 1024;
const UPLOAD_FILE_MAX: u64 = 512 * 1024 * 1024;
/// An upload not finished within this long is dropped.
const UPLOAD_STALE: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// Drops uploads left unfinished.
pub(super) async fn prune_uploads() {
    let Ok(mut entries) = tokio::fs::read_dir(archive_root()).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stale = entry
            .metadata()
            .await
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > UPLOAD_STALE);
        if name.starts_with(".upload-") && stale {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}

/// An uploaded kept job read back: its files under one folder (or none),
/// checked to be what a job keeps. Other entries (a desktop's own, as
/// `__MACOSX/`) are passed over. The error says what is wrong with it.
pub(super) fn unpack_blocking(path: &Path) -> Result<(ArchiveMeta, ArchiveFiles), String> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|_| "The upload could not be read".to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "The upload is not a zip".to_string())?;
    let mut found: std::collections::HashMap<String, Vec<usize>> = Default::default();
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|_| "The zip could not be read".to_string())?;
        let name = entry
            .name()
            .map_err(|_| "The zip could not be read".to_string())?;
        if entry.is_dir() || name.starts_with("__MACOSX/") {
            continue;
        }
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        found.entry(base).or_default().push(index);
    }
    let mut total = 0u64;
    let mut read = |name: &str| -> Result<Vec<u8>, String> {
        let index = match found.get(name).map(Vec::as_slice) {
            Some([index]) => *index,
            Some(_) => return Err(format!("The zip holds more than one {name}")),
            None => return Err(format!("The zip is missing {name}")),
        };
        let entry = zip
            .by_index(index)
            .map_err(|_| format!("{name} could not be read"))?;
        let mut bytes = Vec::new();
        entry
            .take(UPLOAD_FILE_MAX + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| format!("{name} could not be read"))?;
        total += bytes.len() as u64;
        if bytes.len() as u64 > UPLOAD_FILE_MAX || total > UPLOAD_MAX {
            return Err(format!("{name} is too large"));
        }
        Ok(bytes)
    };
    let psd = |name: &str, bytes: Vec<u8>| {
        if bytes.starts_with(b"8BPS") {
            Ok(bytes)
        } else {
            Err(format!("{name} is not a PSD"))
        }
    };
    let png = |name: &str, bytes: Vec<u8>| {
        if bytes.starts_with(b"\x89PNG") {
            Ok(bytes)
        } else {
            Err(format!("{name} is not a PNG"))
        }
    };
    let known = |media_type: &str| matches!(media_type, "image/png" | "image/jpeg" | "image/webp");

    let meta: ArchiveMeta =
        serde_json::from_slice(&read(META)?).map_err(|_| format!("{META} is not a kept job's"))?;
    if !known(&meta.master_media_type) {
        return Err(format!("{META} names an unsupported picture"));
    }
    let master = read(master_file(&meta.master_media_type))?;
    let mut turned: [Vec<u8>; 4] = Default::default();
    for (slot, name) in turned.iter_mut().zip(TURNED) {
        *slot = png(name, read(name)?)?;
    }
    let mut decompositions: [Vec<u8>; 5] = Default::default();
    for (slot, name) in decompositions.iter_mut().zip(DECOMPOSITIONS) {
        *slot = psd(name, read(name)?)?;
    }
    let keyforms = read(KEYFORMS)?;
    if !serde_json::from_slice::<serde_json::Value>(&keyforms).is_ok_and(|keys| keys.is_object()) {
        return Err(format!("{KEYFORMS} holds no keys"));
    }
    let baked = psd(BAKED, read(BAKED)?)?;
    let figure = match &meta.figure {
        Some(figure) if !known(&figure.master_media_type) => {
            return Err(format!("{META} names an unsupported picture"));
        }
        Some(figure) => Some(FigureFiles {
            master: read(figure_file(&figure.master_media_type))?,
            decomposition: psd(FIGURE_DECOMPOSITION, read(FIGURE_DECOMPOSITION)?)?,
        }),
        None => None,
    };
    Ok((
        meta,
        ArchiveFiles {
            master,
            turned,
            decompositions,
            keyforms,
            baked,
            figure,
        },
    ))
}

/// Whether two pictures are the same drawing: alike when both are brought
/// to one small size over white, so a copy saved again or enlarged still is,
/// another portrait is not.
pub(super) fn same_picture(a: &[u8], b: &[u8]) -> bool {
    const SIDE: u32 = 64;
    let small = |bytes: &[u8]| {
        let picture = image::load_from_memory(bytes).ok()?.to_rgba8();
        let aspect = picture.width() as f64 / picture.height().max(1) as f64;
        let small =
            image::imageops::resize(&picture, SIDE, SIDE, image::imageops::FilterType::Triangle);
        Some((aspect, small))
    };
    let (Some((aspect_a, a)), Some((aspect_b, b))) = (small(a), small(b)) else {
        return false;
    };
    if (aspect_a / aspect_b - 1.0).abs() > 0.02 {
        return false;
    }
    let over_white = |pixel: &image::Rgba<u8>, channel: usize| {
        let alpha = pixel[3] as f64 / 255.0;
        pixel[channel] as f64 * alpha + 255.0 * (1.0 - alpha)
    };
    let difference: f64 = a
        .pixels()
        .zip(b.pixels())
        .map(|(a, b)| {
            (0..3)
                .map(|channel| (over_white(a, channel) - over_white(b, channel)).abs())
                .sum::<f64>()
        })
        .sum();
    difference / (SIDE * SIDE * 3) as f64 <= 3.0
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
            figure: None,
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
            figure: None,
            body_motions: None,
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
    async fn a_figure_keyed_by_its_head_keeps_the_figure_beside_it() {
        let root = scratch();
        let mut kept = meta("/media/assets/a/figure.png", 1);
        kept.figure = Some(FigureMeta {
            master: String::new(),
            master_media_type: "image/webp".into(),
            crop_box: [-4, 2, 371, 502],
        });
        let mut made = files(7);
        made.figure = Some(FigureFiles {
            master: vec![9; 4],
            decomposition: vec![9; 6],
        });
        let id = save_in(&root, &mut kept, &made).await.unwrap();
        let read = read_meta_in(&root, &id).await.unwrap();
        let figure = read.figure.clone().unwrap();
        assert_eq!(figure.master, "figure.webp");
        assert_eq!(figure.crop_box, [-4, 2, 371, 502]);
        assert_eq!(read.bytes, 3 + 4 * 2 + 5 * 2 + 30 + 5 + 4 + 6);
        let back = read_files_in(&root, &read).await.unwrap().figure.unwrap();
        assert_eq!((back.master, back.decomposition), (vec![9; 4], vec![9; 6]));
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

    fn kept_files() -> ArchiveFiles {
        let psd = |tag: u8| [b"8BPS".as_slice(), &[tag]].concat();
        let png = |tag: u8| [b"\x89PNG".as_slice(), &[tag]].concat();
        ArchiveFiles {
            master: vec![1; 3],
            turned: std::array::from_fn(|i| png(i as u8)),
            decompositions: std::array::from_fn(|i| psd(i as u8)),
            keyforms: b"{\"canvas\":[1,1],\"keyforms\":{}}".to_vec(),
            baked: psd(9),
            figure: None,
        }
    }

    fn zip_of(entries: &[(String, Vec<u8>)]) -> PathBuf {
        let path = scratch().join("upload.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[tokio::test]
    async fn a_downloaded_job_uploaded_again_reads_back_whole() {
        let root = scratch();
        let id = save_in(&root, &mut meta("/m/one.png", 1), &kept_files())
            .await
            .unwrap();
        let path = root.join("upload.zip");
        std::fs::write(&path, zip_in(&root, &id).unwrap()).unwrap();
        let (read, files) = unpack_blocking(&path).unwrap();
        assert_eq!(read.id, id);
        assert_eq!(files.master, vec![1; 3]);
        assert_eq!(files.decompositions[3], b"8BPS\x03");
        assert!(files.figure.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_job_zipped_again_on_a_desktop_still_reads() {
        let made = kept_files();
        let json = serde_json::to_vec(&meta("/m/one.png", 1)).unwrap();
        let mut entries = vec![
            ("kept/archive.json".to_string(), json),
            ("kept/master.png".to_string(), made.master),
            ("kept/keyforms.json".to_string(), made.keyforms),
            ("kept/baked.psd".to_string(), made.baked),
            ("__MACOSX/kept/._baked.psd".to_string(), vec![0; 4]),
        ];
        for (name, bytes) in TURNED.iter().zip(made.turned) {
            entries.push((format!("kept/{name}"), bytes));
        }
        for (name, bytes) in DECOMPOSITIONS.iter().zip(made.decompositions) {
            entries.push((format!("kept/{name}"), bytes));
        }
        let (_, files) = unpack_blocking(&zip_of(&entries)).unwrap();
        assert_eq!(files.baked, b"8BPS\x09");

        entries.retain(|(name, _)| !name.ends_with("decomposition-2.psd"));
        let missing = unpack_blocking(&zip_of(&entries)).err().unwrap();
        assert!(missing.contains("decomposition-2.psd"), "{missing}");
        entries.push(("kept/decomposition-2.psd".into(), b"GIF89a".to_vec()));
        let wrong = unpack_blocking(&zip_of(&entries)).err().unwrap();
        assert!(wrong.contains("not a PSD"), "{wrong}");
    }

    #[test]
    fn anything_but_a_zip_is_refused() {
        let path = scratch().join("upload.zip");
        std::fs::write(&path, b"not a zip").unwrap();
        assert_eq!(
            unpack_blocking(&path).err().unwrap(),
            "The upload is not a zip"
        );
    }

    fn picture(
        width: u32,
        height: u32,
        paint: impl Fn(u32, u32) -> [u8; 4],
        format: image::ImageFormat,
    ) -> Vec<u8> {
        let picture = image::RgbaImage::from_fn(width, height, |x, y| image::Rgba(paint(x, y)));
        let mut bytes = Vec::new();
        let picture = if format == image::ImageFormat::Jpeg {
            image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(picture).to_rgb8())
        } else {
            image::DynamicImage::ImageRgba8(picture)
        };
        picture
            .write_to(&mut std::io::Cursor::new(&mut bytes), format)
            .unwrap();
        bytes
    }

    #[test]
    fn the_same_drawing_saved_again_or_enlarged_is_the_same_picture() {
        let girl = |scale: u32| {
            move |x: u32, y: u32| {
                let (x, y) = (x / scale, y / scale);
                if (x as i32 - 40).pow(2) + (y as i32 - 50).pow(2) < 900 {
                    [230, 180, 170, 255]
                } else if y > 70 {
                    [40, 60, 120, 255]
                } else {
                    [255, 255, 255, 0]
                }
            }
        };
        let master = picture(80, 120, girl(1), image::ImageFormat::Png);
        assert!(same_picture(&master, &master));
        assert!(same_picture(
            &master,
            &picture(80, 120, girl(1), image::ImageFormat::Jpeg)
        ));
        assert!(same_picture(
            &master,
            &picture(160, 240, girl(2), image::ImageFormat::Png)
        ));
        let other = picture(
            80,
            120,
            |x, _| {
                if x < 40 {
                    [20, 20, 20, 255]
                } else {
                    [255, 255, 255, 0]
                }
            },
            image::ImageFormat::Png,
        );
        assert!(!same_picture(&master, &other));
        assert!(
            !same_picture(
                &master,
                &picture(120, 120, girl(1), image::ImageFormat::Png)
            ),
            "another shape"
        );
        assert!(!same_picture(&master, b"not a picture"));
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
