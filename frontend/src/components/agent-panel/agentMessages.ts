/**
 * 对话内容的全站唯一副本。
 *
 * 和状态那份一样的路数：谁在执行谁往这里写，谁要显示谁从这里读。现在写的是旧
 * 面板，读的是新的 Full 层 —— 这样能一边把渲染搬过来，一边不动那套跑通了的
 * SSE 状态机。等旧面板整个删掉时，换成新的执行方往这里写就行。
 *
 * 刻意不复用旧面板的 `ChatMessage`：那上面挂着执行追踪、调试轨迹、待答问题
 * 一大串，新 UI 只需要「谁说的、说了什么、说完没有」。把它们分开，删旧面板时
 * 才不会连着一串类型一起拆。
 */

import type { AgentAttachment } from './agentAttachments'
import type { AgentMessageStep } from './agentThinking'
import { useSyncExternalStore } from 'react'

/** 助手停下来问的一句话。敏感操作确认不走这里 —— 那是操作卡片。 */
export interface AgentMessageQuestion {
  id: string
  text: string
  context?: string
  options?: Array<{ value: string; label: string; description?: string }>
  /** 已经答过的那个选项。答过之后按钮冻结，但仍然看得见选了什么。 */
  answered?: string
}

export interface AgentMessage {
  id: string
  role: 'user' | 'assistant' | 'system'
  content: string
  /**
   * `streaming` 还在说；`error` 这轮出了问题。
   * 说完的不带状态 —— 正常结束不需要额外标记。
   */
  state?: 'streaming' | 'error'
  /** 助手这轮产出的图片 */
  imageUrls?: string[]
  /** 用户这条附上的文件 */
  attachments?: AgentAttachment[]
  /** 这轮走了哪几步。空着表示没有值得摆出来的过程。 */
  steps?: AgentMessageStep[]
  /**
   * 思考过程本文：Planner 的判断说明，或还没排出步骤时的进度句。
   * 跟正文不是同一段 —— 正文是答，这段是怎么想到的。
   */
  thought?: string
  /** 它反过来问你的话 */
  question?: AgentMessageQuestion
  /** 答完之后给的下一步建议 */
  suggestions?: string[]
  /** 闲聊里听成了要干活，点了就进做事档 */
  workOffer?: { input: string }
  /** 说这句话的时刻（epoch ms） */
  at?: number
}

const EMPTY: readonly AgentMessage[] = Object.freeze([])

let messages: readonly AgentMessage[] = EMPTY

/** 现在读的是哪一条会话。历史列表靠它标出「就是这条」。 */
let sessionId: string | null = null

const listeners = new Set<() => void>()

function sameSteps(
  a: readonly AgentMessageStep[] | undefined,
  b: readonly AgentMessageStep[] | undefined,
): boolean {
  if (a === b) return true
  if (!a || !b) return false
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i += 1) {
    // 名字和状态会变（重试会换说法），时长跑完才有 —— 三样都要比
    if (
      a[i].status !== b[i].status ||
      a[i].name !== b[i].name ||
      a[i].durationMs !== b[i].durationMs ||
      a[i].note !== b[i].note
    ) {
      return false
    }
  }
  return true
}

function sameMessage(x: AgentMessage, y: AgentMessage): boolean {
  return (
    x.id === y.id &&
    x.role === y.role &&
    x.content === y.content &&
    x.state === y.state &&
    x.imageUrls?.length === y.imageUrls?.length &&
    x.attachments?.length === y.attachments?.length &&
    !x.attachments?.some(
      (item, index) => item.id !== y.attachments?.[index]?.id,
    ) &&
    x.question?.id === y.question?.id &&
    x.question?.answered === y.question?.answered &&
    x.suggestions?.length === y.suggestions?.length &&
    x.workOffer?.input === y.workOffer?.input &&
    x.thought === y.thought &&
    sameSteps(x.steps, y.steps)
  )
}

function sameList(
  a: readonly AgentMessage[],
  b: readonly AgentMessage[],
): boolean {
  if (a === b) return true
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i += 1) {
    if (!sameMessage(a[i], b[i])) return false
  }
  return true
}

/**
 * 换一份对话。逐条比过再决定要不要通知 —— 流式回复每个 token 都会重建数组，
 * 只看引用的话每秒会把整个列表重渲染几十次。没变的那条沿用原来的对象，
 * 列表重绘时旧气泡才能跳过。
 */
export function setAgentMessages(next: readonly AgentMessage[]): void {
  if (sameList(messages, next)) return
  if (next.length === 0) {
    messages = EMPTY
  } else {
    const prevById = new Map<string, AgentMessage>()
    for (const item of messages) prevById.set(item.id, item)
    messages = next.map((item) => {
      const old = prevById.get(item.id)
      return old && sameMessage(old, item) ? old : item
    })
  }
  for (const listener of listeners) listener()
}

export function setAgentSessionId(next: string | null): void {
  if (sessionId === next) return
  sessionId = next
  for (const listener of listeners) listener()
}

export function getAgentSessionIdSnapshot(): string | null {
  return sessionId
}

export function getServerAgentSessionIdSnapshot(): string | null {
  return null
}

export function useAgentSessionId(): string | null {
  return useSyncExternalStore(
    subscribeAgentMessages,
    getAgentSessionIdSnapshot,
    getServerAgentSessionIdSnapshot,
  )
}

export function subscribeAgentMessages(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function getAgentMessagesSnapshot(): readonly AgentMessage[] {
  return messages
}

export function getServerAgentMessagesSnapshot(): readonly AgentMessage[] {
  return EMPTY
}

export function useAgentMessages(): readonly AgentMessage[] {
  return useSyncExternalStore(
    subscribeAgentMessages,
    getAgentMessagesSnapshot,
    getServerAgentMessagesSnapshot,
  )
}

/** 外壳只要条数：内容在流，条数没变就不该把输入行跟着刷。 */
export function getAgentMessageCountSnapshot(): number {
  return messages.length
}

export function useAgentMessageCount(): number {
  return useSyncExternalStore(
    subscribeAgentMessages,
    getAgentMessageCountSnapshot,
    () => 0,
  )
}
