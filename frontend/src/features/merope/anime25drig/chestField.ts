import type { Anime25DChestProfile, Anime25DPlayback } from './types'

const CHEST_BONE_ID = 'a25d-chest'
const TOPWEAR_PART_ID = 'a25d-topwear'
const COORDINATE_EPSILON = 1e-5
const DEFAULT_SUPPORT_SCALE = 0.45
const DEFAULT_GARMENT_MOTION_SCALE = 0.65
const CHEST_LOBE_CENTER = 0.58
const CHEST_LOBE_RADIUS = 0.62
const SOFT_CENTER_BRIDGE = 0.7
const STRUCTURED_CENTER_BRIDGE = 0.36
const SOFT_DEPTH_RATIO = 0.34
const STRUCTURED_DEPTH_RATIO = 0.24
const SOFT_NEAR_DEPTH_GAIN = 0.35
const STRUCTURED_NEAR_DEPTH_GAIN = 0.18
const SOFT_FAR_DEPTH_GAIN = 0.15
const STRUCTURED_FAR_DEPTH_GAIN = 0.08
const SOFT_SILHOUETTE_RATIO = 0.065
const STRUCTURED_SILHOUETTE_RATIO = 0.03
const SOFT_BREATH_VOLUME_GAIN = 0.08
const STRUCTURED_BREATH_VOLUME_GAIN = 0.025

const CHEST_VERTICAL_CURVE = [
  { y: -1.2, weight: 0 },
  { y: -0.55, weight: 0.62 },
  { y: 0, weight: 1 },
  { y: 0.62, weight: 0.68 },
  { y: 1.35, weight: 0 },
] as const

interface ChestRigVertex {
  position: { x: number; y: number }
  joints: readonly number[]
  weights: readonly number[]
}

interface ChestRigSource {
  bones: ReadonlyArray<{ id: string }>
  parts: ReadonlyArray<{
    id: string
    vertices: readonly ChestRigVertex[]
  }>
}

export interface ChestWeightField {
  xs: Float32Array
  ys: Float32Array
  weights: Float32Array
}

export interface ChestSpatialField {
  source: Anime25DChestProfile['source']
  lobeCenter: number
  lobeRadius: number
  centerBridge: number
  depthRatio: number
  nearDepthGain: number
  farDepthGain: number
  silhouetteRatio: number
  breathVolumeGain: number
  breathMotionGain: number
}

type ChestWeightProfile = Pick<Anime25DChestProfile, 'enabled' | 'source'>

type ChestSpatialProfile = Pick<
  Anime25DChestProfile,
  'source' | 'supportScale' | 'garmentMotionScale'
>

export function chestProfileUsesGeometryWeights(
  profile: ChestWeightProfile,
): boolean {
  return profile.enabled && profile.source !== 'ai-vision'
}

export function resolveChestSpatialField(
  profile: ChestSpatialProfile,
): ChestSpatialField {
  const support = clamp(profile.supportScale, 0, 1)
  const garmentMotion = clamp(profile.garmentMotionScale, 0, 1)
  const structure = smoothstep(
    clamp(support * 0.65 + (1 - garmentMotion) * 0.35, 0, 1),
  )
  const breathTransmission =
    (1 - structure * 0.72) * (0.55 + garmentMotion * 0.45)
  return {
    source: profile.source,
    lobeCenter: CHEST_LOBE_CENTER,
    lobeRadius: CHEST_LOBE_RADIUS,
    centerBridge: mix(SOFT_CENTER_BRIDGE, STRUCTURED_CENTER_BRIDGE, structure),
    depthRatio: mix(SOFT_DEPTH_RATIO, STRUCTURED_DEPTH_RATIO, structure),
    nearDepthGain: mix(
      SOFT_NEAR_DEPTH_GAIN,
      STRUCTURED_NEAR_DEPTH_GAIN,
      structure,
    ),
    farDepthGain: mix(
      SOFT_FAR_DEPTH_GAIN,
      STRUCTURED_FAR_DEPTH_GAIN,
      structure,
    ),
    silhouetteRatio: mix(
      SOFT_SILHOUETTE_RATIO,
      STRUCTURED_SILHOUETTE_RATIO,
      structure,
    ),
    breathVolumeGain:
      mix(SOFT_BREATH_VOLUME_GAIN, STRUCTURED_BREATH_VOLUME_GAIN, structure) *
      (0.65 + garmentMotion * 0.35),
    breathMotionGain: breathTransmission,
  }
}

