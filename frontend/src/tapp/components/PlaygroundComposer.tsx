/**
 * Tapp Playground 底部控制岛（Composer）
 *
 * 重构后的信息架构，自上而下：
 * - 通知层：错误/警告/成功卡片浮在岛上方，独立卡片、可关闭
 * - 状态带：生成中为紧凑两行（标题+计时 / 阶段字幕）+ 流光进度条；
 *   失败时同构两行 + Retry；完成后显示验证徽标与可展开的 Agent 轨迹
 * - 输入区：多行输入独占一行（composer 范式）
 * - 工具栏：左侧版本导航与会话操作，右侧安装与生成主操作
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
  FaRedo,
  FaTimes,
  FaUndo,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { useEffect, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { PlaygroundTraceIcon, TappPlaygroundIcon } from './PlaygroundIcons'

/** Persisted generate failure for status-band + Retry (sessionStorage) */
export interface PlaygroundLastFailedAttempt {
  instruction: string
  error: string
  elapsedMs: number
  finishedAt: number
  origin: 'user' | 'runtime-repair'
  /** Frozen busy-phase index (plan/retrieve/code/validate) from elapsed time */
  phaseIndex?: number
}

export interface PlaygroundComposerProps {
  /** 桌面浮动布局（absolute）或移动端固定布局（fixed） */
  interactive: boolean
  busy: boolean
  busyMode: 'user' | 'runtime-repair'
  installing: boolean
  hasProject: boolean
  instruction: string
  revisionIndex: number
  revisionCount: number
  error: string
  previewError: string
  notice: string
  warnings: string[]
  agentTrace?: PlaygroundAgentStep[]
  knowledgeSources?: PlaygroundKnowledgeSource[]
  validation?: PlaygroundValidationReport
  /** Persisted last failed generate attempt (sessionStorage via parent) */
  lastFailedAttempt?: PlaygroundLastFailedAttempt | null
  onInstructionChange: (value: string) => void
  onSubmit: () => void
  onInstall: () => void
  onMoveRevision: (delta: number) => void
  onClear: () => void
  onDismissError: () => void
  onDismissPreviewError: () => void
  onDismissNotice: () => void
  onRetryFailed?: () => void
  onDismissFailed?: () => void
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
          className={`min-w-0 flex-1 break-words text-xs leading-relaxed ${textClass}`}
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

export function PlaygroundComposer({
  interactive,
  busy,
  busyMode,
  installing,
  hasProject,
  instruction,
  revisionIndex,
  revisionCount,
  error,
  previewError,
  notice,
  warnings,
  agentTrace,
  knowledgeSources,
  validation,
  lastFailedAttempt,
  onInstructionChange,
  onSubmit,
  onInstall,
  onMoveRevision,
  onClear,
  onDismissError,
  onDismissPreviewError,
  onDismissNotice,
  onRetryFailed,
  onDismissFailed,
}: PlaygroundComposerProps) {
  const { t, format } = useI18n()
  const animConfig = useAnimationLevel()
  const animationsEnabled = animConfig.level !== 'none'
  const springTransition = animConfig.spring
    ? ({ type: 'spring', stiffness: 400, damping: 30 } as const)
    : ({ type: 'tween', duration: 0.25 * animConfig.durationScale } as const)

  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const [traceOpen, setTraceOpen] = useState(false)
  const [failedDetailOpen, setFailedDetailOpen] = useState(true)

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

  // 生成阶段（按典型耗时估算推进；最后一个阶段开放式等待响应）
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
  const phaseIndex =
    busyElapsed < 5 ? 0 : busyElapsed < 14 ? 1 : busyElapsed < 90 ? 2 : 3
  const formatElapsed = (totalSeconds: number) =>
    `${Math.floor(totalSeconds / 60)}:${String(totalSeconds % 60).padStart(2, '0')}`
  const elapsedLabel = formatElapsed(busyElapsed)

  const failedElapsedLabel = lastFailedAttempt
    ? formatElapsed(Math.max(0, Math.floor(lastFailedAttempt.elapsedMs / 1000)))
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

  // Collapse open detail when a new failure arrives so users see the error
  useEffect(() => {
    if (lastFailedAttempt) setFailedDetailOpen(true)
  }, [lastFailedAttempt?.finishedAt])

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

        <div className="relative rounded-[1.6rem] bg-white/90 dark:bg-[#1a1a1a]/90 backdrop-blur-xl shadow-2xl ring-1 ring-black/5 dark:ring-white/10 overflow-hidden">
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
                  {/* Line 1: spinner + title · elapsed */}
                  <div className="flex items-center gap-2">
                    <span
                      className="w-3.5 h-3.5 border-2 rounded-full animate-spin shrink-0"
                      style={{
                        borderColor:
                          'color-mix(in srgb, var(--color-primary) 25%, transparent)',
                        borderTopColor: 'var(--color-primary)',
                      }}
                    />
                    <motion.span
                      className="text-xs font-semibold truncate"
                      style={{ color: 'var(--text-primary)' }}
                      animate={
                        animationsEnabled
                          ? { opacity: [0.85, 1, 0.85] }
                          : undefined
                      }
                      transition={
                        animationsEnabled
                          ? {
                              duration: 2.4,
                              repeat: Infinity,
                              ease: 'easeInOut',
                            }
                          : undefined
                      }
                    >
                      {busyMode === 'runtime-repair'
                        ? t.tapp.playgroundRepairingRuntime
                        : t.tapp.playgroundGenerating}
                    </motion.span>
                    <span
                      className="ml-auto text-[10px] font-mono tabular-nums shrink-0"
                      style={{ color: 'var(--text-muted)' }}
                    >
                      {elapsedLabel}
                    </span>
                  </div>

                  {/* Flowing progress track (not a text line) */}
                  <div className="mt-2 playground-progress-track">
                    {animationsEnabled && (
                      <div className="playground-progress-bar" />
                    )}
                  </div>

                  {/* Line 2: cross-fading phase subtitle only */}
                  <div className="mt-1.5 relative min-h-[1.25rem] overflow-hidden">
                    <AnimatePresence mode="wait" initial={false}>
                      <motion.p
                        key={`phase-desc-${phaseIndex}`}
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
                        {phases[phaseIndex].desc}
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
                        {format(t.tapp.playgroundFailedPhase, {
                          phase: phases[failedPhaseIndex]?.label || '',
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
            ) : agentTrace?.length ? (
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
                    onClick={() => setTraceOpen((open) => !open)}
                    className="w-full flex items-center gap-2 px-4 py-2.5 text-left hover:bg-black/[0.02] dark:hover:bg-white/[0.03] transition-colors"
                    aria-expanded={traceOpen}
                  >
                    <PlaygroundTraceIcon
                      className="w-3.5 h-3.5 shrink-0"
                      style={{ color: 'var(--color-primary)' }}
                    />
                    <span
                      className="text-xs font-semibold"
                      style={{ color: 'var(--text-primary)' }}
                    >
                      {t.tapp.playgroundAgentTrace}
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
                    <span
                      className="ml-auto text-[10px] font-mono"
                      style={{ color: 'var(--text-muted)' }}
                    >
                      {agentTrace.length}
                    </span>
                    <motion.span
                      animate={{ rotate: traceOpen ? 180 : 0 }}
                      transition={{ duration: 0.2 }}
                      className="shrink-0"
                      style={{ color: 'var(--text-muted)' }}
                    >
                      <FaChevronDown className="w-2.5 h-2.5" />
                    </motion.span>
                  </button>

                  <AnimatePresence initial={false}>
                    {traceOpen && (
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
                    )}
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
          </div>

          {/* ---------- 工具栏 ---------- */}
          <div className="flex items-center gap-1.5 px-2.5 pb-2.5 pt-1">
            <div
              className="w-8 h-8 shrink-0 rounded-full grid place-items-center"
              style={{
                color: 'var(--color-primary)',
                background:
                  'color-mix(in srgb, var(--color-primary) 12%, transparent)',
              }}
            >
              <TappPlaygroundIcon className="w-4 h-4" />
            </div>

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

            {revisionCount > 0 && (
              <button
                onClick={onClear}
                disabled={busy}
                className="shrink-0 px-2 h-7 rounded-full text-[10px] font-semibold text-red-500/80 hover:text-red-500 hover:bg-red-500/8 transition-colors disabled:opacity-30"
              >
                {t.tapp.playgroundClear}
              </button>
            )}

            {/* 提示语：空闲时快捷键说明，生成时耐心提示 */}
            <span
              className="hidden md:block flex-1 min-w-0 truncate text-right pr-1 text-[10px] text-gray-400 dark:text-gray-500"
            >
              {busy ? t.tapp.playgroundBusyHint : t.tapp.playgroundShortcut}
            </span>
            <span className="md:hidden flex-1" />

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
                  disabled={installing || busy}
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
          </div>
        </div>
      </div>
    </motion.div>
  )
}
