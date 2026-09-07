import type {
  PerformanceCue,
  SpeechPhrase,
} from '../../../services/agent/types'
import type { BehaviorPlan, BehaviorSnapshot } from '../motion/behavior'
import type { SpeechProsodyPlan } from './prosody'
import contract from '../../../../../shared/merope_performance_contract.json'
import { speechAccentBehaviorId } from '../motion/speechBehaviorPlan'
import { unquotedSpeechText } from './phraseGestures'

type Gesture = NonNullable<SpeechProsodyPlan['accents'][number]['gesture']>
const FORM: Record<SpeechPhrase['intent'], Gesture> = {
  ask: 'question',
  hesitate: 'hesitate',
  tease: 'tease',
  explain: 'contrast',
  'check-in': 'check-in',
  laugh: 'laugh',
  none: 'none',
}
const CUE_FORM: Partial<Record<PerformanceCue['intent'], Gesture>> = {
  question: 'question',
  think: 'hesitate',
  silly: 'tease',
  emphasize: 'contrast',
  respond: 'check-in',
  maniac: 'laugh',
}

export interface PhraseCoverage {
  gesture: Gesture
  startMs: number
  endMs: number
}

export function sanitizeSpeechPhrases(value: unknown): SpeechPhrase[] {
  if (!Array.isArray(value)) return []
  const result: SpeechPhrase[] = []
  for (const item of value.slice(0, 6)) {
    if (
      !item ||
      typeof item !== 'object' ||
      typeof item.text !== 'string' ||
      !contract.phraseIntents.includes(item.intent)
    ) {
      continue
    }
    const text = item.text.normalize('NFKC')
    if (
      [...text].length < 2 ||
      [...text].length > 120 ||
      text.trim() !== text ||
      result.some((other) => other.text === text)
    ) {
      continue
    }
    result.push({ text, intent: item.intent })
  }
  return result
}

/** Updates describe fragments, not a replacement for queued spoken segments. */
export function mergeSpeechPhrases(
  previous: readonly SpeechPhrase[],
  incoming: unknown,
): SpeechPhrase[] {
  const phrases = new Map(previous.map((phrase) => [phrase.text, phrase]))
  for (const phrase of sanitizeSpeechPhrases(incoming)) {
    // A correction (including `none`) replaces and refreshes just this fragment.
    phrases.delete(phrase.text)
    phrases.set(phrase.text, phrase)
  }
  return [...phrases.values()].slice(-24)
}

export function directorPhraseCoverage(
  plan: BehaviorPlan | null,
  active: readonly BehaviorSnapshot[] = [],
): PhraseCoverage[] {
  if (!plan) return []
  const times = new Map(plan.pegs.map((peg) => [peg.id, peg.atMs]))
  return plan.behaviors.flatMap((behavior) => {
    const gesture = CUE_FORM[behavior.form.id as PerformanceCue['intent']]
    const resolved = active.find((item) => item.id === behavior.id)
    const startMs = resolved?.startedAtMs ?? times.get(behavior.timing.start)
    const endMs =
      resolved?.endsAtMs ??
      (behavior.timing.end ? times.get(behavior.timing.end) : undefined)
    return gesture && startMs !== undefined && endMs !== undefined
      ? [{ gesture, startMs, endMs }]
      : []
  })
}

/** Same commitment boundary as refinement; never invent a second speech clock. */
export function upcomingSpeechText(
  base: SpeechProsodyPlan,
  text: string,
  active: readonly BehaviorSnapshot[],
  nowMs: number,
): string {
  let from = 0
  let future = false
  base.accents.forEach((accent, index) => {
    if (accent.textOffset === undefined) return
    if (nowMs >= accentCommitment(base, index, active)) {
      from = Math.max(from, accent.textOffset)
    } else {
      future = true
    }
  })
  // Strip quotes before slicing, so a cut inside a quote cannot turn another
  // person's words into this character's emotional evidence.
  return future ? unquotedSpeechText(text.normalize('NFKC')).slice(from) : ''
}

function accentCommitment(
  base: SpeechProsodyPlan,
  index: number,
  active: readonly BehaviorSnapshot[],
): number {
  const accent = base.accents[index]!
  return (
    active.find(
      (item) =>
        item.id === speechAccentBehaviorId(base.utteranceId, accent, index),
    )?.strokeStartAtMs ?? base.startedAtMs + accent.offsetMs - 44
  )
}

/** Only a future beat can change delivery; the scheduler owns commitment. */
export function refineSpeechPhrases(
  base: SpeechProsodyPlan,
  text: string,
  phrases: readonly SpeechPhrase[],
  coverage: readonly PhraseCoverage[],
  previous: SpeechProsodyPlan | null,
  active: readonly BehaviorSnapshot[],
  nowMs: number,
): SpeechProsodyPlan {
  const normalized = text.normalize('NFKC')
  const unquoted = unquotedSpeechText(normalized)
  const spans = phrases.flatMap((phrase) => {
    const fragment = phrase.text.normalize('NFKC')
    const start = normalized.indexOf(fragment)
    if (start < 0 || normalized.includes(fragment, start + 1)) return []
    const end = start + fragment.length
    if (unquoted.slice(start, end) !== fragment) return []
    const anchors = base.accents.filter(
      (accent) =>
        accent.textOffset !== undefined &&
        accent.textOffset >= start &&
        accent.textOffset <= end,
    )
    const chosen =
      phrase.intent === 'hesitate' || phrase.intent === 'explain'
        ? anchors[0]
        : anchors.at(-1)
    const gesture = FORM[phrase.intent]
    const expressed =
      previous?.utteranceId === base.utteranceId &&
      previous.accents.some(
        (accent) =>
          accent.textOffset !== undefined &&
          accent.textOffset >= start &&
          accent.textOffset <= end &&
          accent.gesture === gesture &&
          previous.startedAtMs + accent.offsetMs - 44 <= nowMs,
      )
    return [{ start, end, gesture, chosen: chosen?.textOffset, expressed }]
  })
  const accents = base.accents.map((accent, index) => {
    const old =
      previous?.utteranceId === base.utteranceId
        ? previous.accents.find(
            (item) =>
              item.textOffset !== undefined &&
              item.textOffset === accent.textOffset,
          )
        : undefined
    const behavior = active.find(
      (item) =>
        item.id === speechAccentBehaviorId(base.utteranceId, accent, index),
    )
    const committedAt = accentCommitment(base, index, active)
    if (nowMs >= committedAt) return old ?? accent
    const matches =
      accent.textOffset === undefined
        ? []
        : spans.filter(
            (span) =>
              accent.textOffset! >= span.start &&
              accent.textOffset! <= span.end,
          )
    // Ambiguous or overlapping fragment annotations are not an invitation to guess.
    const match = matches.length === 1 ? matches[0] : undefined
    let gesture = match
      ? match.expressed || match.chosen !== accent.textOffset
        ? 'none'
        : match.gesture
      : accent.gesture
    const peakMs =
      behavior?.strokePeakAtMs ?? base.startedAtMs + accent.offsetMs
    if (
      gesture &&
      coverage.some(
        (item) =>
          item.gesture === gesture &&
          peakMs >= item.startMs &&
          peakMs <= item.endMs + 250,
      )
    ) {
      gesture = 'none'
    }
    return gesture === accent.gesture ? accent : { ...accent, gesture }
  })
  return { ...base, accents }
}
