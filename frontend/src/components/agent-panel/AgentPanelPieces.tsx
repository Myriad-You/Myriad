import type { PieceView } from './agentPieces'
import React, { useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { AgentMarkdown } from './AgentMarkdown'
import { getAgentMessageSnapshot } from './agentMessages'
import { AgentPanelQuestion } from './AgentPanelQuestion'
import { AgentPanelThinking } from './AgentPanelThinking'
import {
  answerPiece,
  loadPieces,
  placePieces,
  stopPiece,
  usePieces,
} from './agentPieces'

function FreeAnswer({ number }: { number: number }) {
  const { t } = useI18n()
  const words = t.agentPanel.pieces
  const [value, setValue] = useState('')
  return (
    <form
      className="agent-panel-piece-answer"
      onSubmit={(event) => {
        event.preventDefault()
        const answer = value.trim()
        if (!answer) return
        setValue('')
        void answerPiece(number, answer)
      }}
    >
      <input
        value={value}
        onChange={(event) => setValue(event.target.value)}
        placeholder={words.answerPlaceholder}
        aria-label={words.answerPlaceholder}
      />
      <button type="submit" className="agent-panel-tag agent-panel-tag-strong">
        <span className="agent-panel-tag-text">{words.answer}</span>
      </button>
    </form>
  )
}

function startTime(startedAt: number, locale?: string): string {
  return new Date(startedAt * 1000).toLocaleTimeString(locale, {
    hour: '2-digit',
    minute: '2-digit',
  })
}

function Piece({ piece }: { piece: PieceView }) {
  const { t, format, locale } = useI18n()
  const words = t.agentPanel.pieces
  const question = piece.question
  const state =
    piece.state === 'waiting' && question?.questionType === 'confirmation'
      ? words.confirming
      : words[piece.state]
  const unfinished = piece.state === 'doing' || piece.state === 'waiting'
  return (
    <li
      className="agent-panel-piece glass"
      data-state={piece.state}
      aria-label={format(words.label, { number: piece.number, asked: piece.asked })}
    >
      <div className="agent-panel-piece-head">
        <span className="agent-panel-piece-kicker">{words.kicker}</span>
        <span className="agent-panel-piece-number">#{piece.number}</span>
        <span className="agent-panel-piece-state" role="status">
          {state}
        </span>
        <time
          className="agent-panel-piece-time"
          dateTime={new Date(piece.startedAt * 1000).toISOString()}
        >
          {startTime(piece.startedAt, locale)}
        </time>
        {unfinished ? (
          <button
            type="button"
            className="agent-panel-tag"
            onClick={() => void stopPiece(piece.number)}
          >
            <span className="agent-panel-tag-text">{words.stop}</span>
          </button>
        ) : null}
      </div>
      <p className="agent-panel-piece-asked">{piece.asked}</p>
      {piece.plan && piece.plan.length > 0 && unfinished ? (
        <AgentPanelThinking plan={piece.plan} steps={[]} live={piece.state === 'doing'} />
      ) : null}
      {piece.state === 'waiting' && question ? (
        question.options && question.options.length > 0 ? (
          <AgentPanelQuestion
            question={question}
            onAnswer={(value) => void answerPiece(piece.number, value)}
          />
        ) : (
          <>
            <AgentPanelQuestion question={question} onAnswer={() => {}} />
            {question.answered ? null : <FreeAnswer number={piece.number} />}
          </>
        )
      ) : null}
      {(piece.state === 'done' || piece.state === 'failed') && piece.result ? (
        <div className="agent-panel-piece-result">
          <AgentMarkdown text={piece.result} />
        </div>
      ) : null}
    </li>
  )
}

/** Pieces shown together at one place in the conversation. */
export const AgentPanelPieceList: React.FC<{ pieces?: readonly PieceView[] }> = ({
  pieces,
}) => {
  const { t } = useI18n()
  if (!pieces || pieces.length === 0) return null
  return (
    <ol className="agent-panel-pieces" aria-label={t.agentPanel.pieces.title}>
      {pieces.map((piece) => (
        <Piece key={piece.number} piece={piece} />
      ))}
    </ol>
  )
}

/**
 * The pieces she handed off in conversation `chat`, each placed after her
 * line that handed it off (see `placePieces`), keyed by that message's id.
 */
export function usePlacedPieces(
  chat: string | null,
  messageIds: readonly string[],
): Map<string, PieceView[]> {
  const { chat: shown, pieces } = usePieces()
  useEffect(() => {
    void loadPieces(chat)
  }, [chat])
  return useMemo(() => {
    if (!chat || shown !== chat || pieces.length === 0) return new Map()
    const messages = messageIds.map((id) => ({
      id,
      at: getAgentMessageSnapshot(id)?.at,
    }))
    return placePieces(pieces, messages)
  }, [chat, shown, pieces, messageIds])
}
