import {
  isAnime25DPlayback,
  type Anime25DPlayback,
} from '../anime25drig/types'
import {
  CHARACTER_ASSET_CONTRACT_VERSION,
  MAX_RIG_BONES,
  MAX_RIG_COLLISION_VOLUMES,
  MAX_RIG_PARTS,
  MAX_RIG_TEXTURES,
  MAX_RIG_TOTAL_VERTICES,
  MAX_RIG_VERTICES_PER_PART,
  MIN_SUPPORTED_RIG_IR_VERSION,
  RIG_IR_VERSION,
  RIG_PRESENTATION_SLOTS,
  RIG_SCHEMA_VERSION,
  RIG_SEMANTIC_BONE_ROLES,
  RIG_SEMANTIC_CHAIN_ROLES,
} from './contract'

export type RigQuality = 'layered-2d'

export interface RigPoint {
  x: number
  y: number
}

export interface RigSize {
  width: number
  height: number
}

export type RigRect = RigPoint & RigSize

export interface RigTexture {
  id: string
  url: string
  width: number
  height: number
}

export interface RigBone {
  id: string
  parent: string | null
  pivot: RigPoint
}

export type RigSemanticBoneRole = (typeof RIG_SEMANTIC_BONE_ROLES)[number]
export type RigSemanticChainRole = (typeof RIG_SEMANTIC_CHAIN_ROLES)[number]

/** Versioned semantic IR decouples runtime behavior from source bone names. */
export interface RigSemantics {
  bones: Partial<Record<RigSemanticBoneRole, string>>
  chains: Partial<Record<RigSemanticChainRole, string[]>>
  secondaryBoneIds: string[]
}

export const RIG_OUTFIT_TOPOLOGIES = [
  'fitted',
  'short-skirt',
  'long-skirt',
  'long-coat',
  'wide-sleeve',
  'cape',
  'armor',
] as const

export type RigOutfitTopology = (typeof RIG_OUTFIT_TOPOLOGIES)[number]

export interface RigOutfitProfile {
  topologies: RigOutfitTopology[]
  secondaryPartIds: string[]
  /** Optional for manifests compiled before adaptive torso safety was added. */
  torsoTwistScale?: number
  /** Optional for manifests compiled before garment-aware spring tuning was added. */
  secondaryMotionScale?: number
}

export interface RigSemanticAnchor {
  boneId: string
  offset: RigPoint
}

export interface RigCollisionVolume {
  id: string
  boneId: string
  offset: RigPoint
  radius: RigPoint
  padding: number
}

/** Character-local body volumes consumed by final-pose contact and collision. */
export interface RigSpatialProfile {
  collisionVolumes: RigCollisionVolume[]
}

export interface RigVertex {
  position: RigPoint
  uv: RigPoint
  joints: [number, number, number, number]
  weights: [number, number, number, number]
}

export interface RigPart {
  id: string
  textureId: string
  zIndex: number
  opacity: number
  slot?: string
  variant?: string
  vertices: RigVertex[]
  indices: number[]
}

export interface RigMotionProfile {
  seed: number
  breath: {
    minFrequencyHz: number
    maxFrequencyHz: number
    amplitude: number
  }
  blink: {
    minIntervalSeconds: number
    maxIntervalSeconds: number
    durationSeconds: number
    doubleChance: number
  }
  secondary: {
    enabled: boolean
    frequencyHz: number
    dampingRatio: number
    response: number
  }
}

export interface RigBoneHandle {
  boneId: string
  start: RigPoint
  end: RigPoint
  falloff: number
}

/**
 * Optional importer-authored topology for layers that need interior vertices.
 * The backend still owns UV generation and skin-weight normalization. Legacy
 * contour-only sources remain unchanged.
 */
export interface RigLayerMeshSource {
  vertices: RigPoint[]
  indices: number[]
}

export interface RigLayerSource {
  id: string
  textureId: string
  textureBounds: RigRect
  zIndex: number
  opacity: number
  slot?: string
  variant?: string
  contours: RigPoint[][]
  mesh?: RigLayerMeshSource
  boneHandles: RigBoneHandle[]
}

