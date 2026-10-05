import type { AgentAttachment } from './agentAttachments'
import type { AgentPanelMode } from './agentPanelMode'
import { authSubject } from '../../utils/authSubject'
import { createStore, patchStore } from '../../utils/store'
import {
  getAgentPanelMode,
  isAgentPanelMode,
  setAgentPanelMode,
} from './agentPanelMode'

export const AGENT_PANEL_SUBMIT_EVENT = 'agent-panel-submit'

export interface AgentPanelSubmitDetail {
  text: string
  attachments?: AgentAttachment[]
  mode: AgentPanelMode
  intentionId?: string
}

export function dispatchAgentPanelSubmit(
  text: string,
  attachments?: readonly AgentAttachment[],
  mode: AgentPanelMode = getAgentPanelMode(),
  intentionId?: string,
): void {
  const trimmed = text.trim()
  const files = attachments?.length ? Iterator.from(attachments).toArray() : undefined
  if (!trimmed && !files?.length) return
  window.dispatchEvent(
    new CustomEvent<AgentPanelSubmitDetail>(AGENT_PANEL_SUBMIT_EVENT, {
      detail: {
        text: trimmed,
        mode,
        ...(intentionId ? { intentionId } : {}),
        ...(files ? { attachments: files } : {}),
      },
    }),
  )
}

export function agentPanelSubmitDetail(
  event: Event,
): AgentPanelSubmitDetail | null {
  const detail = (event as CustomEvent<AgentPanelSubmitDetail>).detail
  const text = typeof detail?.text === 'string' ? detail.text.trim() : ''
  const attachments = Array.isArray(detail?.attachments)
    ? detail.attachments
    : undefined
  const intentionId =
    typeof detail?.intentionId === 'string' && detail.intentionId
      ? detail.intentionId
      : undefined
  if (!text && !attachments?.length) return null
  return {
    text,
    mode: isAgentPanelMode(detail?.mode) ? detail.mode : 'work',
    ...(intentionId ? { intentionId } : {}),
    ...(attachments?.length ? { attachments } : {}),
  }
}

export const AGENT_PANEL_OPEN_SESSION_EVENT = 'agent-panel-open-session'

export function dispatchAgentPanelOpenSession(sessionId: string, messageCount = 0): void {
  if (!sessionId) return
  window.dispatchEvent(
    new CustomEvent<{ sessionId: string; messageCount: number }>(AGENT_PANEL_OPEN_SESSION_EVENT, {
      detail: { sessionId, messageCount },
    }),
  )
}

export function agentPanelOpenSessionId(event: Event): string | null {
  const detail = (event as CustomEvent<{ sessionId?: string }>).detail
  const id = typeof detail?.sessionId === 'string' ? detail.sessionId : ''
  return id || null
}

export const AGENT_PANEL_OPEN_EVENT = 'agent-panel-open'

export type AgentPanelOpenView = 'messages' | 'manage'

export function dispatchAgentPanelOpen(view: AgentPanelOpenView): void {
  window.dispatchEvent(
    new CustomEvent<{ view: AgentPanelOpenView }>(AGENT_PANEL_OPEN_EVENT, {
      detail: { view },
    }),
  )
}

export function agentPanelOpenView(event: Event): AgentPanelOpenView {
  const raw = (event as CustomEvent<{ view?: string }>).detail?.view
  return raw === 'manage' ? 'manage' : 'messages'
}

export interface QueuedAgentPanelOpen {
  view: AgentPanelOpenView
  stage: 'overlay' | 'full'
}

export interface QueuedAgentSessionOpen {
  sessionId: string
  runId?: string
  taskId?: string
}

/**
 * 面板/引擎挂上之前（访问检查、语言包、懒加载 chunk）打开请求没有监听者。
 * 先记在这里，由挂上的一方 attach 时消费；attach 之后由它自己听事件，不再排队。
 */
const panelQueue = createStore<{ open: QueuedAgentPanelOpen | null, attached: boolean }>({
  open: null,
  attached: false,
})
let queuedAgentSessionOpen: QueuedAgentSessionOpen | null = null
let queuedAgentSubmit: AgentPanelSubmitDetail | null = null
let engineAttached = false

// 排队的会话 id 与要说的话都属于排队时的主体；换号后不能替新主体发出去。
authSubject.subscribe(() => {
  queuedAgentSessionOpen = null
  queuedAgentSubmit = null
})

/** Fires when a panel open is queued, consumed or discarded, or the panel attaches/detaches. */
export const subscribeAgentOpenQueue = panelQueue.subscribe

export function isAgentPanelAttached(): boolean {
  return panelQueue.get().attached
}

export function queueAgentPanelOpen(next: QueuedAgentPanelOpen): void {
  if (!panelQueue.get().attached) patchStore(panelQueue, { open: next })
}

export function hasQueuedAgentPanelOpen(): boolean {
  return panelQueue.get().open !== null
}

