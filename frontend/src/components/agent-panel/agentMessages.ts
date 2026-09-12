import type { AgentAttachment } from './agentAttachments'
import type { AgentMessageStep } from './agentThinking'
import { useSyncExternalStore } from 'react'

/** Follow-up question; confirmations are action cards. */
export interface AgentMessageQuestion {
  id: string
  text: string
  context?: string
  options?: Array<{ value: string; label: string; description?: string }>
  answered?: string
}

export interface AgentMessage {
  id: string
  role: 'user' | 'assistant' | 'system'
  content: string
  state?: 'streaming' | 'error'
  imageUrls?: string[]
  attachments?: AgentAttachment[]
  steps?: AgentMessageStep[]
  /** Planner reasoning; not the reply. */
  thought?: string
  question?: AgentMessageQuestion
  suggestions?: string[]
  workOffer?: { input: string }
  at?: number
}

const EMPTY: readonly AgentMessage[] = Object.freeze([])

let messages: readonly AgentMessage[] = EMPTY

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

/** Reuse unchanged message objects; streaming rebuilds the array every token. */
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

/** Count only: streaming content must not refresh the composer. */
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