export interface MeropeRigCompileRequest {
  rigIrVersion?: number
  characterAssetContractVersion: number
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  canvas: RigSize
  textures: Array<{
    id: string
    assetId: string
    width: number
    height: number
  }>
  bones: RigBone[]
  layers: RigLayerSource[]
  motionProfile?: RigMotionProfile
  outfitProfile?: RigOutfitProfile
  semanticAnchors?: Record<string, RigSemanticAnchor>
  semantics?: RigSemantics
  spatialProfile?: RigSpatialProfile
}

export interface MeropeRigImportSource {
  rigIrVersion?: number
  characterAssetContractVersion: number
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  canvas: RigSize
  atlas: {
    id: string
    width: number
    height: number
  }
  bones: RigBone[]
  layers: RigLayerSource[]
  motionProfile?: RigMotionProfile
  outfitProfile?: RigOutfitProfile
  semanticAnchors?: Record<string, RigSemanticAnchor>
  semantics?: RigSemantics
  spatialProfile?: RigSpatialProfile
  anime25dPlayback?: Anime25DPlayback
}

export interface MeropeRigManifest {
  schemaVersion: number
  rigIrVersion?: number
  characterAssetContractVersion?: number
  sourceMasterAssetId?: string
  sourceGenerationFingerprint?: string
  quality: RigQuality
  canvas: RigSize
  textures: RigTexture[]
  bones: RigBone[]
  parts: RigPart[]
  motionProfile?: RigMotionProfile
  outfitProfile?: RigOutfitProfile
  semanticAnchors?: Record<string, RigSemanticAnchor>
  semantics?: RigSemantics
  spatialProfile?: RigSpatialProfile
  anime25dPlayback?: Anime25DPlayback
}

export function isLiveMeropeManifest(
  value: unknown,
): value is MeropeRigManifest {
  return (
    isRigManifest(value) &&
    value.quality === 'layered-2d' &&
    isAnime25DPlayback(value.anime25dPlayback)
  )
}

