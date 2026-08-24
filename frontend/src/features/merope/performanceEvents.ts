import type {
  MoodTransition,
  PerformanceBaseline,
  PerformanceCue,
  PerformanceDirective,
} from '../../services/agent/types'

export const MEROPE_PERFORMANCE_EVENT = 'arael-merope-performance'
export const MEROPE_STATE_EVENT = 'arael-merope-state'

export interface MeropePerformanceEventDetail {
  text: string
  source: 'reply' | 'proactive' | 'interaction' | 'preview'
  messageId?: string
  performance?: PerformanceDirective
}

export interface MeropeStateEventDetail {
  mood: MoodTransition
  activity: string
}

export function dispatchMeropePerformance(
  detail: unknown,
): void {
  const sanitized = meropePerformanceEventDetail(detail)
  if (!sanitized || typeof window === 'undefined') return
  window.dispatchEvent(
    new CustomEvent<MeropePerformanceEventDetail>(
      MEROPE_PERFORMANCE_EVENT,
      { detail: sanitized },
    ),
  )
}

export function meropePerformanceEventDetail(
  value: unknown,
): MeropePerformanceEventDetail | null {
  if (!isRecord(value)) return null
  const text = typeof value.text === 'string' ? value.text.trim().slice(0, 2_000) : ''
  const performance = sanitizePerformanceDirective(value.performance)
  if (!text && !performance) return null
  const source = ['reply', 'proactive', 'interaction', 'preview'].includes(
    String(value.source),
  )
    ? (value.source as MeropePerformanceEventDetail['source'])
    : 'reply'
  return {
    text,
    source,
    ...(typeof value.messageId === 'string'
      ? { messageId: value.messageId.slice(0, 160) }
      : {}),
    ...(performance ? { performance } : {}),
  }
}

export function dispatchMeropeState(value: unknown): void {
  const detail = meropeStateEventDetail(value)
  if (!detail || typeof window === 'undefined') return
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
  const bands = ['floor', 'low', 'normal', 'high'] as const
  const numbers = [mood.before, mood.after, mood.delta, mood.revision]
  if (
    !numbers.every((number) => typeof number === 'number' && Number.isFinite(number)) ||
    !bands.includes(mood.bandBefore as (typeof bands)[number]) ||
    !bands.includes(mood.bandAfter as (typeof bands)[number])
  ) {
    return null
  }
  return {
    mood: {
      before: clamp(mood.before as number, 0, 100),
      after: clamp(mood.after as number, 0, 100),
      bandBefore: mood.bandBefore as MoodTransition['bandBefore'],
      bandAfter: mood.bandAfter as MoodTransition['bandAfter'],
      delta: clamp(mood.delta as number, -10, 10),
      cause: typeof mood.cause === 'string' ? mood.cause.slice(0, 80) : 'unknown',
      revision: Math.max(0, Math.trunc(mood.revision as number)),
    },
    activity: typeof value.activity === 'string' ? value.activity.slice(0, 32) : 'idle',
  }
}

export function sanitizePerformanceDirective(
  value: unknown,
): PerformanceDirective | null {
  if (!isRecord(value) || !isRecord(value.plan)) return null
  const phases = ['reaction', 'delivery', 'outcome', 'proactive', 'mood'] as const
  if (!phases.includes(value.phase as (typeof phases)[number])) return null
  if (typeof value.moodRevision !== 'number' || !Number.isFinite(value.moodRevision)) return null
  const baseline = sanitizeBaseline(value.plan.baseline)
  const cues = Array.isArray(value.plan.cues)
    ? value.plan.cues.slice(0, 3).map(sanitizeCue).filter((cue): cue is PerformanceCue => cue !== null)
    : []
  if (!baseline && cues.length === 0) return null
  return {
    phase: value.phase as PerformanceDirective['phase'],
    moodRevision: Math.max(0, Math.trunc(value.moodRevision)),
    plan: { ...(baseline ? { baseline } : {}), cues },
  }
}

function sanitizeBaseline(value: unknown): PerformanceBaseline | null {
  if (!isRecord(value)) return null
  const expressions = ['withdrawn', 'subdued', 'steady', 'warm'] as const
  const postures = ['closed', 'neutral', 'open'] as const
  if (
    !expressions.includes(value.expression as (typeof expressions)[number]) ||
    !postures.includes(value.posture as (typeof postures)[number]) ||
    typeof value.motionEnergy !== 'number' ||
    !Number.isFinite(value.motionEnergy) ||
    typeof value.attention !== 'number' ||
    !Number.isFinite(value.attention)
  ) return null
  return {
    expression: value.expression as PerformanceBaseline['expression'],
    posture: value.posture as PerformanceBaseline['posture'],
    motionEnergy: clamp(value.motionEnergy, 0.2, 1.4),
    attention: clamp(value.attention, 0, 1),
  }
}

function sanitizeCue(value: unknown): PerformanceCue | null {
  if (!isRecord(value)) return null
  const intents = ['greet', 'respond', 'question', 'delight', 'emphasize', 'listen', 'notify'] as const
  const interrupts = ['replace', 'queue', 'if-lower'] as const
  if (
    !intents.includes(value.intent as (typeof intents)[number]) ||
    !interrupts.includes(value.interrupt as (typeof interrupts)[number])
  ) return null
  const numericKeys = ['atMs', 'intensity', 'tempo', 'fadeInMs', 'fadeOutMs'] as const
  if (!numericKeys.every((key) => typeof value[key] === 'number' && Number.isFinite(value[key]))) return null
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
