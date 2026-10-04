import type {
  BodyControl,
  BodyPose,
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

export const BODY_CONTROLS = contract.bodyControls
export const BODY_POSE_TIMING = contract.bodyPoseTiming

/** Full target restatement, including {} as an explicit release. */
export function sanitizeBodyPose(value: unknown): BodyPose | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const raw = value as Record<string, unknown>
  if (Object.keys(raw).some(k => !['targets', 'transitionMs', 'holdMs'].includes(k))) return null
  if (!raw.targets || typeof raw.targets !== 'object' || Array.isArray(raw.targets)) return null
  const targets: BodyPose['targets'] = {}
  for (const [key, value] of Object.entries(raw.targets)) {
    if (!Object.hasOwn(BODY_CONTROLS, key) || typeof value !== 'number' || !Number.isFinite(value)) return null
    const axis = key as BodyControl
    const limits = BODY_CONTROLS[axis]
    targets[axis] = Math.max(limits.min, Math.min(limits.max, value))
  }
  const transition = raw.transitionMs ?? BODY_POSE_TIMING.transitionMs
  const hold = raw.holdMs ?? BODY_POSE_TIMING.holdMs
  if (typeof transition !== 'number' || !Number.isInteger(transition) || transition < 0
    || typeof hold !== 'number' || !Number.isInteger(hold) || hold < 0) { return null
}
  return {
    targets,
    transitionMs: Math.max(BODY_POSE_TIMING.minTransitionMs, Math.min(BODY_POSE_TIMING.maxTransitionMs, transition)),
    holdMs: Math.min(BODY_POSE_TIMING.maxHoldMs, hold),
  }
}

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