export function isRigManifest(value: unknown): value is MeropeRigManifest {
  if (!isRecord(value)) return false
  if (
    value.schemaVersion !== RIG_SCHEMA_VERSION ||
    value.quality !== 'layered-2d' ||
    !isPositiveSize(value.canvas) ||
    !Array.isArray(value.textures) ||
    !Array.isArray(value.bones) ||
    !Array.isArray(value.parts) ||
    value.clips != null ||
    value.defaultClip != null ||
    value.standardClipLibraryVersion != null ||
    value.bones.length === 0 ||
    value.bones.length > MAX_RIG_BONES ||
    value.textures.length === 0 ||
    value.textures.length > MAX_RIG_TEXTURES ||
    value.parts.length === 0 ||
    value.parts.length > MAX_RIG_PARTS
  ) {
    return false
  }
  if (
    value.rigIrVersion !== undefined &&
    (typeof value.rigIrVersion !== 'number' ||
      !Number.isInteger(value.rigIrVersion) ||
      value.rigIrVersion < MIN_SUPPORTED_RIG_IR_VERSION ||
      value.rigIrVersion > RIG_IR_VERSION)
  ) {
    return false
  }
  if (
    value.characterAssetContractVersion !== undefined &&
    value.characterAssetContractVersion !== CHARACTER_ASSET_CONTRACT_VERSION
  ) {
    return false
  }
  if (
    value.sourceMasterAssetId !== undefined &&
    (typeof value.sourceMasterAssetId !== 'string' ||
      value.sourceMasterAssetId.trim() === '' ||
      value.sourceMasterAssetId.length > 512)
  ) {
    return false
  }
  if (
    value.sourceGenerationFingerprint !== undefined &&
    (typeof value.sourceGenerationFingerprint !== 'string' ||
      !/^[0-9a-f]{64}$/iu.test(value.sourceGenerationFingerprint))
  ) {
    return false
  }
  if (
    value.motionProfile !== undefined &&
    !isMotionProfile(value.motionProfile)
  ) {
    return false
  }
  if (
    value.outfitProfile !== undefined &&
    !isOutfitProfile(value.outfitProfile, value.parts)
  ) {
    return false
  }
  if (
    value.anime25dPlayback !== undefined &&
    !isAnime25DPlayback(value.anime25dPlayback)
  ) {
    return false
  }
  const textures = value.textures
  const bones = value.bones
  const parts = value.parts
  const textureIds = new Set<string>()
  for (const texture of textures) {
    if (
      !isRecord(texture) ||
      !addIdentifier(textureIds, texture.id) ||
      typeof texture.url !== 'string' ||
      texture.url.trim() === '' ||
      !isPositiveNumber(texture.width) ||
      !isPositiveNumber(texture.height) ||
      texture.width > 16_384 ||
      texture.height > 16_384
    ) {
      return false
    }
  }
  const boneIds = new Set<string>()
  for (const bone of bones) {
    if (
      !isRecord(bone) ||
      !addIdentifier(boneIds, bone.id) ||
      !(bone.parent === null || typeof bone.parent === 'string') ||
      !isPoint(bone.pivot)
    ) {
      return false
    }
  }
  if (
    bones.some((bone) => bone.parent !== null && !boneIds.has(bone.parent)) ||
    hasBoneCycle(bones)
  ) {
    return false
  }
  if (
    value.semanticAnchors !== undefined &&
    (!isRecord(value.semanticAnchors) ||
      Object.entries(value.semanticAnchors).some(
        ([name, anchor]) =>
          name.length === 0 ||
          name.length > 64 ||
          !isRecord(anchor) ||
          typeof anchor.boneId !== 'string' ||
          !boneIds.has(anchor.boneId) ||
          !isPoint(anchor.offset),
      ))
  ) {
    return false
  }
  if (
    value.semantics !== undefined &&
    !isRigSemantics(value.semantics, bones)
  ) {
    return false
  }
  if (
    value.spatialProfile !== undefined &&
    !isSpatialProfile(value.spatialProfile, boneIds, value.canvas)
  ) {
    return false
  }
  if (
    value.rigIrVersion !== undefined &&
    (value.semantics === undefined || value.spatialProfile === undefined)
  ) {
    return false
  }
  const partIds = new Set<string>()
  const variantsBySlot = new Map<string, Set<string>>()
  let totalVertices = 0
  for (const part of parts) {
    if (
      !isRecord(part) ||
      !addIdentifier(partIds, part.id) ||
      !textureIds.has(String(part.textureId)) ||
      !isFiniteNumber(part.zIndex) ||
      !isUnitNumber(part.opacity) ||
      (part.slot !== undefined &&
        (typeof part.slot !== 'string' || part.slot.length === 0)) ||
      (part.variant !== undefined &&
        (typeof part.variant !== 'string' || part.variant.length === 0)) ||
      (part.slot === undefined) !== (part.variant === undefined) ||
      !Array.isArray(part.vertices) ||
      !Array.isArray(part.indices) ||
      part.vertices.length < 3 ||
      part.vertices.length > MAX_RIG_VERTICES_PER_PART ||
      part.indices.length < 3 ||
      part.indices.length % 3 !== 0
    ) {
      return false
    }
    if (
      value.rigIrVersion === RIG_IR_VERSION &&
      part.slot !== undefined &&
      part.variant !== undefined
    ) {
      const definition = (
        RIG_PRESENTATION_SLOTS as Record<
          string,
          { fallback: string; variants: readonly string[] }
        >
      )[part.slot]
      if (!definition || !definition.variants.includes(part.variant)) {
        return false
      }
      const variants = variantsBySlot.get(part.slot) ?? new Set<string>()
      variants.add(part.variant)
      variantsBySlot.set(part.slot, variants)
    }
    const vertices = part.vertices
    const indices = part.indices
    totalVertices += vertices.length
    if (totalVertices > MAX_RIG_TOTAL_VERTICES) return false
    if (
      !vertices.every((vertex) => isVertex(vertex, bones.length)) ||
      !indices.every(
        (index) =>
          Number.isInteger(index) && index >= 0 && index < vertices.length,
      )
    ) {
      return false
    }
  }
  for (const [slot, variants] of variantsBySlot) {
    const definition = (
      RIG_PRESENTATION_SLOTS as Record<string, { fallback: string }>
    )[slot]
    if (!variants.has(definition.fallback)) return false
  }
  return true
}

