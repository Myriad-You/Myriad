import type { AgentMessageQuestion } from './agentMessages'
import type { BlogNoteChange } from './confirmationDetail'
import React from 'react'
import { useI18n } from '../../contexts/I18nContext'

function ChangeSide({ label, text, tone }: { label: string; text: string; tone: 'before' | 'after' }) {
  const { t } = useI18n()
  return (
    <div className="agent-panel-change-side" data-tone={tone}>
      <span className="agent-panel-change-label">{label}</span>
      <p className="agent-panel-change-text">{text || t.agentPanel.confirm.empty}</p>
    </div>
  )
}

function ChangeItem({ change }: { change: BlogNoteChange }) {
  const { t } = useI18n()
  const copy = t.agentPanel.confirm
  return (
    <li className="agent-panel-change">
      {change.field === 'body' ? null : (
        <span className="agent-panel-change-field">{copy[change.field]}</span>
      )}
      {change.before ? <ChangeSide label={copy.before} text={change.before} tone="before" /> : null}
      <ChangeSide label={copy.after} text={change.after} tone="after" />
    </li>
  )
}

/**
 * A question from the Agent. A confirmation with structured detail reads as what it does
 * (the capability by its local name, a note save as its changes); the raw arguments stay
 * one click away instead of being the card.
 */
export const AgentPanelQuestion: React.FC<{
  question: AgentMessageQuestion
  onAnswer: (value: string) => void
}> = ({ question, onAnswer }) => {
  const { t, format } = useI18n()
  const copy = t.agentPanel.confirm
  const detail = question.detail
  const caps = t.agentCaps as Record<string, string | undefined>
  const capability = detail?.capabilityId ? caps[detail.capabilityId] : undefined
  const note = detail?.blogNote
  const title = note
    ? format(copy.saveNote, { title: note.title.trim() || copy.untitled })
    : capability
      ? format(copy.run, { name: capability })
      : question.text
  const confirming = !!detail && question.options?.every((option) => option.value === 'yes' || option.value === 'no')
  const label = (value: string, fallback: string) =>
    confirming ? (value === 'yes' ? copy.yes : copy.no) : fallback
  const hidden = note ? note.total - note.changes.length : 0

  return (
    <div className="agent-panel-question">
      <p className="agent-panel-question-text">{title}</p>
      {note ? (
        note.changes.length > 0 ? (
          <ol className="agent-panel-changes">
            {note.changes.map((change, index) => (
              // Changes have no identity beyond their place in this one preview.
              <ChangeItem key={index} change={change} />
            ))}
          </ol>
        ) : (
          <p className="agent-panel-question-context">{copy.noChange}</p>
        )
      ) : null}
      {hidden > 0 ? (
        <p className="agent-panel-question-context">{format(copy.more, { count: hidden })}</p>
      ) : null}
      {question.context ? (
        detail ? (
          note ? null : (
            <details className="agent-panel-question-raw">
              <summary>{copy.details}</summary>
              <pre>{question.context}</pre>
            </details>
          )
        ) : (
          <p className="agent-panel-question-context">{question.context}</p>
        )
      ) : null}
      {question.options && question.options.length > 0 ? (
        <div className="agent-panel-question-options">
          {question.options.map((option) => (
            <button
              key={option.value}
              type="button"
              className={question.answered ? 'agent-panel-tag' : 'agent-panel-tag agent-panel-tag-strong'}
              data-active={question.answered === option.value ? 'true' : 'false'}
              disabled={!!question.answered}
              title={option.description}
              onClick={() => onAnswer(option.value)}
            >
              <span className="agent-panel-tag-text">{label(option.value, option.label)}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  )
}
