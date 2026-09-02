import type { SpeechProsodyPlan } from './prosody'
import { estimateVisualSpeechDurationMs } from './textTiming'

export interface TextProsodyInput {
  utteranceId: string
  text: string
  locale?: string
  startedAtMs: number
}

const MAX_ACCENTS = 12
const MIN_ACCENT_GAP_MS = 380
const SEMANTIC_FOCUS =
  /但是|不过|其实|重点|关键|一定|必须|特别|最|因此|所以|because|but|however|actually|important|must|especially|therefore/giu

/**
 * Predicts phrase-level visual prosody when no audio timeline exists.
 * Stable utterance hashing adds correlated variation without frame randomness.
 */
export function predictTextProsody(input: TextProsodyInput): SpeechProsodyPlan {
  const text = input.text.normalize('NFKC').slice(0, 2_000)
  const symbols = [...text]
  const durationMs = estimateVisualSpeechDurationMs(text, input.locale)
  const candidates: Array<{ index: number; intensity: number }> = []
  symbols.forEach((symbol, index) => {
    if (/[!?！？]/u.test(symbol)) {
      candidates.push({ index: Math.max(0, index - 1), intensity: 0.92 })
    } else if (/[。.]/u.test(symbol)) {
      candidates.push({ index: Math.max(0, index - 1), intensity: 0.76 })
    } else if (/[，、,;；:：…—]/u.test(symbol) && index >= 4) {
      candidates.push({ index: Math.max(0, index - 1), intensity: 0.66 })
    }
  })
  for (const match of text.matchAll(SEMANTIC_FOCUS)) {
    const codeUnitEnd = (match.index ?? 0) + match[0].length
    candidates.push({
      index: Math.max(0, [...text.slice(0, codeUnitEnd)].length - 1),
      intensity: 0.86,
    })
  }
  if (candidates.length === 0 && symbols.length >= 4) {
    candidates.push({
      index: Math.max(1, Math.round(symbols.length * 0.58)),
      intensity: 0.72,
    })
  }

  const accents: SpeechProsodyPlan['accents'][number][] = []
  for (const [sequence, candidate] of candidates
    .sort((left, right) => left.index - right.index)
    .entries()) {
    const progress = clamp(
      (candidate.index + 0.5) / Math.max(1, symbols.length),
      0,
      1,
    )
    const variation = stableSigned(
      `${input.utteranceId}:${candidate.index}:${sequence}`,
    )
    const offsetMs = Math.round(
      clamp(
        120 + progress * Math.max(180, durationMs - 300) + variation * 52,
        100,
        Math.max(100, durationMs - 120),
      ),
    )
    const previous = accents.at(-1)
    if (previous && offsetMs - previous.offsetMs < MIN_ACCENT_GAP_MS) {
      if (candidate.intensity > previous.intensity) {
        previous.offsetMs = Math.max(previous.offsetMs, offsetMs)
        previous.intensity = clamp(
          candidate.intensity + variation * 0.04,
          0.58,
          1,
        )
      }
      continue
    }
    accents.push({
      offsetMs,
      intensity: clamp(candidate.intensity + variation * 0.06, 0.58, 1),
    })
    if (accents.length >= MAX_ACCENTS) break
  }
  return {
    utteranceId: input.utteranceId,
    startedAtMs: input.startedAtMs,
    durationMs,
    accents,
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
