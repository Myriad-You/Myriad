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

/** The only body adapter Myriad ships */
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
    if (intent.touchContinuation && !intent.speechRefinement && source === 'proactive'
      && intent.messageId && intent.speechText) {
      this.runtime.touch.accompanySpeech(intent.messageId, performance.now())
    }
    if (
      intent.speechRefinement &&
      (!intent.messageId ||
        !this.runtime.speech.hasPlayback({
          messageId: intent.messageId,
          source,
          generation,
        }))
    ) {
      return
}
    if (!intent.speechRefinement && intent.speechText && intent.messageId) {
      getSpeechPipeline().speakLine({
        messageId: intent.messageId,
        text: intent.speechText,
        generation,
        source,
        interrupt: 'queue',
      })
    }
    if (intent.performance?.plan) {
      if (intent.speechRefinement && source === 'proactive' && intent.messageId
        && !this.runtime.touch.acceptsSpeechRefinement(intent.messageId, intent.performance, performance.now())) { return
}
      this.runtime.performance.handle({
        text: intent.speechRefinement
          ? this.runtime.speech.upcomingText({
              messageId: intent.messageId!,
              source,
              generation,
            })
          : (intent.speechText ?? ''),
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
