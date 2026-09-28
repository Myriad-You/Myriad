//! A sticker of hers as each chat app shows one: on Telegram a real sticker
//! (WebP, at most 512 across, transparent); elsewhere a small picture, about
//! the size people's stickers are, not a full-screen image.

use crate::services::channel_platform::ChannelPlatform;

const TELEGRAM_EDGE: u32 = 512;
const PICTURE_EDGE: u32 = 320;

pub struct Prepared {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// Her sticker, from its drawn PNG, as `platform` should get it.
pub async fn prepare(png: Vec<u8>, platform: ChannelPlatform) -> Option<Prepared> {
    tokio::task::spawn_blocking(move || prepare_now(&png, platform == ChannelPlatform::Telegram))
        .await
        .ok()
        .flatten()
}

fn prepare_now(png: &[u8], telegram: bool) -> Option<Prepared> {
    let image = image::load_from_memory(png).ok()?;
    let edge = if telegram {
        TELEGRAM_EDGE
    } else {
        PICTURE_EDGE
    };
    let image = if image.width().max(image.height()) > edge {
        image.resize(edge, edge, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    let mut bytes = Vec::new();
    if telegram {
        image::codecs::webp::WebPEncoder::new_lossless(&mut bytes)
            .encode(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )
            .ok()?;
        Some(Prepared {
            bytes,
            mime: "image/webp",
        })
    } else {
        rgba.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .ok()?;
        Some(Prepared {
            bytes,
            mime: "image/png",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sticker_is_small_and_keeps_its_transparency() {
        let mut drawn = image::RgbaImage::new(1024, 1024);
        drawn.put_pixel(512, 512, image::Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(drawn)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let telegram = prepare_now(&png, true).unwrap();
        assert_eq!(telegram.mime, "image/webp");
        let back = image::load_from_memory(&telegram.bytes).unwrap();
        assert_eq!((back.width(), back.height()), (512, 512));
        assert_eq!(back.to_rgba8().get_pixel(0, 0)[3], 0, "still transparent");
        let picture = prepare_now(&png, false).unwrap();
        assert_eq!(picture.mime, "image/png");
        let back = image::load_from_memory(&picture.bytes).unwrap();
        assert_eq!((back.width(), back.height()), (320, 320));
        assert!(prepare_now(b"not an image", false).is_none());
    }
}