function isMotionProfile(value: unknown): value is RigMotionProfile {
  if (
    !isRecord(value) ||
    !Number.isInteger(value.seed) ||
    (value.seed as number) < 0 ||
    (value.seed as number) > 4_294_967_295 ||
    !isRecord(value.breath) ||
    !isRecord(value.blink) ||
    !isRecord(value.secondary)
  ) {
    return false
  }
  const breath = value.breath
  const blink = value.blink
  const secondary = value.secondary
  return (
    isNumberInRange(breath.minFrequencyHz, 0.05, 2) &&
    isNumberInRange(
      breath.maxFrequencyHz,
      breath.minFrequencyHz as number,
      2,
    ) &&
    isNumberInRange(breath.amplitude, 0, 0.05) &&
    isNumberInRange(blink.minIntervalSeconds, 0.5, 30) &&
    isNumberInRange(
      blink.maxIntervalSeconds,
      blink.minIntervalSeconds as number,
      30,
    ) &&
    isNumberInRange(blink.durationSeconds, 0.05, 1) &&
    isUnitNumber(blink.doubleChance) &&
    typeof secondary.enabled === 'boolean' &&
    isNumberInRange(secondary.frequencyHz, 0.1, 12) &&
    isNumberInRange(secondary.dampingRatio, 0.05, 3) &&
    isNumberInRange(secondary.response, 0, 2)
  )
}

function isRigSemantics(
  value: unknown,
  bones: RigBone[],
): value is RigSemantics {
  if (
    !isRecord(value) ||
    !isRecord(value.bones) ||
    !isRecord(value.chains) ||
    !Array.isArray(value.secondaryBoneIds)
  ) {
    return false
  }
  const boneIds = new Set(bones.map((bone) => bone.id))
  const roles = new Set<string>(RIG_SEMANTIC_BONE_ROLES)
  const chainRoles = new Set<string>(RIG_SEMANTIC_CHAIN_ROLES)
  if (
    Object.entries(value.bones).some(
      ([role, boneId]) =>
        !roles.has(role) || typeof boneId !== 'string' || !boneIds.has(boneId),
    ) ||
    Object.entries(value.chains).some(
      ([role, chain]) =>
        !chainRoles.has(role) ||
        !Array.isArray(chain) ||
        chain.length < 2 ||
        chain.length > bones.length ||
        new Set(chain).size !== chain.length ||
        chain.some(
          (boneId) => typeof boneId !== 'string' || !boneIds.has(boneId),
        ) ||
        !isConnectedBoneChain(chain as string[], bones),
    ) ||
    value.secondaryBoneIds.length > bones.length ||
    new Set(value.secondaryBoneIds).size !== value.secondaryBoneIds.length ||
    value.secondaryBoneIds.some(
      (boneId) => typeof boneId !== 'string' || !boneIds.has(boneId),
    )
  ) {
    return false
  }
  return true
}

function isConnectedBoneChain(chain: string[], bones: RigBone[]): boolean {
  const parents = new Map(bones.map((bone) => [bone.id, bone.parent]))
  return chain.slice(1).every((boneId, index) => {
    const previous = chain[index]
    return parents.get(boneId) === previous || parents.get(previous) === boneId
  })
}

function isSpatialProfile(
  value: unknown,
  boneIds: Set<string>,
  canvas: RigSize,
): value is RigSpatialProfile {
  if (!isRecord(value) || !Array.isArray(value.collisionVolumes)) return false
  if (value.collisionVolumes.length > MAX_RIG_COLLISION_VOLUMES) return false
  const ids = new Set<string>()
  return value.collisionVolumes.every(
    (volume) =>
      isRecord(volume) &&
      addIdentifier(ids, volume.id) &&
      typeof volume.boneId === 'string' &&
      boneIds.has(volume.boneId) &&
      isPoint(volume.offset) &&
      isPoint(volume.radius) &&
      volume.radius.x > 0 &&
      volume.radius.y > 0 &&
      volume.radius.x <= canvas.width &&
      volume.radius.y <= canvas.height &&
      Math.abs(volume.offset.x) <= canvas.width * 2 &&
      Math.abs(volume.offset.y) <= canvas.height * 2 &&
      isNumberInRange(volume.padding, 0, Math.max(canvas.width, canvas.height)),
  )
}

