import contract from '../../../../../shared/merope_rig_contract.json'

export const RIG_SCHEMA_VERSION = contract.schemaVersion
export const RIG_IR_VERSION = contract.rigIrVersion
export const MIN_SUPPORTED_RIG_IR_VERSION = contract.minSupportedRigIrVersion
export const CHARACTER_ASSET_CONTRACT_VERSION =
  contract.characterAsset.contractVersion
export const PORTRAIT_CANVAS = contract.characterAsset.portrait.canvas
export const CHARACTER_ASSET_REQUIRED_CAPABILITIES =
  contract.characterAsset.rig.requiredCapabilities
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
