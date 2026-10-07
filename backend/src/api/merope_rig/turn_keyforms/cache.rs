//! Cached drawings and decompositions, keyed by the portrait and providers.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::turn_prompts;
use crate::services::{image_generation, see_through};

/// Bump when what a cached picture or decomposition means changes.
const CACHE_VERSION: &str = "turn-keyforms-cache-v1";
/// Masters whose pictures and decompositions are kept; older ones are removed.
pub(super) const CACHE_KEEP: usize = 4;

/// The four turned drawings and five decompositions made for one master, on disk.
pub(super) struct TurnCache {
    dir: PathBuf,
}

impl TurnCache {
    /// Keyed by everything that decides them: the master's pixels, how the
    /// drawings are asked for, and where and how they are decomposed.
    pub(super) fn for_master(
        reference: &[u8],
        generation: &image_generation::ImageGenerationConfig,
        space: &str,
        options: see_through::DecomposeOptions,
    ) -> Self {
        Self::in_root(cache_root(), reference, generation, space, options)
    }

    fn in_root(
        root: PathBuf,
        reference: &[u8],
        generation: &image_generation::ImageGenerationConfig,
        space: &str,
        options: see_through::DecomposeOptions,
    ) -> Self {
        let mut hasher = Sha256::new();
        for part in [
            CACHE_VERSION,
            &generation.provider,
            &generation.model,
            space,
            see_through::CANVAS,
            &format!(
                "{}/{}/{}",
                options.resolution, options.seed, options.split_arms_and_legs
            ),
        ]
        .into_iter()
        .chain(turn_prompts().iter().map(String::as_str))
        {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        hasher.update(reference);
        let key = hex::encode(hasher.finalize());
        Self {
            dir: root.join(&key[..32]),
        }
    }

    /// Marks the master as just used, so pruning keeps it even when nothing new is written.
    pub(super) async fn touch(&self) {
        self.write("last-used", chrono::Utc::now().to_rfc3339().as_bytes())
            .await;
    }

    pub(super) async fn read(&self, name: &str) -> Option<Vec<u8>> {
        tokio::fs::read(self.dir.join(name))
            .await
            .ok()
            .filter(|bytes| !bytes.is_empty())
    }

    /// Written whole or not at all; a cache that cannot be written only costs a redo.
    pub(super) async fn write(&self, name: &str, bytes: &[u8]) {
        let result = async {
            tokio::fs::create_dir_all(&self.dir).await?;
            let partial = self.dir.join(format!(".{name}.partial"));
            tokio::fs::write(&partial, bytes).await?;
            tokio::fs::rename(&partial, self.dir.join(name)).await
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(%error, dir = %self.dir.display(), name, "turn keyforms cache write failed");
        }
    }
}

pub(super) fn cache_root() -> PathBuf {
    crate::services::data_paths::paths()
        .root
        .join("merope/turn-keyforms")
}

/// Keeps the masters touched last; a master in use is touched as it is read or written.
pub(super) async fn prune_cache(root: PathBuf, keep: usize) {
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return;
    };
    let mut dirs = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await
            && meta.is_dir()
        {
            dirs.push((meta.modified().ok(), entry.path()));
        }
    }
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, dir) in dirs.into_iter().skip(keep) {
        if let Err(error) = tokio::fs::remove_dir_all(&dir).await {
            tracing::warn!(%error, dir = %dir.display(), "turn keyforms cache prune failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn generation(model: &str) -> image_generation::ImageGenerationConfig {
        image_generation::ImageGenerationConfig {
            provider: "openai".into(),
            model: model.into(),
            api_key: "secret".into(),
            base_url: String::new(),
        }
    }

    #[tokio::test]
    async fn a_master_keeps_its_drawings_and_decompositions_across_jobs() {
        let root = scratch_dir();
        let options = see_through::DecomposeOptions::default();
        let cache = TurnCache::in_root(root.clone(), b"master", &generation("m"), "a/b", options);
        assert!(cache.read("turned-0.png").await.is_none());
        cache.write("turned-0.png", b"png").await;
        // The same master, asked for the same way, finds it; another master, model or Space does not.
        let again = TurnCache::in_root(root.clone(), b"master", &generation("m"), "a/b", options);
        assert_eq!(
            again.read("turned-0.png").await.as_deref(),
            Some(&b"png"[..])
        );
        for other in [
            TurnCache::in_root(root.clone(), b"other", &generation("m"), "a/b", options),
            TurnCache::in_root(root.clone(), b"master", &generation("n"), "a/b", options),
            TurnCache::in_root(root.clone(), b"master", &generation("m"), "a/c", options),
        ] {
            assert!(other.read("turned-0.png").await.is_none());
        }
        // The key never carries the provider's secret.
        assert!(!cache.dir.to_string_lossy().contains("secret"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn pruning_keeps_the_masters_used_last() {
        let root = scratch_dir();
        let options = see_through::DecomposeOptions::default();
        let caches: Vec<_> = (0..3u8)
            .map(|n| TurnCache::in_root(root.clone(), &[n], &generation("m"), "a/b", options))
            .collect();
        for cache in &caches {
            cache.touch().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        caches[0].touch().await;
        prune_cache(root.clone(), 2).await;
        assert!(caches[0].dir.exists());
        assert!(!caches[1].dir.exists());
        assert!(caches[2].dir.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    fn scratch_dir() -> PathBuf {
        std::env::temp_dir().join(format!("turn-cache-test-{}", uuid::Uuid::new_v4().simple()))
    }
}
