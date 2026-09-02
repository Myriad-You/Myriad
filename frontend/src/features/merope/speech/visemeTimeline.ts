import type { TextVisemeCue } from '../anime25drig/textVisemes'
import type { SpeechViseme } from '../rig/articulation'

/**
 * A spoken segment's mouth shapes, stretched onto the audio that says them.
 *
 * Loudness is not a mouth shape. Deriving the viseme from RMS made a loud /m/
 * — lips shut — read as the widest vowel the table had, so the mouth moved in
 * time with the voice while saying something else. `compileTextVisemes`
 * already turns pinyin, kana and latin into a real phoneme sequence; all it
 * lacked was a clock. The text supplies proportions, the decoded buffer
 * supplies the duration, and the audio keeps deciding *how much* the mouth
 * opens — only no longer *which* shape it makes.
 */
export interface VisemeSpan {
  viseme: SpeechViseme
  /** Seconds from the start of the segment's audio. */
  endsAt: number
  emphasis: boolean
}

/** Below this the segment is a pause, and the mouth closes regardless of shape. */
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
  return spans[spans.length - 1] ?? null
}

/**
 * Emphasis is a stressed syllable, so it opens the mouth further; it never
 * closes one the audio says is loud.
 */
export function visemeAmount(energy: number, emphasis: boolean): number {
  const amount = emphasis ? energy * 1.18 : energy
  return Math.max(0, Math.min(1, amount))
}
