import type { PerformanceDirective } from '../../services/agent/types'
import type { MeropePerformanceEventDetail } from './performanceEvents'
import type { MeropeSpeechEventDetail } from './speechEvents'
import { acceptLiveMotionGeneration } from './motion/liveGeneration'

export interface PerformanceLifecycleTarget {
  applyPerformanceDirective: (performance: PerformanceDirective) => boolean
  clearPerformanceDirective: () => void
}

/** Forwards bounded semantic plans to the mounted rig; text stays speech-owned. */
export class PerformanceLifecycleController {
  private activeMessageId: string | null = null
  private activePlanKey: string | null = null
  private readonly cancelledMessageIds = new Set<string>()
  private readonly cancellationOrder: string[] = []

  constructor(private readonly target: PerformanceLifecycleTarget) {}

  handle(event: MeropePerformanceEventDetail): void {
    if (!event.performance) return
    if (!acceptLiveMotionGeneration(event.generation)) return
    if (event.messageId && this.cancelledMessageIds.has(event.messageId)) return
    const planKey =
      event.motionIntentId ||
      `${event.messageId ?? ''}:${event.performance.phase}:${event.performance.plan.cues.length}`
    if (planKey && planKey === this.activePlanKey) return
    if (!this.target.applyPerformanceDirective(event.performance)) return
    this.activeMessageId = event.messageId ?? null
    this.activePlanKey = planKey
  }

  handleSpeech(event: MeropeSpeechEventDetail): void {
    if (event.phase !== 'cancel' && event.phase !== 'end') return
    if (event.phase === 'cancel') this.rememberCancellation(event.messageId)
    if (event.messageId !== this.activeMessageId) return
    this.activeMessageId = null
    this.activePlanKey = null
    // A finished utterance keeps the landing baseline; only cancel dumps it.
    if (event.phase === 'cancel') this.target.clearPerformanceDirective()
  }

  dispose(): void {
    this.activeMessageId = null
    this.activePlanKey = null
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
}
