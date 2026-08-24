import type { SpeechArticulation, SpeechViseme } from './rig/articulation'

export const MEROPE_SPEECH_EVENT = 'arael-merope-speech'

export type MeropeSpeechSource =
  | 'reply'
  | 'proactive'
  | 'interaction'
  | 'preview'

interface SpeechEventBase {
  messageId: string
  source: MeropeSpeechSource
  utteranceId: string
}

export type MeropeSpeechEventDetail =
  | (SpeechEventBase & { phase: 'start' | 'end' })
  | (SpeechEventBase & { phase: 'chunk'; text: string })
  | (SpeechEventBase & { phase: 'energy'; energy: number })
  | (SpeechEventBase & {
      phase: 'articulation'
      articulation: SpeechArticulation
    })
  | (Omit<SpeechEventBase, 'utteranceId'> & {
      phase: 'cancel'
      utteranceId?: string
    })

interface SpeechUtteranceInput {
  messageId: string
  source: MeropeSpeechSource
  text: string
  utteranceId: string
}

const SOURCES: readonly MeropeSpeechSource[] = [
  'reply',
  'proactive',
  'interaction',
  'preview',
]
const VISEMES: readonly SpeechViseme[] = [
  'rest',
  'closed',
  'open',
  'wide',
  'round',
  'narrow',
]

export function dispatchMeropeSpeech(detail: unknown): void {
  const sanitized = meropeSpeechEventDetail(detail)
  if (!sanitized || typeof window === 'undefined') return
  window.dispatchEvent(
    new CustomEvent<MeropeSpeechEventDetail>(MEROPE_SPEECH_EVENT, {
      detail: sanitized,
    }),
  )
}

/** Dispatch a complete non-streamed reply through the same lifecycle as SSE. */
export function dispatchMeropeSpeechUtterance(
  utterance: SpeechUtteranceInput,
): void {
  const text = boundedText(utterance.text)
  if (!text) return
  const base = {
    messageId: utterance.messageId,
    source: utterance.source,
    utteranceId: utterance.utteranceId,
  }
  dispatchMeropeSpeech({ ...base, phase: 'start' })
  dispatchMeropeSpeech({ ...base, phase: 'chunk', text })
  dispatchMeropeSpeech({ ...base, phase: 'end' })
}

export function meropeSpeechEventDetail(
  value: unknown,
): MeropeSpeechEventDetail | null {
  if (!isRecord(value)) return null
  const phase = String(value.phase)
  const messageId = boundedId(value.messageId)
  if (!messageId) return null
  const source = SOURCES.includes(value.source as MeropeSpeechSource)
    ? (value.source as MeropeSpeechSource)
    : 'reply'
  const utteranceId = boundedId(value.utteranceId)

  if (phase === 'cancel') {
    return {
      phase,
      messageId,
      source,
      ...(utteranceId ? { utteranceId } : {}),
    }
  }
  if (!utteranceId) return null
  const base = { messageId, source, utteranceId }
  if (phase === 'start' || phase === 'end') return { ...base, phase }
  if (phase === 'chunk') {
    const text = boundedChunk(value.text)
    return text ? { ...base, phase, text } : null
  }
  if (phase === 'energy') {
    return typeof value.energy === 'number' && Number.isFinite(value.energy)
      ? { ...base, phase, energy: clamp(value.energy, 0, 1) }
      : null
  }
  if (phase === 'articulation') {
    const articulation = sanitizeArticulation(value.articulation)
    return articulation ? { ...base, phase, articulation } : null
  }
  return null
}

function sanitizeArticulation(value: unknown): SpeechArticulation | null {
  if (!isRecord(value) || !VISEMES.includes(value.viseme as SpeechViseme)) {
    return null
  }
  const energy = value.energy
  if (
    energy !== null &&
    (typeof energy !== 'number' || !Number.isFinite(energy))
  ) {
    return null
  }
  if (typeof value.amount !== 'number' || !Number.isFinite(value.amount)) {
    return null
  }
  return {
    energy: energy === null ? null : clamp(energy, 0, 1),
    viseme: value.viseme as SpeechViseme,
    amount: clamp(value.amount, 0, 1),
  }
}

function boundedText(value: unknown): string {
  return typeof value === 'string' ? value.trim().slice(0, 2_000) : ''
}

function boundedChunk(value: unknown): string {
  if (typeof value !== 'string') return ''
  const text = value.slice(0, 2_000)
  return text.trim() ? text : ''
}

function boundedId(value: unknown): string {
  return typeof value === 'string' ? value.trim().slice(0, 160) : ''
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
