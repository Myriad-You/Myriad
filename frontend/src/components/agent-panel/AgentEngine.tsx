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
import { playbackDirection, retainPlaybackDirection, startPlaybackDirection } from '../../features/merope/motion/playbackDirectionHost'
import { interruptAgoraConversation, stopAgoraConversation } from '../../features/merope/speech/agoraConversation'
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
import { authSubject } from '../../utils/authSubject'
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
import { FrontendActionQueue } from './frontendActionQueue'
import { syncProjectedMessages } from './projectAgentMessage'

import { SessionLoadScope } from './sessionLoadScope'
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

/** Inverse from the actual path change, not the declared target. */
async function runFrontendAction(action: FrontendAction, signal: AbortSignal): Promise<unknown> {
  if (signal.aborted) return
  const beforePath = currentPath()
  const result = await executeFrontendAction(action, signal)
  if (signal.aborted) return
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
  const [sessionLoads] = useState(() => new SessionLoadScope())
  useEffect(() => () => sessionLoads.reset(), [sessionLoads])
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

  useLayoutEffect(() => {
    syncProjectedMessages(messages)
  }, [messages])

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
      speechOutput?: 'local' | 'external', runId?: string, subject?: AbortSignal) => (event: ProgressEvent) => void) | null
  >(null)
  const MAX_RESPONSE_GUARD_KEYS = 200
  const [frontendActionQueue] = useState(() =>
    new FrontendActionQueue(runFrontendAction, MAX_RESPONSE_GUARD_KEYS))
  useEffect(() => () => frontendActionQueue.reset(), [frontendActionQueue])

  const capSet = (set: Set<string>) => {
    while (set.size > MAX_RESPONSE_GUARD_KEYS) {
      const oldest = set.values().next().value
      if (oldest === undefined) break
      set.delete(oldest)
    }
  }

  const enqueueFrontendActions = useCallback(
    (
      messageId: string,
      actions: Array<FrontendAction | null | undefined>,
      subject = authSubject.signal,
    ) => frontendActionQueue.enqueue(messageId, actions, subject),
    [frontendActionQueue],
  )
  const answerQuestionRef =
    useRef<(messageId: string, answer: string) => void>(null)
  const sessionTitleSetByModeRef = useRef<Record<AgentPanelMode, boolean>>({
    work: false,
    chat: false,
  })
  const handledResponseKeysRef = useRef(new Set<string>())
  const chatTurnClockRef = useRef(new ChatTurnClock())

  const pendingAnswerMsg = useMemo(() => {
    return (
      messages.findLast(
        (m) =>
          Boolean(m.pendingQuestion) &&
          !m.selectedAnswer &&
          m.taskExecution?.status === 'waiting',
      ) ?? null
    )
  }, [messages])

  const startNewSession = useCallback(async () => {
    const current = getAgentPanelMode()
    sessionLoads.reset(current)
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
  }, [setSessionId, setMessages, sessionLoads])

  /** GET run stream / task; do not POST process. */
  const reattachLiveWork = useCallback(
    async (
      messagesToScan: ChatMessage[],
      hints?: { runId?: string; taskId?: string },
      subject = authSubject.signal,
    ) => {
      if (subject.aborted) return
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
        if (subject.aborted) return
        try {
          const taskId = candidate.taskId
          const runId = candidate.runId
          let progress = 0
          let isWaiting = false
          let pendingQ: PendingQuestion | undefined

          if (taskId) {
            const task = await agentService.getTask(taskId, subject)
            if (subject.aborted) return
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
            'work', 0, 'local', runId, subject,
          )
          if (!onProgress) continue
          void agentService
            .subscribeRun(runId, onProgress, 'work', subject)
            .then((response) => {
              if (subject.aborted) return
              return handleAgentResponseRef.current?.(candidate.messageId, response)
            })
            .catch((error) => {
              if (subject.aborted) return
              stopTurnSpeech(candidate.messageId)
              console.warn('[AgentEngine] reattach stream ended:', error)
            })
            .finally(() => {
              if (subject.aborted) return
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
          break
        } catch (error) {
          if (subject.aborted) return
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
      const subject = sessionLoads.begin(requestedMode)
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
          subject,
        )
        if (subject.aborted) return
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
        if (requestedMode === 'work') void reattachLiveWork(loaded, reattachHints, subject)
      } catch (error) {
        if (subject.aborted) return
        console.error('[AgentEngine] 加载会话消息失败:', error)
      }
    },
    [reattachLiveWork, sessionLoads],
  )

  useEffect(() => {
    const handleOpenSession = (e: Event) => {
      const detail = (e as CustomEvent).detail as {
        sessionId?: string
        runId?: string
        taskId?: string
      } | null
      const sid = detail?.sessionId
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
    }
    window.addEventListener('arael-open-manage', handleOpenManage)
    return () =>
      window.removeEventListener('arael-open-manage', handleOpenManage)
  }, [])

  useEffect(() => {
    const handleSubmit = (event: Event) => {
      const detail = agentPanelSubmitDetail(event)
      if (!detail) return
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

  useEffect(() => authSubject.subscribe(() => {
    sessionLoads.reset()
    frontendActionQueue.reset()
    // Identity loss, not attention transfer: detach both old transport lanes.
    // This does not cancel the previous user's server-side Work task.
    for (const lane of ['work', 'chat'] as const) {
      agentService.abortCurrentRequest(lane)
      const id = loadingMessageIdByModeRef.current[lane]
      if (id) discardedResponseIdsRef.current.add(id)
      loadingMessageIdByModeRef.current[lane] = null
      loadingByModeRef.current[lane] = false
      setAgentLaneLoading(lane, false)
      setSessionId(null, lane)
      setMessages([], lane)
      sessionTitleSetByModeRef.current[lane] = false
    }
    capSet(discardedResponseIdsRef.current)
    setTurnGeneration(chatTurnClockRef.current.next())
    setIsLoading(false)
    resetAgentStatus()
    clearChatOutfitOverlay()
    void stopAgoraConversation()
  }), [setMessages, setSessionId, frontendActionQueue, sessionLoads])

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

  useEffect(() => {
    setAgentSessionId(sessionId)
  }, [sessionId])

  const interruptCurrentTask = useCallback(async () => {
    const current = getAgentPanelMode()
    // Work and Chat can be in flight together
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

    // drop occupancy before awaiting cancel, else a late token writes thinking
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

  const createProgressHandler = useCallback(
    (
      assistantMessageId: string,
      mode: AgentPanelMode = 'work',
      generation = 0,
      speechOutput: 'local' | 'external' = 'local',
      initialRunId?: string,
      subject = authSubject.signal,
    ) => {
      if (subject.aborted) return () => {}
      let streamedSummary = ''
      let streamedThinking = ''
      let performancePlanCount = 0
      let performanceRunId = initialRunId
      let notedStaleGeneration = false
      let liveTaskId = ''
      const speech = openTurnSpeech(mode, assistantMessageId, generation, locale, speechOutput)
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
        if (subject.aborted) return
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
        pushAgentStatusEvent(event)
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

            if (
              tcEvent.stepDescriptions &&
              tcEvent.stepDescriptions.length > 0
            ) {
              execUpdates.planStepDescriptions = tcEvent.stepDescriptions
            }

            updateMessageExecution(assistantMessageId, execUpdates)
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
            streamedSummary = ''
            const stepEvent = event as StepStartedEvent
            addExecutionStep(assistantMessageId, {
              id: stepEvent.stepId,
              name: userFacingError(stepEvent.description),
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
                  subject,
                )
                if (subject.aborted) return
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
                  if (Object.hasOwn(row, 'isPlaying') || Object.hasOwn(row, 'isEnabled')) {
                    musicStatus = result
                  }
                  if (Object.hasOwn(row, 'windows') || Object.hasOwn(row, 'available')) {
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
                  subject,
                )
              })()
            }
            break
          }

          case 'step_retrying': {
            const retryEvent =
              event as import('../../services/agent/types').StepRetryingEvent
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
            const completedEvent =
              event as import('../../services/agent/types').TaskCompletedEvent
            const taskInfo = completedEvent.response?.task as
              Record<string, unknown> | undefined
            const isStillWaiting = taskInfo?.status === 'waiting_for_input'

            if (!isStillWaiting) {
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
                  const entries = Iterator.from(existing.stepDebugEntries).toArray()

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

  const handleSend = useCallback(
    async (
      text: string,
      attachments: readonly AgentAttachment[] = [],
      mode: AgentPanelMode = getAgentPanelMode(),
      intentionId?: string,
    ) => {
      const subject = authSubject.signal
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
            target: location.pathname.split('/').find(Boolean) || 'home',
            throttleMs: 2000,
          })
        },
      )

      // guest can open the panel; Agent still requires JWT
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

      if (mode === 'work' && pendingAnswerMsg && answerQuestionRef.current) {
        answerQuestionRef.current(pendingAnswerMsg.id, requestText)
        return
      }

      if (loadingByModeRef.current[mode] && mode !== 'chat') {
        const activeTaskMessage = messages.findLast(
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
          ...(attachments.length ? { attachments: Iterator.from(attachments).toArray() } : {}),
        }
        setMessages((prev) => [...prev, userMessage], mode)
        try {
          const result = await agentService.steerSession(
            requestText,
            activeTaskMessage.taskExecution.taskId,
            subject,
          )
          if (subject.aborted) return
          updateMessageExecution(activeTaskMessage.id, {
            statusMessage: result.message,
          })
        } catch (error) {
          if (subject.aborted) return
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

      const userMsgId = nextAgentMessageId('user')
      const userMessage: ChatMessage = {
        id: userMsgId,
        sessionId: modeSessionId || '',
        role: 'user',
        content: messageText,
        createdAt: new Date(),
        ...(attachments.length ? { attachments: Iterator.from(attachments).toArray() } : {}),
      }

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
        const context: Record<string, unknown> = {
          currentRoute: location.pathname,
        }

        if (modeSessionId) {
          context.sessionId = modeSessionId
        }
        context.mode = mode
        if (intentionId) context.intentionId = intentionId
        const customData: Record<string, unknown> = {}
        const pageConsent = getAgentContextConsent()
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
            // Hidden controls are not what the user is reading.
            // eslint-disable-next-line unicorn/prefer-dom-node-text-content
            const text = (main?.innerText ?? '').replaceAll(/\s+/g, ' ').trim()
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
            }, subject)
            if (windowState && typeof windowState === 'object') {
              customData.windowState = windowState
            }
          } catch {
            /* typed handler missing mid-unmount */
          }
        }
        if (subject.aborted) return
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
          createProgressHandler(assistantMsgId, mode, chatGeneration, 'local', undefined, subject),
          context,
          subject,
        )

        if (subject.aborted) return
        if (handleAgentResponseRef.current) {
          await handleAgentResponseRef.current(
            assistantMsgId,
            response,
            mode,
            chatGeneration,
          )
        }
      } catch (error) {
        if (subject.aborted) return
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
        // quota arrives as HTTP 200; the error-event code is the only signal
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
        pushAgentStatusEvent({
          type: 'error',
          message: errorMsg,
          code: errorCode(error) ?? 'agent_processing_failed',
        })

        setMessages(
          (prev) =>
            prev.map((m) => {
              if (m.id !== assistantMsgId) return m
              const existing = m.taskExecution
              return {
                ...m,
                content:
                  m.content ||
                  format(t.agentPanel.errorWithDetail, { error: errorMsg }),
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
        if (!subject.aborted && loadingMessageIdByModeRef.current[mode] === assistantMsgId) {
          loadingByModeRef.current[mode] = false
          loadingMessageIdByModeRef.current[mode] = null
          setAgentLaneLoading(mode, false)
        }
        if (!subject.aborted) {
          setIsLoading(loadingByModeRef.current.work || loadingByModeRef.current.chat)
        }
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

  const handleAgentResponse = useCallback(
    async (
      messageId: string,
      response: AgentResponse,
      mode: AgentPanelMode = 'work',
      generation = 0,
      speechOutput: 'local' | 'external' = 'local',
    ) => {
      if (discardedResponseIdsRef.current.has(messageId)) return
      const subject = authSubject.signal
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

      if (pendingQuestion?.question) {
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
      if (mode === 'chat' && responseData && Object.hasOwn(responseData, 'outfitId')) {
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

      // ai_summarize overwrites announce_plan already streamed into content
      let displayMessage: string | undefined

      if (isMultiStep) {
        displayMessage = response.message
      } else {
        displayMessage =
          messageFromStepOutput(response.data) || response.message
      }

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

      const fallbackImageUrls = imageUrlsFromAgentPayload(
        response.data,
        stepHistory,
      )

      const existingImageUrls: string[] = ((): string[] => {
        const msg = findMessage(messageId)
        return msg?.imageUrls ?? []
      })()
      const mergedImageUrls = Iterator.from(existingImageUrls).toArray()
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

      // stream already ran same-timestamp actions; fill gaps only
      const frontendActions = responseData?.frontendActions as
        (typeof response.frontendAction)[] | undefined
      let frontendAction =
        response.frontendAction ||
        (responseData?.frontendAction as typeof response.frontendAction) ||
        (responseData?.action as typeof response.frontendAction)

      if (
        frontendAction &&
        typeof frontendAction === 'object' &&
        Object.hasOwn(frontendAction, 'type')
      ) {
        const actionObj = frontendAction as unknown as Record<string, unknown>
        if (!Object.hasOwn(actionObj, 'timestamp')) {
          frontendAction = {
            ...actionObj,
            timestamp: Date.now(),
          } as typeof response.frontendAction
        }
        if (responseData?.criteria && !Object.hasOwn(actionObj, 'criteria')) {
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
        subject,
      )
      if (subject.aborted) return
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
      frontendActionQueue.forget(messageId)
      finishTurnTrace()
    },
    [locale, updateMessage, updateMessageExecution, enqueueFrontendActions, frontendActionQueue],
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
      // external audio must not be synthesized or lip-synced twice
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

  const answerQuestion = useCallback(
    async (messageId: string, answer: string) => {
      const msg = findMessage(messageId)
      if (!msg?.taskExecution?.taskId || !msg.pendingQuestion) return

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

  return null
}

export default AgentEngine
