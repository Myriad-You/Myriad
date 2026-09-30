import type {
  PerformanceBaseline,
  PerformanceCue,
  SpeechPhrase,
} from '../../../services/agent/types'
import contract from '../../../../../shared/merope_performance_contract.json' with { type: 'json' }

export const PERFORMANCE_BASELINE_EXPRESSIONS =
  contract.baselineExpressions as PerformanceBaseline['expression'][]
export const PERFORMANCE_POSTURES =
  contract.postures as PerformanceBaseline['posture'][]
export const PERFORMANCE_CUE_INTENTS =
  contract.cueIntents as PerformanceCue['intent'][]
export const PERFORMANCE_INTERRUPT_MODES =
  contract.interruptModes as PerformanceCue['interrupt'][]

const PERFORMANCE_CUE_PRIORITIES = contract.cuePriorities as Record<
  PerformanceCue['intent'],
  number
>

export function performanceCuePriority(
  intent: PerformanceCue['intent'],
): number {
  return PERFORMANCE_CUE_PRIORITIES[intent]
}

export function performanceContractIsComplete(): boolean {
  return (
    new Set(PERFORMANCE_CUE_INTENTS).size === PERFORMANCE_CUE_INTENTS.length &&
    Object.keys(PERFORMANCE_CUE_PRIORITIES).length ===
      PERFORMANCE_CUE_INTENTS.length &&
    PERFORMANCE_CUE_INTENTS.every((intent) => {
      const priority = PERFORMANCE_CUE_PRIORITIES[intent]
      return Number.isInteger(priority) && priority >= 1 && priority <= 3
    })
  )
}

/** Speech phrases as the contract allows them; anything else is dropped. */
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
    const units = Iterator.from(text).reduce((n: number) => n + 1, 0)
    if (
      units < 2 ||
      units > 120 ||
      text.trim() !== text ||
      result.some((other) => other.text === text)
    ) {
      continue
    }
    result.push({ text, intent: item.intent })
  }
  return result
}
