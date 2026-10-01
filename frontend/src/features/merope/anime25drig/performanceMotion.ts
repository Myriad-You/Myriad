import type {
  PerformanceBaseline,
} from '../../../services/agent/types'
import type { Anime25DDriver } from './driver'
import { DEFAULT_FRONT_HAIR_SWAY, DEFAULT_REAR_HAIR_SWAY } from './driver'

export type { ScheduledBodyCue } from '../motion/performanceCueTiming'
export {
  authoredCueEnvelope,
  cueDurationMs,
  cueVisualEnvelope,
  MIN_STICKER_FADE_OUT,
  scheduleBodyCues,
  scheduledBodyCueRemainingDurationMs,
} from '../motion/performanceCueTiming'
export { cueIsSticker } from './performanceCueDefinitions'

type SpeechOwnedDriver = Pick<
  Anime25DDriver,
  'eyeSmile' | 'mouthForm' | 'mouthOpen' | 'talk'
>

/** This patch owns only what the per-frame offset cannot reach */
export function baselineDriverPatch(
  baseline: PerformanceBaseline,
): Partial<Anime25DDriver> {
  const energy = clamp(baseline.motionEnergy, 0.2, 1.4)
  const swayEnergy = 0.7 + energy * 0.3
  return {
    physAmp: DEFAULT_REAR_HAIR_SWAY * swayEnergy,
    soft: 1.3 + energy * 0.6,
    fhAmp: DEFAULT_FRONT_HAIR_SWAY * swayEnergy,
    idle: true,
    blink: true,
    rand: true,
    phys: true,
  }
}

export function restEnergyDriverPatch(): Partial<Anime25DDriver> {
  return {
    physAmp: DEFAULT_REAR_HAIR_SWAY,
    soft: 2,
    fhAmp: DEFAULT_FRONT_HAIR_SWAY,
  }
}

/** Restores only performance-owned pose and secondary-motion channels. */
export function performanceRestDriverPatch(
  baseline: PerformanceBaseline | null,
  thinking: boolean,
): Partial<Anime25DDriver> {
  return {
    body: 0,
    armY: 0,
    armPos: 0,
    bust: 2.5,
    physAmp: DEFAULT_REAR_HAIR_SWAY,
    soft: 2,
    fhAmp: DEFAULT_FRONT_HAIR_SWAY,
    idle: true,
    blink: true,
    phys: true,
    ...(baseline ? baselineDriverPatch(baseline) : {}),
    thinking,
    rand: !thinking,
  }
}

/** A full base refresh must not take mouth ownership during active speech. */
export function idleSpeechDriverPatch(
  mood: number,
  speechActive: boolean,
): Partial<SpeechOwnedDriver> {
  if (speechActive) return {}
  const smile = Math.max(0, (mood - 50) / 80)
  return {
    talk: false,
    mouthOpen: 0,
    mouthForm: smile * 0.28,
    // A good mood shows in the eyes too; it stays while she talks.
    eyeSmile: smile * 0.45,
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