export function deriveGeometryChestProfile(
  playback: Readonly<
    Pick<Anime25DPlayback, 'pixelCanvas' | 'layers' | 'anchors'>
  >,
): Anime25DChestProfile {
  const { width, height } = playback.pixelCanvas
  const faceWidth = Math.max(
    1,
    Math.abs(playback.anchors.face.x1 - playback.anchors.face.x0),
  )
  const faceHeight = Math.max(
    1,
    Math.abs(playback.anchors.face.y1 - playback.anchors.face.y0),
  )
  const topwear = playback.layers.find((layer) => layer.role === 'topwear')
  let centerX = clamp(playback.anchors.neckPivot.x, 0, width)
  let centerY = clamp(playback.anchors.neckBottom + faceHeight * 0.5, 0, height)
  if (topwear && topwear.w > 0 && topwear.h > 0) {
    centerX = clamp(
      centerX,
      topwear.x + topwear.w * 0.15,
      topwear.x + topwear.w * 0.85,
    )
    const minimumY = clamp(
      Math.max(
        topwear.y + topwear.h * 0.2,
        playback.anchors.neckBottom + faceHeight * 0.12,
      ),
      0,
      height,
    )
    const maximumY = clamp(
      Math.min(
        topwear.y + topwear.h * 0.62,
        playback.anchors.neckBottom + faceHeight * 0.78,
      ),
      minimumY,
      height,
    )
    centerY = clamp(centerY, minimumY, maximumY)
  }
  return {
    version: 2,
    enabled: true,
    source: 'geometry-fallback',
    centerX,
    centerY,
    radiusX: clamp(faceWidth * 0.6, 1, width * 0.5),
    radiusY: clamp(faceHeight * 0.32, 1, height * 0.22),
    visibleScale: 0.5,
    motionScale: 1,
    frequencyScale: 1,
    supportScale: DEFAULT_SUPPORT_SCALE,
    garmentMotionScale: DEFAULT_GARMENT_MOTION_SCALE,
    confidence: 0,
  }
}

export function chestDeformationWeight(
  field: Readonly<ChestSpatialField>,
  normalizedX: number,
  normalizedY: number,
  skinWeight: number,
): number {
  const verticalWeight = sampleChestVerticalWeight(normalizedY)
  if (verticalWeight <= 0) return 0
  if (field.source !== 'ai-vision') {
    return (
      clamp(skinWeight, 0, 1) *
      Math.exp(-(normalizedX * normalizedX)) *
      verticalWeight
    )
  }
  const absoluteX = Math.abs(normalizedX)
  const lobeX = (absoluteX - field.lobeCenter) / field.lobeRadius
  const pairedWeight = Math.exp(-(lobeX * lobeX)) * verticalWeight
  const bridgeEased = smoothstep(absoluteX / field.lobeCenter)
  return (
    pairedWeight * (field.centerBridge + (1 - field.centerBridge) * bridgeEased)
  )
}

export function sampleChestVerticalWeight(normalizedY: number): number {
  if (!Number.isFinite(normalizedY)) return 0
  for (let index = 1; index < CHEST_VERTICAL_CURVE.length; index += 1) {
    const left = CHEST_VERTICAL_CURVE[index - 1]
    const right = CHEST_VERTICAL_CURVE[index]
    if (normalizedY > right.y) continue
    const progress = smoothstep(
      (normalizedY - left.y) / Math.max(1e-6, right.y - left.y),
    )
    return mix(left.weight, right.weight, progress)
  }
  return 0
}

