//! A full body's gesture arms. Its hand is a third of a bust's on the drawn
//! picture, too small for the image model to draw well, so the gesture is
//! drawn on a window of the figure (the head down past the hanging hands)
//! taken from its enlarged picture, at about a bust's size; what comes back is
//! enlarged by the same super-resolution as the figure and laid back on its
//! enlarged picture, which the importer cuts the arm from by difference.

use std::io::Cursor;

use image::{DynamicImage, RgbaImage, imageops};

/// A window of the enlarged picture, in its pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Window {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The sizes a window is drawn at, about a bust's (1152 × 1536): an image
/// model asked for another shape takes the nearest it knows and frames the
/// picture anew in it, which then no longer lies on the window.
const DRAWN: [(u32, u32); 4] = [(1280, 1280), (1152, 1536), (1024, 1536), (864, 1536)];
/// A pose point the model is at least this sure of.
const SURE: f32 = 0.3;
/// Room about the arms for a hand raised beside the face, by the shoulders' width.
const SIDE_ROOM: f32 = 0.35;
/// Room below the hanging hands, by the arm's length.
const FOOT_ROOM: f32 = 0.15;
/// Room above the nose for the crown and hair, by the nose's height above the shoulders.
const CROWN_ROOM: f32 = 1.6;
/// The window's edge fades into the enlarged picture over this many of its pixels.
const SEAM: u32 = 12;

/// The window on a picture `size` large whose pose (on that picture's pixels) is `keypoints`: the head down past
/// the hanging hands, the arms and a raised hand's room either side. None without shoulders, nose and wrists.
pub(super) fn window(keypoints: &[[f32; 3]], size: (u32, u32)) -> Option<Window> {
    let sure = |i: usize| {
        keypoints
            .get(i)
            .filter(|p| p[2] >= SURE)
            .map(|p| [p[0], p[1]])
    };
    let (nose, left, right) = (sure(0)?, sure(5)?, sure(6)?);
    let (left_wrist, right_wrist) = (sure(9)?, sure(10)?);
    let shoulders = (left[0] - right[0]).abs().max(1.0);
    let shoulder_y = (left[1] + right[1]) / 2.0;
    let arm = |s: [f32; 2], e: Option<[f32; 2]>, w: [f32; 2]| match e {
        Some(e) => dist(s, e) + dist(e, w),
        None => dist(s, w),
    };
    let arm_length = (arm(left, sure(7), left_wrist) + arm(right, sure(8), right_wrist)) / 2.0;
    // The arms: shoulders, elbows, wrists and every sure point of either hand.
    let mut points: Vec<[f32; 2]> = [5, 6, 7, 8, 9, 10]
        .iter()
        .filter_map(|&i| sure(i))
        .collect();
    points.extend((91..133).filter_map(sure));
    let x0 = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) - SIDE_ROOM * shoulders;
    let x1 = points
        .iter()
        .map(|p| p[0])
        .fold(f32::NEG_INFINITY, f32::max)
        + SIDE_ROOM * shoulders;
    let y0 = nose[1] - CROWN_ROOM * (shoulder_y - nose[1]).max(1.0);
    let y1 = points
        .iter()
        .map(|p| p[1])
        .fold(f32::NEG_INFINITY, f32::max)
        + FOOT_ROOM * arm_length;
    let (width, height) = (size.0 as f32, size.1 as f32);
    let x0 = x0.clamp(0.0, width - 1.0).floor();
    let y0 = y0.clamp(0.0, height - 1.0).floor();
    let x1 = x1.clamp(x0 + 1.0, width).ceil();
    let y1 = y1.clamp(y0 + 1.0, height).ceil();
    Some(Window {
        x: x0 as u32,
        y: y0 as u32,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    })
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The size the window is drawn at: a bust's area, its shape, in sixteens.
pub(super) fn drawn_size(window: Window) -> (u32, u32) {
    let aspect = (window.width as f32 / window.height.max(1) as f32).ln();
    let off = |&(w, h): &(u32, u32)| ((w as f32 / h as f32).ln() - aspect).abs();
    DRAWN
        .into_iter()
        .min_by(|a, b| off(a).total_cmp(&off(b)))
        .unwrap_or(DRAWN[1])
}

/// The window grown about its middle to the shape it is drawn at, kept on a
/// picture `size` large (moved back on where it would run off; cut where the
/// picture is smaller, then drawn a little stretched).
pub(super) fn shaped(window: Window, drawn: (u32, u32), size: (u32, u32)) -> Window {
    let aspect = drawn.0 as f32 / drawn.1 as f32;
    let (mut width, mut height) = (window.width as f32, window.height as f32);
    if width / height < aspect {
        width = height * aspect;
    } else {
        height = width / aspect;
    }
    let width = (width.round() as u32).min(size.0);
    let height = (height.round() as u32).min(size.1);
    let place = |at: u32, own: u32, grown: u32, room: u32| {
        let middle = at as f32 + own as f32 / 2.0;
        ((middle - grown as f32 / 2.0).round().max(0.0) as u32).min(room - grown)
    };
    Window {
        x: place(window.x, window.width, width, size.0),
        y: place(window.y, window.height, height, size.1),
        width,
        height,
    }
}

/// The window of the enlarged picture, at the size it is drawn at, as PNG.
pub(super) fn crop(enlarged: &DynamicImage, window: Window, drawn: (u32, u32)) -> Option<Vec<u8>> {
    let cut = enlarged.crop_imm(window.x, window.y, window.width, window.height);
    encode(&cut.resize_exact(drawn.0, drawn.1, imageops::FilterType::Lanczos3))
}

