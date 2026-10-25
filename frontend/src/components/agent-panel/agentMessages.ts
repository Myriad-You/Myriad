import type { AgentAttachment } from './agentAttachments'
import type { AgentMessageStep } from './agentThinking'
import { useCallback, useSyncExternalStore } from 'react'

/** Follow-up question; confirmations are action cards. */
export interface AgentMessageQuestion {
  id: string
  text: string
  context?: string
  options?: Array<{ value: string; label: string; description?: string }>
  answered?: string
  detail?: import('./confirmationDetail').ConfirmationDetail
}

export interface AgentMessage {
  workPlan?: import('../../services/agent/types').WorkPlanItem[]
  id: string
  role: 'user' | 'assistant' | 'system'
  content: string
  body?: import('./messageBody').MessageBodyRef
  bodyUnavailable?: boolean
  thoughtBody?: import('./messageBody').MessageBodyRef
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

let messages: readonly AgentMessage[] | null = EMPTY
const EMPTY_IDS: readonly string[] = Object.freeze([])
let messageIds: readonly string[] = EMPTY_IDS
let messagesById = new Map<string, AgentMessage>()
const messageListeners = new Map<string, Set<() => void>>()

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
    x.body?.id === y.body?.id && x.body?.chars === y.body?.chars &&
    x.bodyUnavailable === y.bodyUnavailable &&
    x.thoughtBody?.id === y.thoughtBody?.id && x.thoughtBody?.chars === y.thoughtBody?.chars &&
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
    JSON.stringify(x.workPlan) === JSON.stringify(y.workPlan) &&
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

/** Full replacement for history loads and structural changes. */
export function setAgentMessages(next: readonly AgentMessage[]): void {
  const previous = getAgentMessagesSnapshot()
  if (sameList(previous, next)) return
  const changed = new Set<string>(messageIds)
  const nextById = new Map<string, AgentMessage>()
  messages =
    next.length === 0
      ? EMPTY
      : next.map((item) => {
          const old = messagesById.get(item.id)
          if (old && sameMessage(old, item)) {
            changed.delete(item.id)
            nextById.set(item.id, old)
            return old
          }
          changed.add(item.id)
          nextById.set(item.id, item)
          return item
        })
  messagesById = nextById
  if (
    messageIds.length !== next.length ||
    next.some((item, i) => item.id !== messageIds[i])
  ) {
    messageIds = next.length ? next.map((item) => item.id) : EMPTY_IDS
  }
  for (const id of changed) notifyMessage(id)
  for (const listener of listeners) listener()
}

function notifyMessage(id: string): void {
  for (const listener of messageListeners.get(id) ?? []) listener()
}

/** Content updates do not copy history or invalidate the list's ids snapshot. */
export function updateAgentMessage(next: AgentMessage): void {
  const old = messagesById.get(next.id)
  if (!old || sameMessage(old, next)) return
  messagesById.set(next.id, next)
  messages = null
  notifyMessage(next.id)
  for (const listener of listeners) listener()
}

export function getAgentMessageIdsSnapshot(): readonly string[] {
  return messageIds
}

export function getAgentMessageSnapshot(id: string): AgentMessage | undefined {
  return messagesById.get(id)
}

export function subscribeAgentMessage(
  id: string,
  listener: () => void,
): () => void {
  let group = messageListeners.get(id)
  if (!group) messageListeners.set(id, (group = new Set()))
  group.add(listener)
  return () => {
    group.delete(listener)
    if (!group.size) messageListeners.delete(id)
  }
}

export function useAgentMessageIds(): readonly string[] {
  return useSyncExternalStore(
    subscribeAgentMessages,
    getAgentMessageIdsSnapshot,
    () => EMPTY_IDS,
  )
}

export function useAgentMessage(id: string): AgentMessage | undefined {
  const subscribe = useCallback(
    (listener: () => void) => subscribeAgentMessage(id, listener),
    [id],
  )
  const snapshot = useCallback(() => getAgentMessageSnapshot(id), [id])
  return useSyncExternalStore(subscribe, snapshot, () => undefined)
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
  return (messages ??= messageIds.map((id) => messagesById.get(id)!))
}

/** Count only: streaming content must not refresh the composer. */
export function getAgentMessageCountSnapshot(): number {
  return messageIds.length
}

export function useAgentMessageCount(): number {
  return useSyncExternalStore(
    subscribeAgentMessages,
    getAgentMessageCountSnapshot,
    () => 0,
  )
}
