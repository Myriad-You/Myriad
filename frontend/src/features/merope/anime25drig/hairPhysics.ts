const COMPOSITE_TAIL_START = 0.2
const COMPOSITE_TAIL_END = 0.45
const FRONT_HAIR_UPPER_PARALLAX_FLOOR = 0.2
const FRONT_HAIR_UPPER_RELEASE_START = 0.45
const FRONT_HAIR_UPPER_RELEASE_END = 0.75
const MIN_STRAND_LENGTH_RATIO = 0.5
const MAX_STRAND_LENGTH_RATIO = 2.5
const LENGTH_RESPONSE_EXPONENT = -0.25
const MAX_HAIR_SPRING_STEP_SECONDS = 1 / 120
const MAX_HAIR_SPRING_ELAPSED_SECONDS = 0.05

interface LayerVerticalBounds {
  y: number
  h: number
}

interface FaceVerticalBounds {
  y0: number
  y1: number
}

export interface HairStrandDynamics {
  /** Displacement gain relative to a strand one face-height long. */
  amplitudeScale: number
  /** Spring stiffness multiplier; paired with dampingScale below. */
  stiffnessScale: number
  /** Preserves the authored damping ratio as response speed changes. */
  dampingScale: number
}

export interface HairSpringState {
  x: number
  v: number
  dx: number
}

/**
 * Derive restrained per-strand motion from its projected pixel length.
 * Displacement follows length approximately linearly, while response speed
 * changes much more gently than a literal physical cantilever would.
 */
export function hairStrandDynamics(
  rootY: number,
  tipY: number,
  referenceHeight: number,
): HairStrandDynamics {
  const strandLength = tipY - rootY
  if (
    !Number.isFinite(strandLength) ||
    !Number.isFinite(referenceHeight) ||
    strandLength <= 0 ||
    referenceHeight <= 0
  ) {
    return {
      amplitudeScale: 1,
      stiffnessScale: 1,
      dampingScale: 1,
    }
  }

  const lengthRatio = clamp(
    strandLength / referenceHeight,
    MIN_STRAND_LENGTH_RATIO,
    MAX_STRAND_LENGTH_RATIO,
  )
  const frequencyScale = lengthRatio ** LENGTH_RESPONSE_EXPONENT
  return {
    amplitudeScale: lengthRatio,
    stiffnessScale: frequencyScale ** 2,
    dampingScale: frequencyScale,
  }
}

/** Semi-implicit spring integration with bounded substeps for stable playback. */
export function stepHairSpring(
  spring: HairSpringState,
  target: number,
  stiffness: number,
  damping: number,
  pull: number,
  elapsedSeconds: number,
): void {
  if (!Number.isFinite(elapsedSeconds) || elapsedSeconds <= 0) return
  const boundedElapsed = Math.min(
    elapsedSeconds,
    MAX_HAIR_SPRING_ELAPSED_SECONDS,
  )
  const steps = Math.max(
    1,
    Math.ceil(boundedElapsed / MAX_HAIR_SPRING_STEP_SECONDS),
  )
  const dt = boundedElapsed / steps
  for (let step = 0; step < steps; step += 1) {
    const acceleration =
      -stiffness * (spring.x - target) - damping * spring.v
    spring.v += acceleration * dt
    spring.x += spring.v * dt
  }
  spring.dx = -(spring.x - target) * pull
}

/**
 * Reduce only the upper excess-depth parallax of a composite front-hair layer.
 * The face-plane head motion remains intact; the retained depth motion then
 * smoothly returns to the authored amount above the long-lock tips.
 */
export function frontHairUpperParallaxScale(
  vertexY: number,
  layer: LayerVerticalBounds,
  face: FaceVerticalBounds,
): number {
  const layerProgress = clamp((vertexY - layer.y) / Math.max(1, layer.h), 0, 1)
  const lowerRelease = smoothstep(
    (layerProgress - FRONT_HAIR_UPPER_RELEASE_START) /
      (FRONT_HAIR_UPPER_RELEASE_END - FRONT_HAIR_UPPER_RELEASE_START),
  )
  const compositeScale =
    FRONT_HAIR_UPPER_PARALLAX_FLOOR +
    (1 - FRONT_HAIR_UPPER_PARALLAX_FLOOR) * lowerRelease
  const compositeMix = compositeHairMix(layer, face)
  return 1 - compositeMix * (1 - compositeScale)
}

function compositeHairMix(
  layer: LayerVerticalBounds,
  face: FaceVerticalBounds,
): number {
  const faceHeight = Math.max(1, face.y1 - face.y0)
  const tailRatio = Math.max(0, layer.y + layer.h - face.y1) / faceHeight
  return smoothstep(
    (tailRatio - COMPOSITE_TAIL_START) /
      (COMPOSITE_TAIL_END - COMPOSITE_TAIL_START),
  )
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
