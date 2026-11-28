//! 全身站立的增强：转身两张图、超分分块拆层再拼回、拟合。

use super::*;

/// A standing figure's head is a couple of hundred pixels on a decomposition's
/// canvas: too little for a figure's detail, too small to fit turn keys well.
/// Its picture is enlarged (the Space's `upscale`), decomposed whole, planned
/// into three tiles (`figure_plan`: head and chest framed as a bust, waist,
/// legs), each tile decomposed on its own canvas, and the tiles stitched on
/// the whole (`figure_stitch`). With the head keyed, the upper tile is keyed
/// as a bust (`run`, on its own cache, its front decomposition the tile's)
/// and the keys placed on the stitched figure (`figure_keys`); with the body
/// keyed, its turn and weight shift are made alongside (`body_turn`). A keyed
/// head's decompositions go first: the other two tiles' and the body's wait
/// for them (`head_first`). A Space without these endpoints, or a head big
/// enough already, has the figure decomposed whole.
///
/// The job counts the enlarging, then the whole decomposition as one of one
/// with the head keyed (its stages follow, then the stitching) and as the
/// first of four with the three tiles counted after it.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_figure(
    id: &str,
    storage: &rig_jobs::Storage,
    generation: &image_generation::ImageGenerationConfig,
    reference: image_generation::ImageReference,
    client: &see_through::SeeThroughClient,
    space_name: &str,
    keys: FigureKeys,
) -> ApiResult<Made> {
    let options = see_through::DecomposeOptions::default();
    update(id, |job| {
        job.stage = Stage::Upscaling;
        job.done = 0;
        job.total = 1;
    });
    let picture = timed(id, "upscale", enlarged(client, &reference, storage)).await;
    let cache = TurnCache::for_master(storage, &picture.bytes, generation, space_name, options);
    update(id, |job| {
        job.stage = Stage::Decomposing;
        job.done = 0;
        job.total = if keys.head { 1 } else { 4 };
    });
    let whole = decomposition(
        client,
        &cache,
        0,
        &picture.bytes,
        &picture.media_type,
        options,
    );
    let whole = timed(id, "whole", whole).await?;
    update(id, |job| job.done += 1);
    let drawn = (picture.bytes.to_vec(), picture.media_type.clone());
    let plan = retried("plan", || client.figure_plan(whole.clone(), drawn.clone()));
    let plan = timed(id, "plan", plan).await.map_err(see_through_error)?;
    // A keyed head's decompositions are what the others give way to.
    let (split, way) = if keys.head {
        let (split, way) = head_first();
        (Some(split), way)
    } else {
        (None, Way::open())
    };
    // The body's keys are made alongside the head's work: their drawings and
    // fit need only the figure's whole decomposition (body_turn).
    let body = async {
        if keys.body {
            Some(
                body_turn::body_turn_keys(
                    id, generation, &reference, &picture, &whole, client, &cache, &way,
                )
                .await,
            )
        } else {
            None
        }
    };
    let Some(plan) = plan else {
        tracing::info!(job = %id, "figure decomposed whole");
        let head = async {
            match split {
                Some(split) => {
                    update(id, |job| job.phase = Phase::Head);
                    run(id, generation, picture.clone(), client, &cache, split).await
                }
                None => Ok(plain(whole.clone(), drawn.1.clone())),
            }
        };
        return with_body(id, head, body).await;
    };
    tracing::info!(job = %id, tiles = ?plan.tiles, "figure decomposed in tiles");
    let [upper, middle, lower] = plan.crops;
    let upper = image_generation::ImageReference {
        bytes: axum::body::Bytes::from(upper),
        media_type: "image/png".to_string(),
    };
    let head_cache = TurnCache::for_master(storage, &upper.bytes, generation, space_name, options);
    let steps = Tiles {
        id,
        storage,
        generation,
        client,
        cache: &cache,
        head_cache: &head_cache,
        drawn: &drawn,
        whole: &whole,
        tiles: plan.tiles,
        upper,
        rest: [middle, lower],
    };
    with_body(id, in_tiles(id, &steps, split, &way), body).await
}

