import type { SpeechArticulation, SpeechViseme } from './rig/articulation'
import type { SpeechGesture } from './speech/phraseGestures'
import type { SpeechProsodyPlan } from './speech/prosody'
import { SPEECH_GESTURES } from './speech/phraseGestures'
import {
  MAX_AUDIO_PROSODY_ACCENTS,
  MAX_AUDIO_PROSODY_MS,
} from './speech/prosody'

export const MEROPE_SPEECH_EVENT = 'merope-speech'

export type MeropeSpeechSource =
  'reply' | 'proactive' | 'interaction' | 'preview'

interface SpeechEventBase {
  messageId: string
  source: MeropeSpeechSource
  utteranceId: string
  locale?: string
  generation?: number
}

export type MeropeSpeechEventDetail =
  | (SpeechEventBase & { phase: 'start' | 'end' })
  | (SpeechEventBase & { phase: 'chunk'; text: string })
  | (SpeechEventBase & { phase: 'energy'; energy: number })
  | (SpeechEventBase & {
      phase: 'articulation'
      articulation: SpeechArticulation
    })
  | (SpeechEventBase & {
      phase: 'prosody'
      prosody: SpeechProsodyPlan
      text?: string
    })
  | (Omit<SpeechEventBase, 'utteranceId'> & {
      phase: 'cancel'
      utteranceId?: string
    })

export interface SpeechUtteranceInput {
  messageId: string
  source: MeropeSpeechSource
  text: string
  utteranceId: string
  locale?: string
  generation?: number
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
    ...(utterance.locale ? { locale: utterance.locale } : {}),
    ...(utterance.generation && utterance.generation > 0
      ? { generation: utterance.generation }
      : {}),
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
  const locale = boundedLocale(value.locale)
  const generation = boundedGeneration(value.generation)

  if (phase === 'cancel') {
    return {
      phase,
      messageId,
      source,
      ...(locale ? { locale } : {}),
      ...(utteranceId ? { utteranceId } : {}),
      ...(generation ? { generation } : {}),
    }
  }
  if (!utteranceId) return null
  const base = {
    messageId,
    source,
    utteranceId,
    ...(locale ? { locale } : {}),
    ...(generation ? { generation } : {}),
  }
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
  if (phase === 'prosody') {
    const prosody = sanitizeProsody(value.prosody, utteranceId)
    return prosody
      ? {
          ...base,
          phase,
          prosody,
          ...(typeof value.text === 'string'
            ? { text: value.text.slice(0, 2_000) }
            : {}),
        }
      : null
  }
  return null
}

function sanitizeProsody(
  value: unknown,
  utteranceId: string,
): SpeechProsodyPlan | null {
  if (!isRecord(value)) return null
  if (
    typeof value.startedAtMs !== 'number' ||
    !Number.isFinite(value.startedAtMs) ||
    typeof value.durationMs !== 'number' ||
    !Number.isFinite(value.durationMs)
  ) {
    return null
  }
  const rawAccents = Array.isArray(value.accents) ? value.accents : []
  const accents = rawAccents
    .filter(isRecord)
    .flatMap((accent) => {
      if (
        typeof accent.offsetMs !== 'number' ||
        !Number.isFinite(accent.offsetMs) ||
        typeof accent.intensity !== 'number' ||
        !Number.isFinite(accent.intensity)
      ) {
        return []
      }
      return [
        {
          offsetMs: clamp(accent.offsetMs, 0, MAX_AUDIO_PROSODY_MS),
          intensity: clamp(accent.intensity, 0, 1),
          ...(accent.gesture === 'none' ||
          SPEECH_GESTURES.includes(accent.gesture as SpeechGesture)
            ? { gesture: accent.gesture as SpeechGesture | 'none' }
            : {}),
          ...(typeof accent.textOffset === 'number' &&
          Number.isInteger(accent.textOffset) &&
          accent.textOffset >= 0 &&
          accent.textOffset <= 2_000
            ? { textOffset: accent.textOffset }
            : {}),
        },
      ]
    })
    .slice(0, MAX_AUDIO_PROSODY_ACCENTS)
    .sort((left, right) => left.offsetMs - right.offsetMs)
  return {
    utteranceId,
    startedAtMs: Math.max(0, value.startedAtMs),
    durationMs: clamp(value.durationMs, 0, MAX_AUDIO_PROSODY_MS),
    accents,
  }
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

function boundedGeneration(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
    ? Math.min(1_000_000_000, Math.trunc(value))
    : 0
}

function boundedLocale(value: unknown): string {
  return typeof value === 'string' &&
    /^[A-Z]{2,3}(?:-[A-Z0-9]{2,8})?$/i.test(value)
    ? value.slice(0, 24)
    : ''
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
