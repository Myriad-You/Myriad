import type { PerformanceDirective } from '../../../services/agent/types'
import type { MeropePerformanceEventDetail } from '../performanceEvents'
import type { MeropeSpeechEventDetail } from '../speechEvents'
import type { RigBearing } from './bearing'
import type { BehaviorPlan, BehaviorSnapshot } from './behavior'
import type { RigMotionCoordinator } from './coordinator'
import type { PerformanceIntent } from './intents'
import {
  currentMeropeState,
  MEROPE_PERFORMANCE_EVENT,
  MEROPE_STATE_EVENT,
  meropePerformanceEventDetail,
  meropeStateEventDetail,
} from '../performanceEvents'
import { PerformanceLifecycleController } from '../performanceLifecycle'
import { MEROPE_SPEECH_EVENT, meropeSpeechEventDetail } from '../speechEvents'
import { markTurnTrace } from '../turnTrace'
import { bearingFromDirective } from './bearing'
import { newMotionIntentId } from './liveGeneration'
import {
  compilePerformanceBehaviorPlan,
  PERFORMANCE_BEHAVIOR_PLAN_ID,
} from './performanceBehaviorPlan'
import { PerformanceMotionLeases } from './performanceLeases'
import { HumanReactionPolicy } from './reactionPolicy'

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
  private preview = false

  constructor(
    coordinator: RigMotionCoordinator,
    private readonly onChange: (intent: PerformanceIntent) => void,
    private readonly externalBehaviors: () => readonly BehaviorSnapshot[] = () => [],
    private readonly onDirective: (
      directive: PerformanceDirective,
      event: MeropePerformanceEventDetail | undefined,
      plan: BehaviorPlan | null,
    ) => void = () => {},
  ) {
    this.leases = new PerformanceMotionLeases(coordinator)
  }

  current(_nowMs: number = currentNow()): PerformanceIntent {
    return { ...this.intent, behaviors: this.externalBehaviors() }
  }

  currentBearing(): RigBearing | null {
    return this.bearing
  }

  clearBearing(): void {
    this.bearing = null
  }

  start(): void {
    if (this.listening) return
    this.controller = new PerformanceLifecycleController({
      applyPerformanceDirective: (performance, event) =>
        this.publish(performance, event),
      clearPerformanceDirective: () => this.cancel(),
    })
    if (typeof window !== 'undefined') {
      window.addEventListener(MEROPE_PERFORMANCE_EVENT, this.onPerformance)
      window.addEventListener(MEROPE_SPEECH_EVENT, this.onSpeech)
      window.addEventListener(MEROPE_STATE_EVENT, this.onState)
    }
    this.listening = true
  }

  stop(): void {
    if (!this.listening) return
    if (typeof window !== 'undefined') {
      window.removeEventListener(MEROPE_PERFORMANCE_EVENT, this.onPerformance)
      window.removeEventListener(MEROPE_SPEECH_EVENT, this.onSpeech)
      window.removeEventListener(MEROPE_STATE_EVENT, this.onState)
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

  handleForTest(
    performance: PerformanceDirective,
    event?: MeropePerformanceEventDetail,
  ): boolean {
    return this.publish(performance, event)
  }

  private publish(
    performance: PerformanceDirective,
    event?: MeropePerformanceEventDetail,
  ): boolean {
    this.preview = event?.source === 'preview'
    const scope =
      event && (event.runId || event.messageId)
        ? JSON.stringify([
            event.source,
            event.generation ?? 0,
            event.runId ?? event.messageId,
          ])
        : undefined
    const startedAtMs = currentNow()
    // Keep that reply's gesture, but never reinstall its outdated standing face.
    if (!this.preview) {
      performance = performanceAtMoodRevision(
        performance,
        currentMeropeState()?.mood.revision ?? 0,
      )
    }
    if (
      !performance.plan.baseline &&
      performance.plan.cues.length === 0 &&
      !performance.phrases?.length
    ) {
      return false
    }
    this.bearing = bearingFromDirective(performance) ?? this.bearing
    const selection = this.reactionPolicy.select(
      performance,
      this.externalBehaviors(),
      startedAtMs,
      scope,
      event?.generation,
    )
    const selected = selection.directive
    if (!this.preview) {
      for (const decision of selection.decisions) {
        markTurnTrace('performance_decision', {
          intent: decision.intent,
          reason: decision.reason,
          atMs: decision.atMs,
          phase: performance.phase,
          generation: event?.generation ?? 0,
          ...(event?.runId ? { runId: event.runId } : {}),
          ...(event?.messageId ? { messageId: event.messageId } : {}),
        })
      }
    }
    if (selected.plan.cues.length === 0) {
      this.intent = { ...this.intent, directive: selected }
      this.onDirective(selected, event, null)
      this.onChange(this.intent)
      return true
    }
    const motionIntentId = newMotionIntentId()
    const behaviorPlan = compilePerformanceBehaviorPlan(
      selected,
      startedAtMs,
      PERFORMANCE_BEHAVIOR_PLAN_ID,
      scope,
      event?.generation,
    )
    const windows = this.leases.apply(selected, startedAtMs, behaviorPlan)
    this.intent = {
      directive: selected,
      startedAtMs,
      motionIntentId,
      generation: event?.generation,
      behaviorPlan,
      behaviors: [],
    }
    this.armSettle(windows.planUntilMs - startedAtMs)
    this.onDirective(selected, event, behaviorPlan)
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

  private readonly onState = (event: Event): void => {
    if (this.preview) return
    const detail = meropeStateEventDetail(
      (event as CustomEvent<unknown>).detail,
    )
    if (
      !detail ||
      !this.bearing ||
      this.bearing.revision >= detail.mood.revision
    ) {
      return
    }
    this.clearBearing()
    this.onChange(this.intent)
  }

  private readonly onSpeech = (event: Event): void => {
    const detail = meropeSpeechEventDetail(
      (event as CustomEvent<unknown>).detail,
    )
    if (detail) this.handleSpeech(detail)
  }
}

export function performanceAtMoodRevision(
  performance: PerformanceDirective,
  revision: number,
): PerformanceDirective {
  if (performance.moodRevision >= revision || !performance.plan.baseline)
    return performance
  return { ...performance, plan: { cues: performance.plan.cues } }
}

function currentNow(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now()
}
