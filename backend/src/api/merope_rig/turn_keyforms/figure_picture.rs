//! A standing figure's picture enlarged by the Space's super-resolution
//! before it is decomposed in tiles, kept by the master's own pixels: the
//! decomposition is cut from it, so the importer checks it against the same
//! picture. The master itself stays as it was drawn.

use std::path::{Path, PathBuf};

use axum::{
    Json,
    body::Body,
    extract::{Extension, Path as UrlPath, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use sea_orm::DatabaseConnection;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    api::merope_rig::{
        ApiError, ApiResult, internal_error,
        master::{MasterSlot, current_master},
        require_merope_enabled, require_owner,
    },
    middleware::auth::Claims,
    services::{image_generation, rig_jobs::Storage},
};

/// Enlarged pictures kept, the newest; one per figure master in use.
const KEEP: usize = 8;

/// Where an owner's enlarged pictures are kept.
fn root(storage: &Storage) -> PathBuf {
    storage.root().join("figure-picture")
}

fn file_in(root: &Path, master: &[u8]) -> PathBuf {
    let digest = Sha256::digest(master);
    let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    root.join(format!("{name}.png"))
}

/// The enlarged picture of a master, if one was made.
pub(super) async fn read(storage: &Storage, master: &[u8]) -> Option<Vec<u8>> {
    read_in(&root(storage), master).await
}

async fn read_in(root: &Path, master: &[u8]) -> Option<Vec<u8>> {
    tokio::fs::read(file_in(root, master)).await.ok()
}

/// Keeps a master's enlarged picture, dropping the oldest made past KEEP.
pub(super) async fn write(storage: &Storage, master: &[u8], picture: &[u8]) {
    write_in(&root(storage), master, picture).await
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

/// A full-body set's picture as its tiles were cut from (enlarged), for the
/// importer to check its decomposition against; 404 while there is none.
pub async fn get_full_body_figure_picture(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    UrlPath(outfit_id): UrlPath<String>,
) -> ApiResult<Response> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let master = current_master(&db, MasterSlot::FullBody(&outfit_id))
        .await?
        .ok_or_else(missing_picture)?;
    let reference = image_generation::load_local_reference(&master.asset_id)
        .await
        .map_err(|_| missing_picture())?;
    let picture = read(&Storage::site(), &reference.bytes)
        .await
        .ok_or_else(missing_picture)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .body(Body::from(picture))
        .map_err(internal_error)
}

fn missing_picture() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(
            json!({ "error": "No enlarged picture for this set", "code": "figure_picture_missing" }),
        ),
    )
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
