//! Small copies of private-chat attachments for the model to look at.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

const MODEL_IMAGE_EDGE: u32 = 1024;

/// The cached attachments, with a small copy of each image the model can
/// look at. The copies are taken out of the request before anything is kept.
pub(super) async fn with_model_images(cached: Option<&Value>) -> Option<Value> {
    let mut data = cached?.clone();
    let Some(attachments) = data.get_mut("attachments").and_then(Value::as_array_mut) else {
        return Some(data);
    };
    for attachment in attachments.iter_mut() {
        let Some(url) = attachment
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(image) = super::transport::load_channel_image_bytes(&url).await else {
            continue;
        };
        let bytes = image.bytes;
        if let Ok(Some(small)) = tokio::task::spawn_blocking(move || model_copy(&bytes)).await {
            attachment["image"] = json!(small);
        }
    }
    Some(data)
}

/// A JPEG data URL no longer than `MODEL_IMAGE_EDGE` on its long side.
fn model_copy(bytes: &[u8]) -> Option<String> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width().max(image.height()) > MODEL_IMAGE_EDGE {
        image.resize(
            MODEL_IMAGE_EDGE,
            MODEL_IMAGE_EDGE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let mut out = Vec::new();
    image
        .to_rgb8()
        .write_to(
            &mut std::io::Cursor::new(&mut out),
            image::ImageFormat::Jpeg,
        )
        .ok()?;
    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photos_are_shrunk_for_the_model() {
        let mut big = image::RgbImage::new(3000, 1500);
        big.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(big)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let url = model_copy(&png).unwrap();
        let bytes = STANDARD
            .decode(url.strip_prefix("data:image/jpeg;base64,").unwrap())
            .unwrap();
        let small = image::load_from_memory(&bytes).unwrap();
        assert_eq!((small.width(), small.height()), (1024, 512));
        assert!(model_copy(b"not an image").is_none());
    }
}
