//! Pictures in groups (see `myriad_merope::seeing`): looked at when she reads
//! the talk they are in, known again by their key without looking twice, and
//! counted a month at a time. Fetching a picture is the group's (it knows
//! the platform and its token).

use std::time::Duration;

use base64::Engine as _;
use myriad_merope::seeing::{MONTHLY, Seen, parse, prompt, sticker_prompt};
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

/// What she saw is kept under this, so what was kept while pictures did not
/// reach the model (the Anthropic protocol sent the text alone) is not read
/// back as if she had seen them; those records run out on their own.
const SEEN_SINCE: &str = "v2";

fn record(key: &str) -> String {
    hex::encode(Sha256::digest(format!("{SEEN_SINCE}:{key}").as_bytes()))
}

/// What she saw of the picture under `key` before, if she did lately.
pub async fn known(db: &DatabaseConnection, key: &str) -> Option<Seen> {
    registry::get(db, SEEN, &record(key)).await.ok().flatten()
}

/// Look at a picture (its bytes, and what the app calls it), billed to
/// `owner`; kept under `key` so it is known again. A sticker is looked at as
/// she is (`soul`): whether she would keep it to send herself. None when this
/// month's looks are used up, or the picture cannot be read.
pub async fn look(
    db: &DatabaseConnection,
    owner: i32,
    key: &str,
    bytes: Vec<u8>,
    hint: Option<&str>,
    soul: Option<&str>,
) -> Option<Seen> {
    if let Some(seen) = known(db, key).await {
        return Some(seen);
    }
    let month = super::clock::local_now().format("%Y-%m").to_string();
    let until = (crate::clock::now() + chrono::Duration::days(40)).timestamp();
    let looked = registry::increment(db, LOOKS, &month, until).await.ok()?;
    if looked as usize > MONTHLY {
        tracing::info!("[Merope] no more pictures to look at this month");
        return None;
    }
    let seen = looked_at(owner, bytes, hint, soul).await?;
    let keep_until = (crate::clock::now() + chrono::Duration::days(KEEP_DAYS)).timestamp();
    let _ = registry::put(db, SEEN, &record(key), identity(), &seen, keep_until).await;
    Some(seen)
}

/// What the model makes of a picture, as [`look`] asks it.
async fn looked_at(
    owner: i32,
    bytes: Vec<u8>,
    hint: Option<&str>,
    soul: Option<&str>,
) -> Option<Seen> {
    let picture = tokio::task::spawn_blocking(move || small(&bytes))
        .await
        .ok()
        .flatten()?;
    let raw = call::Ask::new(Voice::HersAtLength, owner, "look")
        .within(CALL_TIMEOUT)
        .model()
        .await
        .ok()?
        .look(
            &soul.map_or_else(|| prompt(hint), |soul| sticker_prompt(hint, soul)),
            &[picture],
        )
        .await
        .ok()?;
    parse(&raw)
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
    /// What the site's model, looking as she is, makes of a sticker in her
    /// own style (she would keep it) and of a settings icon (printed: whether
    /// she keeps it is her taste). Run when the protocol or the model changes:
    /// the picture has to reach the model.
    /// `cargo test --bin myriad-backend a_sticker_she_sees -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "spends on the site's model"]
    async fn a_sticker_she_sees_as_she_is() {
        let config_db = super::super::super::semantic_eval::load_configured_lite().await;
        config_db.close().await.ok();
        let soul = crate::services::agent::identity::get_speaking_soul()
            .await
            .unwrap_or_else(|| "一个爱吐槽、喜欢可爱东西的女生".to_string());
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |path: &str| std::fs::read(root.join(path)).unwrap();
        let hers = super::looked_at(
            1,
            read("assets/merope/sticker-style-reference.webp"),
            None,
            Some(&soul),
        )
        .await
        .expect("looked at");
        println!("her style: {hers:?}");
        assert!(hers.keep, "{hers:?}");
        let icon = super::looked_at(
            1,
            read("../frontend/public/icons/config/advanced.webp"),
            None,
            Some(&soul),
        )
        .await
        .expect("looked at");
        println!("a settings icon: {icon:?}");
    }

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
