import contract from '../../../../../shared/merope_rig_contract.json' with { type: 'json' }

export const RIG_SCHEMA_VERSION = contract.schemaVersion
export const RIG_IR_VERSION = contract.rigIrVersion
export const MIN_SUPPORTED_RIG_IR_VERSION = contract.minSupportedRigIrVersion
/**
 * The two kinds of character asset: the bust the panel shows, and the
 * optional standing full figure. Each has its own portrait, rig and contract
 * version, so changing one never invalidates the other.
 */
export const CHARACTER_ASSET_PROFILES = contract.characterAsset.profiles
export type CharacterAssetProfile = keyof typeof CHARACTER_ASSET_PROFILES

export function isCharacterAssetProfile(
  value: unknown,
): value is CharacterAssetProfile {
  return value === 'bust' || value === 'fullBody'
}

/** Manifests written before the full-body mode carry no profile; they are busts. */
export function characterAssetProfileOf(manifest: {
  profile?: CharacterAssetProfile
}): CharacterAssetProfile {
  return manifest.profile ?? 'bust'
}
export const RIG_SEMANTIC_BONE_ROLES =
  contract.semanticBoneRoles as unknown as readonly [
    'root',
    'torso',
    'head',
    'face',
    'left-eye',
    'right-eye',
    'mouth',
    'handwear',
  ]
export const RIG_SEMANTIC_CHAIN_ROLES =
  contract.semanticChainRoles as unknown as readonly ['torso']
export const MAX_RIG_BONES = contract.limits.maxBones
export const RIG_MATRIX_CAPACITY = contract.limits.maxGpuBones
export const MAX_RIG_TEXTURES = contract.limits.maxTextures
export const MAX_RIG_PARTS = contract.limits.maxParts
export const MAX_RIG_VERTICES_PER_PART = contract.limits.maxVerticesPerPart
export const MAX_RIG_TOTAL_VERTICES = contract.limits.maxTotalVertices
export const MAX_RIG_COLLISION_VOLUMES = contract.limits.maxCollisionVolumes
export const RIG_SECONDARY_PART_PATTERNS = contract.secondaryPartPatterns
export const RIG_PRESENTATION_SLOTS = contract.presentationSlots

export const RIG_OUTFIT_SAFETY = contract.outfitSafety

if (RIG_MATRIX_CAPACITY < MAX_RIG_BONES) {
  throw new Error(
    `Rig GPU capacity ${RIG_MATRIX_CAPACITY} is below manifest capacity ${MAX_RIG_BONES}`,
  )
}
