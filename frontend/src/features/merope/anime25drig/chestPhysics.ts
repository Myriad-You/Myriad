import type { Anime25DChestProfile, Anime25DPlayback } from './types'

const CHEST_BONE_ID = 'a25d-chest'
const TOPWEAR_PART_ID = 'a25d-topwear'
const COORDINATE_EPSILON = 1e-5
const MAX_SPRING_STEP_SECONDS = 1 / 120
const HORIZONTAL_SPRING = { stiffness: 68, damping: 7.4 } as const
const VERTICAL_SPRING = { stiffness: 82, damping: 8 } as const
const BODY_LAYER_INFLUENCE = 0.16
const MIN_AI_MOTION_SCALE = 0.22
const MAX_AI_MOTION_SCALE = 1.14
const AI_MOTION_RAMP_START = 0.1
const AI_MOTION_RAMP_END = 0.85
const DEFAULT_SUPPORT_SCALE = 0.45
const DEFAULT_GARMENT_MOTION_SCALE = 0.65
const REFERENCE_BUST_CONTROL = 2.5
const CHEST_OUTPUT_GAIN = 2
const MAX_RESPONSE_MIX = 0.94
const MAX_FOLLOW_MIX = 0.58
const MIN_SIZE_TRANSMISSION = 0.04
const SIZE_BOOST_START = 0.38
const SIZE_BOOST_END = 0.65
const MAX_SIZE_TRANSMISSION_BOOST = 0.75
const MIN_GARMENT_TRANSMISSION = 0.35
const MIN_RESPONSE_GARMENT_TRANSMISSION = 0.55
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
const CHEST_BREATH_TRAVEL = 1.2
const CHEST_BODY_EXCITATION_TRAVEL = 20
const DYNAMIC_BOOST_START = 0.3
const DYNAMIC_BOOST_END = 0.6
const MAX_INERTIA_GAIN = 5.5
const MAX_DAMPING_REDUCTION = 0.58

const CHEST_VERTICAL_CURVE = [
  { y: -1.2, weight: 0 },
  { y: -0.55, weight: 0.62 },
  { y: 0, weight: 1 },
  { y: 0.62, weight: 0.68 },
  { y: 1.35, weight: 0 },
] as const

interface ChestMotionDriver {
  angleX: number
  angleY: number
  angleZ: number
  body: number
}

export interface ChestMotionGeometry {
  faceScale: number
  faceCenterY: number
  neckX: number
  neckY: number
  centerX: number
  centerY: number
  depth: number
}

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

export interface ChestMotionTarget {
  x: number
  y: number
}

