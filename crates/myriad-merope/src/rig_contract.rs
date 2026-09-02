#[derive(Debug, Clone, Copy)]
pub(crate) struct RigOutfitSafetyContract {
    pub torso_twist_scale: f32,
    pub secondary_motion_scale: f32,
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
