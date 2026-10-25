import type { AgentAttachment } from './agentAttachments'
import type { AgentPanelFullView } from './AgentPanelFull'
import type { AgentPanelMode } from './agentPanelMode'
import type { AgentPanelPhase, AgentPanelStage, AgentPanelStageAction } from './agentPanelStage'
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
  attachAgentPanelOpenQueue,
  dispatchAgentPanelSubmit,
} from './agentPanelEvents'
import { AgentPanelFull, AgentPanelSessionChrome } from './AgentPanelFull'
import { AgentPanelHerAway, useHerAway } from './AgentPanelHerAway'
import { AgentPanelHerTime } from './AgentPanelHerTime'
import { AgentPanelIntention } from './AgentPanelIntention'
import {
  cycleAgentPanelMode,
  setAgentPanelMode,
  shouldCaptureModeTab,
  useAgentPanelMode,
} from './agentPanelMode'
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
  useAgentStatusKind,
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
  const island = useAgentStatusKind()
  const laneLoading = useAgentLaneLoading(mode)
  const status = agentStatusForLane(island, laneLoading)
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

/**
 * `resident`: the composer lives inside another surface (the note editor) and never goes
 * away. Closing only folds the conversation back down to the composer; there is no aurora
 * over the page and no long-press to summon it.
 */
