import type { AgentPanelMode } from '../../../components/agent-panel/agentPanelMode'
import type { Locale } from '../../../i18n'
import type {
  MoodTransition,
  RigStateSummary,
} from '../../../services/agent/types'
import type { PerceptionAdapter } from '../body/types'
import type { SpeechSegment } from '../speech/speechSegmenter'
import type { FaceDelivery, FaceSpeechLine } from './faceSpeechArbitration'
import { openVoiceStream } from '../../../services/speechApi'
import { authSubject } from '../../../utils/authSubject'
import { getLocalPerception, getProductionBody } from '../body/host'
import { setLiveMotionGeneration } from '../motion/liveGeneration'
import { captureProductionRigStateSummary } from '../motion/runtimeHost'
import { notePresenceRoute, startPresenceInbound } from '../perception/inbound'
import { getSpeechPipeline } from '../speech/speechPipelineHost'
import { SpeechSegmenter } from '../speech/speechSegmenter'
import { agentFace } from './agentFaceChannel'
import {
  cancelGatedSpeech,
  deliverGatedLine,
  faceSpeechGate,
  openGatedReply,
  setLiveBody,
  silentReplyUtterance,
} from './faceSpeechArbitration'
import { livePresenceFacts } from './livePresence'

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
  if (pipeline.ownVoiceAvailable) {
    const own = openOwnVoice(messageId, generation, segmenter)
    if (own) return own
  }
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

/** What was said aloud, kept for her to hear (Omni mode), with its words. */
let heardVoice: { token: string; text: string } | undefined

export function noteHeardVoice(token: string, text: string): void {
  heardVoice = { token, text: text.trim() }
}

/**
 * The recording a turn was said in, to send with it: only for the words it
 * was written down as, so a later typed message never carries an old
 * recording. Taken once.
 */
export function takeHeardVoice(text: string): string | undefined {
  const heard = heardVoice
  heardVoice = undefined
  return heard && heard.text === text.trim() ? heard.token : undefined
}

/** Tokens of the turns that asked for her own voice, by reply. */
const turnVoiceTokens = new Map<string, string>()

/** The token this reply's voice comes under, to send with the turn (once). */
export function takeTurnVoiceToken(messageId: string): string | undefined {
  const token = turnVoiceTokens.get(messageId)
  turnVoiceTokens.delete(messageId)
  return token
}

/**
 * Her own voice for this reply: the turn says it and the sound comes on a
 * stream of its own. Her words are cut into sentences as usual but kept;
 * once the sound starts they are let go, and if it never comes (the turn
 * did not speak) they are read aloud after all.
 */
function openOwnVoice(
  messageId: string,
  generation: number,
  segmenter: SpeechSegmenter,
) {
  const pipeline = getSpeechPipeline()
  const stream = pipeline.openStream(messageId, generation, 'reply')
  if (!stream) return null
  const token = crypto.randomUUID()
  turnVoiceTokens.set(messageId, token)
  if (turnVoiceTokens.size > 64)
    turnVoiceTokens.delete(turnVoiceTokens.keys().next().value!)
  const subject = authSubject.signal
  const abort = new AbortController()
  let heard = false
  let readInstead = false
  const kept: SpeechSegment[] = []
  const keep = (segments: SpeechSegment[]): number => {
    if (readInstead) pipeline.feed(segments)
    else if (!heard) kept.push(...segments)
    return segments.length
  }
  void openVoiceStream(token, abort.signal, (bytes) => {
    if (subject.aborted) return
    heard = true
    kept.length = 0
    stream.push(bytes)
  })
    .catch(() => 0)
    .then(() => {
      if (subject.aborted || abort.signal.aborted) return
      stream.end()
      if (heard) return
      readInstead = true
      pipeline.feed(kept.splice(0))
    })
  return {
    cancel() {
      abort.abort()
      turnVoiceTokens.delete(messageId)
      if (subject.aborted) return
      pipeline.cancel(messageId)
    },
    push(token: string): number | null {
      if (subject.aborted) return 0
      return keep(segmenter.push(token))
    },
    end(): number {
      if (subject.aborted) return 0
      return keep(segmenter.end())
    },
  }
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
