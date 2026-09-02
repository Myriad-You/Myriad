/**
 * The whole Merope surface the agent engine is allowed to touch.
 *
 * The engine runs a turn: it does not own a mouth, a gate or a rig. Binding
 * `agentFace` and `faceSpeechGate` here keeps the two singletons out of the
 * engine's call sites, so a turn state machine never names a body part.
 */
import type { AgentPanelMode } from '../../components/agent-panel/agentPanelMode'
import type { Locale } from '../../i18n'
import type { MoodTransition, RigStateSummary } from '../../services/agent/types'
import type { PerceptionAdapter } from './body/types'
import type { FaceDelivery, FaceSpeechLine } from './faceSpeechArbitration'
import { agentFace } from './agentFaceChannel'
import { getLocalPerception, getProductionBody } from './body/host'
import {
  cancelGatedSpeech,
  deliverGatedLine,
  faceSpeechGate,
  openGatedReply,
  setLiveBody,
} from './faceSpeechArbitration'
import { livePresenceFacts } from './livePresence'
import { setLiveMotionGeneration } from './motion/liveGeneration'
import { captureProductionRigStateSummary } from './motion/runtimeHost'
import { notePresenceRoute, startPresenceInbound } from './perception/inbound'
import { getSpeechPipeline } from './speech/speechPipelineHost'
import { SpeechSegmenter } from './speech/speechSegmenter'

export { notePresenceRoute, startPresenceInbound }

/** Token-to-speech feed for one turn. The engine never names the splitter. */
export function openTurnSpeech(
  messageId: string,
  generation = 0,
  locale?: Locale,
) {
  const pipeline = getSpeechPipeline()
  void pipeline.probe()
  const segmenter = new SpeechSegmenter(messageId, generation, locale)
  return {
    cancel() {
      pipeline.cancel(messageId)
    },
    /** `null` = pipeline is off, caller should fall back to the live utterance. */
    push(token: string): number | null {
      if (!pipeline.available) return null
      const segments = segmenter.push(token)
      pipeline.feed(segments)
      return segments.length
    },
    end(): number {
      if (!pipeline.available) return 0
      const tail = segmenter.end()
      pipeline.feed(tail)
      return tail.length
    },
  }
}

/** Mount the live body for as long as the engine is mounted. */
export function attachLiveBody(): () => void {
  setLiveBody(getProductionBody())
  return () => setLiveBody(null)
}

/** One turn replaces the last: motion and face must agree on which. */
export function setTurnGeneration(generation: number): void {
  setLiveMotionGeneration(generation)
  agentFace.setGeneration(generation)
}

export function openTurnReply(
  mode: AgentPanelMode,
  messageId: string,
  locale?: string,
): ReturnType<typeof openGatedReply> {
  return openGatedReply(agentFace, faceSpeechGate, mode, messageId, locale)
}

export function deliverTurnLine(
  mode: AgentPanelMode,
  line: FaceSpeechLine,
): FaceDelivery {
  return deliverGatedLine(agentFace, faceSpeechGate, mode, line)
}

export function stopTurnSpeech(messageId: string): void {
  cancelGatedSpeech(agentFace, faceSpeechGate, messageId)
  getSpeechPipeline().cancel(messageId)
}

export function turnSpeechAlreadyFed(messageId: string): boolean {
  return getSpeechPipeline().alreadyFed(messageId)
}

export function setFaceMood(mood: MoodTransition, activity: string): void {
  agentFace.updateState({ mood, activity })
}

type PerceptionInput = Parameters<PerceptionAdapter['capture']>[0]

export interface TurnBodyContext {
  rigState: RigStateSummary
  perception: ReturnType<PerceptionAdapter['capture']>
  presence: ReturnType<typeof livePresenceFacts>
}

/** What the body can tell the model about right now. */
export function captureTurnBody(input: PerceptionInput): TurnBodyContext {
  return {
    rigState: captureProductionRigStateSummary(),
    perception: getLocalPerception().capture(input),
    presence: livePresenceFacts(),
  }
}
