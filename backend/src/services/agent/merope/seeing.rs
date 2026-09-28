//! Pictures in groups (see `myriad_merope::seeing`): looked at when she reads
//! the talk they are in, known again by their key without looking twice, and
//! counted a month at a time. Fetching a picture is the group's (it knows
//! the platform and its token).

use std::time::Duration;

use base64::Engine as _;
use myriad_merope::seeing::{MONTHLY, Seen, parse, prompt};
use sea_orm::DatabaseConnection;
use sha2::{Digest, Sha256};

use super::call::{self, Voice};
use crate::services::runtime_registry as registry;

const SEEN: &str = "merope_seen";
const LOOKS: &str = "merope_looks";
/// What she saw of a picture is kept this long.
const KEEP_DAYS: i64 = 30;
const CALL_TIMEOUT: Duration = Duration::from_secs(40);
/// Pictures go to the model no larger than this across.
const EDGE: u32 = 768;

fn identity() -> registry::RegistryIdentity<'static> {
    registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

fn record(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// What she saw of the picture under `key` before, if she did lately.
pub async fn known(db: &DatabaseConnection, key: &str) -> Option<Seen> {
    registry::get(db, SEEN, &record(key)).await.ok().flatten()
}

/// Look at a picture (its bytes, and what the app calls it), billed to
/// `owner`; kept under `key` so it is known again. None when this month's
/// looks are used up, or the picture cannot be read.
pub async fn look(
    db: &DatabaseConnection,
    owner: i32,
    key: &str,
    bytes: Vec<u8>,
    hint: Option<&str>,
) -> Option<Seen> {
    if let Some(seen) = known(db, key).await {
        return Some(seen);
    }
    let month = chrono::Local::now().format("%Y-%m").to_string();
    let until = (chrono::Utc::now() + chrono::Duration::days(40)).timestamp();
    let looked = registry::increment(db, LOOKS, &month, until).await.ok()?;
    if looked as usize > MONTHLY {
        tracing::info!("[Merope] no more pictures to look at this month");
        return None;
    }
    let picture = tokio::task::spawn_blocking(move || small(&bytes))
        .await
        .ok()
        .flatten()?;
    let raw = call::Ask::new(Voice::HersAtLength, owner, "look")
        .within(CALL_TIMEOUT)
        .model()
        .await
        .ok()?
        .look(&prompt(hint), &[picture])
        .await
        .ok()?;
    let seen = parse(&raw)?;
    let keep_until = (chrono::Utc::now() + chrono::Duration::days(KEEP_DAYS)).timestamp();
    let _ = registry::put(db, SEEN, &record(key), identity(), &seen, keep_until).await;
    Some(seen)
}

/// The picture as the model takes it: its first frame, no larger than
/// `EDGE` across, as PNG.
fn small(bytes: &[u8]) -> Option<crate::services::analyzer::ImageInput> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width().max(image.height()) > EDGE {
        image.resize(EDGE, EDGE, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut png = Vec::new();
    image
        .to_rgba8()
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(crate::services::analyzer::ImageInput {
        mime: "image/png".to_string(),
        base64: base64::engine::general_purpose::STANDARD.encode(png),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_picture_goes_to_the_model_small() {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(2000, 1000))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let input = super::small(&png).unwrap();
        assert_eq!(input.mime, "image/png");
        use base64::Engine as _;
        let back = image::load_from_memory(
            &base64::engine::general_purpose::STANDARD
                .decode(input.base64)
                .unwrap(),
        )
        .unwrap();
        assert_eq!((back.width(), back.height()), (768, 384));
        assert!(super::small(b"not a picture").is_none());
    }
}
