//! Anime2.5D rig store.
//!
//! Packages live on disk by content id and can coexist. The live pointer is
//! the worn outfit's package. Atlas validation walks PNG chunks and inflates
//! IDAT (no RGBA bitmap); truncated/invalid zlib fails before adopt.
//! Compilation stays in `myriad-merope`.

use std::{
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context};
use myriad_merope::RigManifest;
use once_cell::sync::Lazy;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::services::data_paths;

pub const ATLAS_FILENAME: &str = "atlas.png";
pub const MANIFEST_FILENAME: &str = "manifest.json";

const PNG_SIG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

pub fn normalize_asset_id(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.len() != 64 {
        return None;
    }
    if value.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(value.to_ascii_lowercase())
    } else {
        None
    }
}

/// Content identity for the complete immutable package. Runtime asset URLs are
/// normalized out so the package id does not recursively hash itself.
pub fn package_id_for_manifest(atlas: &[u8], manifest: &RigManifest) -> anyhow::Result<String> {
    let mut normalized = manifest.clone();
    for texture in &mut normalized.textures {
        texture.url = format!("asset://atlas/{}", texture.id);
    }
    let value = serde_json::to_value(normalized)?;
    let mut canonical_manifest = Vec::new();
    write_canonical_json(&value, &mut canonical_manifest)?;

    let mut hasher = Sha256::new();
    hasher.update(b"myriad-rig-package-v1\0");
    hasher.update((atlas.len() as u64).to_be_bytes());
    hasher.update(atlas);
    hasher.update((canonical_manifest.len() as u64).to_be_bytes());
    hasher.update(canonical_manifest);
    Ok(hex::encode(hasher.finalize()))
}

fn write_canonical_json(value: &Value, output: &mut Vec<u8>) -> serde_json::Result<()> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(value) => output.extend_from_slice(if *value { b"true" } else { b"false" }),
        Value::Number(value) => output.extend_from_slice(value.to_string().as_bytes()),
        Value::String(_) => serde_json::to_writer(output, value)?,
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key)?;
                output.push(b':');
                write_canonical_json(&values[key], output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

pub fn rig_root() -> PathBuf {
    data_paths::paths().root.join("merope/rig")
}

pub fn asset_dir(asset_id: &str) -> PathBuf {
    rig_root().join(asset_id)
}

pub fn atlas_path(asset_id: &str) -> PathBuf {
    asset_dir(asset_id).join(ATLAS_FILENAME)
}

pub fn manifest_path(asset_id: &str) -> PathBuf {
    asset_dir(asset_id).join(MANIFEST_FILENAME)
}

/// Validates the browser-produced RGBA8 atlas stream without materializing
/// pixels. Structural truncation and invalid zlib data must fail before a rig
/// package can be activated.
pub fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() < 8 || !bytes.starts_with(PNG_SIG) {
        return Err("Rig atlas must be a PNG image".to_string());
    }
    let mut offset = PNG_SIG.len();
    let mut dimensions = None;
    let mut compressed = Vec::new();
    let mut saw_iend = false;
    while offset < bytes.len() {
        let header_end = offset
            .checked_add(8)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "Rig atlas PNG is truncated".to_string())?;
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let chunk_type = &bytes[offset + 4..header_end];
        let data_end = header_end
            .checked_add(length)
            .filter(|end| {
                end.checked_add(4)
                    .is_some_and(|crc_end| crc_end <= bytes.len())
            })
            .ok_or_else(|| "Rig atlas PNG chunk is truncated".to_string())?;
        let chunk_end = data_end + 4;
        let data = &bytes[header_end..data_end];
        match chunk_type {
            b"IHDR" if dimensions.is_none() && offset == PNG_SIG.len() && length == 13 => {
                let width = u32::from_be_bytes(data[0..4].try_into().unwrap());
                let height = u32::from_be_bytes(data[4..8].try_into().unwrap());
                if data[8..13] != [8, 6, 0, 0, 0] {
                    return Err("Rig atlas PNG must be non-interlaced RGBA8".to_string());
                }
                dimensions = Some((width, height));
            }
            b"IHDR" => return Err("Rig atlas PNG has an invalid IHDR".to_string()),
            b"IDAT" if dimensions.is_some() && !saw_iend => {
                compressed.extend_from_slice(data);
            }
            b"IEND" if length == 0 && chunk_end == bytes.len() => {
                saw_iend = true;
            }
            b"IEND" => return Err("Rig atlas PNG has an invalid IEND".to_string()),
            _ if dimensions.is_none() || saw_iend => {
                return Err("Rig atlas PNG chunk order is invalid".to_string())
            }
            _ => {}
        }
        offset = chunk_end;
    }
    let (width, height) =
        dimensions.ok_or_else(|| "Rig atlas PNG is missing an IHDR".to_string())?;
    if width < 256 || height < 256 || width > 8_192 || height > 8_192 {
        return Err("Rig atlas must be between 256 and 8192 pixels on each side".to_string());
    }
    if !saw_iend || compressed.is_empty() {
        return Err("Rig atlas PNG is incomplete".to_string());
    }
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .and_then(|width| width.checked_add(1))
        .ok_or_else(|| "Rig atlas PNG dimensions overflow".to_string())?;
    // Validate one scanline at a time: at 8192² the old full inflated buffer
    // cost 256 MiB. This scratch buffer is at most 32 KiB + one filter byte.
    let invalid = || "Rig atlas PNG pixel stream is invalid".to_string();
    let mut decoded = flate2::read::ZlibDecoder::new(compressed.as_slice());
    let mut row = vec![0; row_bytes];
    for _ in 0..height {
        decoded.read_exact(&mut row).map_err(|_| invalid())?;
        if row[0] > 4 {
            return Err(invalid());
        }
    }
    // Read through the zlib trailer/checksum; surplus pixels are also invalid.
    if decoded.read(&mut [0]).map_err(|_| invalid())? != 0 {
        return Err("Rig atlas PNG pixel stream is invalid".to_string());
    }
    Ok((width, height))
}

