/**
 * 执行引擎 —— 助手真正干活的地方，一个像素都不画。
 *
 * 会话持久化、SSE 流式进度、断线重连、敏感操作确认、错误兜底都在这里；结果全部
 * 写进 store，由 `AgentPanel` 那一层去画。**分开是因为这两件事的变更节奏完全不同**：
 * 界面会一改再改，而这套状态机是跑通过的，不该被布局调整牵连。
 *
 * 它挂在 App 根上，跟面板开没开无关 —— 面板收起来的时候任务照样跑完。
 */

import type React from 'react'

import type {
  AgentResponse,
  FrontendAction,
  MeropeStateChangedEvent,
  MusicControlEvent,
  OutfitOverlayEvent,
  PerformancePlanEvent,
  PlannerDecisionEvent,
  ProgressEvent,
  ProgressUpdateEvent,
  StepCompletedEvent,
  StepDebugEvent,
  StepStartedEvent,
  SummaryTokenEvent,
  TaskCreatedEvent,
  ThinkingTokenEvent,
} from '../../services/agent'
import type { AgentAttachment } from './agentAttachments'
import type { AgentPanelMode } from './agentPanelMode'
import type {
  ChatMessage,
  ChatSession,
  ExecutionTrace,
  PendingQuestion,
  TaskExecution,
} from './engineTypes'
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { useAuth } from '../../contexts/AuthContext'
import { agentMusicStatus } from '../../contexts/currentSong'
import { useI18n } from '../../contexts/I18nContext'
import { usePageContentOptional } from '../../contexts/PageContentContext'
import {
  clearChatOutfitOverlay,
  setChatOutfitOverlay,
  stripChatWearMarker,
} from '../../features/merope/chatOutfitOverlay'
import {
  attachLiveBody,
  captureTurnBody,
  deliverTurnLine,
  notePresenceRoute,
  openTurnReply,
  openTurnSpeech,
  setFaceMood,
  setTurnGeneration,
  startPresenceInbound,
  stopTurnSpeech,
  turnSpeechAlreadyFed,
} from '../../features/merope/engineFace'
import { interruptAgoraConversation, stopAgoraConversation } from '../../features/merope/speech/agoraConversation'
import { playbackDirection, retainPlaybackDirection, startPlaybackDirection } from '../../features/merope/motion/playbackDirectionHost'
import { bindRealtimeChat } from '../../features/merope/speech/realtimeChat'
import {
  beginTurnTrace,
  markTurnTrace,
  markTurnTraceOnce,
  noteTurnTraceDrop,
} from '../../features/merope/turnTrace'
import { sampleTurnTraceLeaks } from '../../features/merope/turnTraceSample'
import {
  agentService,
  executeFrontendAction,
  frontendActionDedupeKey,
  hasActionHandler,
} from '../../services/agent'
import {
  collectReattachCandidates,
  isNonTerminalTaskStatus,
} from '../../services/agent/reattach'
import {
  imageUrlsFromAgentPayload,
  messageFromStepOutput,
} from '../../services/agent/taskEnvelope'
import {
  ChatTurnClock,
  isCurrentChatGeneration,
  isStreamSupersededError,
  isUserInterruptError,
  nextAgentMessageId,
} from '../../services/agent/turnIdentity'
import { userFacingError } from '../../utils/userFacingError'
import {
  errorCode,
  generationFailureMessage,
} from '../agent/onboarding/generationError'
import { buildAgentPendingAction } from './agentAction'
import { attachmentsForRequest } from './agentAttachments'
import { getAgentContextConsent } from './agentContextConsent'
import { setAgentSessionId } from './agentMessages'
import {
  AGENT_PANEL_ACTION_EVENT,
  AGENT_PANEL_ANSWER_EVENT,
  AGENT_PANEL_COMMAND_EVENT,
  AGENT_PANEL_OPEN_SESSION_EVENT,
  AGENT_PANEL_SUBMIT_EVENT,
  agentPanelActionDetail,
  agentPanelAnswerDetail,
  agentPanelCommand,
  agentPanelOpenSessionId,
  agentPanelSubmitDetail,
  dispatchAgentPanelOpen,
} from './agentPanelEvents'
import { getAgentPanelMode, useAgentPanelMode } from './agentPanelMode'
import { turnSelectionText } from './agentSelection'
import {
  clearAgentPendingAction,
  pushAgentStatusEvent,
  resetAgentStatus,
  setAgentLaneLoading,
  setAgentPendingAction,
  setAgentStatusAwaitingConfirmation,
  setAgentStatusThinking,
  setAgentUndoOffer,
} from './agentStatusStore'
import {
  nonemptyContent,
  peelThoughtFromContent,
  splitThinkContent,
} from './agentThinking'
import { planAgentUndo } from './agentUndo'
import { executionStepsFromHistory } from './engineTypes'

import { syncProjectedMessages } from './projectAgentMessage'
import {
  pendingQuestionFromMetadata,
  restoreFollowUpQuestion,
  restorePendingActionFromMessages,
} from './sessionPendingRestore'
import { useMessageState } from './useMessageState'

function currentPath(): string {
  return `${window.location.pathname}${window.location.search}`
}

function finishTurnTrace(): void {
  sampleTurnTraceLeaks()
  markTurnTraceOnce('turn_completed')
}

/**
 * 执行一条 Agent 给的前端操作，顺带记下它能不能退回去。
 *
 * 用真实发生的路由变化来判断，不看指令声明的目标 —— 指令可能被处理器改写，也
 * 可能压根没跳成，给一个不管用的撤销比不给更糟。
 */
async function runFrontendAction(action: FrontendAction): Promise<unknown> {
  const beforePath = currentPath()
  const result = await executeFrontendAction(action)
  const offer = planAgentUndo({
    action,
    beforePath,
    afterPath: currentPath(),
    nowMs: Date.now(),
  })
  if (offer) setAgentUndoOffer(offer)
  return result
}

function chatPagePayload(
  page: Record<string, unknown>,
): Record<string, unknown> {
  const content = typeof page.content === 'string' ? page.content.slice(0, 400) : null
  return {
    type: page.type,
    title: page.title,
    author: page.author,
    content,
  }
}

