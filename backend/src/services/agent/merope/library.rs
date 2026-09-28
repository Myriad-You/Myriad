//! The library she picks a book to follow from (see `serial`): the catalogs
//! of Project Gutenberg and Aozora Bunko, fetched about once a week and kept
//! on disk as the works in them she could read (`myriad_merope::library`).
//! Both archives publish these catalogs for anyone to use; the books
//! themselves are fetched one at a time, only when she might read them.
//!
//! When a catalog cannot be fetched the last one kept stands in; with none
//! at all, the few books picked by hand do.

use std::time::Duration;

use chrono::{DateTime, Utc};
use myriad_merope::library::{AOZORA_CATALOG, GUTENBERG_CATALOG, from_aozora, from_gutenberg};
use myriad_merope::serial::{Source, Work};
use serde::{Deserialize, Serialize};

/// A catalog this old is fetched again.
const REFRESH_AFTER: chrono::Duration = chrono::Duration::days(7);
/// After a failed fetch, not again for this long.
const RETRY_AFTER: chrono::Duration = chrono::Duration::hours(6);
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_CATALOG_BYTES: usize = 64 * 1024 * 1024;
const USER_AGENT: &str = "MyriadSerialReader/1.0";

#[derive(Default, Serialize, Deserialize)]
struct Kept {
    /// When the catalogs were last fetched, or last tried.
    fetched: Option<DateTime<Utc>>,
    tried: Option<DateTime<Utc>>,
    works: Vec<Work>,
}

fn path() -> std::path::PathBuf {
    crate::services::data_paths::paths()
        .agent
        .join("serials")
        .join("library.json")
}

async fn kept() -> Kept {
    let Ok(bytes) = tokio::fs::read(path()).await else {
        return Kept::default();
    };
    tokio::task::spawn_blocking(move || serde_json::from_slice(&bytes).unwrap_or_default())
        .await
        .unwrap_or_default()
}

async fn keep(kept: &Kept) {
    let Ok(json) = serde_json::to_vec(kept) else {
        return;
    };
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    // Written whole, then moved into place: a reader never sees half.
    let partial = path.with_extension("json.partial");
    if tokio::fs::write(&partial, json).await.is_ok() {
        let _ = tokio::fs::rename(&partial, &path).await;
    }
}

async fn fetch(url: &str) -> Option<Vec<u8>> {
    let fetched = crate::services::outbound_security::get_public_following_redirects(
        url,
        FETCH_TIMEOUT,
        Some(USER_AGENT),
    )
    .await
    .ok()?;
    if !fetched.response.status().is_success() {
        tracing::info!(url, status = %fetched.response.status(), "[Merope] could not fetch a book catalog");
        return None;
    }
    crate::services::outbound_security::read_limited_body(fetched.response, MAX_CATALOG_BYTES)
        .await
        .ok()
}

async fn gutenberg() -> Option<Vec<Work>> {
    let bytes = fetch(GUTENBERG_CATALOG).await?;
    tokio::task::spawn_blocking(move || from_gutenberg(&String::from_utf8_lossy(&bytes)))
        .await
        .ok()
        .filter(|works| !works.is_empty())
}

async fn aozora() -> Option<Vec<Work>> {
    let bytes = fetch(AOZORA_CATALOG).await?;
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
        let file = archive.by_index(0).ok()?;
        let mut csv = String::new();
        file.take(MAX_CATALOG_BYTES as u64)
            .read_to_string(&mut csv)
            .ok()?;
        Some(from_aozora(&csv))
    })
    .await
    .ok()
    .flatten()
    .filter(|works| !works.is_empty())
}

static REFRESHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The works she could pick from; empty when no catalog was ever fetched.
pub async fn works() -> Vec<Work> {
    let _one_at_a_time = REFRESHING.lock().await;
    let mut kept = kept().await;
    let now = Utc::now();
    let stale = kept
        .fetched
        .is_none_or(|fetched| now.signed_duration_since(fetched) > REFRESH_AFTER);
    let tried_lately = kept
        .tried
        .is_some_and(|tried| now.signed_duration_since(tried) < RETRY_AFTER);
    if !stale || tried_lately {
        return kept.works;
    }
    let (english_and_chinese, japanese) = tokio::join!(gutenberg(), aozora());
    kept.tried = Some(now);
    let got_any = english_and_chinese.is_some() || japanese.is_some();
    // A catalog that could not be fetched keeps what it had.
    let old = std::mem::take(&mut kept.works);
    let from = |aozora: bool| {
        old.iter()
            .filter(move |work| matches!(work.source, Source::Aozora { .. }) == aozora)
            .cloned()
    };
    kept.works = english_and_chinese
        .unwrap_or_else(|| from(false).collect())
        .into_iter()
        .chain(japanese.unwrap_or_else(|| from(true).collect()))
        .collect();
    if got_any {
        kept.fetched = Some(now);
        tracing::info!(
            works = kept.works.len(),
            "[Merope] the library's catalogs are fetched"
        );
    }
    keep(&kept).await;
    kept.works
}

#[cfg(test)]
mod live {
    /// Fetch the real catalogs and show what the library holds.
    /// `cargo test … library_holds -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "fetches real catalogs"]
    async fn library_holds() {
        let english_and_chinese = super::gutenberg().await.expect("gutenberg");
        let japanese = super::aozora().await.expect("aozora");
        for lang in myriad_merope::library::LANGUAGES {
            let works: Vec<_> = english_and_chinese
                .iter()
                .chain(&japanese)
                .filter(|work| work.lang == lang)
                .collect();
            println!("{lang}: {} works", works.len());
            for work in works.iter().step_by(works.len().max(5) / 5).take(5) {
                println!(
                    "  {} | {} | {} | {}",
                    work.id, work.title, work.author, work.about
                );
            }
        }
    }
}
