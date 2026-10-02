import type { Anime25DPlayback } from '../anime25drig/types'
import type {
  CharacterAssetProfile,
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
  torsoTwistScale?: number
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

/** Legacy contour-only sources remain unchanged. */
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
  /** Absent on a bust. */
  profile?: CharacterAssetProfile
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
