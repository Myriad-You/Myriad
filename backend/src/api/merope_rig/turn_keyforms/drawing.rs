//! 生图与拆层：转头图的生成、评判后重画，拆层的检查、重试与等待。

use super::*;

const KEEP: &str = "Edit this exact illustration. Keep the canvas size, framing, body, shoulders, \
neck base, outfit, colors, line style and shading exactly the same and pixel-aligned with the \
original, on the same background.";
const HEAD_ALONE: &str = "The head stays the same size and stays attached at the same neck \
position; the hair and every hair accessory and earring move with the head and keep their exact \
design, every lock of hair the same lock seen from the new angle: the bangs are not restyled, \
tucked or parted anew, and hair covering an eye still covers it. Same facial expression and the \
same eye openness. Clean finished art in the original's style.";

/// The four turned drawings, in the Space's order: toward image right and
/// left (Myriad's angleX ±1), raised and lowered (angleY ±1).
pub(super) fn turn_prompts() -> [String; 4] {
    let turn = |side: &str| {
        format!(
            "{KEEP} Change only the head: the character turns the head about 30 degrees toward \
             the viewer's {side}, a natural three-quarter view, the eyes looking where the face \
             points. {HEAD_ALONE}"
        )
    };
    let nod = |motion: &str| {
        format!(
            "{KEEP} Change only the head: the character {motion} about 30 degrees, facing \
             straight toward the viewer with no turn to either side and no tilt, the eyes looking \
             where the face points. {HEAD_ALONE}"
        )
    };
    [
        turn("right"),
        turn("left"),
        nod("tilts the head back, the face looking upward,"),
        nod("lowers the head, the face looking downward,"),
    ]
}

/// One turned drawing of the master, at the master's size.
pub(super) async fn draw_turn(
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    prompt: &str,
    reference: &image_generation::ImageReference,
    width: u32,
    height: u32,
) -> ApiResult<Vec<u8>> {
    let generated = crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "turn-reference",
        image_generation::generate_image(generation, prompt, width, height, Some(reference)),
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    let (bytes, _) = image_generation::load_generated_bytes(generated)
        .await
        .map_err(portrait_generation_provider_error)?;
    // The provider may answer at its own size; lined up means the master's.
    fit_to(&bytes, width, height)
}

/// The sides of the turned drawings, in their order.
pub(super) const TURN_SIDES: [&str; 4] = ["plus", "minus", "up", "down"];

/// Draws again, once each, the turned drawings the Space's `check_turns`
/// finds redrawn past what keys of the front drawing can follow (a restyled
/// fringe, a moved clip). A check or a drawing that fails keeps what there is:
/// the turn is then fitted as drawn.
#[allow(clippy::too_many_arguments)]
pub(super) async fn redraw_bad_turns(
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    reference: &image_generation::ImageReference,
    (width, height): (u32, u32),
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    front_psd: &[u8],
    mut turned: Vec<Vec<u8>>,
) -> Vec<Vec<u8>> {
    let pictures: [(Vec<u8>, String); 5] =
        std::iter::once((reference.bytes.to_vec(), reference.media_type.clone()))
            .chain(
                turned
                    .iter()
                    .map(|png| (png.clone(), "image/png".to_string())),
            )
            .collect::<Vec<_>>()
            .try_into()
            .expect("five pictures");
    let check = match client
        .check_turned_pictures(front_psd.to_vec(), pictures)
        .await
    {
        Ok(check) => check,
        Err(error) => {
            tracing::warn!(%error, "turned drawings check failed; keeping the drawings");
            return turned;
        }
    };
    let prompts = turn_prompts();
    for side in &check.bad {
        let Some(index) = TURN_SIDES.iter().position(|known| known == side) else {
            continue;
        };
        let redrawn = format!("turned-{index}.redrawn");
        if cache.read(&redrawn).await.is_some() {
            continue;
        }
        tracing::info!(side, residual = ?check.residual, "turned drawing drawn again");
        match draw_turn(
            user_id,
            generation,
            &prompts[index],
            reference,
            width,
            height,
        )
        .await
        {
            Ok(png) => {
                cache.write(&format!("turned-{index}.png"), &png).await;
                // Its decomposition was of the drawing it replaces (an empty entry reads as none).
                cache
                    .write(&format!("decomposition-{}.psd", index + 1), b"")
                    .await;
                cache
                    .write(&format!("decomposition-{}.checked", index + 1), b"")
                    .await;
                cache.write(&redrawn, b"redrawn").await;
                turned[index] = png;
            }
            Err(error) => tracing::warn!(side, ?error, "turned drawing could not be drawn again"),
        }
    }
    turned
}

