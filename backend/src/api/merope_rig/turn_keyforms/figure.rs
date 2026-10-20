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
/// keyed, its turn and weight shift are made alongside (`body_turn`). A Space
/// without these endpoints, or a head big enough already, has the figure
/// decomposed whole.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_figure(
    id: &str,
    user_id: i32,
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
    let picture = enlarged(client, &reference).await;
    let cache = TurnCache::for_master(&picture.bytes, generation, space_name, options);
    update(id, |job| {
        job.stage = Stage::Decomposing;
        job.done = 0;
        job.total = if keys.head { 3 } else { 4 };
    });
    let whole = decomposition(
        client,
        &cache,
        0,
        &picture.bytes,
        &picture.media_type,
        options,
    )
    .await?;
    update(id, |job| job.done += 1);
    let drawn = (picture.bytes.to_vec(), picture.media_type.clone());
    let plan = client
        .figure_plan(whole.clone(), drawn.clone())
        .await
        .map_err(see_through_error)?;
    // The body's keys are made alongside the head's work: their drawings and
    // fit need only the figure's whole decomposition (body_turn).
    let body = async {
        if keys.body {
            Some(
                body_turn::body_turn_keys(
                    id, user_id, generation, &reference, &picture, &whole, client, &cache,
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
            if keys.head {
                update(id, |job| job.phase = Phase::Head);
                run(id, user_id, generation, picture.clone(), client, &cache).await
            } else {
                Ok(plain(whole.clone(), drawn.1.clone()))
            }
        };
        return with_body(id, head, body).await;
    };
    tracing::info!(job = %id, tiles = ?plan.tiles, "figure decomposed in tiles");
    let [upper, middle, lower] = plan.crops;
    let tiles = plan.tiles;
    let head = async {
        let middle = tile_decomposition(client, &cache, "middle", middle).await?;
        update(id, |job| job.done += 1);
        let lower = tile_decomposition(client, &cache, "lower", lower).await?;
        update(id, |job| job.done += 1);
        let upper = image_generation::ImageReference {
            bytes: axum::body::Bytes::from(upper),
            media_type: "image/png".to_string(),
        };
        let head_cache = TurnCache::for_master(&upper.bytes, generation, space_name, options);
        let (upper_psd, made) = if keys.head {
            // The bust's front decomposition is the upper tile's.
            update(id, |job| job.phase = Phase::Head);
            let made = run(id, user_id, generation, upper, client, &head_cache).await?;
            (made.files.decompositions[0].clone(), Some(made))
        } else {
            let psd = decomposition(
                client,
                &head_cache,
                0,
                &upper.bytes,
                &upper.media_type,
                options,
            )
            .await?;
            update(id, |job| job.done += 1);
            (psd, None)
        };
        prune_cache(cache_root(), CACHE_KEEP).await;
        update(id, |job| {
            job.phase = Phase::Figure;
            job.stage = Stage::Stitching;
            job.done = 0;
            job.total = 1;
        });
        let stitched = client
            .figure_stitch(
                drawn.clone(),
                whole.clone(),
                [upper_psd, middle, lower],
                &tiles,
            )
            .await
            .map_err(see_through_error)?;
        update(id, |job| job.done += 1);
        let Some(made) = made else {
            return Ok(plain(stitched, drawn.1.clone()));
        };
        let figure = FigureMeta {
            master: String::new(),
            master_media_type: drawn.1.clone(),
            crop_box: tiles.upper,
        };
        let kept = FigureFiles {
            master: drawn.0.clone(),
            decomposition: stitched,
        };
        put_back(client, made, figure, kept).await
    };
    with_body(id, head, body).await
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

/// The figure's picture enlarged (kept by the master, `figure_picture`); the
/// master as it is when the Space cannot enlarge it.
pub(super) async fn enlarged(
    client: &see_through::SeeThroughClient,
    reference: &image_generation::ImageReference,
) -> image_generation::ImageReference {
    let png = |bytes: Vec<u8>| image_generation::ImageReference {
        bytes: axum::body::Bytes::from(bytes),
        media_type: "image/png".to_string(),
    };
    if let Some(kept) = figure_picture::read(&reference.bytes).await {
        return png(kept);
    }
    match client
        .upscale((reference.bytes.to_vec(), reference.media_type.clone()))
        .await
    {
        Ok(Some(bytes)) => {
            figure_picture::write(&reference.bytes, &bytes).await;
            png(bytes)
        }
        Ok(None) => reference.clone(),
        Err(error) => {
            tracing::warn!(%error, "figure picture could not be enlarged; decomposed as drawn");
            reference.clone()
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
/// parts were only what the keys were fitted on.
pub(super) async fn put_back(
    client: &see_through::SeeThroughClient,
    made: Made,
    figure: FigureMeta,
    kept: FigureFiles,
) -> ApiResult<Made> {
    let keyforms = client
        .figure_keys(
            (kept.master.clone(), figure.master_media_type.clone()),
            kept.decomposition.clone(),
            made.files.keyforms,
            figure.crop_box,
        )
        .await
        .map_err(see_through_error)?;
    // The keys were fitted on the head's hair as the head's decomposition
    // split it (side locks in the front hair, the front hair in locks): the
    // figure takes that hair. Should the Space not manage, the figure keeps
    // its own, as before.
    let kept = match client
        .figure_hair(
            kept.decomposition.clone(),
            made.files.baked.clone(),
            keyforms.clone(),
        )
        .await
    {
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
    let mut attempt = 0;
    loop {
        match client
            .turn_keyforms(front.clone(), turned.clone(), pictures.clone())
            .await
        {
            Ok(output) => return Ok(output),
            Err(error) if attempt < TRANSIENT_RETRIES && transient(&error) => {
                attempt += 1;
                tracing::warn!(%error, attempt, "turn keyforms fit failed; retrying");
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Err(error) => return Err(see_through_error(error)),
        }
    }
}
