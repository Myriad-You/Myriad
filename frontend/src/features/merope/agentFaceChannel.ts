import type {
  MeropeSpeechEventDetail,
  MeropeSpeechSource,
  SpeechUtteranceInput,
} from './speechEvents'
import { authSubject } from '../../utils/authSubject'
import { newMotionIntentId } from './motion/liveGeneration'
import {
  dispatchMeropePerformance,
  dispatchMeropeState,
} from './performanceEvents'
import {
  dispatchMeropeSpeech,
  dispatchMeropeSpeechUtterance,
} from './speechEvents'

/** 超出按最早丢。 */
const MAX_REMEMBERED_MESSAGES = 200

export interface AgentFaceSink {
  speech: (detail: MeropeSpeechEventDetail) => void
  utterance: (input: SpeechUtteranceInput) => void
  performance: (detail: unknown) => void
  state: (detail: unknown) => void
}

const windowSink: AgentFaceSink = {
  speech: dispatchMeropeSpeech,
  utterance: dispatchMeropeSpeechUtterance,
  performance: dispatchMeropePerformance,
  state: dispatchMeropeState,
}

/** 比对前规范化空白。 */
function normalizeSpokenText(text: string): string {
  return text.trim().replaceAll(/\s+/gu, ' ').slice(0, 2_000)
}

interface ReplyUtteranceContext {
  messageId: string
  locale?: string
  nextUtteranceId: () => string
  emit: (detail: MeropeSpeechEventDetail) => void
  remember: (text: string) => void
}

/** 首个非空 token 才开口；end/cancel 幂等；可再开口并换 utterance id。 */
export class ReplyUtterance {
  private utteranceId: string | null = null
  private text = ''

  constructor(private readonly context: ReplyUtteranceContext) {}

  /** 空白 token 只在已开口时并进文本。 */
  chunk(token: string): void {
    if (!token.trim()) {
      if (this.utteranceId) this.text += token
      return
    }
    this.open()
    this.text += token
    this.context.emit({
      phase: 'chunk',
      messageId: this.context.messageId,
      source: 'reply',
      utteranceId: this.utteranceId as string,
      text: token,
      ...(this.context.locale ? { locale: this.context.locale } : {}),
    })
  }

  /** end 记账，挡住整句兜底。 */
  end(): void {
    this.close('end')
  }

  /** cancel 不记账。 */
  cancel(): void {
    this.close('cancel')
  }

  private open(): void {
    if (this.utteranceId) return
    this.utteranceId = this.context.nextUtteranceId()
    this.text = ''
    this.context.emit({
      phase: 'start',
      messageId: this.context.messageId,
      source: 'reply',
      utteranceId: this.utteranceId,
      ...(this.context.locale ? { locale: this.context.locale } : {}),
    })
  }

  private close(phase: 'end' | 'cancel'): void {
    if (!this.utteranceId) return
    this.context.emit({
      phase,
      messageId: this.context.messageId,
      source: 'reply',
      utteranceId: this.utteranceId,
      ...(this.context.locale ? { locale: this.context.locale } : {}),
    })
    if (phase === 'end') {
      const spoken = normalizeSpokenText(this.text)
      if (spoken) this.context.remember(spoken)
    }
    this.utteranceId = null
    this.text = ''
  }
}

/** 全站唯一形象事件出口；改载荷即改契约。 */
export class AgentFaceChannel {
  private subjectEpoch = 0
  private readonly openMessages = new Set<string>()
  private sequence = 0
  private generation = 0
  private readonly spoken = new Map<string, string>()

  constructor(private readonly sink: AgentFaceSink = windowSink) {}

  setGeneration(generation: number): void {
    this.generation = Math.max(0, Math.trunc(generation))
  }

  openReply(messageId: string, locale?: string): ReplyUtterance {
    const epoch = this.subjectEpoch
    return new ReplyUtterance({
      messageId,
      locale,
      nextUtteranceId: () => this.nextUtteranceId('stream', messageId),
      emit: (detail) => {
        if (epoch !== this.subjectEpoch) return
        if (detail.phase === 'start') this.openMessages.add(messageId)
        if (detail.phase === 'end' || detail.phase === 'cancel') this.openMessages.delete(messageId)
        this.sink.speech(this.withGeneration(detail, 'reply'))
      },
      remember: (text) => {
        if (epoch === this.subjectEpoch) this.remember(messageId, text)
      },
    })
  }

  resetSubject(): void {
    this.subjectEpoch += 1
    for (const id of this.openMessages.union(new Set(this.spoken.keys())))
      this.cancel(id)
    this.openMessages.clear()
    this.spoken.clear()
  }

  /** 先表演后整句；都缺才跳过。按 messageId 去重；回复与通知 id 空间分开。 */
  deliver(line: {
    messageId: string
    runId?: string
    text?: string
    source?: MeropeSpeechSource
    locale?: string
    /** 未校验，转交 `performanceEvents`。 */
    performance?: unknown
  }): void {
    const source = line.source ?? 'reply'
    const text = line.text?.trim() ? line.text : ''
    if (text || line.performance) {
      this.sink.performance(
        this.withGeneration(
          {
            text,
            source,
            messageId: line.messageId,
            ...(line.runId ? { runId: line.runId } : {}),
            ...(line.performance
              ? {
                  performance: line.performance,
                  motionIntentId: newMotionIntentId(),
                }
              : {}),
          },
          source,
        ),
      )
    }
    if (!text) return
    const spoken = normalizeSpokenText(text)
    if (!spoken || this.spoken.get(line.messageId) === spoken) return
    this.remember(line.messageId, spoken)
    this.sink.utterance(
      this.withGeneration(
        {
          messageId: line.messageId,
          source,
          text: spoken,
          utteranceId: this.nextUtteranceId(source, line.messageId),
          ...(line.locale ? { locale: line.locale } : {}),
        },
        source,
      ),
    )
  }

  updateState(state: unknown): void {
    this.sink.state(state)
  }

  cancel(messageId: string): void {
    this.openMessages.delete(messageId)
    this.sink.speech({ phase: 'cancel', messageId, source: 'reply' })
  }

  private withGeneration<T extends { source?: MeropeSpeechSource | string }>(
    detail: T,
    source: MeropeSpeechSource | string,
  ): T {
    if (source !== 'reply' || this.generation <= 0) return detail
    return { ...detail, generation: this.generation }
  }

  private nextUtteranceId(prefix: string, messageId: string): string {
    this.sequence += 1
    return `${prefix}-${this.sequence}-${messageId}`
  }

  private remember(messageId: string, text: string): void {
    if (
      !this.spoken.has(messageId) &&
      this.spoken.size >= MAX_REMEMBERED_MESSAGES
    ) {
      const oldest = this.spoken.keys().next().value
      if (typeof oldest === 'string') this.spoken.delete(oldest)
    }
    this.spoken.set(messageId, text)
  }
}

/** 回复与通知共用序号和去重账本。 */
export const agentFace = new AgentFaceChannel()
authSubject.subscribe(() => agentFace.resetSubject())