/// The drawn window laid back on the enlarged picture: `enlarged_drawing` is the drawing super-resolved (any size
/// of its shape), brought to the window's size; its edge fades into the picture over a few pixels.
pub(super) fn lay_back(
    enlarged: &DynamicImage,
    window: Window,
    enlarged_drawing: &[u8],
) -> Option<Vec<u8>> {
    let drawing = image::load_from_memory(enlarged_drawing).ok()?;
    let drawing = drawing
        .resize_exact(window.width, window.height, imageops::FilterType::Lanczos3)
        .to_rgba8();
    let mut out: RgbaImage = enlarged.to_rgba8();
    for y in 0..window.height {
        for x in 0..window.width {
            let edge = x
                .min(y)
                .min(window.width - 1 - x)
                .min(window.height - 1 - y);
            let share = ((edge as f32 + 0.5) / SEAM as f32).min(1.0);
            let (tx, ty) = (window.x + x, window.y + y);
            if tx >= out.width() || ty >= out.height() {
                continue;
            }
            let over = drawing.get_pixel(x, y);
            let under = out.get_pixel_mut(tx, ty);
            for c in 0..4 {
                under[c] = (f32::from(under[c]) * (1.0 - share) + f32::from(over[c]) * share)
                    .round() as u8;
            }
        }
    }
    encode(&DynamicImage::ImageRgba8(out))
}

/// Points found on the drawing (`drawn` large) moved onto the enlarged picture.
pub(super) fn on_picture(points: &[[f32; 3]], window: Window, drawn: (u32, u32)) -> Vec<[f32; 3]> {
    let sx = window.width as f32 / drawn.0.max(1) as f32;
    let sy = window.height as f32 / drawn.1.max(1) as f32;
    points
        .iter()
        .map(|&[x, y, score]| [window.x as f32 + x * sx, window.y as f32 + y * sy, score])
        .collect()
}

fn encode(picture: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    picture.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A standing figure on 864 × 1536: nose, shoulders, elbows, wrists, and a hand by each wrist.
    fn figure() -> Vec<[f32; 3]> {
        let mut k = vec![[0.0, 0.0, 0.0]; 133];
        k[0] = [427.0, 210.0, 0.9];
        k[5] = [555.0, 343.0, 0.8];
        k[6] = [320.0, 348.0, 0.8];
        k[7] = [602.0, 548.0, 0.8];
        k[8] = [281.0, 541.0, 0.8];
        k[9] = [654.0, 733.0, 0.8];
        k[10] = [214.0, 728.0, 0.8];
        for (i, point) in k[91..112].iter_mut().enumerate() {
            *point = [660.0, 740.0 + 3.0 * i as f32, 0.6];
        }
        for (i, point) in k[112..133].iter_mut().enumerate() {
            *point = [210.0, 735.0 + 3.0 * i as f32, 0.6];
        }
        k
    }

    #[test]
    fn the_window_runs_from_the_crown_past_the_hanging_hands_with_room_either_side() {
        let w = window(&figure(), (864, 1536)).unwrap();
        // Shoulders 235 apart: 82 either side of the hands.
        assert_eq!((w.x, w.x + w.width), (127, 743));
        // Nose 136 above the shoulders: the crown 218 above the nose; the hands' foot plus an arm's 15%.
        assert_eq!(w.y, 0);
        assert!(w.y + w.height > 795 && w.y + w.height < 900, "{w:?}");
    }

    #[test]
    fn the_window_stays_on_the_picture_and_needs_the_arms() {
        let mut k = figure();
        k[9] = [860.0, 733.0, 0.8];
        let w = window(&k, (864, 1536)).unwrap();
        assert_eq!(w.x + w.width, 864);
        k[10][2] = 0.1;
        assert!(window(&k, (864, 1536)).is_none());
    }

    #[test]
    fn a_window_is_drawn_at_the_nearest_shape_a_model_knows_and_grown_to_it() {
        let window = Window {
            x: 228,
            y: 0,
            width: 1868,
            height: 2420,
        };
        let drawn = drawn_size(window);
        assert_eq!(drawn, (1152, 1536));
        // Grown to 3:4 about its middle, held on the picture's top.
        let grown = shaped(window, drawn, (2304, 4096));
        assert_eq!((grown.width, grown.height), (1868, 2491));
        assert_eq!((grown.x, grown.y), (228, 0));
        assert_eq!(
            drawn_size(Window {
                x: 0,
                y: 0,
                width: 500,
                height: 900
            }),
            (864, 1536)
        );
        // Grown past the picture's top: moved back onto it.
        let wide = shaped(
            Window {
                x: 50,
                y: 100,
                width: 800,
                height: 400,
            },
            (1280, 1280),
            (864, 1536),
        );
        assert_eq!((wide.x, wide.y, wide.width, wide.height), (50, 0, 800, 800));
    }

    #[test]
    fn points_found_on_the_drawing_land_on_the_enlarged_picture() {
        let window = Window {
            x: 100,
            y: 40,
            width: 600,
            height: 900,
        };
        assert_eq!(
            on_picture(&[[300.0, 450.0, 0.7]], window, (1200, 1800)),
            vec![[250.0, 265.0, 0.7]]
        );
    }

    #[test]
    fn laid_back_the_drawing_covers_its_window_and_fades_at_its_edge() {
        let enlarged =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255])));
        let drawing = encode(&DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([255, 255, 255, 255]),
        )))
        .unwrap();
        let window = Window {
            x: 8,
            y: 8,
            width: 40,
            height: 40,
        };
        let out = image::load_from_memory(&lay_back(&enlarged, window, &drawing).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(out.get_pixel(0, 0)[0], 0);
        assert_eq!(out.get_pixel(28, 28)[0], 255);
        let edge = out.get_pixel(8, 28)[0];
        assert!(edge > 0 && edge < 64, "{edge}");
    }
}
