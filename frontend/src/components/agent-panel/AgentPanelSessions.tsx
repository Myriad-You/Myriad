import type { SessionInfo } from '../../services/agent'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { useAgentPanelMode } from './agentPanelMode'
import { relativeTimeBucket } from './agentRelativeTime'
import { AgentPresence, AgentPresenceList } from './useAgentPresence'
import { useConversationPan } from './useConversationPan'

export interface AgentPanelSessionsProps {
  activeSessionId: string | null
  onSelect: (sessionId: string) => void
  exiting?: boolean
  sessions: SessionInfo[] | null
  error: string | null
  onNearStart: () => void
  removeSession: (id: string) => void
}

const SESSION_PAGE = 20
const SESSION_FILL = 8

export function useAgentSessionList(enabled: boolean): {
  sessions: SessionInfo[] | null
  error: string | null
  onNearStart: () => void
  removeSession: (id: string) => void
} {
  const { t } = useI18n()
  const { isAuthenticated } = useAuth()
  const mode = useAgentPanelMode()
  const [sessions, setSessions] = useState<SessionInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [page, setPage] = useState(1)
  const hasMoreRef = useRef(false)
  const fetchingRef = useRef(false)
  const sessionsRef = useRef(sessions)
  sessionsRef.current = sessions

  useEffect(() => {
    if (enabled) return
    setPage(1)
  }, [enabled])

  useEffect(() => {
    if (!enabled) return
    if (!isAuthenticated) {
      setSessions([])
      setError(t.agentPanel.sessions.needLogin)
      hasMoreRef.current = false
      return
    }
    let cancelled = false
    fetchingRef.current = true
    void (async () => {
      try {
        const list = await agentService.listSessions(page, SESSION_PAGE)
        if (cancelled) return
        const fresh = list.filter(
          (item) =>
            item.messageCount > 0 && (item.mode ?? 'work') === mode,
        )
        const current = page === 1 ? null : sessionsRef.current
        const next =
          !current || page === 1
            ? fresh
            : current.concat(
                fresh.filter(
                  (item) => !current.some((seen) => seen.id === item.id),
                ),
              )
        sessionsRef.current = next
        hasMoreRef.current = list.length >= SESSION_PAGE
        setSessions(next)
        setError(null)
        if (hasMoreRef.current && next.length < SESSION_FILL) {
          setPage((value) => value + 1)
        }
      } catch {
        if (cancelled) return
        if (page === 1) {
          sessionsRef.current = []
          setSessions([])
        }
        hasMoreRef.current = false
        setError(t.agentPanel.sessions.loadFailed)
      } finally {
        fetchingRef.current = false
      }
    })()
    return () => {
      cancelled = true
    }
  }, [enabled, isAuthenticated, mode, page])

  const onNearStart = useCallback(() => {
    if (fetchingRef.current || !hasMoreRef.current) return
    fetchingRef.current = true
    setPage((current) => current + 1)
  }, [])

  const removeSession = useCallback(
    (id: string) => {
      setSessions((current) => {
        const next = current?.filter((item) => item.id !== id) ?? current
        sessionsRef.current = next
        return next
      })
      void agentService.archiveSession(id).catch(() => {
        setError(t.agentPanel.sessions.loadFailed)
      })
    },
    [t.agentPanel.sessions.loadFailed],
  )

  return { sessions, error, onNearStart, removeSession }
}

export const AgentPanelSessions: React.FC<AgentPanelSessionsProps> = ({
  activeSessionId,
  onSelect,
  exiting = false,
  sessions,
  error,
  onNearStart,
  removeSession,
}) => {
  const { t, format, locale } = useI18n()
  const listRef = useRef<HTMLDivElement>(null)
  const trackRef = useRef<HTMLDivElement>(null)

  useConversationPan(
    listRef,
    trackRef,
    !!sessions && sessions.length > 0,
    'sessions',
    '.agent-panel-session',
    onNearStart,
  )

  const describe = (session: SessionInfo): string => {
    const bucket = relativeTimeBucket(session.lastActiveAt, Date.now())
    if (!bucket) return ''
    switch (bucket.kind) {
      case 'justNow':
        return t.agentPanel.sessions.justNow
      case 'minutes':
        return format(t.agentPanel.sessions.minutesAgo, { value: bucket.value })
      case 'hours':
        return format(t.agentPanel.sessions.hoursAgo, { value: bucket.value })
      case 'days':
        return format(t.agentPanel.sessions.daysAgo, { value: bucket.value })
      default:
        return bucket.date.toLocaleDateString(locale || undefined)
    }
  }

  const visible = useMemo(
    () => (sessions ? sessions.toReversed() : []),
    [sessions],
  )

  return (
    <div
      className="agent-panel-messages-slot"
      data-exiting={exiting ? 'true' : undefined}
    >
      <div className="agent-panel-messages" ref={listRef}>
        <div className="agent-panel-messages-track" ref={trackRef}>
          <AgentPresence open={!!error} kind="row" from="composer">
            <span
              className="agent-panel-tag"
              data-block="true"
              data-tone="alert"
            >
              <span className="agent-panel-tag-text">{error}</span>
            </span>
          </AgentPresence>
          <AgentPresence
            open={!error && sessions !== null && sessions.length === 0}
            kind="row"
            from="composer"
          >
            <span className="agent-panel-tag" data-block="true">
              <span className="agent-panel-tag-text">
                {t.agentPanel.sessions.empty}
              </span>
            </span>
          </AgentPresence>
          <AgentPresenceList
            items={visible}
            keyOf={(session) => session.id}
            kind="row"
            from="composer"
          >
            {(session) => {
              const when = describe(session)
              return (
                <div
                  className="agent-panel-session glass"
                  data-active={
                    session.id === activeSessionId ? 'true' : 'false'
                  }
                >
                  <button
                    type="button"
                    className="agent-panel-session-open"
                    onClick={() => onSelect(session.id)}
                  >
                    <span className="agent-panel-session-title">
                      {session.title || t.agentPanel.sessions.untitled}
                    </span>
                    {when ? (
                      <span className="agent-panel-session-time">{when}</span>
                    ) : null}
                  </button>
                  <button
                    type="button"
                    className="agent-panel-tag-dismiss"
                    title={t.agentPanel.removeSession}
                    aria-label={t.agentPanel.removeSession}
                    onClick={() => {
                      if (!window.confirm(t.agentPanel.confirmRemoveSession)) {
                        return
                      }
                      removeSession(session.id)
                    }}
                  >
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      aria-hidden="true"
                    >
                      <path d="M6 6l12 12M18 6l-12 12" />
                    </svg>
                  </button>
                </div>
              )
            }}
          </AgentPresenceList>
        </div>
      </div>
    </div>
  )
}
