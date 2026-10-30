import type { HairChain, HairChainTuning } from './hairChain'
import { stepHairChain } from './hairChain'

const COMPOSITE_TAIL_START = 0.2
const COMPOSITE_TAIL_END = 0.45
const FRONT_HAIR_UPPER_PARALLAX_FLOOR = 0.2
const FRONT_HAIR_UPPER_RELEASE_START = 0.45
const FRONT_HAIR_UPPER_RELEASE_END = 0.75
const MIN_STRAND_LENGTH_RATIO = 0.5
const MAX_STRAND_LENGTH_RATIO = 2.5
// A lock swings as a pendulum: its frequency goes as one over the root of its length.
const LENGTH_RESPONSE_EXPONENT = -0.5
/** Locks of one front hair are not tuned alike: each is off by up to this share, by its number. */
const LOCK_DETUNE = 0.12
/** A tuft standing above the head (an ahoge) is light and springy: quicker, and it bounces. */
const TUFT_FREQUENCY = 1.5
const TUFT_DAMPING = 0.8

interface LayerVerticalBounds {
  y: number
  h: number
}

interface FaceVerticalBounds {
  y0: number
  y1: number
}

export interface HairStrandDynamics {
  /** The lock's length over the face's height (1 when it cannot be read). */
  lengthRatio: number
  amplitudeScale: number
  stiffnessScale: number
  dampingScale: number
}

export interface Anime25DHairSpringBinding {
  /** Displacement of the strand's root this frame, from its primary deformation. */
  supportX: number
  supportY: number
  /** The strand's root at rest, in canvas pixels. */
  rootX: number
  rootY: number
  chain: HairChain
  phase: number
  /** The lock's length over the face's height. */
  lengthRatio: number
  /** How much the strand's length amplified the old single-spring lag; its weights still carry it. */
  amplitudeScale: number
  stiffnessScale: number
  dampingScale: number
  /** The lock's own tuning, beyond its length: a share of the frequency and of the damping ratio. */
  frequencyScale?: number
  dampingRatioScale?: number
}

export interface Anime25DHairSpringLayer {
  frontHair: boolean
  springs: readonly Anime25DHairSpringBinding[] | null
}

export interface Anime25DHairSpringFrame {
  enabled: boolean
  idle: boolean
  faceScale: number
  time: number
  /** Softness drivers: a softer lock bends more toward its tip. */
  frontSoft: number
  rearSoft: number
  /** The head's tilt in the picture, radians: its roll on the body's lean. */
  tilt: number
}

/** Links per lock: enough for the bend to travel down it. */
export const HAIR_CHAIN_LINKS = 5

/**
 * Bangs are short and hold their shape: they swing back about a fifth of how
 * far the head went and are still within 0.8 s. Long rear hair is heavier
 * and slower, swings back about a third and drifts more in idle air.
 */
const FRONT_HAIR_CHAIN: Readonly<HairChainTuning> = { omega: 24, damping: 0.7, carry: 0.8, tipStiffness: 0.8, drag: 10, limit: 0.52 }
const REAR_HAIR_CHAIN: Readonly<HairChainTuning> = { omega: 12, damping: 0.9, carry: 0.55, tipStiffness: 0.6, drag: 3, limit: 0.79 }
/**
 * Idle air holds a lock aside by the same few pixels however stiff it is, in
 * face-scale pixels per unit of wind: about ±4 at a bang's tip, ±8 at the
 * rear hair's.
 */
const FRONT_WIND_SWAY = 0.2
const REAR_WIND_SWAY = 0.4

/**
 * How much of a tilt a lock hangs back from, toward the vertical. Bangs are
 * short and held at the hairline, so they go round with the head; a lock
 * longer than the face hangs free like a rigger's droop-by-AngleZ sway.
 */
const FRONT_HAIR_HANG = 0.5
const REAR_HAIR_HANG = 0.7
const HANG_SHORT = 0.5
const HANG_LONG = 1.3

export function hairHangShare(lengthRatio: number, frontHair: boolean): number {
  const t = clamp((lengthRatio - HANG_SHORT) / (HANG_LONG - HANG_SHORT), 0, 1)
  return (frontHair ? FRONT_HAIR_HANG : REAR_HAIR_HANG) * t * t * (3 - 2 * t)
}

/**
 * How one lock of front hair is tuned apart from its length. Locks cut from
 * one front hair (front-hair-N) are each detuned by their number, spread by
 * the golden ratio so neighbours differ, and do not swing in step; a tuft
 * standing wholly above the face is quicker and bouncier.
 */
export function hairLockCharacter(
  layer: { name: string; y: number; h: number },
  face: FaceVerticalBounds,
): { frequencyScale: number; dampingRatioScale: number } {
  const lock = /^front-hair-(\d+)$/.exec(layer.name)
  const detune = lock ? 1 + LOCK_DETUNE * (2 * ((Number(lock[1]) * 0.6180339887) % 1) - 1) : 1
  const tuft = layer.y + layer.h < face.y0 + (face.y1 - face.y0) * 0.1
  return {
    frequencyScale: detune * (tuft ? TUFT_FREQUENCY : 1),
    dampingRatioScale: tuft ? TUFT_DAMPING : 1,
  }
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
      lengthRatio: 1,
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
    lengthRatio,
    amplitudeScale: lengthRatio,
    stiffnessScale: frequencyScale ** 2,
    dampingScale: frequencyScale,
  }
}

const chainTuning: HairChainTuning = { omega: 0, damping: 0, carry: 0, tipStiffness: 1, drag: 0, limit: Math.PI }

export function stepAnime25DHairLayerSprings(
  layers: readonly Anime25DHairSpringLayer[],
  frame: Readonly<Anime25DHairSpringFrame>,
  elapsedSeconds: number,
): void {
  if (!frame.enabled) return
  const windAmplitude = frame.idle ? 1 : 0
  for (const layer of layers) {
    if (!layer.springs) continue
    const base = layer.frontHair ? FRONT_HAIR_CHAIN : REAR_HAIR_CHAIN
    const soft = Math.max(0, layer.frontHair ? frame.frontSoft : frame.rearSoft)
    for (const spring of layer.springs) {
      const wind =
        windAmplitude *
        (1.8 * Math.sin(frame.time * 0.8 + spring.phase) +
          Math.sin(frame.time * 1.9 + spring.phase * 2.3))
      // Longer locks swing slower, as a pendulum does.
      chainTuning.omega = base.omega * Math.sqrt(spring.stiffnessScale) * (spring.frequencyScale ?? 1)
      chainTuning.damping = base.damping * (spring.dampingRatioScale ?? 1)
      chainTuning.carry = base.carry
      chainTuning.drag = base.drag
      chainTuning.tipStiffness = base.tipStiffness / (1 + 0.3 * soft)
      chainTuning.limit = base.limit
      chainTuning.hang = -frame.tilt * hairHangShare(spring.lengthRatio, layer.frontHair)
      stepHairChain(
        spring.chain,
        spring.rootX + spring.supportX,
        spring.rootY + spring.supportY,
        chainTuning,
        wind * (layer.frontHair ? FRONT_WIND_SWAY : REAR_WIND_SWAY) * frame.faceScale * chainTuning.omega * chainTuning.omega,
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
