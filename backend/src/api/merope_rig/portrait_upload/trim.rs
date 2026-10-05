//! 主人上传的画四周常带一圈空边：白底、纯色底、透明底，宽窄各不相同。
//! 存下来之前裁到画的内容，只留一道窄边，和生成的立绘一样；手机拍的照片
//! 先按 EXIF 转正。铺满整幅的画不裁。

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageFormat, Rgba, RgbaImage, metadata::Orientation};

const MAX_EDGE: u32 = 8192;
const MAX_PIXELS: u64 = 16_777_216;
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;
/// At or below this alpha a pixel shows nothing.
const CLEAR_ALPHA: u8 = 16;
/// How far a channel may stray from the backdrop and still be backdrop:
/// JPEG noise, paper grain, a faint vignette.
const BACKDROP_TOLERANCE: u8 = 28;
/// The border's colour is a backdrop only when this share of its visible
/// pixels is near it; a gradient or a scene is not one.
const FLAT_BORDER_SHARE: f32 = 0.6;
/// Nothing is cut unless this share of the border is blank: a picture that
/// runs to its edges stays whole.
const BLANK_BORDER_SHARE: f32 = 0.5;
/// Up to this share of what is drawn may fall outside the cut: dust, a stray
/// dot. The tip of a thin strand of hair that falls with it is back inside the
/// gutter.
const STRAY_SHARE: f32 = 0.0002;
/// The gutter kept around the content, as a share of its longer side.
const GUTTER: f32 = 0.03;
/// A cut smaller than this share of a side is not worth re-encoding for.
const MIN_CUT_SHARE: f32 = 0.02;
const JPEG_QUALITY: u8 = 92;

pub(super) struct Trimmed {
    pub bytes: Vec<u8>,
    pub media_type: &'static str,
}

/// The picture cut to its content with a slim gutter, and upright. None when
/// it already is, or when it cannot be read here; the media check that stores
/// it has the last word on what is acceptable.
pub(super) fn trim_portrait(bytes: &[u8], max_bytes: usize) -> Option<Trimmed> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let format = reader.format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut picture = DynamicImage::from_decoder(decoder).ok()?;
    picture.apply_orientation(orientation);
    let (width, height) = (picture.width(), picture.height());
    let cut = content_bounds(&picture.to_rgba8())
        .map(|bounds| with_gutter(bounds, width, height))
        .filter(|&[left, top, right, bottom]| {
            let kept = |kept: u32, whole: u32| kept as f32 > whole as f32 * (1.0 - MIN_CUT_SHARE);
            !(kept(right - left, width) && kept(bottom - top, height))
        });
    if cut.is_none() && orientation == Orientation::NoTransforms {
        return None;
    }
    if let Some([left, top, right, bottom]) = cut {
        picture = picture.crop_imm(left, top, right - left, bottom - top);
    }
    let trimmed = encode(picture, format)?;
    (trimmed.bytes.len() <= max_bytes).then_some(trimmed)
}

/// A photo stays a JPEG; everything else becomes a PNG, which keeps
/// transparency and line art as they were.
fn encode(picture: DynamicImage, format: ImageFormat) -> Option<Trimmed> {
    let mut bytes = Vec::new();
    if format == ImageFormat::Jpeg {
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY);
        DynamicImage::ImageRgb8(picture.into_rgb8())
            .write_with_encoder(encoder)
            .ok()?;
        return Some(Trimmed {
            bytes,
            media_type: "image/jpeg",
        });
    }
    picture
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .ok()?;
    Some(Trimmed {
        bytes,
        media_type: "image/png",
    })
}

