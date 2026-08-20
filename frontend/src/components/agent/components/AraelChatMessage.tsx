/**
 * AraelChatMessage - 现代聊天消息组件
 *
 * 根据 role 渲染不同样式：
 * - User: 右对齐气泡，主题色
 * - Assistant: 左对齐，底部紧凑进度提示
 * - System: 居中分隔线
 */

import type { ChatMessage, ExecutionStep } from '../types'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { API_URL } from '../../../config'
import { useI18n } from '../../../contexts/I18nContext'

function resolveAgentImageUrl(url: string): string {
  return url.startsWith('/') ? `${API_URL}${url}` : url
}

// 轻量 Markdown → React 渲染

// 匹配顺序: 行内代码 > 图片 > 链接 > 加粗 > 斜体
const INLINE_RE =
  /(`[^`]+`)|!\[([^\]]*)\]\(([^)]+)\)|\[([^\]]+)\]\(([^)]+)\)|\*\*(.+?)\*\*|\*([^*]+)\*/g
const HEADING_RE = /^(#{1,3})\s+(.+)/
const UL_RE = /^\s*[-*+]\s+/
const OL_RE = /^\s*\d+\.\s+/
const BLOCKQUOTE_RE = /^>\s?(.*)/
const HR_RE = /^(?:-{3,}|_{3,}|\*{3,})\s*$/
const TABLE_SEP_RE = /^\|[\s:|-]+\|$/

/** 解析行内元素：图片、链接、加粗、斜体、行内代码 */
function parseInline(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = []
  const inlineRe = new RegExp(INLINE_RE.source, INLINE_RE.flags)
  let lastIndex = 0
  let key = 0

  for (
    let match = inlineRe.exec(text);
    match !== null;
    match = inlineRe.exec(text)
  ) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index))
    }
    if (match[1]) {
      // 行内代码
      nodes.push(
        <code key={key++} className="arael-md-code">
          {match[1].slice(1, -1)}
        </code>,
      )
    } else if (match[3]) {
      // 图片 ![alt](url)
      nodes.push(
        <img
          key={key++}
          className="arael-md-img"
          src={match[3]}
          alt={match[2] || ''}
          loading="lazy"
        />,
      )
    } else if (match[4] && match[5]) {
      // 链接 [text](url)
      nodes.push(
        <a
          key={key++}
          className="arael-md-link"
          href={match[5]}
          target="_blank"
          rel="noopener noreferrer"
        >
          {match[4]}
        </a>,
      )
    } else if (match[6]) {
      // **加粗**
      nodes.push(<strong key={key++}>{match[6]}</strong>)
    } else if (match[7]) {
      // *斜体*
      nodes.push(<em key={key++}>{match[7]}</em>)
    }
    lastIndex = match.index + match[0].length
  }
  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex))
  }
  return nodes
}

/** 渲染消息内容（支持基础 Markdown） */
function renderMessageContent(message: string): React.ReactNode {
  const lines = message.split('\n')
  const elements: React.ReactNode[] = []
  let key = 0
  let i = 0

  while (i < lines.length) {
    const line = lines[i]

    // 代码块 ```
    if (line.trimStart().startsWith('```')) {
      const lang = line.trimStart().slice(3).trim()
      const codeLines: string[] = []
      i++
      while (i < lines.length && !lines[i].trimStart().startsWith('```')) {
        codeLines.push(lines[i])
        i++
      }
      i++ // skip closing ```
      elements.push(
        <pre key={key++} className="arael-md-pre" data-lang={lang || undefined}>
          <code>{codeLines.join('\n')}</code>
        </pre>,
      )
      continue
    }

    // 标题 # ## ###
    const headingMatch = line.match(HEADING_RE)
    if (headingMatch) {
      const level = headingMatch[1].length as 1 | 2 | 3
      const Tag = `h${level + 2}` as 'h3' | 'h4' | 'h5' // # → h3, ## → h4, ### → h5
      elements.push(
        <Tag key={key++} className="arael-md-heading">
          {parseInline(headingMatch[2])}
        </Tag>,
      )
      i++
      continue
    }

    // 水平线 --- / ___ / ***
    if (HR_RE.test(line)) {
      elements.push(<hr key={key++} className="arael-md-hr" />)
      i++
      continue
    }

    // 引用块 > text
    if (BLOCKQUOTE_RE.test(line)) {
      const quoteLines: React.ReactNode[] = []
      while (i < lines.length) {
        const qm = lines[i].match(BLOCKQUOTE_RE)
        if (!qm) break
        quoteLines.push(
          <p key={key++} className="arael-md-p">
            {parseInline(qm[1])}
          </p>,
        )
        i++
      }
      elements.push(
        <blockquote key={key++} className="arael-md-blockquote">
          {quoteLines}
        </blockquote>,
      )
      continue
    }

    // 表格 | col | col |
    if (
      line.includes('|') &&
      line.trim().startsWith('|') &&
      i + 1 < lines.length &&
      TABLE_SEP_RE.test(lines[i + 1].trim())
    ) {
      const parseRow = (row: string) =>
        row
          .split('|')
          .slice(1, -1)
          .map((c) => c.trim())
      const headers = parseRow(line)
      i += 2 // skip header + separator
      const rows: string[][] = []
      while (
        i < lines.length &&
        lines[i].trim().startsWith('|') &&
        lines[i].trim().endsWith('|')
      ) {
        rows.push(parseRow(lines[i]))
        i++
      }
      elements.push(
        <table key={key++} className="arael-md-table">
          <thead>
            <tr>
              {headers.map((h, ci) => (
                <th key={ci}>{parseInline(h)}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, ri) => (
              <tr key={ri}>
                {row.map((cell, ci) => (
                  <td key={ci}>{parseInline(cell)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>,
      )
      continue
    }

    // 无序列表 - item
    if (UL_RE.test(line)) {
      const items: React.ReactNode[] = []
      while (i < lines.length && UL_RE.test(lines[i])) {
        items.push(
          <li key={key++}>{parseInline(lines[i].replace(UL_RE, ''))}</li>,
        )
        i++
      }
      elements.push(
        <ul key={key++} className="arael-md-list">
          {items}
        </ul>,
      )
      continue
    }

    // 有序列表 1. item
    if (OL_RE.test(line)) {
      const items: React.ReactNode[] = []
      while (i < lines.length && OL_RE.test(lines[i])) {
        items.push(
          <li key={key++}>{parseInline(lines[i].replace(OL_RE, ''))}</li>,
        )
        i++
      }
      elements.push(
        <ol key={key++} className="arael-md-list">
          {items}
        </ol>,
      )
      continue
    }

    // 空行
    if (!line.trim()) {
      i++
      continue
    }

    // 普通段落
    elements.push(
      <p key={key++} className="arael-md-p">
        {parseInline(line)}
      </p>,
    )
    i++
  }

  return <>{elements}</>
}

/** 格式化时间 */
function formatTime(date: Date, locale?: string): string {
  return date.toLocaleTimeString(locale || undefined, {
    hour: '2-digit',
    minute: '2-digit',
  })
}

/** 格式化耗时 */
function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

function remainingConfirmSeconds(
  pendingQuestion: NonNullable<ChatMessage['pendingQuestion']>,
): number | null {
  const ttl = pendingQuestion.expiresInSeconds
  const received = pendingQuestion.receivedAtMs
  if (
    typeof ttl !== 'number' ||
    ttl <= 0 ||
    typeof received !== 'number' ||
    !pendingQuestion.confirmationId
  ) {
    return null
  }
  return Math.max(0, ttl - Math.floor((Date.now() - received) / 1000))
}

function riskLevelClass(level?: string): string {
  const n = (level || '').toLowerCase()
  if (n.includes('critical')) return 'arael-risk-critical'
  if (n.includes('high')) return 'arael-risk-high'
  if (n.includes('medium')) return 'arael-risk-medium'
  if (n.includes('low')) return 'arael-risk-low'
  return 'arael-risk-unknown'
}

/** 问题输入组件 — 选项按钮 + 自由文本输入；敏感确认展示风险与倒计时 */
const QuestionInput: React.FC<{
  messageId: string
  pendingQuestion: NonNullable<ChatMessage['pendingQuestion']>
  selectedAnswer?: string
  onAnswer: (messageId: string, answer: string) => void
}> = ({ messageId, pendingQuestion, selectedAnswer, onAnswer }) => {
  const { t, format } = useI18n()
  const hasOptions =
    pendingQuestion.options && pendingQuestion.options.length > 0
  const isLocked = !!selectedAnswer
  const [remaining, setRemaining] = React.useState<number | null>(() =>
    remainingConfirmSeconds(pendingQuestion),
  )

  React.useEffect(() => {
    const initial = remainingConfirmSeconds(pendingQuestion)
    setRemaining(initial)
    if (initial === null) return
    const id = window.setInterval(() => {
      setRemaining(remainingConfirmSeconds(pendingQuestion))
    }, 1000)
    return () => window.clearInterval(id)
  }, [
    pendingQuestion.confirmationId,
    pendingQuestion.expiresInSeconds,
    pendingQuestion.receivedAtMs,
  ])

  const isExpired = remaining === 0
  const showConfirmMeta =
    !!pendingQuestion.confirmationId &&
    (!!pendingQuestion.riskLevel || remaining !== null)

  return (
    <div className="arael-question-input-area">
      <div className="arael-question-text">{pendingQuestion.question}</div>
      {showConfirmMeta && (
        <div className="arael-confirm-meta">
          {pendingQuestion.riskLevel && (
            <span
              className={`arael-risk-badge ${riskLevelClass(pendingQuestion.riskLevel)}`}
            >
              {format(t.arael.confirmRisk, {
                level: pendingQuestion.riskLevel,
              })}
            </span>
          )}
          {remaining !== null && (
            <span
              className={`arael-confirm-ttl${isExpired ? ' arael-confirm-ttl-expired' : ''}`}
            >
              {isExpired
                ? t.arael.confirmExpired
                : format(t.arael.confirmExpiresIn, {
                    seconds: String(remaining),
                  })}
            </span>
          )}
        </div>
      )}
      {isExpired && (
        <div className="arael-confirm-expired-hint">
          {t.arael.confirmExpiredHint}
        </div>
      )}
      {pendingQuestion.context && (
        <div className="arael-question-context">{pendingQuestion.context}</div>
      )}
      {hasOptions && (
        <div className="arael-question-options">
          {pendingQuestion.options!.map((option, idx) => {
            const isSelected = selectedAnswer === option.value
            // 已选中时只显示被选中的那个
            if (isLocked && !isSelected) return null
            const isConfirm = option.value === 'confirm'
            const disabled =
              isLocked || (isExpired && isConfirm && !isSelected)
            return (
              <button
                key={idx}
                className={`arael-option-btn${isSelected ? ' arael-option-btn-selected' : ''}${disabled && !isSelected ? ' arael-option-btn-disabled' : ''}`}
                onClick={() => !disabled && onAnswer(messageId, option.value)}
                title={
                  disabled && isConfirm && isExpired
                    ? t.arael.confirmExpired
                    : option.description
                }
                disabled={disabled}
              >
                {isSelected && (
                  <svg
                    className="arael-option-check"
                    width="14"
                    height="14"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.5"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <polyline points="20 6 9 17 4 12" />
                  </svg>
                )}
                {option.label}
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}

export interface AraelChatMessageProps {
  message: ChatMessage
  onRetry?: (content: string) => void
  onAnswerQuestion?: (messageId: string, answer: string) => void
  onSuggestionClick?: (text: string) => void
}

export const AraelChatMessage: React.FC<AraelChatMessageProps> = React.memo(
  ({ message, onRetry, onAnswerQuestion, onSuggestionClick }) => {
    const { t, locale } = useI18n()

    // User
    if (message.role === 'user') {
      return (
        <div className="arael-msg arael-msg-user">
          <div className="arael-msg-bubble">
            <div className="arael-msg-content">{message.content}</div>
            <div className="arael-msg-time">
              {formatTime(message.createdAt, locale)}
            </div>
          </div>
        </div>
      )
    }

    // System
    if (message.role === 'system') {
      return (
        <div className="arael-msg arael-msg-system">
          <div className="arael-msg-system-line" />
          <span className="arael-msg-system-text">{message.content}</span>
          <div className="arael-msg-system-line" />
        </div>
      )
    }

    // Assistant
    const exec = message.taskExecution
    const isProcessing =
      exec?.status === 'processing' || exec?.status === 'cancelling'
    const isCompleted = exec?.status === 'completed'
    const isError = exec?.status === 'error'

    // 计算总耗时
    const totalDurationMs = exec
      ? (exec.executionTrace?.totalDurationMs ??
        exec.steps.reduce((sum, s) => sum + (s.durationMs ?? 0), 0))
      : 0

    const canRetry = (isCompleted || isError) && onRetry
    const hasContent = !!message.content

    // 复制状态
    const [copied, setCopied] = useState(false)
    const copyTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
    const handleCopy = useCallback(() => {
      if (!message.content) return
      navigator.clipboard
        .writeText(message.content)
        .then(() => {
          setCopied(true)
          if (copyTimeoutRef.current) clearTimeout(copyTimeoutRef.current)
          copyTimeoutRef.current = setTimeout(setCopied, 1500, false)
        })
        .catch(() => {})
    }, [message.content])
    useEffect(
      () => () => {
        if (copyTimeoutRef.current) clearTimeout(copyTimeoutRef.current)
      },
      [],
    )

    // Trace 数据（完成后从 executionTrace 获取层级信息）
    const trace = exec?.executionTrace
    const tierUsageMap = trace?.tierUsage
    const proCount = tierUsageMap?.pro ?? tierUsageMap?.Pro ?? 0
    const stdCount = tierUsageMap?.standard ?? tierUsageMap?.Standard ?? 0

    // 根据 trace 步骤数据丰富 liveStep 的 tier 信息
    const getStepTier = (step: ExecutionStep): string | undefined => {
      if (step.tierUsed) return step.tierUsed
      const traceStep = trace?.steps?.find((ts) => ts.stepId === step.id)
      return traceStep?.tierUsed
    }

    // 执行中的步骤列表
    const liveSteps = exec?.steps ?? []
    const planDescs = exec?.planStepDescriptions ?? []
    const runningStep = liveSteps.findLast((s) => s.status === 'running')
    const finishedSteps = liveSteps.filter(
      (s) => s.status === 'completed' || s.status === 'error',
    )

    // 思考面板展开状态
    const [thinkingExpanded, setThinkingExpanded] = useState(false)

    // 实时计时器（处理中时每秒刷新）
    const [elapsed, setElapsed] = useState(0)
    const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)
    useEffect(() => {
      if (isProcessing) {
        const start = message.createdAt.getTime()
        setElapsed(Math.floor((Date.now() - start) / 1000))
        timerRef.current = setInterval(() => {
          setElapsed(Math.floor((Date.now() - start) / 1000))
        }, 1000)
        return () => {
          if (timerRef.current) clearInterval(timerRef.current)
        }
      } else {
        if (timerRef.current) clearInterval(timerRef.current)
        timerRef.current = null
      }
    }, [isProcessing, message.createdAt])

    const formatElapsed = (s: number) => {
      if (s < 60) return `${s}s`
      return `${Math.floor(s / 60)}m${s % 60}s`
    }

    // 生成步骤显示文本："第X步 描述"
    const formatStepLabel = (step: ExecutionStep) => {
      const idx = step.stepIndex != null ? step.stepIndex + 1 : null
      const total = step.totalSteps ?? null
      if (idx && total && total > 1) {
        return t.arael.stepLabel
          .replace('{idx}', String(idx))
          .replace('{name}', step.name)
      }
      return step.name
    }

    // 当前显示的思考标签文本
    const thinkingLabel = (() => {
      if (isProcessing) {
        if (runningStep) return formatStepLabel(runningStep)
        if (planDescs.length > 0 && liveSteps.length === 0) return planDescs[0]
        return null
      }
      if (isCompleted || isError) {
        const lastStep = liveSteps.at(-1)
        if (lastStep) return formatStepLabel(lastStep)
        return null
      }
      return null
    })()

    // 是否显示思考头部（处理中/完成/错误 且有执行上下文）
    const showThinkingHeader =
      !!exec && (isProcessing || isCompleted || isError)

    return (
      <div className="arael-msg arael-msg-assistant">
        <div className="arael-msg-body">
          <div className="arael-msg-bubble">
            {/* ====== 思考指示器（气泡左上角） ====== */}
            {showThinkingHeader && (
              <div
                className="arael-thinking-header"
                onClick={() => setThinkingExpanded(!thinkingExpanded)}
              >
                <span
                  className={`arael-thinking-header-text ${isProcessing ? 'arael-current-step-text' : ''} ${isCompleted ? 'arael-thinking-header-text-done' : ''} ${isError ? 'arael-thinking-header-text-error' : ''}`}
                >
                  {isProcessing
                    ? thinkingLabel || t.arael.thinking
                    : isCompleted
                      ? `${t.arael.completed}${totalDurationMs > 0 ? ` · ${t.arael.totalTime.replace('{time}', formatDuration(totalDurationMs))}` : ''}`
                      : t.arael.errorOccurred}
                </span>
                <span className="arael-thinking-header-arrow">
                  {thinkingExpanded ? '▾' : '▸'}
                </span>
              </div>
            )}

            {/* ====== 展开的思考详情面板 ====== */}
            {showThinkingHeader && thinkingExpanded && (
              <div className="arael-thinking-panel">
                {/* 已消耗时间 */}
                <div className="arael-thinking-panel-timer">
                  <svg
                    width="11"
                    height="11"
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
                  <span>
                    {isProcessing
                      ? t.arael.elapsedTime.replace(
                          '{time}',
                          formatElapsed(elapsed),
                        )
                      : totalDurationMs > 0
                        ? t.arael.totalTime.replace(
                            '{time}',
                            formatDuration(totalDurationMs),
                          )
                        : t.arael.completed}
                  </span>
                </div>

                {/* 步骤列表 */}
                {(finishedSteps.length > 0 || runningStep) && (
                  <div className="arael-thinking-panel-steps">
                    {finishedSteps.map((step) => {
                      const tier = getStepTier(step)
                      return (
                        <div
                          key={step.id}
                          className={`arael-thinking-panel-step arael-thinking-panel-step-${step.status}`}
                        >
                          <span className="arael-thinking-panel-step-icon">
                            {step.status === 'completed' ? '✓' : '✗'}
                          </span>
                          <span className="arael-thinking-panel-step-name">
                            {formatStepLabel(step)}
                          </span>
                          {tier && (
                            <span
                              className={`arael-tier-badge arael-tier-${tier}`}
                            >
                              {tier === 'pro' ? 'Pro' : 'Std'}
                            </span>
                          )}
                          {step.durationMs != null && step.durationMs > 0 && (
                            <span className="arael-thinking-panel-step-dur">
                              {formatDuration(step.durationMs)}
                            </span>
                          )}
                          {step.degraded && (
                            <span className="arael-thinking-panel-step-note">
                              {t.arael.autoDegraded}
                            </span>
                          )}
                          {step.retryAttempt != null &&
                            step.retryAttempt > 0 && (
                              <span className="arael-thinking-panel-step-note">
                                {t.arael.retryCount.replace(
                                  '{n}',
                                  String(step.retryAttempt),
                                )}
                              </span>
                            )}
                        </div>
                      )
                    })}
                    {runningStep && (
                      <div className="arael-thinking-panel-step arael-thinking-panel-step-running">
                        <span className="arael-thinking-panel-step-name arael-current-step-text">
                          {formatStepLabel(runningStep)}
                        </span>
                      </div>
                    )}
                  </div>
                )}

                {/* 计划步骤（尚未开始执行） */}
                {planDescs.length > 0 && liveSteps.length === 0 && (
                  <div className="arael-thinking-panel-steps">
                    {planDescs.map((desc, idx) => (
                      <div
                        key={idx}
                        className={`arael-thinking-panel-step ${idx === 0 ? 'arael-thinking-panel-step-running' : 'arael-thinking-panel-step-pending'}`}
                      >
                        <span
                          className={`arael-thinking-panel-step-name ${idx === 0 ? 'arael-current-step-text' : ''}`}
                        >
                          {planDescs.length > 1
                            ? t.arael.stepLabel
                                .replace('{idx}', String(idx + 1))
                                .replace('{name}', desc)
                            : desc}
                        </span>
                      </div>
                    ))}
                  </div>
                )}

                {/* 底部附加信息：层级统计 / 技能 / 记忆 */}
                {(isCompleted || isError) && (
                  <div className="arael-thinking-panel-footer">
                    {(proCount > 0 || stdCount > 0 || exec?.skillName) && (
                      <div className="arael-thinking-panel-meta">
                        {proCount > 0 && (
                          <span className="arael-tier-pro">
                            Pro x{proCount}
                          </span>
                        )}
                        {stdCount > 0 && (
                          <span className="arael-tier-std">
                            Std x{stdCount}
                          </span>
                        )}
                        {exec?.skillName && (
                          <span className="arael-thinking-panel-skill">
                            {exec.skillName}
                          </span>
                        )}
                      </div>
                    )}
                    {exec?.recalledMemories &&
                      exec.recalledMemories.length > 0 && (
                        <div className="arael-thinking-panel-memories">
                          {exec.recalledMemories.map((mem, i) => (
                            <div
                              key={i}
                              className="arael-thinking-panel-memory-item"
                            >
                              {mem}
                            </div>
                          ))}
                        </div>
                      )}
                  </div>
                )}
              </div>
            )}

            {/* 消息文本 — 流式回复或最终回复 */}
            {hasContent && (
              <div className="arael-msg-content">
                {renderMessageContent(message.content)}
              </div>
            )}

            {/* 图片结果（支持多张） */}
            {message.imageUrls && message.imageUrls.length > 0 && (
              <div
                className={`arael-msg-image${message.imageUrls.length > 1 ? ' arael-msg-image-grid' : ''}`}
              >
                {message.imageUrls.map((url, idx) => (
                  <img
                    key={idx}
                    src={resolveAgentImageUrl(url)}
                    alt={t.arael.aiGeneratedImage}
                    loading="lazy"
                    className="arael-msg-image-clickable"
                    onClick={() =>
                      window.open(
                        resolveAgentImageUrl(url),
                        '_blank',
                        'noopener,noreferrer',
                      )
                    }
                    onError={(e) => {
                      const el = e.currentTarget
                      el.style.opacity = '0.3'
                      el.style.minHeight = '60px'
                      el.alt = t.arael.imageLoadFailed
                    }}
                  />
                ))}
              </div>
            )}

            {/* 等待用户输入/选择 */}
            {message.pendingQuestion && onAnswerQuestion && (
              <QuestionInput
                messageId={message.id}
                pendingQuestion={message.pendingQuestion}
                selectedAnswer={message.selectedAnswer}
                onAnswer={onAnswerQuestion}
              />
            )}

            {/* 建议选项（Planner 澄清 / 推荐操作） */}
            {!message.pendingQuestion &&
              message.suggestions &&
              message.suggestions.length > 0 &&
              onSuggestionClick && (
                <div className="arael-question-input-area">
                  <div className="arael-question-options">
                    {message.suggestions.map((suggestion, idx) => (
                      <button
                        key={idx}
                        className="arael-option-btn"
                        onClick={() => onSuggestionClick(suggestion)}
                      >
                        {suggestion}
                      </button>
                    ))}
                  </div>
                </div>
              )}

            {/* 底部：复制 + 重试 */}
            {(canRetry || hasContent) && (isCompleted || isError) && (
              <div className="arael-msg-footer">
                <div className="arael-msg-footer-right">
                  {hasContent && (
                    <button
                      className={`arael-copy-btn${copied ? ' arael-copy-btn-done' : ''}`}
                      onClick={handleCopy}
                      title={copied ? t.arael.copied : t.arael.copyMessage}
                    >
                      {copied ? (
                        <svg
                          width="12"
                          height="12"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="2.5"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        >
                          <polyline points="20 6 9 17 4 12" />
                        </svg>
                      ) : (
                        <svg
                          width="12"
                          height="12"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="2.5"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        >
                          <rect
                            x="9"
                            y="9"
                            width="13"
                            height="13"
                            rx="2"
                            ry="2"
                          />
                          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                        </svg>
                      )}
                    </button>
                  )}
                  {canRetry && (
                    <button
                      className="arael-retry-btn"
                      onClick={() => onRetry(message.content)}
                      title={t.arael.retryRequest}
                    >
                      <svg
                        width="12"
                        height="12"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.5"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <polyline points="23 4 23 10 17 10" />
                        <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
                      </svg>
                    </button>
                  )}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    )
  },
)

export default AraelChatMessage