export function buildChestWeightField(
  source: ChestRigSource | null | undefined,
): ChestWeightField | null {
  if (!source) return null
  const chestJoint = source.bones.findIndex((bone) => bone.id === CHEST_BONE_ID)
  const topwear = source.parts.find((part) => part.id === TOPWEAR_PART_ID)
  if (chestJoint < 0 || !topwear || topwear.vertices.length === 0) return null

  const xs = uniqueCoordinates(
    topwear.vertices.map((vertex) => vertex.position.x),
  )
  const ys = uniqueCoordinates(
    topwear.vertices.map((vertex) => vertex.position.y),
  )
  if (xs.length * ys.length !== topwear.vertices.length) return null

  const weights = new Float32Array(xs.length * ys.length)
  const populated = new Uint8Array(weights.length)
  for (const vertex of topwear.vertices) {
    const column = coordinateIndex(xs, vertex.position.x)
    const row = coordinateIndex(ys, vertex.position.y)
    if (column < 0 || row < 0) return null
    const index = row * xs.length + column
    let chestWeight = 0
    for (let influence = 0; influence < vertex.joints.length; influence += 1) {
      if (vertex.joints[influence] === chestJoint) {
        chestWeight += vertex.weights[influence] ?? 0
      }
    }
    weights[index] = clamp(chestWeight, 0, 1)
    populated[index] = 1
  }
  if (populated.includes(0)) return null
  return {
    xs: Float32Array.from(xs),
    ys: Float32Array.from(ys),
    weights,
  }
}

export function sampleChestWeight(
  field: ChestWeightField,
  x: number,
  y: number,
): number {
  const xSample = interpolationSample(field.xs, x)
  const ySample = interpolationSample(field.ys, y)
  const columns = field.xs.length
  const topLeft = field.weights[ySample.lower * columns + xSample.lower]
  const topRight = field.weights[ySample.lower * columns + xSample.upper]
  const bottomLeft = field.weights[ySample.upper * columns + xSample.lower]
  const bottomRight = field.weights[ySample.upper * columns + xSample.upper]
  const top = mix(topLeft, topRight, xSample.mix)
  const bottom = mix(bottomLeft, bottomRight, xSample.mix)
  return clamp(mix(top, bottom, ySample.mix), 0, 1)
}

function uniqueCoordinates(values: number[]): number[] {
  const sorted = values
    .filter(Number.isFinite)
    .toSorted((left, right) => left - right)
  const unique: number[] = []
  for (const value of sorted) {
    if (
      unique.length === 0 ||
      Math.abs(value - unique.at(-1)!) > COORDINATE_EPSILON
    ) {
      unique.push(value)
    }
  }
  return unique
}

function coordinateIndex(coordinates: number[], value: number): number {
  return coordinates.findIndex(
    (coordinate) => Math.abs(coordinate - value) <= COORDINATE_EPSILON,
  )
}

function interpolationSample(
  coordinates: Float32Array,
  value: number,
): { lower: number; upper: number; mix: number } {
  if (coordinates.length <= 1 || value <= coordinates[0]) {
    return { lower: 0, upper: 0, mix: 0 }
  }
  const last = coordinates.length - 1
  if (value >= coordinates[last]) return { lower: last, upper: last, mix: 0 }
  for (let upper = 1; upper < coordinates.length; upper += 1) {
    if (value > coordinates[upper]) continue
    const lower = upper - 1
    const span = Math.max(
      COORDINATE_EPSILON,
      coordinates[upper] - coordinates[lower],
    )
    return {
      lower,
      upper,
      mix: clamp((value - coordinates[lower]) / span, 0, 1),
    }
  }
  return { lower: last, upper: last, mix: 0 }
}

export function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount
}

export function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
