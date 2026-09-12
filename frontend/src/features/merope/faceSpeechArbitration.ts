import type { AgentPanelMode } from '../../components/agent-panel/agentPanelMode'
import type { AgentFaceChannel, ReplyUtterance } from './agentFaceChannel'
import type { BodyAdapter } from './body/types'
import { getAgentPanelMode } from '../../components/agent-panel/agentPanelMode'
import { authSubject } from '../../utils/authSubject'
import { liveFaceVisible } from './faceVisible'
import { liveMotionGeneration } from './motion/liveGeneration'
import { sanitizePerformanceDirective } from './performanceEvents'
import { getSpeechPipeline } from './speech/speechPipelineHost'
import { noteTurnTraceDrop } from './turnTrace'

export type FaceSpeechVerdict = 'speak' | 'record-without-speech'

export interface FaceSpeechLine {
  touchContinuation?: boolean
  runId?: string
  messageId: string
  text?: string
  source?: 'reply' | 'proactive' | 'interaction' | 'preview'
  locale?: string
  performance?: unknown
}

export interface FaceDelivery {
  surface: 'speech' | 'record'
  messageId: string
  text?: string
}

/** Only the currently visible mode speaks immediately. */
export function arbitrateFaceSpeech(input: {
  visibleMode: AgentPanelMode
  incomingMode: AgentPanelMode
  chatUtteranceActive: boolean
}): FaceSpeechVerdict {
  if (input.incomingMode === 'work' && input.chatUtteranceActive) {
    return 'record-without-speech'
  }
  if (input.incomingMode !== input.visibleMode) {
    return 'record-without-speech'
  }
  return 'speak'
}

/** A ReplyUtterance stand-in that never touches the face protocol. */
export function silentReplyUtterance(): Pick<
  ReplyUtterance,
  'chunk' | 'end' | 'cancel'
> {
  return {
    chunk() {},
    end() {},
    cancel() {},
  }
}

/** Tracks whether Chat currently owns an open utterance so a finishing Work turn cannot barge in. */
export class FaceSpeechGate {
  chatUtteranceActive = false
  private chatMessageId: string | null = null

  constructor(
    private readonly visibleMode: () => AgentPanelMode = getAgentPanelMode,
  ) {}

  get chatBusy(): boolean {
    return (
      this.chatUtteranceActive ||
      (this.chatMessageId != null &&
        getSpeechPipeline().isBusyWith(this.chatMessageId))
    )
  }

  decide(incomingMode: AgentPanelMode): FaceSpeechVerdict {
    return arbitrateFaceSpeech({
      visibleMode: this.visibleMode(),
      incomingMode,
      chatUtteranceActive: this.chatBusy,
    })
  }

  beginIncoming(
    incomingMode: AgentPanelMode,
    messageId?: string,
  ): FaceSpeechVerdict {
    const verdict = this.decide(incomingMode)
    if (verdict === 'speak' && incomingMode === 'chat') {
      this.chatUtteranceActive = true
      this.chatMessageId = messageId ?? this.chatMessageId
    }
    return verdict
  }

  endIncoming(incomingMode: AgentPanelMode, messageId?: string): void {
    if (incomingMode !== 'chat') return
    if (messageId && this.chatMessageId && messageId !== this.chatMessageId) return
    this.chatUtteranceActive = false
    if (
      this.chatMessageId &&
      getSpeechPipeline().isBusyWith(this.chatMessageId)
    ) {
      return
    }
    this.releaseChat(messageId)
  }

  releaseChat(messageId?: string): void {
    if (messageId && this.chatMessageId && messageId !== this.chatMessageId) {
      return
    }
    this.chatUtteranceActive = false
    this.chatMessageId = null
  }

  cancelSpeech(channel: AgentFaceChannel, messageId: string): void {
    channel.cancel(messageId)
    this.releaseChat(messageId)
  }
}

export const faceSpeechGate = new FaceSpeechGate()
authSubject.subscribe(() => faceSpeechGate.releaseChat())

let liveBody: BodyAdapter | null = null

export function setLiveBody(body: BodyAdapter | null): void {
  liveBody = body
}

/** Open a streamed reply only when the gate allows speech. */
export function openGatedReply(
  channel: AgentFaceChannel,
  gate: FaceSpeechGate,
  incomingMode: AgentPanelMode,
  messageId: string,
  locale?: string,
): Pick<ReplyUtterance, 'chunk' | 'end' | 'cancel'> {
  const subject = authSubject.signal
  const verdict = gate.beginIncoming(incomingMode, messageId)
  const inner =
    verdict === 'speak'
      ? channel.openReply(messageId, locale)
      : silentReplyUtterance()
  return {
    chunk: (token: string) => { if (!subject.aborted) inner.chunk(token) },
    end: () => {
      if (subject.aborted) return
      inner.end()
      gate.endIncoming(incomingMode, messageId)
    },
    cancel: () => {
      if (subject.aborted) return
      inner.cancel()
      gate.endIncoming(incomingMode, messageId)
    },
  }
}

