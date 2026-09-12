import type { TextVisemeCue } from '../anime25drig/textVisemes'
import type { SpeechViseme } from '../rig/articulation'

/** Loudness is not a mouth shape. */
export interface VisemeSpan {
  viseme: SpeechViseme
  endsAt: number
  emphasis: boolean
}

export const VISEME_SILENCE_ENERGY = 0.06

export function alignVisemeTimeline(
  cues: readonly TextVisemeCue[],
  audioDurationSeconds: number,
): VisemeSpan[] {
  if (!Number.isFinite(audioDurationSeconds) || audioDurationSeconds <= 0) {
    return []
  }
  let nominal = 0
  for (const cue of cues) {
    if (Number.isFinite(cue.duration) && cue.duration > 0) nominal += cue.duration
  }
  if (nominal <= 0) return []
  const scale = audioDurationSeconds / nominal
  const spans: VisemeSpan[] = []
  let cursor = 0
  for (const cue of cues) {
    if (!Number.isFinite(cue.duration) || cue.duration <= 0) continue
    cursor += cue.duration * scale
    spans.push({ viseme: cue.viseme, endsAt: cursor, emphasis: cue.emphasis })
  }
  return spans
}

export function visemeAt(
  spans: readonly VisemeSpan[],
  seconds: number,
): VisemeSpan | null {
  if (!spans.length || !Number.isFinite(seconds) || seconds < 0) return null
  for (const span of spans) {
    if (seconds < span.endsAt) return span
  }
  return spans.at(-1) ?? null
}

export function visemeAmount(energy: number, emphasis: boolean): number {
  const amount = emphasis ? energy * 1.18 : energy
  return Math.max(0, Math.min(1, amount))
}