/// Draws once more the turn whose front hair the fit could not follow
/// (hair_misfit), decomposes and fits it again, and keeps whichever fit
/// follows that turn's front hair better. Once per drawing set: a cache
/// marker keeps a rerun from drawing it yet again.
#[allow(clippy::too_many_arguments)]
pub(super) async fn redraw_misfit(
    id: &str,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    reference: &image_generation::ImageReference,
    (width, height): (u32, u32),
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    options: see_through::DecomposeOptions,
    (decompositions, pictures, output): (
        [Vec<u8>; 5],
        [(Vec<u8>, String); 5],
        see_through::TurnKeyformsOutput,
    ),
    redrawn: &mut Vec<String>,
) -> ApiResult<(
    [Vec<u8>; 5],
    [(Vec<u8>, String); 5],
    see_through::TurnKeyformsOutput,
)> {
    let Some(index) = hair_misfit::misfit_side(&output.keyforms) else {
        return Ok((decompositions, pictures, output));
    };
    let side = TURN_SIDES[index];
    let marker = format!("turned-{index}.hair-retried");
    if cache.read(&marker).await.is_some() {
        return Ok((decompositions, pictures, output));
    }
    cache.write(&marker, b"retried").await;
    let before = hair_misfit::hair_outline(&output.keyforms, side).unwrap_or(f64::INFINITY);
    tracing::info!(
        side,
        before,
        "turned drawing's front hair not followed; drawing it again"
    );
    let png = match draw_turn(
        user_id,
        generation,
        &turn_prompts()[index],
        reference,
        width,
        height,
    )
    .await
    {
        Ok(png) => png,
        Err(error) => {
            tracing::warn!(side, ?error, "turned drawing could not be drawn again");
            return Ok((decompositions, pictures, output));
        }
    };
    update(id, |job| {
        job.stage = Stage::Decomposing;
        job.done = 0;
        job.total = 1;
    });
    // Decomposed apart from the cached one, which stays the drawing's until this one wins.
    let bytes = axum::body::Bytes::from(png.clone());
    let psd = decompose_checked(client, &bytes, "image/png", options, &TURNED_REDOS, None)
        .await?
        .0;
    let mut retried_decompositions = decompositions.clone();
    retried_decompositions[index + 1] = psd.clone();
    let mut retried_pictures = pictures.clone();
    retried_pictures[index + 1] = (png.clone(), "image/png".to_string());
    update(id, |job| {
        job.stage = Stage::Fitting;
        job.done = 0;
        job.total = 1;
    });
    let retried = fit(client, &retried_decompositions, &retried_pictures).await?;
    let after = hair_misfit::hair_outline(&retried.keyforms, side).unwrap_or(f64::INFINITY);
    if after >= before {
        tracing::info!(
            side,
            before,
            after,
            "the turn drawn again was no better; kept the first"
        );
        return Ok((decompositions, pictures, output));
    }
    tracing::info!(
        side,
        before,
        after,
        "the turn drawn again is followed better; kept it"
    );
    cache.write(&format!("turned-{index}.png"), &png).await;
    cache
        .write(&format!("decomposition-{}.psd", index + 1), &psd)
        .await;
    cache
        .write(&format!("decomposition-{}.checked", index + 1), b"checked")
        .await;
    cache
        .write(&format!("turned-{index}.redrawn"), b"redrawn")
        .await;
    if !redrawn.iter().any(|known| known == side) {
        redrawn.push(side.to_string());
    }
    Ok((retried_decompositions, retried_pictures, retried))
}

/// A drawing's decomposition, checked (decompose_checked) and kept in the cache.
pub(super) async fn decomposition(
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    index: usize,
    bytes: &axum::body::Bytes,
    media_type: &str,
    options: see_through::DecomposeOptions,
) -> ApiResult<Vec<u8>> {
    let name = format!("decomposition-{index}.psd");
    let checked = format!("decomposition-{index}.checked");
    let cached = cache.read(&name).await;
    let redos: &[(bool, u16)] = if index == 0 {
        &FRONT_REDOS
    } else {
        &TURNED_REDOS
    };
    Ok(match cached {
        Some(psd) if cache.read(&checked).await.is_some() => psd,
        cached => {
            let (psd, settled) =
                decompose_checked(client, bytes, media_type, options, redos, cached).await?;
            cache.write(&name, &psd).await;
            if settled {
                cache.write(&checked, b"checked").await;
            }
            psd
        }
    })
}

