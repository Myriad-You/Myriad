import type { AgentAttachment } from './agentAttachments'
import type { AgentPanelFullView } from './AgentPanelFull'
import type { AgentPanelMode } from './agentPanelMode'
import type { AgentPanelPhase, AgentPanelStage } from './agentPanelStage'
import React, {
  useCallback,
  useEffect,
  useReducer,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { useLocation } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { useImmersiveChrome } from '../../contexts/NavigationContext'
import { executeFrontendAction } from '../../services/agent'
import {
  getNavLayoutSnapshot,
  getServerNavLayoutSnapshot,
  subscribeNavLayout,
} from '../../utils/navLayout'
import { useAgentMessageCount } from './agentMessages'
import { AgentPanelComposer } from './AgentPanelComposer'
import {
  AGENT_PANEL_CLOSE_EVENT,
  AGENT_PANEL_OPEN_EVENT,
  agentPanelOpenView,
  dispatchAgentPanelAction,
  dispatchAgentPanelSubmit,
  takeQueuedAgentPanelOpen,
} from './agentPanelEvents'
import { AgentPanelFull, AgentPanelSessionChrome } from './AgentPanelFull'
import { AgentPanelIntention } from './AgentPanelIntention'
import {
  cycleAgentPanelMode,
  setAgentPanelMode,
  shouldCaptureModeTab,
  useAgentPanelMode,
} from './agentPanelMode'
import { AgentPanelOverlay } from './AgentPanelOverlay'
import {
  agentPanelIsOpen,
  agentPanelSettleTimeoutMs,
  agentPanelShowsStage,
  agentPanelStageReducer,
  INITIAL_AGENT_PANEL_STAGE,
} from './agentPanelStage'
import { setAgentPanelVisible } from './agentPanelVisible'
import { clearAgentSelection, watchAgentSelection } from './agentSelection'
import { agentStatusForLane } from './agentStatus'
import {
  clearAgentUndoOffer,
  useAgentLaneLoading,
  useAgentPendingAction,
  useAgentStatus,
  useAgentUndoOffer,
} from './agentStatusStore'
import { useAgentAuroraPrism } from './useAgentAuroraPrism'
import { AgentPresence } from './useAgentPresence'
import { LONG_PRESS_DURATION, useLongPress } from './useLongPress'
import './agent-panel.css'

const AgentPanelAurora: React.FC<{
  phase: AgentPanelPhase
  stage: AgentPanelStage
}> = ({ phase, stage }) => {
  const mode = useAgentPanelMode()
  const island = useAgentStatus()
  const laneLoading = useAgentLaneLoading(mode)
  const status = agentStatusForLane(island.status, laneLoading)
  const auroraRef = useRef<HTMLDivElement>(null)
  const prismARef = useRef<HTMLSpanElement>(null)
  const prismBRef = useRef<HTMLSpanElement>(null)
  useAgentAuroraPrism(status, true, prismARef, prismBRef)

  useEffect(() => {
    const node = auroraRef.current
    if (!node) return
    const sync = () => {
      if (document.hidden) node.dataset.paused = 'true'
      else delete node.dataset.paused
    }
    sync()
    document.addEventListener('visibilitychange', sync)
    return () => document.removeEventListener('visibilitychange', sync)
  }, [])

  return (
    <div
      ref={auroraRef}
      className="agent-panel-aurora"
      data-phase={phase}
      data-stage={stage}
      data-status={status}
      aria-hidden="true"
    >
      <span className="agent-panel-aurora-flow" />
      <span ref={prismARef} className="agent-panel-aurora-prism" />
      <span ref={prismBRef} className="agent-panel-aurora-prism" />
      <span className="agent-panel-aurora-alert" />
    </div>
  )
}

export const AgentPanel: React.FC = () => {
  const { t } = useI18n()
  const location = useLocation()
  const [stage, dispatch] = useReducer(
    agentPanelStageReducer,
    INITIAL_AGENT_PANEL_STAGE,
  )
  const overlayRef = useRef<HTMLDivElement>(null)
  const open = agentPanelIsOpen(stage)
  const showsOverlay = agentPanelShowsStage(stage, 'overlay')
  const showsFull = agentPanelShowsStage(stage, 'full')
  const messageCount = useAgentMessageCount()

  useEffect(() => {
    const visible = stage.stage !== 'island'
    setAgentPanelVisible(visible)
    return () => setAgentPanelVisible(false)
  }, [stage.stage])

  const navLayout = useSyncExternalStore(
    subscribeNavLayout,
    getNavLayoutSnapshot,
    getServerNavLayoutSnapshot,
  )
  const pendingAction = useAgentPendingAction()
  const undoOffer = useAgentUndoOffer()
  const mode = useAgentPanelMode()
  const [fullView, setFullView] = useState<AgentPanelFullView>('messages')

  useEffect(() => {
    if (!showsFull) setFullView('messages')
  }, [showsFull])

  const showsComposer = !pendingAction && !(showsFull && fullView === 'manage')

  useEffect(() => {
    const queued = takeQueuedAgentPanelOpen()
    if (queued) {
      setFullView(queued.view)
      dispatch({ type: 'open', stage: queued.stage })
    }
    const onOpen = (event: Event) => {
      setFullView(agentPanelOpenView(event))
      dispatch({ type: 'open', stage: 'full' })
    }
    window.addEventListener(AGENT_PANEL_OPEN_EVENT, onOpen)
    return () => window.removeEventListener(AGENT_PANEL_OPEN_EVENT, onOpen)
  }, [])

  useEffect(() => {
    const onClose = () => dispatch({ type: 'close' })
    window.addEventListener(AGENT_PANEL_CLOSE_EVENT, onClose)
    return () => window.removeEventListener(AGENT_PANEL_CLOSE_EVENT, onClose)
  }, [])

  useEffect(() => {
    if (pendingAction) dispatch({ type: 'open', stage: 'overlay' })
  }, [pendingAction])

  useImmersiveChrome('agent-panel-overlay', navLayout === 'mobile' && open)

  const hasConversation = messageCount > 0
  const { indicator } = useLongPress(
    LONG_PRESS_DURATION,
    useCallback(
      () =>
        dispatch({
          type: 'open',
          stage: hasConversation ? 'full' : 'overlay',
        }),
      [hasConversation],
    ),
    !open,
  )

  // do not depend on message count: streaming tokens would cancel the double rAF
  useEffect(() => {
    if (stage.phase !== 'opening') return
    let inner = 0
    const outer = requestAnimationFrame(() => {
      inner = requestAnimationFrame(() => dispatch({ type: 'settle' }))
    })
    const fallback = setTimeout(dispatch, 80, { type: 'settle' })
    return () => {
      cancelAnimationFrame(outer)
      cancelAnimationFrame(inner)
      clearTimeout(fallback)
    }
  }, [stage.phase, stage.stage])

  useEffect(() => {
    if (stage.phase !== 'closing') return
    const timer = setTimeout(
      dispatch,
      agentPanelSettleTimeoutMs(
        stage.stage,
        showsFull && fullView === 'messages' ? messageCount : undefined,
      ),
      { type: 'settle' },
    )
    return () => clearTimeout(timer)
  }, [fullView, messageCount, showsFull, stage.phase, stage.stage])

  useEffect(() => watchAgentSelection(), [])

  useEffect(() => {
    dispatch({ type: 'close' })
    clearAgentSelection()
  }, [location.pathname])

  useEffect(() => {
    if (!open) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        dispatch({ type: 'close' })
        return
      }
      if (!showsComposer) return
      if (!shouldCaptureModeTab(event, overlayRef.current)) return
      event.preventDefault()
      cycleAgentPanelMode(event.shiftKey ? -1 : 1)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [open, showsComposer])

  useEffect(() => {
    if (!open) return
    const onPointerDown = (event: MouseEvent) => {
      const node = overlayRef.current
      const target = event.target
      if (target instanceof Element && target.closest('.tour-overlay')) return
      if (node && !node.contains(target as Node)) {
        dispatch({ type: 'close' })
      }
    }
    // same long-press mouseup would close the panel if this listener were immediate
    const timer = setTimeout(
      () => document.addEventListener('mousedown', onPointerDown),
      100,
    )
    return () => {
      clearTimeout(timer)
      document.removeEventListener('mousedown', onPointerDown)
    }
  }, [open])

  const submit = useCallback(
    (
      text: string,
      attachments?: AgentAttachment[],
      submitMode: AgentPanelMode = mode,
    ) => {
      if (submitMode !== mode) setAgentPanelMode(submitMode)
      dispatchAgentPanelSubmit(text, attachments, submitMode)
      setFullView('messages')
      dispatch({ type: 'open', stage: 'full' })
    },
    [mode],
  )

  const acceptIntention = useCallback((intentionId: string, input: string) => {
    setAgentPanelMode('work')
    dispatchAgentPanelSubmit(input, undefined, 'work', intentionId)
    setFullView('messages')
    dispatch({ type: 'open', stage: 'full' })
  }, [])

  const undo = useCallback(() => {
    if (!undoOffer) return
    clearAgentUndoOffer(undoOffer.id)
    dispatch({ type: 'close' })
    void executeFrontendAction(undoOffer.inverse)
  }, [undoOffer])

  const decide = useCallback(
    (approved: boolean) => {
      if (!pendingAction) return
      dispatchAgentPanelAction(pendingAction.id, approved)
      dispatch({ type: 'close' })
    },
    [pendingAction],
  )

  return (
    <>
      {(showsOverlay || showsFull) && (
        <>
          <AgentPanelAurora phase={stage.phase} stage={stage.stage} />
          <div
            ref={overlayRef}
            className="agent-panel-overlay-anchor"
            data-tour="home-agent-panel"
            data-phase={stage.phase}
            data-stage={stage.stage}
            data-mode={mode}
          >
            {showsFull ? (
              <AgentPanelFull
                view={fullView}
                onView={setFullView}
                onSubmit={submit}
                onWorkOffer={(input) => submit(input, undefined, 'work')}
                showChrome={fullView === 'manage'}
              />
            ) : (
              <AgentPanelOverlay
                pendingAction={pendingAction}
                onDecide={decide}
              />
            )}

            {!pendingAction && (!showsFull || fullView === 'messages') ? (
              <AgentPanelIntention
                enabled={open && stage.phase === 'settled'}
                onAccept={acceptIntention}
              />
            ) : null}

            {showsComposer && (
              <AgentPanelComposer
                onSubmit={submit}
                autoFocus={fullView !== 'sessions' && stage.phase === 'settled'}
                leading={
                  <>
                    <AgentPresence open={!!undoOffer} kind="chip" from="self">
                      {undoOffer ? (
                        <span className="agent-panel-tag" data-tone="primary">
                          <span className="agent-panel-tag-text">
                            {t.agentPanel.undo.did[undoOffer.actionType]}
                          </span>
                        </span>
                      ) : null}
                    </AgentPresence>
                    <AgentPresence open={!!undoOffer} kind="chip" from="self">
                      {undoOffer ? (
                        <button
                          type="button"
                          className="agent-panel-tag agent-panel-tag-strong"
                          data-tone="primary"
                          onClick={undo}
                        >
                          <span className="agent-panel-tag-text">
                            {t.agentPanel.undo.button}
                          </span>
                        </button>
                      ) : null}
                    </AgentPresence>
                  </>
                }
                trailing={
                  <>
                    <AgentPanelSessionChrome
                      view={showsFull ? fullView : 'messages'}
                      onView={(next) => {
                        setFullView(next)
                        if (next !== 'messages') {
                          dispatch({ type: 'open', stage: 'full' })
                        }
                      }}
                    />
                    <AgentPresence
                      open={!showsFull && messageCount > 0}
                      kind="chip"
                      from="self"
                    >
                      <button
                        type="button"
                        className="agent-panel-tag"
                        data-icon="true"
                        onClick={() =>
                          dispatch({ type: 'open', stage: 'full' })
                        }
                        title={t.agentPanel.expand}
                        aria-label={t.agentPanel.expand}
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
                          <path d="m6 15 6-6 6 6" />
                        </svg>
                      </button>
                    </AgentPresence>
                  </>
                }
              />
            )}
          </div>
        </>
      )}

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
    </>
  )
}

export default AgentPanel
