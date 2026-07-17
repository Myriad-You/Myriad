/**
 * Tapp Playground 底部控制岛（Composer）
 *
 * 重构后的信息架构，自上而下：
 * - 通知层：错误/警告/成功卡片浮在岛上方，独立卡片、可关闭
 * - 状态带：生成中为紧凑两行（标题+计时 / 阶段字幕），标题带流光扫字；
 *   失败时同构两行 + Retry；完成后显示验证徽标与可展开的 Agent 轨迹
 * - 输入区：多行输入独占一行（composer 范式）
 * - 工具栏：左侧版本导航、历史/会话面板与清空，右侧安装与生成主操作
 */

import type {
  PlaygroundAgentStep,
  PlaygroundKnowledgeSource,
  PlaygroundValidationReport,
} from '../services/TappPlaygroundService'
import {
  FaArrowUp,
  FaCheck,
  FaChevronDown,
  FaDownload,
  FaHistory,
  FaPlus,
  FaRedo,
  FaTimes,
  FaTrash,
  FaUndo,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { PlaygroundTraceIcon } from './PlaygroundIcons'

/** Persisted generate failure for status-band + Retry (localStorage via parent) */
export interface PlaygroundLastFailedAttempt {
  instruction: string
  error: string
  elapsedMs: number
  finishedAt: number
  origin: 'user' | 'runtime-repair'
  /**
   * Frozen busy-phase index (plan/retrieve/code/validate) from elapsed time.
   * Kept for older sessions; prefer `lastStepSummary` when present.
   */
  phaseIndex?: number
  /** Latest real agent step summary observed before failure (stream path). */
  lastStepSummary?: string
}

export interface PlaygroundHistoryRevisionItem {
  id: string
  index: number
  instruction: string
  createdAt: number
  origin?: 'user' | 'runtime-repair' | 'manual'
  explanation?: string
}

export interface PlaygroundSessionSummary {
  id: string
  title: string
  updatedAt: number
  revisionCount: number
}

export interface PlaygroundExamplePrompt {
  id: string
  label: string
  prompt: string
}

export interface PlaygroundComposerProps {
  /** 桌面浮动布局（absolute）或移动端固定布局（fixed） */
  interactive: boolean
  busy: boolean
  busyMode: 'user' | 'runtime-repair'
  /**
   * Latest real agent step summary from the generate stream.
   * When null/empty while busy, a short starting fallback is shown.
   */
  busyStepSummary?: string | null
  installing: boolean
  exporting?: boolean
  hasProject: boolean
  instruction: string
  revisionIndex: number
  revisionCount: number
  /** Current session revision timeline (for history panel) */
  historyRevisions?: PlaygroundHistoryRevisionItem[]
  /** Multi-session list (newest first) */
  sessions?: PlaygroundSessionSummary[]
  activeSessionId?: string
  error: string
  previewError: string
  notice: string
  warnings: string[]
  agentTrace?: PlaygroundAgentStep[]
  knowledgeSources?: PlaygroundKnowledgeSource[]
  validation?: PlaygroundValidationReport
  /** Persisted last failed generate attempt (localStorage via parent) */
  lastFailedAttempt?: PlaygroundLastFailedAttempt | null
  /** Last successful generation elapsed ms (for status band). */
  lastSuccessElapsedMs?: number | null
  /** One-click example prompts (empty create state). */
  examplePrompts?: PlaygroundExamplePrompt[]
  /** Dismissible preview capability note (null/empty = hidden). */
  capabilityNote?: string
  /** One-shot localStorage prune notice. */
  storageNotice?: string
  onInstructionChange: (value: string) => void
  onSubmit: () => void
  /** Cancel in-flight generation (shown while busy). */
  onCancel?: () => void
  onInstall: () => void
  onExport?: () => void
  onMoveRevision: (delta: number) => void
  onJumpToRevision?: (index: number) => void
  onClear: () => void
  onCreateSession?: () => void
  onSwitchSession?: (sessionId: string) => void
  onDeleteSession?: (sessionId: string) => void
  onDismissError: () => void
  onDismissPreviewError: () => void
  onDismissNotice: () => void
  onRetryFailed?: () => void
  onDismissFailed?: () => void
  onPickExample?: (prompt: string) => void
  onDismissCapabilityNote?: () => void
  onDismissStorageNotice?: () => void
}

function originLabel(
  origin: 'user' | 'runtime-repair' | 'manual' | undefined,
  copy: {
    playgroundOriginUser: string
    playgroundOriginRepair: string
    playgroundOriginManual: string
  },
): string {
  if (origin === 'runtime-repair') return copy.playgroundOriginRepair
  if (origin === 'manual') return copy.playgroundOriginManual
  return copy.playgroundOriginUser
}

function formatRelativeTime(
  timestamp: number,
  labels: {
    justNow: string
    minutesAgo: string
    hoursAgo: string
    daysAgo: string
  },
  fmt: (template: string, params: Record<string, string | number>) => string,
): string {
  const diffMs = Date.now() - timestamp
  const diffMin = Math.floor(diffMs / 60000)
  if (diffMin < 1) return labels.justNow
  if (diffMin < 60) return fmt(labels.minutesAgo, { n: diffMin })
  const diffHour = Math.floor(diffMin / 60)
  if (diffHour < 24) return fmt(labels.hoursAgo, { n: diffHour })
  const diffDay = Math.floor(diffHour / 24)
  if (diffDay < 7) return fmt(labels.daysAgo, { n: diffDay })
  return new Date(timestamp).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
  })
}

function summarizeInstruction(text: string, max = 48): string {
  const cleaned = text.replace(/\s+/g, ' ').trim()
  if (!cleaned) return ''
  if (cleaned.length <= max) return cleaned
  return `${cleaned.slice(0, max - 1)}…`
}

/* ---------- 通知卡片 ---------- */

function NotificationCard({
  tone,
  onDismiss,
  dismissLabel,
  children,
}: {
  tone: 'error' | 'warning' | 'success'
  onDismiss?: () => void
  dismissLabel: string
  children: React.ReactNode
}) {
  const dotClass = {
    error: 'bg-red-500',
    warning: 'bg-amber-500',
    success: 'bg-emerald-500',
  }[tone]
  const textClass = {
    error: 'text-red-600 dark:text-red-300',
    warning: 'text-amber-700 dark:text-amber-300',
    success: 'text-emerald-700 dark:text-emerald-300',
  }[tone]

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 10, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: 6, scale: 0.98 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      className="rounded-2xl bg-white/90 dark:bg-[#1c1c1e]/90 backdrop-blur-xl shadow-lg ring-1 ring-black/5 dark:ring-white/10 overflow-hidden"
    >
      <div className="flex items-start gap-2.5 px-3.5 py-2.5">
        {tone === 'success' ? (
          <FaCheck className="mt-1 w-2.5 h-2.5 shrink-0 text-emerald-500" />
        ) : (
          <span className={`mt-1.5 w-2 h-2 rounded-full shrink-0 ${dotClass}`} />
        )}
        <span
          className={`min-w-0 flex-1 break-words whitespace-pre-line text-xs leading-relaxed ${textClass}`}
        >
          {children}
        </span>
        {onDismiss && (
          <button
            onClick={onDismiss}
            className="shrink-0 -m-1 p-1 rounded-md text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
            aria-label={dismissLabel}
          >
            <FaTimes className="w-2.5 h-2.5" />
          </button>
        )}
      </div>
    </motion.div>
  )
}

