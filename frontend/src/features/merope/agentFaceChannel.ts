import type {
  MeropeSpeechEventDetail,
  MeropeSpeechSource,
  SpeechUtteranceInput,
} from './speechEvents'
import { newMotionIntentId } from './motion/liveGeneration'
import {
  dispatchMeropePerformance,
  dispatchMeropeState,
} from './performanceEvents'
import {
  dispatchMeropeSpeech,
  dispatchMeropeSpeechUtterance,
} from './speechEvents'

/** 一次挂载里记住的消息条数；超出按最早说过的那条丢。 */
const MAX_REMEMBERED_MESSAGES = 200

/** 事件出口。注入实现让协议本身可以脱离 window 测试。 */
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

/** 说出口的文本按同一套规范化后再比对，避免同一句话因空白差异重说。 */
function normalizeSpokenText(text: string): string {
  return text.trim().replace(/\s+/gu, ' ').slice(0, 2_000)
}

interface ReplyUtteranceContext {
  messageId: string
  locale?: string
  nextUtteranceId: () => string
  emit: (detail: MeropeSpeechEventDetail) => void
  remember: (text: string) => void
}

/**
 * 一条回复的流式发声。
 *
 * 首个非空 token 才真正开口，`end` / `cancel` 幂等；结束后同一个对象可以再次
 * 开口（announce_plan 说完、ai_summarize 接着说就是这条路径），每次拿新的
 * utterance id。
 */
export class ReplyUtterance {
  private utteranceId: string | null = null
  private text = ''

  constructor(private readonly context: ReplyUtteranceContext) {}

  /** 推进一个 SSE token。空白 token 只在已开口时并进文本，不单独发事件。 */
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

  /** 说完。说出去的内容记账，整句兜底不会把同一句再说一遍。 */
  end(): void {
    this.close('end')
  }

  /** 中断。不记账 —— 没说完的内容不该挡住之后的整句兜底。 */
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

/**
 * 面板到形象的唯一出口。
 *
 * 全站只有这里往 window 上发形象事件：说话（流式与整句）、表演指令、心情/活动。
 * 它只装协议 —— utterance id、整句去重、start / chunk / end / cancel 的时序、
 * 表演与说话的先后 —— 不认识面板的消息模型，也不知道形象挂在哪个宿主。
 * 事件载荷是与接收端（`useRig*Lifecycle` 及其宿主）之间的契约，改这里不该改到
 * 线上的形状。
 */
export class AgentFaceChannel {
  private sequence = 0
  private generation = 0
  private readonly spoken = new Map<string, string>()

  constructor(private readonly sink: AgentFaceSink = windowSink) {}

  setGeneration(generation: number): void {
    this.generation = Math.max(0, Math.trunc(generation))
  }

  /** 流式回复：SSE token 边到边说。 */
  openReply(messageId: string, locale?: string): ReplyUtterance {
    return new ReplyUtterance({
      messageId,
      locale,
      nextUtteranceId: () => this.nextUtteranceId('stream', messageId),
      emit: (detail) => this.sink.speech(this.withGeneration(detail, 'reply')),
      remember: (text) => this.remember(messageId, text),
    })
  }

  /**
   * 交付一条说出口的话：先发表演指令，再说整句。
   *
   * 表演可以没有台词（后端单独推来的 performance_plan），台词也可以没有表演
   * （追问、Lite 没给出计划）—— 两者都缺才什么都不做。
   *
   * 整句与流式共用一份账本：账本按 messageId 记，回复消息和通知是两套 id 空间，
   * 互相不会挡；挡的是同一条消息的同一句话被说第二遍。
   */
  deliver(line: {
    messageId: string
    text?: string
    source?: MeropeSpeechSource
    locale?: string
    /** 原样转交 —— 校验归 `performanceEvents`，通知携带的计划本来就是未校验的。 */
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

  /** 心情/活动变了。与某一条话无关，原样转交给事件层做校验。 */
  updateState(state: unknown): void {
    this.sink.state(state)
  }

  /** 整条消息级中断：新建会话、打断执行、请求失败、重连断流。 */
  cancel(messageId: string): void {
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

/**
 * 全站唯一实例。两个发送方 —— 执行引擎的回复、通知中心的主动开口 ——
 * 共用同一份 id 序号和去重账本，形象那边只看见一条说话流。
 */
export const agentFace = new AgentFaceChannel()
