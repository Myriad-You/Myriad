#[derive(Debug, Clone, Copy)]
pub(crate) struct RigOutfitSafetyContract {
    pub torso_twist_scale: f32,
    pub secondary_motion_scale: f32,
}

include!(concat!(env!("OUT_DIR"), "/merope_rig_contract.rs"));

const _: () = assert!(MAX_GPU_RIG_BONES >= MAX_RIG_BONES);
