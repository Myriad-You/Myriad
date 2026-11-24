//! A full body's sheet: the window of its enlarged picture a gesture is drawn
//! on, and the drawn window laid back on that picture.

use std::{collections::BTreeMap, sync::Weak};

use super::*;

/// The sheets gestures are being drawn on now, by their set's master picture.
type Sheets<T> = tokio::sync::Mutex<BTreeMap<String, Weak<T>>>;
static SHEETS: Sheets<Sheet> = tokio::sync::Mutex::const_new(BTreeMap::new());

/// A full body's sheet, one for the gestures of a set drawn together: all of them are drawn on the same window,
/// so its enlarged picture (some 2304×4096) is read, decoded, cut, sized and encoded once, not once for each.
pub(super) async fn figure_sheet(
    master_id: &str,
    master: &image_generation::ImageReference,
) -> ApiResult<Arc<Sheet>> {
    shared(&SHEETS, master_id, own_figure_sheet(master)).await
}

/// What `make` makes for `key`, or what is still held of it: shared for as long as someone holds it, and made
/// anew after that, from whatever its picture is then. One is made at a time; the ones asking meanwhile wait for
/// it, and one that could not be made is tried again by the next.
async fn shared<T>(
    held: &Sheets<T>,
    key: &str,
    make: impl Future<Output = ApiResult<T>>,
) -> ApiResult<Arc<T>> {
    let mut held = held.lock().await;
    held.retain(|_, sheet| sheet.strong_count() > 0);
    if let Some(sheet) = held.get(key).and_then(Weak::upgrade) {
        return Ok(sheet);
    }
    let sheet = Arc::new(make.await?);
    held.insert(key.to_string(), Arc::downgrade(&sheet));
    Ok(sheet)
}

/// A full body's window, enlarged as its decomposition was, at about a bust's size.
async fn own_figure_sheet(master: &image_generation::ImageReference) -> ApiResult<Sheet> {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn gestures_drawn_together_share_one_sheet_and_later_ones_get_a_new_one() {
        let held: Sheets<usize> = tokio::sync::Mutex::new(BTreeMap::new());
        let made = &AtomicUsize::new(0);
        let make = || async move {
            Ok::<_, crate::api::merope_rig::ApiError>(made.fetch_add(1, Ordering::SeqCst))
        };
        let asked = (0..8).map(|_| shared(&held, "master", make()));
        let together: Vec<Arc<usize>> = futures::future::join_all(asked)
            .await
            .into_iter()
            .map(|sheet| sheet.unwrap())
            .collect();
        assert_eq!(made.load(Ordering::SeqCst), 1);
        assert!(
            together
                .iter()
                .all(|sheet| Arc::ptr_eq(sheet, &together[0]))
        );
        // Another set's is its own.
        assert_eq!(*shared(&held, "other", make()).await.unwrap(), 1);
        // Still held by one gesture: the same sheet; held by none: made anew.
        let last = together.into_iter().next_back().unwrap();
        assert_eq!(*shared(&held, "master", make()).await.unwrap(), 0);
        drop(last);
        assert_eq!(*shared(&held, "master", make()).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn a_sheet_that_could_not_be_made_is_not_kept() {
        let held: Sheets<usize> = tokio::sync::Mutex::new(BTreeMap::new());
        let failed = shared(&held, "master", async { Err(no_figure()) }).await;
        assert_eq!(failed.unwrap_err().0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(*shared(&held, "master", async { Ok(7) }).await.unwrap(), 7);
    }
}