/// `[left, top, right, bottom)` of what is not backdrop, or None when the
/// picture has no blank border to speak of (or nothing on it).
fn content_bounds(image: &RgbaImage) -> Option<[u32; 4]> {
    let (width, height) = image.dimensions();
    if width < 3 || height < 3 {
        return None;
    }
    let border: Vec<Rgba<u8>> = image
        .enumerate_pixels()
        .filter(|(x, y, _)| *x == 0 || *y == 0 || *x == width - 1 || *y == height - 1)
        .map(|(_, _, pixel)| *pixel)
        .collect();
    let visible: Vec<[u8; 3]> = border
        .iter()
        .filter(|pixel| pixel[3] > CLEAR_ALPHA)
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect();
    let backdrop = median(&visible).filter(|backdrop| {
        let near = visible
            .iter()
            .filter(|colour| near(colour, backdrop))
            .count();
        near as f32 >= visible.len() as f32 * FLAT_BORDER_SHARE
    });
    let blank = |pixel: &Rgba<u8>| {
        pixel[3] <= CLEAR_ALPHA
            || backdrop.is_some_and(|backdrop| near(&[pixel[0], pixel[1], pixel[2]], &backdrop))
    };
    let blank_border = border.iter().filter(|pixel| blank(pixel)).count();
    if (blank_border as f32) < border.len() as f32 * BLANK_BORDER_SHARE {
        return None;
    }
    let mut rows = vec![0u32; height as usize];
    let mut columns = vec![0u32; width as usize];
    for (x, y, pixel) in image.enumerate_pixels() {
        if !blank(pixel) {
            rows[y as usize] += 1;
            columns[x as usize] += 1;
        }
    }
    let drawn: u64 = rows.iter().map(|&count| u64::from(count)).sum();
    let stray = (drawn as f32 * STRAY_SHARE) as u64;
    // From each side, the first line past which more than the strays are drawn.
    let span = |lines: &[u32]| {
        let past = |seen: &mut u64, count: u32| {
            *seen += u64::from(count);
            *seen > stray
        };
        let (mut from_start, mut from_end) = (0, 0);
        let first = lines
            .iter()
            .position(|&count| past(&mut from_start, count))?;
        let last = lines
            .iter()
            .rposition(|&count| past(&mut from_end, count))?;
        Some((first as u32, last as u32 + 1))
    };
    let (top, bottom) = span(&rows)?;
    let (left, right) = span(&columns)?;
    Some([left, top, right, bottom])
}

fn with_gutter([left, top, right, bottom]: [u32; 4], width: u32, height: u32) -> [u32; 4] {
    let gutter = (((right - left).max(bottom - top) as f32 * GUTTER).round() as u32).max(1);
    [
        left.saturating_sub(gutter),
        top.saturating_sub(gutter),
        (right + gutter).min(width),
        (bottom + gutter).min(height),
    ]
}

fn near(colour: &[u8; 3], backdrop: &[u8; 3]) -> bool {
    colour
        .iter()
        .zip(backdrop)
        .all(|(a, b)| a.abs_diff(*b) <= BACKDROP_TOLERANCE)
}

