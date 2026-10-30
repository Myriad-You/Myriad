import type { SpeechGesture } from './phraseGestures'
import type { VisemeSpan } from './visemeTimeline'

export interface SpeechAccentAnchor {
  textOffset?: number
  gesture?: SpeechGesture | 'none'
  offsetMs: number
  intensity: number
}

export interface SpeechProsodyTimeline {
  durationMs: number
  accents: readonly SpeechAccentAnchor[]
}

export interface SpeechProsodyPlan extends SpeechProsodyTimeline {
  utteranceId: string
  startedAtMs: number
}

const MIN_ACCENT_GAP_MS = 360
export const MAX_AUDIO_PROSODY_MS = 30_000
export const MAX_AUDIO_PROSODY_ACCENTS =
  Math.ceil(MAX_AUDIO_PROSODY_MS / MIN_ACCENT_GAP_MS) + 1

export function speechProsodyTimeline(
  spans: readonly VisemeSpan[],
  durationSeconds: number,
): SpeechProsodyTimeline {
  const durationMs = Math.max(0, Math.round(durationSeconds * 1_000))
  if (durationMs === 0) return { durationMs: 0, accents: [] }
  const accents: SpeechAccentAnchor[] = []
  let startsAt = 0
  let groupStart = -1
  let groupEnd = -1
  for (const span of spans) {
    const endsAt = Math.max(startsAt, span.endsAt)
    if (span.emphasis) {
      if (groupStart < 0) groupStart = startsAt
      groupEnd = endsAt
    } else if (groupStart >= 0) {
      pushAccent(accents, groupStart, groupEnd, durationSeconds)
      groupStart = -1
      groupEnd = -1
    }
    startsAt = endsAt
  }
  if (groupStart >= 0) {
    pushAccent(accents, groupStart, groupEnd, durationSeconds)
  }
  return { durationMs, accents: accents.slice(0, MAX_AUDIO_PROSODY_ACCENTS) }
}

function pushAccent(
  accents: SpeechAccentAnchor[],
  startsAt: number,
  endsAt: number,
  durationSeconds: number,
): void {
  const offsetMs = Math.round(
    Math.max(0, Math.min(durationSeconds, (startsAt + endsAt) / 2)) * 1_000,
  )
  const previous = accents.at(-1)
  if (previous && offsetMs - previous.offsetMs < MIN_ACCENT_GAP_MS) {
    if (offsetMs > previous.offsetMs) previous.offsetMs = offsetMs
    previous.intensity = Math.min(1, previous.intensity + 0.12)
    return
  }
  accents.push({ offsetMs, intensity: 0.78 })
}
