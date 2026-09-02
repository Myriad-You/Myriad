import type {
  PerformanceBaseline,
  PerformanceCue,
} from '../../services/agent/types'
import contract from '../../../../shared/merope_performance_contract.json'

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
