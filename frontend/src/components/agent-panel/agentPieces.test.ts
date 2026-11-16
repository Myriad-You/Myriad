import type { PieceView } from './agentPieces'
import type { ChatMessage } from './engineTypes'
import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  pieceChange,
  placePieces,
  readListed,
  setPieceLineSink,
  showPieces,
  toFollow,
  withToldLines,
} from './agentPieces'

function piece(number: number, startedAt: number): PieceView {
  return {
    number,
    asked: `piece ${number}`,
    startedAt,
    sessionId: `s${number}`,
    state: 'doing',
  }
}

test('each piece shows after her line that handed it off', () => {
  // Seconds for pieces, milliseconds for messages, as each side keeps them.
  const messages = [
    { id: 'ask-1', at: 100_000 },
    { id: 'her-1', at: 101_000 },
    { id: 'ask-2', at: 200_000 },
    { id: 'her-2', at: 201_000 },
  ]
  const placed = placePieces([piece(2, 203), piece(1, 103), piece(3, 205)], messages)
  assert.deepEqual(placed.get('her-1')?.map((one) => one.number), [1])
  assert.deepEqual(placed.get('her-2')?.map((one) => one.number), [2, 3])
  assert.equal(placed.get(''), undefined)
})

test('a piece older than every shown message goes first', () => {
  const placed = placePieces([piece(1, 10)], [{ id: 'later', at: 500_000 }])
  assert.deepEqual(placed.get('')?.map((one) => one.number), [1])
})

test('a waiting piece reads its question, a confirmation kept as one', () => {
  const read = readListed({
    number: 2,
    asked: '订周五的餐厅',
    startedAt: 1_700_000_000,
    sessionId: 'work-2',
    runId: 'run-2',
    state: 'waiting',
    detail: {
      taskId: 'task-2',
      question: {
        question_id: 'q-1',
        question_type: 'confirmation',
        question: '要订 19:00 吗？',
        options: [{ value: 'yes', label: '订' }],
      },
    },
  })
  assert.equal(read?.state, 'waiting')
  assert.equal(read?.question?.taskId, 'task-2')
  assert.equal(read?.question?.questionType, 'confirmation')
  assert.equal(read?.question?.options?.[0]?.label, '订')
  assert.equal(readListed({ number: 0 }), null)
  assert.equal(readListed({ number: 3, state: 'failed', detail: { result: 'x' } })?.state, 'failed')
})

test('a run ending tells the piece how it ended', () => {
  assert.deepEqual(
    pieceChange({ type: 'task_completed', success: false, response: { message: '没订上' } } as never),
    { state: 'failed', question: undefined, result: '没订上' },
  )
  assert.equal(pieceChange({ type: 'thinking' } as never), null)
})

function said(id: string, at: number): ChatMessage {
  return { id, sessionId: 'c', role: 'assistant', content: id, createdAt: new Date(at) }
}

test('her line about how a piece ended goes where it ended, once', () => {
  const shown = [said('ask', 1_000), said('her', 2_000), said('later', 9_000)]
  const line = said('loaded_7', 5_000)
  const next = withToldLines(shown, [line])
  assert.deepEqual(next.map((one) => one.id), ['ask', 'her', 'loaded_7', 'later'])
  // Read back with the transcript, or told again: not twice.
  assert.equal(withToldLines(next, [line]), next)
  // Ended after everything shown: last.
  assert.equal(withToldLines(shown, [said('loaded_8', 10_000)]).at(-1)?.id, 'loaded_8')
})

test('a line older than every message shown is not put in', () => {
  const shown = [said('her', 2_000)]
  assert.equal(withToldLines(shown, [said('loaded_1', 1_000)]), shown)
  // Nothing shown yet: it is all there is.
  assert.deepEqual(withToldLines([], [said('loaded_1', 1_000)]).map((one) => one.id), ['loaded_1'])
})

test('pieces listed with her lines hand the lines to the conversation', () => {
  const heard: Array<{ chat: string; ids: number[]; content: string[] }> = []
  setPieceLineSink((chat, lines) =>
    heard.push({ chat, ids: lines.map((line) => line.id), content: lines.map((line) => line.content) }),
  )
  try {
    showPieces(
      'chat-1',
      [{ number: 1, asked: '查天气', startedAt: 1, state: 'done', detail: { result: '晴' } }],
      [
        { id: 7, role: 'assistant', content: '晴', createdAt: '2026-11-01T10:00:00Z' },
        { id: 'not a line' },
      ],
    )
    assert.deepEqual(heard, [{ chat: 'chat-1', ids: [7], content: ['晴'] }])
    // No lines: nothing handed over.
    showPieces('chat-1', [])
    assert.equal(heard.length, 1)
  } finally {
    setPieceLineSink(null)
  }
})

test('only a run going on is followed, and once to its end', () => {
  const going = { ...piece(1, 1), runId: 'run-1' }
  assert.equal(toFollow(going, new Set()), true)
  // Parked on a question: its run has ended.
  assert.equal(toFollow({ ...going, state: 'waiting' }, new Set()), false)
  // Listed again after its run ended: not followed again.
  assert.equal(toFollow(going, new Set(['run-1'])), false)
  assert.equal(toFollow(piece(2, 1), new Set()), false)
})