/** 面板 mount：取走排队的打开请求，并接管后续打开。 */
export function attachAgentPanelOpenQueue(): {
  queued: QueuedAgentPanelOpen | null
  detach: () => void
} {
  const queued = panelQueue.get().open
  patchStore(panelQueue, { open: null, attached: true })
  return {
    queued,
    detach: () => {
      patchStore(panelQueue, { attached: false })
    },
  }
}

export function queueAgentSessionOpen(event: Event): void {
  if (engineAttached) return
  const detail = (event as CustomEvent<Partial<QueuedAgentSessionOpen> | null>).detail
  if (typeof detail?.sessionId !== 'string' || !detail.sessionId) return
  queuedAgentSessionOpen = {
    sessionId: detail.sessionId,
    runId: typeof detail.runId === 'string' ? detail.runId : undefined,
    taskId: typeof detail.taskId === 'string' ? detail.taskId : undefined,
  }
}

/** 引擎 mount：取走排队的会话打开请求，并接管后续请求。 */
export function attachAgentSessionOpenQueue(): {
  queued: QueuedAgentSessionOpen | null
  detach: () => void
} {
  engineAttached = true
  const queued = queuedAgentSessionOpen
  queuedAgentSessionOpen = null
  return {
    queued,
    detach: () => {
      engineAttached = false
    },
  }
}

/**
 * 面板之外（Agent 设置里的「她最近」）让她开口：切到该模式、发出这句、打开面板。
 * 引擎还没挂时提交事件没人听，先记下，由挂上的引擎用 takeQueuedAgentSubmit 兑现。
 */
export function askAgentPanel(text: string, mode: AgentPanelMode): void {
  const trimmed = text.trim()
  if (!trimmed) return
  setAgentPanelMode(mode)
  if (engineAttached) dispatchAgentPanelSubmit(trimmed, undefined, mode)
  else queuedAgentSubmit = { text: trimmed, mode }
  dispatchAgentPanelOpen('messages')
}

/** 引擎能发送之后取走排队的那句话。 */
export function takeQueuedAgentSubmit(): AgentPanelSubmitDetail | null {
  const queued = queuedAgentSubmit
  queuedAgentSubmit = null
  return queued
}

/** 访问被拒时丢弃未兑现的请求，避免之后获准时突然弹出。 */
export function clearQueuedAgentOpens(): void {
  queuedAgentSessionOpen = null
  queuedAgentSubmit = null
  patchStore(panelQueue, { open: null })
}

export const AGENT_PANEL_CLOSE_EVENT = 'agent-panel-close'

export function dispatchAgentPanelClose(): void {
  window.dispatchEvent(new Event(AGENT_PANEL_CLOSE_EVENT))
}

export const AGENT_PANEL_COMMAND_EVENT = 'agent-panel-command'

export type AgentPanelCommand = 'new-session' | 'interrupt'

export function dispatchAgentPanelCommand(command: AgentPanelCommand): void {
  window.dispatchEvent(
    new CustomEvent<{ command: AgentPanelCommand }>(AGENT_PANEL_COMMAND_EVENT, {
      detail: { command },
    }),
  )
}

export function agentPanelCommand(event: Event): AgentPanelCommand | null {
  const raw = (event as CustomEvent<{ command?: string }>).detail?.command
  return raw === 'new-session' || raw === 'interrupt' ? raw : null
}

export const AGENT_PANEL_ANSWER_EVENT = 'agent-panel-answer'
export const AGENT_PANEL_HISTORY_ANSWER_RESULT_EVENT = 'agent-panel-history-answer-result'

export function dispatchHistoryAnswerResult(sessionId: string, messageId: string, success: boolean): void {
  window.dispatchEvent(new CustomEvent(AGENT_PANEL_HISTORY_ANSWER_RESULT_EVENT, {
    detail: { sessionId, messageId, success },
  }))
}

export interface AgentPanelAnswerDetail {
  messageId: string
  answer: string
  history?: import('./restoreHistoryAnswer').HistoryAnswerSource
}

export function dispatchAgentPanelAnswer(
  messageId: string,
  answer: string,
  history?: import('./restoreHistoryAnswer').HistoryAnswerSource,
): void {
  if (!messageId || !answer.trim()) return
  window.dispatchEvent(
    new CustomEvent<AgentPanelAnswerDetail>(AGENT_PANEL_ANSWER_EVENT, {
      detail: { messageId, answer, ...(history ? { history } : {}) },
    }),
  )
}

export function agentPanelAnswerDetail(
  event: Event,
): AgentPanelAnswerDetail | null {
  const detail = (event as CustomEvent<AgentPanelAnswerDetail>).detail
  if (!detail?.messageId || !detail.answer) return null
  return detail
}

export function agentPanelOpenSessionCount(event: Event): number {
  const value = (event as CustomEvent<{ messageCount?: number }>).detail?.messageCount
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0
}
