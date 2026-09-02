import type {
  BehaviorFeedback,
  BehaviorPhase,
  BehaviorPlan,
  BehaviorSnapshot,
  ScheduledBehavior,
  TimePeg,
} from './behavior'

export type BehaviorFeedbackListener = (event: BehaviorFeedback) => void

export type TimePegRetimeResult = 'retimed' | 'missing' | 'locked' | 'invalid'

export type RealizerResult = 'accepted' | 'rejected'

export interface BehaviorPlanRetimeReport {
  compatible: boolean
  pegs: Readonly<Record<string, TimePegRetimeResult>>
}

export interface BehaviorPlanReconcileReport extends BehaviorPlanRetimeReport {
  added: readonly string[]
  removed: readonly string[]
}

/** Preparation may adapt, but cannot chase a moving event indefinitely. */
export const MAX_PREPARATION_RETIME_MS = 160

interface RuntimeBehavior {
  spec: ScheduledBehavior
  phase: BehaviorPhase
  realizer: RealizerResult | null
}

/**
 * Renderer-neutral temporal scheduler.
 *
 * It owns mutable semantic time pegs and behavior lifecycle only. Channel
 * ownership remains with RigMotionCoordinator, and pose generation remains in
 * the body adapter. This keeps scheduling reusable without creating another
 * renderer or another pose writer.
 */
export class BehaviorScheduler {
  private readonly pegs = new Map<string, TimePeg>()
  private readonly behaviors = new Map<string, RuntimeBehavior>()
  private readonly listeners = new Set<BehaviorFeedbackListener>()

