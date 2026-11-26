//! 生图与拆层：转头图的生成、评判后重画，拆层的检查、重试与等待。

use super::{fit_turns::Turns, together::TurnedSteps, *};

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
    let frame = Frame::new(
        image_generation::drawn_aspect(generation, width, height),
        width,
        height,
    );
    let framed = frame.reference(reference)?;
    let (request_width, request_height) = frame.request();
    let generated = crate::services::ai_cost_ledger::with_ai_ledger_attribution(
        super::live::drawing_attribution(user_id),
        image_generation::generate_image(
            generation,
            prompt,
            request_width,
            request_height,
            Some(&framed),
        ),
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    let (bytes, _) = image_generation::load_generated_bytes(generated)
        .await
        .map_err(portrait_generation_provider_error)?;
    // The provider may answer at its own size; lined up means the master's.
    frame.fit(&bytes)
}

/// The longest side `generate_image` passes on unclamped.
const MAX_REQUEST_SIDE: u32 = 2048;

/// The master centred on a canvas of the shape the provider draws at. A
/// provider asked for a named ratio draws the whole picture at that ratio, so
/// a master of another shape, stretched back from it, comes back squeezed;
/// padded to the ratio first and cut back out after, it keeps its proportions.
/// A master already of that shape is its own canvas and goes as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    aspect: (u32, u32),
    scale: u32,
    master: (u32, u32),
    offset: (u32, u32),
}

impl Frame {
    fn new((a, b): (u32, u32), width: u32, height: u32) -> Self {
        let (a, b) = (a.max(1), b.max(1));
        let (width, height) = (width.max(1), height.max(1));
        let scale = width.div_ceil(a).max(height.div_ceil(b));
        Self {
            aspect: (a, b),
            scale,
            master: (width, height),
            offset: ((scale * a - width) / 2, (scale * b - height) / 2),
        }
    }

    fn canvas(&self) -> (u32, u32) {
        (self.scale * self.aspect.0, self.scale * self.aspect.1)
    }

    fn padded(&self) -> bool {
        self.canvas() != self.master
    }

    /// The size to ask for: the master's own, or the canvas kept to the
    /// exact ratio within the sides the request is clamped to.
    fn request(&self) -> (u32, u32) {
        if !self.padded() {
            return self.master;
        }
        let (a, b) = self.aspect;
        let scale = self.scale.min(MAX_REQUEST_SIDE / a.max(b)).max(1);
        (scale * a, scale * b)
    }

    /// The master on the canvas, the rest filled with its edge's colour, so a
    /// plain background just goes on.
    fn reference(
        &self,
        reference: &image_generation::ImageReference,
    ) -> ApiResult<image_generation::ImageReference> {
        if !self.padded() {
            return Ok(reference.clone());
        }
        let master = image::load_from_memory(&reference.bytes)
            .map_err(internal_error)?
            .to_rgba8();
        let (width, height) = self.canvas();
        let mut canvas = image::RgbaImage::from_pixel(width, height, edge_colour(&master));
        image::imageops::replace(
            &mut canvas,
            &master,
            i64::from(self.offset.0),
            i64::from(self.offset.1),
        );
        let png = encode_png(&image::DynamicImage::ImageRgba8(canvas))?;
        image_generation::ImageReference::new(png, "image/png")
            .map_err(|error| internal_error(error.to_string()))
    }

    /// The master's place cut out of a drawing of the canvas, at the master's
    /// size, as PNG.
    fn fit(&self, bytes: &[u8]) -> ApiResult<Vec<u8>> {
        let (width, height) = self.master;
        if !self.padded() {
            return fit_to(bytes, width, height);
        }
        let image = image::load_from_memory(bytes).map_err(internal_error)?;
        let (canvas_width, canvas_height) = self.canvas();
        let (x0, x1) = span(self.offset.0, width, canvas_width, image.width());
        let (y0, y1) = span(self.offset.1, height, canvas_height, image.height());
        fit_image(image.crop_imm(x0, y0, x1 - x0, y1 - y0), width, height)
    }
}

/// `[start, start + len)` of `whole`, scaled to `drawn`, at least a pixel wide.
fn span(start: u32, len: u32, whole: u32, drawn: u32) -> (u32, u32) {
    let at = |value: u32| {
        ((u64::from(value) * u64::from(drawn) + u64::from(whole) / 2) / u64::from(whole)) as u32
    };
    let first = at(start).min(drawn.saturating_sub(1));
    (first, at(start + len).clamp(first + 1, drawn))
}

