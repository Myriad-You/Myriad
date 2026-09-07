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
    const source = intent.source ?? 'reply'
    const generation = source === 'reply' ? liveMotionGeneration() : 0
    if (intent.speechText && intent.messageId) {
      getSpeechPipeline().speakLine({
        messageId: intent.messageId,
        text: intent.speechText,
        generation,
        source,
        interrupt: 'queue',
      })
    }
    if (intent.performance?.plan) {
      this.runtime.performance.handle({
        text: intent.speechText ?? '',
        source,
        ...(intent.runId ? { runId: intent.runId } : {}),
        ...(intent.messageId ? { messageId: intent.messageId } : {}),
        ...(generation ? { generation } : {}),
        motionIntentId: newMotionIntentId(),
        performance: intent.performance,
      })
    }
  }
}
