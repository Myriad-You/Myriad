import type { SpeechProsodyPlan, SpeechProsodyTimeline } from './prosody'
import { speechPhraseGestures } from './phraseGestures'
import { MAX_AUDIO_PROSODY_ACCENTS } from './prosody'
import {
  MAX_VISUAL_SPEECH_TEXT_UNITS,
  visualSpeechPrefixMs,
} from './textTiming'

export interface TextProsodyInput {
  utteranceId: string
  text: string
  locale?: string
  startedAtMs: number
  /** Do not invent a movable midpoint while more text can still arrive. */
  streaming?: boolean
}

const MIN_ACCENT_GAP_MS = 380
const SEMANTIC_FOCUS =
  /但是|不过|其实|重点|关键|一定|必须|特别|因此|所以|\b(?:because|but|however|actually|important|must|especially|therefore)\b/giu

/**
 * Predicts phrase-level visual prosody when no audio timeline exists.
 * Stable utterance hashing adds correlated variation without frame randomness.
 */
export function predictTextProsody(input: TextProsodyInput): SpeechProsodyPlan {
  const text = input.text
    .normalize('NFKC')
    .slice(0, MAX_VISUAL_SPEECH_TEXT_UNITS)
  const symbols = [...text]
  const clock = visualSpeechPrefixMs(text, input.locale)
  const durationMs = clock.at(-1)!
  const candidates: Array<{
    textOffset: number
    intensity: number
    semantic?: boolean
    gesture?: SpeechProsodyPlan['accents'][number]['gesture']
  }> = []
  for (const cue of speechPhraseGestures(text, input.streaming)) {
    candidates.push({
      ...cue,
      semantic: true,
      intensity: cue.gesture === 'laugh' ? 0.95 : 0.9,
    })
  }
  let textOffset = 0
  symbols.forEach((symbol, index) => {
    const boundary = textOffset
    textOffset += symbol.length
    if (/[!?！？]/u.test(symbol)) {
      if (/[!?！？]/u.test(symbols[index - 1] ?? '')) return
      candidates.push({ textOffset: boundary, intensity: 0.92 })
    } else if (
      symbol === '。' ||
      (symbol === '.' && /\s/u.test(symbols[index + 1] ?? '')) ||
      (symbol === '.' && !input.streaming && index === symbols.length - 1)
    ) {
      candidates.push({ textOffset: boundary, intensity: 0.76 })
    } else if (/[，、,;；:：…—]/u.test(symbol) && index >= 4) {
      candidates.push({ textOffset: boundary, intensity: 0.66 })
    }
  })
  for (const match of text.matchAll(SEMANTIC_FOCUS)) {
    const codeUnitEnd = (match.index ?? 0) + match[0].length
    // A streamed "but" may still become "butterfly" in the next delta.
    if (
      input.streaming &&
      codeUnitEnd === text.length &&
      /[a-z]$/iu.test(match[0])
    ) {
      continue
    }
    candidates.push({
      textOffset: codeUnitEnd,
      intensity: 0.86,
      semantic: true,
    })
  }
  if (!input.streaming && candidates.length === 0 && symbols.length >= 4) {
    candidates.push({
      textOffset: symbols
        .slice(0, Math.max(1, Math.round(symbols.length * 0.58)))
        .join('').length,
      intensity: 0.72,
    })
  }

  const accents: SpeechProsodyPlan['accents'][number][] = []
  for (const candidate of candidates.sort(
    (left, right) =>
      left.textOffset - right.textOffset ||
      Number(Boolean(right.semantic)) - Number(Boolean(left.semantic)),
  )) {
    const variation = stableSigned(
      `${input.utteranceId}:${candidate.textOffset}`,
    )
    // Prefix timing is independent of later sentences. In particular, do not
    // squeeze old accents along a new whole-reply character/duration ratio.
    const offsetMs = Math.round(
      Math.max(100, clock[candidate.textOffset]! - 150 + variation * 52),
    )
    const previous = accents.at(-1)
    if (previous && offsetMs - previous.offsetMs < MIN_ACCENT_GAP_MS) {
      // A new nearby punctuation mark must not move or relabel a beat that
      // the body may already have committed to.
      continue
    }
    accents.push({
      textOffset: candidate.textOffset,
      ...(candidate.gesture ? { gesture: candidate.gesture } : {}),
      offsetMs,
      intensity: clamp(candidate.intensity + variation * 0.06, 0.58, 1),
    })
    // Retain the bounded text's whole timeline. The speech source admits only
    // a rolling window to the scheduler; later clauses must not disappear.
  }
  return {
    utteranceId: input.utteranceId,
    startedAtMs: input.startedAtMs,
    durationMs,
    accents,
  }
}

