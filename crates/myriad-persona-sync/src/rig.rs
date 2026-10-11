//! A compiled rig's id: its content, so every side names the same package
//! the same way.

use myriad_merope::RigManifest;
use sha2::{Digest, Sha256};

use crate::canonical::{canonical_bytes, hex};

/// Content identity for the complete immutable package: the atlas bytes and
/// the canonical manifest, under a domain prefix. Runtime asset URLs are
/// normalized out so the package id does not recursively hash itself, and
/// the atlas file's name does not matter.
pub fn rig_package_id(atlas: &[u8], manifest: &RigManifest) -> serde_json::Result<String> {
    let mut normalized = manifest.clone();
    for texture in &mut normalized.textures {
        texture.url = format!("asset://atlas/{}", texture.id);
    }
    let canonical_manifest = canonical_bytes(&serde_json::to_value(normalized)?);

    let mut hasher = Sha256::new();
    hasher.update(b"myriad-rig-package-v1\0");
    hasher.update((atlas.len() as u64).to_be_bytes());
    hasher.update(atlas);
    hasher.update((canonical_manifest.len() as u64).to_be_bytes());
    hasher.update(canonical_manifest);
    Ok(hex(&hasher.finalize()))
}
