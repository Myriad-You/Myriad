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
  amplitudeScale: number
  stiffnessScale: number
  dampingScale: number
}

export interface HairSpringState {
  x: number
  v: number
  dx: number
}

export interface Anime25DHairSpringBinding {
  stiff: HairSpringState
  soft: HairSpringState
  phase: number
  stiffnessScale: number
  dampingScale: number
}

export interface Anime25DHairSpringLayer {
  springs: readonly Anime25DHairSpringBinding[] | null
}

export interface Anime25DHairSpringFrame {
  enabled: boolean
  idle: boolean
  angleX: number
  angleZ: number
  faceScale: number
  neckPivotY: number
  faceCenterY: number
  time: number
}

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
    const acceleration = -stiffness * (spring.x - target) - damping * spring.v
    spring.v += acceleration * dt
    spring.x += spring.v * dt
  }
  spring.dx = -(spring.x - target) * pull
}

export function stepAnime25DHairLayerSprings(
  layers: readonly Anime25DHairSpringLayer[],
  frame: Readonly<Anime25DHairSpringFrame>,
  elapsedSeconds: number,
): void {
  if (!frame.enabled) return
  const headOffsetX =
    (frame.angleX * 14 +
      frame.angleZ * 0.07 * (frame.neckPivotY - frame.faceCenterY)) *
    frame.faceScale
  const windAmplitude = frame.idle ? 1 : 0
  for (const layer of layers) {
    if (!layer.springs) continue
    for (const spring of layer.springs) {
      const wind =
        windAmplitude *
        (1.8 * Math.sin(frame.time * 0.8 + spring.phase) +
          Math.sin(frame.time * 1.9 + spring.phase * 2.3))
      const target = headOffsetX + wind * frame.faceScale
      stepHairSpring(
        spring.stiff,
        target,
        70 * spring.stiffnessScale,
        9 * spring.dampingScale,
        2.2,
        elapsedSeconds,
      )
      stepHairSpring(
        spring.soft,
        target,
        16 * spring.stiffnessScale,
        1.3 * spring.dampingScale,
        3,
        elapsedSeconds,
      )
    }
  }
}

/** Reduce only the upper excess-depth parallax of a composite front-hair layer. */
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