  subscribe(listener: BehaviorFeedbackListener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  replace(plan: BehaviorPlan, nowMs: number, recoveryMs = 180): void {
    this.interruptAll(nowMs, recoveryMs)
    for (const peg of plan.pegs) {
      this.pegs.set(peg.id, sanitizePeg(peg))
    }
    for (const spec of plan.behaviors) {
      if (!this.hasValidTiming(spec)) continue
      const phase = this.phaseAt(spec, nowMs)
      const runtime: RuntimeBehavior = { spec, phase, realizer: null }
      this.behaviors.set(spec.id, runtime)
      this.emit({
        type: 'scheduled',
        behaviorId: spec.id,
        atMs: nowMs,
        phase,
      })
    }
    this.prune(nowMs)
  }

  clear(nowMs: number, recoveryMs = 180): void {
    this.interruptAll(nowMs, recoveryMs)
    this.tick(nowMs)
  }

  /**
   * Applies a revised timing estimate without restarting compatible behavior.
   * This is the event-to-TimePeg bridge used by streaming speech prosody.
   */
  retimePlan(plan: BehaviorPlan, nowMs: number): BehaviorPlanRetimeReport {
    const compatible = plan.behaviors.every((next) => {
      const current = this.behaviors.get(next.id)?.spec
      return current !== undefined && sameTimingTopology(current, next)
    })
    if (
      !compatible ||
      plan.pegs.some((peg) => !this.pegs.has(peg.id)) ||
      plan.behaviors.length !== this.behaviors.size
    ) {
      return { compatible: false, pegs: {} }
    }
    const results: Record<string, TimePegRetimeResult> = {}
    for (const peg of plan.pegs) {
      const current = this.pegs.get(peg.id)
      if (current?.atMs === peg.atMs && current.confidence === peg.confidence) {
        continue
      }
      results[peg.id] = this.retimePeg(peg.id, peg.atMs, nowMs, peg.confidence)
    }
    return { compatible: true, pegs: results }
  }

  /**
   * Reconciles one incremental revision without restarting unchanged behavior.
   * New increments are appended, removed increments recover, and compatible
   * increments keep their lifecycle while their future TimePegs move.
   */
  reconcilePlan(
    plan: BehaviorPlan,
    nowMs: number,
    recoveryMs = 180,
  ): BehaviorPlanReconcileReport {
    const nextById = new Map(
      plan.behaviors.map((behavior) => [behavior.id, behavior]),
    )
    for (const next of plan.behaviors) {
      const current = this.behaviors.get(next.id)?.spec
      if (current && !sameTimingTopology(current, next)) {
        return { compatible: false, pegs: {}, added: [], removed: [] }
      }
    }

    const added: string[] = []
    const removed: string[] = []
    for (const peg of plan.pegs) {
      if (!this.pegs.has(peg.id)) this.pegs.set(peg.id, sanitizePeg(peg))
    }
    const results: Record<string, TimePegRetimeResult> = {}
    for (const peg of plan.pegs) {
      const current = this.pegs.get(peg.id)
      if (
        !current ||
        (current.atMs === peg.atMs && current.confidence === peg.confidence)
      ) {
        continue
      }
      results[peg.id] = this.retimePeg(peg.id, peg.atMs, nowMs, peg.confidence)
    }
    for (const id of this.behaviors.keys()) {
      if (nextById.has(id)) continue
      if (this.interrupt(id, nowMs, recoveryMs)) removed.push(id)
    }
    for (const next of plan.behaviors) {
      const current = this.behaviors.get(next.id)
      if (current) {
        current.spec = next
        continue
      }
      if (!this.hasValidTiming(next)) continue
      const phase = this.phaseAt(next, nowMs)
      this.behaviors.set(next.id, { spec: next, phase, realizer: null })
      added.push(next.id)
      this.emit({ type: 'scheduled', behaviorId: next.id, atMs: nowMs, phase })
    }
    this.prune(nowMs)
    return { compatible: true, pegs: results, added, removed }
  }

  tick(nowMs: number): readonly BehaviorSnapshot[] {
    const now = finiteTime(nowMs)
    for (const runtime of this.behaviors.values()) {
      if (runtime.phase === 'rejected') continue
      const next = this.phaseAt(runtime.spec, now)
      if (next === runtime.phase) continue
      const previous = runtime.phase
      runtime.phase = next
      this.emit({
        type: 'phase',
        behaviorId: runtime.spec.id,
        atMs: now,
        phase: next,
        from: previous,
      })
    }
    const snapshots = this.snapshots(now)
    this.prune(now)
    return snapshots
  }

  snapshots(nowMs: number): readonly BehaviorSnapshot[] {
    const now = finiteTime(nowMs)
    const snapshots: BehaviorSnapshot[] = []
    for (const runtime of this.behaviors.values()) {
      snapshots.push(this.snapshot(runtime, now))
    }
    return snapshots.sort((left, right) => left.startedAtMs - right.startedAtMs)
  }

  /** Restates a candidate plan with the scheduler's accepted peg positions. */
  resolvePlan(plan: BehaviorPlan): BehaviorPlan {
    return {
      ...plan,
      pegs: plan.pegs.map((peg) => {
        const resolved = this.pegs.get(peg.id)
        return resolved ? { ...resolved } : peg
      }),
    }
  }

  retimePeg(
    pegId: string,
    requestedAtMs: number,
    nowMs: number,
    confidence?: number,
  ): TimePegRetimeResult {
    const peg = this.pegs.get(pegId)
    if (!peg) return 'missing'
    if (!Number.isFinite(requestedAtMs)) return 'invalid'
    const now = finiteTime(nowMs)
    let nextAt = Math.max(0, requestedAtMs)
    const affected = [...this.behaviors.values()].filter((runtime) =>
      timingPegIds(runtime.spec).includes(pegId),
    )
    for (const runtime of affected) {
      const anticipation = runtime.spec.anticipation === pegId
      const role = timingPegRole(runtime.spec, pegId)
      const phase = this.phaseAt(runtime.spec, now)
      if (!anticipation && pegLocked(role, phase)) return 'locked'
      if (!anticipation && phase === 'preparing') {
        nextAt = clamp(
          nextAt,
          peg.atMs - MAX_PREPARATION_RETIME_MS,
          peg.atMs + MAX_PREPARATION_RETIME_MS,
        )
      }
      if (!anticipation) {
        const bounds = timingBounds(runtime.spec, role, this.pegs)
        nextAt = clamp(nextAt, bounds.minimum, bounds.maximum)
      }
    }
    const previous = peg.atMs
    peg.atMs = nextAt
    if (affected.some((runtime) => !this.hasValidTiming(runtime.spec))) {
      peg.atMs = previous
      return 'invalid'
    }
    if (confidence !== undefined) peg.confidence = unit(confidence)
    peg.revision += 1
    for (const runtime of affected) {
      this.emit({
        type: 'retimed',
        behaviorId: runtime.spec.id,
        atMs: now,
        phase: this.phaseAt(runtime.spec, now),
        pegId,
      })
    }
    return 'retimed'
  }

  interrupt(behaviorId: string, nowMs: number, recoveryMs = 180): boolean {
    const runtime = this.behaviors.get(behaviorId)
    if (
      !runtime ||
      runtime.phase === 'complete' ||
      runtime.phase === 'rejected'
    ) {
      return false
    }
    const now = finiteTime(nowMs)
    const phase = this.phaseAt(runtime.spec, now)
    if (phase === 'planned') {
      runtime.phase = 'complete'
    } else {
      const relaxId = `${runtime.spec.id}:interrupt-relax`
      const endId = `${runtime.spec.id}:interrupt-end`
      this.pegs.set(relaxId, { id: relaxId, atMs: now, revision: 0 })
      this.pegs.set(endId, {
        id: endId,
        atMs: now + Math.max(1, finiteTime(recoveryMs)),
        revision: 0,
      })
      runtime.spec = {
        ...runtime.spec,
        timing: {
          ...runtime.spec.timing,
          ...(phase === 'preparing'
            ? {
                ready: relaxId,
                strokeStart: relaxId,
                strokePeak: relaxId,
                strokeEnd: relaxId,
              }
            : {}),
          relax: relaxId,
          end: endId,
        },
      }
      runtime.phase = 'recovering'
    }
    this.emit({
      type: 'interrupted',
      behaviorId,
      atMs: now,
      phase: runtime.phase,
      from: phase,
    })
    return true
  }

  reportRealizer(
    behaviorId: string,
    result: RealizerResult,
    nowMs: number,
    reason?: BehaviorFeedback['reason'],
  ): boolean {
    const runtime = this.behaviors.get(behaviorId)
    if (!runtime || runtime.realizer === result) return false
    runtime.realizer = result
    if (result === 'rejected') runtime.phase = 'rejected'
    this.emit({
      type: result,
      behaviorId,
      atMs: finiteTime(nowMs),
      phase: runtime.phase,
      ...(reason ? { reason } : {}),
    })
    return true
  }

  private interruptAll(nowMs: number, recoveryMs: number): void {
    for (const behaviorId of [...this.behaviors.keys()]) {
      this.interrupt(behaviorId, nowMs, recoveryMs)
    }
  }

  private phaseAt(spec: ScheduledBehavior, nowMs: number): BehaviorPhase {
    const timing = this.resolvedTiming(spec)
    if (nowMs < timing.start) return 'planned'
    if (nowMs < timing.strokePeak) return 'preparing'
    if (nowMs < timing.strokeEnd) return 'committed'
    if (timing.relax === null || nowMs < timing.relax) return 'holding'
    if (timing.end === null || nowMs < timing.end) return 'recovering'
    return 'complete'
  }

  private snapshot(runtime: RuntimeBehavior, nowMs: number): BehaviorSnapshot {
    const { spec } = runtime
    const timing = this.resolvedTiming(spec)
    const anticipation = spec.anticipation
      ? this.pegs.get(spec.anticipation)
      : undefined
    return {
      id: spec.id,
      function: spec.function,
      kind: spec.kind,
      source: spec.source,
      resources: spec.resources,
      channels: spec.channels,
      form: spec.form,
      phase: runtime.phase,
      startedAtMs: timing.start,
      readyAtMs: timing.ready,
      strokeStartAtMs: timing.strokeStart,
      strokePeakAtMs: timing.strokePeak,
      strokeEndAtMs: timing.strokeEnd,
      relaxAtMs: timing.relax,
      endsAtMs: timing.end,
      remainingMs:
        timing.end === null
          ? null
          : Math.max(0, Math.round(timing.end - nowMs)),
      ...(anticipation
        ? {
            anticipatedAtMs: anticipation.atMs,
            anticipationConfidence: anticipation.confidence ?? 0,
          }
        : {}),
    }
  }

  private hasValidTiming(spec: ScheduledBehavior): boolean {
    const timing = this.resolvedTiming(spec)
    const times = [
      timing.start,
      timing.ready,
      timing.strokeStart,
      timing.strokePeak,
      timing.strokeEnd,
      timing.relax,
      timing.end,
    ].filter((value): value is number => value !== null)
    return times.every(
      (value, index) => index === 0 || value >= times[index - 1]!,
    )
  }

  private resolvedTiming(spec: ScheduledBehavior): ResolvedBehaviorTiming {
    return {
      start: this.pegTime(spec.timing.start),
      ready: this.pegTime(spec.timing.ready),
      strokeStart: this.pegTime(spec.timing.strokeStart),
      strokePeak: this.pegTime(spec.timing.strokePeak),
      strokeEnd: this.pegTime(spec.timing.strokeEnd),
      relax: this.optionalPegTime(spec.timing.relax),
      end: this.optionalPegTime(spec.timing.end),
    }
  }

  private pegTime(id: string): number {
    return this.pegs.get(id)?.atMs ?? Number.POSITIVE_INFINITY
  }

  private optionalPegTime(id: string | null): number | null {
    return id === null ? null : this.pegTime(id)
  }

  private prune(nowMs: number): void {
    for (const [id, runtime] of this.behaviors) {
      const phase =
        runtime.phase === 'rejected'
          ? 'rejected'
          : this.phaseAt(runtime.spec, nowMs)
      if (phase !== 'complete' && phase !== 'rejected') continue
      this.behaviors.delete(id)
    }
    const used = new Set<string>()
    for (const runtime of this.behaviors.values()) {
      for (const id of timingPegIds(runtime.spec)) used.add(id)
    }
    for (const id of this.pegs.keys()) {
      if (!used.has(id)) this.pegs.delete(id)
    }
  }

  private emit(event: BehaviorFeedback): void {
    for (const listener of this.listeners) listener(event)
  }
}

function timingPegIds(spec: ScheduledBehavior): string[] {
  return [
    spec.timing.start,
    spec.timing.ready,
    spec.timing.strokeStart,
    spec.timing.strokePeak,
    spec.timing.strokeEnd,
    spec.timing.relax,
    spec.timing.end,
    spec.anticipation,
  ].filter((id): id is string => typeof id === 'string')
}

function sameTimingTopology(
  left: ScheduledBehavior,
  right: ScheduledBehavior,
): boolean {
  return (
    left.kind === right.kind &&
    left.form.family === right.form.family &&
    left.timing.start === right.timing.start &&
    left.timing.ready === right.timing.ready &&
    left.timing.strokeStart === right.timing.strokeStart &&
    left.timing.strokePeak === right.timing.strokePeak &&
    left.timing.strokeEnd === right.timing.strokeEnd &&
    left.timing.relax === right.timing.relax &&
    left.timing.end === right.timing.end &&
    left.anticipation === right.anticipation
  )
}

function timingPegRole(
  spec: ScheduledBehavior,
  pegId: string,
): keyof ScheduledBehavior['timing'] | null {
  for (const role of [
    'start',
    'ready',
    'strokeStart',
    'strokePeak',
    'strokeEnd',
    'relax',
    'end',
  ] as const) {
    if (spec.timing[role] === pegId) return role
  }
  return null
}

function pegLocked(
  role: keyof ScheduledBehavior['timing'] | null,
  phase: BehaviorPhase,
): boolean {
  if (phase === 'complete' || phase === 'rejected' || phase === 'recovering') {
    return true
  }
  if (phase === 'committed' || phase === 'holding') {
    return role !== 'relax' && role !== 'end'
  }
  return false
}

function timingBounds(
  spec: ScheduledBehavior,
  role: keyof ScheduledBehavior['timing'] | null,
  pegs: ReadonlyMap<string, TimePeg>,
): { minimum: number; maximum: number } {
  if (role === null) return { minimum: 0, maximum: Number.POSITIVE_INFINITY }
  const roles = [
    'start',
    'ready',
    'strokeStart',
    'strokePeak',
    'strokeEnd',
    'relax',
    'end',
  ] as const
  const index = roles.indexOf(role)
  let minimum = 0
  let maximum = Number.POSITIVE_INFINITY
  for (let cursor = index - 1; cursor >= 0; cursor -= 1) {
    const id = spec.timing[roles[cursor]!]
    if (!id) continue
    minimum = pegs.get(id)?.atMs ?? minimum
    break
  }
  for (let cursor = index + 1; cursor < roles.length; cursor += 1) {
    const id = spec.timing[roles[cursor]!]
    if (!id) continue
    maximum = pegs.get(id)?.atMs ?? maximum
    break
  }
  return { minimum, maximum }
}

interface ResolvedBehaviorTiming {
  start: number
  ready: number
  strokeStart: number
  strokePeak: number
  strokeEnd: number
  relax: number | null
  end: number | null
}

function sanitizePeg(peg: TimePeg): TimePeg {
  return {
    id: peg.id,
    atMs: finiteTime(peg.atMs),
    revision: Math.max(0, Math.trunc(peg.revision)),
    ...(peg.confidence === undefined
      ? {}
      : { confidence: unit(peg.confidence) }),
  }
}

function finiteTime(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function unit(value: number): number {
  return Number.isFinite(value) ? clamp(value, 0, 1) : 0
}
