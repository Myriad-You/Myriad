import type { QueuedAgentPanelOpen } from './agentPanelEvents'
import { useCallback, useEffect, useState, useSyncExternalStore } from 'react'
import { isDocumentReady, subscribeDocumentReady } from '../../utils/pageLoader'
import {
  AGENT_PANEL_OPEN_EVENT,
  AGENT_PANEL_OPEN_SESSION_EVENT,
  agentPanelOpenView,
  hasQueuedAgentPanelOpen,
  queueAgentPanelOpen,
  queueAgentSessionOpen,
  subscribeAgentOpenQueue,
} from './agentPanelEvents'
import { LONG_PRESS_DURATION, useLongPress } from './useLongPress'
import './agent-panel-longpress.css'

export const AGENT_SESSION_DELAY_MS = 5000
export const AGENT_SESSION_IDLE_TIMEOUT_MS = 4000

const WAKE_EVENTS = [
  AGENT_PANEL_OPEN_EVENT,
  AGENT_PANEL_OPEN_SESSION_EVENT,
  'arael-open-session',
  'arael-open-manage',
] as const

const LONG_PRESS_OPEN: QueuedAgentPanelOpen = { view: 'messages', stage: 'overlay' }

// React lazy surfaces import failures through its render boundary; warming is optional.
function preloadAgentSession(): void {
  void import('./AgentEngine').catch(() => {})
  void import('./AgentPanel').catch(() => {})
}

function queueOpenFor(event: Event): void {
  if (event.type === 'arael-open-session') queueAgentSessionOpen(event)
  queueAgentPanelOpen({
    view: event.type === AGENT_PANEL_OPEN_EVENT
      ? agentPanelOpenView(event)
      : event.type === 'arael-open-manage' ? 'manage' : 'messages',
    stage: 'full',
  })
}

/**
 * 面板 attach 之前（访问检查、语言包、懒加载 chunk）替它收下长按与打开事件，
 * 记进队列由面板/引擎挂上后兑现。只收集意图，不画长按指示——那由宿主负责。
 */
export function AgentOpenIntentCapture() {
  useEffect(() => {
    for (const name of WAKE_EVENTS) window.addEventListener(name, queueOpenFor)
    return () => {
      for (const name of WAKE_EVENTS) window.removeEventListener(name, queueOpenFor)
    }
  }, [])
  useLongPress(
    LONG_PRESS_DURATION,
    useCallback(() => queueAgentPanelOpen(LONG_PRESS_OPEN), []),
    true,
  )
  return null
}

/**
 * 推迟 AgentEngine / AgentPanel 的第一次 import。
 * 有排队的打开请求立刻挂；否则首屏后 5s + idle，避开 3s 后台 Tapp。挂上之后关面板不卸。
 */
export function AgentSessionHost({ children }: { children: React.ReactNode }) {
  const documentReady = useSyncExternalStore(subscribeDocumentReady, isDocumentReady, () => false)
  const openQueued = useSyncExternalStore(
    subscribeAgentOpenQueue,
    hasQueuedAgentPanelOpen,
    () => false,
  )
  const [ready, setReady] = useState(false)

  const wake = useCallback(() => {
    preloadAgentSession()
    setReady(true)
  }, [])

  useEffect(() => {
    if (!ready && openQueued) wake()
  }, [openQueued, ready, wake])

  useEffect(() => {
    if (ready || !documentReady) return
    let idleId: number | null = null
    const timerId = window.setTimeout(() => {
      if ('requestIdleCallback' in window) {
        idleId = requestIdleCallback(() => wake(), {
          timeout: AGENT_SESSION_IDLE_TIMEOUT_MS,
        })
      } else {
        wake()
      }
    }, AGENT_SESSION_DELAY_MS)
    return () => {
      window.clearTimeout(timerId)
      if (idleId !== null && 'cancelIdleCallback' in window) {
        cancelIdleCallback(idleId)
      }
    }
  }, [documentReady, ready, wake])

  // 触发由 AgentOpenIntentCapture 排队；这里只画按压反馈。
  const { indicator } = useLongPress(LONG_PRESS_DURATION, undefined, !ready)

  if (ready) return children

  return (
    <div
      className={`agent-panel-longpress${indicator.active ? ' active' : ''}`}
      style={{ left: indicator.x, top: indicator.y }}
    >
      <div className="agent-panel-lp-dot" />
      <div className="agent-panel-lp-pulse" />
      <svg className="agent-panel-lp-svg" viewBox="0 0 40 40">
        <circle className="agent-panel-lp-track" cx="20" cy="20" r="16" />
        <circle className="agent-panel-lp-ring" cx="20" cy="20" r="16" />
      </svg>
    </div>
  )
}