#[derive(Debug)]
pub enum PngValidationError {
    Busy,
    Invalid(String),
    WorkerFailed,
}

static PNG_VALIDATORS: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(2)));

/// No bitmap, no CPU-heavy inflate on an async request worker, no unbounded
/// queue of uploads waiting for a decoder. The task owns the permit even when
/// its HTTP request disappears: spawn_blocking itself cannot be cancelled.
pub async fn validate_png_dimensions(
    bytes: Vec<u8>,
) -> Result<(Vec<u8>, (u32, u32)), PngValidationError> {
    validate_png_with_pool(bytes, PNG_VALIDATORS.clone(), png_dimensions).await
}

async fn validate_png_with_pool(
    bytes: Vec<u8>,
    pool: Arc<Semaphore>,
    validate: impl FnOnce(&[u8]) -> Result<(u32, u32), String> + Send + 'static,
) -> Result<(Vec<u8>, (u32, u32)), PngValidationError> {
    let permit = pool
        .try_acquire_owned()
        .map_err(|_| PngValidationError::Busy)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let dimensions = validate(&bytes).map_err(PngValidationError::Invalid)?;
        Ok((bytes, dimensions))
    })
    .await
    .map_err(|_| PngValidationError::WorkerFailed)?
}

pub async fn persist_package(
    asset_id: &str,
    atlas: &[u8],
    manifest_json: &str,
) -> anyhow::Result<PathBuf> {
    let root = rig_root();
    tokio::fs::create_dir_all(&root)
        .await
        .with_context(|| format!("create rig root {}", root.display()))?;
    let dir = asset_dir(asset_id);
    if dir.exists() {
        verify_existing_package(&dir, atlas, manifest_json).await?;
        return Ok(dir);
    }

    let temp_dir = root.join(format!(".{asset_id}.{}.tmp", Uuid::new_v4().simple()));
    tokio::fs::create_dir(&temp_dir)
        .await
        .with_context(|| format!("create rig temp dir {}", temp_dir.display()))?;
    let write_result = async {
        tokio::fs::write(temp_dir.join(ATLAS_FILENAME), atlas).await?;
        tokio::fs::write(temp_dir.join(MANIFEST_FILENAME), manifest_json).await?;
        tokio::fs::rename(&temp_dir, &dir).await
    }
    .await;
    if let Err(error) = write_result {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        if dir.exists() {
            verify_existing_package(&dir, atlas, manifest_json).await?;
            return Ok(dir);
        }
        return Err(error).with_context(|| format!("commit rig package {}", dir.display()));
    }
    Ok(dir)
}

async fn verify_existing_package(
    dir: &Path,
    atlas: &[u8],
    manifest_json: &str,
) -> anyhow::Result<()> {
    let stored_atlas = tokio::fs::read(dir.join(ATLAS_FILENAME)).await?;
    let stored_manifest = tokio::fs::read(dir.join(MANIFEST_FILENAME)).await?;
    let stored_manifest: Value = serde_json::from_slice(&stored_manifest)?;
    let requested_manifest: Value = serde_json::from_str(manifest_json)?;
    if stored_atlas != atlas || stored_manifest != requested_manifest {
        return Err(anyhow!(
            "rig package id collision or incomplete package at {}",
            dir.display()
        ));
    }
    Ok(())
}

