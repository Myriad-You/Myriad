#[derive(Debug, Clone, Copy)]
pub(crate) struct RigOutfitSafetyContract {
    pub torso_twist_scale: f32,
    pub secondary_motion_scale: f32,
}

/// What one character-asset mode asks of its master portrait and its rig.
#[derive(Debug, Clone, Copy)]
pub struct CharacterAssetProfileContract {
    pub contract_version: u16,
    pub aspect_width: u32,
    pub aspect_height: u32,
    pub generation_width: u32,
    pub generation_height: u32,
    pub canvas_width: f32,
    pub canvas_height: f32,
    pub framing: &'static str,
    pub background: &'static str,
    pub max_rigid_arm_rotation_degrees: f32,
    pub required_capabilities: &'static [&'static str],
}

/// The two kinds of character asset: the bust the panel shows, and the
/// optional standing full figure. Each has its own portrait, rig and contract
/// version, so changing one never invalidates the other.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum CharacterAssetProfile {
    #[default]
    Bust,
    FullBody,
}

impl CharacterAssetProfile {
    pub const fn contract(self) -> &'static CharacterAssetProfileContract {
        match self {
            Self::Bust => &BUST_ASSET_CONTRACT,
            Self::FullBody => &FULL_BODY_ASSET_CONTRACT,
        }
    }

    /// Manifests written before the full-body mode carry no profile; they are busts.
    pub fn is_bust(&self) -> bool {
        *self == Self::Bust
    }
}

include!(concat!(env!("OUT_DIR"), "/merope_rig_contract.rs"));

const _: () = assert!(MAX_GPU_RIG_BONES >= MAX_RIG_BONES);

#[cfg(test)]
mod tests {
    use std::fs;

    /// `shared/merope_*_contract.json` is only a contract if this crate
    /// generates from it. A third file next to the two readers is a
    /// document that looks authoritative and is not.
    #[test]
    fn shared_merope_contracts_are_exactly_the_ones_this_crate_generates() {
        let shared = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shared");
        let mut names: Vec<String> = fs::read_dir(&shared)
            .expect("shared/")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                (name.starts_with("merope_") && name.ends_with("_contract.json")).then_some(name)
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "merope_performance_contract.json".to_string(),
                "merope_rig_contract.json".to_string()
            ]
        );
    }
}
