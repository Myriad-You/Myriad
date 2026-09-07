import type { RigMotionStyle } from '../../../services/agent/types'
import type {
  BehaviorPlan,
  BehaviorRealizerReport,
  BehaviorSnapshot,
  ScheduledBehavior,
  TimePeg,
} from './behavior'
import { BehaviorScheduler } from './behaviorScheduler'

export interface HumanPerformanceFrame {
  /** Candidate plan delivered to the body adapter. Recovery may outlive it. */
  plan: BehaviorPlan | null
  /** Changes only when the body adapter needs a new realization. */
  revision: number
  /** Scheduler-authoritative lifecycle, including graceful recovery. */
  behaviors: readonly BehaviorSnapshot[]
}

const PLAN_ID = 'human-performance'

/**
 * The one behavior clock for speech, music and semantic reactions.
 *
 * Producers publish renderer-neutral candidate plans. This runtime merges
 * them, retimes compatible pegs without restarting live work, and exposes one
 * plan plus one lifecycle stream to every mounted rig.
 */
export class HumanPerformanceRuntime {
  private readonly scheduler = new BehaviorScheduler()
  private schedulerFingerprint = ''
  private deliveryFingerprint = ''
  private deliveryRevision = 0
  private plan: BehaviorPlan | null = null
  private motionStyle: RigMotionStyle = 'even'
  private mergedFrom: readonly (BehaviorPlan | null | undefined)[] | null = null
  private mergedOriginFloor = Number.POSITIVE_INFINITY

  setMotionStyle(style: RigMotionStyle): void {
    this.motionStyle = style
    this.mergedFrom = null
  }

  frame(
    plans: readonly (BehaviorPlan | null | undefined)[],
    nowMs: number,
  ): HumanPerformanceFrame {
    // The fingerprints exist to keep an unchanged plan from disturbing the
    // scheduler, but computing them means serializing every peg and behavior.
    // On the workbench runtime this ran on a 16ms clock, so the check cost far
    // more than the work it was avoiding. Producers rebuild a plan object only
    // when its content changes, so identical references are identical plans
    // and both fingerprints are already known to match.
    if (!this.mergedFrom || !sameBehaviorPlans(plans, this.mergedFrom)) {
      this.mergedFrom = [...plans]
      this.mergedOriginFloor = originFloor(plans)
      const next = mergeBehaviorPlans(plans, nowMs, this.motionStyle)
      const schedulerFingerprint = planFingerprint(next, true)
      if (schedulerFingerprint !== this.schedulerFingerprint) {
        const scheduled = next ?? emptyPlan(nowMs)
        const reconciled = this.scheduler.reconcilePlan(scheduled, nowMs)
        if (!reconciled.compatible) this.scheduler.replace(scheduled, nowMs)
        this.schedulerFingerprint = schedulerFingerprint
        this.plan = next ? this.scheduler.resolvePlan(next) : null
      }
      const deliveryFingerprint = planFingerprint(this.plan, false)
      if (deliveryFingerprint !== this.deliveryFingerprint) {
        this.deliveryFingerprint = deliveryFingerprint
        this.deliveryRevision += 1
      }
    } else if (this.plan) {
      // The one part of a merge that moves without the inputs moving.
      const originMs = Math.min(this.mergedOriginFloor, nowMs)
      if (originMs !== this.plan.originMs) {
        this.plan = { ...this.plan, originMs }
      }
    }
    return {
      plan: this.plan,
      revision: this.deliveryRevision,
      behaviors: this.scheduler.tick(nowMs),
    }
  }

  snapshots(nowMs: number): readonly BehaviorSnapshot[] {
    return this.scheduler.snapshots(nowMs)
  }

  reportRealizer(
    planId: string,
    behaviorId: string,
    result: 'accepted' | 'rejected',
    nowMs: number,
    reason?: BehaviorRealizerReport['reason'],
  ): boolean {
    if (planId !== PLAN_ID) return false
    return this.scheduler.reportRealizer(behaviorId, result, nowMs, reason)
  }

  clear(nowMs: number): void {
    this.scheduler.clear(nowMs)
    this.schedulerFingerprint = ''
    this.deliveryFingerprint = ''
    this.mergedFrom = null
    this.mergedOriginFloor = Number.POSITIVE_INFINITY
    this.plan = null
    this.deliveryRevision += 1
  }
}

function sameBehaviorPlans(
  left: readonly (BehaviorPlan | null | undefined)[],
  right: readonly (BehaviorPlan | null | undefined)[],
): boolean {
  if (left.length !== right.length) return false
  return left.every((plan, index) => plan === right[index])
}

