import type {
  PerformanceBaseline,
  PerformanceCue,
} from '../../../services/agent/types'
import type { Anime25DDriver } from './driver'
import { performanceCuePriority } from '../performanceContract'
import { DEFAULT_FRONT_HAIR_SWAY, DEFAULT_REAR_HAIR_SWAY } from './driver'
import { cueIsSticker } from './performanceCueDefinitions'

export { cueIsSticker } from './performanceCueDefinitions'

export interface ScheduledBodyCue {
  cue: PerformanceCue
  startMs: number
  endMs: number
}

type SpeechOwnedDriver = Pick<
  Anime25DDriver,
  'mouthForm' | 'mouthOpen' | 'talk'
>

/**
 * How hard the body carries itself, from the baseline the director chose.
 *
 * Posture is deliberately absent: `writeBaselineOffset` already puts the same
 * body/armY/armPos on the composed pose every frame, and duplicating it here
 * would apply it twice. This patch owns only what the per-frame offset cannot
 * reach — secondary motion and softness — which is the half of `motionEnergy`
 * that never made it to the rig.
 */
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

/** Secondary-motion defaults, for when no baseline is installed. */
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
    // Thinking has a dedicated constrained loop, so full autonomous actions
    // remain off even when a semantic baseline is currently installed.
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
  }
}

function cuePriority(cue: PerformanceCue): number {
  return performanceCuePriority(cue.intent)
}

const MIN_STICKER_FADE_IN = 0.18
export const MIN_STICKER_FADE_OUT = 0.42

/** Face and body share this envelope so they peak and release together. */
export function cueVisualEnvelope(cue: PerformanceCue): {
  fadeIn: number
  hold: number
  fadeOut: number
} {
  // A scheduled cue carries the hold the scheduler resolved; only an
  // unscheduled one is guessed from tempo.
  return {
    ...authoredCueEnvelope(cue),
    ...(cue.holdMs == null ? {} : { hold: Math.max(0.24, cue.holdMs / 1_000) }),
  }
}

/**
 * The envelope the plan authors, ignoring any hold a previous realization
 * wrote back. The planner must not read its own output: peg spacing derives
 * from this, and feeding a realized hold back in would shrink the cue on
 * every recompile.
 */
export function authoredCueEnvelope(cue: PerformanceCue): {
  fadeIn: number
  hold: number
  fadeOut: number
} {
  const sticker = cueIsSticker(cue.intent)
  return {
    fadeIn: Math.max(cue.fadeInMs / 1_000, sticker ? MIN_STICKER_FADE_IN : 0),
    hold: Math.max(0.24, 0.72 / clamp(cue.tempo, 0.5, 1.6)),
    fadeOut: Math.max(
      cue.fadeOutMs / 1_000,
      sticker ? MIN_STICKER_FADE_OUT : 0,
    ),
  }
}

/** Playback duration, including a scheduler-resolved hold. */
export function cueDurationMs(cue: PerformanceCue): number {
  const envelope = cueVisualEnvelope(cue)
  return Math.round((envelope.fadeIn + envelope.hold + envelope.fadeOut) * 1_000)
}

/** Duration the plan lays out with, before any realization writes back. */
function authoredCueDurationMs(cue: PerformanceCue): number {
  const envelope = authoredCueEnvelope(cue)
  return Math.round((envelope.fadeIn + envelope.hold + envelope.fadeOut) * 1_000)
}

/**
 * Prevents throttled browser timers from replaying an already-expired pose,
 * and respects an interval shortened by a later replacement cue.
 */
export function scheduledBodyCueRemainingDurationMs(
  scheduled: ScheduledBodyCue,
  nowMs: number,
): number {
  return intervalRemainingDurationMs(scheduled.startMs, scheduled.endMs, nowMs)
}

/** Resolves queue/replace timing once so timer throttling cannot change order. */
export function scheduleBodyCues(
  cues: readonly PerformanceCue[],
  originMs: number,
): ScheduledBodyCue[] {
  const scheduled: ScheduledBodyCue[] = []
  const ordered = [...cues].sort((left, right) => left.atMs - right.atMs)
  for (const cue of ordered) {
    let startMs = originMs + cue.atMs
    if (cue.interrupt === 'queue') {
      for (const existing of scheduled) {
        if (existing.endMs > startMs) startMs = existing.endMs
      }
    } else {
      const active = selectedBodyCueAt(scheduled, startMs)
      if (
        cue.interrupt === 'if-lower' &&
        active &&
        cuePriority(cue) <= cuePriority(active.cue)
      ) {
        continue
      }
      for (const existing of scheduled) {
        if (
          existing.startMs <= startMs &&
          existing.endMs > startMs &&
          (cue.interrupt === 'replace' ||
            cuePriority(existing.cue) < cuePriority(cue))
        ) {
          existing.endMs = startMs
        }
      }
    }
    scheduled.push({
      cue,
      startMs,
      endMs: startMs + authoredCueDurationMs(cue),
    })
  }
  return scheduled.sort((left, right) => left.startMs - right.startMs)
}

function selectedBodyCueAt(
  cues: readonly ScheduledBodyCue[],
  nowMs: number,
): ScheduledBodyCue | null {
  let selected: ScheduledBodyCue | null = null
  for (const cue of cues) {
    if (nowMs < cue.startMs || nowMs >= cue.endMs) continue
    if (!selected || cue.cue.interrupt === 'replace') {
      selected = cue
    } else if (
      cue.cue.interrupt === 'if-lower' &&
      cuePriority(cue.cue) > cuePriority(selected.cue)
    ) {
      selected = cue
    }
  }
  return selected
}

function intervalRemainingDurationMs(
  startMs: number,
  endMs: number,
  nowMs: number,
): number {
  if (
    !Number.isFinite(startMs) ||
    !Number.isFinite(endMs) ||
    !Number.isFinite(nowMs) ||
    endMs <= startMs
  ) {
    return 0
  }
  return Math.max(0, endMs - Math.max(startMs, nowMs))
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