/// A figure's tiles as a job makes them (`in_tiles`).
struct Tiles<'a> {
    id: &'a str,
    storage: &'a rig_jobs::Storage,
    generation: &'a image_generation::ImageGenerationConfig,
    client: &'a see_through::SeeThroughClient,
    /// The figure's own cache: its whole decomposition, the tiles without a head.
    cache: &'a TurnCache,
    /// The upper tile's: the head's drawings and decompositions.
    head_cache: &'a TurnCache,
    /// The figure's picture, and its decomposition whole.
    drawn: &'a (Vec<u8>, String),
    whole: &'a [u8],
    tiles: see_through::FigureTiles,
    /// The pictures the tiles are decomposed from: the upper one, then the
    /// waist's and the legs'.
    upper: image_generation::ImageReference,
    rest: [Vec<u8>; 2],
}

/// The tiles without a head, as the cache names them.
const HEADLESS_TILES: [&str; 2] = ["middle", "lower"];

impl TileSteps for Tiles<'_> {
    async fn head(&self, split: HeadSplit) -> ApiResult<Made> {
        run(
            self.id,
            self.generation,
            self.upper.clone(),
            self.client,
            self.head_cache,
            split,
        )
        .await
    }

    async fn upper(&self) -> ApiResult<Vec<u8>> {
        decomposition(
            self.client,
            self.head_cache,
            0,
            &self.upper.bytes,
            &self.upper.media_type,
            see_through::DecomposeOptions::default(),
        )
        .await
    }

    async fn tile(&self, at: usize) -> ApiResult<Vec<u8>> {
        let picture = self.rest[at].clone();
        tile_decomposition(self.client, self.cache, HEADLESS_TILES[at], picture).await
    }

    async fn stitch(&self, tiles: [Vec<u8>; 3]) -> ApiResult<Vec<u8>> {
        prune_cache(cache_root(self.storage), CACHE_KEEP).await;
        retried("stitch", || {
            self.client.figure_stitch(
                self.drawn.clone(),
                self.whole.to_vec(),
                tiles.clone(),
                &self.tiles,
            )
        })
        .await
        .map_err(see_through_error)
    }

    async fn finish(&self, made: Option<Made>, stitched: Vec<u8>) -> ApiResult<Made> {
        let Some(made) = made else {
            return Ok(plain(stitched, self.drawn.1.clone()));
        };
        let figure = FigureMeta {
            master: String::new(),
            master_media_type: self.drawn.1.clone(),
            crop_box: self.tiles.upper,
        };
        let kept = FigureFiles {
            master: self.drawn.0.clone(),
            decomposition: stitched,
        };
        put_back(self.id, self.client, made, figure, kept).await
    }
}

/// The head's work with the body's keys made alongside (`alongside`) and
/// put beside the head's in its keys JSON.
pub(super) async fn with_body(
    id: &str,
    head: impl Future<Output = ApiResult<Made>>,
    body: impl Future<Output = Option<body_turn::BodyKeys>>,
) -> ApiResult<Made> {
    let (made, body) = alongside(id, head, body).await;
    let mut made = made?;
    made.files.keyforms = body_turn::with_body_keys(made.files.keyforms, body.flatten().as_ref());
    Ok(made)
}

/// The head's work and the body's turn, run together. The job reports the
/// head's stages; done first, it then waits on the body as fitting. A failed
/// head does not wait: the body is dropped with it.
pub(super) async fn alongside<B: Future>(
    id: &str,
    head: impl Future<Output = ApiResult<Made>>,
    body: B,
) -> (ApiResult<Made>, Option<B::Output>) {
    tokio::pin!(head);
    tokio::pin!(body);
    let mut body_out = None;
    let head_out = loop {
        tokio::select! {
            out = &mut body, if body_out.is_none() => body_out = Some(out),
            out = &mut head => break out,
        }
    };
    if head_out.is_err() {
        return (head_out, None);
    }
    let body_out = match body_out {
        Some(out) => out,
        None => {
            update(id, |job| {
                job.phase = Phase::Body;
                job.stage = Stage::Fitting;
                job.done = 0;
                job.total = 1;
            });
            body.await
        }
    };
    (head_out, Some(body_out))
}