export const AgentPanel: React.FC<{ resident?: boolean }> = ({ resident = false }) => {
  const { t } = useI18n()
  const location = useLocation()
  const [stage, rawDispatch] = useReducer(
    agentPanelStageReducer,
    INITIAL_AGENT_PANEL_STAGE,
  )
  /**
   * Resident fold: the cards leave exactly as they do when the panel closes (the anchor
   * reads as closing, so the pan staggers them bottom-up), but the composer stays put.
   * Opening the conversation again mid-fold brings the cards straight back.
   */
  const [folding, setFolding] = useState(false)
  const foldTimer = useRef(0)
  const cancelFold = useCallback(() => {
    if (!foldTimer.current) return
    window.clearTimeout(foldTimer.current)
    foldTimer.current = 0
    setFolding(false)
  }, [])
  useEffect(() => () => window.clearTimeout(foldTimer.current), [])
  const dispatch = useCallback(
    (action: AgentPanelStageAction) => {
      if (action.type === 'open' && action.stage === 'full') cancelFold()
      rawDispatch(action)
    },
    [cancelFold],
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
  const undoOffer = useAgentUndoOffer()
  const mode = useAgentPanelMode()
  const [fullView, setFullView] = useState<AgentPanelFullView>('messages')

  useEffect(() => {
    if (!showsFull) setFullView('messages')
  }, [showsFull])

  // She is not on this site (H3e-9): no chat with her here; Work stays.
  const { away, device: awayOn } = useHerAway(open)
  const awayFromChat = away !== null && mode === 'chat'
  const showsComposer = !(showsFull && fullView === 'manage')

  useEffect(() => {
    const { queued, detach } = attachAgentPanelOpenQueue()
    if (queued) {
      setFullView(queued.view)
      dispatch({ type: 'open', stage: queued.stage })
    }
    const onOpen = (event: Event) => {
      setFullView(agentPanelOpenView(event))
      dispatch({ type: 'open', stage: 'full' })
    }
    window.addEventListener(AGENT_PANEL_OPEN_EVENT, onOpen)
    return () => {
      window.removeEventListener(AGENT_PANEL_OPEN_EVENT, onOpen)
      detach()
    }
  }, [])

  const showsFullRef = useRef(showsFull)
  showsFullRef.current = showsFull
  const messageCountRef = useRef(messageCount)
  messageCountRef.current = messageCount

  /** Folds the panel away: shut on the site, back to the composer when resident. */
  const collapse = useCallback(() => {
    if (!resident) {
      dispatch({ type: 'close' })
      return
    }
    if (!showsFullRef.current || foldTimer.current) return
    const still =
      document.documentElement.dataset.perfMode === 'exlight' ||
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    if (still) {
      dispatch({ type: 'open', stage: 'overlay' })
      return
    }
    setFolding(true)
    foldTimer.current = window.setTimeout(() => {
      foldTimer.current = 0
      setFolding(false)
      rawDispatch({ type: 'open', stage: 'overlay' })
    }, agentPanelSettleTimeoutMs('full', messageCountRef.current))
  }, [dispatch, resident])

  useEffect(() => {
    if (resident) dispatch({ type: 'open', stage: 'overlay' })
  }, [resident])

  useEffect(() => {
    const onClose = collapse
    window.addEventListener(AGENT_PANEL_CLOSE_EVENT, onClose)
    return () => window.removeEventListener(AGENT_PANEL_CLOSE_EVENT, onClose)
  }, [collapse])

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
    !open && !resident,
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

  // 只在路由真正变化时收起：面板常由一次打开请求触发首次挂载，
  // 挂载时也执行会把刚兑现的排队打开立刻关掉（首次打开不显示）。
  const shownPathname = useRef(location.pathname)
  useEffect(() => {
    if (shownPathname.current === location.pathname) return
    shownPathname.current = location.pathname
    collapse()
    clearAgentSelection()
  }, [collapse, location.pathname])

  useEffect(() => {
    if (!open) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        if (!resident || showsFull) collapse()
        return
      }
      if (!showsComposer || resident) return
      if (!shouldCaptureModeTab(event, overlayRef.current)) return
      event.preventDefault()
      cycleAgentPanelMode(event.shiftKey ? -1 : 1)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [collapse, open, resident, showsComposer, showsFull])

  useEffect(() => {
    if (!open) return
    const onPointerDown = (event: MouseEvent) => {
      const node = overlayRef.current
      const target = event.target
      if (target instanceof Element && target.closest('.tour-overlay')) return
      if (node && !node.contains(target as Node)) collapse()
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
  }, [collapse, open])

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
    collapse()
    void executeFrontendAction(undoOffer.inverse)
  }, [collapse, undoOffer])

  return (
    <>
      {(showsOverlay || showsFull) && (
        <>
          {resident ? null : <AgentPanelAurora phase={stage.phase} stage={stage.stage} />}
          <div
            ref={overlayRef}
            className="agent-panel-overlay-anchor"
            data-tour={resident ? undefined : 'home-agent-panel'}
            data-resident={resident ? 'true' : undefined}
            data-phase={folding ? 'closing' : stage.phase}
            data-stage={stage.stage}
            data-mode={mode}
          >
            {showsFull ? (
              <AgentPanelFull
                resident={resident}
                view={fullView}
                onView={setFullView}
                onSubmit={submit}
                onWorkOffer={(input) => submit(input, undefined, 'work')}
                showChrome={fullView === 'manage'}
              />
            ) : null}

            {!showsFull || fullView === 'messages' ? (
              <AgentPanelIntention
                enabled={open && stage.phase === 'settled'}
                onAccept={acceptIntention}
              />
            ) : null}

            {showsComposer && awayFromChat && away ? (
              <AgentPanelHerAway away={away} device={awayOn} mode={mode} />
            ) : null}
            {showsComposer && !awayFromChat && (
              <AgentPanelComposer
                resident={resident}
                onSubmit={submit}
                autoFocus={fullView !== 'sessions' && stage.phase === 'settled'}
                leading={
                  <>
                    <AgentPanelHerTime
                      open={!resident && open && stage.phase === 'settled' && !undoOffer}
                    />
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
                trailing={resident ? (
                  <AgentPresence open={messageCount > 0} kind="chip" from="self">
                    <button
                      type="button"
                      className="agent-panel-tag"
                      data-icon="true"
                      aria-expanded={showsFull && !folding}
                      onClick={() =>
                        showsFull && !folding
                          ? collapse()
                          : dispatch({ type: 'open', stage: 'full' })
                      }
                      title={showsFull && !folding ? t.agentPanel.collapse : t.agentPanel.expand}
                      aria-label={showsFull && !folding ? t.agentPanel.collapse : t.agentPanel.expand}
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
                        <path d={showsFull && !folding ? 'm6 9 6 6 6-6' : 'm6 15 6-6 6 6'} />
                      </svg>
                    </button>
                  </AgentPresence>
                ) : (
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
                )}
              />
            )}
          </div>
        </>
      )}

      {resident ? null : (
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
      )}
    </>
  )
}

export default AgentPanel
