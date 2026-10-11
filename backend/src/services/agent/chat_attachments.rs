//! What someone attaches to a chat message: images she can look at and small
//! text files she can read.
//!
//! Images are never stored. The browser shrinks each one and sends it inside
//! this one request; the server takes it out of the request data (so it is
//! never persisted or logged with the context), checks it really is an image,
//! and hands it to the model for this turn only.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;

use crate::services::analyzer::ImageInput;

pub const MAX_IMAGES: usize = 4;
/// Decoded bytes per image. The browser sends about 1024 px on the long side,
/// far below this.
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_FILE_CHARS: usize = 8_000;
const MAX_NAME_CHARS: usize = 120;

/// Take the images out of `customData.attachments`, keeping the valid ones.
/// Every `image` field is removed, valid or not.
pub fn take_images(custom_data: Option<&mut Value>) -> Vec<ImageInput> {
    let Some(Value::Array(attachments)) = custom_data.and_then(|data| data.get_mut("attachments"))
    else {
        return Vec::new();
    };
    let mut images = Vec::new();
    for attachment in attachments.iter_mut() {
        let Some(image) = attachment
            .as_object_mut()
            .and_then(|object| object.remove("image"))
        else {
            continue;
        };
        if images.len() >= MAX_IMAGES {
            continue;
        }
        if let Some(image) = image.as_str().and_then(image_from_data_url) {
            images.push(image);
        }
    }
    images
}

fn image_from_data_url(url: &str) -> Option<ImageInput> {
    let data = url
        .strip_prefix("data:")?
        .split_once(";base64,")
        .map(|(_, data)| data)?;
    // Four base64 characters carry three bytes.
    if data.len() / 4 * 3 > MAX_IMAGE_BYTES + 3 {
        return None;
    }
    let bytes = STANDARD.decode(data).ok()?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    // Trust the bytes, not the declared type.
    Some(ImageInput {
        mime: sniff(&bytes)?.to_string(),
        base64: data.to_string(),
    })
}

/// The image type the bytes are, whatever they were declared as.
pub(crate) fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        _ if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WEBP"[..]) => {
            Some("image/webp")
        }
        _ => None,
    }
}

/// What came with the message, for the untrusted scene: how many images she
/// can see, and the text of attached files.
pub fn format_attached(custom_data: Option<&Value>, images: usize) -> String {
    let mut lines = Vec::new();
    if images > 0 {
        lines.push(format!(
            "They attached {images} image{} to this message; you can see {}.",
            if images == 1 { "" } else { "s" },
            if images == 1 { "it" } else { "them" },
        ));
    }
    let files = custom_data
        .and_then(|data| data.get("attachments"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|attachment| {
            let text = attachment.get("text")?.as_str()?.trim();
            (!text.is_empty()).then(|| {
                let name: String = attachment
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("file")
                    .chars()
                    .take(MAX_NAME_CHARS)
                    .collect();
                let text: String = text.chars().take(MAX_FILE_CHARS).collect();
                format!("Attached file \"{name}\":\n{text}")
            })
        })
        .take(MAX_IMAGES);
    lines.extend(files);
    lines.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    fn data_url(mime: &str, bytes: &[u8]) -> String {
        format!("data:{mime};base64,{}", STANDARD.encode(bytes))
    }

    #[test]
    fn images_leave_the_request_data_and_only_real_ones_are_kept() {
        let mut data = json!({
            "attachments": [
                { "name": "a.png", "mime": "image/png", "image": data_url("image/png", PNG) },
                { "name": "fake.png", "mime": "image/png", "image": data_url("image/png", b"<svg/>") },
                { "name": "b.jpg", "mime": "image/jpeg", "image": "not a data url" },
                { "name": "notes.txt", "mime": "text/plain", "text": "hi" },
            ]
        });
        let images = take_images(Some(&mut data));
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].mime, "image/png");
        assert!(
            !data.to_string().contains("base64"),
            "no image data stays in the context"
        );
        assert_eq!(data["attachments"][3]["text"], "hi");
    }

    #[test]
    fn the_declared_type_is_not_trusted() {
        let mut data = json!({
            "attachments": [{ "image": data_url("image/webp", PNG) }]
        });
        assert_eq!(take_images(Some(&mut data))[0].mime, "image/png");
        let webp = b"RIFF\x10\0\0\0WEBPVP8 ";
        let mut data = json!({
            "attachments": [{ "image": data_url("image/png", webp) }]
        });
        assert_eq!(take_images(Some(&mut data))[0].mime, "image/webp");
    }

    #[test]
    fn images_are_bounded_in_count_and_size() {
        let mut big = PNG.to_vec();
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        let mut data = json!({
            "attachments": (0..6)
                .map(|_| json!({ "image": data_url("image/png", PNG) }))
                .chain([json!({ "image": data_url("image/png", &big) })])
                .collect::<Vec<_>>()
        });
        assert_eq!(take_images(Some(&mut data)).len(), MAX_IMAGES);
        let mut data = json!({ "attachments": [{ "image": data_url("image/png", &big) }] });
        assert!(take_images(Some(&mut data)).is_empty());
        assert!(!data.to_string().contains("base64"));
        assert!(take_images(None).is_empty());
    }

    #[test]
    fn she_is_told_what_came_with_the_message() {
        let data = json!({
            "attachments": [
                { "name": "a.png", "mime": "image/png" },
                { "name": "todo.md", "mime": "text/markdown", "text": "  买牛奶  " },
            ]
        });
        let attached = format_attached(Some(&data), 1);
        assert!(attached.starts_with("They attached 1 image to this message; you can see it."));
        assert!(attached.contains("Attached file \"todo.md\":\n买牛奶"));
        assert_eq!(format_attached(None, 0), "");
    }
}