/// A plain figure: its decomposition, no keys (`null`), nothing kept.
pub(super) fn plain(psd: Vec<u8>, media_type: String) -> Made {
    Made {
        files: ArchiveFiles {
            master: Vec::new(),
            turned: Default::default(),
            decompositions: Default::default(),
            keyforms: b"null".to_vec(),
            baked: psd,
            figure: None,
        },
        master_media_type: media_type,
        redrawn: Vec::new(),
        figure: None,
        keep: false,
    }
}

/// The figure's picture enlarged (kept by the master in the owner's storage,
/// `figure_picture`); the master as it is when the Space cannot enlarge it.
pub(super) async fn enlarged(
    client: &see_through::SeeThroughClient,
    reference: &image_generation::ImageReference,
    storage: &rig_jobs::Storage,
) -> image_generation::ImageReference {
    let png = |bytes: Vec<u8>| image_generation::ImageReference {
        bytes: axum::body::Bytes::from(bytes),
        media_type: "image/png".to_string(),
    };
    if let Some(kept) = figure_picture::read(storage, &reference.bytes).await {
        return png(kept);
    }
    // One at a time: a set's gestures are asked for together with its decomposition, and the first to come
    // enlarges the picture for all of them. A picture that could not be enlarged just now is not tried again
    // by the ones that waited behind it: they go on with it as drawn.
    const RETRIED_AFTER: std::time::Duration = std::time::Duration::from_secs(120);
    static ENLARGING: tokio::sync::Mutex<Option<(u64, std::time::Instant)>> =
        tokio::sync::Mutex::const_new(None);
    let mut enlarging = ENLARGING.lock().await;
    if let Some(kept) = figure_picture::read(storage, &reference.bytes).await {
        return png(kept);
    }
    let master = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        reference.bytes.hash(&mut hasher);
        hasher.finish()
    };
    if matches!(*enlarging, Some((failed, at)) if failed == master && at.elapsed() < RETRIED_AFTER)
    {
        return reference.clone();
    }
    let upscale = || client.upscale((reference.bytes.to_vec(), reference.media_type.clone()));
    match upscaled(upscale).await {
        Some(bytes) => {
            figure_picture::write(storage, &reference.bytes, &bytes).await;
            png(bytes)
        }
        None => {
            *enlarging = Some((master, std::time::Instant::now()));
            reference.clone()
        }
    }
}

/// What the Space's `upscale` made of a figure's picture, a passing failure
/// tried again (`retried_when`). A time-out is not one of them here: it is
/// the longest a call waits, and the set's gestures wait behind this picture
/// meanwhile. None when the figure is to be decomposed as drawn, with a line
/// telling which it was: a Space without the endpoint, or an enlargement
/// that failed, and after how many tries.
pub(super) async fn upscaled<C>(upscale: impl FnMut() -> C) -> Option<Vec<u8>>
where
    C: Future<Output = Result<Option<Vec<u8>>, see_through::SeeThroughError>>,
{
    let passing = |error: &see_through::SeeThroughError| {
        transient(error) && !matches!(error, see_through::SeeThroughError::Timeout)
    };
    match retried_when("upscale", passing, upscale).await {
        Ok(Some(bytes)) => Some(bytes),
        Ok(None) => {
            tracing::info!("the Space has no upscale endpoint; figure decomposed as drawn");
            None
        }
        Err(error) => {
            let tries = if passing(&error) {
                TRANSIENT_RETRIES + 1
            } else {
                1
            };
            tracing::warn!(%error, tries, "figure picture could not be enlarged; decomposed as drawn");
            None
        }
    }
}

