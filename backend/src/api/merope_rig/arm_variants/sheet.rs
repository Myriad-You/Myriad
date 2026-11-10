//! A full body's sheet: the window of its enlarged picture a gesture is drawn
//! on, and the drawn window laid back on that picture.

use super::*;

/// A full body's window, enlarged as its decomposition was, at about a bust's size.
pub(super) async fn figure_sheet(master: &image_generation::ImageReference) -> ApiResult<Sheet> {
    let enlarged = crate::api::merope_rig::turn_keyforms::enlarged_figure(master).await?;
    let enlarged = off_runtime(move || image::load_from_memory(&enlarged.bytes).ok())
        .await?
        .ok_or_else(portrait_required)?;
    figure_sheet_on(master, enlarged).await
}

/// The window of `enlarged` (the master enlarged) the pose model's arms on the master call for.
pub(super) async fn figure_sheet_on(
    master: &image_generation::ImageReference,
    enlarged: image::DynamicImage,
) -> ApiResult<Sheet> {
    let pose = pose_estimation::estimate(&master.bytes)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "full-body arm variant: the pose model could not look");
            no_figure()
        })?;
    let size = (enlarged.width(), enlarged.height());
    let keypoints = in_portrait(&pose.keypoints, (pose.width, pose.height), size);
    let found = figure::window(&keypoints, size).ok_or_else(no_figure)?;
    let drawn = figure::drawn_size(found);
    let window = figure::shaped(found, drawn, size);
    let enlarged = Arc::new(enlarged);
    let whole = enlarged.clone();
    let crop = off_runtime(move || figure::crop(&whole, window, drawn))
        .await?
        .ok_or_else(portrait_required)?;
    Ok(Sheet {
        reference: image_generation::ImageReference::new(crop, "image/png")
            .map_err(portrait_generation_provider_error)?,
        drawn,
        figure: Some((window, enlarged)),
    })
}

/// Picture work (a full body's enlarged picture is some 2304×4096) done off the threads requests are answered on.
async fn off_runtime<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> ApiResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| internal_error(error.to_string()))
}

fn no_figure() -> crate::api::merope_rig::ApiError {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({
            "error": "No arms were found on the full-body picture",
            "code": "skeleton_not_found"
        })),
    )
}

/// A full body's drawn window super-resolved as its picture was and laid back on
/// the enlarged picture, its points with it. Not super-resolved when the Space
/// cannot: stretched, softer than the figure about it.
pub(super) async fn laid_back(
    enlarged: &Arc<image::DynamicImage>,
    window: figure::Window,
    drawn: (u32, u32),
    png: &[u8],
    found: Found,
) -> ApiResult<(Vec<u8>, Found)> {
    let sharper = crate::api::merope_rig::turn_keyforms::super_resolved(png).await;
    let (whole, drawing) = (enlarged.clone(), sharper.unwrap_or_else(|| png.to_vec()));
    let composite = off_runtime(move || figure::lay_back(&whole, window, &drawing))
        .await?
        .ok_or_else(|| internal_error("full-body arm variant could not be laid back"))?;
    let hand: Vec<[f32; 3]> = found.hand.iter().map(|&[x, y]| [x, y, 1.0]).collect();
    Ok((
        composite,
        Found {
            hand: figure::on_picture(&hand, window, drawn)
                .into_iter()
                .map(|[x, y, _]| [x, y])
                .collect(),
            keypoints: figure::on_picture(&found.keypoints, window, drawn),
        },
    ))
}