/** Mirrors the `active` filter in `mergeBehaviorPlans`; a test holds them together. */
function originFloor(
  plans: readonly (BehaviorPlan | null | undefined)[],
): number {
  let floor = Number.POSITIVE_INFINITY
  for (const plan of plans) {
    if (!plan?.behaviors.length) continue
    floor = Math.min(floor, plan.originMs)
  }
  return floor
}

export function mergeBehaviorPlans(
  plans: readonly (BehaviorPlan | null | undefined)[],
  nowMs: number,
  motionStyle: RigMotionStyle = 'even',
): BehaviorPlan | null {
  const active = plans.filter((plan): plan is BehaviorPlan =>
    Boolean(plan?.behaviors.length),
  )
  if (active.length === 0) return null
  const pegById = new Map<string, TimePeg>()
  const behaviorById = new Map<string, ScheduledBehavior>()
  for (const plan of active) {
    for (const peg of plan.pegs) {
      const current = pegById.get(peg.id)
      if (!current || peg.revision >= current.revision) pegById.set(peg.id, peg)
    }
    for (const behavior of plan.behaviors) {
      behaviorById.set(behavior.id, applyMotionStyle(behavior, motionStyle))
    }
  }
  return {
    id: PLAN_ID,
    originMs: Math.min(...active.map((plan) => plan.originMs), nowMs),
    metadata: {
      sources: active
        .map((plan) => plan.id)
        .sort()
        .join(','),
    },
    pegs: [...pegById.values()],
    behaviors: [...behaviorById.values()],
  }
}

const BASE_QUALITY = {
  extent: 1,
  tempo: 1,
  power: 1,
  fluidity: 0.8,
  directness: 0.72,
  rebound: 0.35,
  asymmetry: 0.2,
  density: 0.8,
} as const

const STYLE_SCALE: Record<
  RigMotionStyle,
  Readonly<Record<keyof typeof BASE_QUALITY, number>>
> = {
  restrained: {
    extent: 0.78,
    tempo: 0.92,
    power: 0.76,
    fluidity: 1.08,
    directness: 1.04,
    rebound: 0.7,
    asymmetry: 0.8,
    density: 0.72,
  },
  even: {
    extent: 1,
    tempo: 1,
    power: 1,
    fluidity: 1,
    directness: 1,
    rebound: 1,
    asymmetry: 1,
    density: 1,
  },
  open: {
    extent: 1.22,
    tempo: 1.04,
    power: 1.18,
    fluidity: 1.02,
    directness: 0.94,
    rebound: 1.18,
    asymmetry: 1.12,
    density: 1.2,
  },
}

function applyMotionStyle(
  behavior: ScheduledBehavior,
  style: RigMotionStyle,
): ScheduledBehavior {
  if (style === 'even') return behavior
  const scale = STYLE_SCALE[style]
  const source = { ...BASE_QUALITY, ...behavior.quality }
  return {
    ...behavior,
    quality: {
      extent: clamp(source.extent * scale.extent, 0.2, 1.6),
      tempo: clamp(source.tempo * scale.tempo, 0.35, 1.8),
      power: clamp(source.power * scale.power, 0.2, 1.6),
      fluidity: clamp(source.fluidity * scale.fluidity, 0.2, 1.4),
      directness: clamp(source.directness * scale.directness, 0.2, 1.4),
      rebound: clamp(source.rebound * scale.rebound, 0, 1.4),
      asymmetry: clamp(source.asymmetry * scale.asymmetry, 0, 1.4),
      density: clamp(source.density * scale.density, 0.2, 1.5),
    },
  }
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function emptyPlan(nowMs: number): BehaviorPlan {
  return { id: PLAN_ID, originMs: nowMs, pegs: [], behaviors: [] }
}

/**
 * Anticipator positions affect scheduler feedback but not body-adapter setup;
 * the live music evidence already reaches the procedural rhythm unit.
 */
function planFingerprint(
  plan: BehaviorPlan | null,
  includeAnticipation: boolean,
): string {
  if (!plan) return ''
  const anticipation = new Set(
    plan.behaviors
      .map((behavior) => behavior.anticipation)
      .filter((id): id is string => typeof id === 'string'),
  )
  const pegs = plan.pegs
    .filter((peg) => includeAnticipation || !anticipation.has(peg.id))
    .map((peg) => [peg.id, peg.atMs, peg.revision, peg.confidence ?? null])
  const behaviors = plan.behaviors.map((behavior) => ({
    id: behavior.id,
    function: behavior.function,
    kind: behavior.kind,
    source: behavior.source,
    resources: behavior.resources,
    timing: behavior.timing,
    form: behavior.form,
    intensity: behavior.intensity,
    quality: behavior.quality ?? null,
    confidence: behavior.confidence ?? null,
  }))
  return JSON.stringify({ pegs, behaviors })
}