export const AgentEngine: React.FC = () => {
  const location = useLocation()
  const { t, format, locale } = useI18n()
  const { isAuthenticated } = useAuth()
  const navigate = useNavigate()

  // 页面内容上下文
  const pageContentContext = usePageContentOptional()

  useEffect(() => startPresenceInbound(), [])
  useEffect(() => {
    if (isAuthenticated) return retainPlaybackDirection()
    playbackDirection.stop()
  }, [isAuthenticated])
  useEffect(() => {
    notePresenceRoute(location.pathname)
  }, [location.pathname])

  const [_isLoading, setIsLoading] = useState(false)
  const mode = useAgentPanelMode()

  const {
    messages,
    setMessages,
    messagesRef,
    findMessage,
    findMessageWhere,
    updateMessage,
    updateMessageExecution,
    addExecutionStep,
    updateExecutionStep,
  } = useMessageState(mode)

  const [sessionIdByMode, setSessionIdByMode] = useState<
    Record<AgentPanelMode, string | null>
  >({
    work: null,
    chat: null,
  })
  const sessionId = sessionIdByMode[mode]
  const setSessionId = useCallback(
    (value: string | null, forMode: AgentPanelMode = mode) => {
      setSessionIdByMode((prev) =>
        prev[forMode] === value ? prev : { ...prev, [forMode]: value },
      )
      sessionIdsByModeRef.current[forMode] = value
    },
    [mode],
  )
  const sessionIdsByModeRef = useRef<Record<AgentPanelMode, string | null>>({
    work: null,
    chat: null,
  })
  const loadingByModeRef = useRef<Record<AgentPanelMode, boolean>>({
    work: false,
    chat: false,
  })
  const loadingMessageIdByModeRef = useRef<
    Record<AgentPanelMode, string | null>
  >({
    work: null,
    chat: null,
  })
  const discardedResponseIdsRef = useRef(new Set<string>())

  const handleSendRef =
    useRef<
      (
        text: string,
        attachments?: readonly AgentAttachment[],
        mode?: AgentPanelMode,
        intentionId?: string,
      ) => Promise<void>
    >(null)

  // 把对话同步给新 UI 的 Full 层。只送「谁说的、说了什么、说完没有」，执行追踪
  // 那一堆留在这边 —— 新 UI 不该认识旧面板的消息模型。
  useLayoutEffect(() => {
    syncProjectedMessages(messages)
  }, [messages])

  // Refs
  const handleAgentResponseRef =
    useRef<
      (
        messageId: string,
        response: AgentResponse,
        mode?: AgentPanelMode,
        generation?: number,
      ) => Promise<void>
    >(null)
  const createProgressHandlerRef = useRef<
    ((assistantMessageId: string, mode?: AgentPanelMode, generation?: number,
      speechOutput?: 'local' | 'external', runId?: string) => (event: ProgressEvent) => void) | null
  >(null)
  const dispatchedFrontendKeysRef = useRef(new Map<string, Set<string>>())
  const frontendActionChainRef = useRef(new Map<string, Promise<void>>())
  const MAX_RESPONSE_GUARD_KEYS = 200

  const capSet = (set: Set<string>) => {
    while (set.size > MAX_RESPONSE_GUARD_KEYS) {
      const oldest = set.values().next().value
      if (oldest === undefined) break
      set.delete(oldest)
    }
  }

  const enqueueFrontendActions = useCallback(
    async (
      messageId: string,
      actions: Array<FrontendAction | null | undefined>,
    ): Promise<unknown[]> => {
      const visible: unknown[] = []
      const keys =
        dispatchedFrontendKeysRef.current.get(messageId) ?? new Set<string>()
      dispatchedFrontendKeysRef.current.set(messageId, keys)
      while (dispatchedFrontendKeysRef.current.size > MAX_RESPONSE_GUARD_KEYS) {
        const oldest = dispatchedFrontendKeysRef.current.keys().next().value
        if (oldest === undefined || oldest === messageId) break
        dispatchedFrontendKeysRef.current.delete(oldest)
        frontendActionChainRef.current.delete(oldest)
      }
      const run = async () => {
        for (const action of actions) {
          if (!action || typeof action !== 'object' || !('type' in action)) {
            continue
          }
          const key = frontendActionDedupeKey(action)
          if (keys.has(key)) continue
          keys.add(key)
          try {
            const result = await runFrontendAction(action)
            if (
              result &&
              typeof result === 'object' &&
              [
                'query_windows',
                'music_get_status',
                'show_data',
                'show_report',
              ].includes(action.type)
            ) {
              visible.push(result)
            }
          } catch (error) {
            console.error('[AgentEngine] Frontend action failed:', error)
          }
        }
      }
      const prev =
        frontendActionChainRef.current.get(messageId) ?? Promise.resolve()
      const next = prev.then(run, run)
      frontendActionChainRef.current.set(
        messageId,
        next.then(
          () => undefined,
          () => undefined,
        ),
      )
      await next
      return visible
    },
    [],
  )
  const answerQuestionRef =
    useRef<(messageId: string, answer: string) => void>(null)
  const sessionTitleSetByModeRef = useRef<Record<AgentPanelMode, boolean>>({
    work: false,
    chat: false,
  })
  const handledResponseKeysRef = useRef(new Set<string>())
  const chatTurnClockRef = useRef(new ChatTurnClock())

  // 检测是否有待回答的问题（用于将主输入框路由到回答逻辑）
  const pendingAnswerMsg = useMemo(() => {
    // 从后往前找第一个有 pendingQuestion 且未回答的消息
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i]
      if (
        m.pendingQuestion &&
        !m.selectedAnswer &&
        m.taskExecution?.status === 'waiting'
      ) {
        return m
      }
    }
    return null
  }, [messages])

  // 会话管理

  const startNewSession = useCallback(async () => {
    // 新建会话只切换前端视图。旧任务由后端 run 持续执行，并通过通知中心报告状态。
    const current = getAgentPanelMode()
    if (current === 'chat') void stopAgoraConversation()
    const loadingId = loadingMessageIdByModeRef.current[current]
    if (loadingId) {
      discardedResponseIdsRef.current.add(loadingId)
      capSet(discardedResponseIdsRef.current)
      stopTurnSpeech(loadingId)
    }
    loadingByModeRef.current[current] = false
    loadingMessageIdByModeRef.current[current] = null
    setAgentLaneLoading(current, false)
    setIsLoading(loadingByModeRef.current.work || loadingByModeRef.current.chat)
    const otherRunning =
      current === 'chat'
        ? loadingByModeRef.current.work
        : loadingByModeRef.current.chat
    resetAgentStatus()
    if (otherRunning) setAgentStatusThinking()
    sessionIdsByModeRef.current[current] = null
    setSessionId(null, current)
    setMessages([], current)
    sessionTitleSetByModeRef.current[current] = false
    if (current === 'chat') clearChatOutfitOverlay()
  }, [setSessionId, setMessages])

  /**
   * 将已加载会话中的非终态任务重新挂到 UI，并订阅 run 进度流。
   * 不重新 POST process；仅 GET run stream / task 状态。
   * Candidate 合并逻辑见 `collectReattachCandidates`（跨消息补 runId、runId-only 通知）。
   */
  const reattachLiveWork = useCallback(
    async (
      messagesToScan: ChatMessage[],
      hints?: { runId?: string; taskId?: string },
    ) => {
      const candidates = collectReattachCandidates(
        messagesToScan.map((m) => ({
          id: m.id,
          role: m.role,
          taskId: m.taskExecution?.taskId,
          runId: m.taskExecution?.runId,
        })),
        hints,
      )

      for (const candidate of candidates) {
        try {
          const taskId = candidate.taskId
          const runId = candidate.runId
          let progress = 0
          let isWaiting = false
          let pendingQ: PendingQuestion | undefined

          if (taskId) {
            const task = await agentService.getTask(taskId)
            if (!isNonTerminalTaskStatus(task.status)) continue
            isWaiting = task.status === 'waiting_for_input'
            progress = task.progress ?? 0
            const existing = messagesToScan.find(
              (message) => message.id === candidate.messageId,
            )?.pendingQuestion
            if (task.pendingQuestion) {
              pendingQ = {
                questionId: task.pendingQuestion.questionId,
                confirmationId: existing?.confirmationId,
                questionType: task.pendingQuestion.questionType,
                question: task.pendingQuestion.question,
                context: task.pendingQuestion.context,
                options: task.pendingQuestion.options,
                required: task.pendingQuestion.required,
                defaultValue: task.pendingQuestion.defaultValue,
                riskLevel: existing?.riskLevel,
                expiresInSeconds: existing?.expiresInSeconds,
                receivedAtMs: existing?.receivedAtMs,
                pendingSteps: existing?.pendingSteps,
              }
            } else if (existing) {
              pendingQ = existing
            }
          } else if (!runId) {
            continue
          }

          // runId-only：没有 task 时也挂 processing，靠 SSE 回放补全
          updateMessage(candidate.messageId, {
            pendingQuestion: pendingQ,
            taskExecution: {
              taskId: taskId || '',
              runId,
              status: isWaiting ? 'waiting' : 'processing',
              progress,
              steps: [],
            },
          })

          if (!runId) continue

          loadingMessageIdByModeRef.current.work = candidate.messageId
          loadingByModeRef.current.work = true
          setAgentLaneLoading('work', true)
          setIsLoading(true)
          setAgentStatusThinking()
          const onProgress = createProgressHandlerRef.current?.(
            candidate.messageId,
            'work', 0, 'local', runId,
          )
          if (!onProgress) continue
          void agentService
            .subscribeRun(runId, onProgress)
            .then((response) =>
              handleAgentResponseRef.current?.(candidate.messageId, response),
            )
            .catch((error) => {
              stopTurnSpeech(candidate.messageId)
              console.warn('[AgentEngine] reattach stream ended:', error)
            })
            .finally(() => {
              if (
                loadingMessageIdByModeRef.current.work === candidate.messageId
              ) {
                loadingByModeRef.current.work = false
                loadingMessageIdByModeRef.current.work = null
                setAgentLaneLoading('work', false)
              }
              setIsLoading(
                loadingByModeRef.current.work || loadingByModeRef.current.chat,
              )
            })
          // 同一时刻只恢复一条 live stream
          break
        } catch (error) {
          console.warn('[AgentEngine] reattach task probe failed:', error)
        }
      }
    },
    [updateMessage],
  )

  const loadSession = useCallback(
    async (
      session: ChatSession,
      reattachHints?: { runId?: string; taskId?: string },
      requestedMode: AgentPanelMode = session.mode ?? getAgentPanelMode(),
    ) => {
      if (requestedMode === 'chat' && sessionIdsByModeRef.current.chat !== session.id) {
        void stopAgoraConversation()
      }
      sessionIdsByModeRef.current[requestedMode] = session.id
      setSessionId(session.id, requestedMode)
      sessionTitleSetByModeRef.current[requestedMode] = !!session.title

      try {
        const sessionMessages = await agentService.getSessionMessages(
          session.id,
          1,
          50,
        )
        const loaded: ChatMessage[] = sessionMessages.map((m, idx) => {
          const meta = m.metadata as Record<string, unknown> | undefined
          const data = meta?.data
          const stepHistory = (
            meta?.task as Record<string, unknown> | undefined
          )?.stepHistory as Array<Record<string, unknown>> | undefined
          const imageUrls = imageUrlsFromAgentPayload(data, stepHistory)

          const metaTaskId =
            (typeof meta?.taskId === 'string' && meta.taskId) ||
            (typeof meta?.task_id === 'string' && meta.task_id) ||
            m.taskId ||
            undefined
          const metaRunId =
            (typeof meta?.runId === 'string' && meta.runId) ||
            (typeof meta?.run_id === 'string' && meta.run_id) ||
            undefined
          const taskMeta = meta?.task as Record<string, unknown> | undefined
          const statusFromMeta =
            typeof taskMeta?.status === 'string' ? taskMeta.status : undefined
          const historySteps = executionStepsFromHistory(stepHistory)

          let taskExecution: TaskExecution | undefined
          if (metaTaskId || metaRunId || historySteps.length) {
            const waiting =
              statusFromMeta === 'waiting_for_input' ||
              !!taskMeta?.pendingQuestion
            taskExecution = {
              taskId: metaTaskId || '',
              runId: metaRunId,
              status: waiting ? 'waiting' : 'completed',
              progress:
                typeof taskMeta?.progress === 'number'
                  ? (taskMeta.progress as number)
                  : waiting
                    ? 50
                    : 100,
              steps: historySteps,
            }
          }

          // 从持久化 metadata 恢复等待中的问题（reattach 会再与后端对齐）
          const pendingQuestion = pendingQuestionFromMetadata(
            meta,
            new Date(m.createdAt).getTime(),
          )
          if (pendingQuestion && taskExecution) taskExecution.status = 'waiting'

          return {
            id: `loaded_${m.id}_${idx}`,
            sessionId: session.id,
            role: m.role as ChatMessage['role'],
            content: m.content,
            createdAt: new Date(m.createdAt),
            suggestions: meta?.suggestions as string[] | undefined,
            data: data ?? undefined,
            imageUrls: imageUrls.length > 0 ? imageUrls : undefined,
            taskExecution,
            pendingQuestion,
          }
        })
        setMessages(loaded, requestedMode)
        if (requestedMode === 'work') {
          const action = restorePendingActionFromMessages(loaded, Date.now())
          if (action) {
            setAgentPendingAction(action)
          } else {
            clearAgentPendingAction()
            const followUp = restoreFollowUpQuestion(loaded)
            if (followUp) setAgentStatusAwaitingConfirmation(followUp)
          }
        }
        // 刷新 / 通知打开：探测非终态任务并 re-subscribe
        void reattachLiveWork(loaded, reattachHints)
      } catch (error) {
        console.error('[AgentEngine] 加载会话消息失败:', error)
      }
    },
    [reattachLiveWork],
  )

  // 外部打开指定会话（通知中心点击任务通知跳转，可带 runId/taskId）
  useEffect(() => {
    const handleOpenSession = (e: Event) => {
      const detail = (e as CustomEvent).detail as {
        sessionId?: string
        runId?: string
        taskId?: string
      } | null
      const sid = detail?.sessionId
      // 通知中心还在发这条旧事件：改成把新面板叫出来，会话仍然由这边去取
      dispatchAgentPanelOpen('messages')
      if (typeof sid !== 'string' || !sid) return
      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.AGENT_OPEN, {
            target: 'session',
            throttleMs: 5000,
          })
        },
      )
      void loadSession(
        {
          id: sid,
          title: null,
          messageCount: 0,
          lastActiveAt: '',
        },
        {
          runId: typeof detail?.runId === 'string' ? detail.runId : undefined,
          taskId:
            typeof detail?.taskId === 'string' ? detail.taskId : undefined,
        },
        'work',
      )
    }
    window.addEventListener('arael-open-session', handleOpenSession)
    return () =>
      window.removeEventListener('arael-open-session', handleOpenSession)
  }, [loadSession])

  useEffect(() => {
    const handleOpenManage = () => {
      dispatchAgentPanelOpen('manage')
      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.AGENT_OPEN, {
            target: 'manage',
            throttleMs: 5000,
          })
        },
      )
      // 具体看哪一面由 AgentPanel 那边的 requestedView 决定
    }
    window.addEventListener('arael-open-manage', handleOpenManage)
    return () =>
      window.removeEventListener('arael-open-manage', handleOpenManage)
  }, [])

  // Quick Overlay 只负责把话递过来，执行仍然在这边：打开自己，照常发送。
  // 等 Full 层重做完，接住这条事件的换成新面板，overlay 那边不用改。
  useEffect(() => {
    const handleSubmit = (event: Event) => {
      const detail = agentPanelSubmitDetail(event)
      if (!detail) return
      // 不再把自己显示出来 —— 新 UI 的 Full 层已经在画这段对话了，
      // 两个面板同时开着只会让人不知道该看哪个。这边只管跑。
      void handleSendRef.current?.(
        detail.text,
        detail.attachments,
        detail.mode,
        detail.intentionId,
      )
    }
    window.addEventListener(AGENT_PANEL_SUBMIT_EVENT, handleSubmit)
    return () =>
      window.removeEventListener(AGENT_PANEL_SUBMIT_EVENT, handleSubmit)
  }, [])

  // 操作卡片上按的那一下。卡片只递决定，真正调 /agent/confirm/stream 的仍然是这里，
  // 过期校验、进度流、失败兜底都在原来那条路上。
  useEffect(() => {
    const handleDecision = (event: Event) => {
      const detail = agentPanelActionDetail(event)
      if (!detail) return
      clearAgentPendingAction(detail.id)
      const target = findMessageWhere(
        (message) =>
          message.pendingQuestion?.confirmationId === detail.id &&
          !message.selectedAnswer,
      )
      if (!target) return
      answerQuestionRef.current?.(
        target.id,
        detail.approved ? 'confirm' : 'cancel',
      )
    }
    window.addEventListener(AGENT_PANEL_ACTION_EVENT, handleDecision)
    return () =>
      window.removeEventListener(AGENT_PANEL_ACTION_EVENT, handleDecision)
  }, [findMessageWhere])

  // 界面上点的那个选项。走的是和打字回答同一条路。
  useEffect(() => {
    const handleAnswer = (event: Event) => {
      const detail = agentPanelAnswerDetail(event)
      if (!detail) return
      answerQuestionRef.current?.(detail.messageId, detail.answer)
    }
    window.addEventListener(AGENT_PANEL_ANSWER_EVENT, handleAnswer)
    return () =>
      window.removeEventListener(AGENT_PANEL_ANSWER_EVENT, handleAnswer)
  }, [])

  useEffect(() => {
    return attachLiveBody()
  }, [])

  // 新 UI 的历史列表挑了一条。取消息、重连进行中的任务都还是这边的活。
  useEffect(() => {
    const handleOpenSession = (event: Event) => {
      const id = agentPanelOpenSessionId(event)
      if (!id) return
      const mode = getAgentPanelMode()
      void loadSession({
        id,
        mode,
        title: null,
        messageCount: 0,
        lastActiveAt: '',
      })
    }
    window.addEventListener(AGENT_PANEL_OPEN_SESSION_EVENT, handleOpenSession)
    return () =>
      window.removeEventListener(
        AGENT_PANEL_OPEN_SESSION_EVENT,
        handleOpenSession,
      )
  }, [loadSession])

  // 当前是哪一条会话，历史列表要靠它标出「就是这条」
  useEffect(() => {
    setAgentSessionId(sessionId)
  }, [sessionId])

  // 中断

  const interruptCurrentTask = useCallback(async () => {
    const current = getAgentPanelMode()
    // Only abort this mode's SSE. Work and Chat can be in flight together.
    agentService.abortCurrentRequest(current)
    if (current === 'chat') {
      void interruptAgoraConversation()
      const sessionId = sessionIdsByModeRef.current.chat
      void agentService.cancelChatTurn(sessionId || '')
      const generation = chatTurnClockRef.current.next()
      setTurnGeneration(generation)
    }

    const discardedId = loadingMessageIdByModeRef.current[current]
    if (discardedId) discardedResponseIdsRef.current.add(discardedId)

    const processingMsgs = messagesRef.current[current].filter(
      (m) =>
        m.taskExecution?.status === 'processing' ||
        m.taskExecution?.status === 'waiting' ||
        m.taskExecution?.status === 'cancelling',
    )
    for (const msg of processingMsgs) {
      discardedResponseIdsRef.current.add(msg.id)
      stopTurnSpeech(msg.id)
    }
    capSet(discardedResponseIdsRef.current)

    // Drop occupancy before awaiting cancel, otherwise a late token writes
    // thinking back. Always idle the island; if the other lane is still in
    // flight, put thinking back so that lane's stop button still has a home.
    loadingByModeRef.current[current] = false
    loadingMessageIdByModeRef.current[current] = null
    setAgentLaneLoading(current, false)
    setIsLoading(loadingByModeRef.current.work || loadingByModeRef.current.chat)
    const otherRunning =
      current === 'chat'
        ? loadingByModeRef.current.work
        : loadingByModeRef.current.chat
    resetAgentStatus()
    if (otherRunning) setAgentStatusThinking()

    for (const msg of processingMsgs) {
      const taskId = msg.taskExecution?.taskId
      updateMessage(msg.id, {
        taskExecution: msg.taskExecution
          ? { ...msg.taskExecution, status: 'error' }
          : undefined,
        content: msg.content || t.agentPanel.interrupted,
      })
      if (taskId && !taskId.startsWith('confirmation:')) {
        try {
          await agentService.cancelTask(taskId)
        } catch {
          updateMessage(msg.id, {
            content:
              msg.content ||
              `${t.agentPanel.interrupted} (${t.agentPanel.cancelFailed})`,
          })
        }
      }
    }
  }, [messagesRef, updateMessage, updateMessageExecution, t])
  // 界面上按的「开新对话」「停下」。真正的动作在这边，界面只递一个意思。
  useEffect(() => {
    const handleCommand = (event: Event) => {
      const command = agentPanelCommand(event)
      if (command === 'new-session') void startNewSession()
      if (command === 'interrupt') void interruptCurrentTask()
    }
    window.addEventListener(AGENT_PANEL_COMMAND_EVENT, handleCommand)
    return () =>
      window.removeEventListener(AGENT_PANEL_COMMAND_EVENT, handleCommand)
  }, [startNewSession, interruptCurrentTask])

  // SSE 进度处理

  const createProgressHandler = useCallback(
    (
      assistantMessageId: string,
      mode: AgentPanelMode = 'work',
      generation = 0,
      speechOutput: 'local' | 'external' = 'local',
      initialRunId?: string,
    ) => {
      let streamedSummary = ''
      let streamedThinking = ''
      let performancePlanCount = 0
      let performanceRunId = initialRunId
      let notedStaleGeneration = false
      let liveTaskId = ''
      const speech = openTurnSpeech(assistantMessageId, generation, locale, speechOutput)
      const utterance = openTurnReply(
        mode,
        assistantMessageId,
        locale,
        speechOutput,
      )

      const publishThinking = (text: string) => {
        streamedThinking = text
        updateMessageExecution(assistantMessageId, { reasoning: text })
      }

      return (event: ProgressEvent) => {
        if (
          mode === 'chat' &&
          !isCurrentChatGeneration(generation, chatTurnClockRef.current.current())
        ) {
          if (!notedStaleGeneration) {
            notedStaleGeneration = true
            noteTurnTraceDrop('stale_generation')
          }
          return
        }
        if (loadingMessageIdByModeRef.current[mode] !== assistantMessageId) {
          return
        }
        // 岛与面板读同一份状态：这里是唯一的入口，别处不再解读 SSE
        pushAgentStatusEvent(event)
        // 记录关键 SSE 事件到调试日志
        switch (event.type) {
          case 'run_started': {
            if (event.sessionId) {
              sessionIdsByModeRef.current[mode] = event.sessionId
              setSessionId(event.sessionId, mode)
            }
            if (event.runId) {
              performanceRunId = event.runId
              if (mode === 'chat') startPlaybackDirection({ runId: event.runId, messageId: assistantMessageId, generation })
              updateMessageExecution(assistantMessageId, {
                runId: event.runId,
              })
            }
            break
          }

          case 'session_created': {
            sessionIdsByModeRef.current[mode] = event.sessionId
            setSessionId(event.sessionId, mode)
            break
          }

          case 'session_title_updated': {
            // 后端并行 AI 生成的标题通过 SSE 推送
            if (event.title) {
              sessionTitleSetByModeRef.current[mode] = true
            }
            break
          }

          case 'task_created': {
            const tcEvent = event as TaskCreatedEvent
            liveTaskId = tcEvent.taskId
            const execUpdates: Partial<TaskExecution> = {
              taskId: tcEvent.taskId,
              progress: 5,
            }
            if (tcEvent.skillId) {
              execUpdates.skillId = tcEvent.skillId
              execUpdates.skillName = tcEvent.skillName
            }
            if (tcEvent.queuePosition != null && tcEvent.queuePosition > 0) {
              execUpdates.queuePosition = tcEvent.queuePosition
            }

            // 存储计划步骤描述（用于前端显示执行计划概览）
            if (
              tcEvent.stepDescriptions &&
              tcEvent.stepDescriptions.length > 0
            ) {
              execUpdates.planStepDescriptions = tcEvent.stepDescriptions
            }

            updateMessageExecution(assistantMessageId, execUpdates)
            // content 留空 — 进度信息由 live steps 展示，避免与步骤进度重复
            break
          }

          case 'task_assigned': {
            const assignEvent =
              event as import('../../services/agent/types').TaskAssignedEvent
            updateMessageExecution(assistantMessageId, {
              assignment: assignEvent.assignment,
            })
            break
          }

          case 'step_started': {
            utterance.end()
            streamedSummary = '' // ai_summarize 从零开始，替换 announce_plan
            const stepEvent = event as StepStartedEvent
            addExecutionStep(assistantMessageId, {
              id: stepEvent.stepId,
              name: stepEvent.description,
              status: 'running',
              stepIndex: stepEvent.stepIndex,
              totalSteps: stepEvent.totalSteps,
              capabilityCategory: stepEvent.capabilityCategory,
              retryAttempt: stepEvent.retryAttempt,
            })
            updateMessageExecution(assistantMessageId, {
              queuePosition: 0,
            })
            break
          }

          case 'step_completed': {
            const stepEvent = event as StepCompletedEvent
            updateExecutionStep(assistantMessageId, stepEvent.stepId, {
              status: stepEvent.success ? 'completed' : 'error',
              message: stepEvent.outputSummary,
              tierUsed: stepEvent.tierUsed,
              degraded: stepEvent.degraded,
              durationMs: stepEvent.durationMs,
              imageUrl: stepEvent.imageUrl,
            })
            if (stepEvent.imageUrl) {
              setMessages(
                (prev) =>
                  prev.map((m) =>
                    m.id === assistantMessageId
                      ? {
                          ...m,
                          imageUrls: [
                            ...(m.imageUrls || []).filter(
                              (u) => u !== stepEvent.imageUrl,
                            ),
                            stepEvent.imageUrl!,
                          ],
                        }
                      : m,
                  ),
                mode,
              )
            }
            const frontendActions = stepEvent.frontendActions
            if (frontendActions && frontendActions.length > 0) {
              void (async () => {
                const results = await enqueueFrontendActions(
                  assistantMessageId,
                  frontendActions,
                )
                const needsAck = frontendActions.some(
                  (action) =>
                    action &&
                    ['query_windows', 'music_get_status'].includes(action.type),
                )
                if (!needsAck || !liveTaskId) return
                let musicStatus: unknown
                let windowState: unknown
                for (const result of results) {
                  if (!result || typeof result !== 'object') continue
                  const row = result as Record<string, unknown>
                  if ('isPlaying' in row || 'isEnabled' in row) {
                    musicStatus = result
                  }
                  if ('windows' in row || 'available' in row) {
                    windowState = result
                  }
                }
                const published = (
                  window as unknown as {
                    __musicPlayerState?: {
                      isPlaying?: boolean
                      isEnabled?: boolean
                      currentSong?: unknown
                      currentSongIndex?: number
                      playlistLength?: number
                    }
                  }
                ).__musicPlayerState
                if (!musicStatus && published) {
                  musicStatus = agentMusicStatus(
                    published as Record<string, unknown>,
                  )
                }
                await agentService.submitFrontendAck(
                  liveTaskId,
                  stepEvent.stepId,
                  { musicStatus, windowState },
                )
              })()
            }
            break
          }

          case 'step_retrying': {
            const retryEvent =
              event as import('../../services/agent/types').StepRetryingEvent
            // 更新步骤状态为重试中
            updateExecutionStep(assistantMessageId, retryEvent.stepId, {
              status: 'running',
              message: `${retryEvent.reason} (${retryEvent.retryCount}/${retryEvent.maxRetries})`,
              retryAttempt: retryEvent.retryCount,
            })
            break
          }

          case 'progress': {
            const progressEvent = event as ProgressUpdateEvent
            updateMessageExecution(assistantMessageId, {
              progress: progressEvent.progress,
              ...(progressEvent.message?.trim()
                ? { statusMessage: progressEvent.message }
                : {}),
            })
            break
          }

          case 'waiting_for_input': {
            const wEvent =
              event as import('../../services/agent/types').WaitingForInputEvent
            const pendingQ: import('./engineTypes').PendingQuestion = {
              questionId: wEvent.questionId,
              questionType: wEvent.questionType,
              question: wEvent.question,
              context: wEvent.context,
              options: wEvent.options,
              required: wEvent.required,
              defaultValue: wEvent.defaultValue,
            }
            updateMessage(assistantMessageId, {
              pendingQuestion: pendingQ,
              selectedAnswer: undefined,
            })
            updateMessageExecution(assistantMessageId, {
              status: 'waiting',
              taskId: wEvent.taskId,
            })
            break
          }

          case 'error':
            playbackDirection.cancel(assistantMessageId)
            utterance.cancel()
            speech.cancel()
            updateMessageExecution(assistantMessageId, { status: 'error' })
            updateMessage(assistantMessageId, { content: event.message })
            break

          case 'thinking_token': {
            const tokenEvent = event as ThinkingTokenEvent
            if (!tokenEvent.done && tokenEvent.token) {
              publishThinking(streamedThinking + tokenEvent.token)
            }
            break
          }

          case 'summary_token': {
            const tokenEvent = event as SummaryTokenEvent
            if (tokenEvent.done) {
              const split = splitThinkContent(streamedSummary)
              if (split.thought && split.thought !== streamedThinking) {
                publishThinking(split.thought)
              }
              const body = nonemptyContent(
                stripChatWearMarker(
                  peelThoughtFromContent(split.content, streamedThinking),
                ),
              )
              if (body) {
                updateMessageExecution(assistantMessageId, {
                  statusMessage: body,
                })
                updateMessage(assistantMessageId, { content: body })
              }
              if (speech.end()) markTurnTraceOnce('first_sentence')
              utterance.end()
              if (mode === 'chat') playbackDirection.textEnded(assistantMessageId)
            } else {
              if (tokenEvent.token) markTurnTraceOnce('llm_first_token')
              streamedSummary += tokenEvent.token
              const split = splitThinkContent(streamedSummary)
              if (split.thought && split.thought !== streamedThinking) {
                publishThinking(split.thought)
              }
              const body = nonemptyContent(
                stripChatWearMarker(
                  peelThoughtFromContent(split.content, streamedThinking),
                ),
              )
              if (body) {
                const sealed = speech.push(tokenEvent.token)
                if (sealed == null) utterance.chunk(tokenEvent.token)
                else if (sealed > 0) markTurnTraceOnce('first_sentence')
                updateMessage(assistantMessageId, { content: body })
              }
            }
            break
          }

          case 'merope_state_changed': {
            const stateEvent = event as MeropeStateChangedEvent
            setFaceMood(stateEvent.mood, stateEvent.activity)
            break
          }

          case 'outfit_overlay': {
            const overlayEvent = event as OutfitOverlayEvent
            setChatOutfitOverlay(overlayEvent.outfitId)
            break
          }

          case 'music_control': {
            const musicEvent = event as MusicControlEvent
            const action =
              musicEvent.action === 'prev' ? 'previous' : musicEvent.action
            if (
              action === 'play' ||
              action === 'pause' ||
              action === 'toggle' ||
              action === 'next' ||
              action === 'previous'
            ) {
              void executeFrontendAction({
                type: 'music_control',
                action,
                timestamp: Date.now(),
              })
            }
            break
          }

          case 'performance_plan': {
            const performanceEvent = event as PerformancePlanEvent
            const performancePhase = performanceEvent.performance.phase
            performancePlanCount += 1
            markTurnTrace('performance_received', {
              phase: performancePhase, plan: performancePlanCount,
              ...(performanceRunId ? { runId: performanceRunId } : {}),
            })
            if (performancePhase === 'reaction') {
              markTurnTraceOnce('reaction_ready', { phase: performancePhase })
            } else if (performancePhase === 'delivery') {
              markTurnTraceOnce('delivery_ready', {
                phase: performancePhase,
                plan: performancePlanCount,
              })
            }
            deliverTurnLine(mode, {
              messageId: assistantMessageId,
              runId: performanceRunId,
              performance: performanceEvent.performance,
            })
            break
          }

          case 'task_completed': {
            utterance.end()
            if (mode === 'chat') playbackDirection.textEnded(assistantMessageId)
            // 检查任务是否真正完成（多轮问答时可能仍在等待用户输入）
            const completedEvent =
              event as import('../../services/agent/types').TaskCompletedEvent
            const taskInfo = completedEvent.response?.task as
              Record<string, unknown> | undefined
            const isStillWaiting = taskInfo?.status === 'waiting_for_input'

            if (!isStillWaiting) {
              // 任务真正完成：清除 pendingQuestion、更新状态、确保 isLoading 归位
              updateMessage(assistantMessageId, {
                pendingQuestion: undefined,
                selectedAnswer: undefined,
              })
              updateMessageExecution(assistantMessageId, {
                status: completedEvent.success ? 'completed' : 'error',
                progress: 100,
              })
              if (
                loadingMessageIdByModeRef.current[mode] === assistantMessageId
              ) {
                loadingMessageIdByModeRef.current[mode] = null
                loadingByModeRef.current[mode] = false
                setAgentLaneLoading(mode, false)
                setIsLoading(
                  loadingByModeRef.current.work ||
                    loadingByModeRef.current.chat,
                )
              }
            }
            break
          }

          case 'planner_decision': {
            const pdEvent = event as PlannerDecisionEvent
            if (pdEvent.reasoning && !streamedThinking) {
              publishThinking(pdEvent.reasoning)
            }
            setMessages(
              (prev) =>
                prev.map((m) => {
                  if (m.id !== assistantMessageId || !m.taskExecution) return m
                  const existing = m.taskExecution.debugTrace ?? {
                    stepDebugEntries: [],
                  }
                  const planned =
                    m.taskExecution.planStepDescriptions ??
                    pdEvent.steps
                      .map((step) => (step.action || step.capabilityId).trim())
                      .filter(Boolean)
                  return {
                    ...m,
                    taskExecution: {
                      ...m.taskExecution,
                      ...(planned.length
                        ? { planStepDescriptions: planned }
                        : {}),
                      debugTrace: {
                        ...existing,
                        plannerDecision: {
                          status: pdEvent.status,
                          reasoning: pdEvent.reasoning,
                          confidence: pdEvent.confidence,
                          steps: pdEvent.steps,
                          userRequest: pdEvent.userRequest,
                        },
                      },
                    },
                  }
                }),
              mode,
            )
            break
          }

          case 'step_debug': {
            const sdEvent = event as StepDebugEvent
            setMessages(
              (prev) =>
                prev.map((m) => {
                  if (m.id !== assistantMessageId || !m.taskExecution) return m
                  const existing = m.taskExecution.debugTrace ?? {
                    stepDebugEntries: [],
                  }
                  const entries = [...existing.stepDebugEntries]

                  if (sdEvent.phase === 'start') {
                    entries.push({
                      stepId: sdEvent.stepId,
                      capabilityId: sdEvent.capabilityId,
                      isDynamic: sdEvent.isDynamic,
                      directive: sdEvent.directive,
                      userRequest: sdEvent.userRequest,
                      params: sdEvent.params,
                    })
                  } else if (sdEvent.phase === 'complete') {
                    const idx = entries.findIndex(
                      (e) => e.stepId === sdEvent.stepId,
                    )
                    if (idx >= 0) {
                      entries[idx] = {
                        ...entries[idx],
                        outputPreview: sdEvent.outputPreview,
                        durationMs: sdEvent.durationMs,
                        success: sdEvent.success,
                        error: sdEvent.error,
                      }
                    } else {
                      entries.push({
                        stepId: sdEvent.stepId,
                        capabilityId: sdEvent.capabilityId,
                        isDynamic: sdEvent.isDynamic,
                        outputPreview: sdEvent.outputPreview,
                        durationMs: sdEvent.durationMs,
                        success: sdEvent.success,
                        error: sdEvent.error,
                      })
                    }
                  }

                  return {
                    ...m,
                    taskExecution: {
                      ...m.taskExecution,
                      debugTrace: { ...existing, stepDebugEntries: entries },
                    },
                  }
                }),
              mode,
            )
            break
          }
        }
      }
    },
    [
      updateMessage,
      updateMessageExecution,
      addExecutionStep,
      updateExecutionStep,
      locale,
      setSessionId,
      setMessages,
      enqueueFrontendActions,
    ],
  )

  createProgressHandlerRef.current = createProgressHandler

  // 发送消息

  const handleSend = useCallback(
    async (
      text: string,
      attachments: readonly AgentAttachment[] = [],
      mode: AgentPanelMode = getAgentPanelMode(),
      intentionId?: string,
    ) => {
      const messageText = text.trim()
      if (!messageText && attachments.length === 0) return
      const requestText =
        messageText ||
        format(t.agentPanel.attach.fallback, {
          names: attachments.map((item) => item.name).join(', '),
        })
      const modeSessionId = sessionIdsByModeRef.current[mode]

      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.AGENT_SEND, {
            target: location.pathname.split('/').filter(Boolean)[0] || 'home',
            throttleMs: 2000,
          })
        },
      )

      // 游客可开面板（guest visible / guest_perm_ai_chat），但 BE Agent 全线要 JWT。
      // 发消息前引导登录，避免必 401。
      if (!isAuthenticated) {
        const loginHint = t.agentPanel.loginRequiredHint
        setMessages(
          (prev) => [
            ...prev,
            {
              id: `msg_guest_hint_${Date.now()}`,
              sessionId: modeSessionId || '',
              role: 'assistant',
              content: loginHint,
              createdAt: new Date(),
            },
          ],
          mode,
        )
        window.setTimeout(() => {
          navigate(`/login?redirect=${encodeURIComponent(location.pathname)}`)
        }, 600)
        return
      }

      // 如果有待回答的问题，将输入路由到 answerQuestion（即使 isLoading 也允许）
      if (mode === 'work' && pendingAnswerMsg && answerQuestionRef.current) {
        answerQuestionRef.current(pendingAnswerMsg.id, requestText)
        return
      }

      if (loadingByModeRef.current[mode] && mode !== 'chat') {
        const activeTaskMessage = [...messages]
          .reverse()
          .find(
            (message) =>
              message.taskExecution?.status === 'processing' &&
              !!message.taskExecution.taskId &&
              !message.taskExecution.taskId.startsWith('confirmation:'),
          )
        if (!activeTaskMessage?.taskExecution?.taskId) return

        const userMessage: ChatMessage = {
          id: nextAgentMessageId('user'),
          sessionId: modeSessionId || '',
          role: 'user',
          content: messageText,
          createdAt: new Date(),
          ...(attachments.length ? { attachments: [...attachments] } : {}),
        }
        setMessages((prev) => [...prev, userMessage], mode)
        try {
          const result = await agentService.steerSession(
            requestText,
            activeTaskMessage.taskExecution.taskId,
          )
          updateMessageExecution(activeTaskMessage.id, {
            statusMessage: result.message,
          })
        } catch (error) {
          const errorMessage = userFacingError(
            error,
            t.errors.agentSteeringFailed,
          )
          setMessages(
            (prev) => [
              ...prev,
              {
                id: nextAgentMessageId('assistant'),
                sessionId: modeSessionId || '',
                role: 'assistant',
                content: format(t.agentPanel.errorWithDetail, {
                  error: errorMessage,
                }),
                createdAt: new Date(),
              },
            ],
            mode,
          )
        }
        return
      }

      // 切回对话视图

      // 1. 创建 user 消息
      const userMsgId = nextAgentMessageId('user')
      const userMessage: ChatMessage = {
        id: userMsgId,
        sessionId: modeSessionId || '',
        role: 'user',
        content: messageText,
        createdAt: new Date(),
        ...(attachments.length ? { attachments: [...attachments] } : {}),
      }

      // 2. 创建 placeholder assistant 消息
      const assistantMsgId = nextAgentMessageId('assistant')
      const assistantMessage: ChatMessage = {
        id: assistantMsgId,
        sessionId: modeSessionId || '',
        role: 'assistant',
        content: '',
        createdAt: new Date(),
        taskExecution: {
          taskId: '',
          status: 'processing',
          progress: 0,
          steps: [],
        },
      }

      const chatGeneration =
        mode === 'chat' ? chatTurnClockRef.current.next() : 0
      beginTurnTrace(assistantMsgId)
      markTurnTraceOnce('input_started')
      markTurnTraceOnce('input_final')
      if (mode === 'chat') {
        void interruptAgoraConversation()
        setTurnGeneration(chatGeneration)
        const previousChatId = loadingMessageIdByModeRef.current.chat
        if (previousChatId) {
          stopTurnSpeech(previousChatId)
        }
      }

      setMessages((prev) => [...prev, userMessage, assistantMessage], mode)
      loadingMessageIdByModeRef.current[mode] = assistantMsgId
      loadingByModeRef.current[mode] = true
      setAgentLaneLoading(mode, true)
      setIsLoading(true)
      setAgentStatusThinking()

      try {
        // 构建上下文
        const context: Record<string, unknown> = {
          currentRoute: location.pathname,
        }

        if (modeSessionId) {
          context.sessionId = modeSessionId
        }
        context.mode = mode
        if (intentionId) context.intentionId = intentionId
        // 页面内容
        const customData: Record<string, unknown> = {}
        const pageConsent = getAgentContextConsent()
        // 用户关掉「读当前页」之后就真的不读 —— 界面上说了不看，请求里也不能捎上
        if (pageConsent && pageContentContext?.hasContent) {
          const contentForAgent = pageContentContext.getContentForAgent()
          if (contentForAgent) {
            customData.pageContent =
              mode === 'chat'
                ? chatPagePayload(contentForAgent)
                : contentForAgent
          }
        } else if (pageConsent && typeof document !== 'undefined') {
          if (mode === 'chat') {
            const title = document.title.trim()
            if (title) {
              customData.pageContent = { type: 'custom', title }
            }
          } else {
            const main =
              document.querySelector('main') ?? document.body
            // Hidden controls are not part of what the user is currently reading.
            // eslint-disable-next-line unicorn/prefer-dom-node-text-content
            const text = (main?.innerText ?? '').replace(/\s+/g, ' ').trim()
            if (text) {
              customData.pageContent = {
                type: 'custom',
                title: document.title,
                content: text.slice(0, 8000),
                currentPath: location.pathname,
              }
            }
          }
        }
        const published = (
          window as unknown as {
            __musicPlayerState?: {
              isPlaying?: boolean
              isEnabled?: boolean
              currentSong?: unknown
              currentSongIndex?: number
              playlistLength?: number
            }
          }
        ).__musicPlayerState
        if (published) {
          const musicStatus = agentMusicStatus(
            published as Record<string, unknown>,
          )
          if (musicStatus) customData.musicStatus = musicStatus
        }
        if (mode !== 'chat' && hasActionHandler('query_windows')) {
          try {
            const windowState = await executeFrontendAction({
              type: 'query_windows',
              timestamp: Date.now(),
            })
            if (windowState && typeof windowState === 'object') {
              customData.windowState = windowState
            }
          } catch {
            // typed handler missing mid-unmount
          }
        }
        const body = captureTurnBody({
          route: location.pathname,
          page: pageConsent ? (pageContentContext?.pageContent ?? null) : null,
          pageConsent,
          selection: turnSelectionText(),
        })
        context.rigState = body.rigState
        customData.perception = body.perception
        customData.presence = body.presence
        if (attachments.length) {
          customData.attachments = attachmentsForRequest(attachments)
        }
        if (Object.keys(customData).length > 0) {
          context.customData = customData
        }

        markTurnTraceOnce('request_sent')
        const response = await agentService.processWithProgress(
          requestText,
          createProgressHandler(assistantMsgId, mode, chatGeneration),
          context,
        )

        if (handleAgentResponseRef.current) {
          await handleAgentResponseRef.current(
            assistantMsgId,
            response,
            mode,
            chatGeneration,
          )
        }
      } catch (error) {
        stopTurnSpeech(assistantMsgId)
        finishTurnTrace()
        if (isStreamSupersededError(error)) {
          noteTurnTraceDrop('superseded')
          updateMessageExecution(assistantMsgId, { status: 'error' })
          return
        }
        if (isUserInterruptError(error)) {
          noteTurnTraceDrop('cancelled')
          updateMessageExecution(assistantMsgId, { status: 'error' })
          return
        }
        // A budget rejection arrives on the same channel as a real failure and
        // reads as "出错了" without this: the stream is already HTTP 200 by then,
        // so the quota code on the error event is the only signal.
        const errorMsg = generationFailureMessage(
          error,
          t.agentPanel.executionFailed,
          t.agentPanel.requestTimeout,
          {
            AI_COOLDOWN_ACTIVE: t.agentPanel.quotaCooldown,
            AI_DAILY_CALL_LIMIT: t.agentPanel.quotaExhausted,
            AI_ANONYMOUS_DAILY_CALL_LIMIT: t.agentPanel.quotaExhausted,
            AI_DAILY_TOKEN_LIMIT: t.agentPanel.quotaExhausted,
            AI_ANONYMOUS_DAILY_TOKEN_LIMIT: t.agentPanel.quotaExhausted,
            AI_QUOTA_EXCEEDED: t.agentPanel.quotaExhausted,
            QUEUE_FULL: t.agentPanel.queueBusy,
            agent_access_denied: t.agentPanel.accessDenied,
            admin_required: t.agentPanel.accessDenied,
            agent_processing_failed: t.agentPanel.executionFailed,
            NETWORK_ERROR: t.agentPanel.streamError,
          },
        )
        // 传输层直接抛出时后端来不及发 error 事件，补一条给状态岛
        pushAgentStatusEvent({
          type: 'error',
          message: errorMsg,
          code: errorCode(error) ?? 'agent_processing_failed',
        })

        // 保留已收集的 debugTrace 和步骤信息，只更新状态
        setMessages(
          (prev) =>
            prev.map((m) => {
              if (m.id !== assistantMsgId) return m
              const existing = m.taskExecution
              return {
                ...m,
                content:
                  m.content ||
                  t.agentPanel.errorWithDetail.replace('{error}', errorMsg),
                taskExecution: {
                  taskId: existing?.taskId ?? '',
                  status: 'error' as const,
                  progress: existing?.progress ?? 0,
                  steps: existing?.steps ?? [],
                  debugTrace: existing?.debugTrace,
                  executionTrace: existing?.executionTrace,
                },
              }
            }),
          mode,
        )
      } finally {
        if (loadingMessageIdByModeRef.current[mode] === assistantMsgId) {
          loadingByModeRef.current[mode] = false
          loadingMessageIdByModeRef.current[mode] = null
          setAgentLaneLoading(mode, false)
        }
        setIsLoading(
          loadingByModeRef.current.work || loadingByModeRef.current.chat,
        )
      }
    },
    [
      messages,
      location.pathname,
      pageContentContext,
      createProgressHandler,
      updateMessage,
      pendingAnswerMsg,
      isAuthenticated,
      navigate,
      t,
      format,
    ],
  )

  useEffect(() => {
    handleSendRef.current = handleSend
  }, [handleSend])

  // 处理 Agent 响应

  const handleAgentResponse = useCallback(
    async (
      messageId: string,
      response: AgentResponse,
      mode: AgentPanelMode = 'work',
      generation = 0,
      speechOutput: 'local' | 'external' = 'local',
    ) => {
      if (discardedResponseIdsRef.current.delete(messageId)) return
      const taskData = response.task as Record<string, unknown> | undefined
      let pendingQuestion = taskData?.pendingQuestion as
        PendingQuestion | undefined
      if (
        response.responseType === 'confirmation_required' &&
        response.confirmation
      ) {
        const confirmation = response.confirmation
        const details = confirmation.pendingSteps
          .map((step) => {
            const impact =
              step.impact.length > 0 ? `\n${step.impact.join('\n')}` : ''
            return `${step.capabilityName}: ${step.message}${impact}`
          })
          .join('\n\n')
        pendingQuestion = {
          questionId: `confirmation:${confirmation.confirmationId}`,
          confirmationId: confirmation.confirmationId,
          questionType: 'confirmation',
          question: response.message,
          context: details || undefined,
          options: [
            { value: 'confirm', label: t.common.confirm },
            { value: 'cancel', label: t.common.cancel },
          ],
          required: true,
          riskLevel: confirmation.riskLevel,
          expiresInSeconds: confirmation.expiresInSeconds,
          receivedAtMs: Date.now(),
        }
        // 新 UI 的操作卡片从这里拿料；它按风险决定摊开多少
        setAgentPendingAction(
          buildAgentPendingAction({
            confirmation,
            prompt: response.message,
            nowMs: Date.now(),
          }),
        )
      }
      const taskId = taskData?.taskId as string | undefined
      const taskStatus = taskData?.status as string | undefined
      const responseKey = response.confirmation?.confirmationId
        ? `confirmation:${response.confirmation.confirmationId}`
        : taskId
          ? `${taskId}:${taskStatus ?? response.responseType}:${pendingQuestion?.questionId ?? ''}`
          : null
      if (responseKey) {
        if (handledResponseKeysRef.current.has(responseKey)) return
        handledResponseKeysRef.current.add(responseKey)
        capSet(handledResponseKeysRef.current)
      }

      if (pendingQuestion && pendingQuestion.question) {
        if (!pendingQuestion.confirmationId) {
          setAgentStatusAwaitingConfirmation(pendingQuestion.question)
        }
        updateMessage(messageId, {
          pendingQuestion,
          selectedAnswer: undefined,
        })
        updateMessageExecution(messageId, {
          status: 'waiting',
          taskId:
            (taskData?.taskId as string) ||
            (pendingQuestion.confirmationId
              ? `confirmation:${pendingQuestion.confirmationId}`
              : ''),
          progress: 100,
        })
        deliverTurnLine(mode, {
          messageId,
          text: pendingQuestion.question,
          locale,
        })
        finishTurnTrace()
        return
      }

      const isSuccess =
        response.success !== false &&
        (response.task?.status === 'completed' ||
          response.responseType === 'answer' ||
          response.responseType === 'task_completed')

      const responseData = response.data as Record<string, unknown> | undefined
      if (mode === 'chat' && responseData && 'outfitId' in responseData) {
        const overlayId = responseData.outfitId
        setChatOutfitOverlay(
          typeof overlayId === 'string' ? overlayId : null,
        )
      }

      const stepHistory = taskData?.stepHistory as
        | Array<{
            stepId: string
            status: string
            outputSummary?: string
            capabilityName?: string
            durationMs?: number
            error?: string
            imageUrl?: string
          }>
        | undefined

      const isMultiStep = stepHistory && stepHistory.length > 1

      // 构建显示内容
      // 多步骤：response.message 已由后端 ai_summarize 生成人格化汇总，直接使用
      // 单步骤：优先使用 data 中的 AI 文本（reply/aiSummary/analysis/summary）
      // 注：announce_plan 已通过 SSE 实时写入正文，此处 response.message（= ai_summarize）会覆盖它
      let displayMessage: string | undefined

      if (isMultiStep) {
        // 多步骤：后端 response.message 是人格化汇总
        displayMessage = response.message
      } else {
        // 单步骤：从 data 提取 AI 文本
        displayMessage =
          messageFromStepOutput(response.data) || response.message
      }

      // 失败步骤信息追加
      if (stepHistory && stepHistory.length > 0) {
        const failedSteps = stepHistory.filter((s) => s.status === 'failed')
        if (failedSteps.length > 0 && failedSteps.length < stepHistory.length) {
          const failInfo = failedSteps
            .map((s) =>
              userFacingError(
                s.error || s.outputSummary,
                t.agentPanel.executionFailed,
              ),
            )
            .join('；')
          displayMessage = `${displayMessage || ''}\n${format(t.agentPanel.failReason, { reason: failInfo })}`
        } else if (failedSteps.length === stepHistory.length) {
          displayMessage = t.agentPanel.executionFailed
          const failInfo = failedSteps
            .map((s) =>
              userFacingError(
                s.error || s.outputSummary,
                t.agentPanel.executionFailed,
              ),
            )
            .join('；')
          displayMessage += `\n${failInfo}`
        }
      }

      console.log('[AgentEngine] handleAgentResponse:', {
        responseType: response.responseType,
        message: response.message,
        isMultiStep,
        dataKeys: responseData ? Object.keys(responseData) : [],
        displayMessage,
      })

      // 从 response.data 和 stepHistory 中兜底提取 imageUrls（SSE 丢失时恢复）
      // 与已通过 SSE 实时收集的 imageUrls 合并（不覆盖）
      const fallbackImageUrls = imageUrlsFromAgentPayload(
        response.data,
        stepHistory,
      )

      // 合并：SSE 实时收集的 + fallback，去重
      const existingImageUrls: string[] = ((): string[] => {
        const msg = findMessage(messageId)
        return msg?.imageUrls ?? []
      })()
      const mergedImageUrls = [...existingImageUrls]
      for (const url of fallbackImageUrls) {
        if (!mergedImageUrls.includes(url)) {
          mergedImageUrls.push(url)
        }
      }

      updateMessage(messageId, {
        content:
          displayMessage || response.message || t.agentPanel.taskCompleted,
        suggestions: response.suggestions?.length
          ? response.suggestions
          : undefined,
        data: response.data,
        pendingQuestion: undefined,
        selectedAnswer: undefined,
        ...(mergedImageUrls.length > 0 ? { imageUrls: mergedImageUrls } : {}),
      })

      const spokenReply = displayMessage || response.message
      const staleChat =
        mode === 'chat' &&
        generation > 0 &&
        !isCurrentChatGeneration(
          generation,
          chatTurnClockRef.current.current(),
        )
      if (isSuccess && !staleChat) {
        deliverTurnLine(mode, {
          messageId,
          text: speechOutput === 'external' || turnSpeechAlreadyFed(messageId)
            ? undefined
            : spokenReply,
          locale,
          performance: response.performance,
        })
      }

      const hasFailedSteps =
        stepHistory?.some((s) => s.status === 'failed') ?? false

      // 从 TaskInfo 中解析 executionTrace
      const rawTrace = taskData?.executionTrace as
        | {
            trace_id?: string
            total_duration_ms?: number
            totalDurationMs?: number
            tier_usage?: Record<string, number>
            tierUsage?: Record<string, number>
            steps?: Array<{
              step_id?: string
              stepId?: string
              capability_id?: string
              capabilityId?: string
              tier_used?: string
              tierUsed?: string
              duration_ms?: number
              durationMs?: number
              success?: boolean
              error?: string
            }>
          }
        | undefined

      const executionTrace: ExecutionTrace | undefined = rawTrace
        ? {
            totalDurationMs:
              rawTrace.totalDurationMs ?? rawTrace.total_duration_ms ?? 0,
            tierUsage: rawTrace.tierUsage ?? rawTrace.tier_usage ?? {},
            steps: (rawTrace.steps ?? []).map((s) => ({
              stepId: s.stepId ?? s.step_id ?? '',
              capabilityId: s.capabilityId ?? s.capability_id ?? '',
              tierUsed: s.tierUsed ?? s.tier_used ?? '',
              durationMs: s.durationMs ?? s.duration_ms ?? 0,
              success: s.success ?? true,
              error: s.error,
            })),
          }
        : undefined

      const liveSteps = findMessage(messageId)?.taskExecution?.steps ?? []
      const historySteps = executionStepsFromHistory(
        stepHistory as Array<Record<string, unknown>> | undefined,
      )

      updateMessageExecution(messageId, {
        status:
          isSuccess && !hasFailedSteps
            ? 'completed'
            : response.success === false || response.responseType === 'error'
              ? 'error'
              : hasFailedSteps || response.task?.status === 'failed'
                ? 'error'
                : 'completed',
        progress: 100,
        ...(executionTrace ? { executionTrace } : {}),
        ...(liveSteps.length === 0 && historySteps.length > 0
          ? { steps: historySteps }
          : {}),
      })

      // 执行前端动作。流式路径已在 step_completed 跑过同 timestamp 的动作，这里只补漏。
      const frontendActions = responseData?.frontendActions as
        (typeof response.frontendAction)[] | undefined
      let frontendAction =
        response.frontendAction ||
        (responseData?.frontendAction as typeof response.frontendAction) ||
        (responseData?.action as typeof response.frontendAction)

      if (
        frontendAction &&
        typeof frontendAction === 'object' &&
        'type' in frontendAction
      ) {
        const actionObj = frontendAction as unknown as Record<string, unknown>
        if (!('timestamp' in actionObj)) {
          frontendAction = {
            ...actionObj,
            timestamp: Date.now(),
          } as typeof response.frontendAction
        }
        if (responseData?.criteria && !('criteria' in actionObj)) {
          frontendAction = {
            ...(frontendAction as unknown as Record<string, unknown>),
            criteria: responseData.criteria as string,
          } as typeof response.frontendAction
        }
      }

      const pendingActions =
        frontendActions &&
        Array.isArray(frontendActions) &&
        frontendActions.length > 0
          ? frontendActions
          : frontendAction
            ? [frontendAction]
            : []
      const visibleResults = await enqueueFrontendActions(
        messageId,
        pendingActions,
      )
      if (visibleResults.length > 0) {
        const serialized = JSON.stringify(visibleResults, null, 2).slice(
          0,
          4000,
        )
        updateMessage(messageId, {
          content: `${displayMessage || response.message}\n\n\`\`\`json\n${serialized}\n\`\`\``,
          data: {
            ...(responseData ?? {}),
            frontendActionResults: visibleResults,
          },
        })
      }
      dispatchedFrontendKeysRef.current.delete(messageId)
      frontendActionChainRef.current.delete(messageId)
      finishTurnTrace()
    },
    [locale, updateMessage, updateMessageExecution, enqueueFrontendActions],
  )

  useEffect(() => {
    handleAgentResponseRef.current = handleAgentResponse
  }, [handleAgentResponse])

  useEffect(() => bindRealtimeChat({
    sessionId: () => sessionIdsByModeRef.current.chat,
    adopt: (notice) => {
      const messageId = `msg_rtc_${notice.runId}`
      const generation = chatTurnClockRef.current.next()
      const previous = loadingMessageIdByModeRef.current.chat
      if (previous) {
        stopTurnSpeech(previous)
        updateMessageExecution(previous, { status: 'error' })
      }
      agentService.abortCurrentRequest('chat')
      setTurnGeneration(generation)
      beginTurnTrace(messageId)
      markTurnTraceOnce('input_final')
      setSessionId(notice.sessionId, 'chat')
      setMessages((rows) => [...rows, {
        id: nextAgentMessageId('user'), sessionId: notice.sessionId, role: 'user',
        content: notice.input, createdAt: new Date(),
      }, {
        id: messageId, sessionId: notice.sessionId, role: 'assistant', content: '', createdAt: new Date(),
        taskExecution: { taskId: '', runId: notice.runId, status: 'processing', progress: 0, steps: [] },
      }], 'chat')
      loadingMessageIdByModeRef.current.chat = messageId
      loadingByModeRef.current.chat = true
      setAgentLaneLoading('chat', true)
      setIsLoading(true)
      setAgentStatusThinking()
      // Same progress and final-response reducers. Only the audio outlet is
      // external: cloud audio must not be synthesized or text-lip-synced twice.
      void agentService.subscribeRun(notice.runId, createProgressHandler(messageId, 'chat', generation, 'external', notice.runId), 'chat')
        .then((response) => handleAgentResponse(messageId, response, 'chat', generation, 'external'))
        .catch((error) => {
          if (!isCurrentChatGeneration(generation, chatTurnClockRef.current.current())) return
          updateMessageExecution(messageId, { status: 'error' })
          if (!isUserInterruptError(error) && !isStreamSupersededError(error)) {
            updateMessage(messageId, { content: userFacingError(error, t.agentPanel.streamError) })
          }
        })
        .finally(() => {
          if (loadingMessageIdByModeRef.current.chat !== messageId) return
          loadingMessageIdByModeRef.current.chat = null
          loadingByModeRef.current.chat = false
          setAgentLaneLoading('chat', false)
          setIsLoading(loadingByModeRef.current.work)
        })
      return { messageId, generation }
    },
  }), [createProgressHandler, handleAgentResponse, setMessages, setSessionId, t, updateMessage, updateMessageExecution])

  // 回答问题

  const answerQuestion = useCallback(
    async (messageId: string, answer: string) => {
      const msg = findMessage(messageId)
      if (!msg?.taskExecution?.taskId || !msg.pendingQuestion) return

      // 敏感确认过期后禁止 Confirm（Cancel 仍可关卡）
      const pq = msg.pendingQuestion
      if (
        answer === 'confirm' &&
        pq.confirmationId &&
        typeof pq.expiresInSeconds === 'number' &&
        pq.expiresInSeconds > 0 &&
        typeof pq.receivedAtMs === 'number'
      ) {
        const remaining =
          pq.expiresInSeconds -
          Math.floor((Date.now() - pq.receivedAtMs) / 1000)
        if (remaining <= 0) {
          updateMessage(messageId, {
            content: t.agentPanel.confirmExpiredHint,
          })
          updateMessageExecution(messageId, { status: 'error' })
          return
        }
      }

      // 保留 pendingQuestion 以显示选中状态，同时用 selectedAnswer 锁定
      updateMessage(messageId, {
        selectedAnswer: answer,
      })
      updateMessageExecution(messageId, {
        status: 'processing',
        progress: 50,
      })
      loadingMessageIdByModeRef.current.work = messageId
      loadingByModeRef.current.work = true
      setAgentLaneLoading('work', true)
      setIsLoading(true)
      setAgentStatusThinking()

      try {
        beginTurnTrace(messageId)
        markTurnTraceOnce('request_sent')
        const response = msg.pendingQuestion.confirmationId
          ? await agentService.confirmOperation(
              msg.pendingQuestion.confirmationId,
              answer === 'confirm',
              undefined,
              createProgressHandler(messageId, 'work'),
            )
          : await agentService.answerQuestionWithProgress(
              msg.taskExecution.taskId,
              msg.pendingQuestion.questionId,
              answer,
              createProgressHandler(messageId, 'work'),
            )
        await handleAgentResponseRef.current?.(messageId, response, 'work')
      } catch (error) {
        stopTurnSpeech(messageId)
        finishTurnTrace()
        if (isUserInterruptError(error) || isStreamSupersededError(error)) {
          updateMessageExecution(messageId, { status: 'error' })
          return
        }
        const errorMsg = userFacingError(error, t.errors.agentConfirmFailed)
        updateMessage(messageId, {
          content: format(t.agentPanel.answerFailed, { error: errorMsg }),
        })
        updateMessageExecution(messageId, { status: 'error' })
      } finally {
        if (loadingMessageIdByModeRef.current.work === messageId) {
          loadingByModeRef.current.work = false
          loadingMessageIdByModeRef.current.work = null
          setAgentLaneLoading('work', false)
        }
        setIsLoading(
          loadingByModeRef.current.work || loadingByModeRef.current.chat,
        )
      }
    },
    [
      findMessage,
      updateMessage,
      updateMessageExecution,
      createProgressHandler,
      t.agentPanel.confirmExpiredHint,
      t.errors.agentConfirmFailed,
      t.agentPanel.answerFailed,
      format,
    ],
  )

  useEffect(() => {
    answerQuestionRef.current = answerQuestion
  }, [answerQuestion])

  // 这个组件不画任何东西。它是执行引擎：SSE、会话、重连、确认、错误处理都在
  // 这里跑，结果通过 store 交给新 UI 去画。
  return null
}

export default AgentEngine
