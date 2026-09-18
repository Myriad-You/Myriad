import type {
  MoodTransition,
  PerformanceBaseline,
  PerformanceCue,
  PerformanceDirective,
  RigMotionStyle,
} from '../../services/agent/types'
import { moodBand } from '../../components/agent/meropeVitals'
import {
  PERFORMANCE_BASELINE_EXPRESSIONS,
  PERFORMANCE_CUE_INTENTS,
  PERFORMANCE_INTERRUPT_MODES,
  PERFORMANCE_POSTURES,
} from './performanceContract'
import { sanitizeSpeechPhrases } from './speech/phrasePlan'

import {
  currentMeropeState,
  writeMeropeState,
  type MeropeStateEventDetail,
} from './meropeAffectState'

export const MEROPE_PERFORMANCE_EVENT = 'merope-performance'
export const MEROPE_STATE_EVENT = 'merope-state'

export {
  currentMeropeState,
  resetMeropeState,
  type MeropeStateEventDetail,
} from './meropeAffectState'

export interface MeropePerformanceEventDetail {
  text: string
  source: 'reply' | 'proactive' | 'interaction' | 'preview'
  messageId?: string
  runId?: string
  generation?: number
  motionIntentId?: string
  performance?: PerformanceDirective
}

/** A slow GET must not undo a newer live event */
export function resolveLoadedMeropeAffect(snapshot: {
  mood?: number
  arousal?: number
  moodRevision?: number
  activity?: string
}): { mood: number; arousal: number } {
  const mood = snapshot.mood ?? 70
  const arousal = snapshot.arousal ?? 48
  const revision = snapshot.moodRevision ?? 0
  dispatchMeropeState({
    mood: {
      before: mood,
      after: mood,
      arousalBefore: arousal,
      arousalAfter: arousal,
      bandBefore: moodBand(mood, arousal),
      bandAfter: moodBand(mood, arousal),
      delta: 0,
      cause: 'state_snapshot',
      revision,
    },
    activity: snapshot.activity ?? 'idle',
  })
  const live = currentMeropeState()
  if (live && live.mood.revision > revision) {
    return {
      mood: live.mood.after,
      arousal: live.mood.arousalAfter ?? arousal,
    }
  }
  return { mood, arousal }
}

export function dispatchMeropePerformance(detail: unknown): void {
  const sanitized = meropePerformanceEventDetail(detail)
  if (!sanitized || typeof window === 'undefined') return
  window.dispatchEvent(
    new CustomEvent<MeropePerformanceEventDetail>(MEROPE_PERFORMANCE_EVENT, {
      detail: sanitized,
    }),
  )
}

export function meropePerformanceEventDetail(
  value: unknown,
): MeropePerformanceEventDetail | null {
  if (!isRecord(value)) return null
  const text =
    typeof value.text === 'string' ? value.text.trim().slice(0, 2_000) : ''
  const performance = sanitizePerformanceDirective(value.performance)
  if (!text && !performance) return null
  const source = ['reply', 'proactive', 'interaction', 'preview'].includes(
    String(value.source),
  )
    ? (value.source as MeropePerformanceEventDetail['source'])
    : 'reply'
  const messageId =
    typeof value.messageId === 'string'
      ? value.messageId.trim().slice(0, 160)
      : ''
  const generation =
    typeof value.generation === 'number' &&
    Number.isFinite(value.generation) &&
    value.generation > 0
      ? Math.min(1_000_000_000, Math.trunc(value.generation))
      : 0
  const runId =
    typeof value.runId === 'string' ? value.runId.trim().slice(0, 160) : ''
  const motionIntentId =
    typeof value.motionIntentId === 'string'
      ? value.motionIntentId.trim().slice(0, 160)
      : ''
  return {
    text,
    source,
    ...(messageId ? { messageId } : {}),
    ...(runId ? { runId } : {}),
    ...(generation ? { generation } : {}),
    ...(motionIntentId ? { motionIntentId } : {}),
    ...(performance ? { performance } : {}),
  }
}

export function dispatchMeropeState(value: unknown): void {
  const detail = meropeStateEventDetail(value)
  const live = currentMeropeState()
  if (!detail || (live && detail.mood.revision <= live.mood.revision)) {
    return
  }
  writeMeropeState(detail)
  if (typeof window === 'undefined') return
  window.dispatchEvent(
    new CustomEvent<MeropeStateEventDetail>(MEROPE_STATE_EVENT, {
      detail,
    }),
  )
}

