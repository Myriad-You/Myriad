import type { AgentMessage } from './agentMessages'
import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { AgentMarkdown } from './AgentMarkdown'
import { useAgentPanelMode } from './agentPanelMode'
import { AgentPanelThinking } from './AgentPanelThinking'
import {
  BUBBLE_GROW_TAU,
  BUBBLE_SHRINK_TAU,
  messageHasAnswer,
  messageShowsThinking,
  THINKING_FOLD_MS,
} from './agentThinking'
import { invalidateComposerFavorites } from './composerFavorites'

export interface AgentPanelMessageProps {
  message: AgentMessage
  onRetry?: () => void
  onAnswer: (messageId: string, answer: string) => void
  onSuggest: (text: string) => void
  onWorkOffer?: (input: string) => void
  onZoomImage: (url: string) => void
}

function formatTime(at: number, locale?: string): string {
  return new Date(at).toLocaleTimeString(locale || undefined, {
    hour: '2-digit',
    minute: '2-digit',
  })
}

function motionAllowed(): boolean {
  if (typeof document === 'undefined') return false
  if (document.documentElement.dataset.perfMode === 'exlight') return false
  return !window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

const BUBBLE_SETTLE_PX = 1.5

function useHeldOpen(open: boolean, holdMs: number): boolean {
  const [held, setHeld] = useState(open)
  useEffect(() => {
    if (open) {
      setHeld(true)
      return undefined
    }
    const wait = motionAllowed() ? holdMs : 0
    const timer = setTimeout(setHeld, wait, false)
    return () => clearTimeout(timer)
  }, [holdMs, open])
  return open || held
}

/** Follow inner content height, never the written outer box. */
function useBubbleHeight(open: boolean): {
  ref: React.RefObject<HTMLDivElement | null>
  growRef: React.RefObject<HTMLDivElement | null>
} {
  const ref = useRef<HTMLDivElement>(null)
  const growRef = useRef<HTMLDivElement>(null)
  const currentH = useRef(0)
  const targetH = useRef(0)
  const settledH = useRef(0)
  const chromeH = useRef(0)
  const raf = useRef(0)
  const lastTs = useRef(0)
  const primed = useRef(false)
  const running = useRef(false)
  const followRef = useRef<() => void>(() => {})

  useLayoutEffect(() => {
    const el = ref.current
    const grow = growRef.current
    if (!open || !el || !grow) {
      followRef.current = () => {}
      if (raf.current) cancelAnimationFrame(raf.current)
      raf.current = 0
      running.current = false
      primed.current = false
      if (el) {
        el.style.height = ''
        el.style.overflow = ''
      }
      return
    }

    if (!motionAllowed()) {
      followRef.current = () => {}
      primed.current = true
      el.style.height = ''
      el.style.overflow = ''
      return
    }

    const css = getComputedStyle(el)
    chromeH.current =
      (Number.parseFloat(css.paddingTop) || 0) +
      (Number.parseFloat(css.paddingBottom) || 0) +
      (Number.parseFloat(css.borderTopWidth) || 0) +
      (Number.parseFloat(css.borderBottomWidth) || 0)

    const dest = () => grow.offsetHeight + chromeH.current

    const rest = () => {
      if (raf.current) cancelAnimationFrame(raf.current)
      raf.current = 0
      lastTs.current = 0
      running.current = false
      const to = dest()
      currentH.current = to
      settledH.current = to
      el.style.height = ''
      el.style.overflow = ''
    }

    const tick = (ts: number) => {
      const dt = lastTs.current
        ? Math.min(0.048, (ts - lastTs.current) / 1000)
        : 1 / 60
      lastTs.current = ts
      const to = targetH.current
      let cur = currentH.current
      const shrinking = to < cur - 0.5
      const tau = shrinking ? BUBBLE_SHRINK_TAU : BUBBLE_GROW_TAU
      cur += (to - cur) * (1 - Math.exp(-dt / tau))
      if (Math.abs(to - cur) < BUBBLE_SETTLE_PX) {
        rest()
        return
      }
      currentH.current = cur
      el.style.height = `${cur}px`
      el.style.overflow = 'hidden'
      raf.current = requestAnimationFrame(tick)
    }

    const follow = () => {
      const to = dest()
      targetH.current = to
      if (!primed.current) {
        primed.current = true
        currentH.current = el.getBoundingClientRect().height
        settledH.current = to
        if (Math.abs(to - currentH.current) < BUBBLE_SETTLE_PX) return
      }
      if (Math.abs(to - currentH.current) < BUBBLE_SETTLE_PX) {
        if (running.current) rest()
        return
      }
      if (
        !running.current &&
        Math.abs(to - settledH.current) < BUBBLE_SETTLE_PX
      ) {
        return
      }
      if (!running.current) {
        if (to < currentH.current - BUBBLE_SETTLE_PX) {
          el.style.height = `${currentH.current}px`
          el.style.overflow = 'hidden'
        }
        running.current = true
        lastTs.current = 0
      }
      if (!raf.current) raf.current = requestAnimationFrame(tick)
    }

    followRef.current = follow
    const ro = new ResizeObserver(() => follow())
    ro.observe(grow)
    follow()

    return () => {
      followRef.current = () => {}
      ro.disconnect()
      if (raf.current) cancelAnimationFrame(raf.current)
      raf.current = 0
      running.current = false
      el.style.height = ''
      el.style.overflow = ''
    }
  }, [open])

  useLayoutEffect(() => {
    followRef.current()
  })

  return { ref, growRef }
}

export const AgentPanelMessage: React.FC<AgentPanelMessageProps> = React.memo(
  ({ message, onRetry, onAnswer, onSuggest, onWorkOffer, onZoomImage }) => {
    const { t, locale } = useI18n()
    const mode = useAgentPanelMode()
    const [copied, setCopied] = useState(false)
    const [workOfferSent, setWorkOfferSent] = useState(false)

    const copy = useCallback(() => {
      void navigator.clipboard
        ?.writeText(message.content)
        .then(() => {
          setCopied(true)
          setTimeout(setCopied, 1600, false)
        })
        .catch(() => {
          /* clipboard blocked */
        })
    }, [message.content])

    const [saved, setSaved] = useState(false)

    const save = useCallback(() => {
      setSaved(true)
      void agentService
        .addToFavorites(message.content)
        .then(() => {
          invalidateComposerFavorites()
        })
        .catch(() => {
          setSaved(false)
        })
    }, [message.content])

    const isAssistant = message.role === 'assistant'
    const question = message.question
    const hasAnswer = messageHasAnswer(message)
    const showsFooter = message.state !== 'streaming' && hasAnswer
    const showsThinking = messageShowsThinking({
      role: message.role,
      hasAnswer,
      streaming: message.state === 'streaming',
      hasProcess: !!(message.steps?.length || message.thought),
      hideThinking: mode === 'chat',
    })
    const keepThinking = useHeldOpen(showsThinking, THINKING_FOLD_MS)
    const showsBody =
      message.role === 'user'
        ? !!(message.content || message.attachments?.length)
        : hasAnswer || showsThinking || keepThinking
    const { ref: bubbleRef, growRef } = useBubbleHeight(showsBody)

    return (
      <div
        className="agent-panel-message"
        data-role={message.role}
        data-state={message.state ?? 'settled'}
      >
        {showsBody ? (
          <div
            ref={bubbleRef}
            className={
              message.role === 'system'
                ? 'agent-panel-message-body'
                : 'agent-panel-message-body glass'
            }
          >
            <div className="agent-panel-message-grow" ref={growRef}>
              {keepThinking ? (
                <div
                  className="agent-panel-thinking-slot"
                  data-open={showsThinking ? 'true' : 'false'}
                >
                  <div className="agent-panel-thinking-slot-body">
                    <AgentPanelThinking
                      steps={message.steps ?? []}
                      thought={message.thought}
                      live={message.state === 'streaming'}
                    />
                  </div>
                </div>
              ) : null}
              {hasAnswer ? (
                <div className="agent-panel-message-answer">
                  {message.role === 'user' ? (
                    <>
                      {message.attachments && message.attachments.length > 0 ? (
                        <div className="agent-panel-message-attach">
                          {message.attachments.map((item) => {
                            const preview = item.previewUrl
                            return preview ? (
                              <button
                                key={item.id}
                                type="button"
                                className="agent-panel-image-open"
                                onClick={() => onZoomImage(preview)}
                                aria-label={item.name}
                              >
                                <img src={preview} alt={item.name} />
                              </button>
                            ) : (
                              <span key={item.id} className="agent-panel-tag">
                                <span className="agent-panel-tag-text">
                                  {item.name}
                                </span>
                              </span>
                            )
                          })}
                        </div>
                      ) : null}
                      {message.content ? (
                        <p className="agent-md-p">{message.content}</p>
                      ) : null}
                    </>
                  ) : message.content ? (
                    <AgentMarkdown text={message.content} />
                  ) : null}

                  {message.imageUrls && message.imageUrls.length > 0 && (
                    <div className="agent-panel-message-images">
                      {message.imageUrls.map((url) => (
                        <button
                          key={url}
                          type="button"
                          className="agent-panel-image-open"
                          onClick={() => onZoomImage(url)}
                          aria-label={t.agentPanel.zoomImage}
                        >
                          <img src={url} alt="" loading="lazy" />
                        </button>
                      ))}
                    </div>
                  )}

                  {question ? (
                    <div className="agent-panel-question">
                      <p className="agent-panel-question-text">
                        {question.text}
                      </p>
                      {question.context ? (
                        <p className="agent-panel-question-context">
                          {question.context}
                        </p>
                      ) : null}
                      {question.options && question.options.length > 0 ? (
                        <div className="agent-panel-question-options">
                          {question.options.map((option) => (
                            <button
                              key={option.value}
                              type="button"
                              className={
                                question.answered
                                  ? 'agent-panel-tag'
                                  : 'agent-panel-tag agent-panel-tag-strong'
                              }
                              data-active={
                                question.answered === option.value
                                  ? 'true'
                                  : 'false'
                              }
                              disabled={!!question.answered}
                              title={option.description}
                              onClick={() => onAnswer(message.id, option.value)}
                            >
                              <span className="agent-panel-tag-text">
                                {option.label}
                              </span>
                            </button>
                          ))}
                        </div>
                      ) : null}
                    </div>
                  ) : null}

                  {message.workOffer && onWorkOffer ? (
                    <div className="agent-panel-work-offer">
                      <div className="agent-panel-work-offer-copy">
                        <span>{t.agentPanel.workOffer.kicker}</span>
                        <small>{t.agentPanel.workOffer.hint}</small>
                      </div>
                      <button
                        type="button"
                        className="agent-panel-work-offer-accept"
                        disabled={workOfferSent}
                        onClick={() => {
                          if (workOfferSent) return
                          setWorkOfferSent(true)
                          onWorkOffer(message.workOffer!.input)
                        }}
                      >
                        {workOfferSent
                          ? t.agentPanel.workOffer.sent
                          : t.agentPanel.workOffer.accept}
                      </button>
                    </div>
                  ) : null}

                  {message.suggestions && message.suggestions.length > 0 ? (
                    <div className="agent-panel-question-options">
                      {message.suggestions.map((suggestion) => (
                        <button
                          key={suggestion}
                          type="button"
                          className="agent-panel-tag"
                          onClick={() => onSuggest(suggestion)}
                        >
                          <span className="agent-panel-tag-text">
                            {suggestion}
                          </span>
                        </button>
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>
          </div>
        ) : null}
        {showsFooter ? (
          <div className="agent-panel-message-footer">
            {message.at && (
              <span className="agent-panel-message-time">
                {formatTime(message.at, locale)}
              </span>
            )}
            <button
              type="button"
              className="agent-panel-tag"
              onClick={copy}
              title={copied ? t.agentPanel.copied : t.agentPanel.copy}
              aria-label={copied ? t.agentPanel.copied : t.agentPanel.copy}
            >
              <span className="agent-panel-tag-text">
                {copied ? t.agentPanel.copied : t.agentPanel.copy}
              </span>
            </button>
            {!isAssistant && (
              <button
                type="button"
                className="agent-panel-tag"
                onClick={save}
                disabled={saved}
              >
                <span className="agent-panel-tag-text">
                  {saved ? t.agentPanel.saved : t.agentPanel.save}
                </span>
              </button>
            )}
            {message.state === 'error' && onRetry ? (
              <button
                type="button"
                className="agent-panel-tag"
                onClick={onRetry}
              >
                <span className="agent-panel-tag-text">
                  {t.agentPanel.retry}
                </span>
              </button>
            ) : null}
          </div>
        ) : null}
      </div>
    )
  },
  (prev, next) => prev.message === next.message,
)
