/**
 * Arael - AI 助手浮动面板
 *
 * 对话系统重构版：
 * - message-centric 聊天 UI（替代 task-list）
 * - 服务端 session 持久化多轮上下文
 * - 保留丰富的任务执行可视化（嵌入聊天气泡）
 * - 全局任意区域长按 500ms 触发
 * - SSE 流式进度更新
 * - 统一面板内切换（对话/会话列表/管理）
 */

import type { TranslationKeys } from '../../i18n'
import type {
  AgentResponse,
  PlannerDecisionEvent,
  ProgressEvent,
  ProgressUpdateEvent,
  SessionInfo,
  StepCompletedEvent,
  StepDebugEvent,
  StepStartedEvent,
  SummaryTokenEvent,
  TaskCreatedEvent,
  TaskPreset,
} from '../../services/agent'

import type {
  ChatMessage,
  ChatSession,
  ExecutionTrace,
  PanelVisibility,
  PendingQuestion,
  TaskExecution,
} from './types'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useLocation } from 'react-router-dom'
import { useI18n } from '../../contexts/I18nContext'
import { usePageContentOptional } from '../../contexts/PageContentContext'
import { agentService, executeFrontendAction } from '../../services/agent'
import { isImeComposing } from '../../utils/ime'
import {
  collectReattachCandidates,
  isNonTerminalTaskStatus,
} from '../../services/agent/reattach'

import { AraelChatMessage } from './components/AraelChatMessage'
import { AraelDebugPanel } from './components/AraelDebugPanel'
import { AraelInput } from './components/AraelInput'
import { AraelManageDrawer } from './components/AraelManageDrawer'
import { AraelPresets } from './components/AraelPresets'
import { AraelSessionList } from './components/AraelSessionList'
import { useLongPress, useMessageState, useVoiceRecording } from './hooks'
import { LONG_PRESS_DURATION, SPRING_SNAPPY } from './types'
import './AraelPanel.css'

/** 面板内视图 */
type PanelView = 'chat' | 'sessions' | 'manage' | 'debug'

const ARAEL_PREFIX_RE = /^Arael\s*/

/** 连点打开 debug 面板：窗口内点击次数 / 时间窗 */
const DEBUG_MULTI_CLICK_COUNT = 5
const DEBUG_MULTI_CLICK_WINDOW_MS = 2000

// 智能提示词生成
function getSmartGreeting(
  pathname: string,
  _historyCount: number,
  arael: TranslationKeys['arael'],
): string {
  const hour = new Date().getHours()
  const g = arael.greeting

  const timeGreeting =
    hour < 6
      ? g.lateNight
      : hour < 12
        ? g.morning
        : hour < 18
          ? g.afternoon
          : g.evening

  const pageHintsMap: Record<string, string[]> = {
    '/library': arael.pageHints.library,
    '/brew': arael.pageHints.brew,
    '/reports': arael.pageHints.reports,
    '/config': arael.pageHints.config,
    '/data-management': arael.pageHints.dataManagement,
    '/tapp': arael.pageHints.tapp,
  }

  for (const [path, hints] of Object.entries(pageHintsMap)) {
    if (pathname.startsWith(path)) {
      const hint = hints[Math.floor(Math.random() * hints.length)]
      return `Arael ${timeGreeting}，${hint}`
    }
  }

  const hint =
    arael.generalHints[Math.floor(Math.random() * arael.generalHints.length)]
  return `Arael ${timeGreeting}，${hint}`
}

/** 调试日志条目 */
interface DebugLogEntry {
  time: string
  type: 'response' | 'sse' | 'error'
  data: unknown
}

// ============ 组件 ============

