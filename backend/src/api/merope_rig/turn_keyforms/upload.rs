//! 上传增强素材：下载下来的存档 zip（或同样的文件重新打包）分块传回，读出后
//! 核对是当前槽位立绘的那一套，存成一份新存档，之后照常载入（不再生图、拆层）。
//! 一份存档一百多 MB，一次传完会撞上反向代理和 CDN 的请求体上限，所以分块。

use axum::extract::Query;

use super::*;

/// One piece of an upload, at most this large.
pub const UPLOAD_PIECE_MAX: usize = 9 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct UploadPiece {
    /// Where the piece goes: the bytes received so far.
    offset: u64,
}

/// Appends one piece of an upload; a piece that does not follow the last is
/// refused with how much was received, so the page can go on from there.
pub async fn put_turn_archive_piece(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(upload): Path<String>,
    Query(piece): Query<UploadPiece>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    use tokio::io::AsyncWriteExt;

    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let path = archive::upload_path(&upload).ok_or_else(|| bad_request("Invalid upload id"))?;
    if piece.offset == 0 {
        archive::prune_uploads().await;
        tokio::fs::create_dir_all(archive::archive_root())
            .await
            .map_err(internal_error)?;
        let _ = tokio::fs::remove_file(&path).await;
    }
    let received = match tokio::fs::metadata(&path).await {
        Ok(meta) => meta.len(),
        Err(_) => 0,
    };
    if received != piece.offset {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "The piece does not follow the last one",
                "code": "turn_archive_upload_offset",
                "received": received,
            })),
        ));
    }
    if received + body.len() as u64 > archive::UPLOAD_MAX {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(bad_request("The upload is too large"));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .map_err(internal_error)?;
    file.write_all(&body).await.map_err(internal_error)?;
    file.flush().await.map_err(internal_error)?;
    Ok(Json(json!({ "received": received + body.len() as u64 })))
}

pub async fn finish_turn_archive_upload(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(upload): Path<String>,
) -> ApiResult<Json<Value>> {
    finish_upload(db, &claims, None, upload).await
}

pub async fn finish_full_body_turn_archive_upload(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((outfit_id, upload)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    finish_upload(db, &claims, Some(outfit_id), upload).await
}

/// Reads the whole upload back and keeps it for the slot's current portrait:
/// the kept job must be a bust's for the bust and a figure's for a set, and
/// drawn from the same picture (a figure's enlarged picture counts). It is
/// kept as made for that portrait, so it is loaded as any of its own.
async fn finish_upload(
    db: DatabaseConnection,
    claims: &Claims,
    outfit_id: Option<String>,
    upload: String,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(claims, &db).await?;
    let path = archive::upload_path(&upload).ok_or_else(|| bad_request("Invalid upload id"))?;
    let read = path.clone();
    let unpacked = tokio::task::spawn_blocking(move || archive::unpack_blocking(&read))
        .await
        .map_err(internal_error)?;
    let _ = tokio::fs::remove_file(&path).await;
    let (kept, files) =
        unpacked.map_err(|message| refused(&message, "turn_archive_upload_invalid"))?;
    if kept.outfit_id.is_some() != outfit_id.is_some() {
        return Err(refused(
            if outfit_id.is_some() {
                "These are a bust's enhancement, not a full-body set's"
            } else {
                "These are a full-body set's enhancement, not the bust's"
            },
            "turn_archive_upload_kind",
        ));
    }
    let master = super::super::master::current_master(&db, slot_of(outfit_id.as_deref()))
        .await?
        .ok_or_else(|| not_found("Site portrait is missing"))?;
    let reference = image_generation::load_local_reference(&master.asset_id)
        .await
        .map_err(|_| bad_request("The current master portrait is not available"))?;
    let drawn = files
        .figure
        .as_ref()
        .map_or(&files.master, |figure| &figure.master)
        .clone();
    let current = reference.bytes.clone();
    let same = tokio::task::spawn_blocking(move || archive::same_picture(&current, &drawn))
        .await
        .map_err(internal_error)?;
    if !same {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "These enhancements were made from another portrait",
                "code": "turn_archive_other_portrait"
            })),
        ));
    }
    // A figure's decomposition was cut from its enlarged picture: the
    // importer checks it against that picture, kept by the master.
    if let Some(figure) = &files.figure
        && figure_picture::read(&reference.bytes).await.is_none()
    {
        figure_picture::write(&reference.bytes, &figure.master).await;
    }
    let uploaded_id = kept.id.clone();
    let meta = ArchiveMeta {
        id: archive::new_id(),
        created_at: chrono::Utc::now(),
        outfit_id,
        source_master_asset_id: master.asset_id.clone(),
        source_generation_fingerprint: master.generation_fingerprint.clone(),
        parent: None,
        bytes: 0,
        ..kept
    };
    let id = archive::save(meta, &files).await.map_err(internal_error)?;
    tracing::info!(archive = %id, uploaded = %uploaded_id, "turn keys archive uploaded");
    Ok(Json(json!({ "id": id })))
}

fn refused(message: &str, code: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message, "code": code })),
    )
}