export interface ChestDeformationRegion {
  centerX: number
  centerY: number
  radiusX: number
  radiusY: number
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
type ChestRegionProfile = Pick<
  Anime25DChestProfile,
  'centerX' | 'centerY' | 'radiusX' | 'radiusY'
>
type ChestMotionProfile = Pick<
  Anime25DChestProfile,
  'enabled' | 'source' | 'visibleScale' | 'motionScale'
>

type ChestDynamicsProfile = Pick<
  Anime25DChestProfile,
  | 'enabled'
  | 'source'
  | 'visibleScale'
  | 'motionScale'
  | 'frequencyScale'
  | 'supportScale'
  | 'garmentMotionScale'
>

type ChestSpatialProfile = Pick<
  Anime25DChestProfile,
  'source' | 'supportScale' | 'garmentMotionScale'
>

export interface ChestDynamicsTuning {
  followScale: number
  responseScale: number
  frequencyScale: number
  dampingScale: number
  /** Size-conditioned gain applied only to the spring's relative offset. */
  inertiaGain: number
  /** Share of whole-body motion injected into the spring, never direct travel. */
  bodyExcitationScale: number
  breathMotionScale: number
  breathVolumeScale: number
}

export function chestProfileUsesGeometryWeights(
  profile: ChestWeightProfile,
): boolean {
  return profile.enabled && profile.source !== 'ai-vision'
}

export function resolveChestDeformationRegion(
  profile: ChestRegionProfile,
): ChestDeformationRegion {
  return {
    centerX: profile.centerX,
    centerY: profile.centerY,
    radiusX: profile.radiusX,
    radiusY: profile.radiusY,
  }
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

export function chestBreathResidual(timeSeconds: number): number {
  const time = Number.isFinite(timeSeconds) ? timeSeconds : 0
  return 0.5 * Math.sin((time * Math.PI * 2) / 3.4)
}

export function chestBreathTargetY(
  breathResidual: number,
  faceScale: number,
  motionScale: number,
): number {
  if (breathResidual === 0 || motionScale <= 0) return 0
  return (
    -clamp(breathResidual, -0.5, 0.5) *
    CHEST_BREATH_TRAVEL *
    Math.max(0.01, faceScale) *
    clamp(motionScale, 0, 1)
  )
}

export function resolveChestMotionScale(profile: ChestMotionProfile): number {
  if (!profile.enabled) return 0
  const authoredScale = clamp(profile.motionScale, 0, 1.25)
  if (profile.source !== 'ai-vision') return authoredScale
  const visibleScale = clamp(profile.visibleScale, 0, 1)
  const progress = clamp(
    (visibleScale - AI_MOTION_RAMP_START) /
      (AI_MOTION_RAMP_END - AI_MOTION_RAMP_START),
    0,
    1,
  )
  const eased = progress * progress * (3 - 2 * progress)
  const sizeLimit =
    MIN_AI_MOTION_SCALE + (MAX_AI_MOTION_SCALE - MIN_AI_MOTION_SCALE) * eased
  return Math.min(authoredScale, sizeLimit)
}

export function resolveChestDynamics(
  profile: ChestDynamicsProfile,
  field: Readonly<ChestSpatialField> = resolveChestSpatialField(profile),
): ChestDynamicsTuning {
  if (!profile.enabled) {
    return {
      followScale: 0,
      responseScale: 0,
      frequencyScale: 1,
      dampingScale: 1,
      inertiaGain: 1,
      bodyExcitationScale: 0,
      breathMotionScale: 0,
      breathVolumeScale: 0,
    }
  }
  const support = clamp(profile.supportScale, 0, 1)
  const garmentMotion = clamp(profile.garmentMotionScale, 0, 1)
  // Visual estimates near the restrained end must not erase the authored motion.
  const garmentTransmission =
    MIN_GARMENT_TRANSMISSION +
    (1 - MIN_GARMENT_TRANSMISSION) * garmentMotion ** 0.25
  const responseGarmentTransmission =
    MIN_RESPONSE_GARMENT_TRANSMISSION +
    (1 - MIN_RESPONSE_GARMENT_TRANSMISSION) * garmentMotion ** 0.25
  const sizeTransmission = resolveChestSizeTransmission(profile)
  const dynamicBoost = resolveChestDynamicBoost(profile.visibleScale)
  const followAttenuation = 1 - 0.12 * support ** 1.1
  const supportAttenuation = 1 - 0.08 * support ** 1.15
  const followScale = garmentTransmission * followAttenuation * sizeTransmission
  return {
    followScale,
    responseScale: Math.min(
      followScale,
      resolveChestMotionScale(profile) *
        responseGarmentTransmission *
        supportAttenuation *
        sizeTransmission,
    ),
    frequencyScale: clamp(
      profile.frequencyScale * (0.94 + support * 0.08),
      0.7,
      1.45,
    ),
    dampingScale: clamp(
      (0.86 + support * 0.82 + (1 - garmentMotion) * 0.18) *
        (1 - MAX_DAMPING_REDUCTION * dynamicBoost),
      0.4,
      1.9,
    ),
    inertiaGain: mix(1, MAX_INERTIA_GAIN, dynamicBoost),
    bodyExcitationScale: dynamicBoost,
    breathMotionScale: field.breathMotionGain,
    breathVolumeScale: field.breathVolumeGain,
  }
}

function resolveChestDynamicBoost(visibleScale: number): number {
  return smoothstep(
    clamp(
      (visibleScale - DYNAMIC_BOOST_START) /
        (DYNAMIC_BOOST_END - DYNAMIC_BOOST_START),
      0,
      1,
    ),
  )
}

function resolveChestSizeTransmission(profile: ChestDynamicsProfile): number {
  if (profile.source !== 'ai-vision') return 1
  const progress = clamp(
    (profile.visibleScale - AI_MOTION_RAMP_START) /
      (AI_MOTION_RAMP_END - AI_MOTION_RAMP_START),
    0,
    1,
  )
  const eased = progress * progress * (3 - 2 * progress)
  const baseTransmission =
    MIN_SIZE_TRANSMISSION + (1 - MIN_SIZE_TRANSMISSION) * eased
  const boostProgress = clamp(
    (profile.visibleScale - SIZE_BOOST_START) /
      (SIZE_BOOST_END - SIZE_BOOST_START),
    0,
    1,
  )
  const boostEased = boostProgress ** 2 * (3 - 2 * boostProgress)
  return Math.min(
    1,
    baseTransmission * (1 + MAX_SIZE_TRANSMISSION_BOOST * boostEased),
  )
}

export function chestFollowMix(
  bustControl: number,
  followScale: number,
): number {
  const authoredStrength = normalizedChestStrength(bustControl)
  return clamp(followScale * authoredStrength, 0, MAX_FOLLOW_MIX)
}

export function chestResponseMix(
  bustControl: number,
  responseScale: number,
): number {
  const authoredStrength = normalizedChestStrength(bustControl)
  return clamp(responseScale * authoredStrength, 0, MAX_RESPONSE_MIX)
}

function normalizedChestStrength(bustControl: number): number {
  return clamp(
    (bustControl / REFERENCE_BUST_CONTROL) * CHEST_OUTPUT_GAIN,
    0,
    1.6,
  )
}

export interface ChestSpringState {
  x: number
  y: number
  vx: number
  vy: number
  previousTargetX: number
  previousTargetY: number
  offsetX: number
  offsetY: number
  initialized: boolean
}

export function createChestSpringState(): ChestSpringState {
  return {
    x: 0,
    y: 0,
    vx: 0,
    vy: 0,
    previousTargetX: 0,
    previousTargetY: 0,
    offsetX: 0,
    offsetY: 0,
    initialized: false,
  }
}

export function chestMotionTarget(
  driver: ChestMotionDriver,
  faceScale: number,
  target: ChestMotionTarget = { x: 0, y: 0 },
): ChestMotionTarget {
  const scale = Math.max(0.01, faceScale)
  target.x = (driver.angleX * 5.5 - driver.angleZ * 4.5) * scale
  // Subtracted rather than negated so a neutral pose resolves to +0.
  target.y = 0 - driver.angleY * 4.5 * scale
  return target
}

/** Whole-body rotation is rendered globally, so it must not be added to direct chest travel. */
export function chestBodyExcitationY(
  body: number,
  faceScale: number,
  excitationScale: number,
): number {
  return (
    body *
    CHEST_BODY_EXCITATION_TRAVEL *
    Math.max(0.01, faceScale) *
    clamp(excitationScale, 0, 1)
  )
}

export function topwearMotionAtChest(
  driver: ChestMotionDriver,
  geometry: ChestMotionGeometry,
  target: ChestMotionTarget = { x: 0, y: 0 },
): ChestMotionTarget {
  const scale = Math.max(0.01, geometry.faceScale)
  const az = driver.angleZ * 0.07
  const cosine = Math.cos(az)
  const sine = Math.sin(az)
  const relativeX = geometry.centerX - geometry.neckX
  const relativeY = geometry.centerY - geometry.neckY
  const rotatedX = relativeX * cosine - relativeY * sine
  const rotatedY = relativeX * sine + relativeY * cosine
  let x = geometry.centerX + (rotatedX - relativeX) * BODY_LAYER_INFLUENCE
  let y = geometry.centerY + (rotatedY - relativeY) * BODY_LAYER_INFLUENCE
  const depthOffset = geometry.depth - 1
  x +=
    BODY_LAYER_INFLUENCE *
    scale *
    (driver.angleX * (14 + 40 * depthOffset) +
      driver.angleX * (geometry.neckY - y) * 0.028)
  y +=
    BODY_LAYER_INFLUENCE *
    scale *
    (-driver.angleY * (9 + 30 * depthOffset) -
      driver.angleY * depthOffset * (y - geometry.faceCenterY) * 0.05)
  target.x = x - geometry.centerX
  target.y = y - geometry.centerY
  return target
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
  if (populated.some((value) => value === 0)) return null
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

export function stepChestSpring(
  state: ChestSpringState,
  targetX: number,
  targetY: number,
  deltaSeconds: number,
  frequencyScale = 1,
  dampingScale = 1,
): void {
  if (!state.initialized) {
    state.x = targetX
    state.y = targetY
    state.previousTargetX = targetX
    state.previousTargetY = targetY
    state.initialized = true
  }
  const dt = clamp(deltaSeconds, 0.001, 0.05)
  const targetVelocityX = (targetX - state.previousTargetX) / dt
  const targetVelocityY = (targetY - state.previousTargetY) / dt
  const frequency = clamp(frequencyScale, 0.7, 1.45)
  const damping = clamp(dampingScale, 0.4, 2)
  const stiffnessScale = frequency * frequency
  const steps = Math.ceil(dt / MAX_SPRING_STEP_SECONDS)
  const stepSeconds = dt / steps
  const fromX = state.previousTargetX
  const fromY = state.previousTargetY
  for (let step = 0; step < steps; step += 1) {
    // The attachment base slides across the frame instead of teleporting at the frame boundary.
    const blend = (step + 1) / steps
    const stepTargetX = fromX + (targetX - fromX) * blend
    const stepTargetY = fromY + (targetY - fromY) * blend
    const accelX =
      -HORIZONTAL_SPRING.stiffness * stiffnessScale * (state.x - stepTargetX) -
      HORIZONTAL_SPRING.damping *
        frequency *
        damping *
        (state.vx - targetVelocityX)
    const accelY =
      -VERTICAL_SPRING.stiffness * stiffnessScale * (state.y - stepTargetY) -
      VERTICAL_SPRING.damping *
        frequency *
        damping *
        (state.vy - targetVelocityY)
    state.vx += accelX * stepSeconds
    state.vy += accelY * stepSeconds
    state.x += state.vx * stepSeconds
    state.y += state.vy * stepSeconds
  }
  state.previousTargetX = targetX
  state.previousTargetY = targetY
  state.offsetX = state.x - targetX
  state.offsetY = state.y - targetY
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

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
