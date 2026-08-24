import type { Anime25DChestProfile } from './types'

const CHEST_BONE_ID = 'a25d-chest'
const TOPWEAR_PART_ID = 'a25d-topwear'
const COORDINATE_EPSILON = 1e-5
const MAX_SPRING_STEP_SECONDS = 1 / 120
const HORIZONTAL_SPRING = { stiffness: 68, damping: 7.4 } as const
const VERTICAL_SPRING = { stiffness: 82, damping: 8 } as const
const MIN_AI_MOTION_SCALE = 0.22
const MAX_AI_MOTION_SCALE = 1.14
const AI_MOTION_RAMP_START = 0.1
const AI_MOTION_RAMP_END = 0.85

interface ChestMotionDriver {
  angleX: number
  angleY: number
  angleZ: number
  body: number
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

type ChestWeightProfile = Pick<Anime25DChestProfile, 'enabled' | 'source'>
type ChestMotionProfile = Pick<
  Anime25DChestProfile,
  'enabled' | 'source' | 'visibleScale' | 'motionScale'
>

/**
 * AI vision already authors the complete two-dimensional deformation region.
 * Intersecting it with the older vertical bone field can erase the AI-selected
 * centre. Geometry fallback still needs that field to retain legacy behavior.
 */
export function chestProfileUsesGeometryWeights(
  profile: ChestWeightProfile | null | undefined,
): boolean {
  return profile?.enabled !== false && profile?.source !== 'ai-vision'
}

/**
 * Bound AI-authored displacement by apparent soft-tissue size. The eased ramp
 * keeps small profiles restrained without introducing a hard size threshold;
 * `min` also makes this backward-compatible with already-persisted profiles.
 */
export function resolveChestMotionScale(
  profile: ChestMotionProfile | null | undefined,
): number {
  if (profile?.enabled === false) return 0
  const authoredScale = clamp(profile?.motionScale ?? 1, 0, 1.25)
  if (profile?.source !== 'ai-vision') return authoredScale
  const visibleScale = clamp(profile.visibleScale, 0, 1)
  const progress = clamp(
    (visibleScale - AI_MOTION_RAMP_START) /
      (AI_MOTION_RAMP_END - AI_MOTION_RAMP_START),
    0,
    1,
  )
  const eased = progress * progress * (3 - 2 * progress)
  const sizeLimit =
    MIN_AI_MOTION_SCALE +
    (MAX_AI_MOTION_SCALE - MIN_AI_MOTION_SCALE) * eased
  return Math.min(authoredScale, sizeLimit)
}

export interface ChestSpringState {
  x: number
  y: number
  vx: number
  vy: number
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
    offsetX: 0,
    offsetY: 0,
    initialized: false,
  }
}

/** Convert the resolved pose into the moving base followed by chest tissue. */
export function chestMotionTarget(
  driver: ChestMotionDriver,
  faceScale: number,
  target: ChestMotionTarget = { x: 0, y: 0 },
): ChestMotionTarget {
  const scale = Math.max(0.01, faceScale)
  target.x =
    (driver.angleX * 5.5 - driver.angleZ * 4.5 + driver.body * 7) * scale
  target.y = -driver.angleY * 4.5 * scale
  return target
}

/**
 * Extract the importer-authored `a25d-chest` skinning influence from the
 * compiled regular topwear grid. The field stays in Rig IR coordinates;
 * playback vertices sample it after converting pixels by the frame width.
 */
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

/**
 * Track the resolved pose with an under-damped two-axis mass. The returned
 * relative offset is zero at a held pose, so the chest reacts to movement
 * without becoming permanently bound to the pointer position.
 */
export function stepChestSpring(
  state: ChestSpringState,
  targetX: number,
  targetY: number,
  deltaSeconds: number,
  frequencyScale = 1,
): void {
  if (!state.initialized) {
    state.x = targetX
    state.y = targetY
    state.initialized = true
  }
  const dt = clamp(deltaSeconds, 0.001, 0.05)
  const frequency = clamp(frequencyScale, 0.75, 1.25)
  const stiffnessScale = frequency * frequency
  const steps = Math.ceil(dt / MAX_SPRING_STEP_SECONDS)
  const stepSeconds = dt / steps
  for (let step = 0; step < steps; step += 1) {
    const accelX =
      -HORIZONTAL_SPRING.stiffness * stiffnessScale * (state.x - targetX) -
      HORIZONTAL_SPRING.damping * frequency * state.vx
    const accelY =
      -VERTICAL_SPRING.stiffness * stiffnessScale * (state.y - targetY) -
      VERTICAL_SPRING.damping * frequency * state.vy
    state.vx += accelX * stepSeconds
    state.vy += accelY * stepSeconds
    state.x += state.vx * stepSeconds
    state.y += state.vy * stepSeconds
  }
  state.offsetX = state.x - targetX
  state.offsetY = state.y - targetY
}

function uniqueCoordinates(values: number[]): number[] {
  const sorted = values
    .filter(Number.isFinite)
    .sort((left, right) => left - right)
  const unique: number[] = []
  for (const value of sorted) {
    if (
      unique.length === 0 ||
      Math.abs(value - unique[unique.length - 1]) > COORDINATE_EPSILON
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

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
