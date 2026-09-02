import type { PerformanceDirective } from '../../../services/agent/types'
import type { BehaviorPlan } from './behavior'
import type { MotionLeaseHandle, RigMotionCoordinator } from './coordinator'
import { resourceInGroup } from './behaviorResources'
import { compilePerformanceBehaviorPlan } from './performanceBehaviorPlan'

type PerformanceLeaseChannels =
  readonly ['expression'] | readonly ['headBody'] | readonly ['gaze']

export interface PerformanceLeaseWindows {
  planUntilMs: number
  expressionCueUntilMs: number | null
  headBodyCueUntilMs: number | null
  /**
   * Cues that drive eyeX/eyeY take the eyes for as long as they play. Without
   * this window they can be classified as gaze motion and still never claim
   * it, leaving ambient drift to pull against the directed look.
   */
  gazeCueUntilMs: number | null
}

/**
 * Timed windows for transient behavior only. Persistent bearing is a base
 * pose, not a lease, and therefore never takes expression or body ownership.
 */
export function performanceLeaseWindows(
  directive: PerformanceDirective,
  originMs: number,
  behaviorPlan: BehaviorPlan = compilePerformanceBehaviorPlan(
    directive,
    originMs,
    'lease-window',
  ),
): PerformanceLeaseWindows {
  let expressionCueUntilMs: number | null = null
  let headBodyCueUntilMs: number | null = null
  let gazeCueUntilMs: number | null = null
  let planUntilMs = originMs
  const pegTimes = new Map(behaviorPlan.pegs.map((peg) => [peg.id, peg.atMs]))
  for (const behavior of behaviorPlan.behaviors) {
    const endMs = behavior.timing.end
      ? (pegTimes.get(behavior.timing.end) ?? originMs)
      : originMs
    planUntilMs = Math.max(planUntilMs, endMs)
    if (
      behavior.resources.some((resource) =>
        resourceInGroup(resource, 'face.expression'),
      )
    ) {
      expressionCueUntilMs = maxTime(expressionCueUntilMs, endMs)
    }
    if (
      behavior.resources.some(
        (resource) =>
          resourceInGroup(resource, 'body') ||
          resourceInGroup(resource, 'secondary'),
      )
    ) {
      headBodyCueUntilMs = maxTime(headBodyCueUntilMs, endMs)
    }
    if (
      behavior.resources.some((resource) =>
        resourceInGroup(resource, 'face.gaze'),
      )
    ) {
      gazeCueUntilMs = maxTime(gazeCueUntilMs, endMs)
    }
  }
  return {
    planUntilMs,
    expressionCueUntilMs,
    headBodyCueUntilMs,
    gazeCueUntilMs,
  }
}

/**
 * One performance producer: baseline expression, and timed expression,
 * head/body and gaze cues. Each is an independent lease handle.
 */
export class PerformanceMotionLeases {
  private expressionCue: MotionLeaseHandle | null = null
  private headBodyCue: MotionLeaseHandle | null = null
  private gazeCue: MotionLeaseHandle | null = null

  constructor(private readonly coordinator: RigMotionCoordinator) {}

  apply(
    directive: PerformanceDirective,
    nowMs: number,
    behaviorPlan?: BehaviorPlan,
  ): PerformanceLeaseWindows {
    const windows = performanceLeaseWindows(directive, nowMs, behaviorPlan)
    this.expressionCue = this.syncTimed(
      this.expressionCue,
      ['expression'],
      windows.expressionCueUntilMs,
      nowMs,
    )
    this.headBodyCue = this.syncTimed(
      this.headBodyCue,
      ['headBody'],
      windows.headBodyCueUntilMs,
      nowMs,
    )
    this.gazeCue = this.syncTimed(
      this.gazeCue,
      ['gaze'],
      windows.gazeCueUntilMs,
      nowMs,
    )
    return windows
  }

  releaseAll(): void {
    this.coordinator.release(this.expressionCue)
    this.coordinator.release(this.headBodyCue)
    this.coordinator.release(this.gazeCue)
    this.expressionCue = null
    this.headBodyCue = null
    this.gazeCue = null
  }

  private ensure(
    current: MotionLeaseHandle | null,
    channels: PerformanceLeaseChannels,
    options: { nowMs: number; ttlMs?: number },
  ): MotionLeaseHandle | null {
    return (
      this.coordinator.renew(current, channels, options) ??
      this.coordinator.claim('performance', channels, options)
    )
  }

  private syncTimed(
    current: MotionLeaseHandle | null,
    channels: PerformanceLeaseChannels,
    untilMs: number | null,
    nowMs: number,
  ): MotionLeaseHandle | null {
    if (untilMs === null || untilMs <= nowMs) {
      this.coordinator.release(current)
      return null
    }
    return this.ensure(current, channels, {
      nowMs,
      ttlMs: untilMs - nowMs,
    })
  }
}

function maxTime(current: number | null, next: number): number {
  return current === null ? next : Math.max(current, next)
}
