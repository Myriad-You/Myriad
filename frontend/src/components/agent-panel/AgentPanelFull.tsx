import React, { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useAgentMessages, useAgentSessionId } from './agentMessages'
import {
  dispatchAgentPanelAnswer,
  dispatchAgentPanelCommand,
  dispatchAgentPanelOpenSession,
} from './agentPanelEvents'
import { AgentPanelManage } from './AgentPanelManage'
import { AgentPanelMessage } from './AgentPanelMessage'
import { AgentPanelSessions, useAgentSessionList } from './AgentPanelSessions'
import { agentPanelRowWaveMs } from './agentPanelStage'
import { AgentPresence, AgentPresenceList } from './useAgentPresence'
import { useConversationPan } from './useConversationPan'

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

export const AgentPanelFull: React.FC<AgentPanelFullProps> = ({
  view,
  onView,
  onSubmit,
  onWorkOffer,
  showChrome = false,
}) => {
  const { t } = useI18n()
  const messages = useAgentMessages()
  const sessionId = useAgentSessionId()
  const sessionCountRef = useRef(0)
  const { held, exiting } = useHeldView(
    view,
    messages.length,
    sessionCountRef.current,
  )
  const sessionList = useAgentSessionList(
    view === 'sessions' || held === 'sessions',
  )
  sessionCountRef.current = sessionList.sessions?.length ?? 0
  const [zoomed, setZoomed] = useState<string | null>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const trackRef = useRef<HTMLDivElement>(null)
  useConversationPan(listRef, trackRef, held === 'messages', sessionId)

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
          dispatchAgentPanelOpenSession(id)
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
            <AgentPresenceList
              items={messages}
              keyOf={(message) => message.id}
              kind="row"
              from="composer"
            >
              {(message) => (
                <AgentPanelMessage
                  message={message}
                  onAnswer={dispatchAgentPanelAnswer}
                  onSuggest={onSubmit}
                  onWorkOffer={onWorkOffer}
                  onZoomImage={setZoomed}
                  onRetry={() => {
                    const index = messages.findIndex(
                      (item) => item.id === message.id,
                    )
                    const asked = messages
                      .slice(0, index)
                      .findLast((item) => item.role === 'user')
                    if (asked) onSubmit(asked.content)
                  }}
                />
              )}
            </AgentPresenceList>
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
