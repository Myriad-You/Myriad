import type { BehaviorPlan, BehaviorRealizerReport } from '../motion/behavior'
import type { BehaviorRealizerContext } from '../motion/behaviorRealizerRegistry'
import type { Anime25DMotionUnit } from './behaviorMotion'
import type { CueIntent } from './performanceCueDefinitions'
import { BehaviorRealizerRegistry } from '../motion/behaviorRealizerRegistry'
import { PERFORMANCE_CUE_INTENTS } from '../performanceContract'
import { completeBehaviorQuality } from './behaviorMotion'

export interface Anime25DBehaviorRealization {
  units: readonly Anime25DMotionUnit[]
  reports: readonly BehaviorRealizerReport[]
}

/**
 * Every family realizes into the same motion unit. A behavior carries a form
 * and a resolved lifecycle; nothing downstream reconstructs the director's
 * wire shape to find out what to draw.
 */
const registry = new BehaviorRealizerRegistry<Anime25DMotionUnit>().register(
  'performance-cue',
  (behavior, context) => {
    if (!PERFORMANCE_CUE_INTENTS.includes(behavior.form.id as CueIntent)) {
      return null
    }
    const timing = realizedUnitTiming(behavior.timing, context)
    if (!timing) return null
    return {
      behaviorId: behavior.id,
      family: 'performance',
      form: behavior.form.id,
      kind: behavior.kind,
      timing,
      intensity: clamp(behavior.intensity, 0.2, 1.4),
      quality: completeBehaviorQuality(behavior.quality),
    }
  },
)

registry.register('co-speech', (behavior, context) => {
  if (behavior.form.id !== 'presence' && behavior.form.id !== 'accent') {
    return null
  }
  const timing = realizedUnitTiming(behavior.timing, context)
  if (!timing) return null
  return {
    behaviorId: behavior.id,
    family: 'co-speech',
    form: behavior.form.id,
    kind: behavior.kind,
    timing,
    intensity: clamp(behavior.intensity, 0.2, 1.4),
    quality: completeBehaviorQuality(behavior.quality),
  }
})

registry.register('music', (behavior, context) => {
  if (behavior.form.id !== 'groove') return null
  const timing = realizedUnitTiming(behavior.timing, context)
  if (!timing) return null
  return {
    behaviorId: behavior.id,
    family: 'music',
    form: behavior.form.id,
    kind: behavior.kind,
    timing,
    intensity: clamp(behavior.intensity, 0.2, 1.4),
    quality: completeBehaviorQuality(behavior.quality),
  }
})

export function realizeAnime25DBehaviorPlan(
  plan: BehaviorPlan,
  nowMs: number,
): Anime25DBehaviorRealization {
  const realized = registry.realize(plan, nowMs)
  return { units: realized.outputs, reports: realized.reports }
}

function realizedUnitTiming(
  timing: BehaviorPlan['behaviors'][number]['timing'],
  context: BehaviorRealizerContext,
): Anime25DMotionUnit['timing'] | null {
  const startMs = context.pegTimes.get(timing.start)
  const readyMs = context.pegTimes.get(timing.ready)
  const strokeStartMs = context.pegTimes.get(timing.strokeStart)
  const strokePeakMs = context.pegTimes.get(timing.strokePeak)
  const strokeEndMs = context.pegTimes.get(timing.strokeEnd)
  const relaxMs =
    timing.relax === null ? null : context.pegTimes.get(timing.relax)
  const endMs = timing.end === null ? null : context.pegTimes.get(timing.end)
  if (
    startMs === undefined ||
    readyMs === undefined ||
    strokeStartMs === undefined ||
    strokePeakMs === undefined ||
    strokeEndMs === undefined ||
    relaxMs === undefined ||
    endMs === undefined ||
    startMs > readyMs ||
    readyMs > strokeStartMs ||
    strokeStartMs > strokePeakMs ||
    strokePeakMs > strokeEndMs ||
    (relaxMs !== null && strokeEndMs > relaxMs) ||
    (endMs !== null && (relaxMs === null || relaxMs > endMs))
  ) {
    return null
  }
  return {
    startMs,
    readyMs,
    strokeStartMs,
    strokePeakMs,
    strokeEndMs,
    relaxMs,
    endMs,
  }
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
