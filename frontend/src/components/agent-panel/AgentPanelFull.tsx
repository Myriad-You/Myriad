import type { AgentPanelMessageProps } from './AgentPanelMessage'
import React, { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  getAgentMessagesSnapshot,
  useAgentMessage,
  useAgentMessageIds,
  useAgentSessionId,
} from './agentMessages'
import {
  AGENT_PANEL_HISTORY_ANSWER_RESULT_EVENT,
  dispatchAgentPanelAnswer,
  dispatchAgentPanelCommand,
  dispatchAgentPanelOpenSession,
} from './agentPanelEvents'
import { AgentPanelManage } from './AgentPanelManage'
import { AgentPanelMessage } from './AgentPanelMessage'
import { useAgentPanelMode } from './agentPanelMode'
import { AgentPanelSessions, useAgentSessionList } from './AgentPanelSessions'
import { agentPanelRowWaveMs } from './agentPanelStage'
import { useConversationHistory } from './conversationHistory'
import { AgentPresence, AgentPresenceList } from './useAgentPresence'
import { useConversationPan } from './useConversationPan'
import { usePersistedHistory } from './usePersistedHistory'

function useHeldView(
  view: AgentPanelFullView,
  messagesCount: number,
  sessionsCount: number,
): {
  held: AgentPanelFullView
  exiting: boolean
} {
  const [held, setHeld] = useState(view)
  const [exiting, setExiting] = useState(false)
  const messagesCountRef = useRef(messagesCount)
  const sessionsCountRef = useRef(sessionsCount)
  messagesCountRef.current = messagesCount
  sessionsCountRef.current = sessionsCount
  useEffect(() => {
    if (view === held) {
      setExiting(false)
      return
    }
    if (held === 'manage' || view === 'manage') {
      setHeld(view)
      setExiting(false)
      return
    }
    setExiting(true)
    const outgoing =
      held === 'sessions' ? sessionsCountRef.current : messagesCountRef.current
    const timer = setTimeout(() => {
      setHeld(view)
      setExiting(false)
    }, agentPanelRowWaveMs(outgoing))
    return () => clearTimeout(timer)
  }, [held, view])
  return { held, exiting }
}

export type AgentPanelFullView = 'messages' | 'sessions' | 'manage'

export interface AgentPanelFullProps {
  view: AgentPanelFullView
  onView: (view: AgentPanelFullView) => void
  onSubmit: (text: string) => void
  onWorkOffer: (input: string) => void
  showChrome?: boolean
}

export const AgentPanelSessionChrome: React.FC<{
  view: AgentPanelFullView
  onView: (view: AgentPanelFullView) => void
}> = ({ view, onView }) => {
  const { t } = useI18n()
  const showsSessions = view === 'sessions'
  const viewLabel = view === 'manage' ? t.agentPanel.manage.title : null

  return (
    <>
      {viewLabel && (
        <span className="agent-panel-tag">
          <span className="agent-panel-tag-text">{viewLabel}</span>
        </span>
      )}
      <button
        type="button"
        className="agent-panel-tag"
        data-icon="true"
        onClick={() => {
          dispatchAgentPanelCommand('new-session')
          onView('messages')
        }}
        title={t.agentPanel.newSession}
        aria-label={t.agentPanel.newSession}
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
          <path d="M12 7v6M9 10h6" />
        </svg>
      </button>
      <button
        type="button"
        className="agent-panel-tag"
        data-icon="true"
        data-tone={showsSessions ? 'primary' : 'neutral'}
        onClick={() => onView(showsSessions ? 'messages' : 'sessions')}
        title={t.agentPanel.sessions.title}
        aria-label={t.agentPanel.sessions.title}
        aria-pressed={showsSessions}
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <circle cx="12" cy="12" r="9" />
          <path d="M12 7v5l3 2" />
        </svg>
      </button>
    </>
  )
}

function SubscribedMessage({
  id,
  onSubmit,
  ...props
}: Omit<AgentPanelMessageProps, 'message' | 'onRetry'> & {
  id: string
  onSubmit: (text: string) => void
}) {
  const message = useAgentMessage(id)
  // Presence retains removed rows for the exit animation.
  const lastMessage = useRef(message)
  if (message) lastMessage.current = message
  const shown = message ?? lastMessage.current
  if (!shown) return null
  return (
    <AgentPanelMessage
      {...props}
      message={shown}
      onRetry={() => {
        const messages = getAgentMessagesSnapshot()
        const index = messages.findIndex((item) => item.id === id)
        const asked = messages
          .slice(0, index)
          .findLast((item) => item.role === 'user')
        if (asked) onSubmit(asked.content)
      }}
    />
  )
}