export const AraelPanel: React.FC = () => {
  const location = useLocation()
  const { t, format, locale } = useI18n()

  // 页面内容上下文
  const pageContentContext = usePageContentOptional()

  // 面板可见性
  const [visibility, setVisibility] = useState<PanelVisibility>('hidden')

  // 输入状态
  const [input, setInput] = useState('')
  const [isLoading, setIsLoading] = useState(false)

  // ============ 对话系统核心状态 ============

  // 消息状态（提取到 useMessageState hook）
  const {
    messages,
    setMessages,
    messagesRef,
    updateMessage,
    updateMessageExecution,
    addExecutionStep,
    updateExecutionStep,
  } = useMessageState()

  // 当前会话 ID（服务端持久化）
  const [sessionId, setSessionId] = useState<string | null>(null)

  // 面板内视图切换
  const [panelView, setPanelView] = useState<PanelView>('chat')

  // 任务预设（收藏）
  const [presetFavorites, setPresetFavorites] = useState<TaskPreset[]>([])

  // 空状态继续对话候选（上一个、上上个）
  const [continueSessions, setContinueSessions] = useState<ChatSession[]>([])

  // 语音录制（提取到 useVoiceRecording hook）
  const handleSendRef = useRef<(text?: string) => Promise<void>>(null)
  const voiceResultHandler = useCallback((text: string) => {
    setInput(text)
    setTimeout(() => handleSendRef.current?.(text), 100)
  }, [])
  const { speechAvailable, isRecording, isProcessingVoice, toggleRecording } =
    useVoiceRecording(voiceResultHandler, locale)

  // Refs
  const handleAgentResponseRef =
    useRef<(messageId: string, response: AgentResponse) => Promise<void>>(null)
  const createProgressHandlerRef = useRef<
    ((assistantMessageId: string) => (event: ProgressEvent) => void) | null
  >(null)
  const answerQuestionRef =
    useRef<(messageId: string, answer: string) => void>(null)
  const sessionTitleSetRef = useRef(false)
  const handledResponseKeysRef = useRef(new Set<string>())
  const loadingMessageIdRef = useRef<string | null>(null)
  const sessionIdRef = useRef(sessionId)
  sessionIdRef.current = sessionId
  const [sessionTitle, setSessionTitle] = useState<string | null>(null)

  // 长按检测（提取到 useLongPress hook）
  const { indicator: longPressIndicator } = useLongPress(
    LONG_PRESS_DURATION,
    useCallback(() => setVisibility('visible'), []),
    visibility === 'hidden',
  )

  // DOM 引用
  const inputRef = useRef<HTMLInputElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const messagesEndRef = useRef<HTMLDivElement>(null)
  const messagesListRef = useRef<HTMLDivElement>(null)
  const isNearBottomRef = useRef(true)

  // 调试日志（最近 30 条原始数据 / SSE 事件 / 错误）
  const debugLogRef = useRef<DebugLogEntry[]>([])
  const pushDebugLog = useCallback(
    (type: DebugLogEntry['type'], data: unknown) => {
      const log = debugLogRef.current
      log.push({ time: new Date().toISOString(), type, data })
      if (log.length > 30) log.splice(0, log.length - 30)
    },
    [],
  )
  // 错误状态仍写入（供 debug 日志链路使用）；入口改为 logo 连点，不再常驻 badge
  const [, setLastError] = useState<string | null>(null)

  // 连点触发 debug：空态 hero logo / 非空态左上标题（5 次 / 2s）
  const debugMultiClickRef = useRef<{ count: number; firstAt: number }>({
    count: 0,
    firstAt: 0,
  })
  const handleDebugMultiClick = useCallback(() => {
    const now = Date.now()
    const state = debugMultiClickRef.current
    if (
      state.count === 0 ||
      now - state.firstAt > DEBUG_MULTI_CLICK_WINDOW_MS
    ) {
      state.count = 1
      state.firstAt = now
      return
    }
    state.count += 1
    if (state.count >= DEBUG_MULTI_CLICK_COUNT) {
      state.count = 0
      state.firstAt = 0
      setPanelView((prev) => (prev === 'debug' ? 'chat' : 'debug'))
    }
  }, [])

  // 最近一次执行（用于调试面板）
  const latestExecution = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].taskExecution) return messages[i].taskExecution
    }
    return undefined
  }, [messages])

  // 计算是否有活跃的处理中消息
  const hasActiveExecution = useMemo(
    () =>
      messages.some(
        (m) =>
          m.taskExecution?.status === 'processing' ||
          m.taskExecution?.status === 'waiting' ||
          m.taskExecution?.status === 'cancelling',
      ),
    [messages],
  )

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

  // 自动滚动到底部（仅当用户已在底部附近时）
  useEffect(() => {
    if (messagesEndRef.current && isNearBottomRef.current) {
      messagesEndRef.current.scrollIntoView({ behavior: 'smooth' })
    }
  }, [messages])

  // 跟踪用户是否在底部附近
  useEffect(() => {
    const el = messagesListRef.current
    if (!el) return
    const onScroll = () => {
      const threshold = 80
      isNearBottomRef.current =
        el.scrollHeight - el.scrollTop - el.clientHeight < threshold
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => el.removeEventListener('scroll', onScroll)
  }, [])

  // ============ 预设管理 ============

  const loadPresets = useCallback(async () => {
    try {
      const response = await agentService.getPresets()
      setPresetFavorites(response.favorites)
    } catch (error) {
      console.error('[AraelPanel] 加载预设失败:', error)
    }
  }, [])

  useEffect(() => {
    if (visibility === 'visible') {
      loadPresets()
    }
  }, [visibility, loadPresets])

  const smartGreeting = useMemo(
    () => getSmartGreeting(location.pathname, 0, t.arael),
    [visibility, location.pathname, t.arael],
  )

  const loadContinueSessions = useCallback(async () => {
    try {
      const sessions = await agentService.listSessions(1, 10)
      const mapped = sessions
        .filter((s: SessionInfo) => !s.archived)
        .filter((s: SessionInfo) => s.messageCount > 0)
        .slice(0, 2)
        .map((s: SessionInfo) => ({
          id: s.id,
          title: s.title,
          messageCount: s.messageCount,
          lastActiveAt: s.lastActiveAt,
          createdAt: s.createdAt,
        }))
      setContinueSessions(mapped)
    } catch (error) {
      console.error('[AraelPanel] 加载继续会话失败:', error)
      setContinueSessions([])
    }
  }, [])

  useEffect(() => {
    if (
      visibility === 'visible' &&
      panelView === 'chat' &&
      messages.length === 0 &&
      !isLoading
    ) {
      loadContinueSessions()
    }
  }, [visibility, panelView, messages.length, isLoading, loadContinueSessions])

  const togglePresetFavorite = useCallback(
    async (presetId: number) => {
      try {
        await agentService.toggleFavorite(presetId)
        loadPresets()
      } catch (error) {
        console.error('[AraelPanel] 切换收藏状态失败:', error)
      }
    },
    [loadPresets],
  )

  const usePreset = useCallback(async (preset: TaskPreset) => {
    setInput(preset.input)
    setTimeout(() => inputRef.current?.focus(), 100)
  }, [])

  // ============ 会话管理 ============

  const startNewSession = useCallback(async () => {
    // 新建会话只切换前端视图。旧任务由后端 run 持续执行，并通过通知中心报告状态。
    loadingMessageIdRef.current = null
    setIsLoading(false)
    setSessionId(null)
    setMessages([])
    setPanelView('chat')
    sessionTitleSetRef.current = false
    setSessionTitle(null)
  }, [])

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
            if (task.pendingQuestion) {
              pendingQ = {
                questionId: task.pendingQuestion.questionId,
                questionType: task.pendingQuestion.questionType,
                question: task.pendingQuestion.question,
                context: task.pendingQuestion.context,
                options: task.pendingQuestion.options,
                required: task.pendingQuestion.required,
                defaultValue: task.pendingQuestion.defaultValue,
              }
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

          loadingMessageIdRef.current = candidate.messageId
          setIsLoading(true)
          const onProgress =
            createProgressHandlerRef.current?.(candidate.messageId)
          if (!onProgress) continue
          void agentService
            .subscribeRun(runId, onProgress)
            .then((response) => {
              handleAgentResponseRef.current?.(candidate.messageId, response)
            })
            .catch((error) => {
              console.warn('[AraelPanel] reattach stream ended:', error)
            })
            .finally(() => {
              if (loadingMessageIdRef.current === candidate.messageId) {
                loadingMessageIdRef.current = null
                setIsLoading(false)
              }
            })
          // 同一时刻只恢复一条 live stream
          break
        } catch (error) {
          console.warn('[AraelPanel] reattach task probe failed:', error)
        }
      }
    },
    [updateMessage],
  )

  const loadSession = useCallback(
    async (
      session: ChatSession,
      reattachHints?: { runId?: string; taskId?: string },
    ) => {
      setPanelView('chat')
      setSessionId(session.id)
      setSessionTitle(session.title || null)
      sessionTitleSetRef.current = !!session.title

      try {
        const sessionMessages = await agentService.getSessionMessages(
          session.id,
          1,
          50,
        )
        const loaded: ChatMessage[] = sessionMessages.map((m, idx) => {
          const meta = m.metadata as Record<string, unknown> | undefined
          const data = meta?.data as Record<string, unknown> | undefined
          const imageUrls: string[] = []
          if (data && typeof data.imageUrl === 'string') {
            imageUrls.push(data.imageUrl)
          }
          const stepHistory = (
            meta?.task as Record<string, unknown> | undefined
          )?.stepHistory as Array<Record<string, unknown>> | undefined
          if (stepHistory) {
            for (const s of stepHistory) {
              if (
                typeof s.imageUrl === 'string' &&
                !imageUrls.includes(s.imageUrl)
              ) {
                imageUrls.push(s.imageUrl)
              }
            }
          }

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

          let taskExecution: TaskExecution | undefined
          if (metaTaskId || metaRunId) {
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
              steps: [],
            }
          }

          // 从持久化 metadata 恢复等待中的问题（reattach 会再与后端对齐）
          let pendingQuestion: PendingQuestion | undefined
          const pq =
            (meta?.pendingQuestion as Record<string, unknown> | undefined) ||
            (taskMeta?.pendingQuestion as Record<string, unknown> | undefined)
          if (pq && typeof pq.question === 'string') {
            pendingQuestion = {
              questionId: String(pq.questionId ?? pq.question_id ?? ''),
              questionType: String(pq.questionType ?? pq.question_type ?? 'free_text'),
              question: pq.question,
              context:
                typeof pq.context === 'string' ? pq.context : undefined,
              options: pq.options as PendingQuestion['options'],
              required: typeof pq.required === 'boolean' ? pq.required : undefined,
              defaultValue:
                typeof pq.defaultValue === 'string'
                  ? pq.defaultValue
                  : typeof pq.default_value === 'string'
                    ? pq.default_value
                    : undefined,
            }
            if (taskExecution) taskExecution.status = 'waiting'
          }

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
        setMessages(loaded)
        // 刷新 / 通知打开：探测非终态任务并 re-subscribe
        void reattachLiveWork(loaded, reattachHints)
      } catch (error) {
        console.error('[AraelPanel] 加载会话消息失败:', error)
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
      if (typeof sid !== 'string' || !sid) return
      setVisibility('visible')
      void loadSession(
        {
          id: sid,
          title: null,
          messageCount: 0,
          lastActiveAt: '',
        },
        {
          runId: typeof detail?.runId === 'string' ? detail.runId : undefined,
          taskId: typeof detail?.taskId === 'string' ? detail.taskId : undefined,
        },
      )
    }
    window.addEventListener('arael-open-session', handleOpenSession)
    return () =>
      window.removeEventListener('arael-open-session', handleOpenSession)
  }, [loadSession])

  useEffect(() => {
    const handleOpenManage = () => {
      setVisibility('visible')
      setPanelView('manage')
    }
    window.addEventListener('arael-open-manage', handleOpenManage)
    return () =>
      window.removeEventListener('arael-open-manage', handleOpenManage)
  }, [])

  // ============ 中断 ============

  const interruptCurrentTask = useCallback(async () => {
    // 用户意图中断：标记 abort intent=user，SSE 层不会 re-subscribe 同一 run
    agentService.abortCurrentRequest()

    const processingMsgs = messages.filter(
      (m) =>
        m.taskExecution?.status === 'processing' ||
        m.taskExecution?.status === 'waiting' ||
        m.taskExecution?.status === 'cancelling',
    )
    for (const msg of processingMsgs) {
      const taskId = msg.taskExecution?.taskId
      // 先进入 cancelling，避免乐观地显示 error 而后端仍在跑
      updateMessageExecution(msg.id, { status: 'cancelling' })
      if (taskId && !taskId.startsWith('confirmation:')) {
        try {
          await agentService.cancelTask(taskId)
          updateMessage(msg.id, {
            taskExecution: msg.taskExecution
              ? { ...msg.taskExecution, status: 'error' }
              : undefined,
            content: msg.content || t.arael.interrupted,
          })
        } catch {
          updateMessage(msg.id, {
            taskExecution: msg.taskExecution
              ? { ...msg.taskExecution, status: 'error' }
              : undefined,
            content:
              msg.content ||
              `${t.arael.interrupted} (${t.arael.unknownError})`,
          })
        }
      } else {
        updateMessage(msg.id, {
          taskExecution: msg.taskExecution
            ? { ...msg.taskExecution, status: 'error' }
            : undefined,
          content: msg.content || t.arael.interrupted,
        })
      }
    }
    loadingMessageIdRef.current = null
    setIsLoading(false)
  }, [messages, updateMessage, updateMessageExecution])

  // ============ 面板控制 ============

  const closePanel = useCallback(() => {
    setVisibility('hidden')
    setInput('')
    setPanelView('chat')
  }, [])

  useEffect(() => {
    if (visibility === 'hidden') return
    const handleClickOutside = (e: MouseEvent) => {
      if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
        if (hasActiveExecution) return
        closePanel()
      }
    }
    const timer = setTimeout(
      () => document.addEventListener('mousedown', handleClickOutside),
      100,
    )
    return () => {
      clearTimeout(timer)
      document.removeEventListener('mousedown', handleClickOutside)
    }
  }, [visibility, hasActiveExecution, closePanel])

  useEffect(() => {
    if (visibility === 'hidden') return
    const handleKeyDown = (e: KeyboardEvent) => {
      // 与点击外部一致：执行中禁止 Escape 关掉面板，避免用户以为任务已停
      if (e.key === 'Escape') {
        if (hasActiveExecution) return
        closePanel()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [visibility, closePanel, hasActiveExecution])

  useEffect(() => {
    if (visibility === 'visible' && inputRef.current) {
      setTimeout(() => inputRef.current?.focus(), 100)
    }
  }, [visibility])

  // ============ SSE 进度处理 ============

  const createProgressHandler = useCallback(
    (assistantMessageId: string) => {
      let streamedSummary = ''
      return (event: ProgressEvent) => {
        // 记录关键 SSE 事件到调试日志
        if (
          event.type === 'task_created' ||
          event.type === 'task_completed' ||
          event.type === 'error' ||
          (event.type === 'step_completed' &&
            !(event as StepCompletedEvent).success)
        ) {
          pushDebugLog('sse', event)
        }
        switch (event.type) {
          case 'run_started': {
            if (event.sessionId) {
              setSessionId(event.sessionId)
              sessionIdRef.current = event.sessionId
            }
            if (event.runId) {
              updateMessageExecution(assistantMessageId, {
                runId: event.runId,
              })
            }
            break
          }

          case 'session_created': {
            setSessionId(event.sessionId)
            // 同步更新 ref，确保后续同帧事件能立即读到
            sessionIdRef.current = event.sessionId
            break
          }

          case 'session_title_updated': {
            // 后端并行 AI 生成的标题通过 SSE 推送
            if (event.title) {
              sessionTitleSetRef.current = true
              setSessionTitle(event.title)
            }
            break
          }

          case 'task_created': {
            const tcEvent = event as TaskCreatedEvent
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
              setMessages((prev) =>
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
              )
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
            })
            break
          }

          case 'waiting_for_input': {
            const wEvent =
              event as import('../../services/agent/types').WaitingForInputEvent
            const pendingQ: import('./types').PendingQuestion = {
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
            updateMessageExecution(assistantMessageId, { status: 'error' })
            updateMessage(assistantMessageId, { content: event.message })
            break

          case 'summary_token': {
            const tokenEvent = event as SummaryTokenEvent
            if (tokenEvent.done) {
              // 一轮流式结束 — 保存快照到 statusMessage 供思考面板引用
              if (streamedSummary) {
                updateMessageExecution(assistantMessageId, {
                  statusMessage: streamedSummary,
                })
              }
              // 不清 streamedSummary — step_started 事件负责在步骤开始时重置
            } else {
              streamedSummary += tokenEvent.token
              // announce_plan 和 ai_summarize 都写入正文，用户都看得到
              // announce_plan: "好的，让我帮你查一下~"（执行前的温暖感）
              // ai_summarize: "东京25°C，芙莉莲好看~"（执行后的结果）
              // ai_summarize 自然替换 announce_plan（因为 step_started 已重置 streamedSummary）
              updateMessage(assistantMessageId, { content: streamedSummary })
            }
            break
          }

          case 'task_completed': {
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
              if (loadingMessageIdRef.current === assistantMessageId) {
                loadingMessageIdRef.current = null
                setIsLoading(false)
              }
            }
            break
          }

          case 'planner_decision': {
            const pdEvent = event as PlannerDecisionEvent
            pushDebugLog('sse', event)
            setMessages((prev) =>
              prev.map((m) => {
                if (m.id !== assistantMessageId || !m.taskExecution) return m
                const existing = m.taskExecution.debugTrace ?? {
                  stepDebugEntries: [],
                }
                return {
                  ...m,
                  taskExecution: {
                    ...m.taskExecution,
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
            )
            break
          }

          case 'step_debug': {
            const sdEvent = event as StepDebugEvent
            setMessages((prev) =>
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
      pushDebugLog,
    ],
  )

  createProgressHandlerRef.current = createProgressHandler

  // ============ 发送消息 ============

  const handleSend = useCallback(
    async (text?: string) => {
      const messageText = text || input.trim()
      if (!messageText) return

      // 如果有待回答的问题，将输入路由到 answerQuestion（即使 isLoading 也允许）
      if (pendingAnswerMsg && answerQuestionRef.current) {
        setInput('')
        answerQuestionRef.current(pendingAnswerMsg.id, messageText)
        return
      }

      if (isLoading) {
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
          id: `msg_user_steer_${Date.now()}`,
          sessionId: sessionId || '',
          role: 'user',
          content: messageText,
          createdAt: new Date(),
        }
        setMessages((prev) => [...prev, userMessage])
        setInput('')
        try {
          const result = await agentService.steerSession(
            messageText,
            activeTaskMessage.taskExecution.taskId,
          )
          updateMessageExecution(activeTaskMessage.id, {
            statusMessage: result.message,
          })
        } catch (error) {
          const errorMessage =
            error instanceof Error ? error.message : t.arael.unknownError
          setLastError(errorMessage)
          setMessages((prev) => [
            ...prev,
            {
              id: `msg_assistant_steer_error_${Date.now()}`,
              sessionId: sessionId || '',
              role: 'assistant',
              content: format(t.arael.errorWithDetail, {
                error: errorMessage,
              }),
              createdAt: new Date(),
            },
          ])
        }
        return
      }

      // 切回对话视图
      setPanelView('chat')

      // 1. 创建 user 消息
      const userMsgId = `msg_user_${Date.now()}`
      const userMessage: ChatMessage = {
        id: userMsgId,
        sessionId: sessionId || '',
        role: 'user',
        content: messageText,
        createdAt: new Date(),
      }

      // 2. 创建 placeholder assistant 消息
      const assistantMsgId = `msg_assistant_${Date.now()}`
      const assistantMessage: ChatMessage = {
        id: assistantMsgId,
        sessionId: sessionId || '',
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

      setMessages((prev) => [...prev, userMessage, assistantMessage])
      setInput('')
      loadingMessageIdRef.current = assistantMsgId
      setIsLoading(true)

      try {
        // 构建上下文
        const context: Record<string, unknown> = {
          currentRoute: location.pathname,
        }

        if (sessionId) {
          context.sessionId = sessionId
        }

        // 页面内容
        const customData: Record<string, unknown> = {}
        if (pageContentContext?.hasContent) {
          const contentForAgent = pageContentContext.getContentForAgent()
          if (contentForAgent) {
            customData.pageContent = contentForAgent
          }
        }
        if (Object.keys(customData).length > 0) {
          context.customData = customData
        }

        const response = await agentService.processWithProgress(
          messageText,
          createProgressHandler(assistantMsgId),
          context,
        )

        pushDebugLog('response', response)
        setLastError(null)

        if (handleAgentResponseRef.current) {
          handleAgentResponseRef.current(assistantMsgId, response)
        }
      } catch (error) {
        const errorMsg =
          error instanceof Error ? error.message : t.arael.unknownError
        pushDebugLog('error', {
          message: errorMsg,
          stack: error instanceof Error ? error.stack : undefined,
        })
        setLastError(errorMsg)

        // 保留已收集的 debugTrace 和步骤信息，只更新状态
        setMessages((prev) =>
          prev.map((m) => {
            if (m.id !== assistantMsgId) return m
            const existing = m.taskExecution
            return {
              ...m,
              content:
                m.content ||
                t.arael.errorWithDetail.replace('{error}', errorMsg),
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
        )
      } finally {
        if (loadingMessageIdRef.current === assistantMsgId) {
          loadingMessageIdRef.current = null
          setIsLoading(false)
        }
      }
    },
    [
      input,
      isLoading,
      messages,
      sessionId,
      location.pathname,
      pageContentContext,
      createProgressHandler,
      updateMessage,
      pushDebugLog,
      pendingAnswerMsg,
    ],
  )

  useEffect(() => {
    handleSendRef.current = handleSend
  }, [handleSend])

  // ============ 处理 Agent 响应 ============

  const handleAgentResponse = useCallback(
    async (messageId: string, response: AgentResponse) => {
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
        }
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
      }

      if (pendingQuestion && pendingQuestion.question) {
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
        return
      }

      const isSuccess =
        response.success !== false &&
        (response.task?.status === 'completed' ||
          response.responseType === 'answer' ||
          response.responseType === 'task_completed')

      const responseData = response.data as Record<string, unknown> | undefined

      const stepHistory = taskData?.stepHistory as
        | Array<{
            stepId: string
            status: string
            outputSummary?: string
            capabilityName?: string
            durationMs?: number
            error?: string
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
        const aiText = responseData
          ? ((typeof responseData.reply === 'string'
              ? responseData.reply
              : undefined) ??
            (typeof responseData.aiSummary === 'string'
              ? responseData.aiSummary
              : undefined) ??
            (typeof responseData.analysis === 'string'
              ? responseData.analysis
              : undefined) ??
            (typeof responseData.summary === 'string'
              ? responseData.summary
              : undefined))
          : undefined
        const dataMessage =
          typeof responseData?.message === 'string'
            ? responseData.message
            : undefined
        displayMessage = aiText || response.message || dataMessage
      }

      // 失败步骤信息追加
      if (stepHistory && stepHistory.length > 0) {
        const failedSteps = stepHistory.filter((s) => s.status === 'failed')
        if (failedSteps.length > 0 && failedSteps.length < stepHistory.length) {
          const failInfo = failedSteps
            .map((s) => s.error || s.outputSummary || t.arael.executionFailed)
            .join('；')
          displayMessage = `${displayMessage || ''}\n${format(t.arael.failReason, { reason: failInfo })}`
        } else if (failedSteps.length === stepHistory.length) {
          displayMessage = t.arael.executionFailed
          const failInfo = failedSteps
            .map((s) => s.error || s.outputSummary || t.arael.unknownError)
            .join('；')
          displayMessage += `\n${failInfo}`
        }
      }

      console.log('[Arael] handleAgentResponse:', {
        responseType: response.responseType,
        message: response.message,
        isMultiStep,
        dataKeys: responseData ? Object.keys(responseData) : [],
        displayMessage,
      })

      // 从 response.data 和 stepHistory 中兜底提取 imageUrls（SSE 丢失时恢复）
      // 与已通过 SSE 实时收集的 imageUrls 合并（不覆盖）
      const fallbackImageUrls: string[] = []
      if (typeof responseData?.imageUrl === 'string') {
        fallbackImageUrls.push(responseData.imageUrl as string)
      }
      if (stepHistory) {
        for (const s of stepHistory) {
          const url = (s as Record<string, unknown>).imageUrl
          if (typeof url === 'string' && !fallbackImageUrls.includes(url)) {
            fallbackImageUrls.push(url)
          }
        }
      }

      // 合并：SSE 实时收集的 + fallback，去重
      const existingImageUrls: string[] = ((): string[] => {
        const msg = messagesRef.current.find((m) => m.id === messageId)
        return msg?.imageUrls ?? []
      })()
      const mergedImageUrls = [...existingImageUrls]
      for (const url of fallbackImageUrls) {
        if (!mergedImageUrls.includes(url)) {
          mergedImageUrls.push(url)
        }
      }

      updateMessage(messageId, {
        content: displayMessage || response.message || t.arael.taskCompleted,
        suggestions: response.suggestions?.length
          ? response.suggestions
          : undefined,
        data: response.data,
        pendingQuestion: undefined,
        selectedAnswer: undefined,
        ...(mergedImageUrls.length > 0 ? { imageUrls: mergedImageUrls } : {}),
      })

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
      })

      // 执行前端动作
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

      if (
        frontendActions &&
        Array.isArray(frontendActions) &&
        frontendActions.length > 0
      ) {
        const visibleResults: unknown[] = []
        for (const action of frontendActions) {
          if (!action) continue
          try {
            const result = await executeFrontendAction(action)
            if (
              result &&
              typeof result === 'object' &&
              ['query_windows', 'music_get_status'].includes(action.type)
            ) {
              visibleResults.push(result)
            }
          } catch (error) {
            console.error('[Arael] Frontend action failed:', error)
          }
        }
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
      } else if (frontendAction) {
        try {
          const result = await executeFrontendAction(frontendAction)
          if (
            result &&
            typeof result === 'object' &&
            ['query_windows', 'music_get_status'].includes(frontendAction.type)
          ) {
            const serialized = JSON.stringify(result, null, 2).slice(0, 4000)
            updateMessage(messageId, {
              content: `${displayMessage || response.message}\n\n\`\`\`json\n${serialized}\n\`\`\``,
              data: {
                ...(responseData ?? {}),
                frontendActionResult: result,
              },
            })
          }
        } catch (error) {
          console.error('[Arael] Frontend action failed:', error)
        }
      }
    },
    [updateMessage, updateMessageExecution],
  )

  useEffect(() => {
    handleAgentResponseRef.current = handleAgentResponse
  }, [handleAgentResponse])

  // ============ 回答问题 ============

  const answerQuestion = useCallback(
    async (messageId: string, answer: string) => {
      const msg = messages.find((m) => m.id === messageId)
      if (!msg?.taskExecution?.taskId || !msg.pendingQuestion) return

      // 保留 pendingQuestion 以显示选中状态，同时用 selectedAnswer 锁定
      updateMessage(messageId, {
        selectedAnswer: answer,
      })
      updateMessageExecution(messageId, {
        status: 'processing',
        progress: 50,
      })
      loadingMessageIdRef.current = messageId
      setIsLoading(true)

      try {
        const response = msg.pendingQuestion.confirmationId
          ? await agentService.confirmOperation(
              msg.pendingQuestion.confirmationId,
              answer === 'confirm',
              undefined,
              createProgressHandler(messageId),
            )
          : await agentService.answerQuestionWithProgress(
              msg.taskExecution.taskId,
              msg.pendingQuestion.questionId,
              answer,
              createProgressHandler(messageId),
            )
        handleAgentResponseRef.current?.(messageId, response)
      } catch (error) {
        const errorMsg =
          error instanceof Error ? error.message : t.arael.unknownError
        updateMessage(messageId, {
          content: format(t.arael.answerFailed, { error: errorMsg }),
        })
        updateMessageExecution(messageId, { status: 'error' })
      } finally {
        // 安全保障：回答流完成后确保 isLoading 归位
        if (loadingMessageIdRef.current === messageId) {
          loadingMessageIdRef.current = null
          setIsLoading(false)
        }
      }
    },
    [messages, updateMessage, updateMessageExecution, createProgressHandler],
  )

  useEffect(() => {
    answerQuestionRef.current = answerQuestion
  }, [answerQuestion])

  // 键盘事件（IME 组字中确认候选时不发送）
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (
        e.key === 'Enter' &&
        !e.shiftKey &&
        !isImeComposing(e)
      ) {
        e.preventDefault()
        handleSend()
      }
    },
    [handleSend],
  )

  // ============ 渲染 ============

  const showFavorites =
    visibility === 'visible' &&
    presetFavorites.length > 0 &&
    !hasActiveExecution &&
    !isLoading
  const hasContent =
    panelView !== 'chat' ||
    messages.length > 0 ||
    (visibility === 'visible' && !isLoading)

  return (
    <div className="arael-panel-container">
      <AnimatePresence>
        {visibility === 'visible' && (
          <motion.div
            ref={panelRef}
            className={`arael-panel${hasActiveExecution ? ' arael-panel-processing' : ''}`}
            initial={{ opacity: 0, y: -20, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -20, scale: 0.96 }}
            transition={SPRING_SNAPPY}
          >
            {/* 顶部区域 */}
            <div
              className="arael-tasks-wrapper"
              data-expanded={hasContent ? 'true' : 'false'}
            >
              <div className="arael-tasks-inner">
                {/* 标题栏 */}
                <div className="arael-tasks-header">
                  {/* 非空态：连点左上标题切换 debug；空态主入口为 empty hero */}
                  <span
                    className="arael-tasks-title"
                    onClick={handleDebugMultiClick}
                  >
                    {hasActiveExecution ? (
                      <span className="arael-tasks-title-rest">
                        {sessionTitle || t.arael.processing}
                      </span>
                    ) : (
                      <span className="arael-tasks-title-rest">
                        {sessionTitle ||
                          smartGreeting.replace(ARAEL_PREFIX_RE, '')}
                      </span>
                    )}
                  </span>
                  <div className="arael-tasks-actions">
                    {/* 新对话 */}
                    <button
                      className="arael-header-btn"
                      onClick={startNewSession}
                      title={t.arael.newSession}
                    >
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.5"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <line x1="12" y1="5" x2="12" y2="19" />
                        <line x1="5" y1="12" x2="19" y2="12" />
                      </svg>
                    </button>
                    {/* 会话列表 */}
                    <button
                      className={`arael-header-btn${panelView === 'sessions' ? ' active' : ''}`}
                      onClick={() =>
                        setPanelView(
                          panelView === 'sessions' ? 'chat' : 'sessions',
                        )
                      }
                      title={t.arael.historyTitle}
                    >
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <circle cx="12" cy="12" r="10" />
                        <polyline points="12 6 12 12 16 14" />
                      </svg>
                    </button>
                    {/* 管理 */}
                    <button
                      className={`arael-header-btn${panelView === 'manage' ? ' active' : ''}`}
                      onClick={() =>
                        setPanelView(panelView === 'manage' ? 'chat' : 'manage')
                      }
                      title={t.arael.manage}
                    >
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <circle cx="12" cy="12" r="3" />
                        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
                      </svg>
                    </button>
                  </div>
                </div>

                {/* 面板视图切换 */}
                {panelView === 'debug' && (
                  <AraelDebugPanel
                    execution={latestExecution}
                    onClose={() => setPanelView('chat')}
                  />
                )}

                {panelView === 'sessions' && (
                  <AraelSessionList
                    activeSessionId={sessionId}
                    onSelectSession={loadSession}
                  />
                )}

                {panelView === 'manage' && <AraelManageDrawer />}

                {panelView === 'chat' && (
                  <div className="arael-msg-list" ref={messagesListRef}>
                    {messages.length === 0 && !isLoading && (
                      <div className="arael-empty-state">
                        {/* Hero */}
                        <div className="arael-empty-hero">
                          <button
                            type="button"
                            className="arael-empty-hero-name qwitcher-grypen"
                            onClick={handleDebugMultiClick}
                            aria-label="Arael"
                          >
                            Arael
                          </button>
                          <span className="arael-empty-hero-sub">
                            {t.arael.heroSub}
                          </span>
                        </div>

                        {/* Recent sessions */}
                        <div className="arael-empty-recent">
                          {continueSessions.length > 0 && (
                            <div className="arael-empty-recent-label">
                              <svg
                                width="12"
                                height="12"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                strokeWidth="2"
                                strokeLinecap="round"
                                strokeLinejoin="round"
                              >
                                <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                              </svg>
                              <span>{t.arael.recentConversations}</span>
                            </div>
                          )}
                          <div className="arael-empty-continue-list">
                            {continueSessions.length > 0 ? (
                              continueSessions.map((session) => (
                                <button
                                  key={session.id}
                                  className="arael-empty-continue-btn"
                                  onClick={() => loadSession(session)}
                                >
                                  <span className="arael-empty-continue-title">
                                    {session.title ||
                                      t.arael.unnamedConversation}
                                  </span>
                                </button>
                              ))
                            ) : (
                              <span className="arael-empty-hint">
                                {t.arael.noRecentConversations}
                              </span>
                            )}
                          </div>
                        </div>
                      </div>
                    )}
                    {messages.map((msg, idx) => (
                      <AraelChatMessage
                        key={msg.id}
                        message={msg}
                        onRetry={() => {
                          // 找到此 assistant 消息之前最近的 user 消息
                          const userMsg = messages
                            .slice(0, idx)
                            .reverse()
                            .find((m) => m.role === 'user')
                          if (userMsg) {
                            handleSendRef.current?.(userMsg.content)
                          }
                        }}
                        onAnswerQuestion={answerQuestion}
                        onSuggestionClick={(text) =>
                          handleSendRef.current?.(text)
                        }
                      />
                    ))}
                    <div ref={messagesEndRef} />
                  </div>
                )}
              </div>
            </div>

            {/* 底部输入框 - 仅在聊天视图显示 */}
            {panelView === 'chat' && (
              <AraelInput
                value={input}
                onChange={setInput}
                onSubmit={() => handleSend()}
                onKeyDown={handleKeyDown}
                isLoading={isLoading && !pendingAnswerMsg}
                allowSubmitWhileLoading={
                  isLoading &&
                  !pendingAnswerMsg &&
                  messages.some(
                    (message) =>
                      message.taskExecution?.status === 'processing' &&
                      !!message.taskExecution.taskId &&
                      !message.taskExecution.taskId.startsWith('confirmation:'),
                  )
                }
                isRecording={isRecording}
                isProcessingVoice={isProcessingVoice}
                onToggleRecording={
                  speechAvailable ? toggleRecording : undefined
                }
                onInterrupt={
                  hasActiveExecution && !pendingAnswerMsg
                    ? interruptCurrentTask
                    : undefined
                }
                placeholder={pendingAnswerMsg ? t.arael.inputAnswer : undefined}
                inputRef={inputRef}
              />
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* 收藏胶囊 */}
      <AraelPresets
        favorites={presetFavorites}
        isVisible={showFavorites}
        onUsePreset={usePreset}
        onToggleFavorite={togglePresetFavorite}
      />

      {/* 长按提示动效 */}
      <div
        className={`arael-longpress-indicator${longPressIndicator.active ? ' active' : ''}`}
        style={{ left: longPressIndicator.x, top: longPressIndicator.y }}
      >
        <div className="arael-lp-dot" />
        <div className="arael-lp-pulse" />
        <svg className="arael-lp-svg" viewBox="0 0 40 40">
          <circle className="arael-lp-track" cx="20" cy="20" r="16" />
          <circle className="arael-lp-ring" cx="20" cy="20" r="16" />
        </svg>
      </div>
    </div>
  )
}

export default AraelPanel
