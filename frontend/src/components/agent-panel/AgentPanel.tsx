/**
 * Agent 面板外壳 —— 只管「现在展开到第几档」。
 *
 * Quick Overlay、Full 是同一块东西的两个大小，所以档位、手势、收起这些跨档的事
 * 集中在这里，各档自己只管长什么样。收起后底部不再留一枚状态胶囊。
 *
 * 输入那一行也归这里摆：它已经从卡片里搬出来，是卡片外面的输入框加一枚动作，两档
 * 共用同一行。「什么时候不该出现」（等人拍板、在设置那一面）于是也成了跨档的规矩。
 */

import type { AgentAttachment } from './agentAttachments'
import type { AgentPanelFullView } from './AgentPanelFull'
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
} from './agentPanelEvents'
import { AgentPanelFull, AgentPanelSessionChrome } from './AgentPanelFull'
import { AgentPanelIntention } from './AgentPanelIntention'
import type { AgentPanelMode } from './agentPanelMode'
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
  /**
   * Full 档正看着哪一面。
   *
   * 放在这里而不是 Full 自己身上，是因为输入那一行也归这里摆 —— 设置那一面没有
   * 「跟它说话」这回事，输入框得收起来，而收的人在外面。
   */
  const [fullView, setFullView] = useState<AgentPanelFullView>('messages')

  // 收起之后重新唤起，从对话那一面开始 —— 上次翻到设置页不该留到下一次
  useEffect(() => {
    if (!showsFull) setFullView('messages')
  }, [showsFull])

  /** 等人拍板时那一档整块让给操作卡片；设置那一面没有「跟它说话」这回事。 */
  const showsComposer = !pendingAction && !(showsFull && fullView === 'manage')

  // 通知中心这些外部入口：叫开面板，并落到它们想让人看的那一面
  useEffect(() => {
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

  // 要人拍板的时候自动展开。这不算抢占：确认是用户自己那条指令的下一步，
  // 而且它有时限，不展开就会过期。
  useEffect(() => {
    if (pendingAction) dispatch({ type: 'open', stage: 'overlay' })
  }, [pendingAction])

  // 移动端展开时给导航岛让位。
  useImmersiveChrome('agent-panel-overlay', navLayout === 'mobile' && open)

  // 档位跟着「有没有话要读」走：正说着的时候唤起，直接展开到能读的那一档，
  // 不该让人先看到一个空输入框再自己点开。
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

  // 入场：先落到 opening（整块锚点 opacity 0），下一帧再 settled，transition 才会播。
  // @starting-style 兜底初次挂上。不能把消息条数算进依赖 —— 流式追加会反复取消双 rAF，卡在 opening。
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

  // 退场停在 closing 等到卡片收完再卸。
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

  // 一直盯着选区。必须常驻 —— 长按那一下会把选区收掉，等面板开了再看就晚了。
  useEffect(() => watchAgentSelection(), [])

  // 换页面时收起：上下文都变了，开着的那句话已经不成立，记着的那段选中也是
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
    // 延后挂载：唤起用的那次长按会以 mouseup 收尾，立刻挂上会被同一串事件关掉
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
      // 展开到能读答案的那一档，而不是收起 —— 问完就把面板关掉等于让人白问
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

  // 撤销就是把逆操作再执行一遍 —— 不另起一套机制
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

            {/* 等人拍板、翻设置的时候没有话可说，这一行就不该杵在那儿 */}
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