/// A tile without a head (waist, legs) decomposed once, kept in the figure's
/// cache. Not checked: the check is of a face.
pub(super) async fn tile_decomposition(
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    name: &str,
    picture: Vec<u8>,
) -> ApiResult<Vec<u8>> {
    let file = format!("tile-{name}.psd");
    if let Some(psd) = cache.read(&file).await.filter(|psd| !psd.is_empty()) {
        return Ok(psd);
    }
    let psd = decompose_waiting(
        client,
        axum::body::Bytes::from(picture),
        "image/png",
        see_through::DecomposeOptions::default(),
    )
    .await?;
    cache.write(&file, &psd).await;
    Ok(psd)
}

/// The head's keys placed on the figure (`figure_keys`), with the figure's
/// own decomposition but the head's hair (`figure_hair`): the head's other
/// parts were only what the keys were fitted on. Either call is tried again
/// after a passing failure (`retried`).
pub(super) async fn put_back(
    id: &str,
    client: &see_through::SeeThroughClient,
    made: Made,
    figure: FigureMeta,
    kept: FigureFiles,
) -> ApiResult<Made> {
    let placing = retried("keys", || {
        client.figure_keys(
            (kept.master.clone(), figure.master_media_type.clone()),
            kept.decomposition.clone(),
            made.files.keyforms.clone(),
            figure.crop_box,
        )
    });
    let keyforms = timed(id, "keys", placing)
        .await
        .map_err(see_through_error)?;
    // The keys were fitted on the head's hair as the head's decomposition
    // split it (side locks in the front hair, the front hair in locks): the
    // figure takes that hair. Should the Space not manage, the figure keeps
    // its own, as before.
    let hair = retried("hair", || {
        client.figure_hair(
            kept.decomposition.clone(),
            made.files.baked.clone(),
            keyforms.clone(),
        )
    });
    let hair = timed(id, "hair", hair).await;
    let kept = match hair {
        Ok(decomposition) => FigureFiles {
            decomposition,
            ..kept
        },
        Err(error) => {
            tracing::warn!(%error, "the figure keeps its own hair");
            kept
        }
    };
    Ok(Made {
        files: ArchiveFiles {
            keyforms,
            baked: kept.decomposition.clone(),
            figure: Some(kept),
            ..made.files
        },
        figure: Some(figure),
        ..made
    })
}

/// The Space's fit of the four turned decompositions onto the front one,
/// matched against the five pictures; passing failures are tried again.
pub(super) async fn fit(
    client: &see_through::SeeThroughClient,
    decompositions: &[Vec<u8>; 5],
    pictures: &[(Vec<u8>, String); 5],
) -> ApiResult<see_through::TurnKeyformsOutput> {
    let [front, turned @ ..] = decompositions.clone();
    retried("fit", || {
        client.turn_keyforms(front.clone(), turned.clone(), pictures.clone())
    })
    .await
    .map_err(see_through_error)
}

/// A full body's master enlarged as its decomposition is (made and kept now
/// in the site's storage if it was not yet), for a gesture arm drawn on it.
pub(in crate::api::merope_rig) async fn enlarged_figure(
    master: &image_generation::ImageReference,
) -> ApiResult<image_generation::ImageReference> {
    let site = rig_jobs::Storage::site();
    if let Some(kept) = figure_picture::read(&site, &master.bytes).await {
        return Ok(image_generation::ImageReference {
            bytes: axum::body::Bytes::from(kept),
            media_type: "image/png".to_string(),
        });
    }
    let (client, _) = space_client().await?;
    Ok(enlarged(&client, master, &site).await)
}

/// A picture enlarged by the same super-resolution as a full body's; None when
/// the Space cannot (not configured, no such endpoint, failed).
pub(in crate::api::merope_rig) async fn super_resolved(picture: &[u8]) -> Option<Vec<u8>> {
    let (client, _) = space_client().await.ok()?;
    match client
        .upscale((picture.to_vec(), "image/png".to_string()))
        .await
    {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(%error, "picture could not be super-resolved");
            None
        }
    }
}

#[cfg(test)]
#[path = "figure_tests.rs"]
mod tests;
