import type { PieceView } from './agentPieces'
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { pieceChange, placePieces, readListed } from './agentPieces'

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
})

test('a run ending tells the piece how it ended', () => {
  assert.deepEqual(
    pieceChange({ type: 'task_completed', success: false, response: { message: '没订上' } } as never),
    { state: 'failed', question: undefined, result: '没订上' },
  )
  assert.equal(pieceChange({ type: 'thinking' } as never), null)
})