/// The commonest colour along the edge (by 16 levels a channel, averaged
/// within): a bust cut off at the bottom edge keeps its background colour.
fn edge_colour(image: &image::RgbaImage) -> image::Rgba<u8> {
    let (width, height) = image.dimensions();
    let mut bins: std::collections::HashMap<[u8; 4], ([u64; 4], u64)> =
        std::collections::HashMap::new();
    for (x, y, pixel) in image.enumerate_pixels() {
        if x == 0 || y == 0 || x + 1 == width || y + 1 == height {
            let (sum, count) = bins.entry(pixel.0.map(|channel| channel >> 4)).or_default();
            for (total, channel) in sum.iter_mut().zip(pixel.0) {
                *total += u64::from(channel);
            }
            *count += 1;
        }
    }
    let (sum, count) = bins
        .into_values()
        .max_by_key(|(_, count)| *count)
        .unwrap_or_default();
    image::Rgba(sum.map(|total| (total / count.max(1)) as u8))
}

/// The sides of the turned drawings, in their order.
pub(super) const TURN_SIDES: [&str; 4] = ["plus", "minus", "up", "down"];

/// The turned drawings' steps beside the front decomposition
/// (`turns_beside_front`), as a job takes them.
impl TurnedSteps for Turns<'_> {
    async fn decompose(&self, png: &[u8], kept: Option<Vec<u8>>) -> ApiResult<(Vec<u8>, bool)> {
        let bytes = axum::body::Bytes::copy_from_slice(png);
        decompose_checked(
            self.client,
            &bytes,
            "image/png",
            self.options,
            &TURNED_REDOS,
            kept,
        )
        .await
    }

    /// The Space's `check_turns`: the turned drawings redrawn past what keys
    /// of the front drawing can follow (a restyled fringe, a moved clip). A
    /// check that fails finds none: the turns are then fitted as drawn.
    async fn drawn_past(&self, front_psd: Vec<u8>, turned: Vec<Vec<u8>>) -> Vec<usize> {
        let front = (
            self.reference.bytes.to_vec(),
            self.reference.media_type.clone(),
        );
        let pictures: Vec<_> = std::iter::once(front)
            .chain(turned.into_iter().map(|png| (png, "image/png".to_string())))
            .collect();
        let Ok(pictures) = <[(Vec<u8>, String); 5]>::try_from(pictures) else {
            return Vec::new();
        };
        match self.client.check_turned_pictures(front_psd, pictures).await {
            Ok(check) => {
                if !check.bad.is_empty() {
                    tracing::info!(bad = ?check.bad, residual = ?check.residual, "turned drawings drawn past what the front one's keys can follow");
                }
                turn_places(&check.bad)
            }
            Err(error) => {
                tracing::warn!(%error, "turned drawings check failed; keeping the drawings");
                Vec::new()
            }
        }
    }

    async fn draw_again(&self, index: usize) -> ApiResult<Vec<u8>> {
        let (width, height) = self.size;
        draw_turn(
            self.user_id,
            self.generation,
            &turn_prompts()[index],
            self.reference,
            width,
            height,
        )
        .await
    }
}

/// The places among the four turned drawings of the sides a check names; a
/// side that is none of them is left out.
pub(super) fn turn_places(sides: &[String]) -> Vec<usize> {
    sides
        .iter()
        .filter_map(|side| TURN_SIDES.iter().position(|known| known == side))
        .collect()
}

/// Makes a turn drawn again the set's own: a rerun finds the new drawing,
/// none of its decomposition yet, and that the turn is not to be drawn yet
/// again. What the cache has of the decomposition goes first (an empty entry
/// reads as none), the drawing after it: at no time does the cache hold a
/// decomposition beside another drawing than the one it is of.
pub(super) async fn keep_redrawn(cache: &TurnCache, index: usize, png: &[u8]) {
    let place = index + 1;
    cache
        .write(&format!("decomposition-{place}.checked"), b"")
        .await;
    cache
        .write(&format!("decomposition-{place}.psd"), b"")
        .await;
    cache.write(&format!("turned-{index}.png"), png).await;
    cache
        .write(&format!("turned-{index}.redrawn"), b"redrawn")
        .await;
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
    let redos: &[(bool, u16)] = if index == 0 {
        &FRONT_REDOS
    } else {
        &TURNED_REDOS
    };
    let kept = match kept_decomposition(cache, index).await {
        Some((psd, true)) => return Ok(psd),
        kept => kept.map(|(psd, _)| psd),
    };
    let (psd, settled) = decompose_checked(client, bytes, media_type, options, redos, kept).await?;
    keep_decomposition(cache, index, &psd, settled).await;
    Ok(psd)
}