pub async fn read_manifest_bytes(asset_id: &str) -> anyhow::Result<Vec<u8>> {
    let path = manifest_path(asset_id);
    tokio::fs::read(&path)
        .await
        .with_context(|| format!("read {}", path.display()))
}

pub async fn read_atlas_bytes(asset_id: &str) -> anyhow::Result<Vec<u8>> {
    let path = atlas_path(asset_id);
    tokio::fs::read(&path)
        .await
        .with_context(|| format!("read {}", path.display()))
}

pub fn public_atlas_url(asset_id: &str) -> String {
    format!("/api/merope/rig/assets/{asset_id}")
}

/// Persist the active pointer through any SeaORM connection, including an
/// existing transaction. The in-process mirror must be updated only after the
/// caller commits that transaction.
pub async fn persist_active_asset<C>(
    db: &C,
    asset_id: Option<&str>,
) -> anyhow::Result<Option<String>>
where
    C: ConnectionTrait,
{
    let normalized = match asset_id {
        Some(value) => {
            Some(normalize_asset_id(value).ok_or_else(|| anyhow!("invalid rig asset id"))?)
        }
        None => None,
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO configurations (key, value, updated_at)
VALUES ($1, $2::jsonb, CURRENT_TIMESTAMP)
ON CONFLICT (key) DO UPDATE
SET value = $2::jsonb, updated_at = CURRENT_TIMESTAMP
"#,
        vec!["agent_rig_asset_id".into(), json!(normalized).into()],
    ))
    .await?;
    Ok(normalized)
}