/* ---------- 控制岛 ---------- */

function formatElapsedClock(totalSeconds: number): string {
  return `${Math.floor(totalSeconds / 60)}:${String(totalSeconds % 60).padStart(2, '0')}`
}

/** Short chip label from a full example prompt (first ~22 chars). */
function exampleChipLabel(text: string, max = 22): string {
  const cleaned = text.replace(/\s+/g, ' ').trim()
  if (cleaned.length <= max) return cleaned
  return `${cleaned.slice(0, max - 1)}…`
}

export function PlaygroundComposer({
  interactive,
  busy,
  busyMode,
  busyStepSummary = null,
  installing,
  exporting = false,
  hasProject,
  instruction,
  revisionIndex,
  revisionCount,
  historyRevisions = [],
  sessions = [],
  activeSessionId,
  error,
  previewError,
  notice,
  warnings,
  agentTrace,
  knowledgeSources,
  validation,
  lastFailedAttempt,
  lastSuccessElapsedMs = null,
  examplePrompts = [],
  capabilityNote,
  storageNotice,
  onInstructionChange,
  onSubmit,
  onCancel,
  onInstall,
  onExport,
  onMoveRevision,
  onJumpToRevision,
  onClear,
  onCreateSession,
  onSwitchSession,
  onDeleteSession,
  onDismissError,
  onDismissPreviewError,
  onDismissNotice,
  onRetryFailed,
  onDismissFailed,
  onPickExample,
  onDismissCapabilityNote,
  onDismissStorageNotice,
}: PlaygroundComposerProps) {
  const { t, format } = useI18n()
  const animConfig = useAnimationLevel()
  const animationsEnabled = animConfig.level !== 'none'
  const springTransition = animConfig.spring
    ? ({ type: 'spring', stiffness: 400, damping: 30 } as const)
    : ({ type: 'tween', duration: 0.25 * animConfig.durationScale } as const)

  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const historyPanelRef = useRef<HTMLDivElement>(null)
  const [traceOpen, setTraceOpen] = useState(false)
  const [failedDetailOpen, setFailedDetailOpen] = useState(true)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [historyTab, setHistoryTab] = useState<
    'revisions' | 'sessions' | 'memory'
  >('revisions')

  const relativeLabels = {
    justNow: t.tapp.playgroundTimeJustNow,
    minutesAgo: t.tapp.playgroundTimeMinutesAgo,
    hoursAgo: t.tapp.playgroundTimeHoursAgo,
    daysAgo: t.tapp.playgroundTimeDaysAgo,
  }

  // 输入框自适应高度
  useEffect(() => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`
  }, [instruction])

  // 生成计时：驱动阶段推进与耗时显示
  const [busyElapsed, setBusyElapsed] = useState(0)
  useEffect(() => {
    if (!busy) {
      setBusyElapsed(0)
      return
    }
    const startedAt = Date.now()
    const timer = window.setInterval(() => {
      setBusyElapsed(Math.floor((Date.now() - startedAt) / 1000))
    }, 1000)
    return () => window.clearInterval(timer)
  }, [busy])

  // Fallback labels for failed runs that only stored a time-based phaseIndex
  // (or no stream step). Busy UI uses real stream summaries, not these.
  const phases = [
    { label: t.tapp.playgroundPhasePlan, desc: t.tapp.playgroundPhasePlanDesc },
    {
      label: t.tapp.playgroundPhaseRetrieve,
      desc: t.tapp.playgroundPhaseRetrieveDesc,
    },
    { label: t.tapp.playgroundPhaseCode, desc: t.tapp.playgroundPhaseCodeDesc },
    {
      label: t.tapp.playgroundPhaseValidate,
      desc: t.tapp.playgroundPhaseValidateDesc,
    },
  ]
  const elapsedLabel = formatElapsedClock(busyElapsed)

  const liveStepText =
    (busyStepSummary && busyStepSummary.trim()) ||
    t.tapp.playgroundPhasePlanDesc

  const failedElapsedLabel = lastFailedAttempt
    ? formatElapsedClock(
        Math.max(0, Math.floor(lastFailedAttempt.elapsedMs / 1000)),
      )
    : ''
  const successElapsedLabel =
    lastSuccessElapsedMs != null && lastSuccessElapsedMs >= 0
      ? formatElapsedClock(Math.max(0, Math.floor(lastSuccessElapsedMs / 1000)))
      : ''
  const failedPhaseIndex = Math.min(
    3,
    Math.max(
      0,
      lastFailedAttempt?.phaseIndex ??
        (lastFailedAttempt
          ? lastFailedAttempt.elapsedMs < 5000
            ? 0
            : lastFailedAttempt.elapsedMs < 14000
              ? 1
              : lastFailedAttempt.elapsedMs < 90000
                ? 2
                : 3
          : 0),
    ),
  )
  const failedStepText =
    (lastFailedAttempt?.lastStepSummary &&
      lastFailedAttempt.lastStepSummary.trim()) ||
    phases[failedPhaseIndex]?.label ||
    ''

  // Collapse open detail when a new failure arrives so users see the error
  useEffect(() => {
    if (lastFailedAttempt) setFailedDetailOpen(true)
  }, [lastFailedAttempt?.finishedAt])

  // Close history panel on outside click / Escape
  useEffect(() => {
    if (!historyOpen) return
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node | null
      if (
        historyPanelRef.current &&
        target &&
        !historyPanelRef.current.contains(target)
      ) {
        setHistoryOpen(false)
      }
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setHistoryOpen(false)
    }
    document.addEventListener('mousedown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('mousedown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [historyOpen])

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (
      event.key === 'Enter' &&
      !event.shiftKey &&
      !event.nativeEvent.isComposing
    ) {
      event.preventDefault()
      onSubmit()
    }
  }

  const handleClear = () => {
    if (busy) return
    if (
      revisionCount > 0 &&
      !window.confirm(t.tapp.playgroundClearConfirm)
    ) {
      return
    }
    onClear()
  }

  const handleDeleteSession = (sessionId: string) => {
    if (busy || !onDeleteSession) return
    if (!window.confirm(t.tapp.playgroundDeleteSessionConfirm)) return
    onDeleteSession(sessionId)
  }

  const orderedRevisions = [...historyRevisions].reverse()
  // Modification chain: chronological user→agent turns (oldest first) + failed tail
  const memoryChain = historyRevisions

  return (
    <motion.div
      initial={animationsEnabled ? { opacity: 0, y: 24 } : false}
      animate={{ opacity: 1, y: 0 }}
      transition={springTransition}
      className={`z-50 ${
        interactive
          ? 'absolute bottom-4 left-1/2 -translate-x-1/2 w-[min(94vw,46rem)]'
          : 'fixed bottom-3 inset-x-3'
      }`}
    >
      {/* ---------- 通知层：浮在岛上方的独立卡片 ---------- */}
      <div className="mb-2 space-y-1.5 max-h-48 overflow-y-auto">
        <AnimatePresence initial={false}>
          {error && (
            <NotificationCard
              key="error"
              tone="error"
              onDismiss={onDismissError}
              dismissLabel={t.common.close}
            >
              {error}
            </NotificationCard>
          )}
          {previewError && (
            <NotificationCard
              key="preview-error"
              tone="warning"
              onDismiss={onDismissPreviewError}
              dismissLabel={t.common.close}
            >
              {previewError}
            </NotificationCard>
          )}
          {capabilityNote && (
            <NotificationCard
              key="capability-note"
              tone="warning"
              onDismiss={onDismissCapabilityNote}
              dismissLabel={
                t.tapp.playgroundPreviewCapabilitiesDismiss || t.common.close
              }
            >
              {capabilityNote}
            </NotificationCard>
          )}
          {storageNotice && (
            <NotificationCard
              key="storage-notice"
              tone="warning"
              onDismiss={onDismissStorageNotice}
              dismissLabel={t.common.close}
            >
              {storageNotice}
            </NotificationCard>
          )}
          {warnings.map((warning) => (
            <NotificationCard
              key={`warning-${warning}`}
              tone="warning"
              dismissLabel={t.common.close}
            >
              {warning}
            </NotificationCard>
          ))}
          {notice && (
            <NotificationCard
              key="notice"
              tone="success"
              onDismiss={onDismissNotice}
              dismissLabel={t.common.close}
            >
              {notice}
            </NotificationCard>
          )}
        </AnimatePresence>
      </div>

      {/* ---------- 岛本体 ---------- */}
      <div className="relative">
        {/* 生成中环绕岛的旋转光晕 */}
        <AnimatePresence>
          {busy && animationsEnabled && (
            <motion.div
              key="island-aura"
              className="playground-island-aura"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.4 }}
            />
          )}
        </AnimatePresence>

        <div
          ref={historyPanelRef}
          className="relative rounded-[1.6rem] bg-white/90 dark:bg-[#1a1a1a]/90 backdrop-blur-xl shadow-2xl ring-1 ring-black/5 dark:ring-white/10 overflow-hidden"
        >
          {/* ---------- 历史 / 会话面板 ---------- */}
          <AnimatePresence initial={false}>
            {historyOpen && (
              <motion.div
                key="history-panel"
                initial={
                  animationsEnabled
                    ? { opacity: 0, height: 0 }
                    : { opacity: 1, height: 'auto' }
                }
                animate={{ opacity: 1, height: 'auto' }}
                exit={
                  animationsEnabled
                    ? { opacity: 0, height: 0 }
                    : { opacity: 0 }
                }
                transition={{ duration: 0.22, ease: 'easeOut' }}
                className="overflow-hidden border-b border-black/5 dark:border-white/5"
              >
                <div className="px-3 pt-3 pb-2">
                  <div className="flex items-center gap-1.5 mb-2">
                    <div className="flex items-center rounded-full bg-black/5 dark:bg-white/10 p-0.5 max-w-full overflow-x-auto">
                      <button
                        type="button"
                        onClick={() => setHistoryTab('revisions')}
                        className={`h-7 px-2.5 rounded-full text-[10px] font-semibold transition-colors whitespace-nowrap ${
                          historyTab === 'revisions'
                            ? 'bg-white dark:bg-white/20 shadow-sm text-gray-900 dark:text-white'
                            : 'text-gray-500 dark:text-gray-400'
                        }`}
                      >
                        {t.tapp.playgroundHistory}
                      </button>
                      <button
                        type="button"
                        onClick={() => setHistoryTab('memory')}
                        className={`h-7 px-2.5 rounded-full text-[10px] font-semibold transition-colors whitespace-nowrap ${
                          historyTab === 'memory'
                            ? 'bg-white dark:bg-white/20 shadow-sm text-gray-900 dark:text-white'
                            : 'text-gray-500 dark:text-gray-400'
                        }`}
                      >
                        {t.tapp.playgroundMemoryChain}
                      </button>
                      <button
                        type="button"
                        onClick={() => setHistoryTab('sessions')}
                        className={`h-7 px-2.5 rounded-full text-[10px] font-semibold transition-colors whitespace-nowrap ${
                          historyTab === 'sessions'
                            ? 'bg-white dark:bg-white/20 shadow-sm text-gray-900 dark:text-white'
                            : 'text-gray-500 dark:text-gray-400'
                        }`}
                      >
                        {t.tapp.playgroundSessions}
                      </button>
                    </div>
                    <button
                      type="button"
                      onClick={() => setHistoryOpen(false)}
                      className="ml-auto shrink-0 p-1.5 rounded-md text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
                      aria-label={t.common.close}
                    >
                      <FaTimes className="w-2.5 h-2.5" />
                    </button>
                  </div>

                  {historyTab === 'memory' ? (
                    <div className="max-h-[min(42vh,16rem)] overflow-y-auto playground-history-scroll">
                      {memoryChain.length === 0 && !lastFailedAttempt ? (
                        <p
                          className="px-2 py-4 text-center text-[11px]"
                          style={{ color: 'var(--text-muted)' }}
                        >
                          {t.tapp.playgroundMemoryChainEmpty}
                        </p>
                      ) : (
                        <ul className="space-y-2.5 px-0.5">
                          {memoryChain.map((rev) => {
                            const isCurrent = rev.index === revisionIndex
                            return (
                              <li key={`mem-${rev.id}`} className="space-y-1">
                                <div className="flex items-center gap-1.5 px-1">
                                  <span
                                    className="text-[9px] font-mono font-bold tabular-nums"
                                    style={{
                                      color: isCurrent
                                        ? 'var(--color-primary)'
                                        : 'var(--text-muted)',
                                    }}
                                  >
                                    v{rev.index + 1}
                                  </span>
                                  <span
                                    className="text-[9px] font-semibold"
                                    style={{ color: 'var(--text-muted)' }}
                                  >
                                    {originLabel(rev.origin, t.tapp)}
                                  </span>
                                  {isCurrent && (
                                    <span
                                      className="text-[9px] font-semibold"
                                      style={{ color: 'var(--color-primary)' }}
                                    >
                                      · {t.tapp.playgroundCurrentRevision}
                                    </span>
                                  )}
                                  <span
                                    className="ml-auto text-[9px] tabular-nums"
                                    style={{ color: 'var(--text-muted)' }}
                                  >
                                    {formatRelativeTime(
                                      rev.createdAt,
                                      relativeLabels,
                                      format,
                                    )}
                                  </span>
                                </div>
                                {/* User / manual turn */}
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => onJumpToRevision?.(rev.index)}
                                  className="w-full text-left rounded-xl px-2.5 py-2 bg-black/[0.03] dark:bg-white/[0.04] hover:bg-black/[0.05] dark:hover:bg-white/[0.07] transition-colors disabled:opacity-50"
                                >
                                  <div
                                    className="text-[9px] font-semibold mb-0.5"
                                    style={{ color: 'var(--color-primary)' }}
                                  >
                                    {rev.origin === 'manual'
                                      ? t.tapp.playgroundOriginManual
                                      : t.tapp.playgroundMemoryUser}
                                  </div>
                                  <p
                                    className="text-[11px] leading-relaxed whitespace-pre-wrap break-words"
                                    style={{ color: 'var(--text-primary)' }}
                                  >
                                    {rev.instruction.trim() || '—'}
                                  </p>
                                </button>
                                {/* Agent turn (skip for pure manual edits — explanation is the edit note) */}
                                {rev.origin === 'manual' ? (
                                  <div className="rounded-xl px-2.5 py-2 bg-black/[0.02] dark:bg-white/[0.03]">
                                    <p
                                      className="text-[11px] leading-relaxed whitespace-pre-wrap break-words"
                                      style={{ color: 'var(--text-muted)' }}
                                    >
                                      {(rev.explanation || '').trim() ||
                                        t.tapp.playgroundMemoryNoExplanation}
                                    </p>
                                  </div>
                                ) : (
                                  <div className="rounded-xl px-2.5 py-2 bg-[color-mix(in_srgb,var(--color-primary),transparent_92%)] ring-1 ring-[color-mix(in_srgb,var(--color-primary),transparent_82%)]">
                                    <div
                                      className="text-[9px] font-semibold mb-0.5"
                                      style={{ color: 'var(--color-primary)' }}
                                    >
                                      {t.tapp.playgroundMemoryAgent}
                                    </div>
                                    <p
                                      className="text-[11px] leading-relaxed whitespace-pre-wrap break-words"
                                      style={{ color: 'var(--text-primary)' }}
                                    >
                                      {(rev.explanation || '').trim() ||
                                        t.tapp.playgroundMemoryNoExplanation}
                                    </p>
                                  </div>
                                )}
                              </li>
                            )
                          })}
                          {lastFailedAttempt && (
                            <li className="space-y-1">
                              <div className="flex items-center gap-1.5 px-1">
                                <span className="w-1.5 h-1.5 rounded-full bg-red-500 shrink-0" />
                                <span className="text-[9px] font-semibold text-red-600 dark:text-red-300">
                                  {t.tapp.playgroundMemoryFailed}
                                </span>
                                <span
                                  className="ml-auto text-[9px] tabular-nums"
                                  style={{ color: 'var(--text-muted)' }}
                                >
                                  {formatRelativeTime(
                                    lastFailedAttempt.finishedAt,
                                    relativeLabels,
                                    format,
                                  )}
                                </span>
                              </div>
                              <div className="rounded-xl border border-red-500/20 bg-red-500/5 px-2.5 py-2">
                                <div
                                  className="text-[9px] font-semibold mb-0.5"
                                  style={{ color: 'var(--color-primary)' }}
                                >
                                  {t.tapp.playgroundMemoryUser}
                                </div>
                                <p className="text-[11px] leading-relaxed whitespace-pre-wrap break-words text-red-700/90 dark:text-red-200/90">
                                  {lastFailedAttempt.instruction}
                                </p>
                                <div className="mt-1.5 pt-1.5 border-t border-red-500/15">
                                  <div className="text-[9px] font-semibold mb-0.5 text-red-600 dark:text-red-300">
                                    {t.tapp.playgroundMemoryFailed}
                                  </div>
                                  <p className="text-[10px] leading-relaxed text-red-600/90 dark:text-red-300/90">
                                    {lastFailedAttempt.error}
                                  </p>
                                </div>
                                {(onRetryFailed || onDismissFailed) && (
                                  <div className="mt-1.5 flex items-center gap-1.5">
                                    {onRetryFailed && (
                                      <button
                                        type="button"
                                        onClick={() => {
                                          setHistoryOpen(false)
                                          onRetryFailed()
                                        }}
                                        disabled={busy}
                                        className="h-6 px-2 rounded-full text-[10px] font-semibold text-white disabled:opacity-40"
                                        style={{
                                          background:
                                            'linear-gradient(135deg, var(--color-primary), color-mix(in srgb, var(--color-primary) 80%, black))',
                                        }}
                                      >
                                        {t.tapp.playgroundRetry}
                                      </button>
                                    )}
                                    {onDismissFailed && (
                                      <button
                                        type="button"
                                        onClick={onDismissFailed}
                                        className="h-6 px-2 rounded-full text-[10px] font-semibold text-gray-500 hover:bg-black/5 dark:hover:bg-white/10"
                                      >
                                        {t.common.close}
                                      </button>
                                    )}
                                  </div>
                                )}
                              </div>
                            </li>
                          )}
                        </ul>
                      )}
                    </div>
                  ) : historyTab === 'revisions' ? (
                    <div className="max-h-[min(42vh,16rem)] overflow-y-auto playground-history-scroll">
                      {orderedRevisions.length === 0 ? (
                        <p
                          className="px-2 py-4 text-center text-[11px]"
                          style={{ color: 'var(--text-muted)' }}
                        >
                          {t.tapp.playgroundHistoryEmpty}
                        </p>
                      ) : (
                        <ul className="space-y-1">
                          {orderedRevisions.map((rev) => {
                            const isCurrent = rev.index === revisionIndex
                            const summary =
                              summarizeInstruction(rev.instruction) ||
                              summarizeInstruction(rev.explanation || '') ||
                              t.tapp.playgroundUntitledSession
                            return (
                              <li key={rev.id}>
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => {
                                    onJumpToRevision?.(rev.index)
                                  }}
                                  title={t.tapp.playgroundJumpToRevision}
                                  className={`w-full text-left rounded-xl px-2.5 py-2 transition-colors disabled:opacity-40 ${
                                    isCurrent
                                      ? 'playground-history-item--active'
                                      : 'hover:bg-black/[0.04] dark:hover:bg-white/[0.05]'
                                  }`}
                                >
                                  <div className="flex items-center gap-2">
                                    <span
                                      className="text-[10px] font-mono font-bold tabular-nums shrink-0"
                                      style={{
                                        color: isCurrent
                                          ? 'var(--color-primary)'
                                          : 'var(--text-muted)',
                                      }}
                                    >
                                      v{rev.index + 1}
                                    </span>
                                    <span
                                      className="min-w-0 flex-1 truncate text-[11px] font-medium"
                                      style={{ color: 'var(--text-primary)' }}
                                    >
                                      {summary}
                                    </span>
                                    <span
                                      className="text-[9px] shrink-0 tabular-nums"
                                      style={{ color: 'var(--text-muted)' }}
                                    >
                                      {formatRelativeTime(
                                        rev.createdAt,
                                        relativeLabels,
                                        format,
                                      )}
                                    </span>
                                  </div>
                                  <div className="mt-0.5 flex items-center gap-1.5 pl-[1.85rem]">
                                    <span
                                      className="text-[9px] font-semibold"
                                      style={{ color: 'var(--text-muted)' }}
                                    >
                                      {originLabel(rev.origin, t.tapp)}
                                    </span>
                                    {isCurrent && (
                                      <span
                                        className="text-[9px] font-semibold"
                                        style={{ color: 'var(--color-primary)' }}
                                      >
                                        · {t.tapp.playgroundCurrentRevision}
                                      </span>
                                    )}
                                  </div>
                                </button>
                              </li>
                            )
                          })}
                        </ul>
                      )}

                      {lastFailedAttempt && (
                        <div className="mt-2 rounded-xl border border-red-500/20 bg-red-500/5 px-2.5 py-2">
                          <div className="flex items-center gap-2">
                            <span className="w-1.5 h-1.5 rounded-full bg-red-500 shrink-0" />
                            <span className="text-[10px] font-semibold text-red-600 dark:text-red-300">
                              {t.tapp.playgroundLastRunFailed}
                            </span>
                            <span
                              className="ml-auto text-[9px] tabular-nums"
                              style={{ color: 'var(--text-muted)' }}
                            >
                              {formatRelativeTime(
                                lastFailedAttempt.finishedAt,
                                relativeLabels,
                                format,
                              )}
                            </span>
                          </div>
                          <p className="mt-1 text-[10px] leading-relaxed text-red-600/90 dark:text-red-300/90 line-clamp-2">
                            {lastFailedAttempt.error}
                          </p>
                          <div className="mt-1.5 flex items-center gap-1.5">
                            {onRetryFailed && (
                              <button
                                type="button"
                                onClick={() => {
                                  setHistoryOpen(false)
                                  onRetryFailed()
                                }}
                                disabled={busy}
                                className="h-6 px-2 rounded-full text-[10px] font-semibold text-white disabled:opacity-40"
                                style={{
                                  background:
                                    'linear-gradient(135deg, var(--color-primary), color-mix(in srgb, var(--color-primary) 80%, black))',
                                }}
                              >
                                {t.tapp.playgroundRetry}
                              </button>
                            )}
                            {onDismissFailed && (
                              <button
                                type="button"
                                onClick={onDismissFailed}
                                className="h-6 px-2 rounded-full text-[10px] font-semibold text-gray-500 hover:bg-black/5 dark:hover:bg-white/10"
                              >
                                {t.common.close}
                              </button>
                            )}
                          </div>
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className="max-h-[min(42vh,16rem)] overflow-y-auto playground-history-scroll">
                      <div className="mb-1.5 flex items-center gap-1.5 px-0.5">
                        <button
                          type="button"
                          onClick={() => {
                            if (busy) return
                            onCreateSession?.()
                          }}
                          disabled={busy || !onCreateSession}
                          className="h-7 px-2.5 rounded-full text-[10px] font-semibold flex items-center gap-1 bg-black/5 dark:bg-white/10 hover:bg-black/10 dark:hover:bg-white/15 transition-colors disabled:opacity-40"
                          style={{ color: 'var(--text-primary)' }}
                        >
                          <FaPlus className="w-2.5 h-2.5" />
                          {t.tapp.playgroundNewSession}
                        </button>
                      </div>
                      {sessions.length === 0 ? (
                        <p
                          className="px-2 py-4 text-center text-[11px]"
                          style={{ color: 'var(--text-muted)' }}
                        >
                          {t.tapp.playgroundSessionsEmpty}
                        </p>
                      ) : (
                        <ul className="space-y-1">
                          {sessions.map((sess) => {
                            const isActive = sess.id === activeSessionId
                            const title =
                              sess.title.trim() || t.tapp.playgroundUntitledSession
                            return (
                              <li
                                key={sess.id}
                                className={`flex items-stretch gap-0.5 rounded-xl ${
                                  isActive ? 'playground-history-item--active' : ''
                                }`}
                              >
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => {
                                    if (!isActive) onSwitchSession?.(sess.id)
                                  }}
                                  className={`min-w-0 flex-1 text-left rounded-xl px-2.5 py-2 transition-colors disabled:opacity-40 ${
                                    isActive
                                      ? ''
                                      : 'hover:bg-black/[0.04] dark:hover:bg-white/[0.05]'
                                  }`}
                                >
                                  <div
                                    className="truncate text-[11px] font-medium"
                                    style={{ color: 'var(--text-primary)' }}
                                  >
                                    {title}
                                  </div>
                                  <div
                                    className="mt-0.5 flex items-center gap-1.5 text-[9px]"
                                    style={{ color: 'var(--text-muted)' }}
                                  >
                                    <span>
                                      {formatRelativeTime(
                                        sess.updatedAt,
                                        relativeLabels,
                                        format,
                                      )}
                                    </span>
                                    <span>·</span>
                                    <span>
                                      {sess.revisionCount > 0
                                        ? `${sess.revisionCount}`
                                        : '—'}
                                    </span>
                                    {isActive && (
                                      <>
                                        <span>·</span>
                                        <span
                                          className="font-semibold"
                                          style={{
                                            color: 'var(--color-primary)',
                                          }}
                                        >
                                          {t.tapp.playgroundActiveSession}
                                        </span>
                                      </>
                                    )}
                                  </div>
                                </button>
                                <button
                                  type="button"
                                  disabled={busy}
                                  onClick={() => handleDeleteSession(sess.id)}
                                  className="shrink-0 w-8 rounded-xl grid place-items-center text-red-400/80 hover:text-red-500 hover:bg-red-500/10 transition-colors disabled:opacity-30"
                                  title={t.tapp.playgroundDeleteSession}
                                  aria-label={t.tapp.playgroundDeleteSession}
                                >
                                  <FaTrash className="w-2.5 h-2.5" />
                                </button>
                              </li>
                            )
                          })}
                        </ul>
                      )}
                    </div>
                  )}
                </div>
              </motion.div>
            )}
          </AnimatePresence>

          {/* ---------- 状态带 ---------- */}
          <AnimatePresence initial={false}>
            {busy ? (
              <motion.div
                key="status-busy"
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                transition={{ duration: 0.25, ease: 'easeOut' }}
                className="overflow-hidden"
              >
                <div className="px-4 pt-3 pb-2.5 border-b border-black/5 dark:border-white/5">
                  {/* Line 1: shimmer title · elapsed */}
                  <div className="flex items-center gap-2">
                    <span
                      className={`text-xs font-semibold truncate playground-text-shimmer${
                        animationsEnabled ? '' : ' playground-text-shimmer--static'
                      }`}
                    >
                      {busyMode === 'runtime-repair'
                        ? t.tapp.playgroundRepairingRuntime
                        : t.tapp.playgroundGenerating}
                    </span>
                    <span
                      className="ml-auto text-[10px] font-mono tabular-nums shrink-0"
                      style={{ color: 'var(--text-muted)' }}
                    >
                      {elapsedLabel}
                    </span>
                  </div>

                  {/* Line 2: latest real agent step (stream), still 2-line UI */}
                  <div className="mt-1.5 relative min-h-[1.25rem] overflow-hidden">
                    <AnimatePresence mode="wait" initial={false}>
                      <motion.p
                        key={`busy-step-${liveStepText}`}
                        initial={
                          animationsEnabled
                            ? { opacity: 0, y: 8, filter: 'blur(2px)' }
                            : false
                        }
                        animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
                        exit={
                          animationsEnabled
                            ? { opacity: 0, y: -6, filter: 'blur(2px)' }
                            : undefined
                        }
                        transition={{
                          duration: 0.32,
                          ease: [0.22, 1, 0.36, 1],
                        }}
                        className="text-[10px] leading-relaxed truncate"
                        style={{ color: 'var(--text-muted)' }}
                      >
                        {liveStepText}
                      </motion.p>
                    </AnimatePresence>
                  </div>
                </div>
              </motion.div>
            ) : lastFailedAttempt ? (
              <motion.div
                key="status-failed"
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                transition={{ duration: 0.25, ease: 'easeOut' }}
                className="overflow-hidden"
              >
                <div className="border-b border-black/5 dark:border-white/5 px-4 pt-3 pb-2.5">
                  {/* Line 1: failed title + elapsed · Retry */}
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      onClick={() => setFailedDetailOpen((open) => !open)}
                      className="min-w-0 flex-1 flex items-center gap-2 text-left hover:opacity-90 transition-opacity"
                      aria-expanded={failedDetailOpen}
                    >
                      <span className="w-2 h-2 rounded-full shrink-0 bg-red-500" />
                      <span className="text-xs font-semibold truncate text-red-600 dark:text-red-300">
                        {t.tapp.playgroundLastRunFailed}
                      </span>
                      <span
                        className="ml-auto text-[10px] font-mono tabular-nums shrink-0"
                        style={{ color: 'var(--text-muted)' }}
                      >
                        {failedElapsedLabel}
                      </span>
                      <motion.span
                        animate={{ rotate: failedDetailOpen ? 180 : 0 }}
                        transition={{ duration: 0.2 }}
                        className="shrink-0"
                        style={{ color: 'var(--text-muted)' }}
                      >
                        <FaChevronDown className="w-2.5 h-2.5" />
                      </motion.span>
                    </button>

                    <button
                      type="button"
                      onClick={onRetryFailed}
                      disabled={!onRetryFailed}
                      className="h-7 shrink-0 rounded-full px-2.5 flex items-center gap-1 text-[10px] font-semibold text-white shadow-sm disabled:opacity-40"
                      style={{
                        background:
                          'linear-gradient(135deg, var(--color-primary), color-mix(in srgb, var(--color-primary) 80%, black))',
                      }}
                      title={t.tapp.playgroundRetry}
                      aria-label={t.tapp.playgroundRetry}
                    >
                      <FaRedo className="w-2.5 h-2.5" />
                      <span>{t.tapp.playgroundRetry}</span>
                    </button>

                    {onDismissFailed && (
                      <button
                        type="button"
                        onClick={onDismissFailed}
                        className="shrink-0 -m-0.5 p-1 rounded-md text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
                        aria-label={t.common.close}
                      >
                        <FaTimes className="w-2.5 h-2.5" />
                      </button>
                    )}
                  </div>

                  {/* Line 2: failed phase / error summary (no 4-dot timeline) */}
                  <button
                    type="button"
                    onClick={() => setFailedDetailOpen((open) => !open)}
                    className="mt-1.5 w-full text-left min-h-[1.25rem]"
                    aria-expanded={failedDetailOpen}
                  >
                    <p
                      className="text-[10px] leading-relaxed truncate"
                      style={{ color: 'var(--text-muted)' }}
                    >
                      <span className="text-red-600/90 dark:text-red-300/90">
                        {lastFailedAttempt?.lastStepSummary
                          ? failedStepText
                          : format(t.tapp.playgroundFailedPhase, {
                              phase: failedStepText,
                            })}
                      </span>
                      {lastFailedAttempt.error ? (
                        <span className="text-red-600/70 dark:text-red-300/70">
                          {' · '}
                          {lastFailedAttempt.error}
                        </span>
                      ) : null}
                    </p>
                  </button>

                  <AnimatePresence initial={false}>
                    {failedDetailOpen && (
                      <motion.div
                        key="failed-body"
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: 'auto' }}
                        exit={{ opacity: 0, height: 0 }}
                        transition={{ duration: 0.22, ease: 'easeOut' }}
                        className="overflow-hidden"
                      >
                        <p className="mt-2 text-xs leading-relaxed whitespace-pre-wrap break-words text-red-600 dark:text-red-300">
                          {lastFailedAttempt.error}
                        </p>
                      </motion.div>
                    )}
                  </AnimatePresence>
                </div>
              </motion.div>
            ) : agentTrace?.length || successElapsedLabel ? (
              <motion.div
                key="status-trace"
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                transition={{ duration: 0.25, ease: 'easeOut' }}
                className="overflow-hidden"
              >
                <div className="border-b border-black/5 dark:border-white/5">
                  <button
                    onClick={() =>
                      agentTrace?.length
                        ? setTraceOpen((open) => !open)
                        : undefined
                    }
                    className="w-full flex items-center gap-2 px-4 py-2.5 text-left hover:bg-black/[0.02] dark:hover:bg-white/[0.03] transition-colors"
                    aria-expanded={agentTrace?.length ? traceOpen : false}
                    disabled={!agentTrace?.length}
                  >
                    <PlaygroundTraceIcon
                      className="w-3.5 h-3.5 shrink-0"
                      style={{ color: 'var(--color-primary)' }}
                    />
                    <span
                      className="text-xs font-semibold"
                      style={{ color: 'var(--text-primary)' }}
                    >
                      {agentTrace?.length
                        ? t.tapp.playgroundAgentTrace
                        : format(t.tapp.playgroundElapsedSuccess, {
                            time: successElapsedLabel,
                          })}
                    </span>
                    {validation?.passed && (
                      <span
                        className="flex items-center gap-1 rounded-full px-2 py-0.5 text-[9px] font-semibold"
                        style={{
                          color: 'var(--color-primary)',
                          background:
                            'color-mix(in srgb, var(--color-primary) 10%, transparent)',
                        }}
                      >
                        <FaCheck className="w-2 h-2" />
                        {t.tapp.playgroundValidated} · {validation.attempts}
                      </span>
                    )}
                    {successElapsedLabel && agentTrace?.length ? (
                      <span
                        className="text-[10px] font-mono tabular-nums"
                        style={{ color: 'var(--text-muted)' }}
                        title={format(t.tapp.playgroundElapsedSuccess, {
                          time: successElapsedLabel,
                        })}
                      >
                        {format(t.tapp.playgroundElapsedSuccess, {
                          time: successElapsedLabel,
                        })}
                      </span>
                    ) : null}
                    {agentTrace?.length ? (
                      <span
                        className="ml-auto text-[10px] font-mono"
                        style={{ color: 'var(--text-muted)' }}
                      >
                        {agentTrace.length}
                      </span>
                    ) : (
                      <span className="ml-auto" />
                    )}
                    {agentTrace?.length ? (
                      <motion.span
                        animate={{ rotate: traceOpen ? 180 : 0 }}
                        transition={{ duration: 0.2 }}
                        className="shrink-0"
                        style={{ color: 'var(--text-muted)' }}
                      >
                        <FaChevronDown className="w-2.5 h-2.5" />
                      </motion.span>
                    ) : null}
                  </button>

                  <AnimatePresence initial={false}>
                    {traceOpen && agentTrace?.length ? (
                      <motion.div
                        key="trace-body"
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: 'auto' }}
                        exit={{ opacity: 0, height: 0 }}
                        transition={{ duration: 0.22, ease: 'easeOut' }}
                        className="overflow-hidden"
                      >
                        <div className="px-4 pb-3 max-h-44 overflow-y-auto">
                          {/* 垂直时间线 */}
                          <div className="relative pl-3.5">
                            <span
                              className="absolute left-[3px] top-1.5 bottom-1.5 w-px"
                              style={{
                                background:
                                  'color-mix(in srgb, var(--color-primary) 25%, transparent)',
                              }}
                            />
                            {agentTrace.map((step, index) => (
                              <motion.div
                                key={`${step.tool}-${index}`}
                                initial={
                                  animationsEnabled
                                    ? { opacity: 0, x: -4 }
                                    : false
                                }
                                animate={{ opacity: 1, x: 0 }}
                                transition={{
                                  duration: 0.2,
                                  delay: animationsEnabled ? index * 0.03 : 0,
                                }}
                                className="relative py-1 text-[10px] leading-relaxed"
                              >
                                <span
                                  className={`absolute -left-3.5 top-[7px] w-[7px] h-[7px] rounded-full ring-2 ring-white dark:ring-[#1a1a1a] ${
                                    step.status === 'success'
                                      ? 'bg-emerald-500'
                                      : step.status === 'failed'
                                        ? 'bg-red-500'
                                        : 'bg-amber-500'
                                  }`}
                                />
                                <span
                                  className="font-mono font-bold"
                                  style={{ color: 'var(--color-primary)' }}
                                >
                                  {step.tool}
                                </span>
                                <span className="text-gray-500 dark:text-gray-400">
                                  {' '}
                                  {step.summary}
                                </span>
                              </motion.div>
                            ))}
                          </div>

                          {knowledgeSources?.length ? (
                            <div
                              className="mt-2 pt-2 border-t"
                              style={{
                                borderColor:
                                  'color-mix(in srgb, var(--color-primary) 10%, transparent)',
                              }}
                            >
                              <div className="text-[10px] font-semibold text-gray-500 dark:text-gray-400">
                                {t.tapp.playgroundKnowledgeSources}
                              </div>
                              <div className="mt-1.5 flex flex-wrap gap-1.5">
                                {knowledgeSources.slice(0, 10).map((source) => (
                                  <span
                                    key={`${source.document}-${source.section}`}
                                    title={source.section}
                                    className="max-w-full truncate rounded-lg bg-black/5 dark:bg-white/5 px-2 py-0.5 text-[9px] font-mono text-gray-600 dark:text-gray-300"
                                  >
                                    {source.document} · {source.section}
                                  </span>
                                ))}
                              </div>
                            </div>
                          ) : null}
                        </div>
                      </motion.div>
                    ) : null}
                  </AnimatePresence>
                </div>
              </motion.div>
            ) : null}
          </AnimatePresence>

          {/* ---------- 输入区 ---------- */}
          <div className="px-4 pt-3">
            <textarea
              ref={textareaRef}
              value={instruction}
              rows={2}
              onChange={(event) => onInstructionChange(event.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={
                hasProject
                  ? t.tapp.playgroundModifyPlaceholder
                  : t.tapp.playgroundCreatePlaceholder
              }
              title={t.tapp.playgroundShortcut}
              className="w-full resize-none text-sm leading-6 max-h-40 placeholder:text-gray-400 dark:placeholder:text-gray-500"
              style={{
                background: 'transparent',
                border: 'none',
                boxShadow: 'none',
                color: 'var(--text-primary)',
              }}
              maxLength={8000}
            />

            {/* Example prompt chips — create / empty instruction only */}
            {!hasProject &&
              !busy &&
              !instruction.trim() &&
              examplePrompts.length > 0 &&
              onPickExample && (
                <div className="pb-1.5">
                  <div
                    className="mb-1.5 text-[10px] font-semibold"
                    style={{ color: 'var(--text-muted)' }}
                  >
                    {t.tapp.playgroundExamplesLabel}
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {examplePrompts.map((example) => (
                      <button
                        key={example.id}
                        type="button"
                        onClick={() => onPickExample(example.prompt)}
                        title={example.prompt}
                        className="max-w-full truncate rounded-full px-2.5 py-1 text-[10px] font-medium transition-colors hover:bg-black/10 dark:hover:bg-white/15"
                        style={{
                          color: 'var(--text-secondary)',
                          background:
                            'color-mix(in srgb, var(--color-primary) 10%, transparent)',
                          border:
                            '1px solid color-mix(in srgb, var(--color-primary) 18%, transparent)',
                        }}
                      >
                        {example.label || exampleChipLabel(example.prompt)}
                      </button>
                    ))}
                  </div>
                </div>
              )}
          </div>

          {/* ---------- 工具栏 ---------- */}
          <div className="flex items-center gap-1.5 px-2.5 pb-2.5 pt-1">
            {revisionCount > 0 && (
              <div className="flex items-center rounded-full bg-black/5 dark:bg-white/10 p-0.5 shrink-0">
                <motion.button
                  onClick={() => onMoveRevision(-1)}
                  disabled={revisionIndex <= 0 || busy}
                  whileTap={animationsEnabled ? { scale: 0.9 } : {}}
                  className="w-7 h-7 rounded-full grid place-items-center text-gray-600 dark:text-gray-300 hover:bg-white hover:shadow-sm dark:hover:bg-white/15 transition-all disabled:opacity-30 disabled:pointer-events-none"
                  title={t.tapp.playgroundUndo}
                  aria-label={t.tapp.playgroundUndo}
                >
                  <FaUndo className="w-3 h-3" />
                </motion.button>
                <span
                  className="px-1 text-[10px] font-mono tabular-nums select-none"
                  style={{ color: 'var(--text-muted)' }}
                >
                  {revisionIndex + 1}/{revisionCount}
                </span>
                <motion.button
                  onClick={() => onMoveRevision(1)}
                  disabled={revisionIndex >= revisionCount - 1 || busy}
                  whileTap={animationsEnabled ? { scale: 0.9 } : {}}
                  className="w-7 h-7 rounded-full grid place-items-center text-gray-600 dark:text-gray-300 hover:bg-white hover:shadow-sm dark:hover:bg-white/15 transition-all disabled:opacity-30 disabled:pointer-events-none"
                  title={t.tapp.playgroundRedo}
                  aria-label={t.tapp.playgroundRedo}
                >
                  <FaRedo className="w-3 h-3" />
                </motion.button>
              </div>
            )}

            <motion.button
              type="button"
              onClick={() => setHistoryOpen((open) => !open)}
              whileTap={animationsEnabled ? { scale: 0.9 } : {}}
              className={`w-7 h-7 shrink-0 rounded-full grid place-items-center transition-all ${
                historyOpen
                  ? 'playground-history-toggle--active'
                  : 'text-gray-600 dark:text-gray-300 hover:bg-black/5 dark:hover:bg-white/10'
              }`}
              title={t.tapp.playgroundHistory}
              aria-label={t.tapp.playgroundHistory}
              aria-expanded={historyOpen}
            >
              <FaHistory className="w-3 h-3" />
            </motion.button>

            {(revisionCount > 0 || lastFailedAttempt) && (
              <button
                type="button"
                onClick={handleClear}
                disabled={busy}
                className="shrink-0 px-2.5 h-7 rounded-full text-[10px] font-semibold text-red-500/90 bg-red-500/10 hover:bg-red-500/15 hover:text-red-500 transition-colors disabled:opacity-30"
                title={t.tapp.playgroundClear}
              >
                {t.tapp.playgroundClear}
              </button>
            )}

            {/* 提示语：空闲时成本/耗时 + 快捷键；生成时耐心提示 */}
            <span
              className="hidden md:block flex-1 min-w-0 truncate text-right pr-1 text-[10px] text-gray-400 dark:text-gray-500"
              title={
                busy
                  ? t.tapp.playgroundBusyHint
                  : `${t.tapp.playgroundCostHint} · ${t.tapp.playgroundShortcut}`
              }
            >
              {busy ? t.tapp.playgroundBusyHint : t.tapp.playgroundCostHint}
            </span>
            <span className="md:hidden flex-1" />

            <AnimatePresence>
              {hasProject && onExport && (
                <motion.button
                  initial={
                    animationsEnabled ? { opacity: 0, scale: 0.8 } : false
                  }
                  animate={{ opacity: 1, scale: 1 }}
                  exit={
                    animationsEnabled ? { opacity: 0, scale: 0.8 } : undefined
                  }
                  transition={springTransition}
                  whileTap={animationsEnabled ? { scale: 0.92 } : {}}
                  onClick={onExport}
                  disabled={exporting || installing || busy}
                  className="h-8 shrink-0 rounded-full px-3 flex items-center gap-1.5 text-xs font-semibold transition-opacity disabled:opacity-40"
                  style={{
                    color: 'var(--text-primary)',
                    backgroundColor:
                      'color-mix(in srgb, var(--bg-card) 90%, transparent)',
                    border: '1px solid var(--border-color)',
                  }}
                  title={t.tapp.playgroundExport}
                  aria-label={t.tapp.playgroundExport}
                >
                  {exporting ? (
                    <span className="w-3 h-3 border-2 border-current/30 border-t-current rounded-full animate-spin" />
                  ) : (
                    <FaDownload className="w-3 h-3" />
                  )}
                  <span className="hidden sm:inline">
                    {t.tapp.playgroundExport}
                  </span>
                </motion.button>
              )}
            </AnimatePresence>

            <AnimatePresence>
              {hasProject && (
                <motion.button
                  initial={
                    animationsEnabled ? { opacity: 0, scale: 0.8 } : false
                  }
                  animate={{ opacity: 1, scale: 1 }}
                  exit={
                    animationsEnabled ? { opacity: 0, scale: 0.8 } : undefined
                  }
                  transition={springTransition}
                  whileTap={animationsEnabled ? { scale: 0.92 } : {}}
                  onClick={onInstall}
                  disabled={installing || exporting || busy}
                  className="h-8 shrink-0 rounded-full px-3 flex items-center gap-1.5 text-xs font-semibold text-white bg-gray-900 dark:bg-white dark:text-gray-900 shadow-sm disabled:opacity-40 transition-opacity"
                  title={t.tapp.playgroundInstall}
                  aria-label={t.tapp.playgroundInstall}
                >
                  {installing ? (
                    <span className="w-3 h-3 border-2 border-current/30 border-t-current rounded-full animate-spin" />
                  ) : (
                    <FaDownload className="w-3 h-3" />
                  )}
                  <span className="hidden sm:inline">
                    {t.tapp.playgroundInstall}
                  </span>
                </motion.button>
              )}
            </AnimatePresence>

            {busy && onCancel ? (
              <motion.button
                type="button"
                onClick={onCancel}
                whileTap={animationsEnabled ? { scale: 0.92 } : {}}
                className="h-8 shrink-0 rounded-full px-3 flex items-center gap-1.5 text-xs font-semibold text-white shadow-md transition-opacity"
                style={{
                  background:
                    'linear-gradient(135deg, #b45309, color-mix(in srgb, #b45309 80%, black))',
                }}
                title={t.tapp.playgroundCancel}
                aria-label={t.tapp.playgroundCancel}
              >
                <FaTimes className="w-3 h-3" />
                <span className="hidden sm:inline">{t.tapp.playgroundCancel}</span>
              </motion.button>
            ) : (
              <motion.button
                onClick={onSubmit}
                disabled={!instruction.trim() || busy}
                whileTap={
                  animationsEnabled && instruction.trim() && !busy
                    ? { scale: 0.92 }
                    : {}
                }
                className="h-8 shrink-0 rounded-full px-3 flex items-center gap-1.5 text-xs font-semibold text-white shadow-md disabled:opacity-40 transition-opacity"
                style={{
                  background:
                    'linear-gradient(135deg, var(--color-primary), color-mix(in srgb, var(--color-primary) 80%, black))',
                }}
                title={
                  hasProject
                    ? t.tapp.playgroundApplyChange
                    : t.tapp.playgroundGenerate
                }
                aria-label={
                  hasProject
                    ? t.tapp.playgroundApplyChange
                    : t.tapp.playgroundGenerate
                }
              >
                {busy ? (
                  <span className="w-3.5 h-3.5 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                ) : (
                  <FaArrowUp className="w-3 h-3" />
                )}
                <span className="hidden sm:inline">
                  {hasProject
                    ? t.tapp.playgroundApplyChange
                    : t.tapp.playgroundGenerate}
                </span>
              </motion.button>
            )}
          </div>
        </div>
      </div>
    </motion.div>
  )
}