fn median(colours: &[[u8; 3]]) -> Option<[u8; 3]> {
    if colours.is_empty() {
        return None;
    }
    let mut median = [0u8; 3];
    for (channel, value) in median.iter_mut().enumerate() {
        let mut values: Vec<u8> = colours.iter().map(|colour| colour[channel]).collect();
        values.sort_unstable();
        *value = values[values.len() / 2];
    }
    Some(median)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX: usize = 10 * 1024 * 1024;

    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn decoded(trimmed: &Trimmed) -> DynamicImage {
        image::load_from_memory(&trimmed.bytes).unwrap()
    }

    /// A 400×600 picture on `backdrop` with a figure in 100..300 × 80..520.
    fn figure_on(backdrop: impl Fn(u32, u32) -> Rgba<u8>) -> RgbaImage {
        RgbaImage::from_fn(400, 600, |x, y| {
            if (100..300).contains(&x) && (80..520).contains(&y) {
                Rgba([40 + (x % 50) as u8, 90, 160, 255])
            } else {
                backdrop(x, y)
            }
        })
    }

    #[test]
    fn white_margins_with_noise_and_specks_are_cut_to_a_slim_gutter() {
        let mut image = figure_on(|x, y| {
            let grain = ((x * 7 + y * 13) % 6) as u8;
            Rgba([249 + grain, 250 + grain.min(5), 248 + grain, 255])
        });
        // Dust far from the figure is not content.
        for (x, y) in [(380, 20), (381, 20), (380, 21), (381, 21)] {
            image.put_pixel(x, y, Rgba([0, 0, 0, 255]));
        }
        // A thin strand of hair rising 20 pixels above the head stays whole.
        for x in 199..202 {
            for y in 60..80 {
                image.put_pixel(x, y, Rgba([30, 30, 30, 255]));
            }
        }
        let trimmed = trim_portrait(&png(&image), MAX).expect("margins cut");
        assert_eq!(trimmed.media_type, "image/png");
        let cut = decoded(&trimmed);
        // The strand's first rows fall with the dust; the gutter takes them back.
        assert_eq!((cut.width(), cut.height()), (200 + 2 * 14, 456 + 2 * 14));
        assert_eq!(cut.to_rgba8().get_pixel(14 + 99, 60 - 50)[0], 30);
    }

    #[test]
    fn transparent_and_coloured_backdrops_are_cut_too() {
        let clear = figure_on(|_, _| Rgba([0, 0, 0, 0]));
        let cut = decoded(&trim_portrait(&png(&clear), MAX).unwrap());
        assert_eq!(cut.height(), 440 + 2 * (440.0 * GUTTER).round() as u32);
        assert_eq!(cut.to_rgba8().get_pixel(0, 0)[3], 0, "transparency is kept");
        let pink = figure_on(|_, _| Rgba([250, 200, 215, 255]));
        assert_eq!(
            decoded(&trim_portrait(&png(&pink), MAX).unwrap()).width(),
            200 + 2 * 13
        );
    }

    #[test]
    fn a_figure_cut_off_by_the_frame_keeps_that_edge() {
        // A bust running off the bottom edge.
        let image = RgbaImage::from_fn(400, 600, |x, y| {
            if (100..300).contains(&x) && y >= 80 {
                Rgba([200, 60, 60, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        let cut = decoded(&trim_portrait(&png(&image), MAX).unwrap());
        let gutter = (520.0 * GUTTER).round() as u32;
        assert_eq!(
            (cut.width(), cut.height()),
            (200 + 2 * gutter, 520 + gutter)
        );
    }

    #[test]
    fn a_picture_that_fills_its_frame_or_is_already_tight_is_left_as_uploaded() {
        let scene = RgbaImage::from_fn(400, 600, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
        });
        assert!(trim_portrait(&png(&scene), MAX).is_none());
        let tight = RgbaImage::from_fn(212, 452, |x, y| {
            if (6..206).contains(&x) && (6..446).contains(&y) {
                Rgba([40, 90, 160, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        assert!(trim_portrait(&png(&tight), MAX).is_none());
        let blank = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        assert!(trim_portrait(&png(&blank), MAX).is_none());
        assert!(trim_portrait(b"not an image", MAX).is_none());
    }

    #[test]
    fn a_photo_stays_a_jpeg() {
        let image = DynamicImage::ImageRgba8(figure_on(|_, _| Rgba([255, 255, 255, 255])));
        let mut jpeg = Vec::new();
        DynamicImage::ImageRgb8(image.into_rgb8())
            .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
            .unwrap();
        let trimmed = trim_portrait(&jpeg, MAX).unwrap();
        assert_eq!(trimmed.media_type, "image/jpeg");
        let cut = decoded(&trimmed);
        // JPEG blurs the figure's edge by a pixel or two.
        assert!(
            (cut.width() as i32 - (200 + 26)).abs() <= 4,
            "{}",
            cut.width()
        );
        assert!(
            trim_portrait(&jpeg, 1).is_none(),
            "too large after re-encoding: keep the upload"
        );
    }
}