/// What the cache has of the decomposition at `index` (the front's at 0, a
/// turn's at its place plus one), and whether its check had its say.
pub(super) async fn kept_decomposition(cache: &TurnCache, index: usize) -> Option<(Vec<u8>, bool)> {
    let psd = cache.read(&format!("decomposition-{index}.psd")).await?;
    let checked = cache.read(&format!("decomposition-{index}.checked")).await;
    Some((psd, checked.is_some()))
}

/// Keeps the decomposition at `index` in the cache; as checked when its check
/// had its say (`settled`), else to be checked again by the job that finds it.
pub(super) async fn keep_decomposition(cache: &TurnCache, index: usize, psd: &[u8], settled: bool) {
    cache
        .write(&format!("decomposition-{index}.psd"), psd)
        .await;
    if settled {
        cache
            .write(&format!("decomposition-{index}.checked"), b"checked")
            .await;
    }
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

/// A decomposition, waiting its turn while as many as are sent at once are running.
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
                super::live::note_retry();
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

/// A call to the Space tried again after a passing failure (`transient`), as
/// often and as far apart as a decomposition is. Any other failure, and a
/// passing one past its tries, is the call's own: quota, a refusal or bad
/// input would only fail the same way again.
pub(super) async fn retried<T, C>(
    step: &'static str,
    call: impl FnMut() -> C,
) -> Result<T, see_through::SeeThroughError>
where
    C: Future<Output = Result<T, see_through::SeeThroughError>>,
{
    retried_when(step, transient, call).await
}

/// `retried`, with the failures tried again named by `passing`: a step for
/// which not every failure that may pass is worth waiting out again.
pub(super) async fn retried_when<T, C>(
    step: &'static str,
    passing: impl Fn(&see_through::SeeThroughError) -> bool,
    mut call: impl FnMut() -> C,
) -> Result<T, see_through::SeeThroughError>
where
    C: Future<Output = Result<T, see_through::SeeThroughError>>,
{
    let mut attempt = 0;
    loop {
        match call().await {
            Err(error) if attempt < TRANSIENT_RETRIES && passing(&error) => {
                attempt += 1;
                tracing::warn!(%error, attempt, step, "Space step failed; retrying");
                tokio::time::sleep(retry_delay(attempt)).await;
                super::live::note_retry();
            }
            out => return out,
        }
    }
}

/// Resized to exactly `width` x `height`, as PNG.
pub(super) fn fit_to(bytes: &[u8], width: u32, height: u32) -> ApiResult<Vec<u8>> {
    fit_image(
        image::load_from_memory(bytes).map_err(internal_error)?,
        width,
        height,
    )
}

fn fit_image(image: image::DynamicImage, width: u32, height: u32) -> ApiResult<Vec<u8>> {
    let image = if image.width() == width && image.height() == height {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
    };
    encode_png(&image)
}

fn encode_png(image: &image::DynamicImage) -> ApiResult<Vec<u8>> {
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(internal_error)?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white_master(width: u32, height: u32, figure: (u32, u32, u32, u32)) -> Vec<u8> {
        let (x, y, w, h) = figure;
        let mut image = image::RgbaImage::from_pixel(width, height, image::Rgba([255; 4]));
        for (px, py, pixel) in image.enumerate_pixels_mut() {
            if (x..x + w).contains(&px) && (y..y + h).contains(&py) {
                *pixel = image::Rgba([200, 40, 60, 255]);
            }
        }
        encode_png(&image::DynamicImage::ImageRgba8(image)).unwrap()
    }

    fn reference(png: Vec<u8>) -> image_generation::ImageReference {
        image_generation::ImageReference::new(png, "image/png").unwrap()
    }

    /// Columns and rows of the figure's colour in `png`.
    fn figure_box(png: &[u8]) -> (u32, u32, u32, u32) {
        let image = image::load_from_memory(png).unwrap().to_rgba8();
        let inside: Vec<_> = image
            .enumerate_pixels()
            .filter(|(_, _, pixel)| pixel.0[1] < 128)
            .map(|(x, y, _)| (x, y))
            .collect();
        let left = inside.iter().map(|p| p.0).min().unwrap();
        let right = inside.iter().map(|p| p.0).max().unwrap();
        let top = inside.iter().map(|p| p.1).min().unwrap();
        let bottom = inside.iter().map(|p| p.1).max().unwrap();
        (left, top, right + 1 - left, bottom + 1 - top)
    }

    /// The provider drawing the canvas unchanged at a size of its own.
    fn drawn_at(reference: &image_generation::ImageReference, width: u32, height: u32) -> Vec<u8> {
        let image = image::load_from_memory(&reference.bytes).unwrap();
        encode_png(&image.resize_exact(width, height, image::imageops::FilterType::Lanczos3))
            .unwrap()
    }

    #[test]
    fn a_master_of_no_named_ratio_keeps_its_proportions() {
        // 1278x1230 was asked as 16:9 and came back 0.6 times as wide.
        let figure = (439, 100, 400, 1130);
        let master = reference(white_master(1278, 1230, figure));
        let frame = Frame::new((1, 1), 1278, 1230);
        assert_eq!(frame.request(), (1278, 1278));
        assert_eq!(image_generation::named_aspect_ratio(1278, 1278), "1:1");

        let framed = frame.reference(&master).unwrap();
        let canvas = image::load_from_memory(&framed.bytes).unwrap().to_rgba8();
        assert_eq!(canvas.dimensions(), (1278, 1278));
        assert_eq!(
            canvas.get_pixel(5, 5).0,
            [255; 4],
            "padded with the background colour"
        );
        assert_eq!(figure_box(&framed.bytes), (439, 124, 400, 1130));

        let fitted = frame.fit(&drawn_at(&framed, 1024, 1024)).unwrap();
        let back = image::load_from_memory(&fitted).unwrap();
        assert_eq!((back.width(), back.height()), (1278, 1230));
        let (x, y, w, h) = figure_box(&fitted);
        for (got, want) in [(x, 439), (y, 100), (w, 400), (h, 1130)] {
            assert!(
                got.abs_diff(want) <= 3,
                "{:?} against {figure:?}",
                (x, y, w, h)
            );
        }
    }

    #[test]
    fn a_portrait_near_three_four_is_padded_to_exactly_three_four() {
        let frame = Frame::new((3, 4), 1000, 1300);
        assert_eq!(frame.canvas(), (1002, 1336));
        assert_eq!(frame.request(), (1002, 1336));
        assert_eq!(image_generation::named_aspect_ratio(1002, 1336), "3:4");
        let master = reference(white_master(1000, 1300, (300, 200, 400, 1100)));
        let fitted = frame
            .fit(&drawn_at(&frame.reference(&master).unwrap(), 1152, 1536))
            .unwrap();
        let (_, _, w, h) = figure_box(&fitted);
        assert!(w.abs_diff(400) <= 3 && h.abs_diff(1100) <= 3, "{w}x{h}");
    }

    #[test]
    fn a_wide_canvas_stays_on_its_ratio_within_the_request_clamp() {
        let frame = Frame::new((16, 9), 3000, 1500);
        assert_eq!(frame.request(), (2048, 1152));
        assert_eq!(image_generation::named_aspect_ratio(2048, 1152), "16:9");
    }

    #[test]
    fn a_master_of_the_drawn_shape_goes_as_it_is() {
        for (aspect, width, height) in [
            ((3, 4), 1152, 1536),
            ((2, 3), 900, 1350),
            ((900, 1350), 900, 1350),
        ] {
            let frame = Frame::new(aspect, width, height);
            assert_eq!(frame.request(), (width, height));
            let master = reference(white_master(width, height, (100, 100, 300, 600)));
            assert_eq!(frame.reference(&master).unwrap(), master);
            let drawn = drawn_at(&master, 768, 1024);
            assert_eq!(
                frame.fit(&drawn).unwrap(),
                fit_to(&drawn, width, height).unwrap()
            );
        }
    }
}
