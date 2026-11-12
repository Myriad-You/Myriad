import type {
  CardAnswerReply,
  CardLine,
  CardNow,
} from '../../services/agent/types'
import React, { useCallback, useEffect, useId, useRef, useState } from 'react'

/** The action that opens what a card is about; on the web there is nothing to open. */
const OPEN = 'open'

export interface AgentPanelCardCopy {
  moved: string
  chose: string
  answered: string
  failed: string
  closed: string
  stale: string
  because: string
  view: string
  states: Record<string, string>
}

export interface AgentPanelCardProps {
  line: CardLine
  copy: AgentPanelCardCopy
  format: (template: string, params: Record<string, string | number>) => string
  answer: (cardId: string, version: string, action: string) => Promise<CardAnswerReply>
}

type Note = 'stale' | 'failed' | 'closed'

function motionAllowed(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return !window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

/** The line holding a card whole, of those on the page now. */
function holderOf(cardId: string): HTMLElement | null {
  if (typeof document === 'undefined') return null
  for (const element of document.querySelectorAll<HTMLElement>('[data-card-holder]')) {
    if (element.dataset.cardHolder === cardId) return element
  }
  return null
}

/** An earlier line of a card: one line pointing to where it stands now. */
function CardPointer({ cardId, copy }: { cardId: string; copy: AgentPanelCardCopy }) {
  return (
    <button
      type="button"
      className="agent-panel-tag agent-panel-card-pointer"
      onClick={() => {
        const holder = holderOf(cardId)
        if (!holder) return
        holder.scrollIntoView({
          block: 'center',
          behavior: motionAllowed() ? 'smooth' : 'auto',
        })
        holder.focus({ preventScroll: true })
      }}
    >
      <span className="agent-panel-tag-text">{copy.moved}</span>
    </button>
  )
}

/**
 * A card as the owner sees it on the web: every kind in the same layout
 * (title, state, numbered facts, her view resting on them, the answers).
 */
export const AgentPanelCard: React.FC<AgentPanelCardProps> = ({
  line,
  copy,
  format,
  answer,
}) => {
  const [now, setNow] = useState<CardNow | undefined>(line.card)
  const [sending, setSending] = useState<string | null>(null)
  const [note, setNote] = useState<Note | null>(null)
  const [lit, setLit] = useState<number | null>(null)
  const alive = useRef(true)
  const domId = useId()
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
    }
  }, [])
  useEffect(() => {
    setNow(line.card)
    setNote(null)
  }, [line.card])

  const choose = useCallback(
    async (action: string) => {
      if (!now || sending) return
      setSending(action)
      setNote(null)
      try {
        const reply = await answer(line.cardId, now.version, action)
        if (!alive.current) return
        if (reply.card) {
          setNow(reply.card)
        } else if (reply.code !== 'card_stale') {
          setNow({
            ...now,
            waiting: false,
            answered: { at: Date.now(), action, version: now.version },
          })
        }
        if (reply.code === 'card_stale') setNote('stale')
      } catch (error) {
        if (!alive.current) return
        const code = (error as { code?: unknown } | null)?.code
        setNote(code === 'card_closed' ? 'closed' : 'failed')
      } finally {
        if (alive.current) setSending(null)
      }
    },
    [answer, line.cardId, now, sending],
  )

  if (!now) return <CardPointer cardId={line.cardId} copy={copy} />

  const card = now.card
  const facts = card.facts ?? []
  const because = (card.view?.because ?? []).filter(
    (n, index, all) =>
      Number.isInteger(n) && n >= 1 && n <= facts.length && all.indexOf(n) === index,
  )
  const cited = new Set(because)
  const actions = (card.actions ?? []).filter((action) => action.id !== OPEN)
  const state = card.state ? copy.states[card.state] : undefined
  const chosen =
    !now.waiting && now.answered
      ? card.actions?.find((action) => action.id === now.answered?.action)?.label
      : undefined
  const titleId = `${domId}-title`
  const factId = (n: number) => `${domId}-fact-${n}`

  return (
    <div
      className="agent-panel-card"
      role="group"
      aria-labelledby={titleId}
      tabIndex={-1}
      data-card-holder={line.cardId}
      data-kind={card.kind}
      data-scene={card.scene}
      data-waiting={now.waiting ? 'true' : undefined}
    >
      <div className="agent-panel-card-head">
        <p className="agent-panel-card-title" id={titleId}>
          {card.title}
        </p>
        {state ? (
          <span className="agent-panel-card-state" data-state={card.state}>
            {state}
          </span>
        ) : null}
      </div>
      {facts.length > 0 ? (
        <ol className="agent-panel-card-facts">
          {facts.map((fact, index) => {
            const n = index + 1
            return (
              <li
                key={n}
                id={factId(n)}
                className="agent-panel-card-fact"
                data-cited={cited.has(n) ? 'true' : undefined}
                data-lit={lit === n ? 'true' : undefined}
              >
                <span className="agent-panel-card-fact-n" aria-hidden="true">
                  {n}
                </span>
                <span className="agent-panel-card-fact-text">{fact.text}</span>
              </li>
            )
          })}
        </ol>
      ) : null}
      {card.view?.text ? (
        <div className="agent-panel-card-view">
          <span className="agent-panel-card-view-label">{copy.view}</span>
          <p className="agent-panel-card-view-text">
            {card.view.text}
            {because.map((n) => (
              <button
                key={n}
                type="button"
                className="agent-panel-card-cite"
                aria-label={format(copy.because, { n })}
                aria-controls={factId(n)}
                title={facts[n - 1]?.text}
                onMouseEnter={() => setLit(n)}
                onMouseLeave={() => setLit(null)}
                onFocus={() => setLit(n)}
                onBlur={() => setLit(null)}
                onClick={() => {
                  setLit(n)
                  document
                    .getElementById(factId(n))
                    ?.scrollIntoView({ block: 'nearest' })
                }}
              >
                {n}
              </button>
            ))}
          </p>
        </div>
      ) : null}
      {now.waiting && actions.length > 0 ? (
        <div className="agent-panel-question-options agent-panel-card-actions">
          {actions.map((action) => (
            <button
              key={action.id}
              type="button"
              className="agent-panel-tag"
              disabled={sending !== null}
              data-active={sending === action.id ? 'true' : undefined}
              onClick={() => void choose(action.id)}
            >
              <span className="agent-panel-tag-text">{action.label}</span>
            </button>
          ))}
        </div>
      ) : null}
      {!now.waiting && now.answered ? (
        <p className="agent-panel-card-chosen">
          {chosen ? format(copy.chose, { label: chosen }) : copy.answered}
        </p>
      ) : null}
      {note ? (
        <p
          className="agent-panel-card-note"
          data-tone={note === 'stale' ? 'info' : 'error'}
          role={note === 'stale' ? 'status' : 'alert'}
        >
          {copy[note]}
        </p>
      ) : null}
    </div>
  )
}
