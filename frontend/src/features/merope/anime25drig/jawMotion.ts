import type { Anime25DPlayback } from './types'

export interface JawMotionInput {
  mouthOpen: number
  mouthWide: number
  mouthRound: number
  mouthNarrow: number
  mouthSeal: number
}

export interface JawMotionState {
  value: number
  velocity: number
}

const OPEN_FREQUENCY_HZ = 7.4
const RELEASE_FREQUENCY_HZ = 5.4
const OPEN_DAMPING_RATIO = 0.86
const RELEASE_DAMPING_RATIO = 0.72
const OPEN_SPRING = springCoefficients(OPEN_FREQUENCY_HZ, OPEN_DAMPING_RATIO)
const RELEASE_SPRING = springCoefficients(
  RELEASE_FREQUENCY_HZ,
  RELEASE_DAMPING_RATIO,
)

export function createJawMotionState(): JawMotionState {
  return { value: 0, velocity: 0 }
}

/**
 * Separates mandible travel from lip articulation. Wide and narrow visemes
 * rely more on lip muscle motion; open and round visemes retain more jaw use.
 * A lip seal only slightly reduces the target so a short bilabial does not
 * force the mandible to snap shut between neighbouring vowels.
 */
export function jawMotionTarget(
  input: Readonly<JawMotionInput>,
  emphasis: number,
): number {
  const wide = clamp01(input.mouthWide)
  const round = clamp01(input.mouthRound)
  const narrow = clamp01(input.mouthNarrow)
  const shapeTotal = wide + round + narrow
  const shapeScale = shapeTotal > 1 ? 1 / shapeTotal : 1
  const normalizedWide = wide * shapeScale
  const normalizedRound = round * shapeScale
  const normalizedNarrow = narrow * shapeScale
  const ordinary = Math.max(
    0,
    1 - normalizedWide - normalizedRound - normalizedNarrow,
  )
  const shapeContribution =
    ordinary +
    normalizedWide * 0.76 +
    normalizedRound * 0.92 +
    normalizedNarrow * 0.58
  const sealRetention = 1 - smootherstep(clamp01(input.mouthSeal)) * 0.08
  const emphasisScale = 1 + clamp01(emphasis) * 0.1
  return clamp01(
    clamp01(input.mouthOpen) *
      shapeContribution *
      sealRetention *
      emphasisScale,
  )
}

/** Exact under-damped spring step; stable across the player's bounded dt. */
export function stepJawMotion(
  state: JawMotionState,
  target: number,
  deltaSeconds: number,
): void {
  const dt = Number.isFinite(deltaSeconds)
    ? Math.max(0, Math.min(0.05, deltaSeconds))
    : 0
  if (dt === 0) return
  const boundedTarget = clamp01(target)
  const opening = boundedTarget > state.value
  const spring = opening ? OPEN_SPRING : RELEASE_SPRING
  const displacement = state.value - boundedTarget
  const exponential = Math.exp(-spring.decay * dt)
  const cosine = Math.cos(spring.dampedOmega * dt)
  const sine = Math.sin(spring.dampedOmega * dt)
  const nextDisplacement =
    exponential *
    (displacement * cosine +
      ((state.velocity + spring.decay * displacement) / spring.dampedOmega) *
        sine)
  const nextVelocity =
    exponential *
    (state.velocity * cosine -
      ((spring.decay * state.velocity + spring.omegaSquared * displacement) /
        spring.dampedOmega) *
        sine)
  state.value = clamp(boundedTarget + nextDisplacement, -0.045, 1.04)
  state.velocity = nextVelocity
  if (
    Math.abs(state.value - boundedTarget) < 1e-5 &&
    Math.abs(state.velocity) < 1e-4
  ) {
    state.value = boundedTarget
    state.velocity = 0
  }
}

function springCoefficients(
  frequency: number,
  damping: number,
): { decay: number; dampedOmega: number; omegaSquared: number } {
  const omega = Math.PI * 2 * frequency
  return {
    decay: damping * omega,
    dampedOmega: omega * Math.sqrt(Math.max(1e-5, 1 - damping ** 2)),
    omegaSquared: omega ** 2,
  }
}

/** Character-scale travel inferred once from the imported mouth silhouette. */
export function jawTravelPixels(playback: Readonly<Anime25DPlayback>): number {
  const faceHeight = Math.max(
    1,
    playback.anchors.face.y1 - playback.anchors.face.y0,
  )
  const open = playback.mouthProfile.silhouettes.find(
    (silhouette) => silhouette.material === 'mouthOpen',
  )
  const round = playback.mouthProfile.silhouettes.find(
    (silhouette) => silhouette.material === 'mouthRound',
  )
  const visibleHeight = Math.max(open?.height ?? 0, (round?.height ?? 0) * 0.88)
  return clamp(visibleHeight * 0.08, faceHeight * 0.011, faceHeight * 0.019)
}

function smootherstep(value: number): number {
  const bounded = clamp01(value)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0
  return clamp(value, 0, 1)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
