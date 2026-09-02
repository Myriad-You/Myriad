import type { PerformanceDirective } from '../../../services/agent/types'
import type { MeropePerformanceEventDetail } from '../performanceEvents'
import type { MeropeSpeechEventDetail } from '../speechEvents'
import type { RigBearing } from './bearing'
import type { BehaviorSnapshot } from './behavior'
import type { RigMotionCoordinator } from './coordinator'
import type { PerformanceIntent } from './intents'
import {
  MEROPE_PERFORMANCE_EVENT,
  meropePerformanceEventDetail,
} from '../performanceEvents'
import { PerformanceLifecycleController } from '../performanceLifecycle'
import { MEROPE_SPEECH_EVENT, meropeSpeechEventDetail } from '../speechEvents'
import { bearingFromDirective } from './bearing'
import { liveMotionGeneration, newMotionIntentId } from './liveGeneration'
import {
  compilePerformanceBehaviorPlan,
  PERFORMANCE_BEHAVIOR_PLAN_ID,
} from './performanceBehaviorPlan'
import { PerformanceMotionLeases } from './performanceLeases'
import { HumanReactionPolicy } from './reactionPolicy'

/**
 * One Lite performance producer for a coordinator. Publishes the directive
 * and timed leases; never writes a rig.
 */
export class PerformanceMotionSource {
  private readonly leases: PerformanceMotionLeases
  private readonly reactionPolicy = new HumanReactionPolicy()
  private controller: PerformanceLifecycleController | null = null
  private settleTimer: ReturnType<typeof setTimeout> | null = null
  private bearing: RigBearing | null = null
  private intent: PerformanceIntent = {
    directive: null,
    startedAtMs: 0,
    motionIntentId: null,
    behaviorPlan: null,
    behaviors: [],
  }

  private listening = false

  constructor(
    coordinator: RigMotionCoordinator,
    private readonly onChange: (intent: PerformanceIntent) => void,
    private readonly externalBehaviors: () => readonly BehaviorSnapshot[] = () => [],
  ) {
    this.leases = new PerformanceMotionLeases(coordinator)
  }

  current(_nowMs: number = currentNow()): PerformanceIntent {
    return { ...this.intent, behaviors: this.externalBehaviors() }
  }

  currentBearing(): RigBearing | null {
    return this.bearing
  }

  /** Mood-band standing face takes over; a later round may revise it. */
  clearBearing(): void {
    this.bearing = null
  }

  start(): void {
    if (this.listening) return
    this.controller = new PerformanceLifecycleController({
      applyPerformanceDirective: (performance) => this.publish(performance),
      clearPerformanceDirective: () => this.cancel(),
    })
    if (typeof window !== 'undefined') {
      window.addEventListener(MEROPE_PERFORMANCE_EVENT, this.onPerformance)
      window.addEventListener(MEROPE_SPEECH_EVENT, this.onSpeech)
    }
    this.listening = true
  }

  stop(): void {
    if (!this.listening) return
    if (typeof window !== 'undefined') {
      window.removeEventListener(MEROPE_PERFORMANCE_EVENT, this.onPerformance)
      window.removeEventListener(MEROPE_SPEECH_EVENT, this.onSpeech)
    }
    this.controller?.dispose()
    this.controller = null
    this.listening = false
  }

  handle(detail: MeropePerformanceEventDetail): void {
    this.controller?.handle(detail)
  }

  handleSpeech(detail: MeropeSpeechEventDetail): void {
    this.controller?.handleSpeech(detail)
  }

  handleForTest(performance: PerformanceDirective): boolean {
    return this.publish(performance)
  }

  private publish(performance: PerformanceDirective): boolean {
    const startedAtMs = currentNow()
    this.bearing = bearingFromDirective(performance) ?? this.bearing
    const selected = this.reactionPolicy.select(
      performance,
      this.externalBehaviors(),
      startedAtMs,
    ).directive
    if (selected.plan.cues.length === 0) {
      this.intent = { ...this.intent, directive: selected }
      this.onChange(this.intent)
      return true
    }
    const motionIntentId = newMotionIntentId()
    // The intent id identifies this publish for lifecycle and telemetry; the
    // plan is keyed by the producer so the scheduler can match beats across
    // publishes.
    const behaviorPlan = compilePerformanceBehaviorPlan(
      selected,
      startedAtMs,
      PERFORMANCE_BEHAVIOR_PLAN_ID,
    )
    const windows = this.leases.apply(selected, startedAtMs, behaviorPlan)
    this.intent = {
      directive: selected,
      startedAtMs,
      motionIntentId,
      generation: liveMotionGeneration() || undefined,
      behaviorPlan,
      behaviors: [],
    }
    this.armSettle(windows.planUntilMs - startedAtMs)
    this.onChange(this.intent)
    return true
  }

  private clearTransient(): void {
    this.clearSettle()
    this.leases.releaseAll()
    this.intent = {
      directive: null,
      startedAtMs: 0,
      motionIntentId: null,
      behaviorPlan: null,
      behaviors: [],
    }
    this.onChange(this.intent)
  }

  private cancel(): void {
    this.bearing = null
    this.clearTransient()
  }

  private armSettle(delayMs: number): void {
    this.clearSettle()
    const wait = Math.max(1, delayMs)
    const timer = setTimeout(() => {
      this.settleTimer = null
      this.clearTransient()
    }, wait)
    this.settleTimer = timer
    if (typeof timer === 'object' && 'unref' in timer) timer.unref()
  }

  private clearSettle(): void {
    if (this.settleTimer == null) return
    clearTimeout(this.settleTimer)
    this.settleTimer = null
  }

  private readonly onPerformance = (event: Event): void => {
    const detail = meropePerformanceEventDetail(
      (event as CustomEvent<unknown>).detail,
    )
    if (detail) this.handle(detail)
  }

  private readonly onSpeech = (event: Event): void => {
    const detail = meropeSpeechEventDetail(
      (event as CustomEvent<unknown>).detail,
    )
    if (detail) this.handleSpeech(detail)
  }
}

function currentNow(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now()
}