/**
 * Add textual phrasing to the decoded segment's audio clock. This is an
 * estimate, not forced word alignment. Viseme emphasis remains useful where
 * it does not double the same phrase beat; no model request or audio wait.
 */
export function alignTextProsody(
  input: TextProsodyInput,
  audio: SpeechProsodyTimeline,
): SpeechProsodyTimeline {
  if (!Number.isFinite(audio.durationMs) || audio.durationMs <= 0)
    return { durationMs: 0, accents: [] }
  const predicted = predictTextProsody({ ...input, streaming: false })
  const accents: SpeechProsodyPlan['accents'][number][] = []
  for (const accent of predicted.accents) {
    const offsetMs = Math.round(
      clamp(
        (accent.offsetMs / predicted.durationMs) * audio.durationMs,
        0,
        audio.durationMs,
      ),
    )
    if (
      accents.some(
        (other) => Math.abs(other.offsetMs - offsetMs) < MIN_ACCENT_GAP_MS,
      )
    ) {
      continue
    }
    accents.push({ ...accent, offsetMs })
  }
  for (const accent of audio.accents) {
    if (
      accents.some(
        (other) =>
          Math.abs(other.offsetMs - accent.offsetMs) < MIN_ACCENT_GAP_MS,
      )
    ) {
      continue
    }
    accents.push(accent)
  }
  return {
    durationMs: audio.durationMs,
    accents: accents
      .sort((a, b) => a.offsetMs - b.offsetMs)
      .slice(0, MAX_AUDIO_PROSODY_ACCENTS),
  }
}

/** Keep published text beats; late stream increments start in the future. */
export function continueTextProsody(
  predicted: SpeechProsodyPlan,
  previous: SpeechProsodyPlan | null,
  nowMs: number,
): SpeechProsodyPlan {
  const retained =
    previous?.utteranceId === predicted.utteranceId ? previous.accents : []
  let shiftMs = 0
  const accents: SpeechProsodyPlan['accents'][number][] = []
  for (const accent of predicted.accents) {
    const existing = retained.find(
      (item) => item.textOffset === accent.textOffset,
    )
    const offsetMs =
      existing?.offsetMs ??
      Math.max(
        accent.offsetMs + shiftMs,
        nowMs - predicted.startedAtMs + 140,
        (accents.at(-1)?.offsetMs ?? -MIN_ACCENT_GAP_MS) + MIN_ACCENT_GAP_MS,
      )
    shiftMs = offsetMs - accent.offsetMs
    accents.push(existing ?? { ...accent, offsetMs })
  }
  return {
    ...predicted,
    accents,
    durationMs: Math.max(
      predicted.durationMs,
      (accents.at(-1)?.offsetMs ?? 0) + 270,
    ),
  }
}

function stableSigned(seed: string): number {
  let hash = 2166136261
  for (let index = 0; index < seed.length; index += 1) {
    hash ^= seed.charCodeAt(index)
    hash = Math.imul(hash, 16777619)
  }
  return ((hash >>> 0) / 4_294_967_295) * 2 - 1
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