export const AgentPanelFull: React.FC<AgentPanelFullProps> = ({
  view,
  onView,
  onSubmit,
  onWorkOffer,
  showChrome = false,
}) => {
  const { t } = useI18n()
  const messageIds = useAgentMessageIds()
  const { visibleIds, onNearStart, onNewer, hasNewer } =
    useConversationHistory(messageIds)
  const sessionId = useAgentSessionId()
  const mode = useAgentPanelMode()
  const history = usePersistedHistory(mode === 'work' ? sessionId : null)
  const [historyAnswerError, setHistoryAnswerError] = useState(false)
  useEffect(() => setHistoryAnswerError(false), [sessionId, history.page])
  useEffect(() => {
    const receive = (event: Event) => {
      const detail = (
        event as CustomEvent<{
          sessionId: string
          messageId: string
          success: boolean
        }>
      ).detail
      if (
        detail?.sessionId !== sessionId ||
        !history.rows.some((row) => row.id === detail.messageId)
      ) {
        return
      }
      if (detail.success) history.select(null)
      else setHistoryAnswerError(true)
    }
    window.addEventListener(AGENT_PANEL_HISTORY_ANSWER_RESULT_EVENT, receive)
    return () =>
      window.removeEventListener(
        AGENT_PANEL_HISTORY_ANSWER_RESULT_EVENT,
        receive,
      )
  }, [history, sessionId])
  const sessionCountRef = useRef(0)
  const { held, exiting } = useHeldView(
    view,
    visibleIds.length,
    sessionCountRef.current,
  )
  const sessionList = useAgentSessionList(
    view === 'sessions' || held === 'sessions',
  )
  sessionCountRef.current = sessionList.sessions?.length ?? 0
  const [zoomed, setZoomed] = useState<string | null>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const trackRef = useRef<HTMLDivElement>(null)
  useConversationPan(
    listRef,
    trackRef,
    held === 'messages',
    `${sessionId}:${history.page ?? 'live'}`,
    '.agent-panel-message',
    history.page ? undefined : onNearStart,
  )

  const conversation =
    held === 'sessions' ? (
      <AgentPanelSessions
        activeSessionId={sessionId}
        exiting={exiting}
        sessions={sessionList.sessions}
        error={sessionList.error}
        onNearStart={sessionList.onNearStart}
        removeSession={sessionList.removeSession}
        onSelect={(id) => {
          dispatchAgentPanelOpenSession(
            id,
            sessionList.sessions?.find((session) => session.id === id)
              ?.messageCount,
          )
          onView('messages')
        }}
      />
    ) : held === 'manage' ? (
      <div className="agent-panel-overlay agent-panel-full glass">
        <AgentPanelManage />
      </div>
    ) : (
      <div
        className="agent-panel-messages-slot"
        data-exiting={exiting ? 'true' : undefined}
      >
        <div className="agent-panel-messages agent-panel-full" ref={listRef}>
          <div className="agent-panel-messages-track" ref={trackRef}>
            {history.page ? (
              <>
                {history.loading && <p role="status">{t.common.loading}</p>}
                {(history.error || historyAnswerError) && (
                  <p role="alert">{t.agentPanel.sessions.loadFailed}</p>
                )}
                {history.rows.map((message) => (
                  <AgentPanelMessage
                    key={message.id}
                    message={message}
                    onAnswer={(messageId, answer) => {
                      if (sessionId && history.page && message.question) {
                        dispatchAgentPanelAnswer(messageId, answer, {
                          sessionId,
                          page: history.page,
                          questionId: message.question.id,
                        })
                      }
                    }}
                    onSuggest={onSubmit}
                    onWorkOffer={onWorkOffer}
                    onZoomImage={setZoomed}
                  />
                ))}
              </>
            ) : (
              <AgentPresenceList
                items={visibleIds}
                retainRemoved={false}
                keyOf={(id) => id}
                kind="row"
                from="composer"
              >
                {(id) => (
                  <SubscribedMessage
                    id={id}
                    onSubmit={onSubmit}
                    onAnswer={dispatchAgentPanelAnswer}
                    onSuggest={onSubmit}
                    onWorkOffer={onWorkOffer}
                    onZoomImage={setZoomed}
                  />
                )}
              </AgentPresenceList>
            )}
          </div>
          <AgentPresence open={!!zoomed} kind="swap" from="self">
            {zoomed ? (
              <button
                type="button"
                className="agent-panel-lightbox"
                onClick={() => setZoomed(null)}
                aria-label={t.agentPanel.closeImage}
              >
                <img src={zoomed} alt="" />
              </button>
            ) : null}
          </AgentPresence>
        </div>
      </div>
    )

  return (
    <>
      {conversation}
      {held === 'messages' &&
        (history.page || hasNewer || (mode === 'work' && sessionId)) && (
          <div className="agent-panel-tag-actions">
            {history.page ? (
              <>
                <button
                  type="button"
                  className="agent-panel-tag"
                  disabled={history.page <= 1 || history.loading}
                  onClick={() => history.select(history.page! - 1)}
                  aria-label={t.common.back}
                >
                  ←
                </button>
                <span>{history.page}</span>
                <button
                  type="button"
                  className="agent-panel-tag"
                  disabled={!history.hasNext || history.loading}
                  onClick={() => history.select(history.page! + 1)}
                  aria-label={t.common.go}
                >
                  →
                </button>
                {history.error && (
                  <button
                    type="button"
                    className="agent-panel-tag"
                    onClick={() => history.select(history.page)}
                  >
                    {t.common.retry}
                  </button>
                )}
                <button
                  type="button"
                  className="agent-panel-tag"
                  onClick={() => history.select(null)}
                >
                  {t.common.close}
                </button>
              </>
            ) : (
              <>
                {mode === 'work' && sessionId ? (
                  <button
                    type="button"
                    className="agent-panel-tag"
                    onClick={() => history.select(1)}
                  >
                    {t.agentPanel.sessions.title}
                  </button>
                ) : null}
                {hasNewer && (
                  <button
                    type="button"
                    className="agent-panel-tag"
                    onClick={onNewer}
                    aria-label={t.common.go}
                  >
                    ↓
                  </button>
                )}
              </>
            )}
          </div>
        )}
      {showChrome ? (
        <div className="agent-panel-tag-rail">
          <div className="agent-panel-tag-actions">
            <AgentPanelSessionChrome view={view} onView={onView} />
          </div>
        </div>
      ) : null}
    </>
  )
}
