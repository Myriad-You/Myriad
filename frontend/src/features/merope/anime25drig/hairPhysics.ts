import type { HairChain, HairChainTuning } from './hairChain'
import { stepHairChain } from './hairChain'

const COMPOSITE_TAIL_START = 0.2
const COMPOSITE_TAIL_END = 0.45
const FRONT_HAIR_UPPER_PARALLAX_FLOOR = 0.2
const FRONT_HAIR_UPPER_RELEASE_START = 0.45
const FRONT_HAIR_UPPER_RELEASE_END = 0.75
const MIN_STRAND_LENGTH_RATIO = 0.5
const MAX_STRAND_LENGTH_RATIO = 2.5
const LENGTH_RESPONSE_EXPONENT = -0.25

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

export interface Anime25DHairSpringBinding {
  /** Displacement of the strand's root this frame, from its primary deformation. */
  supportX: number
  supportY: number
  /** The strand's root at rest, in canvas pixels. */
  rootX: number
  rootY: number
  chain: HairChain
  phase: number
  /** How much the strand's length amplified the old single-spring lag; its weights still carry it. */
  amplitudeScale: number
  stiffnessScale: number
  dampingScale: number
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
}

/** Links per lock: enough for the bend to travel down it. */
export const HAIR_CHAIN_LINKS = 5

/**
 * Bangs are short and hold their shape: they swing back about a fifth of how
 * far the head went and are still within 0.8 s. Long rear hair is heavier
 * and slower, swings back about a third and drifts more in idle air.
 */
const FRONT_HAIR_CHAIN: Readonly<HairChainTuning> = { omega: 24, damping: 1.1, carry: 0.8, tipStiffness: 0.8, drag: 10 }
const REAR_HAIR_CHAIN: Readonly<HairChainTuning> = { omega: 12, damping: 0.9, carry: 0.55, tipStiffness: 0.6, drag: 3 }
/**
 * Idle air holds a lock aside by the same few pixels however stiff it is, in
 * face-scale pixels per unit of wind: about ±4 at a bang's tip, ±8 at the
 * rear hair's.
 */
const FRONT_WIND_SWAY = 0.2
const REAR_WIND_SWAY = 0.4

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

const chainTuning: HairChainTuning = { omega: 0, damping: 0, carry: 0, tipStiffness: 1, drag: 0 }

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
      chainTuning.omega = base.omega * Math.sqrt(spring.stiffnessScale)
      chainTuning.damping = base.damping
      chainTuning.carry = base.carry
      chainTuning.drag = base.drag
      chainTuning.tipStiffness = base.tipStiffness / (1 + 0.3 * soft)
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
