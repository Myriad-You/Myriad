import type { ChestSpatialField } from './chestField'
import type { Anime25DChestProfile } from './types'
import { clamp, mix, resolveChestSpatialField, smoothstep } from './chestField'
import { HEAD_ROLL_RADIANS } from './poseScale'

export { buildChestWeightField, chestDeformationWeight, chestProfileUsesGeometryWeights, type ChestSpatialField, type ChestWeightField, deriveGeometryChestProfile, resolveChestSpatialField, sampleChestVerticalWeight, sampleChestWeight } from './chestField'

const MAX_SPRING_STEP_SECONDS = 1 / 120
const HORIZONTAL_SPRING = { stiffness: 68, damping: 7.4 } as const
const VERTICAL_SPRING = { stiffness: 82, damping: 8 } as const
const BODY_LAYER_INFLUENCE = 0.16
const MIN_AI_MOTION_SCALE = 0.22
const MAX_AI_MOTION_SCALE = 1.14
const AI_MOTION_RAMP_START = 0.1
const AI_MOTION_RAMP_END = 0.85
const REFERENCE_BUST_CONTROL = 2.5
const CHEST_OUTPUT_GAIN = 2
const MAX_RESPONSE_MIX = 0.94
const MAX_FOLLOW_MIX = 0.58
const MIN_SIZE_TRANSMISSION = 0.04
const SIZE_BOOST_START = 0.5
const SIZE_BOOST_END = 0.75
const MAX_SIZE_TRANSMISSION_BOOST = 0.75
const MIN_GARMENT_TRANSMISSION = 0.35
const MIN_RESPONSE_GARMENT_TRANSMISSION = 0.55
const CHEST_BREATH_TRAVEL = 1.2
const CHEST_BODY_EXCITATION_TRAVEL = 20
// A vision pass reads small chests anywhere up to about 0.55 and a medium-large
// one anywhere from 0.55 to 0.72, so a mid estimate bounced a flat chest like an
// average one. The boost now needs a clearly large estimate.
const DYNAMIC_BOOST_START = 0.52
const DYNAMIC_BOOST_END = 0.8
/**
 * Without a vision estimate the size is unknown, and a flat chest must not
 * bounce like an average one: assume a small-to-medium chest.
 */
const UNKNOWN_SIZE_VISIBLE_SCALE = 0.4
/** The chest's own shape change starts only once there is volume to change. */
const VOLUME_SIZE_START = 0.55
const VOLUME_SIZE_END = 0.8
const MAX_INERTIA_GAIN = 5.5
const MAX_DAMPING_REDUCTION = 0.58

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

export interface ChestDynamicsTuning {
  followScale: number
  responseScale: number
  frequencyScale: number
  dampingScale: number
  /** Size-conditioned gain applied only to the spring's relative offset. */
  inertiaGain: number
  /** Share of whole-body motion injected into the spring, never direct travel. */
  bodyExcitationScale: number
  /** How much the chest's shape follows its bounce; nothing for a flat chest. */
  volumeScale: number
  breathMotionScale: number
  breathVolumeScale: number
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
      volumeScale: 0,
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
  const visibleScale = resolveChestVisibleScale(profile)
  const sizeTransmission = resolveChestSizeTransmission(profile)
  const dynamicBoost = resolveChestDynamicBoost(visibleScale)
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
    volumeScale: smoothstep(clamp((visibleScale - VOLUME_SIZE_START) / (VOLUME_SIZE_END - VOLUME_SIZE_START), 0, 1)),
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

/** The apparent size to tune motion by: the estimate, or a cautious guess without one. */
function resolveChestVisibleScale(profile: Pick<ChestDynamicsProfile, 'source' | 'visibleScale'>): number {
  const visibleScale = clamp(profile.visibleScale, 0, 1)
  return profile.source === 'ai-vision' ? visibleScale : Math.min(visibleScale, UNKNOWN_SIZE_VISIBLE_SCALE)
}

function resolveChestSizeTransmission(profile: ChestDynamicsProfile): number {
  const visibleScale = resolveChestVisibleScale(profile)
  const progress = clamp(
    (visibleScale - AI_MOTION_RAMP_START) /
      (AI_MOTION_RAMP_END - AI_MOTION_RAMP_START),
    0,
    1,
  )
  const eased = progress * progress * (3 - 2 * progress)
  const baseTransmission =
    MIN_SIZE_TRANSMISSION + (1 - MIN_SIZE_TRANSMISSION) * eased
  const boostProgress = clamp(
    (visibleScale - SIZE_BOOST_START) /
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
  const az = driver.angleZ * HEAD_ROLL_RADIANS
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
