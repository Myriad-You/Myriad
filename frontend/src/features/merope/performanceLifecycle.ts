import type { PerformanceDirective } from '../../services/agent/types'
import type { MeropePerformanceEventDetail } from './performanceEvents'
import type { MeropeSpeechEventDetail } from './speechEvents'
import { acceptLiveMotionGeneration } from './motion/liveGeneration'
import { markTurnTrace } from './turnTrace'

export interface PerformanceLifecycleTarget {
  applyPerformanceDirective: (
    performance: PerformanceDirective,
    event: MeropePerformanceEventDetail,
  ) => boolean
  clearPerformanceDirective: () => void
}

/** Forwards bounded semantic plans to the mounted rig; text stays speech-owned. */
export class PerformanceLifecycleController {
  private activeMessageId: string | null = null
  private activePlanKey: string | null = null
  private readonly acceptedPlanKeys = new Set<string>()
  private readonly acceptedPlans: string[][] = []
  private readonly cancelledMessageIds = new Set<string>()
  private readonly cancellationOrder: string[] = []

  constructor(private readonly target: PerformanceLifecycleTarget) {}

  handle(event: MeropePerformanceEventDetail): void {
    if (!event.performance) return
    if (!acceptLiveMotionGeneration(event.generation)) return
    if (event.messageId && this.cancelledMessageIds.has(event.messageId)) {
      this.noteDrop(event, 'cancelled')
      return
    }
    // The final HTTP response can omit the stream's run id, but still belongs
    // to the same UI message. Keep content replay protection stable across it.
    const scope = [
      event.generation ?? 0,
      event.source,
      event.messageId ?? event.runId ?? '',
    ]
    // The event boundary has already sanitized the directive into a fixed
    // shape. Cue count alone is not identity: refinements often keep it equal.
    const contentKey = JSON.stringify([...scope, 'content', event.performance])
    const intentKey = event.motionIntentId
      ? JSON.stringify([...scope, 'intent', event.motionIntentId])
      : null
    const planKey = intentKey ?? contentKey
    const rememberContent =
      event.messageId &&
      (event.source === 'reply' || event.source === 'proactive')
    const rememberedKeys = [
      ...(intentKey ? [intentKey] : []),
      ...(rememberContent ? [contentKey] : []),
    ]
    if (
      planKey === this.activePlanKey ||
      rememberedKeys.some((key) => this.acceptedPlanKeys.has(key))
    ) {
      this.noteDrop(event, 'duplicate')
      return
    }
    if (!this.target.applyPerformanceDirective(event.performance, event)) {
      this.noteDrop(event, 'rejected')
      return
    }
    this.activeMessageId = event.messageId ?? null
    this.activePlanKey = planKey
    // A final response or reconnect may assign a fresh transport intent id to
    // the same message's plan. Remember accepted content as well as that id.
    // Explicit preview/interaction intents may intentionally repeat a pose.
    if (rememberedKeys.length) {
      this.acceptedPlans.push(rememberedKeys)
      for (const key of rememberedKeys) this.acceptedPlanKeys.add(key)
      if (this.acceptedPlans.length > 128) {
        for (const key of this.acceptedPlans.shift() ?? [])
          this.acceptedPlanKeys.delete(key)
      }
    }
  }

  handleSpeech(event: MeropeSpeechEventDetail): void {
    // Text completion does not release the plan or its ownership: TTS and
    // body recovery may still be running. A later interruption must be able
    // to clear that message's landing, without touching a newer message.
    if (event.phase !== 'cancel') return
    this.rememberCancellation(event.messageId)
    if (event.messageId !== this.activeMessageId) return
    this.activeMessageId = null
    this.activePlanKey = null
    this.target.clearPerformanceDirective()
  }

  dispose(): void {
    this.activeMessageId = null
    this.activePlanKey = null
    this.acceptedPlanKeys.clear()
    this.acceptedPlans.length = 0
    this.cancelledMessageIds.clear()
    this.cancellationOrder.length = 0
    this.target.clearPerformanceDirective()
  }

  private rememberCancellation(messageId: string): void {
    if (this.cancelledMessageIds.has(messageId)) return
    this.cancelledMessageIds.add(messageId)
    this.cancellationOrder.push(messageId)
    if (this.cancellationOrder.length <= 32) return
    const expired = this.cancellationOrder.shift()
    if (expired) this.cancelledMessageIds.delete(expired)
  }

  private noteDrop(event: MeropePerformanceEventDetail, reason: string): void {
    if (event.source === 'preview') return
    markTurnTrace('performance_dropped', {
      reason,
      generation: event.generation ?? 0,
      ...(event.runId ? { runId: event.runId } : {}),
      ...(event.messageId ? { messageId: event.messageId } : {}),
    })
  }
}