pub async fn mirror_active_asset(asset_id: Option<String>) {
    crate::GLOBAL_DYNAMIC_CONFIG
        .write()
        .await
        .agent_rig_asset_id = asset_id;
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::ZlibEncoder, Compression};
    use myriad_merope::{
        RigBone, RigManifest, RigPart, RigPoint, RigQuality, RigTexture, RigVertex,
        CHARACTER_ASSET_CONTRACT_VERSION, RIG_IR_VERSION, RIG_SCHEMA_VERSION,
    };
    use std::{collections::HashMap, io::Write};

    fn push_png_chunk(bytes: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&0u32.to_be_bytes());
    }

    fn rgba_png(width: u32, height: u32) -> Vec<u8> {
        rgba_png_stream(width, height, 0, 0)
    }

    fn rgba_png_stream(width: u32, height: u32, filter: u8, extra: usize) -> Vec<u8> {
        let mut bytes = PNG_SIG.to_vec();
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        push_png_chunk(&mut bytes, b"IHDR", &ihdr);

        let row_bytes = width as usize * 4 + 1;
        let mut row = vec![0; row_bytes];
        row[0] = filter;
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        for _ in 0..height {
            encoder.write_all(&row).unwrap();
        }
        encoder.write_all(&vec![0; extra]).unwrap();
        push_png_chunk(&mut bytes, b"IDAT", &encoder.finish().unwrap());
        push_png_chunk(&mut bytes, b"IEND", &[]);
        bytes
    }

    #[test]
    fn validates_complete_rgba_atlas_stream() {
        assert_eq!(png_dimensions(&rgba_png(256, 256)).unwrap(), (256, 256));
        assert_eq!(png_dimensions(&rgba_png(1024, 1024)).unwrap(), (1024, 1024));
        assert!(png_dimensions(&rgba_png(64, 64)).is_err());
        let truncated = rgba_png(256, 256);
        assert!(png_dimensions(&truncated[..truncated.len() - 8]).is_err());
        assert!(png_dimensions(b"not-a-png").is_err());
    }

    #[test]
    fn validates_maximum_atlas_without_a_full_bitmap_and_rejects_invalid_scanlines() {
        assert_eq!(png_dimensions(&rgba_png(8192, 8192)).unwrap(), (8192, 8192));
        for filter in 0..=4 {
            assert!(png_dimensions(&rgba_png_stream(256, 256, filter, 0)).is_ok());
        }
        assert!(png_dimensions(&rgba_png_stream(256, 256, 5, 0)).is_err());
        assert!(png_dimensions(&rgba_png_stream(256, 256, 0, 1)).is_err());
        let mut checksum = rgba_png(256, 256);
        // IDAT is followed by its CRC and the 12-byte IEND. Corrupt Adler32.
        let last_zlib_byte = checksum.len() - 17;
        checksum[last_zlib_byte] ^= 0xff;
        assert!(png_dimensions(&checksum).is_err());
    }

    #[tokio::test]
    async fn decoder_permit_survives_cancelled_http_waiter() {
        let pool = Arc::new(Semaphore::new(1));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker_pool = pool.clone();
        let task = tokio::spawn(async move {
            validate_png_with_pool(vec![], worker_pool, move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok((256, 256))
            })
            .await
        });
        started_rx.await.unwrap();
        task.abort();
        let _ = task.await;
        assert!(matches!(
            validate_png_with_pool(vec![], pool.clone(), png_dimensions).await,
            Err(PngValidationError::Busy)
        ));
        release_tx.send(()).unwrap();
        let permit = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            pool.clone().acquire_owned(),
        )
        .await
        .unwrap()
        .unwrap();
        drop(permit);
        let bytes = rgba_png(256, 256);
        let (returned, dimensions) = validate_png_with_pool(bytes.clone(), pool, png_dimensions)
            .await
            .unwrap();
        assert_eq!(returned, bytes);
        assert_eq!(dimensions, (256, 256));
    }

    fn sample_manifest(master: &str, fingerprint: Option<String>) -> RigManifest {
        RigManifest {
            schema_version: RIG_SCHEMA_VERSION,
            rig_ir_version: Some(RIG_IR_VERSION),
            character_asset_contract_version: Some(CHARACTER_ASSET_CONTRACT_VERSION),
            source_master_asset_id: Some(master.to_string()),
            source_generation_fingerprint: fingerprint,
            quality: RigQuality::Layered2d,
            canvas: myriad_merope::RigSize {
                width: 1.0,
                height: 1.3333334,
            },
            textures: vec![RigTexture {
                id: "atlas".to_string(),
                url: "/atlas.png".to_string(),
                width: 256,
                height: 256,
            }],
            bones: vec![RigBone {
                id: "root".to_string(),
                parent: None,
                pivot: RigPoint { x: 0.5, y: 0.8 },
            }],
            parts: vec![RigPart {
                id: "body".to_string(),
                texture_id: "atlas".to_string(),
                z_index: 0,
                opacity: 1.0,
                slot: None,
                variant: None,
                vertices: vec![
                    RigVertex {
                        position: RigPoint { x: 0.0, y: 0.0 },
                        uv: RigPoint { x: 0.0, y: 0.0 },
                        joints: [0, 0, 0, 0],
                        weights: [1.0, 0.0, 0.0, 0.0],
                    },
                    RigVertex {
                        position: RigPoint { x: 1.0, y: 0.0 },
                        uv: RigPoint { x: 1.0, y: 0.0 },
                        joints: [0, 0, 0, 0],
                        weights: [1.0, 0.0, 0.0, 0.0],
                    },
                    RigVertex {
                        position: RigPoint { x: 0.0, y: 1.0 },
                        uv: RigPoint { x: 0.0, y: 1.0 },
                        joints: [0, 0, 0, 0],
                        weights: [1.0, 0.0, 0.0, 0.0],
                    },
                ],
                indices: vec![0, 1, 2],
            }],
            motion_profile: None,
            outfit_profile: None,
            semantic_anchors: HashMap::new(),
            semantics: None,
            spatial_profile: None,
            anime25d_playback: None,
        }
    }

    #[test]
    fn package_identity_covers_provenance_and_manifest_but_not_runtime_url() {
        let atlas = b"atlas-pixels";
        let mut first = sample_manifest("/master-a.png", Some("a".repeat(64)));
        let first_id = package_id_for_manifest(atlas, &first).unwrap();
        assert_eq!(
            normalize_asset_id(&first_id).as_deref(),
            Some(first_id.as_str())
        );
        assert_eq!(normalize_asset_id("abc"), None);

        first.textures[0].url = "/api/merope/rig/assets/runtime-id".to_string();
        assert_eq!(package_id_for_manifest(atlas, &first).unwrap(), first_id);

        first.source_master_asset_id = Some("/master-b.png".to_string());
        assert_ne!(package_id_for_manifest(atlas, &first).unwrap(), first_id);

        first.source_master_asset_id = Some("/master-a.png".to_string());
        first.source_generation_fingerprint = Some("b".repeat(64));
        assert_ne!(package_id_for_manifest(atlas, &first).unwrap(), first_id);
    }
}
