//! Local sticker cutout: if the generator left an opaque backdrop, flood-fill
//! similar border color to alpha 0 and emit PNG. Not a neural matting model.

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;

const ALPHA_OPAQUE: u8 = 250;
const BG_DIST2: u32 = 48 * 48 * 3;
const MIN_OPAQUE_RATIO: f32 = 0.01;
const USEFUL_ALPHA_RATIO: f32 = 0.02;

pub fn ensure_sticker_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Err("generated sticker is empty".to_string());
    }
    let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let mut rgba = decoded.to_rgba8();
    if !has_useful_alpha(&rgba) {
        cut_border_background(&mut rgba);
    }
    encode_png(&rgba)
}

fn has_useful_alpha(image: &RgbaImage) -> bool {
    let total = image.width().saturating_mul(image.height()) as usize;
    if total == 0 {
        return false;
    }
    let transparent = image
        .pixels()
        .filter(|pixel| pixel[3] < ALPHA_OPAQUE)
        .count();
    (transparent as f32 / total as f32) >= USEFUL_ALPHA_RATIO
}

fn average_border_rgb(image: &RgbaImage) -> Rgba<u8> {
    let w = image.width();
    let h = image.height();
    let corners = [
        *image.get_pixel(0, 0),
        *image.get_pixel(w.saturating_sub(1), 0),
        *image.get_pixel(0, h.saturating_sub(1)),
        *image.get_pixel(w.saturating_sub(1), h.saturating_sub(1)),
    ];
    let sum = corners.iter().fold([0u32; 3], |acc, pixel| {
        [
            acc[0] + u32::from(pixel[0]),
            acc[1] + u32::from(pixel[1]),
            acc[2] + u32::from(pixel[2]),
        ]
    });
    Rgba([
        (sum[0] / 4) as u8,
        (sum[1] / 4) as u8,
        (sum[2] / 4) as u8,
        255,
    ])
}

fn color_dist2(left: Rgba<u8>, right: Rgba<u8>) -> u32 {
    let dr = i32::from(left[0]) - i32::from(right[0]);
    let dg = i32::from(left[1]) - i32::from(right[1]);
    let db = i32::from(left[2]) - i32::from(right[2]);
    (dr * dr + dg * dg + db * db) as u32
}

fn cut_border_background(image: &mut RgbaImage) {
    let original = image.clone();
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return;
    }
    let background = average_border_rgb(image);
    let mut seen = vec![false; (width * height) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    let index = |x: u32, y: u32| (y * width + x) as usize;

    for x in 0..width {
        stack.push((x, 0));
        if height > 1 {
            stack.push((x, height - 1));
        }
    }
    for y in 1..height.saturating_sub(1) {
        stack.push((0, y));
        if width > 1 {
            stack.push((width - 1, y));
        }
    }

    while let Some((x, y)) = stack.pop() {
        let i = index(x, y);
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let pixel = *image.get_pixel(x, y);
        if color_dist2(pixel, background) > BG_DIST2 {
            continue;
        }
        image.put_pixel(x, y, Rgba([pixel[0], pixel[1], pixel[2], 0]));
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < width {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < height {
            stack.push((x, y + 1));
        }
    }

    let total = (width * height) as f32;
    let opaque = image.pixels().filter(|pixel| pixel[3] > 0).count() as f32;
    if total > 0.0 && opaque / total < MIN_OPAQUE_RATIO {
        *image = original;
        return;
    }

    feather_edges(image, background);
}

fn feather_edges(image: &mut RgbaImage, background: Rgba<u8>) {
    let width = image.width();
    let height = image.height();
    let snapshot = image.clone();
    for y in 0..height {
        for x in 0..width {
            let pixel = *snapshot.get_pixel(x, y);
            if pixel[3] == 0 {
                continue;
            }
            let neighbor_clear = (x > 0 && snapshot.get_pixel(x - 1, y)[3] == 0)
                || (x + 1 < width && snapshot.get_pixel(x + 1, y)[3] == 0)
                || (y > 0 && snapshot.get_pixel(x, y - 1)[3] == 0)
                || (y + 1 < height && snapshot.get_pixel(x, y + 1)[3] == 0);
            if !neighbor_clear {
                continue;
            }
            let dist = color_dist2(pixel, background).min(BG_DIST2);
            let alpha = ((dist * 255) / BG_DIST2.max(1)) as u8;
            image.put_pixel(x, y, Rgba([pixel[0], pixel[1], pixel[2], alpha.max(32)]));
        }
    }
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{ensure_sticker_png, has_useful_alpha};
    use image::{Rgba, RgbaImage};

    fn encode(image: &RgbaImage) -> Vec<u8> {
        super::encode_png(image).expect("png")
    }

    #[test]
    fn cuts_solid_white_around_a_red_subject() {
        let mut image = RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255]));
        for y in 10..22 {
            for x in 10..22 {
                image.put_pixel(x, y, Rgba([220, 20, 20, 255]));
            }
        }
        let png = ensure_sticker_png(&encode(&image)).expect("cutout");
        let out = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(out.get_pixel(0, 0)[3], 0);
        assert_eq!(out.get_pixel(31, 31)[3], 0);
        assert!(out.get_pixel(16, 16)[3] > 200);
        assert!(out.get_pixel(16, 16)[0] > 180);
    }

    #[test]
    fn keeps_existing_alpha() {
        let mut image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0]));
        image.put_pixel(3, 3, Rgba([10, 200, 10, 255]));
        assert!(has_useful_alpha(&image));
        let png = ensure_sticker_png(&encode(&image)).expect("keep");
        let out = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(out.get_pixel(0, 0)[3], 0);
        assert_eq!(out.get_pixel(3, 3)[3], 255);
    }

    #[test]
    fn rejects_empty_bytes() {
        assert!(ensure_sticker_png(&[]).is_err());
    }
}
