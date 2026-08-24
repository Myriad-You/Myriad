import type {
  PerformanceBaseline,
  PerformanceCue,
} from '../../../services/agent/types'
import type { Anime25DDriver } from './player'
import {
  DEFAULT_FRONT_HAIR_SWAY,
  DEFAULT_REAR_HAIR_SWAY,
} from './player'

/** Deterministic translation from Lite-selected semantics to bounded rig targets. */
export function baselineDriverPatch(
  baseline: PerformanceBaseline,
): Partial<Anime25DDriver> {
  const energy = clamp(baseline.motionEnergy, 0.2, 1.4)
  const swayEnergy = 0.7 + energy * 0.3
  const attention = clamp(baseline.attention, 0, 1)
  const expression: Record<PerformanceBaseline['expression'], Partial<Anime25DDriver>> = {
    withdrawn: { brow: -0.28, mouthForm: -0.24, eyeOpenL: 0.78, eyeOpenR: 0.78, irisScale: 0.92 },
    subdued: { brow: -0.12, mouthForm: -0.1, eyeOpenL: 0.88, eyeOpenR: 0.88, irisScale: 0.96 },
    steady: { brow: 0, mouthForm: 0.02, eyeOpenL: 0.96, eyeOpenR: 0.96, irisScale: 1 },
    warm: { brow: 0.14, mouthForm: 0.34, eyeOpenL: 1, eyeOpenR: 1, irisScale: 1.04 },
  }
  const posture: Record<PerformanceBaseline['posture'], Partial<Anime25DDriver>> = {
    closed: { body: -0.16, armY: -0.14, armPos: -0.18 },
    neutral: { body: 0, armY: 0, armPos: 0 },
    open: { body: 0.12, armY: 0.16, armPos: 0.2 },
  }
  return {
    ...expression[baseline.expression],
    ...posture[baseline.posture],
    angleX: (1 - attention) * 0.1,
    eyeX: (1 - attention) * 0.12,
    physAmp: DEFAULT_REAR_HAIR_SWAY * swayEnergy,
    soft: 1.3 + energy * 0.6,
    fhAmp: DEFAULT_FRONT_HAIR_SWAY * swayEnergy,
    idle: true,
    blink: true,
    rand: true,
    phys: true,
  }
}

export function cueDriverPatch(cue: PerformanceCue): Partial<Anime25DDriver> {
  const amount = clamp(cue.intensity, 0.2, 1.4)
  const patches: Record<PerformanceCue['intent'], Partial<Anime25DDriver>> = {
    greet: { angleZ: -0.08 * amount, body: 0.09 * amount, armY: 0.12 * amount },
    respond: { angleY: -0.06 * amount, brow: 0.06 * amount },
    question: { angleZ: 0.11 * amount, brow: 0.22 * amount, body: 0.06 * amount },
    delight: { angleY: -0.09 * amount, mouthForm: 0.48 * amount, armPos: 0.22 * amount, bust: 2.8 },
    emphasize: { angleX: 0.12 * amount, body: 0.22 * amount, brow: 0.13 * amount },
    listen: { angleY: 0.08 * amount, brow: 0.08 * amount, eyeX: 0 },
    notify: { angleZ: -0.06 * amount, body: 0.18 * amount, brow: 0.18 * amount },
  }
  return patches[cue.intent]
}

export function cuePriority(cue: PerformanceCue): number {
  if (cue.intent === 'delight' || cue.intent === 'notify') return 3
  if (cue.intent === 'greet' || cue.intent === 'question' || cue.intent === 'emphasize') return 2
  return 1
}

export function cueDurationMs(cue: PerformanceCue): number {
  return Math.round(
    cue.fadeInMs + Math.max(240, 720 / clamp(cue.tempo, 0.5, 1.6)) + cue.fadeOutMs,
  )
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