function isOutfitProfile(
  value: unknown,
  parts: unknown[],
): value is RigOutfitProfile {
  if (!isRecord(value)) return false
  const topologies = new Set<RigOutfitTopology>(RIG_OUTFIT_TOPOLOGIES)
  const partIds = new Set(
    parts.flatMap((part) =>
      isRecord(part) && typeof part.id === 'string' ? [part.id] : [],
    ),
  )
  return (
    Array.isArray(value.topologies) &&
    value.topologies.length > 0 &&
    value.topologies.length <= topologies.size &&
    new Set(value.topologies).size === value.topologies.length &&
    value.topologies.every(
      (topology) =>
        typeof topology === 'string' &&
        topologies.has(topology as RigOutfitTopology),
    ) &&
    Array.isArray(value.secondaryPartIds) &&
    value.secondaryPartIds.length <= MAX_RIG_PARTS &&
    new Set(value.secondaryPartIds).size === value.secondaryPartIds.length &&
    value.secondaryPartIds.every(
      (partId) => typeof partId === 'string' && partIds.has(partId),
    ) &&
    (value.torsoTwistScale === undefined ||
      isNumberInRange(value.torsoTwistScale, 0.25, 1)) &&
    (value.secondaryMotionScale === undefined ||
      isNumberInRange(value.secondaryMotionScale, 0.2, 1))
  )
}

function hasBoneCycle(bones: RigBone[]): boolean {
  const parents = new Map(bones.map((bone) => [bone.id, bone.parent]))
  const complete = new Set<string>()
  const active = new Set<string>()
  const visit = (boneId: string): boolean => {
    if (complete.has(boneId)) return false
    if (active.has(boneId)) return true
    active.add(boneId)
    const parent = parents.get(boneId)
    if (parent && visit(parent)) return true
    active.delete(boneId)
    complete.add(boneId)
    return false
  }
  return bones.some((bone) => visit(bone.id))
}

function isVertex(value: unknown, boneCount: number): value is RigVertex {
  if (
    !isRecord(value) ||
    !isPoint(value.position) ||
    !isPoint(value.uv) ||
    !isTuple4(value.joints) ||
    !isTuple4(value.weights)
  ) {
    return false
  }
  if (
    !value.joints.every(
      (joint) => Number.isInteger(joint) && joint >= 0 && joint < boneCount,
    ) ||
    !value.weights.every(isUnitNumber)
  ) {
    return false
  }
  return (
    Math.abs(value.weights.reduce((sum, weight) => sum + weight, 0) - 1) <=
    0.002
  )
}

function isPositiveSize(value: unknown): value is RigSize {
  return (
    isRecord(value) &&
    isPositiveNumber(value.width) &&
    isPositiveNumber(value.height)
  )
}

function isPoint(value: unknown): value is RigPoint {
  return isRecord(value) && isFiniteNumber(value.x) && isFiniteNumber(value.y)
}

function isTuple4(value: unknown): value is [number, number, number, number] {
  return (
    Array.isArray(value) && value.length === 4 && value.every(isFiniteNumber)
  )
}

function addIdentifier(ids: Set<string>, value: unknown): boolean {
  if (typeof value !== 'string' || value.trim() === '' || ids.has(value)) {
    return false
  }
  ids.add(value)
  return true
}

function isPositiveNumber(value: unknown): value is number {
  return isFiniteNumber(value) && value > 0
}

function isNumberInRange(
  value: unknown,
  minimum: number,
  maximum: number,
): value is number {
  return isFiniteNumber(value) && value >= minimum && value <= maximum
}

function isUnitNumber(value: unknown): value is number {
  return isFiniteNumber(value) && value >= 0 && value <= 1
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value)
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}
