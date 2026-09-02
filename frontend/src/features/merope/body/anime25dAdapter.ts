import type { MotionRuntime } from '../motion/runtime'
import type {
  BodyAdapter,
  BodyCapabilities,
  BodyIntent,
  BodyState,
} from './types'
import { liveFaceVisible } from '../faceVisible'
import {
  liveMotionGeneration,
  newMotionIntentId,
} from '../motion/liveGeneration'
import { captureRigStateSummary } from '../motion/rigStateSummary'
import { getSpeechPipeline } from '../speech/speechPipelineHost'

/**
 * The only body adapter Myriad ships: Anime2.5D over the existing runtime.
 * Lite never sees drivers. A second body would have to prove this enough.
 */
export class Anime25DBodyAdapter implements BodyAdapter {
  constructor(private readonly runtime: MotionRuntime) {}

  capabilities(): BodyCapabilities {
    return { semantic: this.runtime.summaryFacts().capabilities }
  }

  state(): BodyState {
    const summary = captureRigStateSummary(this.runtime)
    return {
      expression: summary.expression,
      posture: summary.posture,
      acting: summary.acting.intent,
      speaking: summary.speaking,
      faceVisible: liveFaceVisible() && summary.faceVisible,
      capabilities: summary.capabilities,
    }
  }

  intend(intent: BodyIntent): void {
    if (!liveFaceVisible()) return
    if (intent.speechText && intent.messageId) {
      getSpeechPipeline().speakLine({
        messageId: intent.messageId,
        text: intent.speechText,
        generation: liveMotionGeneration(),
        interrupt: 'queue',
      })
    }
    if (intent.performance?.plan) {
      const generation = liveMotionGeneration()
      this.runtime.performance.handle({
        text: intent.speechText ?? '',
        source: 'reply',
        ...(intent.messageId ? { messageId: intent.messageId } : {}),
        ...(generation ? { generation } : {}),
        motionIntentId: newMotionIntentId(),
        performance: intent.performance,
      })
    }
  }
}