/// How a faulty decomposition is redone, in turn (hair pass, seed). The
/// front one is the rig's own layers: other seeds first, as a decomposition
/// with the hair from a pass of its own keeps the picture less closely at
/// rest. A turned one only guides the fit: the hair pass first, which mended
/// every turned one so far where new seeds did not.
pub(super) const FRONT_REDOS: [(bool, u16); 3] = [(false, 7), (false, 1234), (true, 42)];
pub(super) const TURNED_REDOS: [(bool, u16); 2] = [(true, 42), (true, 7)];

/// A decomposition the fit can key from. One the Space's check finds faulty
/// (the face took in the hair, the headwear the outfit) is redone (`redos`);
/// when none passes, the least faulty. Settled when the check had its say (a
/// check that could not run is asked again next time).
pub(super) async fn decompose_checked(
    client: &see_through::SeeThroughClient,
    image: &axum::body::Bytes,
    media_type: &str,
    options: see_through::DecomposeOptions,
    redos: &[(bool, u16)],
    mut cached: Option<Vec<u8>>,
) -> ApiResult<(Vec<u8>, bool)> {
    let mut best: Option<(f64, Vec<u8>)> = None;
    for &(hair_pass, seed) in std::iter::once(&(options.hair_pass, options.seed)).chain(redos) {
        let psd = match cached.take() {
            Some(psd) => psd,
            None => {
                let options = see_through::DecomposeOptions {
                    hair_pass,
                    seed,
                    ..options
                };
                decompose_waiting(client, image.clone(), media_type, options).await?
            }
        };
        let check = match client.check_decomposition(psd.clone()).await {
            Ok(check) => check,
            Err(error) => {
                tracing::warn!(%error, "decomposition check failed; keeping the decomposition");
                return Ok((psd, false));
            }
        };
        if check.ok {
            // A Space without the check passes everything unmeasured.
            return Ok((psd, check.badness.is_some()));
        }
        tracing::warn!(seed, hair_pass, faults = ?check.faults, badness = ?check.badness, "faulty decomposition");
        let fault = check.badness.unwrap_or(f64::INFINITY);
        if best.as_ref().is_none_or(|(least, _)| fault < *least) {
            best = Some((fault, psd));
        }
    }
    Ok((best.expect("at least one decomposition").1, true))
}

/// A decomposition, waiting its turn while another one holds the Space.
pub(super) async fn decompose_waiting(
    client: &see_through::SeeThroughClient,
    image: axum::body::Bytes,
    media_type: &str,
    options: see_through::DecomposeOptions,
) -> ApiResult<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    let mut attempt = 0;
    loop {
        match client.decompose(image.clone(), media_type, options).await {
            Ok(output) => return Ok(output.psd),
            Err(see_through::SeeThroughError::Busy) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            Err(error) if attempt < TRANSIENT_RETRIES && transient(&error) => {
                attempt += 1;
                tracing::warn!(%error, attempt, "decomposition failed; retrying");
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Err(error) => return Err(see_through_error(error)),
        }
    }
}

/// Tries after a passing failure (a dropped connection, a time-out, the Space restarting).
pub(super) const TRANSIENT_RETRIES: u32 = 2;

/// Failures that may pass by themselves. Quota, a refusal, bad input or a
/// paused Space do not; trying again would only spend the quota.
pub(super) fn transient(error: &see_through::SeeThroughError) -> bool {
    use see_through::SeeThroughError::*;
    match error {
        Timeout | Transport(_) | Waking => true,
        Upstream { status, .. } => *status >= 500 || *status == 408,
        _ => false,
    }
}

pub(super) fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(20 * u64::from(attempt))
}

/// Resized to exactly `width` x `height`, as PNG.
pub(super) fn fit_to(bytes: &[u8], width: u32, height: u32) -> ApiResult<Vec<u8>> {
    let image = image::load_from_memory(bytes).map_err(internal_error)?;
    let image = if image.width() == width && image.height() == height {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
    };
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(internal_error)?;
    Ok(png)
}
