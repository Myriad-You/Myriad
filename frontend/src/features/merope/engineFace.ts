import type { AgentPanelMode } from '../../components/agent-panel/agentPanelMode'
import type { Locale } from '../../i18n'
import type {
  MoodTransition,
  RigStateSummary,
} from '../../services/agent/types'
import type { PerceptionAdapter } from './body/types'
import type { FaceDelivery, FaceSpeechLine } from './faceSpeechArbitration'
import { authSubject } from '../../utils/authSubject'
import { agentFace } from './agentFaceChannel'
import { getLocalPerception, getProductionBody } from './body/host'
import {
  cancelGatedSpeech,
  deliverGatedLine,
  faceSpeechGate,
  openGatedReply,
  setLiveBody,
  silentReplyUtterance,
} from './faceSpeechArbitration'
import { livePresenceFacts } from './livePresence'
import { setLiveMotionGeneration } from './motion/liveGeneration'
import { captureProductionRigStateSummary } from './motion/runtimeHost'
import { notePresenceRoute, startPresenceInbound } from './perception/inbound'
import { getSpeechPipeline } from './speech/speechPipelineHost'
import { SpeechSegmenter } from './speech/speechSegmenter'

export { notePresenceRoute, startPresenceInbound }

/** The engine never names the splitter. */
export function openTurnSpeech(
  mode: AgentPanelMode,
  messageId: string,
  generation = 0,
  locale?: Locale,
  output: 'local' | 'external' = 'local',
) {
  // Admission is shared with the text-mouth outlet. An admitted utterance may
  // finish across a panel switch; a background reply is never replayed later.
  if (output === 'external' || faceSpeechGate.decide(mode) !== 'speak') {
    return {
      cancel() {},
      push: (_token: string): number | null => 0,
      end: () => 0,
    }
  }
  const subject = authSubject.signal
  const pipeline = getSpeechPipeline()
  void pipeline.probe()
  const segmenter = new SpeechSegmenter(messageId, generation, locale)
  return {
    cancel() {
      if (subject.aborted) return
      pipeline.cancel(messageId)
    },
    /** `null` = pipeline is off, caller should fall back to the live utterance. */
    push(token: string): number | null {
      if (subject.aborted) return 0
      if (!pipeline.available) return null
      const segments = segmenter.push(token)
      pipeline.feed(segments)
      return segments.length
    },
    end(): number {
      if (subject.aborted) return 0
      if (!pipeline.available) return 0
      const tail = segmenter.end()
      pipeline.feed(tail)
      return tail.length
    },
  }
}

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
  output: 'local' | 'external' = 'local',
): ReturnType<typeof openGatedReply> {
  if (output === 'external') return silentReplyUtterance()
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

export function captureTurnBody(input: PerceptionInput): TurnBodyContext {
  return {
    rigState: captureProductionRigStateSummary(),
    perception: getLocalPerception().capture(input),
    presence: livePresenceFacts(),
  }
}