export function cancelGatedSpeech(
  channel: AgentFaceChannel,
  gate: FaceSpeechGate,
  messageId: string,
): void {
  gate.cancelSpeech(channel, messageId)
}

export function notificationCarriesMeropeSpeech(
  metadata: Record<string, unknown> | null | undefined,
): metadata is Record<string, unknown> {
  if (!metadata) return false
  if (
    metadata.performance != null ||
    metadata.merope_state != null ||
    metadata.intention_id != null
  ) {
    return true
  }
  return (
    typeof metadata.event_key === 'string' &&
    metadata.event_key.startsWith('agent.merope.')
  )
}

export function deliverWorkNotificationFace(
  channel: AgentFaceChannel,
  gate: FaceSpeechGate,
  notification: {
    id: string
    body?: string
    performance?: unknown
    meropeState?: unknown
  },
): FaceDelivery {
  if (notification.meropeState != null) {
    channel.updateState(notification.meropeState)
  }
  return deliverGatedLine(channel, gate, 'work', {
    messageId: notification.id,
    text: notification.body,
    source: 'proactive',
    performance: notification.performance,
  })
}

export function deliverProactiveFace(
  channel: AgentFaceChannel,
  gate: FaceSpeechGate,
  notification: {
    id: string
    eventKey?: string
    body?: string
    performance?: unknown
    meropeState?: unknown
  },
): FaceDelivery {
  const text = notification.body?.trim() ? notification.body : undefined
  if (
    gate.chatBusy ||
    (notification.eventKey === 'agent.merope.touch' &&
      (liveBody?.state().speaking ||
        !liveFaceVisible() ||
        (typeof document !== 'undefined' && document.hidden)))
  ) {
    noteTurnTraceDrop('gated_record')
    return { surface: 'record', messageId: notification.id, text }
  }
  if (notification.meropeState != null) {
    channel.updateState(notification.meropeState)
  }
  return deliverGatedLine(channel, gate, getAgentPanelMode(), {
    touchContinuation: notification.eventKey === 'agent.merope.touch',
    messageId: notification.id,
    text: notification.body,
    source: 'proactive',
    performance: notification.performance,
  })
}

/** Motion-only update */
export function refineProactiveFace(
  gate: FaceSpeechGate,
  id: string,
  raw: unknown,
): void {
  if (
    gate.chatBusy ||
    !liveBody ||
    !liveFaceVisible() ||
    (typeof document !== 'undefined' && document.hidden)
  ) {
    return
}
  const performance = sanitizePerformanceDirective(raw)
  if (!performance) return
  liveBody.intend({
    messageId: id,
    source: 'proactive',
    performance,
    speechRefinement: true,
  })
}

export function deliverGatedLine(
  channel: AgentFaceChannel,
  gate: FaceSpeechGate,
  incomingMode: AgentPanelMode,
  line: FaceSpeechLine,
): FaceDelivery {
  const text = line.text?.trim() ? line.text : undefined
  if (gate.decide(incomingMode) !== 'speak') {
    noteTurnTraceDrop('gated_record')
    return { surface: 'record', messageId: line.messageId, text }
  }
  if (!liveFaceVisible()) {
    noteTurnTraceDrop('hidden_face')
    return { surface: 'record', messageId: line.messageId, text }
  }
  const performance =
    sanitizePerformanceDirective(line.performance) ?? undefined
  if (liveBody) {
    liveBody.intend({
      messageId: line.messageId,
      ...(line.runId ? { runId: line.runId } : {}),
      ...(line.source ? { source: line.source } : {}),
      speechText: text,
      ...(line.touchContinuation ? { touchContinuation: true } : {}),
      ...(performance ? { performance } : {}),
    })
    if (text && getSpeechPipeline().available) {
      return { surface: 'speech', messageId: line.messageId, text }
    }
    if (text) {
      channel.deliver({ ...line, performance: undefined })
      return { surface: 'speech', messageId: line.messageId, text }
    }
    return { surface: 'speech', messageId: line.messageId }
  }
  if (text && speakUnmountedLine(line.messageId, text, line.source)) {
    if (line.performance) {
      channel.deliver({ ...line, text: undefined })
    }
    return { surface: 'speech', messageId: line.messageId, text }
  }
  channel.deliver(line)
  return { surface: 'speech', messageId: line.messageId, text }
}

/** This is the only app-layer speakLine outside Anime25DBodyAdapter.intend. */
function speakUnmountedLine(
  messageId: string,
  text: string,
  source: FaceSpeechLine['source'] = 'reply',
): boolean {
  return getSpeechPipeline().speakLine({
    messageId,
    text,
    generation: source === 'reply' ? liveMotionGeneration() : 0,
    source,
    interrupt: 'queue',
  })
}