export function meropeStateEventDetail(
  value: unknown,
): MeropeStateEventDetail | null {
  if (!isRecord(value) || !isRecord(value.mood)) return null
  const mood = value.mood
  const bandBefore = moodBandName(mood.bandBefore)
  const bandAfter = moodBandName(mood.bandAfter)
  const numbers = [mood.before, mood.after, mood.delta, mood.revision]
  if (
    !numbers.every(
      (number) => typeof number === 'number' && Number.isFinite(number),
    ) ||
    !bandBefore ||
    !bandAfter
  ) {
    return null
  }
  const arousalBefore = optionalArousal(mood.arousalBefore)
  const arousalAfter = optionalArousal(mood.arousalAfter)
  return {
    mood: {
      before: clamp(mood.before as number, 0, 100),
      after: clamp(mood.after as number, 0, 100),
      ...(arousalBefore !== undefined ? { arousalBefore } : {}),
      ...(arousalAfter !== undefined ? { arousalAfter } : {}),
      bandBefore,
      bandAfter,
      delta: clamp(mood.delta as number, -10, 10),
      cause:
        typeof mood.cause === 'string' ? mood.cause.slice(0, 80) : 'unknown',
      revision: Math.max(0, Math.trunc(mood.revision as number)),
    },
    activity:
      typeof value.activity === 'string' ? value.activity.slice(0, 32) : 'idle',
  }
}

function moodBandName(value: unknown): MoodTransition['bandBefore'] | null {
  switch (value) {
    case 'floor':
      return 'floor'
    case 'sad':
    case 'low':
      return 'sad'
    case 'tense':
      return 'tense'
    case 'calm':
    case 'normal':
      return 'calm'
    case 'excited':
    case 'high':
      return 'excited'
    default:
      return null
  }
}

function optionalArousal(value: unknown): number | undefined {
  if (typeof value !== 'number' || !Number.isFinite(value)) return undefined
  return clamp(value, 0, 100)
}

export function sanitizePerformanceDirective(
  value: unknown,
): PerformanceDirective | null {
  if (!isRecord(value) || !isRecord(value.plan)) return null
  const phases = [
    'reaction',
    'delivery',
    'outcome',
    'proactive',
    'mood',
  ] as const
  if (!phases.includes(value.phase as (typeof phases)[number])) return null
  if (
    typeof value.moodRevision !== 'number' ||
    !Number.isFinite(value.moodRevision)
  ) {
    return null
  }
  const motionStyle = sanitizeMotionStyle(value.motionStyle)
  if (!motionStyle) return null
  const baseline = sanitizeBaseline(value.plan.baseline)
  const cues = Array.isArray(value.plan.cues)
    ? value.plan.cues
        .slice(0, 3)
        .map(sanitizeCue)
        .filter((cue): cue is PerformanceCue => cue !== null)
    : []
  const phrases = sanitizeSpeechPhrases(value.phrases)
  if (!baseline && cues.length === 0 && phrases.length === 0) return null
  return {
    phase: value.phase as PerformanceDirective['phase'],
    moodRevision: Math.max(0, Math.trunc(value.moodRevision)),
    motionStyle,
    plan: { ...(baseline ? { baseline } : {}), cues },
    ...(phrases.length ? { phrases } : {}),
  }
}

function sanitizeMotionStyle(value: unknown): RigMotionStyle | null {
  return value === 'restrained' || value === 'even' || value === 'open'
    ? value
    : null
}

function sanitizeBaseline(value: unknown): PerformanceBaseline | null {
  if (!isRecord(value)) return null
  if (
    !PERFORMANCE_BASELINE_EXPRESSIONS.includes(
      value.expression as PerformanceBaseline['expression'],
    ) ||
    !PERFORMANCE_POSTURES.includes(
      value.posture as PerformanceBaseline['posture'],
    ) ||
    typeof value.motionEnergy !== 'number' ||
    !Number.isFinite(value.motionEnergy) ||
    typeof value.attention !== 'number' ||
    !Number.isFinite(value.attention)
  ) {
    return null
  }
  return {
    expression: value.expression as PerformanceBaseline['expression'],
    posture: value.posture as PerformanceBaseline['posture'],
    motionEnergy: clamp(value.motionEnergy, 0.2, 1.4),
    attention: clamp(value.attention, 0, 1),
  }
}

function sanitizeCue(value: unknown): PerformanceCue | null {
  if (!isRecord(value)) return null
  if (
    !PERFORMANCE_CUE_INTENTS.includes(
      value.intent as PerformanceCue['intent'],
    ) ||
    !PERFORMANCE_INTERRUPT_MODES.includes(
      value.interrupt as PerformanceCue['interrupt'],
    )
  ) {
    return null
  }
  const numericKeys = [
    'atMs',
    'intensity',
    'tempo',
    'fadeInMs',
    'fadeOutMs',
  ] as const
  if (
    !numericKeys.every(
      (key) => typeof value[key] === 'number' && Number.isFinite(value[key]),
    )
  ) {
    return null
  }
  return {
    intent: value.intent as PerformanceCue['intent'],
    atMs: Math.trunc(clamp(value.atMs as number, 0, 5_000)),
    intensity: clamp(value.intensity as number, 0.2, 1.4),
    tempo: clamp(value.tempo as number, 0.5, 1.6),
    fadeInMs: Math.trunc(clamp(value.fadeInMs as number, 40, 600)),
    fadeOutMs: Math.trunc(clamp(value.fadeOutMs as number, 60, 800)),
    interrupt: value.interrupt as PerformanceCue['interrupt'],
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
